use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use tokio::sync::OwnedMutexGuard;
use tokio_util::sync::CancellationToken;

use super::script::Work;
use super::{Ctx, Engine, OperationKind, Outcome};
use crate::driver::{BoxFuture, ContextKind, DbError, ErrorKind, ExecOptions, RowSink, StreamOptions, TrackedState};
use crate::explain::{ExplainRequest, Plan};
use crate::history::{Provenance, Ran};
use crate::params::NamedValue;
use crate::value::{Column, Execution, Value};

const SAVEPOINT: &str = "zen_explain";

const MOST_ROWS: usize = 100_000;

#[derive(Clone, Debug)]
pub struct ExplainRun {
    pub request: ExplainRequest,
    pub judged: String,
    pub rollback: bool,
    pub schema: Option<String>,
    pub read_only: bool,
    pub timeout: Option<Duration>,
    pub provenance: Provenance,
    pub params: Vec<NamedValue>,
}

struct Collect {
    columns: Vec<Column>,
    rows: Vec<Vec<Value>>,
    token: CancellationToken,
}

impl RowSink for Collect {
    fn columns(&mut self, columns: Vec<Column>) {
        self.columns = columns;
    }

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> BoxFuture<'_, usize> {
        Box::pin(async move { if self.token.is_cancelled() || self.rows.len() >= MOST_ROWS { 0 } else { 1000 } })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        self.rows.extend(rows);
        self.rows.len() < MOST_ROWS && !self.token.is_cancelled()
    }

    fn cap(&self) -> usize {
        MOST_ROWS
    }
}

impl Engine {
    pub fn explain(&self, id: &str, context: ContextKind, run: ExplainRun) -> Result<u64, DbError> {
        if !self.state().connections.contains_key(id) {
            return Err(DbError::new(ErrorKind::NotConnected, ""));
        }
        for open in self.shared.results.open_in(id, &context) {
            open.close();
        }
        let (op, token) = self.begin(OperationKind::Explain, Some(id), Some(context.clone()));
        let (engine, id) = (self.clone(), id.to_owned());
        self.spawn(op, async move {
            let (kind, cell) = engine.connected_cell(&id, &context, &token).await?;
            engine.busy(&id, &kind, 1);
            let result = async {
                let mut ctx: OwnedMutexGuard<Ctx> = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                let result = engine.explain_in(&id, &kind, &mut ctx, op, &run, &token).await;
                ctx.last_used = Instant::now();
                engine.update_info(&id, &kind, &ctx);
                result
            }
            .await;
            engine.busy(&id, &kind, -1);
            result.map(|plan| Outcome::Plan(Box::new(plan)))
        });
        Ok(op)
    }

    pub fn take_plan(&self, op: u64) -> Option<Plan> {
        self.state().plans.remove(&op)
    }

    async fn settle_session(
        &self,
        id: &str,
        kind: &ContextKind,
        ctx: &mut Ctx,
        op: u64,
        (schema, read_only): (Option<&String>, bool),
    ) -> Result<(), DbError> {
        let wanted = TrackedState {
            schema: schema.cloned().or_else(|| ctx.tracked.schema.clone()),
            read_only,
            ..ctx.tracked.clone()
        };
        if wanted != ctx.tracked {
            self.ready(id, kind, ctx, op).await?;
            let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
            session.apply(&wanted).await?;
            ctx.tracked = wanted;
        }
        Ok(())
    }

    async fn plain(&self, ctx: &mut Ctx, statement: &str) -> Result<(), DbError> {
        let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        session.execute(statement, &ExecOptions { max_rows: 0, timeout: None }).await.map(|_| ())
    }

    async fn explain_in(
        &self,
        id: &str,
        kind: &ContextKind,
        ctx: &mut Ctx,
        op: u64,
        run: &ExplainRun,
        token: &CancellationToken,
    ) -> Result<Plan, DbError> {
        let driver = self.state().connections.get(id).map(|conn| Arc::clone(&conn.driver));
        let driver = driver.ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        let read_only = run.read_only || *kind == ContextKind::Metadata || self.read_only(id);
        self.settle_session(id, kind, ctx, op, (run.schema.as_ref(), read_only)).await?;
        let judged = run.read_only || (ctx.tracked.read_only && !driver.guards_read_only());
        let (started, clock, schema) = (SystemTime::now(), Instant::now(), ctx.tracked.schema.clone());
        let record = |error: Option<DbError>, duration: Duration| Ran {
            connection: id.to_owned(),
            context: kind.clone(),
            schema: schema.clone(),
            text: run.request.text.clone(),
            started,
            duration,
            rows: None,
            affected: None,
            error,
            provenance: run.provenance.clone(),
            params: run.params.clone(),
        };
        if judged && !driver.assess(&run.judged).allowed_read_only() {
            let verb = crate::sql::first_keyword(&run.judged).unwrap_or_default();
            let error = DbError::new(ErrorKind::Refused, verb).with_code("readOnly");
            self.record(record(Some(error.clone()), Duration::ZERO));
            return Err(error);
        }
        self.ready(id, kind, ctx, op).await?;
        let savepoint = if run.rollback {
            let open = ctx.session.as_ref().is_some_and(|session| session.in_transaction());
            let begin = if open { format!("SAVEPOINT {SAVEPOINT}") } else { "START TRANSACTION".to_owned() };
            self.plain(ctx, &begin).await?;
            Some(open)
        } else {
            None
        };
        let result = if run.params.is_empty() {
            let options = ExecOptions { max_rows: MOST_ROWS, timeout: run.timeout };
            self.cancellable(ctx, &run.request.text, &options, (token, !run.rollback)).await
        } else {
            let values: Vec<Value> = run.params.iter().map(|bound| bound.value.clone()).collect();
            let mut sink = Collect { columns: Vec::new(), rows: Vec::new(), token: token.clone() };
            let options = StreamOptions::default();
            let work = Work::Bound(&values, &options, &mut sink);
            let streamed = self.stream_guarded(ctx, &run.request.text, work, run.timeout, token).await;
            streamed.map(|_| Execution { columns: sink.columns, rows: sink.rows, ..Execution::default() })
        };
        if let Some(open) = savepoint
            && ctx.session.is_some()
        {
            let undone = if open {
                let back = self.plain(ctx, &format!("ROLLBACK TO SAVEPOINT {SAVEPOINT}")).await;
                match back {
                    Ok(()) => self.plain(ctx, &format!("RELEASE SAVEPOINT {SAVEPOINT}")).await,
                    failed => failed,
                }
            } else {
                match ctx.session.as_mut() {
                    Some(session) => session.rollback().await,
                    None => Ok(()),
                }
            };
            if undone.is_err() {
                ctx.session = None;
            }
        }
        self.record(record(result.as_ref().err().cloned(), clock.elapsed()));
        let execution = result?;
        let request = run.request.clone();
        let analyzed = request.executes;
        tokio::task::spawn_blocking(move || crate::explain::parse(request.format, &execution, analyzed))
            .await
            .map_err(|error| DbError::new(ErrorKind::Invalid, error.to_string()))?
    }
}

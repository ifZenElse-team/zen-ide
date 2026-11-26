use std::sync::Arc;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::sync::OwnedMutexGuard;
use tokio_util::sync::CancellationToken;

use super::{ConnectionState, Ctx, CtxCell, Engine, Event, OperationKind, OperationStatus, Outcome};
use crate::driver::{Canceller, ContextKind, DbError, ErrorKind, ExecOptions, StreamEnd, StreamOptions, TrackedState};
use crate::history::{Provenance, Ran};
use crate::model::DataModel;
use crate::results::{Origin, ResultSet};
use crate::value::Value;

const KEPT_QUERIES: usize = 100;

pub(super) enum Work<'a> {
    Rows(&'a StreamOptions, &'a mut dyn crate::driver::RowSink),
    Bound(&'a [Value], &'a StreamOptions, &'a mut dyn crate::driver::RowSink),
    Copy(&'a crate::export::CopyOptions, &'a mut dyn crate::driver::ByteSink),
}

/// One statement of a query, with where it is in the caller's text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryStatement {
    /// The statement's text as it is sent.
    pub text: String,
    /// Where it starts in the caller's text (a character offset of a console), for marking errors.
    pub start: usize,
    /// Where it ends there.
    pub end: usize,
    /// Whether it is a command of the client (a psql meta-command, `DELIMITER`, `@set`), which is not sent.
    #[serde(default)]
    pub client: bool,
    /// The values bound to its placeholders (`$1` or `?` in its text), in the driver's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<crate::params::NamedValue>,
    /// The text kept in the query history and shown in the query's state when it differs from what
    /// is sent: placeholders as written instead of values written as literals (drivers without bind
    /// parameters), and secret-looking `${name}` variables masked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<String>,
}

impl QueryStatement {
    pub fn shown(&self) -> &str {
        self.kept.as_deref().unwrap_or(&self.text)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryOptions {
    pub stop_on_error: bool,
    pub rollback_on_error: bool,
    pub timeout: Option<Duration>,
    pub schema: Option<String>,
    pub provenance: Provenance,
    pub read_only: bool,
    pub notes: Vec<Message>,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            stop_on_error: true,
            rollback_on_error: true,
            timeout: None,
            schema: None,
            provenance: Provenance::default(),
            read_only: false,
            notes: Vec::new(),
        }
    }
}

/// Where a statement of a query is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StatementStatus {
    /// Not run yet.
    Pending,
    /// Running.
    Running,
    /// Ran.
    Done,
    /// Failed; `error` says why.
    Failed,
    /// Cancelled, or not run because the script stopped.
    Cancelled,
    /// Not sent: a command of the client.
    Skipped,
}

/// A statement of a query as views and agents read it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementState {
    /// Its index in the query.
    pub index: usize,
    /// Where it starts in the caller's text.
    pub start: usize,
    /// Where it ends there.
    pub end: usize,
    /// Its first line, cut to 80 characters.
    pub summary: String,
    /// Where it is.
    pub status: StatementStatus,
    /// Milliseconds until the server answered.
    pub execution_ms: u64,
    /// Milliseconds spent reading its rows after that.
    pub fetch_ms: u64,
    /// The rows it changed, when the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// The rows read into its result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    /// Its result, when it returned rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<u64>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
}

/// A message of a query's output: what the server said (`NOTICE`, `RAISE`, `PRINT`) or what the IDE did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// The statement it belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement: Option<usize>,
    /// Its severity as the server names it (`NOTICE`, `WARNING`), or `INFO` for the IDE's own.
    pub severity: String,
    /// The text.
    pub text: String,
    /// The server's code, when it sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// A query as views and agents read it.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryState {
    /// Its id, the operation's.
    pub id: u64,
    /// The connection.
    pub connection: String,
    /// The context it runs in.
    pub context: ContextKind,
    /// Where it is: running until its last statement ends; a result's portal may stay open after.
    pub status: OperationStatus,
    /// Milliseconds it ran, or has run so far.
    pub elapsed_ms: u64,
    /// Its statements.
    pub statements: Vec<StatementState>,
    /// Its output.
    pub messages: Vec<Message>,
    /// Rows changed by all its statements.
    pub updated: u64,
    /// Why it failed or was cancelled as a whole (before any statement ran).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
    #[serde(skip)]
    pub(crate) started: Option<Instant>,
}

fn summary(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let mut shown: String = line.chars().take(80).collect();
    if shown.len() < line.len() || text.trim().lines().count() > 1 {
        shown.push('…');
    }
    shown
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl Engine {
    pub fn results(&self) -> &Arc<crate::results::ResultStore> {
        &self.shared.results
    }

    pub fn query(&self, id: u64) -> Option<QueryState> {
        let mut query = self.state().queries.get(&id)?.clone();
        if query.status != OperationStatus::Running {
            return Some(query);
        }
        if let Some(started) = query.started {
            query.elapsed_ms = millis(started.elapsed());
        }
        for statement in &mut query.statements {
            let Some(id) = statement.result else { continue };
            let Some(result) = self.shared.results.get(id) else {
                statement.result = None;
                continue;
            };
            let info = result.info();
            let rows = !result.columns().is_empty();
            if rows {
                statement.rows = Some(info.rows);
            } else {
                statement.result = None;
            }
            if statement.status == StatementStatus::Running {
                statement.execution_ms = info.execution_ms;
                statement.fetch_ms = info.fetch_ms;
                if info.state == crate::results::ReadState::Open {
                    statement.status = StatementStatus::Done;
                }
            }
        }
        if query
            .statements
            .iter()
            .all(|statement| !matches!(statement.status, StatementStatus::Pending | StatementStatus::Running))
        {
            let has = |status: StatementStatus| query.statements.iter().any(|statement| statement.status == status);
            query.status = if has(StatementStatus::Cancelled) {
                OperationStatus::Cancelled
            } else if has(StatementStatus::Failed) {
                OperationStatus::Failed
            } else {
                OperationStatus::Done
            };
        }
        Some(query)
    }

    pub fn queries(&self) -> Vec<u64> {
        self.state().queries.keys().copied().collect()
    }

    fn with_query(&self, id: u64, change: impl FnOnce(&mut QueryState)) {
        {
            let mut state = self.state();
            if let Some(query) = state.queries.get_mut(&id) {
                change(query);
            }
        }
        self.wake();
    }

    pub(super) async fn connected_cell(
        &self,
        id: &str,
        context: &ContextKind,
        token: &CancellationToken,
    ) -> Result<(ContextKind, CtxCell), DbError> {
        loop {
            let connecting =
                self.state().connections.get(id).is_some_and(|conn| conn.state == ConnectionState::Connecting);
            if !connecting {
                return self.cell(id, context);
            }
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(20)) => {}
                () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
            }
        }
    }

    pub fn run_query(
        &self,
        id: &str,
        context: ContextKind,
        statements: Vec<QueryStatement>,
        options: QueryOptions,
    ) -> Result<u64, DbError> {
        if statements.is_empty() {
            return Err(DbError::new(ErrorKind::Invalid, "no statement"));
        }
        for open in self.shared.results.open_in(id, &context) {
            open.close();
        }
        let (op, token) = self.begin(OperationKind::Query, Some(id), Some(context.clone()));
        {
            let mut state = self.state();
            let states = statements
                .iter()
                .enumerate()
                .map(|(index, statement)| StatementState {
                    index,
                    start: statement.start,
                    end: statement.end,
                    summary: summary(statement.shown()),
                    status: StatementStatus::Pending,
                    execution_ms: 0,
                    fetch_ms: 0,
                    affected: None,
                    rows: None,
                    result: None,
                    error: None,
                })
                .collect();
            state.queries.insert(
                op,
                QueryState {
                    id: op,
                    connection: id.to_owned(),
                    context: context.clone(),
                    status: OperationStatus::Running,
                    elapsed_ms: 0,
                    statements: states,
                    messages: options.notes.clone(),
                    updated: 0,
                    error: None,
                    started: Some(Instant::now()),
                },
            );
            while state.queries.len() > KEPT_QUERIES {
                let Some(oldest) =
                    state.queries.iter().find(|(_, query)| query.status != OperationStatus::Running).map(|(id, _)| *id)
                else {
                    break;
                };
                state.queries.remove(&oldest);
            }
        }
        let (engine, id) = (self.clone(), id.to_owned());
        self.spawn(op, async move {
            let result = engine.query_task(&id, &context, op, &statements, &options, &token).await;
            let status = match &result {
                Ok(()) => OperationStatus::Done,
                Err(error) if error.kind == ErrorKind::Cancelled => OperationStatus::Cancelled,
                Err(_) => OperationStatus::Failed,
            };
            engine.with_query(op, |query| {
                if query.status == OperationStatus::Running {
                    query.status = status;
                    query.elapsed_ms = query.started.map_or(0, |started| millis(started.elapsed()));
                }
                if let Err(error) = &result
                    && query.statements.iter().all(|statement| statement.status == StatementStatus::Pending)
                {
                    query.error = Some(error.clone());
                }
                for statement in &mut query.statements {
                    if matches!(statement.status, StatementStatus::Pending | StatementStatus::Running) {
                        statement.status = StatementStatus::Cancelled;
                    }
                }
            });
            result.map(|()| Outcome::Nothing)
        });
        Ok(op)
    }

    async fn query_task(
        &self,
        id: &str,
        context: &ContextKind,
        op: u64,
        statements: &[QueryStatement],
        options: &QueryOptions,
        token: &CancellationToken,
    ) -> Result<(), DbError> {
        let (kind, cell) = self.connected_cell(id, context, token).await?;
        self.busy(id, &kind, 1);
        let driver = self.state().connections.get(id).map(|conn| Arc::clone(&conn.driver));
        let mut structure = false;
        let ran = async {
            let mut ctx: OwnedMutexGuard<Ctx> = tokio::select! {
                ctx = cell.lock_owned() => ctx,
                () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
            };
            if let Some(schema) = &options.schema
                && ctx.tracked.schema.as_deref() != Some(schema.as_str())
            {
                self.ready(id, &kind, &mut ctx, op).await?;
                let tracked = TrackedState { schema: Some(schema.clone()), ..ctx.tracked.clone() };
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                session.apply(&tracked).await?;
                ctx.tracked = tracked;
            }
            let read_only = options.read_only || kind == ContextKind::Metadata || self.read_only(id);
            if ctx.tracked.read_only != read_only {
                self.ready(id, &kind, &mut ctx, op).await?;
                let tracked = TrackedState { read_only, ..ctx.tracked.clone() };
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                session.apply(&tracked).await?;
                ctx.tracked = tracked;
            }
            let last = statements.len() - 1;
            let mut failed = None;
            for (index, statement) in statements.iter().enumerate() {
                if token.is_cancelled() {
                    return Err(DbError::new(ErrorKind::Cancelled, ""));
                }
                if statement.client {
                    self.with_query(op, |query| {
                        query.statements[index].status = StatementStatus::Skipped;
                        query.messages.push(Message {
                            statement: Some(index),
                            severity: "INFO".to_owned(),
                            text: format!("client command not sent: {}", summary(&statement.text)),
                            code: Some("clientCommand".to_owned()),
                        });
                    });
                    continue;
                }
                self.with_query(op, |query| query.statements[index].status = StatementStatus::Running);
                let judged = driver
                    .as_ref()
                    .filter(|driver| options.read_only || (ctx.tracked.read_only && !driver.guards_read_only()));
                let refused = judged
                    .map(|driver| driver.assess(&statement.text))
                    .filter(|assessment| !assessment.allowed_read_only())
                    .map(|_| self.refuse_read_only(id, &kind, &ctx, statement, options));
                let outcome = match refused {
                    Some(error) => Err(error),
                    None => {
                        self.run_one(id, &kind, &mut ctx, op, (index, index == last), statement, options, token).await
                    }
                };
                if outcome.is_ok() && driver.as_ref().is_some_and(|driver| driver.changes_structure(&statement.text)) {
                    structure = true;
                }
                if let Err(error) = outcome {
                    let cancelled = error.kind == ErrorKind::Cancelled;
                    self.with_query(op, |query| {
                        let state = &mut query.statements[index];
                        state.status = if cancelled { StatementStatus::Cancelled } else { StatementStatus::Failed };
                        state.error = Some(error.clone());
                    });
                    if cancelled {
                        return Err(error);
                    }
                    if options.rollback_on_error
                        && let Some(session) = ctx.session.as_mut()
                        && session.in_transaction()
                        && session.rollback().await.is_ok()
                    {
                        self.with_query(op, |query| {
                            query.messages.push(Message {
                                statement: Some(index),
                                severity: "INFO".to_owned(),
                                text: "transaction rolled back".to_owned(),
                                code: Some("rolledBack".to_owned()),
                            });
                        });
                    }
                    failed = Some(error);
                    if options.stop_on_error {
                        break;
                    }
                }
            }
            ctx.last_used = Instant::now();
            self.update_info(id, &kind, &ctx);
            failed.map_or(Ok(()), Err)
        }
        .await;
        self.busy(id, &kind, -1);
        if structure {
            self.push_event(Event::StructureChanged { connection: id.to_owned() });
        }
        ran
    }

    fn refuse_read_only(
        &self,
        id: &str,
        kind: &ContextKind,
        ctx: &Ctx,
        statement: &QueryStatement,
        options: &QueryOptions,
    ) -> DbError {
        let error = read_only_error(&statement.text);
        self.record(Ran {
            connection: id.to_owned(),
            context: kind.clone(),
            schema: ctx.tracked.schema.clone(),
            text: statement.shown().to_owned(),
            started: std::time::SystemTime::now(),
            duration: Duration::ZERO,
            rows: None,
            affected: None,
            error: Some(error.clone()),
            provenance: options.provenance.clone(),
            params: statement.params.clone(),
        });
        error
    }

    pub fn record_refused(
        &self,
        id: &str,
        context: &ContextKind,
        statements: &[String],
        error: &DbError,
        provenance: &Provenance,
    ) {
        let schema = self.schema(id, context);
        for text in statements {
            self.record(Ran {
                connection: id.to_owned(),
                context: context.clone(),
                schema: schema.clone(),
                text: text.clone(),
                started: std::time::SystemTime::now(),
                duration: Duration::ZERO,
                rows: None,
                affected: None,
                error: Some(error.clone()),
                provenance: provenance.clone(),
                params: Vec::new(),
            });
        }
    }

    fn new_result(&self, origin: Origin) -> Arc<ResultSet> {
        let id = {
            let mut state = self.state();
            state.next_result += 1;
            state.next_result
        };
        let waker = self.shared.waker.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        let store = &self.shared.results;
        Arc::new(ResultSet::new(id, origin, store.paging(), Arc::clone(store.budget()), waker))
    }

    #[allow(clippy::too_many_arguments, reason = "one statement of a running query and where it stands")]
    async fn run_one(
        &self,
        id: &str,
        kind: &ContextKind,
        ctx: &mut Ctx,
        op: u64,
        (index, last): (usize, bool),
        statement: &QueryStatement,
        options: &QueryOptions,
        token: &CancellationToken,
    ) -> Result<(), DbError> {
        self.ready(id, kind, ctx, op).await?;
        let switched = self.smart_before(id, kind, ctx, &statement.text).await;
        if switched {
            self.with_query(op, |query| {
                query.messages.push(Message {
                    statement: Some(index),
                    severity: "INFO".to_owned(),
                    text: "switched to manual commit (smart commit)".to_owned(),
                    code: Some("smartCommit".to_owned()),
                });
            });
        }
        let origin = Origin {
            query: op,
            statement: index,
            connection: id.to_owned(),
            context: kind.clone(),
            text: statement.text.clone(),
            params: statement.params.iter().map(|bound| bound.value.clone()).collect(),
        };
        let result = self.new_result(origin);
        self.shared.results.insert(Arc::clone(&result));
        self.with_query(op, |query| query.statements[index].result = Some(result.id()));
        let mut reader = result.reader(last, token.child_token());
        let page = self.shared.results.paging().page;
        let stream = StreamOptions { page, skip: 0 };
        let (started, clock) = (std::time::SystemTime::now(), Instant::now());
        let values: Vec<Value> = statement.params.iter().map(|bound| bound.value.clone()).collect();
        let work = if values.is_empty() {
            Work::Rows(&stream, &mut reader)
        } else {
            Work::Bound(&values, &stream, &mut reader)
        };
        let ran = self.stream_guarded(ctx, &statement.text, work, options.timeout, token).await;
        let rows = (!result.columns().is_empty()).then(|| result.info().rows);
        self.record(Ran {
            connection: id.to_owned(),
            context: kind.clone(),
            schema: ctx.tracked.schema.clone(),
            text: statement.shown().to_owned(),
            started,
            duration: clock.elapsed(),
            rows,
            affected: ran.as_ref().ok().and_then(|end| end.affected),
            error: ran.as_ref().err().cloned(),
            provenance: options.provenance.clone(),
            params: statement.params.clone(),
        });
        if let Some(session) = ctx.session.as_mut() {
            let notices = session.take_notices();
            if !notices.is_empty() {
                self.with_query(op, |query| {
                    query.messages.extend(notices.into_iter().map(|notice| Message {
                        statement: Some(index),
                        severity: notice.severity,
                        text: notice.message,
                        code: notice.code,
                    }));
                });
            }
        }
        if let Err(error) = &ran
            && error.kind == ErrorKind::Connection
        {
            ctx.session = None;
        }
        let affected = ran.as_ref().ok().and_then(|end| end.affected);
        self.after_statement(id, kind, ctx, &statement.text, (switched, ran.is_err(), affected)).await;
        if switched && ran.is_err() {
            self.with_query(op, |query| {
                query.messages.push(Message {
                    statement: Some(index),
                    severity: "INFO".to_owned(),
                    text: "back to auto-commit: the statement failed (smart commit)".to_owned(),
                    code: Some("smartCommitBack".to_owned()),
                });
            });
        }
        let has_rows = !result.columns().is_empty();
        if !has_rows {
            self.with_query(op, |query| query.statements[index].result = None);
            self.shared.results.remove(result.id());
        }
        match ran {
            Ok(end) => {
                reader.finish(end.complete, false);
                let info = result.info();
                self.with_query(op, |query| {
                    query.messages.extend(end.notices.iter().map(|notice| Message {
                        statement: Some(index),
                        severity: notice.severity.clone(),
                        text: notice.message.clone(),
                        code: notice.code.clone(),
                    }));
                    query.updated += end.affected.filter(|_| !has_rows).unwrap_or(0);
                    let state = &mut query.statements[index];
                    state.status = StatementStatus::Done;
                    state.execution_ms = info.execution_ms;
                    state.fetch_ms = info.fetch_ms;
                    state.affected = end.affected;
                    if has_rows {
                        state.rows = Some(info.rows);
                        state.result = Some(info.id);
                    }
                });
                Ok(())
            }
            Err(error) => {
                reader.finish(false, true);
                let info = result.info();
                self.with_query(op, |query| {
                    let state = &mut query.statements[index];
                    state.execution_ms = info.execution_ms;
                    state.fetch_ms = info.fetch_ms;
                });
                Err(error)
            }
        }
    }

    pub(super) async fn stream_guarded(
        &self,
        ctx: &mut Ctx,
        statement: &str,
        work: Work<'_>,
        timeout: Option<Duration>,
        token: &CancellationToken,
    ) -> Result<StreamEnd, DbError> {
        let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        let canceller: Option<Arc<dyn Canceller>> = session.canceller();
        let wait = self.shared.timing.cancel_wait;
        let mut close = false;
        let result = {
            let run = match work {
                Work::Rows(options, sink) => session.stream(statement, options, sink),
                Work::Bound(params, options, sink) => session.stream_bound(statement, params, options, sink),
                Work::Copy(options, sink) => session.copy_out(statement, options, sink),
            };
            tokio::pin!(run);
            let expired = async move {
                match timeout {
                    Some(timeout) => tokio::time::sleep(timeout).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                result = &mut run => result,
                reason = async {
                    tokio::select! {
                        () = token.cancelled() => ErrorKind::Cancelled,
                        () = expired => ErrorKind::Timeout,
                    }
                } => {
                    let started = Instant::now();
                    let asked = match &canceller {
                        Some(canceller) => matches!(tokio::time::timeout(wait, canceller.cancel()).await, Ok(Ok(()))),
                        None => false,
                    };
                    let left = wait.saturating_sub(started.elapsed());
                    match tokio::time::timeout(left.max(Duration::from_millis(50)), &mut run).await {
                        Ok(Ok(end)) if reason == ErrorKind::Cancelled => Ok(end),
                        Ok(_) if asked || reason == ErrorKind::Cancelled => Err(DbError::new(reason, "")),
                        _ => {
                            close = true;
                            let mut error = DbError::new(reason, "");
                            error.code = Some(if canceller.is_none() { "noServerCancel" } else { "cancelUnconfirmed" }.to_owned());
                            Err(error)
                        }
                    }
                }
            }
        };
        if close {
            ctx.session = None;
        }
        result
    }

    pub fn fetch_more(&self, result_id: u64, rows: Option<usize>) -> Result<Option<u64>, DbError> {
        let result = self
            .shared
            .results
            .get(result_id)
            .ok_or_else(|| DbError::new(ErrorKind::Invalid, format!("result {result_id}")))?;
        let connection = result.connection().to_owned();
        let repeatable = self
            .state()
            .connections
            .get(&connection)
            .is_some_and(|conn| conn.driver.repeatable(result.statement_text()));
        if !result.want_more(rows, repeatable)? {
            return Ok(None);
        }
        let context = result.context().clone();
        let (kind, cell) = self.cell(&connection, &context)?;
        for open in self.shared.results.open_in(&connection, &kind) {
            open.close();
        }
        let (op, token) = self.begin(OperationKind::Fetch, Some(&connection), Some(kind.clone()));
        let engine = self.clone();
        self.busy(&connection, &kind, 1);
        self.spawn(op, async move {
            let ran = async {
                let mut ctx = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                engine.ready(&connection, &kind, &mut ctx, op).await?;
                let mut reader = result.reader(true, token.child_token());
                let page = engine.shared.results.paging().page;
                let options = StreamOptions { page, skip: reader.skip() };
                let text = result.statement_text().to_owned();
                let params = result.statement_params().to_vec();
                let work = if params.is_empty() {
                    Work::Rows(&options, &mut reader)
                } else {
                    Work::Bound(&params, &options, &mut reader)
                };
                let ran = engine.stream_guarded(&mut ctx, &text, work, None, &token).await;
                if let Some(session) = ctx.session.as_mut() {
                    let _ = session.take_notices();
                }
                match ran {
                    Ok(end) => reader.finish(end.complete, false),
                    Err(error) => {
                        reader.finish(false, true);
                        return Err(error);
                    }
                }
                ctx.last_used = Instant::now();
                engine.update_info(&connection, &kind, &ctx);
                Ok(())
            }
            .await;
            engine.busy(&connection, &kind, -1);
            ran.map(|()| Outcome::Nothing)
        });
        Ok(Some(op))
    }

    pub fn count_rows(&self, result_id: u64) -> Result<u64, DbError> {
        let result = self
            .shared
            .results
            .get(result_id)
            .ok_or_else(|| DbError::new(ErrorKind::Invalid, format!("result {result_id}")))?;
        let connection = result.connection().to_owned();
        let countable = self.state().connections.get(&connection).is_some_and(|conn| {
            conn.driver.capabilities().models.contains(&DataModel::Relational)
                && conn.driver.repeatable(result.statement_text())
        });
        if !countable {
            return Err(DbError::new(ErrorKind::Unsupported, "count"));
        }
        let (kind, cell) = self.cell(&connection, result.context())?;
        result.close();
        let (op, token) = self.begin(OperationKind::Count, Some(&connection), Some(kind.clone()));
        let engine = self.clone();
        let text = result.statement_text().trim().trim_end_matches(';').to_owned();
        let counting = format!("SELECT count(*) FROM (\n{text}\n) AS zen_counted");
        let params = result.statement_params().to_vec();
        self.busy(&connection, &kind, 1);
        self.spawn(op, async move {
            let ran = async {
                let mut ctx = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                engine.ready(&connection, &kind, &mut ctx, op).await?;
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                let (canceller, wait) = (session.canceller(), engine.shared.timing.cancel_wait);
                let mut sink = Collect::default();
                let options = StreamOptions { page: 1, skip: 0 };
                let counted = async {
                    if params.is_empty() {
                        session
                            .execute(&counting, &ExecOptions { max_rows: 1, timeout: None })
                            .await
                            .map(|done| done.rows)
                    } else {
                        session.stream_bound(&counting, &params, &options, &mut sink).await?;
                        Ok(std::mem::take(&mut sink.rows))
                    }
                };
                let (rows, close) = super::stop_or_close(counted, canceller, wait, &token).await;
                if close {
                    ctx.session = None;
                    engine.update_info(&connection, &kind, &ctx);
                }
                let rows = rows?;
                let Some(session) = ctx.session.as_mut() else { return Err(DbError::new(ErrorKind::Cancelled, "")) };
                let _ = session.take_notices();
                let total = match rows.first().and_then(|row| row.first()) {
                    Some(Value::Integer(total)) => u64::try_from(*total).unwrap_or(0),
                    Some(Value::Decimal(total)) => total.parse().unwrap_or(0),
                    _ => return Err(DbError::new(ErrorKind::Server, "no count")),
                };
                result.set_total(total);
                ctx.last_used = Instant::now();
                engine.update_info(&connection, &kind, &ctx);
                Ok(())
            }
            .await;
            engine.busy(&connection, &kind, -1);
            ran.map(|()| Outcome::Nothing)
        });
        Ok(op)
    }

    pub fn schema(&self, id: &str, context: &ContextKind) -> Option<String> {
        let cell = self.state().connections.get(id)?.contexts.get(context).cloned()?;
        let ctx = cell.try_lock().ok()?;
        ctx.tracked.schema.clone()
    }

    pub fn cancel_query(&self, id: u64) -> bool {
        let running = self.cancel(id);
        let mut closed = false;
        for result in self.shared.results.all() {
            if result.info().query == id && result.portal_open() {
                result.close();
                closed = true;
            }
        }
        running || closed
    }
}

#[derive(Default)]
struct Collect {
    rows: Vec<Vec<Value>>,
}

impl crate::driver::RowSink for Collect {
    fn columns(&mut self, _columns: Vec<crate::value::Column>) {}

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> crate::driver::BoxFuture<'_, usize> {
        let wanted = usize::from(self.rows.is_empty());
        Box::pin(async move { wanted })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        self.rows.extend(rows);
        self.rows.is_empty()
    }

    fn cap(&self) -> usize {
        1
    }
}

fn read_only_error(statement: &str) -> DbError {
    let verb = crate::sql::first_keyword(statement).unwrap_or_default();
    DbError::new(ErrorKind::Refused, verb).with_code("readOnly")
}

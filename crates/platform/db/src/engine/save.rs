use std::sync::Arc;
use std::time::{Instant, SystemTime};

use super::{Ctx, Engine, OperationKind, Outcome, SaveReport};
use crate::driver::{Canceller, ContextKind, DbError, ErrorKind, ExecOptions, Param};
use crate::edit::{ChangeKind, Planned};
use crate::history::{Provenance, Ran, Source};
use crate::results::SavedRows;
use crate::value::Execution;

const SAVEPOINT: &str = "zen_save";

struct Failure {
    row: usize,
    error: DbError,
    conflict: bool,
}

impl Engine {
    fn set_saved(&self, op: u64, report: SaveReport) {
        if let Some(op) = self.state().operations.get_mut(&op) {
            op.state.saved = Some(report);
        }
    }

    pub fn save_edits(&self, result_id: u64, planned: Vec<Planned>) -> Result<u64, DbError> {
        self.save_edits_from(result_id, planned, Provenance::of(Source::Save))
    }

    pub fn save_edits_from(
        &self,
        result_id: u64,
        planned: Vec<Planned>,
        provenance: Provenance,
    ) -> Result<u64, DbError> {
        let result = self
            .shared
            .results
            .get(result_id)
            .ok_or_else(|| DbError::new(ErrorKind::Invalid, format!("result {result_id}")))?;
        if planned.is_empty() {
            return Err(DbError::new(ErrorKind::Invalid, "nothing to save"));
        }
        let connection = result.connection().to_owned();
        let (bound, transactions) = self.state().connections.get(&connection).map_or((false, false), |conn| {
            let capabilities = conn.driver.capabilities();
            let sql_transactions = capabilities.transactions && !crate::edit::edits_documents(capabilities);
            (crate::edit::edits(capabilities), sql_transactions)
        });
        if !bound {
            return Err(DbError::new(ErrorKind::Unsupported, "editing results"));
        }
        if self.read_only(&connection) {
            return Err(DbError::new(ErrorKind::Refused, "UPDATE").with_code("readOnly"));
        }
        let (kind, cell) = self.cell(&connection, result.context())?;
        for open in self.shared.results.open_in(&connection, &kind) {
            open.close();
        }
        let (op, token) = self.begin(OperationKind::Save, Some(&connection), Some(kind.clone()));
        let engine = self.clone();
        self.busy(&connection, &kind, 1);
        self.spawn(op, async move {
            let ran = async {
                let mut ctx = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                engine.ready(&connection, &kind, &mut ctx, op).await?;
                let place = (connection.as_str(), &kind, &provenance, transactions);
                let outcome = engine.save_in(&mut ctx, &planned, &token, place).await;
                ctx.last_used = Instant::now();
                engine.update_info(&connection, &kind, &ctx);
                match outcome {
                    Ok((report, saved)) => {
                        engine.log_saved(&connection, &kind, &planned);
                        result.apply_saved(&saved);
                        engine.set_saved(op, report);
                        Ok(Outcome::Nothing)
                    }
                    Err(failed) => {
                        let (report, error) = *failed;
                        engine.set_saved(op, report);
                        Err(error)
                    }
                }
            }
            .await;
            engine.busy(&connection, &kind, -1);
            ran
        });
        Ok(op)
    }

    async fn bound(
        &self,
        ctx: &mut Ctx,
        statement: &str,
        params: &[Param],
        token: &tokio_util::sync::CancellationToken,
    ) -> Result<Execution, DbError> {
        let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        let canceller: Option<Arc<dyn Canceller>> = session.canceller();
        let wait = self.shared.timing.cancel_wait;
        let mut lost = false;
        let result = {
            let run = session.execute_bound(statement, params);
            tokio::pin!(run);
            tokio::select! {
                result = &mut run => result,
                () = token.cancelled() => {
                    if let Some(canceller) = &canceller {
                        let _ = tokio::time::timeout(wait, canceller.cancel()).await;
                    }
                    if tokio::time::timeout(wait, &mut run).await.is_err() {
                        lost = true;
                    }
                    Err(DbError::new(ErrorKind::Cancelled, ""))
                }
            }
        };
        if lost {
            ctx.session = None;
        }
        result
    }

    async fn control(&self, ctx: &mut Ctx, statement: &str) -> Result<(), DbError> {
        let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        session.execute(statement, &ExecOptions::default()).await.map(|_| ())
    }

    async fn save_in(
        &self,
        ctx: &mut Ctx,
        planned: &[Planned],
        token: &tokio_util::sync::CancellationToken,
        (connection, kind, provenance, transactions): (&str, &ContextKind, &Provenance, bool),
    ) -> Result<(SaveReport, SavedRows), Box<(SaveReport, DbError)>> {
        let in_transaction = ctx.session.as_ref().is_some_and(|session| session.in_transaction());
        let own = ctx.tracked.auto_commit && !in_transaction;
        let mut report = SaveReport { in_transaction: transactions && !own, ..SaveReport::default() };
        let opening = if own { "BEGIN".to_owned() } else { format!("SAVEPOINT {SAVEPOINT}") };
        if transactions && let Err(error) = self.control(ctx, &opening).await {
            return Err(Box::new((report, error)));
        }
        let mut saved = SavedRows::default();
        let mut failure = None;
        for statement in planned {
            let (started, clock) = (SystemTime::now(), Instant::now());
            let ran = self.bound(ctx, &statement.sql, &statement.params, token).await;
            self.record(Ran {
                connection: connection.to_owned(),
                context: kind.clone(),
                schema: ctx.tracked.schema.clone(),
                text: statement.sql.clone(),
                started,
                duration: clock.elapsed(),
                rows: None,
                affected: ran.as_ref().ok().map(|execution| execution.affected.unwrap_or(execution.rows.len() as u64)),
                error: ran.as_ref().err().cloned(),
                provenance: provenance.clone(),
                params: Vec::new(),
            });
            let execution = match ran {
                Ok(execution) => execution,
                Err(error) => {
                    failure = Some(Failure { row: statement.row, error, conflict: false });
                    break;
                }
            };
            report.statements += 1;
            let changed = execution.affected.unwrap_or(execution.rows.len() as u64);
            if changed == 0 {
                let error = DbError::new(ErrorKind::Server, "0 rows changed").with_code("conflict");
                failure = Some(Failure { row: statement.row, error, conflict: true });
                break;
            }
            if statement.one_row && changed > 1 {
                let error = DbError::new(ErrorKind::Server, format!("{changed} rows changed")).with_code("severalRows");
                failure = Some(Failure { row: statement.row, error, conflict: false });
                break;
            }
            let rows = match &statement.fetch {
                Some(fetch) => match self.bound(ctx, &fetch.sql, &fetch.params, token).await {
                    Ok(fetched) => fetched.rows,
                    Err(error) => {
                        failure = Some(Failure { row: statement.row, error, conflict: false });
                        break;
                    }
                },
                None => execution.rows,
            };
            let returned: Vec<(usize, crate::value::Value)> = rows
                .into_iter()
                .next()
                .map(|row| statement.returning.iter().copied().zip(row).collect())
                .unwrap_or_default();
            match statement.kind {
                ChangeKind::Delete => report.deleted += changed,
                ChangeKind::Insert => {
                    report.inserted += changed;
                    saved.inserted.push(returned);
                }
                ChangeKind::Update => {
                    report.updated += changed;
                    saved.updated.push((statement.row, returned));
                }
            }
        }
        match failure {
            None if !transactions => Ok((report, saved)),
            None => {
                let closing = if own { "COMMIT".to_owned() } else { format!("RELEASE SAVEPOINT {SAVEPOINT}") };
                match self.control(ctx, &closing).await {
                    Ok(()) => Ok((report, saved)),
                    Err(error) => Err(Box::new((report, error))),
                }
            }
            Some(failure) => {
                if !transactions {
                    report.kept = report.statements > 0;
                } else if ctx.session.is_some() {
                    if own {
                        let _ = self.control(ctx, "ROLLBACK").await;
                    } else {
                        let _ = self.control(ctx, &format!("ROLLBACK TO SAVEPOINT {SAVEPOINT}")).await;
                        let _ = self.control(ctx, &format!("RELEASE SAVEPOINT {SAVEPOINT}")).await;
                    }
                }
                report.failed_row = Some(failure.row);
                report.conflict = failure.conflict;
                Err(Box::new((report, failure.error)))
            }
        }
    }
}

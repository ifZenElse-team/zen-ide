use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::{
    Conn, ConnectRequest, Ctx, CtxInfo, Engine, Event, OperationKind, Outcome, State, TransactionPolicy, runtime,
};
use crate::driver::{ContextKind, DbError, Driver, ErrorKind, TrackedState};
use crate::history::{Provenance, Ran, Source};
use crate::model::StatementKind;
use crate::registry::POOLER;
use crate::sql::Dialect;

const KEPT_STATEMENTS: usize = 1000;

/// How a context commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CommitMode {
    /// Every statement commits on its own.
    Auto,
    /// Statements wait for Commit or Rollback.
    Manual,
    /// Auto-commit until the first statement that may change data, then manual.
    Smart,
}

/// A change of a context's commit mode or isolation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ModeChange {
    /// The new commit mode; unchanged when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<CommitMode>,
    /// The new isolation level by the driver's name for it (`null` inside: the server's default); unchanged when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<Option<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ModeSetting {
    pub(super) mode: Option<CommitMode>,
    pub(super) isolation: Option<Option<String>>,
}

#[derive(Clone, Debug)]
pub(super) struct Logged {
    summary: String,
    modifying: bool,
    failed: bool,
    affected: Option<u64>,
    at: Instant,
}

/// A statement of an open transaction, as the transaction log shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoggedStatement {
    /// Its first line, cut to 80 characters.
    pub summary: String,
    /// Whether it may have changed data.
    pub modifying: bool,
    /// Whether it failed.
    pub failed: bool,
    /// The rows it changed, when the server said.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// Seconds since it ran.
    pub seconds_ago: u64,
}

/// The transaction state of one context, never with a secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransactionState {
    /// The connection's id.
    pub connection: String,
    /// The connection's name.
    pub name: String,
    /// The context.
    pub context: ContextKind,
    /// Its commit mode.
    pub mode: CommitMode,
    /// Whether statements commit on their own right now (smart commit turns this off after a change).
    pub auto_commit: bool,
    /// The isolation level set, by the driver's name for it; the server's default when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    /// Whether a transaction is open.
    pub in_transaction: bool,
    /// Statements run in the open transaction.
    pub statements: usize,
    /// Of those, statements that may have changed data.
    pub modifying: usize,
    /// Seconds since the transaction's first statement.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_seconds: Option<u64>,
    /// Seconds since the context was last used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u64>,
    /// Seconds until an idle transaction is rolled back, while that is announced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ending_in_seconds: Option<u64>,
    /// The transaction's statements, the most recent last, at most the `limit` asked for.
    pub log: Vec<LoggedStatement>,
    /// Whether older statements were left out of `log`.
    pub truncated: bool,
}

impl CtxInfo {
    pub(super) fn end_transaction(&mut self) {
        self.in_transaction = false;
        self.log.clear();
        self.logged = 0;
        self.modifying = 0;
        self.began = None;
        if let Some((_, token)) = self.ending.take() {
            token.cancel();
        }
    }
}

fn summary(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    let mut shown: String = line.chars().take(80).collect();
    if shown.len() < line.len() || text.trim().lines().count() > 1 {
        shown.push('…');
    }
    shown
}

fn seconds(since: Instant) -> u64 {
    since.elapsed().as_secs()
}

impl ConnectRequest {
    fn default_mode(&self, context: &ContextKind) -> CommitMode {
        if *context == ContextKind::Metadata || self.source.flag(POOLER) {
            return CommitMode::Auto;
        }
        let auto = self.source.overrides.auto_commit.unwrap_or(self.kind.auto_commit);
        match (auto, self.kind.smart_commit) {
            (false, _) => CommitMode::Manual,
            (true, true) => CommitMode::Smart,
            (true, false) => CommitMode::Auto,
        }
    }

    fn mode_with(&self, context: &ContextKind, modes: &HashMap<(String, ContextKind), ModeSetting>) -> CommitMode {
        if *context == ContextKind::Metadata || self.source.flag(POOLER) {
            return CommitMode::Auto;
        }
        modes
            .get(&(self.source.id.clone(), context.clone()))
            .and_then(|setting| setting.mode)
            .unwrap_or_else(|| self.default_mode(context))
    }
}

fn smart_switches(driver: &dyn Driver, statement: &str) -> bool {
    match Dialect::of_language(&driver.capabilities().language) {
        dialect if dialect.commands() => driver.classify(statement) == StatementKind::Write,
        _ => crate::sql::smart_commit_switches(statement),
    }
}

fn controls(driver: &dyn Driver, statement: &str) -> bool {
    match Dialect::of_language(&driver.capabilities().language) {
        Dialect::Script => false,
        Dialect::Lines => {
            let verb = statement.split_whitespace().next().unwrap_or_default().to_ascii_uppercase();
            matches!(verb.as_str(), "MULTI" | "EXEC" | "DISCARD")
        }
        _ => crate::sql::controls_transaction(statement),
    }
}

fn shared_kind(conn: &Conn, context: &ContextKind) -> ContextKind {
    if conn.driver.capabilities().shared_connection { ContextKind::Main } else { context.clone() }
}

impl Engine {
    fn state_of(
        state: &State,
        request: &ConnectRequest,
        context: &ContextKind,
        info: Option<&CtxInfo>,
        limit: usize,
    ) -> TransactionState {
        let modes = &state.modes;
        let tracked = request.tracked_with(context, modes);
        let mode = request.mode_with(context, modes);
        let smart_manual = info.is_some_and(|info| info.smart_manual);
        let in_transaction = info.is_some_and(|info| info.in_transaction);
        let log: Vec<LoggedStatement> = info
            .map(|info| {
                let skip = info.log.len().saturating_sub(limit);
                info.log
                    .iter()
                    .skip(skip)
                    .map(|logged| LoggedStatement {
                        summary: logged.summary.clone(),
                        modifying: logged.modifying,
                        failed: logged.failed,
                        affected: logged.affected,
                        seconds_ago: seconds(logged.at),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let logged = info.map_or(0, |info| info.logged);
        TransactionState {
            connection: request.source.id.clone(),
            name: request.source.name.clone(),
            context: context.clone(),
            mode,
            auto_commit: tracked.auto_commit && !smart_manual,
            isolation: tracked.isolation,
            in_transaction,
            statements: logged,
            modifying: info.map_or(0, |info| info.modifying),
            age_seconds: info.and_then(|info| info.began).map(seconds),
            idle_seconds: info.and_then(|info| info.last_used).map(seconds),
            ending_in_seconds: info
                .and_then(|info| info.ending.as_ref())
                .map(|(at, _)| at.saturating_duration_since(Instant::now()).as_secs()),
            truncated: logged > log.len(),
            log,
        }
    }

    pub fn transaction(&self, request: &ConnectRequest, context: &ContextKind, limit: usize) -> TransactionState {
        let state = self.state();
        match state.connections.get(&request.source.id) {
            Some(conn) => {
                let kind = shared_kind(conn, context);
                Self::state_of(&state, &conn.request, &kind, conn.info.get(&kind), limit)
            }
            None => Self::state_of(&state, request, context, None, limit),
        }
    }

    pub fn pending(&self) -> Vec<TransactionState> {
        let state = self.state();
        let mut pending: Vec<TransactionState> = state
            .connections
            .values()
            .flat_map(|conn| {
                conn.info
                    .iter()
                    .filter(|(_, info)| info.in_transaction)
                    .map(|(kind, info)| Self::state_of(&state, &conn.request, kind, Some(info), 0))
                    .collect::<Vec<_>>()
            })
            .collect();
        pending.sort_by(|a, b| (&a.name, &a.connection, &a.context).cmp(&(&b.name, &b.connection, &b.context)));
        pending
    }

    pub fn changes_pending(&self, id: &str) -> Vec<ContextKind> {
        let state = self.state();
        state
            .connections
            .get(id)
            .map(|conn| {
                conn.info
                    .iter()
                    .filter(|(_, info)| info.in_transaction && info.modifying > 0)
                    .map(|(kind, _)| kind.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn end_transaction(&self, id: &str, context: ContextKind, commit: bool) -> Result<u64, DbError> {
        self.end_transaction_from(id, context, commit, Provenance::of(Source::Transaction))
    }

    pub fn end_transaction_from(
        &self,
        id: &str,
        context: ContextKind,
        commit: bool,
        provenance: Provenance,
    ) -> Result<u64, DbError> {
        let (kind, cell) = self.cell(id, &context)?;
        for open in self.shared.results.open_in(id, &kind) {
            open.close();
        }
        let operation = if commit { OperationKind::Commit } else { OperationKind::Rollback };
        let (op, token) = self.begin(operation, Some(id), Some(kind.clone()));
        let (engine, id) = (self.clone(), id.to_owned());
        self.busy(&id, &kind, 1);
        self.spawn(op, async move {
            let result = async {
                let mut ctx = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                let schema = ctx.tracked.schema.clone();
                let mut ended = Ok(());
                if let Some(session) = ctx.session.as_mut()
                    && session.in_transaction()
                {
                    let (started, clock) = (std::time::SystemTime::now(), Instant::now());
                    ended = if commit { session.commit().await } else { session.rollback().await };
                    engine.record(Ran {
                        connection: id.clone(),
                        context: kind.clone(),
                        schema,
                        text: if commit { "COMMIT" } else { "ROLLBACK" }.to_owned(),
                        started,
                        duration: clock.elapsed(),
                        rows: None,
                        affected: None,
                        error: ended.as_ref().err().cloned(),
                        provenance,
                        params: Vec::new(),
                    });
                }
                ctx.last_used = Instant::now();
                engine.update_info(&id, &kind, &ctx);
                engine.return_to_auto(&id, &kind, &mut ctx).await;
                ended.map(|()| Outcome::Nothing)
            }
            .await;
            engine.busy(&id, &kind, -1);
            result
        });
        Ok(op)
    }

    async fn return_to_auto(&self, id: &str, kind: &ContextKind, ctx: &mut Ctx) {
        let back = {
            let state = self.state();
            state.connections.get(id).is_some_and(|conn| {
                conn.request.kind.auto_close_transactions && conn.info.get(kind).is_some_and(|info| info.smart_manual)
            })
        };
        if !back {
            return;
        }
        let tracked = TrackedState { auto_commit: true, ..ctx.tracked.clone() };
        let Some(session) = ctx.session.as_mut().filter(|session| !session.in_transaction()) else { return };
        if session.apply(&tracked).await.is_ok() {
            ctx.tracked = tracked;
            self.set_smart_manual(id, kind, false);
        }
    }

    pub(super) fn set_smart_manual(&self, id: &str, kind: &ContextKind, on: bool) {
        if let Some(info) = self.state().connections.get_mut(id).and_then(|conn| conn.info.get_mut(kind)) {
            info.smart_manual = on;
        }
    }

    pub fn set_mode(
        &self,
        id: &str,
        context: ContextKind,
        change: ModeChange,
        policy: TransactionPolicy,
    ) -> Result<Option<u64>, DbError> {
        if context == ContextKind::Metadata {
            return Err(DbError::new(ErrorKind::Invalid, "the metadata context always commits on its own"));
        }
        let leaves_manual = change.mode.is_some_and(|mode| mode != CommitMode::Manual);
        let cell = {
            let mut guard = self.state();
            let State { connections, modes, .. } = &mut *guard;
            let conn = connections.get(id);
            let kind = conn.map_or_else(|| context.clone(), |conn| shared_kind(conn, &context));
            if let (Some(conn), Some(Some(isolation))) = (conn, &change.isolation)
                && !conn
                    .driver
                    .capabilities()
                    .isolation_levels
                    .iter()
                    .any(|level| level.eq_ignore_ascii_case(isolation))
            {
                return Err(DbError::new(ErrorKind::Invalid, isolation));
            }
            let info = conn.and_then(|conn| conn.info.get(&kind));
            if leaves_manual
                && policy == TransactionPolicy::Ask
                && info.is_some_and(|info| info.in_transaction && info.modifying > 0)
            {
                return Err(DbError::new(ErrorKind::OpenTransactions, kind.to_string()));
            }
            let setting = modes.entry((id.to_owned(), kind.clone())).or_default();
            if let Some(mode) = change.mode {
                setting.mode = Some(mode);
            }
            if let Some(isolation) = change.isolation.clone() {
                setting.isolation = Some(isolation);
            }
            conn.and_then(|conn| conn.contexts.get(&kind).map(|cell| (kind.clone(), Arc::clone(cell))))
        };
        let Some((kind, cell)) = cell else { return Ok(None) };
        let keeps_mode = change.mode.is_none();
        let (op, _) = self.begin(OperationKind::Mode, Some(id), Some(kind.clone()));
        let (engine, id) = (self.clone(), id.to_owned());
        self.busy(&id, &kind, 1);
        self.spawn(op, async move {
            let result = async {
                let mut ctx = cell.lock_owned().await;
                let tracked = {
                    let state = engine.state();
                    let conn = state.connections.get(&id).ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                    let mut tracked = TrackedState {
                        schema: ctx.tracked.schema.clone(),
                        database: ctx.tracked.database.clone(),
                        ..conn.request.tracked_with(&kind, &state.modes)
                    };
                    if keeps_mode {
                        tracked.auto_commit = ctx.tracked.auto_commit;
                    }
                    tracked
                };
                if let Some(session) = ctx.session.as_mut() {
                    if !keeps_mode && session.in_transaction() && tracked.auto_commit {
                        match policy {
                            TransactionPolicy::Commit => session.commit().await?,
                            TransactionPolicy::Rollback | TransactionPolicy::Ask => session.rollback().await?,
                        }
                    }
                    session.apply(&tracked).await?;
                }
                ctx.tracked = tracked;
                if !keeps_mode {
                    engine.set_smart_manual(&id, &kind, false);
                }
                engine.update_info(&id, &kind, &ctx);
                Ok(Outcome::Nothing)
            }
            .await;
            engine.busy(&id, &kind, -1);
            result
        });
        Ok(Some(op))
    }

    pub fn postpone(&self, id: &str, context: &ContextKind) -> bool {
        let mut state = self.state();
        let Some(info) = state.connections.get_mut(id).and_then(|conn| conn.info.get_mut(context)) else {
            return false;
        };
        info.last_used = Some(Instant::now());
        match info.ending.take() {
            Some((_, token)) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    pub fn close_context(
        &self,
        id: &str,
        context: ContextKind,
        policy: TransactionPolicy,
    ) -> Result<Option<u64>, DbError> {
        if matches!(context, ContextKind::Main | ContextKind::Metadata) {
            return Err(DbError::new(ErrorKind::Invalid, context.to_string()));
        }
        let cell = {
            let mut state = self.state();
            let Some(conn) = state.connections.get_mut(id) else { return Ok(None) };
            if conn.driver.capabilities().shared_connection {
                return Ok(None);
            }
            let pending = conn.info.get(&context).is_some_and(|info| info.in_transaction && info.modifying > 0);
            if pending && policy == TransactionPolicy::Ask {
                return Err(DbError::new(ErrorKind::OpenTransactions, context.to_string()));
            }
            conn.info.remove(&context);
            conn.contexts.remove(&context)
        };
        let Some(cell) = cell else { return Ok(None) };
        self.cancel_connection(id, Some(&context));
        let (op, _) = self.begin(OperationKind::Close, Some(id), Some(context.clone()));
        let engine = self.clone();
        self.spawn(op, async move {
            let result = super::close_cells(vec![(context, cell)], policy).await;
            engine.wake();
            result.map(|()| Outcome::Nothing)
        });
        Ok(Some(op))
    }

    pub(super) async fn smart_before(&self, id: &str, kind: &ContextKind, ctx: &mut Ctx, statement: &str) -> bool {
        let driver = {
            let state = self.state();
            let Some(conn) = state.connections.get(id) else { return false };
            if conn.request.mode_with(kind, &state.modes) != CommitMode::Smart {
                return false;
            }
            Arc::clone(&conn.driver)
        };
        let Some(session) = ctx.session.as_mut() else { return false };
        if !ctx.tracked.auto_commit || session.in_transaction() || !smart_switches(driver.as_ref(), statement) {
            return false;
        }
        let tracked = TrackedState { auto_commit: false, ..ctx.tracked.clone() };
        if session.apply(&tracked).await.is_err() {
            return false;
        }
        ctx.tracked = tracked;
        self.set_smart_manual(id, kind, true);
        true
    }

    pub(super) async fn after_statement(
        &self,
        id: &str,
        kind: &ContextKind,
        ctx: &mut Ctx,
        statement: &str,
        (switched, failed, affected): (bool, bool, Option<u64>),
    ) {
        if switched && failed {
            if let Some(session) = ctx.session.as_mut() {
                if session.in_transaction() {
                    let _ = session.rollback().await;
                }
                let tracked = TrackedState { auto_commit: true, ..ctx.tracked.clone() };
                if session.apply(&tracked).await.is_ok() {
                    ctx.tracked = tracked;
                }
            } else {
                ctx.tracked.auto_commit = true;
            }
            self.set_smart_manual(id, kind, false);
            self.update_info(id, kind, ctx);
            return;
        }
        if let Some(database) = ctx.session.as_ref().and_then(|session| session.selected_database()) {
            ctx.tracked.database = Some(database);
        }
        let in_transaction = ctx.session.as_ref().is_some_and(|session| session.in_transaction());
        {
            let mut state = self.state();
            if let Some(conn) = state.connections.get_mut(id) {
                let driver = Arc::clone(&conn.driver);
                if let Some(info) = conn.info.get_mut(kind) {
                    if in_transaction && !controls(driver.as_ref(), statement) {
                        let modifying = driver.classify(statement) == StatementKind::Write;
                        info.began.get_or_insert_with(Instant::now);
                        info.logged += 1;
                        if modifying {
                            info.modifying += 1;
                        }
                        info.log.push(Logged {
                            summary: summary(statement),
                            modifying,
                            failed,
                            affected,
                            at: Instant::now(),
                        });
                        if info.log.len() > KEPT_STATEMENTS {
                            info.log.remove(0);
                        }
                    } else if in_transaction {
                        info.began.get_or_insert_with(Instant::now);
                    }
                }
            }
        }
        self.update_info(id, kind, ctx);
        if !in_transaction {
            self.return_to_auto(id, kind, ctx).await;
        }
    }

    pub(super) fn log_saved(&self, id: &str, kind: &ContextKind, planned: &[crate::edit::Planned]) {
        let mut state = self.state();
        let Some(info) = state.connections.get_mut(id).and_then(|conn| conn.info.get_mut(kind)) else { return };
        if !info.in_transaction {
            return;
        }
        info.began.get_or_insert_with(Instant::now);
        for statement in planned {
            info.logged += 1;
            info.modifying += 1;
            info.log.push(Logged {
                summary: summary(&statement.sql),
                modifying: true,
                failed: false,
                affected: None,
                at: Instant::now(),
            });
        }
        let over = info.log.len().saturating_sub(KEPT_STATEMENTS);
        info.log.drain(..over);
    }

    pub(super) fn announce_idle(&self, id: &str, name: &str, kind: &ContextKind) {
        let countdown = self.shared.timing.idle_countdown;
        let token = CancellationToken::new();
        {
            let mut state = self.state();
            let Some(info) = state.connections.get_mut(id).and_then(|conn| conn.info.get_mut(kind)) else { return };
            if info.ending.is_some() {
                return;
            }
            info.ending = Some((Instant::now() + countdown, token.clone()));
        }
        self.push_event(Event::TransactionIdle {
            connection: id.to_owned(),
            name: name.to_owned(),
            context: kind.clone(),
            seconds: countdown.as_secs(),
        });
        let (engine, id, name, kind) = (self.clone(), id.to_owned(), name.to_owned(), kind.clone());
        runtime().spawn(async move {
            tokio::select! {
                () = tokio::time::sleep(countdown) => engine.end_idle(&id, &name, &kind, &token).await,
                () = token.cancelled() => {}
            }
        });
    }

    async fn end_idle(&self, id: &str, name: &str, kind: &ContextKind, token: &CancellationToken) {
        let cell = {
            let state = self.state();
            let Some(conn) = state.connections.get(id) else { return };
            let announced = conn.info.get(kind).is_some_and(|info| info.ending.is_some());
            if !announced || token.is_cancelled() {
                return;
            }
            conn.contexts.get(kind).cloned()
        };
        let Some(cell) = cell else { return };
        let mut ctx = tokio::select! {
            ctx = cell.lock_owned() => ctx,
            () = token.cancelled() => return,
            () = tokio::time::sleep(Duration::from_secs(5)) => return,
        };
        if token.is_cancelled() {
            return;
        }
        let ended = match ctx.session.as_mut() {
            Some(session) if session.in_transaction() => session.rollback().await.is_ok(),
            _ => false,
        };
        self.update_info(id, kind, &ctx);
        self.return_to_auto(id, kind, &mut ctx).await;
        if ended {
            self.push_event(Event::TransactionEnded {
                connection: id.to_owned(),
                name: name.to_owned(),
                context: kind.clone(),
            });
        }
    }
}

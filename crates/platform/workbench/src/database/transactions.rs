use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{CommitMode, ConnectRequest, Engine, ModeChange, TransactionPolicy, TransactionState};
use zen_db::registry::Entry;
use zen_i18n::t;

use super::connections::describe;
use super::consoles::{connection_of, context_of, engine};
use super::sources::{Database, connection_types, find};
use crate::workspaces::{InnerTab, Workspaces};

const LOGGED: usize = 50;
const LISTED: usize = 50;

pub const OPEN_TRANSACTIONS: &str = "openTransactions";

/// Which context's transaction: a console, or a connection and context.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TargetParams {
    /// A console or `.sql` file: its connection and its own context; when nothing is given, the active console, or the main context of the connection of the Database workspace's tab in front (a table's).
    pub path: Option<PathBuf>,
    /// A connection's id, instead of a console.
    pub connection: Option<String>,
    /// The context of `connection` (`{"kind": "main"}` when absent, `{"kind": "console", "name": "…"}`).
    pub context: Option<ContextKind>,
}

struct Target {
    entry: Entry,
    context: ContextKind,
}

fn target(session: &mut Session, params: &TargetParams) -> Result<Target, CommandError> {
    if let Some(id) = &params.connection {
        let entry = find(session, id)?;
        return Ok(Target { entry, context: params.context.clone().unwrap_or(ContextKind::Main) });
    }
    if params.path.is_none()
        && feature_tab_in_front(session)
        && let Some(id) = super::tab_connection(session)
    {
        let entry = find(session, &id)?;
        return Ok(Target { entry, context: ContextKind::Main });
    }
    let path = super::consoles::console_path(session, params.path.clone())?;
    let id = connection_of(session, &path)
        .ok_or_else(|| CommandError::with_code("noConnection", t!("database-error-no-console-connection")))?;
    let entry = find(session, &id)?;
    Ok(Target { entry, context: context_of(&path) })
}

fn feature_tab_in_front(session: &Session) -> bool {
    matches!(crate::workspaces::in_front(session), Some((info, Some(tab))) if info.id == super::WORKSPACE && tab.path.is_none())
}

fn bare_request(session: &Session, entry: &Entry) -> ConnectRequest {
    let (types, _) = connection_types(session);
    ConnectRequest {
        source: entry.source.clone(),
        kind: types.resolve(&entry.source.connection_type),
        password: None,
        stored: None,
        ssh_secret: None,
    }
}

pub fn state_of(session: &mut Session, entry: &Entry, context: &ContextKind, limit: usize) -> TransactionState {
    let request = bare_request(session, entry);
    let engine = session.state_mut::<Database>().engine();
    engine.transaction(&request, context, limit)
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::NotConnected => "notConnected",
        ErrorKind::Invalid => "invalid",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

pub fn context_name(context: &ContextKind) -> String {
    match context {
        ContextKind::Console(name) => name.rsplit('/').next().unwrap_or(name).to_owned(),
        other => other.to_string(),
    }
}

pub fn open_question(
    engine: &Engine,
    error: &DbError,
    connection: Option<&str>,
    retry: (&str, serde_json::Value),
) -> CommandError {
    let pending: Vec<TransactionState> = engine
        .pending()
        .into_iter()
        .filter(|state| state.modifying > 0 && connection.is_none_or(|id| state.connection == id))
        .collect();
    let message = match pending.as_slice() {
        [one] => t!(
            "database-question-pending-one",
            name = one.name.as_str(),
            context = context_name(&one.context),
            count = one.modifying
        ),
        _ => t!("database-error-open-transactions", contexts = error.detail.as_str()),
    };
    CommandError::with_code(OPEN_TRANSACTIONS, message).with_data(serde_json::json!({
        "error": error,
        "pending": pending,
        "retry": { "command": retry.0, "params": retry.1 },
    }))
}

fn refuse_or_fail(
    engine: &Engine,
    error: &DbError,
    connection: Option<&str>,
    retry: (&str, serde_json::Value),
) -> CommandError {
    if error.kind == ErrorKind::OpenTransactions {
        open_question(engine, error, connection, retry)
    } else {
        failed(error)
    }
}

/// A commit or rollback started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Ended {
    /// The operation's id, for `db.operation.state`.
    pub operation: u64,
    /// The connection.
    pub connection: String,
    /// The context.
    pub context: ContextKind,
}

fn end(session: &mut Session, params: &TargetParams, commit: bool) -> Result<Ended, CommandError> {
    let target = target(session, params)?;
    if commit {
        super::safety::agent_changes(session, &target.entry)?;
    }
    let engine = engine(session);
    let id = target.entry.source.id;
    let provenance = super::history::provenance(session, zen_db::history::Source::Transaction, None);
    let operation =
        engine.end_transaction_from(&id, target.context.clone(), commit, provenance).map_err(|error| failed(&error))?;
    Ok(Ended { operation, connection: id, context: target.context })
}

pub struct Commit;

impl Command for Commit {
    type Params = TargetParams;
    type Output = Ended;
    const ID: &'static str = "db.transaction.commit";
    const TITLE: &'static str = "Commit";
    const DESCRIPTION: &'static str = "Commits the open transaction of a console (`path`, the active one when nothing is given) or of `connection` and `context` (main when absent), in the background (⌃4 in the Database workspace; with no console in front, the main context of the connection of the tab in front); waits for a statement running there. With smart commit and the connection type's \"return to auto-commit\", the console goes back to auto-commit. Answers the operation id (db.operation.state). Refused with `notConnected` when the connection is not connected.";

    fn run(&self, session: &mut Session, params: TargetParams) -> Result<Ended, CommandError> {
        end(session, &params, true)
    }
}

pub struct Rollback;

impl Command for Rollback {
    type Params = TargetParams;
    type Output = Ended;
    const ID: &'static str = "db.transaction.rollback";
    const TITLE: &'static str = "Rollback";
    const DESCRIPTION: &'static str = "Rolls back the open transaction of a console (`path`, the active one when nothing is given) or of `connection` and `context` (main when absent), in the background (⌃8 in the Database workspace; with no console in front, the main context of the connection of the tab in front). With smart commit and the connection type's \"return to auto-commit\", the console goes back to auto-commit. Answers the operation id (db.operation.state).";

    fn run(&self, session: &mut Session, params: TargetParams) -> Result<Ended, CommandError> {
        end(session, &params, false)
    }
}

/// A commit mode or isolation to set.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ModeParams {
    /// Which context.
    #[serde(flatten)]
    pub target: TargetParams,
    /// `auto`, `manual` or `smart`; unchanged when absent.
    pub mode: Option<CommitMode>,
    /// The isolation level by the driver's name (`read committed`, `repeatable read`, `serializable` for PostgreSQL); an empty text for the server's default; unchanged when absent.
    pub isolation: Option<String>,
    /// Changes pending when leaving manual commit: `ask` (default) refuses with code `openTransactions`, `commit` or `rollback` ends them first.
    pub transactions: TransactionPolicy,
}

pub struct Mode;

impl Command for Mode {
    type Params = ModeParams;
    type Output = TransactionState;
    const ID: &'static str = "db.transaction.mode";
    const TITLE: &'static str = "Transaction Mode";
    const DESCRIPTION: &'static str = "Sets the commit mode of a console (`path`, the active one) or of `connection` and `context`: `auto` (every statement commits), `manual` (Commit or Rollback ends the transaction) or `smart` (auto-commit until the first statement that does not start with SELECT, SHOW, USE, SET or EXPLAIN, then manual; back to auto when that statement fails), and its `isolation` (empty: the server's default). Kept for the console across reconnects; its connection type gives the default (Production: manual). Leaving manual with changes pending is refused with code `openTransactions` unless `transactions` is `commit` or `rollback`. Connects nowhere. Answers the transaction state.";

    fn run(&self, session: &mut Session, params: ModeParams) -> Result<TransactionState, CommandError> {
        let target = target(session, &params.target)?;
        let id = target.entry.source.id.clone();
        let change = ModeChange {
            mode: params.mode,
            isolation: params.isolation.map(|level| Some(level.trim().to_owned()).filter(|level| !level.is_empty())),
        };
        let engine = engine(session);
        let mut retry = serde_json::json!({ "connection": id, "context": target.context });
        if let Some(mode) = params.mode {
            retry["mode"] = serde_json::json!(mode);
        }
        engine
            .set_mode(&id, target.context.clone(), change, params.transactions)
            .map_err(|error| refuse_or_fail(&engine, &error, Some(&id), (Self::ID, retry)))?;
        Ok(state_of(session, &target.entry, &target.context, LOGGED))
    }
}

/// Which transaction to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusParams {
    /// Which context.
    #[serde(flatten)]
    pub target: TargetParams,
    /// The most statements of the log to answer, the most recent; 50 when absent.
    pub limit: Option<usize>,
}

pub struct Status;

impl Command for Status {
    type Params = StatusParams;
    type Output = TransactionState;
    const ID: &'static str = "db.transaction.status";
    const TITLE: &'static str = "Transaction Log";
    const DESCRIPTION: &'static str = "Answers the transaction of a console (`path`, the active one) or of `connection` and `context`: mode (auto, manual, smart), whether it commits on its own right now, isolation, whether a transaction is open, how many statements it holds and how many may have changed data, its age and idle seconds, the seconds left when an idle transaction is about to be rolled back, and the log of its statements (the last `limit`, 50, with `truncated`). Never values or secrets.";

    fn run(&self, session: &mut Session, params: StatusParams) -> Result<TransactionState, CommandError> {
        let target = target(session, &params.target)?;
        Ok(state_of(session, &target.entry, &target.context, params.limit.unwrap_or(LOGGED)))
    }
}

/// How many pending transactions to answer.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PendingParams {
    /// The most transactions to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// The open transactions of every connection.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingAnswer {
    /// The transactions, by connection name, without their logs.
    pub transactions: Vec<TransactionState>,
    /// How many are open.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct Pending;

impl Command for Pending {
    type Params = PendingParams;
    type Output = PendingAnswer;
    const ID: &'static str = "db.transaction.pending";
    const TITLE: &'static str = "Pending Transactions";
    const DESCRIPTION: &'static str = "Lists the open transactions of every connection of the window (at most `limit`, 50, with `total` and `truncated`): connection, context, mode, statements and changes, age and idle seconds. End one with db.transaction.commit or db.transaction.rollback and its `connection` and `context`.";

    fn run(&self, session: &mut Session, params: PendingParams) -> Result<PendingAnswer, CommandError> {
        let pending = session.state::<Database>().and_then(Database::started).map(Engine::pending).unwrap_or_default();
        let total = pending.len();
        let limit = params.limit.unwrap_or(LISTED);
        Ok(PendingAnswer { transactions: pending.into_iter().take(limit).collect(), total, truncated: total > limit })
    }
}

/// Whether an idle transaction was kept open.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Postponed {
    /// Whether a rollback was announced and is now called off.
    pub postponed: bool,
}

pub struct Postpone;

impl Command for Postpone {
    type Params = TargetParams;
    type Output = Postponed;
    const ID: &'static str = "db.transaction.postpone";
    const TITLE: &'static str = "Keep Transaction Open";
    const DESCRIPTION: &'static str = "Keeps the idle transaction of a console (`path`, the active one) or of `connection` and `context` open: the rollback announced when it was idle as long as its connection type allows is called off, and its idle time starts again.";

    fn run(&self, session: &mut Session, params: TargetParams) -> Result<Postponed, CommandError> {
        let target = target(session, &params)?;
        let engine = engine(session);
        Ok(Postponed { postponed: engine.postpone(&target.entry.source.id, &target.context) })
    }
}

/// The console to close.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CloseParams {
    /// The console; the active one when absent.
    pub path: Option<PathBuf>,
    /// Changes pending in its transaction: `ask` (default) refuses with code `openTransactions`, `commit` or `rollback` ends them first.
    pub transactions: TransactionPolicy,
}

/// What closing a console did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Closed {
    /// The console.
    pub path: PathBuf,
    /// The operation that closes its session, when it had one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
}

pub fn close_console_session(
    session: &mut Session,
    path: &Path,
    policy: TransactionPolicy,
    retry: Option<(&str, serde_json::Value)>,
) -> Result<Option<u64>, CommandError> {
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else { return Ok(None) };
    let Some(id) = connection_of(session, path) else { return Ok(None) };
    let retry = retry.unwrap_or_else(|| (CloseConsole::ID, serde_json::json!({ "path": path })));
    engine
        .close_context(&id, context_of(path), policy)
        .map_err(|error| refuse_or_fail(&engine, &error, Some(&id), retry))
}

pub struct CloseConsole;

impl Command for CloseConsole {
    type Params = CloseParams;
    type Output = Closed;
    const ID: &'static str = "db.console.close";
    const TITLE: &'static str = "Close Console";
    const DESCRIPTION: &'static str = "Closes a console (`path`, the active one) and its session. With changes pending in its transaction and `transactions: ask` (default) it is refused with code `openTransactions`; `commit` or `rollback` ends the transaction first. A transaction of reads only is rolled back.";

    fn run(&self, session: &mut Session, params: CloseParams) -> Result<Closed, CommandError> {
        let path = super::consoles::console_path(session, params.path)?;
        let retry = serde_json::json!({ "path": path, "transactions": params.transactions });
        super::edits::refuse_staged(session, &console_results(session, &path), (Self::ID, retry))?;
        let operation = close_console_session(session, &path, params.transactions, None)?;
        let tab = session.state::<Workspaces>().and_then(|workspaces| {
            workspaces.tabs(super::WORKSPACE).iter().find(|tab| tab.path.as_deref() == Some(&path)).cloned()
        });
        if let Some(tab) = tab {
            crate::workspaces::close_inner_unguarded(session, super::WORKSPACE, &tab.id)?;
        }
        Ok(Closed { path, operation })
    }
}

fn console_results(session: &Session, path: &Path) -> Vec<u64> {
    session
        .state::<super::consoles::Consoles>()
        .map(|consoles| consoles.tabs(path).iter().map(|tab| tab.result).collect())
        .unwrap_or_default()
}

pub fn guard_close(session: &mut Session, workspace: &str, tab: &InnerTab) -> Result<(), CommandError> {
    if workspace != super::WORKSPACE {
        return Ok(());
    }
    let results = match tab.path.as_deref() {
        Some(path) => console_results(session, path),
        None if tab.kind == super::objects::KIND => super::objects::results_of(session, &tab.id),
        None => Vec::new(),
    };
    let retry = serde_json::json!({ "workspace": workspace, "tab": tab.id });
    if tab.kind == super::objects::KIND
        && session.state::<super::alter::Alterations>().is_some_and(|alterations| alterations.staged(&tab.id))
    {
        return Err(CommandError::with_code(
            super::alter::PENDING_STRUCTURE,
            t!("database-alter-question-close", name = tab.title.as_str()),
        )
        .with_data(
            serde_json::json!({ "tab": tab.id, "retry": { "command": "workspace.tab.close", "params": retry } }),
        ));
    }
    super::edits::refuse_staged(session, &results, ("workspace.tab.close", retry))?;
    let Some(path) = tab.path.as_deref() else { return Ok(()) };
    if !path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("sql")) && !super::injected::hosts(path)
    {
        return Ok(());
    }
    close_console_session(session, path, TransactionPolicy::Ask, None).map(|_| ())
}

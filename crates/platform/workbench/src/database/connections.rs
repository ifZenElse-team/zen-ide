use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{Cause, ContextKind, DbError, ErrorKind};
use zen_db::engine::{
    ConnectRequest, ConnectionStatus, Event, OperationKind, OperationState, OperationStatus, TransactionPolicy,
};
use zen_db::registry::{DataSource, Entry, PersonalOverride, Scope, new_id};
use zen_i18n::t;
use zen_secrets::Secret;
use zen_secrets::database::Kind as SecretKind;

use super::sources::{
    Database, SourceFields, apply_fields, check_driver, connection_types, find, has_password, secret_id, secret_store,
};
use crate::notifications::{Group, Kind, NewNotification, post};

const ROWS_ANSWERED: usize = 50;
const LISTED: usize = 50;

pub fn describe(error: &DbError) -> String {
    let detail = error.detail.as_str();
    match (error.kind, error.code.as_deref()) {
        (ErrorKind::Connection, Some("notReplayed")) => t!("database-error-not-replayed", detail = detail),
        (ErrorKind::Cancelled | ErrorKind::Timeout, Some("cancelUnconfirmed")) => {
            t!("database-error-cancel-unconfirmed")
        }
        (ErrorKind::Cancelled | ErrorKind::Timeout, Some("noServerCancel")) => t!("database-error-no-server-cancel"),
        (ErrorKind::Connection | ErrorKind::Timeout, Some(zen_db::ssh::SSH_FAILED)) => {
            t!("database-error-ssh", detail = detail)
        }
        (_, Some(zen_db::sqlite::process::MISSING)) => t!("database-error-sqlite-missing", detail = detail),
        (_, Some(zen_db::sqlite::NO_FILE)) => t!("database-error-file-missing", detail = detail),
        (ErrorKind::Connection, _) => t!("database-error-connection", detail = detail),
        (ErrorKind::Authentication, _) => t!("database-error-authentication", detail = detail),
        (ErrorKind::Tls, _) => t!("database-error-tls", detail = detail),
        (ErrorKind::PasswordNeeded, Some("secretDenied")) => t!("database-error-secret-denied"),
        (ErrorKind::PasswordNeeded, Some("secretTimeout")) => t!("database-error-secret-timeout"),
        (ErrorKind::PasswordNeeded, Some("secretFailed")) => t!("database-error-secret-failed", reason = detail),
        (ErrorKind::PasswordNeeded, _) => t!("database-error-password-needed"),
        (ErrorKind::Timeout, _) => t!("database-error-timeout"),
        (ErrorKind::Cancelled, _) => t!("database-error-cancelled"),
        (ErrorKind::Server, Some(zen_db::postgres::ROLLED_BACK)) => t!("database-error-rolled-back"),
        (ErrorKind::Server, None) => detail.to_owned(),
        (ErrorKind::Server, _) => {
            t!("database-error-server", detail = detail, code = error.code.clone().unwrap_or_default())
        }
        (ErrorKind::NotConnected, _) => t!("database-error-not-connected"),
        (ErrorKind::OpenTransactions, _) => t!("database-error-open-transactions", contexts = detail),
        (ErrorKind::Invalid, _) => t!("database-error-invalid", detail = detail),
        (ErrorKind::Unsupported, Some(zen_db::results::NOT_REPEATABLE)) => t!("database-error-not-repeatable"),
        (ErrorKind::Unsupported, _) => t!("database-error-unsupported", detail = detail),
        (ErrorKind::Refused, Some("readOnly")) => t!("database-error-read-only-statement", verb = detail),
        (ErrorKind::Refused, _) => t!("database-error-refused", detail = detail),
        (ErrorKind::Secret, _) => detail.to_owned(),
    }
}

pub fn unread(status: OperationStatus, error: Option<&DbError>) -> String {
    match (error, status) {
        (Some(error), _) => describe(error),
        (None, OperationStatus::Done) => t!("database-error-no-columns"),
        (None, _) => t!("database-error-cancelled"),
    }
}

pub fn hint(cause: Cause) -> String {
    match cause {
        Cause::Dns => t!("database-cause-dns"),
        Cause::Refused => t!("database-cause-refused"),
        Cause::Tls => t!("database-cause-tls"),
        Cause::Authentication => t!("database-cause-authentication"),
        Cause::Timeout => t!("database-cause-timeout"),
    }
}

pub(super) fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::OpenTransactions => "openTransactions",
        ErrorKind::NotConnected => "notConnected",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

pub fn request(session: &mut Session, entry: &Entry, password: Option<String>) -> ConnectRequest {
    let (types, _) = connection_types(session);
    let kind = types.resolve(&entry.source.connection_type);
    let password = password.filter(|password| !password.is_empty()).map(Secret::new);
    let stored = (password.is_none() && has_password(entry))
        .then(|| {
            let store = secret_store(session).ok()?;
            let id = secret_id(session, entry, SecretKind::Password).ok()?;
            Some(zen_db::engine::StoredSecret { store, id })
        })
        .flatten();
    let ssh_secret = entry.source.ssh.as_ref().filter(|ssh| !ssh.host.trim().is_empty()).and_then(|_| {
        let store = secret_store(session).ok()?;
        let id = secret_id(session, entry, SecretKind::Ssh).ok()?;
        Some(zen_db::engine::StoredSecret { store, id })
    });
    let mut source = entry.source.clone();
    source.database = source.database.map(|database| super::files::resolve(session, &source.driver, database));
    if entry.scope == Scope::Project {
        for key in PROGRAM_SETTINGS {
            source.settings.remove(key);
        }
    }
    ConnectRequest { source, kind, password, stored, ssh_secret }
}

pub const PROGRAM_SETTINGS: [&str; 2] = ["program", "javaHome"];

/// Names a connection, with a password given once.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ConnectParams {
    /// The connection's id.
    pub connection: String,
    /// The password for this connect only, when none is stored; kept in memory until it disconnects, never stored or answered.
    pub password: Option<String>,
}

/// An operation started on a connection.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The operation's id, for `db.operation.state`.
    pub operation: u64,
    /// The connection's status now.
    pub status: ConnectionStatus,
}

pub struct Connect;

impl Command for Connect {
    type Params = ConnectParams;
    type Output = Started;
    const ID: &'static str = "db.connection.connect";
    const TITLE: &'static str = "Connect";
    const DESCRIPTION: &'static str = "Connects database connection `connection` in the background: Main, then Metadata (consoles get their own sessions on first use). Answers at once with the operation id (read it with db.operation.state) and the status. A connect while one runs answers that one. With no stored password and none in `password`, refused with code `passwordNeeded`; `password` is used for this connection only and never stored.";

    fn run(&self, session: &mut Session, params: ConnectParams) -> Result<Started, CommandError> {
        let entry = find(session, &params.connection)?;
        let request = request(session, &entry, params.password);
        let engine = session.state_mut::<Database>().engine();
        let operation = engine.connect(request).map_err(|error| failed(&error))?;
        Ok(Started { operation, status: engine.status(&params.connection) })
    }
}

/// Settings to test: a saved connection, the fields of one not saved yet, or a saved one with fields changed for this test only.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TestParams {
    /// A saved connection whose settings are the start; a new one when absent.
    pub connection: Option<String>,
    /// The scope of a connection not saved yet: `global` (default) or `project`.
    pub scope: Scope,
    /// Settings that change the saved ones, or make a new one, for this test only; `password` is used once and never stored.
    #[serde(flatten)]
    pub fields: SourceFields,
}

/// A test started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TestStarted {
    /// The operation's id, for `db.operation.state`, whose `test` has the server's version and the round trip.
    pub operation: u64,
}

pub struct TestSource;

impl Command for TestSource {
    type Params = TestParams;
    type Output = TestStarted;
    const ID: &'static str = "db.dataSource.test";
    const TITLE: &'static str = "Test Connection";
    const DESCRIPTION: &'static str = "Tests database settings in the background: saved connection `connection`, the fields of a new one (`driver`, `url` or `host`, `port`, `database`, `user`, `password`, `tls`, `settings`, … as for db.dataSource.add), or a saved one with fields changed for this test only. Opens a session of its own, measures the round trip, reads the server's version and closes it; saves nothing and never connects the connection itself. Answers the operation id at once; db.operation.state answers `test { serverVersion, connectMs, roundTripMs }`, or the error with its likely `cause` (dns, refused, tls, authentication, timeout). Without `password` a saved one is used.";

    fn run(&self, session: &mut Session, params: TestParams) -> Result<TestStarted, CommandError> {
        let (mut source, scope) = match &params.connection {
            Some(id) => {
                let entry = find(session, id)?;
                (entry.source, entry.scope)
            }
            None => {
                let mut source = DataSource { save_password: true, ..DataSource::default() };
                source.connection_type = zen_db::types::DEVELOPMENT.to_owned();
                (source, params.scope)
            }
        };
        if source.name.is_empty() {
            source.name =
                params.fields.name.clone().filter(|name| !name.trim().is_empty()).unwrap_or_else(|| "test".to_owned());
        }
        let drivers = session.state_mut::<Database>().drivers();
        let mut personal = PersonalOverride::default();
        let password = apply_fields(&mut source, &mut personal, &params.fields, false, &drivers)?;
        if source.driver.is_empty() {
            source.driver = zen_db::postgres::ID.to_owned();
        }
        check_driver(session, &source.driver)?;
        if source.id.is_empty() {
            source.id = new_id(&source.name);
        }
        let entry = Entry { scope, source, personal: None };
        let request = request(session, &entry, password);
        let engine = session.state_mut::<Database>().engine();
        let operation = engine.test(request).map_err(|error| failed(&error))?;
        Ok(TestStarted { operation })
    }
}

/// How to disconnect.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DisconnectParams {
    /// The connection's id.
    pub connection: String,
    /// Open transactions: `ask` (default) refuses with code `openTransactions` naming the contexts, `commit` or `rollback` ends them first.
    pub transactions: TransactionPolicy,
}

pub struct Disconnect;

impl Command for Disconnect {
    type Params = DisconnectParams;
    type Output = Started;
    const ID: &'static str = "db.connection.disconnect";
    const TITLE: &'static str = "Disconnect";
    const DESCRIPTION: &'static str = "Disconnects database connection `connection`: running statements are cancelled and every session closes; a password given at connect is forgotten. With an open transaction and `transactions: ask` (default) it is refused with code `openTransactions` and the contexts; `commit` or `rollback` ends them first.";

    fn run(&self, session: &mut Session, params: DisconnectParams) -> Result<Started, CommandError> {
        let engine = session.state_mut::<Database>().engine();
        let retry = serde_json::json!({ "connection": params.connection });
        let operation = engine
            .disconnect(&params.connection, params.transactions)
            .map_err(|error| asked(&engine, &error, Some(&params.connection), (Self::ID, retry)))?;
        Ok(Started { operation, status: engine.status(&params.connection) })
    }
}

/// How to disconnect every connection.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DisconnectAllParams {
    /// Open transactions: `ask`, `commit` or `rollback`, as for db.connection.disconnect.
    pub transactions: TransactionPolicy,
}

/// The operations started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartedMany {
    /// Their ids.
    pub operations: Vec<u64>,
}

pub struct DisconnectAll;

impl Command for DisconnectAll {
    type Params = DisconnectAllParams;
    type Output = StartedMany;
    const ID: &'static str = "db.connection.disconnectAll";
    const TITLE: &'static str = "Disconnect All";
    const DESCRIPTION: &'static str = "Disconnects every database connection of the window. With `transactions: ask` (default) an open transaction anywhere refuses it with code `openTransactions`, naming connection and context.";

    fn run(&self, session: &mut Session, params: DisconnectAllParams) -> Result<StartedMany, CommandError> {
        let engine = session.state_mut::<Database>().engine();
        let operations = engine
            .disconnect_all(params.transactions)
            .map_err(|error| asked(&engine, &error, None, (Self::ID, serde_json::json!({}))))?;
        Ok(StartedMany { operations })
    }
}

/// How to reconnect.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InvalidateParams {
    /// The connection's id.
    pub connection: String,
    /// Changes pending in open transactions: `ask` (default) refuses with code `openTransactions`, `commit` or `rollback` ends them first.
    pub transactions: TransactionPolicy,
}

fn asked(
    engine: &zen_db::engine::Engine,
    error: &DbError,
    connection: Option<&str>,
    retry: (&str, serde_json::Value),
) -> CommandError {
    if error.kind == ErrorKind::OpenTransactions {
        super::transactions::open_question(engine, error, connection, retry)
    } else {
        failed(error)
    }
}

pub struct Invalidate;

impl Command for Invalidate {
    type Params = InvalidateParams;
    type Output = Started;
    const ID: &'static str = "db.connection.invalidate";
    const TITLE: &'static str = "Invalidate/Reconnect";
    const DESCRIPTION: &'static str = "Invalidates database connection `connection`: cancels what runs, closes every session and opens them again with their tracked state (schema, isolation, commit mode); nothing is run again. Changes pending in an open transaction refuse it with code `openTransactions` unless `transactions` is `commit` or `rollback`; a transaction of reads only is rolled back.";

    fn run(&self, session: &mut Session, params: InvalidateParams) -> Result<Started, CommandError> {
        let engine = session.state_mut::<Database>().engine();
        let retry = serde_json::json!({ "connection": params.connection });
        let operation = engine
            .invalidate(&params.connection, params.transactions)
            .map_err(|error| asked(&engine, &error, Some(&params.connection), (Self::ID, retry)))?;
        Ok(Started { operation, status: engine.status(&params.connection) })
    }
}

/// Which statuses to answer.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusParams {
    /// One connection; every connection of the registry when absent.
    pub connection: Option<String>,
    /// The most connections to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// Connection statuses.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Statuses {
    /// The statuses.
    pub connections: Vec<ConnectionStatus>,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct Status;

impl Command for Status {
    type Params = StatusParams;
    type Output = Statuses;
    const ID: &'static str = "db.connection.status";
    const TITLE: &'static str = "Connection Status";
    const DESCRIPTION: &'static str = "Answers the state of one database connection or of all (`limit`, 50): disconnected, connecting, connected, closed (by idle time or sleep, opened again on next use) or failed with the error; auto-commit, pooler, and each context (main, metadata, consoles) with open, inTransaction, busy and idle seconds. Never secrets.";

    fn run(&self, session: &mut Session, params: StatusParams) -> Result<Statuses, CommandError> {
        let ids: Vec<String> = match params.connection {
            Some(id) => vec![id],
            None => super::sources::files(session)
                .entries()
                .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?
                .into_iter()
                .map(|entry| entry.source.id)
                .collect(),
        };
        let engine = session.state::<Database>().and_then(Database::started).cloned();
        let total = ids.len();
        let limit = params.limit.unwrap_or(LISTED);
        let connections = ids
            .iter()
            .take(limit)
            .map(|id| match &engine {
                Some(engine) => engine.status(id),
                None => ConnectionStatus::disconnected(id),
            })
            .collect();
        Ok(Statuses { connections, total, truncated: total > limit })
    }
}

/// What to cancel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CancelParams {
    /// The connection whose running operations to cancel.
    pub connection: Option<String>,
    /// Only this context (`{"kind": "console", "name": "console_1"}`, `{"kind": "main"}`).
    pub context: Option<ContextKind>,
    /// One operation instead.
    pub operation: Option<u64>,
}

/// The operations asked to stop.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Their ids; their end, within a second, is in db.operation.state.
    pub operations: Vec<u64>,
}

pub struct Cancel;

impl Command for Cancel {
    type Params = CancelParams;
    type Output = Cancelled;
    const ID: &'static str = "db.connection.cancel";
    const TITLE: &'static str = "Cancel";
    const DESCRIPTION: &'static str = "Cancels running database operations: one `operation`, or those of `connection` (only `context` when given). The local work stops and the server is asked to stop (PostgreSQL's CancelRequest); within a second the operation ends as cancelled, or with code `cancelUnconfirmed` when the server did not confirm, and that session is closed.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<Cancelled, CommandError> {
        let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else {
            return Ok(Cancelled { operations: Vec::new() });
        };
        let operations = match (params.operation, &params.connection) {
            (Some(operation), _) => {
                if engine.cancel(operation) {
                    vec![operation]
                } else {
                    Vec::new()
                }
            }
            (None, Some(connection)) => engine.cancel_connection(connection, params.context.as_ref()),
            (None, None) => return Err(CommandError::new(t!("database-error-cancel-what"))),
        };
        Ok(Cancelled { operations })
    }
}

/// Which operation to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OperationParams {
    /// The operation's id.
    pub operation: u64,
    /// The most rows of a result to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// An operation and, when it failed, the reason in words.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OperationAnswer {
    /// The operation.
    #[serde(flatten)]
    pub state: OperationState,
    /// Why it failed, in the interface's language, with the likely cause when it did not connect.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The likely cause of a failed connect or test: `dns`, `refused`, `tls`, `authentication` or `timeout`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cause: Option<Cause>,
    /// Whether rows were left out because of `limit`.
    pub rows_truncated: bool,
}

pub struct OperationStateCommand;

impl Command for OperationStateCommand {
    type Params = OperationParams;
    type Output = OperationAnswer;
    const ID: &'static str = "db.operation.state";
    const TITLE: &'static str = "Database Operation State";
    const DESCRIPTION: &'static str = "Answers a database operation by id: kind, connection, context, status (running, done, failed, cancelled), milliseconds, the error (with `message` in words), whether it reconnected, and a result's columns and first rows (`limit`, 50). The last 200 operations are kept.";

    fn run(&self, session: &mut Session, params: OperationParams) -> Result<OperationAnswer, CommandError> {
        let state = session
            .state::<Database>()
            .and_then(Database::started)
            .and_then(|engine| engine.operation(params.operation))
            .ok_or_else(|| CommandError::new(t!("database-error-unknown-operation", id = params.operation)))?;
        let mut state = state;
        let limit = params.limit.unwrap_or(ROWS_ANSWERED);
        let mut rows_truncated = false;
        if let Some(execution) = state.execution.as_mut()
            && execution.rows.len() > limit
        {
            execution.rows.truncate(limit);
            rows_truncated = true;
        }
        let connecting = matches!(state.kind, OperationKind::Connect | OperationKind::Test);
        let cause = state.error.as_ref().filter(|_| connecting).and_then(DbError::cause);
        let message = state.error.as_ref().map(|error| match cause {
            Some(cause) => format!("{} {}", describe(error), hint(cause)),
            None => describe(error),
        });
        Ok(OperationAnswer { state, message, cause, rows_truncated })
    }
}

#[derive(Clone, Debug, Default)]
pub struct SshPrompts(pub Vec<zen_db::engine::SshPrompt>);

/// How many questions to answer.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SshPromptsParams {
    /// The most questions to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// The questions ssh is waiting on.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshPromptList {
    /// The questions, oldest first.
    pub prompts: Vec<zen_db::engine::SshPrompt>,
    /// How many are waiting.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct ListSshPrompts;

impl Command for ListSshPrompts {
    type Params = SshPromptsParams;
    type Output = SshPromptList;
    const ID: &'static str = "db.ssh.prompts";
    const TITLE: &'static str = "SSH Questions";
    const DESCRIPTION: &'static str = "Lists the questions ssh is waiting on while a connection's tunnel opens (at most `limit`, 50): `id`, connection, ssh's prompt as it wrote it, whether the answer is a secret (a password or key passphrase) and whether it is a yes-or-no question such as an unknown host key. A saved SSH secret answers the first password or passphrase prompt by itself; everything else waits for an answer (db.ssh.answer) for three minutes. Never an answer.";

    fn run(&self, session: &mut Session, params: SshPromptsParams) -> Result<SshPromptList, CommandError> {
        let prompts = session
            .state::<Database>()
            .and_then(Database::started)
            .map(|engine| engine.ssh_prompts())
            .unwrap_or_default();
        let total = prompts.len();
        let limit = params.limit.unwrap_or(LISTED);
        Ok(SshPromptList { prompts: prompts.into_iter().take(limit).collect(), total, truncated: total > limit })
    }
}

/// The answer to one of ssh's questions.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SshAnswerParams {
    /// The question's id, from db.ssh.prompts.
    pub prompt: u64,
    /// The answer (`yes` or `no` to a yes-or-no question); absent refuses the question and the tunnel fails.
    pub value: Option<String>,
    /// Keep a password or passphrase in the macOS Keychain for this connection.
    pub save: bool,
}

/// Whether the question was waiting.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SshAnswered {
    /// Whether it was still waiting and got the answer.
    pub answered: bool,
}

pub struct AnswerSsh;

impl Command for AnswerSsh {
    type Params = SshAnswerParams;
    type Output = SshAnswered;
    const ID: &'static str = "db.ssh.answer";
    const TITLE: &'static str = "Answer SSH";
    const DESCRIPTION: &'static str = "Answers ssh's question `prompt` (db.ssh.prompts) with `value`, given to ssh through the IDE's askpass helper and never written to a file, a command line or a log; without `value` the question is refused and the tunnel fails. `save` keeps a password or passphrase in the macOS Keychain as the connection's `ssh` secret. An unknown host key is never accepted unless answered `yes`.";

    fn run(&self, session: &mut Session, params: SshAnswerParams) -> Result<SshAnswered, CommandError> {
        session.state_mut::<SshPrompts>().0.retain(|prompt| prompt.id != params.prompt);
        let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else {
            return Ok(SshAnswered { answered: false });
        };
        let answer = params.value.map(Secret::new);
        Ok(SshAnswered { answered: engine.answer_ssh(params.prompt, answer, params.save) })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdleTransaction {
    pub connection: String,
    pub name: String,
    pub context: ContextKind,
    pub until: std::time::Instant,
}

#[derive(Clone, Debug, Default)]
pub struct IdleTransactions(pub Vec<IdleTransaction>);

pub fn step(session: &mut Session) {
    super::navigator::step(session);
    super::migrations::step(session);
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else {
        super::structure::step(session, &[]);
        return;
    };
    let mut changed = Vec::new();
    for event in engine.take_events() {
        let body = match event {
            Event::IdleClosed { name, .. } => t!("database-notice-idle-closed", name = name),
            Event::Slept { count } => t!("database-notice-slept", count = count),
            Event::TransactionEnded { name, context, .. } => {
                session.state_mut::<IdleTransactions>().0.retain(|idle| idle.name != name || idle.context != context);
                t!(
                    "database-notice-transaction-ended",
                    name = name,
                    context = super::transactions::context_name(&context)
                )
            }
            Event::TransactionIdle { connection, name, context, seconds } => {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(seconds);
                session.state_mut::<IdleTransactions>().0.push(IdleTransaction { connection, name, context, until });
                continue;
            }
            Event::Reconnected { .. } => continue,
            Event::ProfileDetected { connection, name, profile } => {
                let Some(found) = zen_db::profile::get(&profile) else { continue };
                let body = t!("database-notice-profile-detected", name = name, profile = found.name);
                let action = crate::notifications::Action::new(
                    "useProfile",
                    t!("database-notice-profile-use", profile = found.name),
                    "db.dataSource.update",
                    serde_json::json!({ "id": connection, "profile": profile }),
                );
                post(session, NewNotification::message(Group::General, Kind::Info, body).action(action));
                continue;
            }
            Event::SecretFailed { connection, stored, detail } => {
                if stored {
                    super::sources::password_not_saved(session, &connection);
                }
                let body = t!("database-notice-secret-failed", detail = detail);
                post(session, NewNotification::message(Group::General, Kind::Warning, body));
                continue;
            }
            Event::SshPrompt { prompt } => {
                session.state_mut::<SshPrompts>().0.push(prompt);
                continue;
            }
            Event::StructureChanged { connection } => {
                changed.push(connection);
                continue;
            }
        };
        post(session, NewNotification::message(Group::General, Kind::Info, body));
    }
    super::structure::step(session, &changed);
    super::alter::step(session);
    super::exports::step(session);
    super::csv_imports::step(session);
    super::transfers::step(session);
    super::clipboard::step(session);
    super::clients::step(session);
}

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::{Capabilities, Node, StatementKind};
use crate::registry::DataSource;
use crate::schema::{DdlOptions, IntrospectRequest, Introspection, Snapshot};
use crate::value::{Execution, KeyValue, Notice};
use zen_secrets::Secret;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// What went wrong, as a stable kind views and agents act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// The server could not be reached or the connection was lost.
    Connection,
    /// The server refused the credentials.
    Authentication,
    /// TLS failed: no TLS on the server, or a certificate that does not verify.
    Tls,
    /// A password is needed and none was given or stored.
    PasswordNeeded,
    /// The deadline passed.
    Timeout,
    /// It was cancelled.
    Cancelled,
    /// The server reported an error for the statement.
    Server,
    /// The connection is not connected.
    NotConnected,
    /// Open transactions stop a disconnect until the person decides.
    OpenTransactions,
    /// The connection or its settings are not usable as they are.
    Invalid,
    /// Refused before it was sent: a change on a read-only connection, or what an agent may not run.
    Refused,
    /// The driver cannot do this.
    Unsupported,
    /// The secret store (the Keychain) refused or failed; the detail says why, never the secret.
    Secret,
}

/// An error of the engine or a driver; its detail never holds a secret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DbError {
    /// The kind.
    pub kind: ErrorKind,
    /// What the server or library said, passwords removed; may be empty.
    pub detail: String,
    /// The SQLSTATE or the server's error code, when it sent one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Where in the statement the server found the error: a one-based character position, when it said.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub position: Option<usize>,
}

impl DbError {
    pub fn new(kind: ErrorKind, detail: impl AsRef<str>) -> Self {
        Self { kind, detail: crate::registry::redact(detail.as_ref()), code: None, position: None }
    }

    #[must_use]
    pub fn at(mut self, position: usize) -> Self {
        self.position = Some(position);
        self
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for DbError {}

/// The likely cause of a failed connect, for a person to act on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Cause {
    /// The host name is not known to DNS.
    Dns,
    /// Nothing listens on that host and port.
    Refused,
    /// TLS failed: the server has none, or its certificate does not verify.
    Tls,
    /// The server refused the user name or the password.
    Authentication,
    /// The server did not answer in time.
    Timeout,
}

impl DbError {
    pub fn cause(&self) -> Option<Cause> {
        let detail = self.detail.to_ascii_lowercase();
        match self.kind {
            ErrorKind::Tls => Some(Cause::Tls),
            ErrorKind::Authentication => Some(Cause::Authentication),
            ErrorKind::Timeout => Some(Cause::Timeout),
            ErrorKind::Connection
                if [
                    "failed to lookup address",
                    "nodename nor servname",
                    "name or service not known",
                    "no such host",
                    "dns",
                ]
                .iter()
                .any(|words| detail.contains(words)) =>
            {
                Some(Cause::Dns)
            }
            ErrorKind::Connection if detail.contains("refused") => Some(Cause::Refused),
            ErrorKind::Connection if detail.contains("timed out") => Some(Cause::Timeout),
            _ => None,
        }
    }
}

/// Which execution context of a connection a session serves (ADR-0164, decision 2).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind", content = "name")]
pub enum ContextKind {
    /// The main context, for the data editors.
    Main,
    /// Reading structure for the navigator and completion; the IDE's own SQL.
    Metadata,
    /// One console, by its name, with its own transaction.
    Console(String),
}

impl fmt::Display for ContextKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ContextKind::Main => f.write_str("main"),
            ContextKind::Metadata => f.write_str("metadata"),
            ContextKind::Console(name) => write!(f, "console:{name}"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Guards {
    pub application_name: String,
    pub statement_timeout: Option<Duration>,
    pub lock_timeout: Option<Duration>,
    pub idle_in_transaction_timeout: Option<Duration>,
}

/// Session state the engine tracks and restores after a reconnect; nothing else is restored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrackedState {
    /// The current database, when the driver switches databases in a session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// The current schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The isolation level, by the driver's name for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
    /// Whether statements commit on their own.
    pub auto_commit: bool,
    /// Whether the session is read-only on the server.
    pub read_only: bool,
}

#[derive(Clone, Debug)]
pub struct ConnectSpec {
    pub source: DataSource,
    pub password: Option<Secret>,
    pub context: ContextKind,
    pub guards: Guards,
    pub state: TrackedState,
    pub timeout: Duration,
    pub via: Option<std::net::SocketAddr>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecOptions {
    pub max_rows: usize,
    pub timeout: Option<Duration>,
}

impl Default for ExecOptions {
    fn default() -> Self {
        Self { max_rows: 200, timeout: None }
    }
}

pub type Param = Option<String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamOptions {
    pub page: usize,
    pub skip: u64,
}

impl Default for StreamOptions {
    fn default() -> Self {
        Self { page: 200, skip: 0 }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamEnd {
    pub affected: Option<u64>,
    pub complete: bool,
    pub notices: Vec<Notice>,
}

/// What to do with a key of a key-value database before reading it back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "action", content = "seconds")]
pub enum KeyChange {
    /// Make it expire after this many seconds.
    Expire(u64),
    /// Make it never expire.
    Persist,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyRequest {
    pub limit: usize,
    pub change: Option<KeyChange>,
}

pub trait RowSink: Send {
    fn columns(&mut self, columns: Vec<crate::value::Column>);
    fn portal(&mut self, in_transaction: bool);
    fn want(&mut self) -> BoxFuture<'_, usize>;
    fn push(&mut self, rows: Vec<Vec<crate::value::Value>>) -> bool;
    fn cap(&self) -> usize;
}

pub trait ByteSink: Send {
    fn columns(&mut self, columns: Vec<crate::value::Column>);
    fn want(&mut self) -> BoxFuture<'_, bool>;
    fn push(&mut self, bytes: Vec<u8>) -> bool;
}

pub trait Canceller: Send + Sync {
    fn cancel(&self) -> BoxFuture<'static, Result<(), DbError>>;
}

pub trait Driver: Send + Sync {
    fn id(&self) -> &str;
    fn name(&self) -> &str;
    fn capabilities(&self) -> &Capabilities;
    fn default_port(&self) -> Option<u16>;
    fn classify(&self, statement: &str) -> StatementKind;
    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> BoxFuture<'a, Result<Box<dyn DriverSession>, DbError>>;
    fn open_text(&self, _path: &[String], _kind: &crate::model::NodeKind) -> Option<String> {
        None
    }
    fn changes_structure(&self, _statement: &str) -> bool {
        false
    }
    fn guards_read_only(&self) -> bool {
        false
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn external(&self) -> Option<&crate::external::ExternalDriver> {
        None
    }
    fn assess(&self, statement: &str) -> crate::safety::Assessment {
        match crate::sql::Dialect::of_language(&self.capabilities().language) {
            dialect if dialect.commands() => {
                crate::safety::assess_command(statement, self.classify(statement) == StatementKind::Read)
            }
            dialect => crate::safety::assess(statement, dialect),
        }
    }
    fn repeatable(&self, statement: &str) -> bool {
        let relational = self.capabilities().models.contains(&crate::model::DataModel::Relational);
        self.assess(statement).reads() && (!relational || crate::sql::pageable(statement))
    }
    fn explain_options(&self) -> Vec<crate::explain::ExplainOption> {
        Vec::new()
    }
    fn explain(
        &self,
        _statement: &str,
        _options: &crate::explain::ExplainOptions,
    ) -> Result<crate::explain::ExplainRequest, DbError> {
        Err(DbError::new(ErrorKind::Unsupported, "explain"))
    }
    fn ddl(&self, _snapshot: &Snapshot, _path: &[String], _options: &DdlOptions) -> Result<String, DbError> {
        Err(DbError::new(ErrorKind::Unsupported, "ddl"))
    }
}

pub trait DriverSession: Send {
    fn execute<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a ExecOptions,
    ) -> BoxFuture<'a, Result<Execution, DbError>>;
    fn stream<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a StreamOptions,
        sink: &'a mut dyn RowSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        Box::pin(async move {
            let skip = usize::try_from(options.skip).unwrap_or(usize::MAX);
            let max_rows = skip.saturating_add(sink.cap().max(1));
            let execution = self.execute(statement, &ExecOptions { max_rows, timeout: None }).await?;
            sink.columns(execution.columns);
            let mut rows = execution.rows.into_iter().skip(skip).peekable();
            let mut kept = true;
            while rows.peek().is_some() {
                let page: Vec<_> = rows.by_ref().take(options.page.max(1)).collect();
                kept = sink.push(page);
                if !kept {
                    break;
                }
            }
            let complete = kept && !execution.truncated;
            Ok(StreamEnd { affected: execution.affected, complete, notices: execution.notices })
        })
    }
    fn copy_out<'a>(
        &'a mut self,
        _statement: &'a str,
        _options: &'a crate::export::CopyOptions,
        _sink: &'a mut dyn ByteSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        Box::pin(async { Err(DbError::new(ErrorKind::Unsupported, "copy")) })
    }
    fn copy_in<'a>(&'a mut self, _statement: &'a str, _data: Vec<u8>) -> BoxFuture<'a, Result<u64, DbError>> {
        Box::pin(async { Err(DbError::new(ErrorKind::Unsupported, "copy")) })
    }
    fn execute_bound<'a>(
        &'a mut self,
        _statement: &'a str,
        _params: &'a [Param],
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(async { Err(DbError::new(ErrorKind::Unsupported, "bound parameters")) })
    }
    fn stream_bound<'a>(
        &'a mut self,
        statement: &'a str,
        params: &'a [crate::value::Value],
        options: &'a StreamOptions,
        sink: &'a mut dyn RowSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        if params.is_empty() {
            return self.stream(statement, options, sink);
        }
        Box::pin(async { Err(DbError::new(ErrorKind::Unsupported, "bound parameters")) })
    }
    fn children<'a>(&'a mut self, path: &'a [String]) -> BoxFuture<'a, Result<Vec<Node>, DbError>>;
    fn introspect<'a>(
        &'a mut self,
        _request: &'a IntrospectRequest,
        _previous: Option<&'a Snapshot>,
    ) -> BoxFuture<'a, Result<Introspection, DbError>> {
        Box::pin(async { Err(DbError::new(ErrorKind::Unsupported, "introspect")) })
    }
    fn key<'a>(
        &'a mut self,
        _path: &'a [String],
        _request: &'a KeyRequest,
    ) -> BoxFuture<'a, Result<KeyValue, DbError>> {
        Box::pin(async { Err(DbError::new(ErrorKind::Unsupported, "key")) })
    }
    fn sessions<'a>(
        &'a mut self,
        _request: &'a crate::sessions::SessionsRequest,
    ) -> BoxFuture<'a, Result<crate::sessions::ServerSessions, DbError>> {
        Box::pin(async { Err(DbError::new(ErrorKind::Unsupported, "sessions")) })
    }
    fn end_session<'a>(
        &'a mut self,
        _id: &'a str,
        _end: crate::sessions::SessionEnd,
    ) -> BoxFuture<'a, Result<crate::sessions::Ended, DbError>> {
        Box::pin(async { Err(DbError::new(ErrorKind::Unsupported, "sessions")) })
    }
    fn selected_database(&self) -> Option<String> {
        None
    }
    fn server_id(&self) -> Option<String> {
        None
    }
    fn ping(&mut self) -> BoxFuture<'_, Result<(), DbError>>;
    fn server_version(&mut self) -> BoxFuture<'_, Result<Option<String>, DbError>> {
        Box::pin(async { Ok(None) })
    }
    fn apply<'a>(&'a mut self, state: &'a TrackedState) -> BoxFuture<'a, Result<(), DbError>>;
    fn commit(&mut self) -> BoxFuture<'_, Result<(), DbError>>;
    fn rollback(&mut self) -> BoxFuture<'_, Result<(), DbError>>;
    fn in_transaction(&self) -> bool;
    fn is_closed(&self) -> bool;
    fn canceller(&self) -> Option<Arc<dyn Canceller>>;
    fn take_notices(&mut self) -> Vec<Notice>;
}

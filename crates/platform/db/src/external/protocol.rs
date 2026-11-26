use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::driver::{DbError, ErrorKind};
use crate::model::{Capabilities, Node};
use crate::schema::Catalog;
use crate::value::{Column, Notice, Value};

pub const VERSION: u32 = 1;

pub const MAX_LINE: usize = 64 << 20;

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const DATABASE_ERROR: i64 = -32000;
pub const UNSUPPORTED_PROTOCOL: i64 = -32010;
pub const UNKNOWN_SESSION: i64 = -32011;
pub const UNKNOWN_CURSOR: i64 = -32012;

/// The IDE as `initialize` names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// The product's name (`Zen GoGarden`).
    pub name: String,
    /// Its version.
    pub version: String,
}

/// `initialize`: the first request, before any other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    /// The protocol version the IDE speaks.
    pub protocol: u32,
    /// The driver id the IDE expects, from the plug-in's manifest.
    pub driver: String,
    /// The IDE.
    pub client: ClientInfo,
}

/// The driver as `initialize` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriverInfo {
    /// Its stable id, the same as in the manifest.
    pub id: String,
    /// Its name as shown.
    pub name: String,
    /// Its version.
    #[serde(default)]
    pub version: String,
}

/// The answer to `initialize`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// The protocol version the driver speaks, the one asked for.
    pub protocol: u32,
    /// The driver.
    pub driver: DriverInfo,
    /// What it can do: data models, query language, transactions, settings.
    pub capabilities: Capabilities,
}

/// `connect`: opens one session; the password travels only here.
#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectParams {
    /// The connection's id.
    pub connection: String,
    /// The execution context: `main`, `metadata` or `console:<name>`.
    pub context: String,
    /// The host to connect to, already the tunnel's when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The port.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The database.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// The user name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// The password, when the connection has one; never logged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// The URI the settings were taken from, without its password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Whether the connection must not change data.
    #[serde(default)]
    pub read_only: bool,
    /// The TLS settings, as the connection keeps them.
    #[serde(default)]
    pub tls: Json,
    /// The driver's own settings, by the keys its capabilities declare.
    #[serde(default)]
    pub settings: serde_json::Map<String, Json>,
    /// The name the session shows on the server.
    #[serde(default)]
    pub application_name: String,
    /// The longest a statement may run, in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement_timeout_ms: Option<u64>,
    /// The state to start in.
    #[serde(default)]
    pub state: crate::driver::TrackedState,
    /// How long connecting may take, in milliseconds.
    pub timeout_ms: u64,
}

impl std::fmt::Debug for ConnectParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectParams")
            .field("connection", &self.connection)
            .field("context", &self.context)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("database", &self.database)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .finish_non_exhaustive()
    }
}

/// The answer to `connect`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResult {
    /// The session's id, which every later request names.
    pub session: String,
    /// The server's name and version, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_version: Option<String>,
    /// Whether a transaction is open.
    #[serde(default)]
    pub in_transaction: bool,
}

/// What a session's state is after a request that may change it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionState {
    /// Whether a transaction is open; unchanged when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_transaction: Option<bool>,
    /// The messages the server sent.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<Notice>,
}

/// The answer to `execute`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteResult {
    /// The columns of the rows; empty when the statement returned none.
    #[serde(default)]
    pub columns: Vec<Column>,
    /// The rows, at most `maxRows`.
    #[serde(default)]
    pub rows: Vec<Vec<Value>>,
    /// Whether the statement had more rows than were sent.
    #[serde(default)]
    pub truncated: bool,
    /// How many rows the statement changed, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// Whether a transaction is open, and the server's messages.
    #[serde(flatten)]
    pub state: SessionState,
}

/// The answer to `query`: the columns and an open cursor, or the end.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    /// The columns of the rows that `fetch` reads.
    #[serde(default)]
    pub columns: Vec<Column>,
    /// The cursor `fetch` reads; absent when there are no rows to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// How many rows the statement changed, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// Whether a transaction is open, and the server's messages.
    #[serde(flatten)]
    pub state: SessionState,
}

/// The answer to `fetch`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FetchResult {
    /// The rows read, at most the count asked for.
    #[serde(default)]
    pub rows: Vec<Vec<Value>>,
    /// Whether the cursor has no more rows; it is closed then.
    #[serde(default)]
    pub done: bool,
    /// How many rows the statement changed, sent with the last page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// Whether a transaction is open, and the server's messages.
    #[serde(flatten)]
    pub state: SessionState,
}

/// The answer to `children`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChildrenResult {
    /// The children, in the order shown.
    pub nodes: Vec<Node>,
}

/// The answer to `introspect`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IntrospectResult {
    /// The catalogues, with the schemas read again.
    pub catalogs: Vec<Catalog>,
    /// The schemas read again, `catalog.schema`.
    #[serde(default)]
    pub read: Vec<String>,
    /// The schemas whose fingerprints did not change, `catalog.schema`, taken from what the IDE kept.
    #[serde(default)]
    pub unchanged: Vec<String>,
}

pub type Fingerprints = BTreeMap<String, String>;

/// The `data` of an error the driver answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ErrorData {
    /// The kind, as the IDE's error kinds name it (`server`, `authentication`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<ErrorKind>,
    /// The server's own code (an SQLSTATE).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// The one-based character position in the statement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<usize>,
}

/// An error object of JSON-RPC.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RpcError {
    /// The code.
    pub code: i64,
    /// What went wrong.
    pub message: String,
    /// The database error's details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Json>,
}

impl RpcError {
    pub fn to_db_error(&self) -> DbError {
        let data: ErrorData = self.data.clone().and_then(|data| serde_json::from_value(data).ok()).unwrap_or_default();
        let kind = data.kind.unwrap_or(match self.code {
            METHOD_NOT_FOUND => ErrorKind::Unsupported,
            UNKNOWN_SESSION => ErrorKind::NotConnected,
            UNSUPPORTED_PROTOCOL | INVALID_PARAMS | INVALID_REQUEST => ErrorKind::Invalid,
            _ => ErrorKind::Server,
        });
        let mut error = DbError::new(kind, &self.message);
        error.code = data.code;
        error.position = data.position;
        error
    }
}

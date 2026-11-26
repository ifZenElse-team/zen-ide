use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{DbError, ErrorKind, KeyChange, KeyRequest};
use zen_db::engine::OperationStatus;
use zen_db::value::{KeyValue, Value};
use zen_i18n::t;

use super::WORKSPACE;
use super::connections::describe;
use super::consoles::{engine, ensure_connected};
use super::sources::{Database, find};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams};

pub const KEY_TAB: &str = "key";

const ITEMS: usize = 100;
const MOST_ITEMS: usize = 1000;
const MOST_WAIT_MS: u64 = 60_000;
const CELL_CHARS: usize = 1000;

/// Which key to read, and how many of its items.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyParams {
    /// The connection's id.
    pub connection: String,
    /// The key's path in the connection's tree, as db.schema.children gives it: `["db0", "user:", "1"]` for the key `user:1`.
    pub path: Vec<String>,
    /// The most items read (fields, elements, members, entries); 100 when absent, at most 1000.
    pub limit: Option<usize>,
    /// How long to wait for the answer, in milliseconds; 0 when absent, at most 60 000.
    pub wait_ms: Option<u64>,
}

/// A key's time to live to set.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExpireParams {
    /// The connection's id.
    pub connection: String,
    /// The key's path in the connection's tree.
    pub path: Vec<String>,
    /// Seconds until it expires; absent to make it never expire.
    pub seconds: Option<u64>,
    /// How long to wait for the answer, in milliseconds; 0 when absent, at most 60 000.
    pub wait_ms: Option<u64>,
}

/// A key read, or the operation reading it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeyAnswer {
    /// The operation; db.key.state or db.operation.state answers it later.
    pub operation: u64,
    /// Where it is.
    pub status: OperationStatus,
    /// The key, when it was read: type, `ttlMs`, size, length, encoding, columns and rows of its items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<KeyValue>,
    /// Whether long values were cut to 1000 characters.
    pub cells_cut: bool,
    /// Why it failed, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        ErrorKind::Unsupported => "unsupported",
        _ if error.code.as_deref() == Some("readOnly") => "readOnly",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn cut(value: Value, cut_any: &mut bool) -> Value {
    match value {
        Value::Text(text) | Value::Json(text) if text.chars().count() > CELL_CHARS => {
            *cut_any = true;
            Value::Text(text.chars().take(CELL_CHARS).collect())
        }
        Value::Bytes(bytes) if bytes.len() > CELL_CHARS / 2 => {
            *cut_any = true;
            Value::Bytes(bytes[..CELL_CHARS / 2].to_vec())
        }
        Value::Map(fields) => Value::Map(fields.into_iter().map(|(name, item)| (name, cut(item, cut_any))).collect()),
        other => other,
    }
}

pub fn answer(session: &Session, operation: u64, wait_ms: Option<u64>) -> Result<KeyAnswer, CommandError> {
    let engine = session
        .state::<Database>()
        .and_then(Database::started)
        .cloned()
        .ok_or_else(|| CommandError::new(t!("database-error-unknown-operation", id = operation)))?;
    let deadline = Instant::now() + Duration::from_millis(wait_ms.unwrap_or(0).min(MOST_WAIT_MS));
    let state = loop {
        let state = engine
            .operation(operation)
            .ok_or_else(|| CommandError::new(t!("database-error-unknown-operation", id = operation)))?;
        if state.status != OperationStatus::Running || Instant::now() >= deadline {
            break state;
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let mut cells_cut = false;
    let key = state.key.map(|mut key| {
        key.rows =
            key.rows.into_iter().map(|row| row.into_iter().map(|value| cut(value, &mut cells_cut)).collect()).collect();
        key
    });
    Ok(KeyAnswer {
        operation,
        status: state.status,
        key,
        cells_cut,
        message: state.error.as_ref().map(describe),
        error: state.error,
    })
}

fn start(session: &mut Session, connection: &str, path: Vec<String>, request: KeyRequest) -> Result<u64, CommandError> {
    let entry = find(session, connection)?;
    let driver =
        session.state_mut::<Database>().drivers().into_iter().find(|driver| driver.id() == entry.source.driver);
    if !driver.is_some_and(|driver| driver.capabilities().models.contains(&zen_db::model::DataModel::KeyValue)) {
        return Err(CommandError::with_code(
            "unsupported",
            t!("database-error-not-key-value", connection = connection),
        ));
    }
    if request.change.is_some() {
        super::safety::agent_changes(session, &entry)?;
    } else {
        super::safety::agent_reads(session, &entry, "GET")?;
    }
    if path.len() < 2 {
        return Err(CommandError::with_code(
            "unknownObject",
            t!("database-error-unknown-object", path = path.join("/"), connection = connection),
        ));
    }
    ensure_connected(session, connection)?;
    engine(session).key(connection, path, request).map_err(|error| failed(&error))
}

pub struct KeyGet;

impl Command for KeyGet {
    type Params = KeyParams;
    type Output = KeyAnswer;
    const ID: &'static str = "db.key.get";
    const TITLE: &'static str = "Get Database Key";
    const DESCRIPTION: &'static str = "Reads the key at `path` of key-value connection `connection` (Redis, Valkey) in the background: its type (string, hash, list, set, zset, stream, json), `ttlMs` (absent when it never expires), size in memory, length, encoding, and its first `limit` (100, at most 1000) items as columns and rows (field and value, index and value, member and score, id and fields); long values cut to 1000 characters. Connects first when needed with the stored password. Waits up to `waitMs` (60 000 at most); otherwise answers the operation to ask db.key.state about.";

    fn run(&self, session: &mut Session, params: KeyParams) -> Result<KeyAnswer, CommandError> {
        let limit = params.limit.unwrap_or(ITEMS).clamp(1, MOST_ITEMS);
        let operation = start(session, &params.connection, params.path, KeyRequest { limit, change: None })?;
        answer(session, operation, params.wait_ms)
    }
}

pub struct KeyExpire;

impl Command for KeyExpire {
    type Params = ExpireParams;
    type Output = KeyAnswer;
    const ID: &'static str = "db.key.expire";
    const TITLE: &'static str = "Set Database Key Expiry";
    const DESCRIPTION: &'static str = "Makes the key at `path` of key-value connection `connection` expire after `seconds`, or never expire when `seconds` is absent, then reads it back as db.key.get does. A read-only connection refuses it (code `readOnly`). Waits up to `waitMs` (60 000 at most).";

    fn run(&self, session: &mut Session, params: ExpireParams) -> Result<KeyAnswer, CommandError> {
        let change = params.seconds.map_or(KeyChange::Persist, KeyChange::Expire);
        let request = KeyRequest { limit: ITEMS, change: Some(change) };
        let operation = start(session, &params.connection, params.path, request)?;
        answer(session, operation, params.wait_ms)
    }
}

/// Which key operation to answer.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyStateParams {
    /// The operation db.key.get or db.key.expire answered.
    pub operation: u64,
    /// How long to wait for its end, in milliseconds; 0 when absent, at most 60 000.
    pub wait_ms: Option<u64>,
}

pub struct KeyState;

impl Command for KeyState {
    type Params = KeyStateParams;
    type Output = KeyAnswer;
    const ID: &'static str = "db.key.state";
    const TITLE: &'static str = "Database Key State";
    const DESCRIPTION: &'static str = "Answers an operation of db.key.get or db.key.expire, waiting up to `waitMs` for its end: its status and, when done, the key with its type, `ttlMs`, size, length, encoding and items; long values cut to 1000 characters.";

    fn run(&self, session: &mut Session, params: KeyStateParams) -> Result<KeyAnswer, CommandError> {
        answer(session, params.operation, params.wait_ms)
    }
}

/// Which key to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyOpenParams {
    /// The connection's id.
    pub connection: String,
    /// The key's path in the connection's tree.
    pub path: Vec<String>,
}

pub fn tab_id(connection: &str, path: &[String]) -> String {
    format!("key:{connection}:{}", path.join("\u{1f}"))
}

pub struct KeyOpen;

impl Command for KeyOpen {
    type Params = KeyOpenParams;
    type Output = InnerTab;
    const ID: &'static str = "db.key.open";
    const TITLE: &'static str = "Open Database Key";
    const DESCRIPTION: &'static str = "Opens the value of the key at `path` of key-value connection `connection` in a tab of the Database workspace (kind `key`), or brings the open one to front, and expands the workspace; the tab reads the key when it shows (db.key.get). Answers the tab.";

    fn run(&self, session: &mut Session, params: KeyOpenParams) -> Result<InnerTab, CommandError> {
        find(session, &params.connection)?;
        let Some((database, key)) = params.path.split_first().filter(|(_, rest)| !rest.is_empty()) else {
            return Err(CommandError::with_code(
                "unknownObject",
                t!(
                    "database-error-unknown-object",
                    path = params.path.join("/"),
                    connection = params.connection.as_str()
                ),
            ));
        };
        let key = key.concat();
        OpenTab.run(
            session,
            OpenTabParams {
                workspace: WORKSPACE.to_owned(),
                group: None,
                kind: Some(KEY_TAB.to_owned()),
                title: Some(key.clone()),
                id: Some(tab_id(&params.connection, &params.path)),
                closable: Some(true),
                data: Some(serde_json::json!({ "connection": params.connection, "path": params.path, "database": database, "key": key })),
                path: None,
            },
        )
    }
}

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::OperationStatus;
use zen_db::history::Source;
use zen_db::registry::Entry;
use zen_db::sessions::{Ended, OWN_SESSION, ServerSession, ServerSessions, SessionEnd, SessionsRequest};
use zen_db::types::AgentAccess;
use zen_i18n::t;

use super::WORKSPACE;
use super::connections::describe;
use super::consoles::{engine, ensure_connected};
use super::safety::{AGENT_NOT_ALLOWED, AGENT_READ_ONLY, CONFIRM_EXECUTE, READ_ONLY, target, type_of};
use super::sources::{Database, find};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};

pub const KIND: &str = "sessions";

const LISTED: usize = 100;
const MOST_LISTED: usize = 1000;
const TEXT: usize = 500;
const MOST_TEXT: usize = 10_000;
const MOST_WAIT_MS: u64 = 60_000;
const SHOWN_QUERY: usize = 200;

#[derive(Default)]
pub struct LastListed(HashMap<String, ServerSessions>);

impl LastListed {
    pub fn session(&self, connection: &str, id: &str) -> Option<&ServerSession> {
        self.0.get(connection)?.sessions.iter().find(|session| session.id == id)
    }
}

pub fn tab_id(connection: &str) -> String {
    format!("{KIND}:{connection}")
}

fn supports(session: &mut Session, entry: &Entry) -> Result<(), CommandError> {
    let supported = session
        .state_mut::<Database>()
        .drivers()
        .into_iter()
        .find(|driver| driver.id() == entry.source.driver)
        .is_some_and(|driver| driver.capabilities().sessions);
    if supported {
        return Ok(());
    }
    Err(CommandError::with_code("unsupported", t!("database-error-no-sessions", name = entry.source.name.as_str())))
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        ErrorKind::Unsupported => "unsupported",
        ErrorKind::Invalid => "invalidSession",
        ErrorKind::Refused => error.code.as_deref().unwrap_or("refused"),
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn agent_none(session: &Session, entry: &Entry) -> Result<(), CommandError> {
    let kind = type_of(session, entry);
    if zen_core::macros::by_agent(session) && kind.agent_access() == AgentAccess::None {
        return Err(CommandError::with_code(
            AGENT_NOT_ALLOWED,
            t!("database-error-agent-not-allowed", name = entry.source.name.as_str(), kind = kind.name.as_str()),
        ));
    }
    Ok(())
}

/// A listing of sessions, ending a session, or the operation doing it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionsAnswer {
    /// The operation; db.session.state answers it later.
    pub operation: u64,
    /// Where it is.
    pub status: OperationStatus,
    /// The connection's id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The sessions and locks listed, when the listing ended: each session's id, user, host, database, application, command, state, query start, seconds in its state, query (cut, passwords masked), what it waits for, `blockedBy`, `blocking` and `own`; the locks waited for and the locks blocking them; the blocking `chains`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sessions: Option<ServerSessions>,
    /// What ending a session did, when it ended: the statement sent and whether the server found the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended: Option<Ended>,
    /// Why it failed, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<DbError>,
}

pub fn answer(session: &mut Session, operation: u64, wait_ms: Option<u64>) -> Result<SessionsAnswer, CommandError> {
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
    if let (Some(connection), Some(listed)) = (&state.connection, &state.sessions) {
        session.state_mut::<LastListed>().0.insert(connection.clone(), listed.clone());
    }
    Ok(SessionsAnswer {
        operation,
        status: state.status,
        connection: state.connection,
        sessions: state.sessions,
        ended: state.ended,
        message: state.error.as_ref().map(describe),
        error: state.error,
    })
}

/// Which sessions to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// The connection's id.
    pub connection: String,
    /// Whether idle sessions are listed too; true when absent. Sessions idle in a transaction, and sessions blocking others, are always listed.
    pub show_idle: Option<bool>,
    /// Whether the locks waited for and the locks blocking them are read; true when absent.
    pub locks: Option<bool>,
    /// The most sessions answered; 100 when absent, at most 1000.
    pub limit: Option<usize>,
    /// The most characters of each session's query; 500 when absent, at most 10 000.
    pub text_limit: Option<usize>,
    /// How long to wait for the answer, in milliseconds; 0 when absent, at most 60 000.
    pub wait_ms: Option<u64>,
}

pub struct ListSessions;

impl Command for ListSessions {
    type Params = ListParams;
    type Output = SessionsAnswer;
    const ID: &'static str = "db.session.list";
    const TITLE: &'static str = "List Database Sessions";
    const DESCRIPTION: &'static str = "Lists the server's sessions of connection `connection` in the background (PostgreSQL: pg_stat_activity and pg_locks; MySQL and MariaDB: the process list and InnoDB's lock waits): each session's `id`, user, host, database, application, command, state, query start, `seconds` in its state, its query (cut to `textLimit`, 500, passwords masked as in the query history), what it waits for, `blockedBy` and `blocking`, and `own` for this IDE's own sessions; the locks waited for and the locks blocking them, and the blocking `chains`. `showIdle: false` leaves idle sessions out (not those idle in a transaction or blocking others). At most `limit` (100, ≤ 1000) sessions with `total` and `truncated`. Connects first when needed with the stored password; other drivers answer code `unsupported`. Waits up to `waitMs`; otherwise answers the operation to ask db.session.state about.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<SessionsAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        supports(session, &entry)?;
        agent_none(session, &entry)?;
        let request = SessionsRequest {
            show_idle: params.show_idle.unwrap_or(true),
            locks: params.locks.unwrap_or(true),
            limit: params.limit.unwrap_or(LISTED).clamp(1, MOST_LISTED),
            text_limit: params.text_limit.unwrap_or(TEXT).clamp(1, MOST_TEXT),
        };
        ensure_connected(session, &params.connection)?;
        let operation = engine(session).sessions(&params.connection, request).map_err(|error| failed(&error))?;
        answer(session, operation, params.wait_ms)
    }
}

/// Which operation of the Sessions commands to answer.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// The operation db.session.list, db.session.cancelQuery or db.session.terminate answered.
    pub operation: u64,
    /// How long to wait for its end, in milliseconds; 0 when absent, at most 60 000.
    pub wait_ms: Option<u64>,
}

pub struct SessionsState;

impl Command for SessionsState {
    type Params = StateParams;
    type Output = SessionsAnswer;
    const ID: &'static str = "db.session.state";
    const TITLE: &'static str = "Database Sessions State";
    const DESCRIPTION: &'static str = "Answers an operation of db.session.list, db.session.cancelQuery or db.session.terminate, waiting up to `waitMs` for its end: its status and, when done, the sessions and locks listed or what ending a session did.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<SessionsAnswer, CommandError> {
        answer(session, params.operation, params.wait_ms)
    }
}

/// Which session to stop, and the confirmation.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EndParams {
    /// The connection's id.
    pub connection: String,
    /// The server's id of the session, from db.session.list.
    pub session: String,
    /// Whether one of this IDE's own sessions may be the one (`own` in db.session.list); refused with code `ownSession` otherwise.
    pub own: bool,
    /// The `token` of the `confirmExecute` refusal that asked about this session.
    pub confirmed: Option<String>,
    /// Indexes the person skipped in the question; skipping the session stops nothing.
    pub skip: Vec<usize>,
    /// How long to wait for the answer, in milliseconds; 0 when absent, at most 60 000.
    pub wait_ms: Option<u64>,
}

fn token_of(connection: &str, end: SessionEnd, id: &str) -> String {
    let mut hasher = DefaultHasher::new();
    (connection, end, id).hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn who(listed: Option<&ServerSession>) -> String {
    let Some(listed) = listed else { return String::new() };
    let mut parts: Vec<String> = Vec::new();
    if let Some(user) = &listed.user {
        parts.push(match &listed.host {
            Some(host) => format!("{user}@{host}"),
            None => user.clone(),
        });
    }
    parts.extend(listed.application.clone());
    parts.extend(listed.database.clone());
    parts.join(", ")
}

fn ask(
    session: &mut Session,
    entry: &Entry,
    (end, statement): (SessionEnd, &str),
    params: &EndParams,
    retry: &str,
) -> CommandError {
    let listed =
        session.state::<LastListed>().and_then(|last| last.session(&entry.source.id, &params.session)).cloned();
    let name = entry.source.name.as_str();
    let id = params.session.as_str();
    let about = who(listed.as_ref());
    let about = if about.is_empty() { about } else { format!(" ({about})") };
    let message = match end {
        SessionEnd::CancelQuery => t!("database-session-confirm-cancel", id = id, name = name, who = about.as_str()),
        SessionEnd::Terminate => t!("database-session-confirm-terminate", id = id, name = name, who = about.as_str()),
    };
    let mut risks = Vec::new();
    if params.own {
        risks.push(t!("database-session-risk-own", id = id));
    }
    if end == SessionEnd::Terminate {
        risks.push(t!("database-session-risk-terminate", id = id));
    }
    if let Some(query) = listed.as_ref().and_then(|listed| listed.query.as_deref()) {
        let shown: String = query.chars().take(SHOWN_QUERY).collect();
        risks.push(t!("database-session-risk-query", query = shown.as_str()));
    }
    let dangers = if end == SessionEnd::Terminate || params.own {
        serde_json::json!([{ "risk": "endSession", "object": id }])
    } else {
        serde_json::json!([])
    };
    let token = token_of(&entry.source.id, end, id);
    let mut retried = serde_json::json!({
        "connection": entry.source.id,
        "session": id,
        "confirmed": token,
    });
    if params.own {
        retried["own"] = serde_json::json!(true);
    }
    CommandError::with_code(CONFIRM_EXECUTE, message).with_data(serde_json::json!({
        "token": token,
        "target": target(session, entry),
        "statements": [{
            "index": 0,
            "text": statement,
            "cut": false,
            "effect": "other",
            "dangers": dangers,
            "risks": risks,
        }],
        "asked": 1,
        "total": 1,
        "truncated": false,
        "dangerous": end == SessionEnd::Terminate || params.own,
        "session": listed,
        "retry": { "command": retry, "params": retried },
    }))
}

fn end_session(
    session: &mut Session,
    mut params: EndParams,
    end: SessionEnd,
    retry: &str,
) -> Result<SessionsAnswer, CommandError> {
    if let Some(id) = zen_db::sessions::numeric_id(&params.session) {
        params.session = id.to_string();
    }
    let entry = find(session, &params.connection)?;
    supports(session, &entry)?;
    let engine = engine(session);
    let language =
        engine.driver(&entry.source.driver).map(|driver| driver.capabilities().language.clone()).unwrap_or_default();
    let statement = zen_db::sessions::end_statement(&language, &params.session, end).ok_or_else(|| {
        CommandError::with_code("invalidSession", t!("database-error-session-id", id = params.session.as_str()))
    })?;
    let kind = type_of(session, &entry);
    let name = entry.source.name.as_str();
    let agent = zen_core::macros::by_agent(session);
    let mut provenance = super::history::provenance(session, Source::Session, Some(params.session.clone()));
    let refuse = |code: &str, message: String| {
        let error = DbError::new(ErrorKind::Refused, "").with_code(code);
        engine.record_refused(
            &entry.source.id,
            &ContextKind::Metadata,
            std::slice::from_ref(&statement),
            &error,
            &provenance,
        );
        CommandError::with_code(code, message)
    };
    if agent && kind.agent_access() == AgentAccess::None {
        return Err(refuse(
            AGENT_NOT_ALLOWED,
            t!("database-error-agent-not-allowed", name = name, kind = kind.name.as_str()),
        ));
    }
    if super::safety::read_only(session, &entry) {
        return Err(refuse(READ_ONLY, t!("database-error-session-read-only", name = name)));
    }
    if agent && kind.agent_access() == AgentAccess::ReadOnly {
        let hand_off = serde_json::json!({ "handOff": { "command": "db.session.open", "params": { "connection": entry.source.id } } });
        return Err(refuse(
            AGENT_READ_ONLY,
            t!("database-error-agent-session", name = name, kind = kind.name.as_str()),
        )
        .with_data(hand_off));
    }
    let own = engine.own_sessions(&entry.source.id).into_iter().find(|(_, server)| *server == params.session);
    if let Some((context, _)) = &own
        && !params.own
    {
        return Err(CommandError::with_code(
            OWN_SESSION,
            t!("database-error-own-session", id = params.session.as_str(), context = context.to_string()),
        ));
    }
    let token = token_of(&entry.source.id, end, &params.session);
    if params.confirmed.as_deref() != Some(token.as_str()) {
        return Err(ask(session, &entry, (end, &statement), &params, retry));
    }
    if params.skip.contains(&0) {
        return Err(CommandError::with_code(super::safety::SKIPPED, t!("database-error-all-skipped")));
    }
    provenance.confirmed = true;
    let operation = engine
        .end_session(&entry.source.id, &params.session, end, params.own, provenance)
        .map_err(|error| failed(&error))?;
    answer(session, operation, params.wait_ms)
}

pub struct CancelSessionQuery;

impl Command for CancelSessionQuery {
    type Params = EndParams;
    type Output = SessionsAnswer;
    const ID: &'static str = "db.session.cancelQuery";
    const TITLE: &'static str = "Cancel Session Query";
    const DESCRIPTION: &'static str = "Stops the running statement of server session `session` of connection `connection` and keeps the session (PostgreSQL's pg_cancel_backend, MySQL's KILL QUERY). It always asks first: without `confirmed` it is refused with code `confirmExecute`, the session as last listed, the statement it sends and a `token`; run it again with `confirmed: token` (the refusal's `retry`). One of this IDE's own sessions also needs `own: true` (code `ownSession`). Refused on a read-only connection (`readOnly`); agents may not do it where they only read (`agentReadOnly`, with a `handOff` to a person) or run nothing (`agentNotAllowed`). Only that session is touched; the statement goes to the query history. Waits up to `waitMs`.";

    fn run(&self, session: &mut Session, params: EndParams) -> Result<SessionsAnswer, CommandError> {
        end_session(session, params, SessionEnd::CancelQuery, Self::ID)
    }
}

pub struct TerminateSession;

impl Command for TerminateSession {
    type Params = EndParams;
    type Output = SessionsAnswer;
    const ID: &'static str = "db.session.terminate";
    const TITLE: &'static str = "Terminate Session";
    const DESCRIPTION: &'static str = "Ends server session `session` of connection `connection`; its open transaction is rolled back (PostgreSQL's pg_terminate_backend, MySQL's KILL CONNECTION). It always asks first: without `confirmed` it is refused with code `confirmExecute`, the session as last listed, the statement it sends and a `token`; run it again with `confirmed: token` (the refusal's `retry`). One of this IDE's own sessions also needs `own: true` (code `ownSession`). Refused on a read-only connection (`readOnly`); agents may not do it where they only read (`agentReadOnly`, with a `handOff` to a person) or run nothing (`agentNotAllowed`). Only that session is touched; the statement goes to the query history. Waits up to `waitMs`.";

    fn run(&self, session: &mut Session, params: EndParams) -> Result<SessionsAnswer, CommandError> {
        end_session(session, params, SessionEnd::Terminate, Self::ID)
    }
}

/// What the Sessions tab opens with; absent settings stay as they are.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenParams {
    /// The connection's id.
    pub connection: String,
    /// Whether idle sessions are shown; true for a new tab.
    pub show_idle: Option<bool>,
    /// Seconds between refreshes while the tab is shown and the connection connected; 0 (the default) refreshes only on demand.
    pub auto_refresh: Option<u64>,
}

pub struct OpenSessions;

impl Command for OpenSessions {
    type Params = OpenParams;
    type Output = InnerTab;
    const ID: &'static str = "db.session.open";
    const TITLE: &'static str = "Open Sessions";
    const DESCRIPTION: &'static str = "Opens the Sessions tab of connection `connection` in the Database workspace (kind `sessions`), or brings it to front and changes its settings, and expands the workspace: `showIdle` and `autoRefresh` (seconds between refreshes while the tab is shown and the connection is connected; 0 refreshes on demand only). The tab lists the sessions when it shows a connected connection; it never connects by itself. Drivers without sessions answer code `unsupported`. Answers the tab; read what it lists with db.session.list.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<InnerTab, CommandError> {
        let entry = find(session, &params.connection)?;
        supports(session, &entry)?;
        let id = tab_id(&params.connection);
        let existing = session.state::<Workspaces>().and_then(|workspaces| {
            workspaces.tabs(WORKSPACE).iter().find(|tab| tab.id == id).map(|tab| tab.data.clone())
        });
        let mut data = existing
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({ "showIdle": true, "autoRefresh": 0 }));
        data["connection"] = serde_json::json!(params.connection);
        if let Some(show_idle) = params.show_idle {
            data["showIdle"] = serde_json::json!(show_idle);
        }
        if let Some(seconds) = params.auto_refresh {
            data["autoRefresh"] = serde_json::json!(seconds.min(3600));
        }
        OpenTab.run(
            session,
            OpenTabParams {
                workspace: WORKSPACE.to_owned(),
                group: None,
                kind: Some(KIND.to_owned()),
                title: Some(t!("database-sessions-title", name = entry.source.name.as_str())),
                id: Some(id),
                closable: Some(true),
                data: Some(data),
                path: None,
            },
        )
    }
}

#[cfg(test)]
mod tests;

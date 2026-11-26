use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const OWN_SESSION: &str = "ownSession";

/// What a driver lists of the server's sessions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SessionsRequest {
    /// Whether idle sessions are listed too.
    pub show_idle: bool,
    /// Whether the locks sessions wait for, and the locks that block them, are read.
    pub locks: bool,
    /// The most sessions listed.
    pub limit: usize,
    /// The most characters of each session's query.
    pub text_limit: usize,
}

impl Default for SessionsRequest {
    fn default() -> Self {
        Self { show_idle: true, locks: true, limit: 100, text_limit: 500 }
    }
}

/// One session of the server, as the server reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerSession {
    /// The server's id of the session: PostgreSQL's process id, MySQL's connection id.
    pub id: String,
    /// The user it logged in as.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Where the client connects from, `address:port`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The database it is in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// The name the client gave itself (PostgreSQL's `application_name`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    /// What the session does as the server names it (MySQL's `Command`: `Query`, `Sleep`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Its state as the server names it (PostgreSQL's `active`, `idle in transaction`; MySQL's `State`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// When its current or last query started, in the server's time: `2026-10-27 14:05:09`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_start: Option<String>,
    /// Seconds it has been in its current state (MySQL's `Time`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u64>,
    /// Its current or last query, passwords masked, cut to the request's `text_limit` characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// Whether the query was cut.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub query_cut: bool,
    /// Whether a password in the query was masked.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub redacted: bool,
    /// What it waits for, as the server names it (`Lock: transactionid`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<String>,
    /// The sessions that hold the locks it waits for.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub blocked_by: Vec<String>,
    /// The sessions that wait for locks it holds.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub blocking: Vec<String>,
    /// Whether it is one of this IDE's own sessions of the connection.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub own: bool,
    /// The IDE's context it serves when it is one of its own: `main`, `metadata`, `console:<name>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// A lock a session waits for, or holds while another session waits for it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerLock {
    /// The session's id.
    pub session: String,
    /// What is locked, as the server names it (`relation`, `transactionid`, `RECORD`, `TABLE`).
    pub kind: String,
    /// The lock's mode (`RowExclusiveLock`, `X,REC_NOT_GAP`).
    pub mode: String,
    /// Whether it is held; false while the session waits for it.
    pub granted: bool,
    /// The object locked, when it has a name: a table, an index, a transaction's id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    /// The sessions it waits for, when it is not granted.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub blocked_by: Vec<String>,
}

/// The server's sessions and locks, as listed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerSessions {
    /// The sessions, by id.
    pub sessions: Vec<ServerSession>,
    /// How many sessions matched before the limit.
    pub total: usize,
    /// Whether sessions were left out because of the limit.
    pub truncated: bool,
    /// The locks waited for and the locks blocking them, waiting ones first.
    pub locks: Vec<ServerLock>,
    /// Whether locks were left out.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub locks_truncated: bool,
    /// Why locks could not be read (a missing privilege), when they could not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locks_error: Option<String>,
    /// Each blocking chain, from a waiting session to the session at its root: `["4243", "4242", "4241"]`.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub chains: Vec<Vec<String>>,
}

/// How a session is ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SessionEnd {
    /// Stops its running statement and keeps the session (PostgreSQL's `pg_cancel_backend`, MySQL's `KILL QUERY`).
    CancelQuery,
    /// Closes the session; its open transaction is rolled back (`pg_terminate_backend`, `KILL CONNECTION`).
    Terminate,
}

/// What ending a session did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Ended {
    /// The statement sent to the server.
    pub statement: String,
    /// Whether the server found the session and took the request.
    pub ended: bool,
}

pub fn numeric_id(id: &str) -> Option<u64> {
    let id = id.trim();
    (!id.is_empty() && id.len() <= 20 && id.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| id.parse().ok())
        .flatten()
}

pub fn end_statement(language: &str, id: &str, end: SessionEnd) -> Option<String> {
    let id = numeric_id(id)?;
    match (language, end) {
        ("sql:postgres", _) if i32::try_from(id).is_err() => None,
        ("sql:postgres", SessionEnd::CancelQuery) => Some(format!("SELECT pg_cancel_backend({id})")),
        ("sql:postgres", SessionEnd::Terminate) => Some(format!("SELECT pg_terminate_backend({id})")),
        ("sql:mysql" | "sql:mariadb", SessionEnd::CancelQuery) => Some(format!("KILL QUERY {id}")),
        ("sql:mysql" | "sql:mariadb", SessionEnd::Terminate) => Some(format!("KILL CONNECTION {id}")),
        _ => None,
    }
}

pub fn ids(list: Option<&str>) -> Vec<String> {
    list.unwrap_or_default().split(',').map(str::trim).filter(|id| !id.is_empty()).map(str::to_owned).collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn finish(list: &mut ServerSessions, text_limit: usize) {
    use std::collections::{BTreeMap, BTreeSet};
    for session in &mut list.sessions {
        if let Some(query) = session.query.take() {
            let (masked, redacted) = crate::history::redact(&query);
            let cut = masked.chars().count() > text_limit;
            session.query = Some(if cut { masked.chars().take(text_limit).collect() } else { masked });
            session.query_cut |= cut;
            session.redacted |= redacted;
        }
    }
    let mut blocking: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for session in &list.sessions {
        for blocker in &session.blocked_by {
            blocking.entry(blocker.clone()).or_default().insert(session.id.clone());
        }
    }
    for session in &mut list.sessions {
        session.blocking =
            blocking.get(&session.id).map(|waiting| waiting.iter().cloned().collect()).unwrap_or_default();
    }
    let blockers: BTreeMap<&str, &[String]> =
        list.sessions.iter().map(|session| (session.id.as_str(), session.blocked_by.as_slice())).collect();
    let mut chains = Vec::new();
    for session in &list.sessions {
        if session.blocked_by.is_empty() || !session.blocking.is_empty() {
            continue;
        }
        let mut chain = vec![session.id.clone()];
        let mut at = session.blocked_by.first().map(String::as_str);
        while let Some(id) = at {
            if chain.iter().any(|seen| seen == id) {
                chain.push(id.to_owned());
                break;
            }
            chain.push(id.to_owned());
            at = blockers.get(id).and_then(|next| next.first()).map(String::as_str);
        }
        chains.push(chain);
    }
    list.chains = chains;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, blocked_by: &[&str]) -> ServerSession {
        ServerSession {
            id: id.to_owned(),
            blocked_by: blocked_by.iter().map(|id| (*id).to_owned()).collect(),
            ..ServerSession::default()
        }
    }

    #[test]
    fn chains_run_from_each_waiting_session_to_the_root_blocker() {
        let mut list = ServerSessions {
            sessions: vec![
                session("10", &[]),
                session("11", &["10"]),
                session("12", &["11"]),
                session("13", &["10"]),
                session("14", &[]),
            ],
            ..ServerSessions::default()
        };
        finish(&mut list, 100);
        assert_eq!(list.chains, vec![vec!["12", "11", "10"], vec!["13", "10"]]);
        assert_eq!(list.sessions[0].blocking, ["11", "13"]);
        assert_eq!(list.sessions[1].blocking, ["12"]);
        assert!(list.sessions[4].blocking.is_empty());
    }

    #[test]
    fn a_deadlock_cycle_ends_its_chain_where_it_repeats() {
        let mut list = ServerSessions {
            sessions: vec![session("1", &["2"]), session("2", &["1"]), session("3", &["1"])],
            ..ServerSessions::default()
        };
        finish(&mut list, 100);
        assert_eq!(list.chains, vec![vec!["3", "1", "2", "1"]]);
    }

    #[test]
    fn queries_are_cut_and_their_passwords_masked() {
        let query = |id: &str, text: &str| ServerSession {
            id: id.to_owned(),
            query: Some(text.to_owned()),
            ..ServerSession::default()
        };
        let mut list = ServerSessions {
            sessions: vec![
                query("1", "ALTER USER ann PASSWORD 's3cret'"),
                query("2", "CREATE ROLE bob LOGIN PASSWORD 'half-sent"),
                query("3", "SELECT 1"),
            ],
            ..ServerSessions::default()
        };
        finish(&mut list, 100);
        for (session, secret) in list.sessions.iter().zip(["s3cret", "half-sent"]) {
            assert!(!session.query.as_deref().unwrap().contains(secret), "{session:?}");
            assert!(session.redacted && !session.query_cut, "{session:?}");
        }
        assert!(!list.sessions[2].redacted);
        let mut list = ServerSessions { sessions: vec![query("4", "SELECT ééééé")], ..ServerSessions::default() };
        finish(&mut list, 9);
        assert_eq!(list.sessions[0].query.as_deref(), Some("SELECT éé"));
        assert!(list.sessions[0].query_cut);
    }

    #[test]
    fn only_plain_numbers_are_session_ids() {
        assert_eq!(numeric_id(" 4242 "), Some(4242));
        assert_eq!(numeric_id("1; DROP TABLE t"), None);
        assert_eq!(numeric_id("-1"), None);
        assert_eq!(numeric_id(""), None);
        assert_eq!(ids(Some("4241, 4242,")), ["4241", "4242"]);
        assert_eq!(
            end_statement("sql:postgres", "12", SessionEnd::Terminate).unwrap(),
            "SELECT pg_terminate_backend(12)"
        );
        assert_eq!(end_statement("sql:mariadb", "12", SessionEnd::CancelQuery).unwrap(), "KILL QUERY 12");
        assert_eq!(end_statement("sql:postgres", "99999999999", SessionEnd::CancelQuery), None);
        assert_eq!(end_statement("redis", "12", SessionEnd::CancelQuery), None);
        assert!(ids(None).is_empty());
    }
}

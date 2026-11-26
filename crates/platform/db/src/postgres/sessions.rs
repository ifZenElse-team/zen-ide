use tokio_postgres::{Client, SimpleQueryMessage, SimpleQueryRow};

use super::error;
use crate::driver::{DbError, ErrorKind};
use crate::sessions::{
    Ended, ServerLock, ServerSession, ServerSessions, SessionEnd, SessionsRequest, end_statement, ids,
};

const MOST_LOCKS: usize = 500;

fn sessions_sql(request: &SessionsRequest) -> String {
    let idle = if request.show_idle { "" } else { " AND a.state IS DISTINCT FROM 'idle'" };
    let text = request.text_limit.saturating_add(1).min(1_000_000);
    let limit = request.limit.max(1);
    format!(
        "SELECT a.pid::text, a.usename::text, a.datname::text, a.application_name, \
         CASE WHEN a.client_addr IS NULL THEN NULL ELSE host(a.client_addr) || ':' || a.client_port END, \
         a.backend_type, a.state, to_char(a.query_start, 'YYYY-MM-DD HH24:MI:SS'), \
         floor(extract(epoch FROM clock_timestamp() - coalesce(a.state_change, a.backend_start)))::bigint::text, \
         left(a.query, {text}), \
         CASE WHEN a.wait_event IS NULL THEN NULL ELSE a.wait_event_type || ': ' || a.wait_event END, \
         array_to_string(pg_blocking_pids(a.pid), ','), count(*) OVER ()::text \
         FROM pg_stat_activity a \
         WHERE a.backend_type IN ('client backend', 'autovacuum worker'){idle} \
         ORDER BY a.pid LIMIT {limit}"
    )
}

fn locks_sql() -> String {
    format!(
        "WITH w AS (SELECT * FROM pg_locks WHERE NOT granted AND pid IS NOT NULL) \
         SELECT l.pid::text, l.locktype, l.mode, l.granted::text, \
         CASE WHEN l.relation IS NOT NULL THEN l.relation::regclass::text \
         WHEN l.transactionid IS NOT NULL THEN l.transactionid::text \
         WHEN l.virtualxid IS NOT NULL THEN l.virtualxid \
         WHEN l.objid IS NOT NULL THEN l.classid::regclass::text || ' ' || l.objid END, \
         CASE WHEN l.granted THEN '' ELSE array_to_string(pg_blocking_pids(l.pid), ',') END \
         FROM pg_locks l \
         WHERE l.pid IS NOT NULL AND (NOT l.granted OR EXISTS (SELECT 1 FROM w \
         WHERE l.pid = ANY(pg_blocking_pids(w.pid)) AND l.locktype = w.locktype \
         AND l.database IS NOT DISTINCT FROM w.database AND l.relation IS NOT DISTINCT FROM w.relation \
         AND l.page IS NOT DISTINCT FROM w.page AND l.tuple IS NOT DISTINCT FROM w.tuple \
         AND l.virtualxid IS NOT DISTINCT FROM w.virtualxid AND l.transactionid IS NOT DISTINCT FROM w.transactionid \
         AND l.classid IS NOT DISTINCT FROM w.classid AND l.objid IS NOT DISTINCT FROM w.objid \
         AND l.objsubid IS NOT DISTINCT FROM w.objsubid)) \
         ORDER BY l.granted, l.pid LIMIT {}",
        MOST_LOCKS + 1
    )
}

fn rows(messages: Vec<SimpleQueryMessage>) -> Vec<SimpleQueryRow> {
    messages
        .into_iter()
        .filter_map(|message| match message {
            SimpleQueryMessage::Row(row) => Some(row),
            _ => None,
        })
        .collect()
}

fn text(row: &SimpleQueryRow, index: usize) -> Option<String> {
    row.get(index).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn session(row: &SimpleQueryRow) -> ServerSession {
    ServerSession {
        id: row.get(0).unwrap_or_default().to_owned(),
        user: text(row, 1),
        database: text(row, 2),
        application: text(row, 3),
        host: text(row, 4),
        command: text(row, 5).filter(|kind| kind != "client backend"),
        state: text(row, 6),
        query_start: text(row, 7),
        seconds: row.get(8).and_then(|seconds| seconds.parse().ok()),
        query: text(row, 9),
        wait: text(row, 10),
        blocked_by: ids(row.get(11)),
        ..ServerSession::default()
    }
}

fn lock(row: &SimpleQueryRow) -> ServerLock {
    ServerLock {
        session: row.get(0).unwrap_or_default().to_owned(),
        kind: row.get(1).unwrap_or_default().to_owned(),
        mode: row.get(2).unwrap_or_default().to_owned(),
        granted: row.get(3) == Some("true"),
        object: text(row, 4),
        blocked_by: ids(row.get(5)),
    }
}

pub(super) async fn list(client: &Client, request: &SessionsRequest) -> Result<ServerSessions, DbError> {
    let found = rows(client.simple_query(&sessions_sql(request)).await.map_err(|failed| error(&failed))?);
    let total = found.first().and_then(|row| row.get(12)).and_then(|total| total.parse().ok()).unwrap_or(0);
    let sessions: Vec<ServerSession> = found.iter().map(session).collect();
    let mut listed = ServerSessions {
        truncated: total > sessions.len(),
        total: total.max(sessions.len()),
        sessions,
        ..ServerSessions::default()
    };
    if request.locks {
        match client.simple_query(&locks_sql()).await {
            Ok(messages) => {
                let mut locks: Vec<ServerLock> = rows(messages).iter().map(lock).collect();
                listed.locks_truncated = locks.len() > MOST_LOCKS;
                locks.truncate(MOST_LOCKS);
                listed.locks = locks;
            }
            Err(failed) => listed.locks_error = Some(error(&failed).detail),
        }
    }
    Ok(listed)
}

pub(super) async fn end(client: &Client, id: &str, end: SessionEnd) -> Result<Ended, DbError> {
    let statement = end_statement(super::capabilities().language.as_str(), id, end)
        .ok_or_else(|| DbError::new(ErrorKind::Invalid, format!("not a process id: {id}")))?;
    let answered = rows(client.simple_query(&statement).await.map_err(|failed| error(&failed))?);
    let ended = answered.first().and_then(|row| row.get(0)) == Some("t");
    Ok(Ended { statement, ended })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_sessions_are_left_out_only_when_asked() {
        let all = sessions_sql(&SessionsRequest::default());
        assert!(!all.contains("'idle'"), "{all}");
        assert!(all.ends_with("LIMIT 100"), "{all}");
        let busy = sessions_sql(&SessionsRequest { show_idle: false, limit: 7, ..SessionsRequest::default() });
        assert!(busy.contains("a.state IS DISTINCT FROM 'idle'") && busy.ends_with("LIMIT 7"), "{busy}");
        assert!(busy.contains("left(a.query, 501)"), "{busy}");
    }
}

use mysql_async::Conn;
use mysql_async::prelude::Queryable;

use super::{error, sets};
use crate::driver::{DbError, ErrorKind};
use crate::sessions::{Ended, ServerLock, ServerSession, ServerSessions, SessionEnd, SessionsRequest, end_statement};

const MOST_LOCKS: usize = 500;

const UNKNOWN_THREAD: u16 = 1094;

fn sessions_sql(request: &SessionsRequest, blockers: &[u64]) -> String {
    let idle = match (request.show_idle, blockers) {
        (true, _) => String::new(),
        (false, []) => " WHERE COMMAND <> 'Sleep'".to_owned(),
        (false, ids) => {
            let ids: Vec<String> = ids.iter().map(u64::to_string).collect();
            format!(" WHERE (COMMAND <> 'Sleep' OR ID IN ({}))", ids.join(", "))
        }
    };
    let text = request.text_limit.saturating_add(1).min(1_000_000);
    format!(
        "SELECT ID, USER, HOST, DB, COMMAND, TIME, STATE, LEFT(INFO, {text}), COUNT(*) OVER () \
         FROM information_schema.PROCESSLIST{idle} ORDER BY ID LIMIT {}",
        request.limit.max(1)
    )
}

fn waits_sql(mariadb: bool) -> &'static str {
    if mariadb {
        "SELECT r.trx_mysql_thread_id, b.trx_mysql_thread_id, w.requested_lock_id \
         FROM information_schema.INNODB_LOCK_WAITS w \
         JOIN information_schema.INNODB_TRX r ON r.trx_id = w.requesting_trx_id \
         JOIN information_schema.INNODB_TRX b ON b.trx_id = w.blocking_trx_id"
    } else {
        "SELECT rt.PROCESSLIST_ID, bt.PROCESSLIST_ID, w.REQUESTING_ENGINE_LOCK_ID \
         FROM performance_schema.data_lock_waits w \
         JOIN performance_schema.threads rt ON rt.THREAD_ID = w.REQUESTING_THREAD_ID \
         JOIN performance_schema.threads bt ON bt.THREAD_ID = w.BLOCKING_THREAD_ID"
    }
}

fn locks_sql(mariadb: bool) -> String {
    if mariadb {
        format!(
            "SELECT t.trx_mysql_thread_id, l.lock_type, l.lock_mode, \
             IF(l.lock_id IN (SELECT requested_lock_id FROM information_schema.INNODB_LOCK_WAITS), 'WAITING', 'GRANTED'), \
             CONCAT_WS(' ', l.lock_table, l.lock_index), l.lock_id \
             FROM information_schema.INNODB_LOCKS l JOIN information_schema.INNODB_TRX t ON t.trx_id = l.lock_trx_id \
             ORDER BY 4 DESC, 1 LIMIT {}",
            MOST_LOCKS + 1
        )
    } else {
        format!(
            "SELECT t.PROCESSLIST_ID, l.LOCK_TYPE, l.LOCK_MODE, l.LOCK_STATUS, \
             CONCAT_WS(' ', CONCAT_WS('.', l.OBJECT_SCHEMA, l.OBJECT_NAME), l.INDEX_NAME), l.ENGINE_LOCK_ID \
             FROM performance_schema.data_locks l JOIN performance_schema.threads t ON t.THREAD_ID = l.THREAD_ID \
             WHERE l.LOCK_STATUS = 'WAITING' \
             OR l.ENGINE_LOCK_ID IN (SELECT BLOCKING_ENGINE_LOCK_ID FROM performance_schema.data_lock_waits) \
             ORDER BY l.LOCK_STATUS DESC, t.PROCESSLIST_ID LIMIT {}",
            MOST_LOCKS + 1
        )
    }
}

fn cell(row: &[Option<String>], index: usize) -> Option<String> {
    row.get(index).cloned().flatten().filter(|value| !value.is_empty())
}

fn session(row: &[Option<String>]) -> ServerSession {
    ServerSession {
        id: cell(row, 0).unwrap_or_default(),
        user: cell(row, 1),
        host: cell(row, 2),
        database: cell(row, 3),
        command: cell(row, 4),
        seconds: cell(row, 5).and_then(|seconds| seconds.parse().ok()),
        state: cell(row, 6),
        query: cell(row, 7),
        ..ServerSession::default()
    }
}

pub(super) async fn list(conn: &mut Conn, mariadb: bool, request: &SessionsRequest) -> Result<ServerSessions, DbError> {
    let mut listed = ServerSessions::default();
    let waits = match sets(conn, &[waits_sql(mariadb).to_owned()]).await {
        Ok(read) => read.into_iter().next().unwrap_or_default(),
        Err(failed) => {
            listed.locks_error = Some(failed.detail);
            Vec::new()
        }
    };
    let mut blockers: Vec<u64> = waits.iter().filter_map(|wait| cell(wait, 1)?.parse().ok()).collect();
    blockers.sort_unstable();
    blockers.dedup();
    let found = sets(conn, &[sessions_sql(request, &blockers)]).await?.into_iter().next().unwrap_or_default();
    let total = found.first().and_then(|row| cell(row, 8)).and_then(|total| total.parse().ok()).unwrap_or(0);
    let mut sessions: Vec<ServerSession> = found.iter().map(|row| session(row)).collect();
    listed.truncated = total > sessions.len();
    listed.total = total.max(sessions.len());
    for wait in &waits {
        let (Some(waiting), Some(blocker)) = (cell(wait, 0), cell(wait, 1)) else { continue };
        if let Some(session) = sessions.iter_mut().find(|session| session.id == waiting)
            && !session.blocked_by.contains(&blocker)
        {
            session.blocked_by.push(blocker);
        }
    }
    listed.sessions = sessions;
    if request.locks && listed.locks_error.is_none() {
        match sets(conn, &[locks_sql(mariadb)]).await {
            Ok(read) => {
                let mut locks: Vec<ServerLock> = read
                    .into_iter()
                    .next()
                    .unwrap_or_default()
                    .iter()
                    .map(|row| {
                        let granted = cell(row, 3).as_deref() != Some("WAITING");
                        let id = cell(row, 5);
                        let mut blocked_by: Vec<String> = Vec::new();
                        if !granted {
                            for wait in waits.iter().filter(|wait| cell(wait, 2) == id) {
                                if let Some(blocker) = cell(wait, 1).filter(|blocker| !blocked_by.contains(blocker)) {
                                    blocked_by.push(blocker);
                                }
                            }
                        }
                        ServerLock {
                            session: cell(row, 0).unwrap_or_default(),
                            kind: cell(row, 1).unwrap_or_default(),
                            mode: cell(row, 2).unwrap_or_default(),
                            granted,
                            object: cell(row, 4),
                            blocked_by,
                        }
                    })
                    .collect();
                listed.locks_truncated = locks.len() > MOST_LOCKS;
                locks.truncate(MOST_LOCKS);
                listed.locks = locks;
            }
            Err(failed) => listed.locks_error = Some(failed.detail),
        }
    }
    Ok(listed)
}

pub(super) async fn end(conn: &mut Conn, id: &str, end: SessionEnd) -> Result<Ended, DbError> {
    let statement = end_statement("sql:mysql", id, end)
        .ok_or_else(|| DbError::new(ErrorKind::Invalid, format!("not a connection id: {id}")))?;
    match conn.query_drop(statement.as_str()).await {
        Ok(()) => Ok(Ended { statement, ended: true }),
        Err(mysql_async::Error::Server(server)) if server.code == UNKNOWN_THREAD => {
            Ok(Ended { statement, ended: false })
        }
        Err(failed) => Err(error(&failed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sleeping_connections_are_left_out_only_when_asked() {
        let all = sessions_sql(&SessionsRequest::default(), &[7]);
        assert!(!all.contains("Sleep") && all.ends_with("LIMIT 100"), "{all}");
        let request = SessionsRequest { show_idle: false, text_limit: 9, ..SessionsRequest::default() };
        let busy = sessions_sql(&request, &[]);
        assert!(busy.contains("WHERE COMMAND <> 'Sleep'") && busy.contains("LEFT(INFO, 10)"), "{busy}");
        let blocking = sessions_sql(&request, &[7, 9]);
        assert!(blocking.contains("WHERE (COMMAND <> 'Sleep' OR ID IN (7, 9))"), "{blocking}");
        assert!(waits_sql(true).contains("INNODB_LOCK_WAITS") && waits_sql(false).contains("data_lock_waits"));
    }
}

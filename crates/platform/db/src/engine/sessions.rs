use std::sync::Arc;
use std::time::{Instant, SystemTime};

use super::{Engine, OperationKind, Outcome};
use crate::driver::{ContextKind, DbError, Driver, ErrorKind};
use crate::history::{Provenance, Ran};
use crate::sessions::{ServerSessions, SessionEnd, SessionsRequest};

impl Engine {
    fn sessions_driver(&self, id: &str) -> Result<Arc<dyn Driver>, DbError> {
        let driver = self
            .state()
            .connections
            .get(id)
            .map(|conn| Arc::clone(&conn.driver))
            .ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
        if !driver.capabilities().sessions {
            return Err(DbError::new(ErrorKind::Unsupported, "sessions"));
        }
        Ok(driver)
    }

    pub fn own_sessions(&self, id: &str) -> Vec<(ContextKind, String)> {
        self.state()
            .connections
            .get(id)
            .map(|conn| {
                conn.info
                    .iter()
                    .filter_map(|(context, info)| Some((context.clone(), info.server_id.clone()?)))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn mark_own(&self, id: &str, listed: &mut ServerSessions) {
        let own = self.own_sessions(id);
        for session in &mut listed.sessions {
            if let Some((context, _)) = own.iter().find(|(_, server)| *server == session.id) {
                session.own = true;
                session.context = Some(context.to_string());
            }
        }
    }

    pub fn sessions(&self, id: &str, request: SessionsRequest) -> Result<u64, DbError> {
        self.sessions_driver(id)?;
        let (op, token) = self.begin(OperationKind::Sessions, Some(id), Some(ContextKind::Metadata));
        let (engine, id) = (self.clone(), id.to_owned());
        self.spawn(op, async move {
            let (kind, cell) = engine.connected_cell(&id, &ContextKind::Metadata, &token).await?;
            engine.busy(&id, &kind, 1);
            let result = async {
                let mut ctx = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                engine.ready(&id, &kind, &mut ctx, op).await?;
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                let read = tokio::select! {
                    read = session.sessions(&request) => read,
                    () = token.cancelled() => Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                ctx.last_used = Instant::now();
                engine.update_info(&id, &kind, &ctx);
                read
            }
            .await;
            engine.busy(&id, &kind, -1);
            let mut listed = result?;
            crate::sessions::finish(&mut listed, request.text_limit);
            engine.mark_own(&id, &mut listed);
            Ok(Outcome::Sessions(Box::new(listed)))
        });
        Ok(op)
    }

    pub fn end_session(
        &self,
        id: &str,
        target: &str,
        end: SessionEnd,
        own: bool,
        provenance: Provenance,
    ) -> Result<u64, DbError> {
        let driver = self.sessions_driver(id)?;
        let text = crate::sessions::end_statement(&driver.capabilities().language, target, end)
            .ok_or_else(|| DbError::new(ErrorKind::Invalid, target))?;
        if self.read_only(id) {
            return Err(DbError::new(ErrorKind::Refused, "").with_code("readOnly"));
        }
        let number = crate::sessions::numeric_id(target);
        if !own && self.own_sessions(id).iter().any(|(_, server)| crate::sessions::numeric_id(server) == number) {
            return Err(DbError::new(ErrorKind::Refused, target).with_code(crate::sessions::OWN_SESSION));
        }
        let (op, token) = self.begin(OperationKind::EndSession, Some(id), Some(ContextKind::Metadata));
        let (engine, id, target) = (self.clone(), id.to_owned(), target.to_owned());
        self.spawn(op, async move {
            let started = SystemTime::now();
            let clock = Instant::now();
            let (kind, cell) = engine.connected_cell(&id, &ContextKind::Metadata, &token).await?;
            engine.busy(&id, &kind, 1);
            let result = async {
                let mut ctx = tokio::select! {
                    ctx = cell.lock_owned() => ctx,
                    () = token.cancelled() => return Err(DbError::new(ErrorKind::Cancelled, "")),
                };
                engine.ready(&id, &kind, &mut ctx, op).await?;
                let session = ctx.session.as_mut().ok_or_else(|| DbError::new(ErrorKind::NotConnected, ""))?;
                let ended = session.end_session(&target, end).await;
                ctx.last_used = Instant::now();
                engine.update_info(&id, &kind, &ctx);
                ended
            }
            .await;
            engine.busy(&id, &kind, -1);
            engine.record(Ran {
                connection: id.clone(),
                context: kind.clone(),
                schema: None,
                text: result.as_ref().map_or(text, |ended| ended.statement.clone()),
                started,
                duration: clock.elapsed(),
                rows: None,
                affected: None,
                error: result.as_ref().err().cloned(),
                provenance,
                params: Vec::new(),
            });
            result.map(Outcome::Ended)
        });
        Ok(op)
    }
}

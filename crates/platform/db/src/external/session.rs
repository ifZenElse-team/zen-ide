use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use zeroize::Zeroize;

use super::Inner;
use super::process::Process;
use super::protocol::{
    ChildrenResult, ConnectParams, ConnectResult, ExecuteResult, FetchResult, Fingerprints, IntrospectResult,
    QueryResult, SessionState,
};
use crate::driver::{
    BoxFuture, Canceller, ConnectSpec, DbError, DriverSession, ErrorKind, ExecOptions, KeyRequest, Param, RowSink,
    StreamEnd, StreamOptions, TrackedState,
};
use crate::model::{Capabilities, Node};
use crate::schema::{IntrospectRequest, Introspection, Snapshot};
use crate::value::{Execution, KeyValue, Notice};

const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_GRACE: Duration = Duration::from_secs(2);
const DEADLINE_GRACE: Duration = Duration::from_secs(5);
const LARGEST_FETCH: usize = 10_000;
const BOUND_ROWS: usize = 1_000_000_000;

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(crate) struct Session {
    driver: Arc<Inner>,
    process: Arc<Process>,
    capabilities: Capabilities,
    id: String,
    server_version: Option<String>,
    in_transaction: bool,
    closed: bool,
    notices: Vec<Notice>,
}

struct Cancel {
    process: Arc<Process>,
    session: String,
}

impl Canceller for Cancel {
    fn cancel(&self) -> BoxFuture<'static, Result<(), DbError>> {
        let process = Arc::clone(&self.process);
        let session = self.session.clone();
        Box::pin(async move {
            let _: serde_json::Value =
                process.request("cancel", &json!({ "session": session }), Some(CONTROL_TIMEOUT)).await?;
            Ok(())
        })
    }
}

impl Session {
    pub(crate) async fn open(
        driver: Arc<Inner>,
        process: Arc<Process>,
        capabilities: Capabilities,
        spec: &ConnectSpec,
    ) -> Result<Self, DbError> {
        if let Some(password) = &spec.password {
            process.remember(password);
        }
        let source = &spec.source;
        let (host, port) = match spec.via {
            Some(via) => (Some(via.ip().to_string()), Some(via.port())),
            None => (source.host.clone(), source.port.or(driver.spec.declaration.default_port)),
        };
        let mut params = ConnectParams {
            connection: source.id.clone(),
            context: spec.context.to_string(),
            host,
            port,
            database: source.database.clone(),
            user: source.user.clone(),
            password: spec.password.as_ref().map(|password| password.expose().to_owned()),
            url: source.url.clone(),
            read_only: spec.state.read_only || source.read_only,
            tls: serde_json::to_value(&source.tls).unwrap_or_default(),
            settings: source.settings.clone(),
            application_name: spec.guards.application_name.clone(),
            statement_timeout_ms: spec.guards.statement_timeout.map(millis),
            state: spec.state.clone(),
            timeout_ms: millis(spec.timeout),
        };
        let answer: Result<ConnectResult, DbError> =
            process.request("connect", &params, Some(spec.timeout + CONNECT_GRACE)).await;
        if let Some(password) = params.password.as_mut() {
            password.zeroize();
        }
        let answer = match answer {
            Ok(answer) => answer,
            Err(error) => {
                driver.log.push(format!("connecting {} failed: {}", source.id, process.redact(&error.detail)));
                return Err(error);
            }
        };
        Ok(Self {
            driver,
            process,
            capabilities,
            id: answer.session,
            server_version: answer.server_version,
            in_transaction: answer.in_transaction,
            closed: false,
            notices: Vec::new(),
        })
    }

    fn track(&mut self, state: &SessionState) {
        if let Some(open) = state.in_transaction {
            self.in_transaction = open;
        }
    }

    fn failed(&mut self, error: DbError) -> DbError {
        if !self.process.alive() {
            self.closed = true;
        }
        if error.kind == ErrorKind::NotConnected {
            self.closed = true;
        }
        error
    }

    async fn call<T: serde::de::DeserializeOwned>(
        &mut self,
        method: &str,
        params: serde_json::Value,
        deadline: Option<Duration>,
    ) -> Result<T, DbError> {
        if self.closed {
            return Err(DbError::new(ErrorKind::NotConnected, ""));
        }
        match self.process.request(method, &params, deadline).await {
            Ok(answer) => Ok(answer),
            Err(error) => Err(self.failed(error)),
        }
    }

    async fn run(
        &mut self,
        statement: &str,
        options: &ExecOptions,
        params: Option<&[Param]>,
    ) -> Result<Execution, DbError> {
        let mut request = json!({
            "session": self.id,
            "statement": statement,
            "maxRows": options.max_rows,
        });
        if let Some(timeout) = options.timeout {
            request["timeoutMs"] = json!(millis(timeout));
        }
        if let Some(params) = params {
            request["params"] = json!(params);
        }
        let deadline = options.timeout.map(|timeout| timeout + DEADLINE_GRACE);
        let answer: Result<ExecuteResult, DbError> = self.call("execute", request, deadline).await;
        let answer = match answer {
            Err(error) if error.kind == ErrorKind::Timeout && !self.closed => {
                let _ = Cancel { process: Arc::clone(&self.process), session: self.id.clone() }.cancel().await;
                return Err(error);
            }
            other => other?,
        };
        self.track(&answer.state);
        let (rows, cut) = checked_rows(answer.columns.len(), answer.rows, Some(options.max_rows))?;
        Ok(Execution {
            columns: answer.columns,
            rows,
            truncated: answer.truncated || cut,
            affected: answer.affected,
            notices: answer.state.notices,
        })
    }

    async fn paged(
        &mut self,
        statement: &str,
        options: &StreamOptions,
        sink: &mut dyn RowSink,
    ) -> Result<StreamEnd, DbError> {
        let opened: QueryResult = self
            .call("query", json!({ "session": self.id, "statement": statement, "skip": options.skip }), None)
            .await?;
        self.track(&opened.state);
        let mut notices = opened.state.notices;
        let mut affected = opened.affected;
        let has_columns = !opened.columns.is_empty();
        let width = opened.columns.len();
        sink.columns(opened.columns);
        let Some(cursor) = opened.cursor else {
            return Ok(StreamEnd { affected, complete: true, notices });
        };
        if has_columns {
            sink.portal(self.in_transaction);
        }
        let page = options.page.clamp(1, LARGEST_FETCH);
        let mut done = false;
        let mut kept = true;
        while !done {
            let wanted = sink.want().await;
            if wanted == 0 {
                break;
            }
            let asked = wanted.min(page);
            let mut fetched: FetchResult =
                self.call("fetch", json!({ "session": self.id, "cursor": cursor, "count": asked }), None).await?;
            fetched.rows = checked_rows(width, std::mem::take(&mut fetched.rows), Some(asked))?.0;
            self.track(&fetched.state);
            notices.extend(fetched.state.notices);
            if fetched.affected.is_some() {
                affected = fetched.affected;
            }
            done = fetched.done;
            if !fetched.rows.is_empty() {
                kept = sink.push(fetched.rows);
                if !kept {
                    break;
                }
            }
        }
        if !done {
            let _: Result<serde_json::Value, DbError> =
                self.call("closeCursor", json!({ "session": self.id, "cursor": cursor }), Some(CONTROL_TIMEOUT)).await;
        }
        Ok(StreamEnd { affected, complete: done && kept, notices })
    }

    async fn all_at_once(
        &mut self,
        statement: &str,
        options: &StreamOptions,
        sink: &mut dyn RowSink,
    ) -> Result<StreamEnd, DbError> {
        let skip = usize::try_from(options.skip).unwrap_or(usize::MAX);
        let max_rows = skip.saturating_add(sink.cap().max(1));
        let execution = self.run(statement, &ExecOptions { max_rows, timeout: None }, None).await?;
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
        Ok(StreamEnd {
            affected: execution.affected,
            complete: kept && !execution.truncated,
            notices: execution.notices,
        })
    }

    async fn read_structure(
        &mut self,
        request: &IntrospectRequest,
        previous: Option<&Snapshot>,
    ) -> Result<Introspection, DbError> {
        let mut fingerprints = Fingerprints::new();
        for catalog in previous.map(|snapshot| snapshot.catalogs.as_slice()).unwrap_or_default() {
            for schema in &catalog.schemas {
                if let Some(fingerprint) = &schema.fingerprint {
                    fingerprints.insert(format!("{}.{}", catalog.name, schema.name), fingerprint.clone());
                }
            }
        }
        let answer: IntrospectResult = self
            .call(
                "introspect",
                json!({
                    "session": self.id,
                    "schemas": request.schemas,
                    "objects": request.objects,
                    "force": request.force,
                    "system": request.system,
                    "largeSchema": request.large_schema,
                    "outline": request.outline,
                    "fingerprints": fingerprints,
                }),
                None,
            )
            .await?;
        let mut catalogs = answer.catalogs;
        let mut unchanged = 0;
        for name in &answer.unchanged {
            let Some((catalog, schema)) = name.split_once('.') else { continue };
            let Some(kept) = previous.and_then(|snapshot| snapshot.schema(catalog, schema)) else { continue };
            let Some(target) = catalogs.iter_mut().find(|known| known.name == catalog) else { continue };
            match target.schemas.iter_mut().find(|known| known.name == schema) {
                Some(slot) => *slot = kept.clone(),
                None => target.schemas.push(kept.clone()),
            }
            unchanged += 1;
        }
        Ok(Introspection { catalogs, read: answer.read, unchanged })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.process.alive() {
            self.process.notify("close", &json!({ "session": self.id }));
        }
        self.driver.release(&self.process);
    }
}

impl DriverSession for Session {
    fn execute<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a ExecOptions,
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(self.run(statement, options, None))
    }

    fn stream<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a StreamOptions,
        sink: &'a mut dyn RowSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        Box::pin(async move {
            if self.capabilities.server_paging {
                self.paged(statement, options, sink).await
            } else {
                self.all_at_once(statement, options, sink).await
            }
        })
    }

    fn execute_bound<'a>(
        &'a mut self,
        statement: &'a str,
        params: &'a [Param],
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(async move {
            if !self.capabilities.bind_parameters && !params.is_empty() {
                return Err(DbError::new(ErrorKind::Unsupported, "bound parameters"));
            }
            let params = Some(params).filter(|params| !params.is_empty());
            self.run(statement, &ExecOptions { max_rows: BOUND_ROWS, timeout: None }, params).await
        })
    }

    fn children<'a>(&'a mut self, path: &'a [String]) -> BoxFuture<'a, Result<Vec<Node>, DbError>> {
        Box::pin(async move {
            let answer: ChildrenResult =
                self.call("children", json!({ "session": self.id, "path": path }), None).await?;
            Ok(answer.nodes)
        })
    }

    fn introspect<'a>(
        &'a mut self,
        request: &'a IntrospectRequest,
        previous: Option<&'a Snapshot>,
    ) -> BoxFuture<'a, Result<Introspection, DbError>> {
        Box::pin(async move {
            if !self.capabilities.introspection {
                return Err(DbError::new(ErrorKind::Unsupported, "introspect"));
            }
            self.read_structure(request, previous).await
        })
    }

    fn key<'a>(&'a mut self, path: &'a [String], request: &'a KeyRequest) -> BoxFuture<'a, Result<KeyValue, DbError>> {
        Box::pin(async move {
            self.call(
                "key",
                json!({ "session": self.id, "path": path, "limit": request.limit, "change": request.change }),
                None,
            )
            .await
        })
    }

    fn ping(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            let _: serde_json::Value = self.call("ping", json!({ "session": self.id }), Some(CONTROL_TIMEOUT)).await?;
            Ok(())
        })
    }

    fn server_version(&mut self) -> BoxFuture<'_, Result<Option<String>, DbError>> {
        let version = self.server_version.clone();
        Box::pin(async move { Ok(version) })
    }

    fn apply<'a>(&'a mut self, state: &'a TrackedState) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            let answer: SessionState =
                self.call("apply", json!({ "session": self.id, "state": state }), Some(CONTROL_TIMEOUT)).await?;
            self.track(&answer);
            self.notices.extend(answer.notices);
            Ok(())
        })
    }

    fn commit(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            let answer: SessionState = self.call("commit", json!({ "session": self.id }), None).await?;
            self.track(&answer);
            self.in_transaction = answer.in_transaction.unwrap_or(false);
            self.notices.extend(answer.notices);
            Ok(())
        })
    }

    fn rollback(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            let answer: SessionState = self.call("rollback", json!({ "session": self.id }), None).await?;
            self.in_transaction = answer.in_transaction.unwrap_or(false);
            self.notices.extend(answer.notices);
            Ok(())
        })
    }

    fn in_transaction(&self) -> bool {
        self.in_transaction && !self.is_closed()
    }

    fn is_closed(&self) -> bool {
        self.closed || !self.process.alive()
    }

    fn canceller(&self) -> Option<Arc<dyn Canceller>> {
        self.capabilities.server_cancel.then(|| {
            Arc::new(Cancel { process: Arc::clone(&self.process), session: self.id.clone() }) as Arc<dyn Canceller>
        })
    }

    fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }
}

pub(crate) fn checked_rows<T>(
    width: usize,
    mut rows: Vec<Vec<T>>,
    limit: Option<usize>,
) -> Result<(Vec<Vec<T>>, bool), DbError> {
    if let Some(row) = rows.iter().position(|row| row.len() != width) {
        return Err(DbError::new(
            ErrorKind::Invalid,
            format!(
                "the driver sent a row of {} values for {width} columns (row {}); its answer was not used",
                rows[row].len(),
                row + 1
            ),
        ));
    }
    let cut = limit.is_some_and(|limit| rows.len() > limit);
    if let Some(limit) = limit {
        rows.truncate(limit);
    }
    Ok((rows, cut))
}

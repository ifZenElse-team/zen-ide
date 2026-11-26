use std::borrow::Cow;
use std::time::Duration;

use futures_util::TryStreamExt;
use serde_json::{Value as Json, json};
use tiberius::{ColumnData, QueryItem, QueryStream, Row, ToSql};
use zen_db_driver_kit::{Answer, BoxFuture, Failure, Inbox, Request, Session, Stop, value};

use crate::connect::Sql;
use crate::values;
use crate::words;

const CANCEL_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) const ISOLATION_LEVELS: [&str; 5] =
    ["READ UNCOMMITTED", "READ COMMITTED", "REPEATABLE READ", "SNAPSHOT", "SERIALIZABLE"];

pub(crate) struct Server {
    client: Sql,
    state: State,
}

#[derive(Default)]
struct State {
    in_transaction: bool,
    broken: bool,
    next_cursor: u64,
}

#[derive(Default)]
struct Ran {
    columns: Vec<Json>,
    rows: Vec<Json>,
    truncated: bool,
    affected: Option<u64>,
    unfinished: bool,
}

impl Server {
    pub(crate) async fn start(client: Sql, state: &Json) -> Result<(Self, String), Failure> {
        let mut server = Self { client, state: State::default() };
        let version = server.version().await;
        server.apply(state).await?;
        Ok((server, version))
    }

    async fn version(&mut self) -> String {
        let sql = "SELECT CAST(SERVERPROPERTY('ProductVersion') AS nvarchar(128)), \
                   CAST(SERVERPROPERTY('Edition') AS nvarchar(128))";
        match select(&mut self.client, sql, &[]).await {
            Ok(rows) => rows
                .first()
                .map(|row| {
                    format!("SQL Server {} ({})", text(row, 0).unwrap_or_default(), text(row, 1).unwrap_or_default())
                })
                .unwrap_or_else(|| "SQL Server".to_owned()),
            Err(_) => "SQL Server".to_owned(),
        }
    }

    async fn serve(mut self, mut inbox: Inbox) {
        let mut pending: Option<Request> = None;
        loop {
            let request = match pending.take() {
                Some(request) => request,
                None => match inbox.next().await {
                    Some(request) => request,
                    None => break,
                },
            };
            if self.state.broken {
                inbox.answer(
                    &request.id,
                    Err(Failure::database("notConnected", "the connection to the server was lost")),
                );
                continue;
            }
            if request.method == "query" {
                pending = self.query(&mut inbox, request).await;
                continue;
            }
            let answer = self.handle(&inbox, &request).await;
            inbox.answer(&request.id, answer);
        }
    }

    async fn handle(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        match request.method.as_str() {
            "ping" => select(&mut self.client, "SELECT 1", &[])
                .await
                .map(|_| json!({}))
                .map_err(|error| self.failed(&error, None)),
            "execute" => self.execute(inbox, request).await,
            "fetch" => Err(Failure::cursor(request.text("cursor").unwrap_or_default())),
            "closeCursor" => Ok(json!({})),
            "apply" => {
                self.apply(request.params.get("state").unwrap_or(&Json::Null)).await?;
                Ok(self.state_answer())
            }
            "commit" => self.end("WHILE @@TRANCOUNT > 0 COMMIT TRANSACTION").await,
            "rollback" => self.end("IF @@TRANCOUNT > 0 ROLLBACK TRANSACTION").await,
            "children" => {
                let path: Vec<String> = request
                    .params
                    .get("path")
                    .and_then(Json::as_array)
                    .map(|path| path.iter().filter_map(Json::as_str).map(str::to_owned).collect())
                    .unwrap_or_default();
                let nodes = crate::catalog::children(&mut self.client, &path).await?;
                Ok(json!({ "nodes": nodes }))
            }
            "introspect" => crate::catalog::introspect(&mut self.client, &request.params).await,
            other => Err(Failure::method(other)),
        }
    }

    fn state_answer(&self) -> Json {
        state_of(self.state.in_transaction)
    }

    async fn track(&mut self) {
        match select(&mut self.client, "SELECT @@TRANCOUNT", &[]).await {
            Ok(rows) => self.state.in_transaction = rows.first().and_then(|row| int(row, 0)).unwrap_or(0) > 0,
            Err(error) => self.state.broken |= lost(&error),
        }
    }

    async fn apply(&mut self, state: &Json) -> Result<(), Failure> {
        let mut sql = String::new();
        if let Some(database) = state.get("database").and_then(Json::as_str).filter(|name| !name.is_empty()) {
            sql.push_str(&format!("USE {};", words::bracket(database)));
        }
        if let Some(auto_commit) = state.get("autoCommit").and_then(Json::as_bool) {
            sql.push_str(if auto_commit { "SET IMPLICIT_TRANSACTIONS OFF;" } else { "SET IMPLICIT_TRANSACTIONS ON;" });
        }
        if let Some(isolation) = state.get("isolation").and_then(Json::as_str) {
            let level = ISOLATION_LEVELS
                .iter()
                .find(|level| level.eq_ignore_ascii_case(isolation))
                .ok_or_else(|| Failure::params(format!("`{isolation}` is not an isolation level of SQL Server")))?;
            sql.push_str(&format!("SET TRANSACTION ISOLATION LEVEL {level};"));
        }
        if !sql.is_empty() {
            select(&mut self.client, &sql, &[]).await.map_err(|error| self.failed(&error, None))?;
        }
        self.track().await;
        Ok(())
    }

    async fn end(&mut self, sql: &str) -> Answer {
        select(&mut self.client, sql, &[]).await.map_err(|error| self.failed(&error, None))?;
        self.track().await;
        Ok(self.state_answer())
    }

    fn failed(&mut self, error: &tiberius::error::Error, statement: Option<&str>) -> Failure {
        self.state.broken |= lost(error);
        failure(error, statement)
    }

    async fn stopped(&mut self, outcome: Result<Ran, Failure>) -> Result<Ran, Failure> {
        let unfinished = match &outcome {
            Ok(ran) => ran.unfinished,
            Err(failure) => matches!(failure.kind(), Some("cancelled" | "timeout")),
        };
        if unfinished {
            self.cancel().await;
        }
        self.track().await;
        outcome
    }

    async fn cancel(&mut self) {
        match tokio::time::timeout(CANCEL_TIMEOUT, self.client.cancel_query()).await {
            Ok(Ok(())) => {}
            Ok(Err(_)) | Err(_) => self.state.broken = true,
        }
    }

    async fn execute(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        let statement = request.statement()?.to_owned();
        let max_rows = usize::try_from(request.number("maxRows").unwrap_or(200)).unwrap_or(usize::MAX);
        let params = params(request);
        let mut stop = inbox.stop(request.number("timeoutMs"));
        let outcome = run(&mut self.client, &statement, params.as_deref(), max_rows, &mut stop).await;
        let ran = self.stopped(outcome).await?;
        let mut answer = self.state_answer();
        answer["columns"] = json!(ran.columns);
        answer["rows"] = json!(ran.rows);
        answer["truncated"] = json!(ran.truncated);
        if let Some(affected) = ran.affected {
            answer["affected"] = json!(affected);
        }
        Ok(answer)
    }

    async fn query(&mut self, inbox: &mut Inbox, request: Request) -> Option<Request> {
        let statement = match request.statement() {
            Ok(statement) => statement.to_owned(),
            Err(failure) => {
                inbox.answer(&request.id, Err(failure));
                return None;
            }
        };
        if words::counts_rows(&statement) {
            let mut stop = inbox.stop(None);
            let outcome = run(&mut self.client, &statement, None, 0, &mut stop).await;
            let answer = self.stopped(outcome).await.map(|ran| {
                let mut answer = self.state_answer();
                answer["columns"] = json!([]);
                answer["affected"] = json!(ran.affected);
                answer
            });
            inbox.answer(&request.id, answer);
            return None;
        }
        let skip = usize::try_from(request.number("skip").unwrap_or(0)).unwrap_or(usize::MAX);
        let sources = if words::selects(&statement) { sources(&mut self.client, &statement).await } else { Vec::new() };
        let end = cursor(&mut self.client, &mut self.state, inbox, &request.id, &statement, skip, &sources).await;
        if !end.finished {
            self.cancel().await;
        }
        self.track().await;
        if let Some((id, answer)) = end.answer {
            let answer = answer.map(|mut answer| {
                answer["inTransaction"] = json!(self.state.in_transaction);
                answer
            });
            inbox.answer(&id, answer);
        }
        end.deferred
    }
}

struct End {
    finished: bool,
    answer: Option<(Json, Answer)>,
    deferred: Option<Request>,
}

async fn cursor(
    client: &mut Sql,
    state: &mut State,
    inbox: &mut Inbox,
    id: &Json,
    statement: &str,
    skip: usize,
    sources: &[(String, Option<String>, Option<u32>)],
) -> End {
    let failed = |error: tiberius::error::Error| failure(&error, Some(statement));
    let mut stop = inbox.stop(None);
    let opened = stop
        .run(async {
            let mut stream = client.simple_query(statement).await.map_err(failed)?;
            let columns = columns(&mut stream, sources).await.map_err(failed)?;
            Ok((stream, columns))
        })
        .await;
    let (mut stream, columns) = match opened {
        Ok(opened) => opened,
        Err(failure) => return End { finished: false, answer: Some((id.clone(), Err(failure))), deferred: None },
    };
    let Some(columns) = columns else {
        return End { finished: true, answer: Some((id.clone(), Ok(json!({ "columns": [] })))), deferred: None };
    };
    if let Err(failure) = stop.run(page(&mut stream, skip, false, statement)).await {
        return End { finished: false, answer: Some((id.clone(), Err(failure))), deferred: None };
    }
    let name = format!("c{}", state.next_cursor);
    state.next_cursor += 1;
    let mut answer = state_of(state.in_transaction);
    answer["columns"] = json!(columns);
    answer["cursor"] = json!(name);
    inbox.answer(id, Ok(answer));
    while let Some(next) = inbox.next().await {
        let this_cursor = next.text("cursor") == Some(name.as_str());
        match next.method.as_str() {
            "fetch" if this_cursor => {
                let count = usize::try_from(next.number("count").unwrap_or(200).max(1)).unwrap_or(usize::MAX);
                let mut stop = inbox.stop(None);
                match stop.run(page(&mut stream, count, true, statement)).await {
                    Ok((rows, true)) => {
                        let answer = json!({ "rows": rows, "done": true });
                        return End { finished: true, answer: Some((next.id, Ok(answer))), deferred: None };
                    }
                    Ok((rows, false)) => {
                        let mut answer = state_of(state.in_transaction);
                        answer["rows"] = json!(rows);
                        answer["done"] = json!(false);
                        inbox.answer(&next.id, Ok(answer));
                    }
                    Err(failure) => {
                        return End { finished: false, answer: Some((next.id, Err(failure))), deferred: None };
                    }
                }
            }
            "fetch" => inbox.answer(&next.id, Err(Failure::cursor(next.text("cursor").unwrap_or_default()))),
            "closeCursor" if this_cursor => {
                return End { finished: false, answer: Some((next.id, Ok(json!({})))), deferred: None };
            }
            "closeCursor" | "ping" => inbox.answer(&next.id, Ok(json!({}))),
            _ => return End { finished: false, answer: None, deferred: Some(next) },
        }
    }
    End { finished: false, answer: None, deferred: None }
}

impl Session for Server {
    fn run(self: Box<Self>, inbox: Inbox) -> BoxFuture<'static, ()> {
        Box::pin(self.serve(inbox))
    }
}

fn state_of(in_transaction: bool) -> Json {
    json!({ "inTransaction": in_transaction })
}

fn params(request: &Request) -> Option<Vec<Option<String>>> {
    let list = request.params.get("params")?.as_array()?;
    Some(list.iter().map(|value| value.as_str().map(str::to_owned)).collect())
}

async fn run(
    client: &mut Sql,
    statement: &str,
    params: Option<&[Option<String>]>,
    max_rows: usize,
    stop: &mut Stop,
) -> Result<Ran, Failure> {
    let failed = |error: tiberius::error::Error| failure(&error, Some(statement));
    let bound: Vec<ColumnData<'_>> = params
        .unwrap_or_default()
        .iter()
        .map(|value| ColumnData::String(value.as_deref().map(Cow::Borrowed)))
        .collect();
    let refs: Vec<&dyn ToSql> = bound.iter().map(|value| value as &dyn ToSql).collect();
    let sources =
        if words::selects(statement) && params.is_none() { sources(client, statement).await } else { Vec::new() };
    stop.run(async {
        if words::counts_rows(statement) {
            let result = client.execute(statement, &refs).await.map_err(failed)?;
            return Ok(Ran { affected: Some(result.total()), ..Ran::default() });
        }
        let mut stream = if params.is_some() {
            client.query(statement, &refs).await.map_err(failed)?
        } else {
            client.simple_query(statement).await.map_err(failed)?
        };
        let Some(columns) = columns(&mut stream, &sources).await.map_err(failed)? else {
            stream.into_results().await.map_err(failed)?;
            return Ok(Ran::default());
        };
        let (rows, done) = page(&mut stream, max_rows.saturating_add(1), true, statement).await?;
        let truncated = rows.len() > max_rows;
        let rows: Vec<Json> = rows.into_iter().take(max_rows).collect();
        Ok(Ran { columns, rows, truncated, affected: None, unfinished: !done })
    })
    .await
}

async fn columns(
    stream: &mut QueryStream<'_>,
    sources: &[(String, Option<String>, Option<u32>)],
) -> tiberius::Result<Option<Vec<Json>>> {
    let Some(columns) = stream.columns().await? else { return Ok(None) };
    let described = sources.len() == columns.len();
    Ok(Some(
        columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let source = sources.get(index).filter(|_| described);
                let type_name = source.map_or(values::type_name(column.column_type()), |source| source.0.as_str());
                let mut wire = value::column(column.name(), type_name);
                if let Some((_, Some(origin), Some(table))) = source {
                    wire["origin"] = json!(origin);
                    wire["source"] = json!({ "table": table, "position": index });
                }
                wire
            })
            .collect(),
    ))
}

async fn page(
    stream: &mut QueryStream<'_>,
    count: usize,
    keep: bool,
    statement: &str,
) -> Result<(Vec<Json>, bool), Failure> {
    let failed = |error: tiberius::error::Error| failure(&error, Some(statement));
    let mut rows = Vec::new();
    let mut read = 0;
    while read < count {
        match stream.try_next().await.map_err(failed)? {
            None => return Ok((rows, true)),
            Some(QueryItem::Metadata(metadata)) if metadata.result_index() > 0 => break,
            Some(QueryItem::Metadata(_)) => {}
            Some(QueryItem::Row(row)) if row.result_index() > 0 => break,
            Some(QueryItem::Row(row)) => {
                read += 1;
                if keep {
                    rows.push(Json::Array(row.cells().map(|(_, data)| values::wire(data)).collect()));
                }
            }
        }
    }
    if read == count {
        return Ok((rows, false));
    }
    while stream.try_next().await.map_err(failed)?.is_some() {}
    Ok((rows, true))
}

async fn sources(client: &mut Sql, statement: &str) -> Vec<(String, Option<String>, Option<u32>)> {
    let sql = "SELECT d.system_type_name, d.source_column, CASE WHEN d.source_database = DB_NAME() THEN \
               OBJECT_ID(QUOTENAME(d.source_database) + N'.' + QUOTENAME(d.source_schema) + N'.' + QUOTENAME(d.source_table)) END \
               FROM sys.dm_exec_describe_first_result_set(@P1, NULL, 1) d WHERE d.is_hidden = 0 ORDER BY d.column_ordinal";
    let text_param = ColumnData::String(Some(Cow::Borrowed(statement)));
    let Ok(rows) = select(client, sql, &[&text_param]).await else { return Vec::new() };
    rows.iter()
        .map(|row| {
            let table = int(row, 2).and_then(|id| u32::try_from(id).ok());
            (text(row, 0).unwrap_or_default(), text(row, 1), table)
        })
        .collect()
}

pub(crate) async fn select(client: &mut Sql, sql: &str, params: &[&dyn ToSql]) -> tiberius::Result<Vec<Row>> {
    let stream = if params.is_empty() { client.simple_query(sql).await? } else { client.query(sql, params).await? };
    stream.into_first_result().await
}

pub(crate) fn text(row: &Row, index: usize) -> Option<String> {
    match row.cells().nth(index)?.1 {
        ColumnData::String(text) => text.as_ref().map(|text| text.to_string()),
        other => {
            let wire = values::wire(other);
            match &wire["value"] {
                Json::Null => None,
                Json::String(text) => Some(text.clone()),
                value => Some(value.to_string()),
            }
        }
    }
}

pub(crate) fn int(row: &Row, index: usize) -> Option<i64> {
    match row.cells().nth(index)?.1 {
        ColumnData::U8(value) => value.map(i64::from),
        ColumnData::I16(value) => value.map(i64::from),
        ColumnData::I32(value) => value.map(i64::from),
        ColumnData::I64(value) => *value,
        ColumnData::Bit(value) => value.map(i64::from),
        ColumnData::Numeric(value) => value.and_then(|number| i64::try_from(number.int_part()).ok()),
        _ => None,
    }
}

pub(crate) fn flag(row: &Row, index: usize) -> bool {
    int(row, index).unwrap_or(0) != 0
}

fn lost(error: &tiberius::error::Error) -> bool {
    matches!(error, tiberius::error::Error::Io { .. })
}

pub(crate) fn failure(error: &tiberius::error::Error, statement: Option<&str>) -> Failure {
    use tiberius::error::Error;
    match error {
        Error::Server(token) => {
            let message = token.message().to_owned();
            match token.code() {
                18456 | 18452 | 18486 | 18487 | 18488 => Failure::database("authentication", message),
                code => {
                    let position = statement.and_then(|statement| words::line_start(statement, token.line()));
                    Failure::server(message, Some(code.to_string()), position)
                }
            }
        }
        Error::Io { message, .. } if message.contains("TLS") => Failure::database("tls", message.clone()),
        Error::Io { message, .. } => Failure::database("connection", message.clone()),
        Error::Tls(message) => Failure::database("tls", message.clone()),
        Error::Routing { host, port } => {
            Failure::database("connection", format!("the server redirected the connection to {host}:{port}"))
        }
        Error::Protocol(message) if message.contains("ncryption") => Failure::database("tls", message.to_string()),
        Error::Protocol(message) => Failure::database("connection", message.to_string()),
        other => Failure::server(other.to_string(), None, None),
    }
}

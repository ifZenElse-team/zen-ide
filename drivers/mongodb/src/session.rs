use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use bson::{Bson, Document, doc};
use serde_json::{Value as Json, json};
use zen_db_driver_kit::{Answer, BoxFuture, Failure, Inbox, Request, Session, Stop};

use crate::connect::{self, Link, number};
use crate::shell::{self, Run, Shape, Statement};
use crate::target::Target;
use crate::values::{self, Columns};
use crate::wire::WireError;

const FIRST_BATCH: i64 = 101;
const KILL_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct Server {
    link: Option<Link>,
    target: Target,
    timeout: Duration,
    database: String,
    read_only: bool,
    cursors: HashMap<String, Cursor>,
    next_cursor: u64,
    auto_commit: bool,
    transaction: Transaction,
}

#[derive(Default)]
struct Transaction {
    lsid: Option<Document>,
    number: i64,
    open: bool,
}

impl Transaction {
    fn session(&mut self) -> Result<Document, Failure> {
        if let Some(lsid) = &self.lsid {
            return Ok(lsid.clone());
        }
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|error| Failure::database("connection", error.to_string()))?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let lsid = doc! { "id": bson::Binary { subtype: bson::spec::BinarySubtype::Uuid, bytes: bytes.to_vec() } };
        self.lsid = Some(lsid.clone());
        Ok(lsid)
    }
}

struct Cursor {
    database: String,
    collection: String,
    id: i64,
    buffer: VecDeque<Document>,
    columns: Columns,
    left: Option<usize>,
}

impl Server {
    pub(crate) fn new(link: Link, target: Target, timeout: Duration, state: &Json) -> Self {
        let mut server = Self {
            link: Some(link),
            database: target.database.clone(),
            target,
            timeout,
            read_only: false,
            cursors: HashMap::new(),
            next_cursor: 1,
            auto_commit: true,
            transaction: Transaction::default(),
        };
        let _ = server.set_state(state);
        server
    }

    pub(crate) async fn version(&mut self) -> String {
        match self.command("admin", doc! { "buildInfo": 1 }).await {
            Ok(info) => format!("MongoDB {}", info.get_str("version").unwrap_or_default()),
            Err(_) => "MongoDB".to_owned(),
        }
    }

    pub(crate) fn set_read_only(&mut self) {
        self.read_only = true;
    }

    fn set_state(&mut self, state: &Json) -> Result<(), Failure> {
        let named = state.get("database").or_else(|| state.get("schema")).and_then(Json::as_str);
        if let Some(database) = named.filter(|name| !name.is_empty()) {
            check_database(database)?;
            database.clone_into(&mut self.database);
        }
        if let Some(read_only) = state.get("readOnly").and_then(Json::as_bool) {
            self.read_only = read_only;
        }
        if let Some(auto_commit) = state.get("autoCommit").and_then(Json::as_bool) {
            self.auto_commit = auto_commit;
        }
        Ok(())
    }

    fn in_transaction(&mut self, command: &mut Document) -> Result<bool, Failure> {
        if self.auto_commit && !self.transaction.open {
            return Ok(false);
        }
        let lsid = self.transaction.session()?;
        let starting = !self.transaction.open;
        if starting {
            self.transaction.number += 1;
            self.transaction.open = true;
        }
        command.insert("lsid", lsid);
        command.insert("txnNumber", self.transaction.number);
        if starting {
            command.insert("startTransaction", true);
        }
        command.insert("autocommit", false);
        Ok(starting)
    }

    async fn end_transaction(&mut self, commit: bool) -> Result<(), Failure> {
        if !self.transaction.open {
            return Ok(());
        }
        self.transaction.open = false;
        let lsid = self.transaction.session()?;
        let name = if commit { "commitTransaction" } else { "abortTransaction" };
        let mut command = Document::new();
        command.insert(name, 1);
        command.insert("lsid", lsid);
        command.insert("txnNumber", self.transaction.number);
        command.insert("autocommit", false);
        match self.command("admin", command).await {
            Err(failure) if !commit && failure.message.contains("NoSuchTransaction") => Ok(()),
            outcome => outcome.map(|_| ()),
        }
    }

    async fn connection(&mut self) -> Result<&mut Link, Failure> {
        if self.link.is_none() {
            self.link = Some(connect::open(&self.target, self.timeout).await?);
        }
        self.link.as_mut().ok_or_else(|| Failure::database("notConnected", "the connection to the server was lost"))
    }

    async fn command(&mut self, database: &str, command: Document) -> Result<Document, Failure> {
        let link = self.connection().await?;
        match link.wire.command(database, command).await {
            Ok(reply) => Ok(reply),
            Err(error) => {
                if error.lost() {
                    self.link = None;
                    self.cursors.clear();
                    self.transaction.open = false;
                }
                Err(error.failure())
            }
        }
    }

    async fn stoppable(&mut self, stop: &mut Stop, database: &str, mut command: Document) -> Result<Document, Failure> {
        let starting = self.in_transaction(&mut command)?;
        let outcome = {
            let future = self.command(database, command);
            stop.run(future).await
        };
        if let Err(failure) = &outcome {
            if starting {
                self.transaction.open = false;
            }
            if matches!(failure.kind(), Some("cancelled" | "timeout")) {
                self.abandon().await;
            }
        }
        outcome
    }

    async fn abandon(&mut self) {
        let Some(old) = self.link.take() else { return };
        self.cursors.clear();
        self.transaction.open = false;
        drop(old.wire);
        let Some(connection_id) = old.connection_id else { return };
        let kill = async {
            let link = self.connection().await?;
            let found = link
                .wire
                .command(
                    "admin",
                    doc! {
                        "aggregate": 1,
                        "pipeline": [{ "$currentOp": { "allUsers": false, "idleConnections": false } }, { "$match": { "connectionId": connection_id } }],
                        "cursor": {},
                    },
                )
                .await
                .map_err(|error| error.failure())?;
            let operations: Vec<Bson> = found
                .get_document("cursor")
                .and_then(|cursor| cursor.get_array("firstBatch"))
                .map(|batch| {
                    batch.iter().filter_map(|op| op.as_document().and_then(|op| op.get("opid").cloned())).collect()
                })
                .unwrap_or_default();
            for op in operations {
                let _ = link.wire.command("admin", doc! { "killOp": 1, "op": op }).await;
            }
            Ok::<(), Failure>(())
        };
        let _ = tokio::time::timeout(KILL_TIMEOUT, kill).await;
    }

    async fn serve(mut self, mut inbox: Inbox) {
        while let Some(request) = inbox.next().await {
            let answer = self.handle(&inbox, &request).await;
            inbox.answer(&request.id, answer);
        }
        self.close_cursors().await;
    }

    async fn handle(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        match request.method.as_str() {
            "ping" => self.command("admin", doc! { "ping": 1 }).await.map(|_| json!({})),
            "execute" => self.execute(inbox, request).await,
            "query" => self.query(inbox, request).await,
            "fetch" => self.fetch(inbox, request).await,
            "closeCursor" => {
                let name = request.text("cursor").unwrap_or_default().to_owned();
                if let Some(cursor) = self.cursors.remove(&name) {
                    self.kill(&cursor).await;
                }
                Ok(json!({}))
            }
            "apply" => {
                self.set_state(request.params.get("state").unwrap_or(&Json::Null))?;
                Ok(json!({ "inTransaction": self.transaction.open }))
            }
            "commit" | "rollback" => {
                self.end_transaction(request.method == "commit").await?;
                Ok(json!({ "inTransaction": self.transaction.open }))
            }
            "children" => {
                let path: Vec<String> = request
                    .params
                    .get("path")
                    .and_then(Json::as_array)
                    .map(|path| path.iter().filter_map(Json::as_str).map(str::to_owned).collect())
                    .unwrap_or_default();
                let nodes = self.children(&path).await?;
                Ok(json!({ "nodes": nodes }))
            }
            other => Err(Failure::method(other)),
        }
    }

    fn plan(&mut self, statement: &str, timeout_ms: Option<u64>) -> Result<Option<Run>, Failure> {
        match shell::parse(statement)? {
            Statement::Use(database) => {
                check_database(&database)?;
                self.database = database;
                Ok(None)
            }
            Statement::Run(mut run) => {
                if run.writes && self.read_only {
                    return Err(Failure::read_only("the connection is read-only: this statement changes data"));
                }
                if let Some(ms) = timeout_ms.filter(|ms| *ms > 0)
                    && !run.command.contains_key("maxTimeMS")
                    && run.shape != Shape::Written
                {
                    run.command.insert("maxTimeMS", i64::try_from(ms).unwrap_or(i64::MAX));
                }
                Ok(Some(run))
            }
        }
    }

    async fn execute(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        let timeout = request.number("timeoutMs");
        let Some(run) = self.plan(request.statement()?, timeout)? else {
            return Ok(
                json!({ "columns": [], "rows": [], "truncated": false, "inTransaction": self.transaction.open }),
            );
        };
        let max_rows = usize::try_from(request.number("maxRows").unwrap_or(200)).unwrap_or(usize::MAX);
        let mut stop = inbox.stop(timeout);
        let database = run.database.clone().unwrap_or_else(|| self.database.clone());
        let mut command = run.command.clone();
        if run.shape == Shape::Cursor {
            let batch = i64::try_from(max_rows.saturating_add(1)).unwrap_or(i64::MAX).min(FIRST_BATCH.max(1));
            cursor_batch(&mut command, batch);
        }
        let reply = self.stoppable(&mut stop, &database, command).await?;
        if run.shape != Shape::Cursor {
            return Ok(match outcome(&run, reply) {
                Outcome::Affected(affected) => {
                    json!({ "columns": [], "rows": [], "truncated": false, "affected": affected, "inTransaction": self.transaction.open })
                }
                Outcome::Documents(documents) => {
                    let truncated = documents.len() > max_rows;
                    let (columns, rows) = values::table(&documents[..documents.len().min(max_rows)]);
                    json!({ "columns": columns, "rows": rows, "truncated": truncated, "inTransaction": self.transaction.open })
                }
            });
        }
        let (mut cursor, _) = open_cursor(&database, &reply, run.limit)?;
        let mut documents = Vec::new();
        while documents.len() <= max_rows {
            let wanted = max_rows.saturating_add(1) - documents.len();
            match self.next(&mut stop, &mut cursor, wanted).await? {
                Some(document) => documents.push(document),
                None => break,
            }
        }
        let truncated = documents.len() > max_rows;
        documents.truncate(max_rows);
        self.kill(&cursor).await;
        let (columns, rows) = values::table(&documents);
        Ok(json!({ "columns": columns, "rows": rows, "truncated": truncated, "inTransaction": self.transaction.open }))
    }

    async fn next(&mut self, stop: &mut Stop, cursor: &mut Cursor, wanted: usize) -> Result<Option<Document>, Failure> {
        loop {
            if cursor.left == Some(0) {
                return Ok(None);
            }
            if let Some(document) = cursor.buffer.pop_front() {
                if let Some(left) = cursor.left.as_mut() {
                    *left -= 1;
                }
                return Ok(Some(document));
            }
            if cursor.id == 0 {
                return Ok(None);
            }
            self.more(stop, cursor, wanted).await?;
        }
    }

    async fn more(&mut self, stop: &mut Stop, cursor: &mut Cursor, wanted: usize) -> Result<(), Failure> {
        let batch = i64::try_from(wanted.max(1)).unwrap_or(i64::MAX).min(100_000);
        let command = doc! { "getMore": cursor.id, "collection": cursor.collection.clone(), "batchSize": batch };
        let reply = self.stoppable(stop, &cursor.database.clone(), command).await?;
        let found = reply.get_document("cursor").map_err(|_| broken())?;
        cursor.id = found.get("id").and_then(number).unwrap_or(0);
        if let Ok(batch) = found.get_array("nextBatch") {
            cursor.buffer.extend(batch.iter().filter_map(|document| document.as_document().cloned()));
        }
        Ok(())
    }

    async fn kill(&mut self, cursor: &Cursor) {
        if cursor.id == 0 || self.link.is_none() {
            return;
        }
        let command = doc! { "killCursors": cursor.collection.clone(), "cursors": [cursor.id] };
        let _ = tokio::time::timeout(KILL_TIMEOUT, self.command(&cursor.database.clone(), command)).await;
    }

    async fn close_cursors(&mut self) {
        let cursors: Vec<Cursor> = self.cursors.drain().map(|(_, cursor)| cursor).collect();
        for cursor in cursors {
            self.kill(&cursor).await;
        }
    }

    async fn query(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        let Some(run) = self.plan(request.statement()?, None)? else {
            return Ok(json!({ "columns": [], "inTransaction": self.transaction.open }));
        };
        let mut stop = inbox.stop(None);
        let database = run.database.clone().unwrap_or_else(|| self.database.clone());
        if run.shape != Shape::Cursor {
            let reply = self.stoppable(&mut stop, &database, run.command.clone()).await?;
            let documents = match outcome(&run, reply) {
                Outcome::Affected(affected) => {
                    return Ok(json!({ "columns": [], "affected": affected, "inTransaction": self.transaction.open }));
                }
                Outcome::Documents(documents) => documents,
            };
            let columns = Columns::of(&documents);
            let wire = columns.wire();
            let skip = usize::try_from(request.number("skip").unwrap_or(0)).unwrap_or(usize::MAX);
            let name = self.remember(Cursor {
                database,
                collection: String::new(),
                id: 0,
                buffer: documents.into_iter().skip(skip).collect(),
                columns,
                left: None,
            });
            return Ok(json!({ "columns": wire, "cursor": name, "inTransaction": self.transaction.open }));
        }
        let mut command = run.command.clone();
        cursor_batch(&mut command, FIRST_BATCH);
        let reply = self.stoppable(&mut stop, &database, command).await?;
        let (mut cursor, first) = open_cursor(&database, &reply, run.limit)?;
        let mut skip = usize::try_from(request.number("skip").unwrap_or(0)).unwrap_or(usize::MAX);
        while skip > 0 && self.next(&mut stop, &mut cursor, skip.min(10_000)).await?.is_some() {
            skip -= 1;
        }
        cursor.columns = Columns::of(&first);
        if cursor.columns.is_empty() {
            cursor.columns = Columns::of(cursor.buffer.iter());
        }
        let columns = cursor.columns.wire();
        let name = self.remember(cursor);
        Ok(json!({ "columns": columns, "cursor": name, "inTransaction": self.transaction.open }))
    }

    fn remember(&mut self, cursor: Cursor) -> String {
        let name = format!("c{}", self.next_cursor);
        self.next_cursor += 1;
        self.cursors.insert(name.clone(), cursor);
        name
    }

    async fn fetch(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        let name = request.text("cursor").unwrap_or_default().to_owned();
        let Some(mut cursor) = self.cursors.remove(&name) else { return Err(Failure::cursor(&name)) };
        let count = usize::try_from(request.number("count").unwrap_or(200).max(1)).unwrap_or(usize::MAX);
        let mut stop = inbox.stop(None);
        let mut rows = Vec::new();
        while rows.len() < count {
            match self.next(&mut stop, &mut cursor, count - rows.len()).await? {
                Some(document) => rows.push(cursor.columns.row(&document)),
                None => break,
            }
        }
        let done = cursor.left == Some(0) || (cursor.buffer.is_empty() && cursor.id == 0);
        if done {
            self.kill(&cursor).await;
        } else {
            self.cursors.insert(name, cursor);
        }
        Ok(json!({ "rows": rows, "done": done, "inTransaction": self.transaction.open }))
    }

    async fn children(&mut self, path: &[String]) -> Result<Vec<Json>, Failure> {
        let node = |path: Vec<&str>, kind: &str, label: &str, has_children: bool| json!({ "path": path, "kind": kind, "label": label, "hasChildren": has_children });
        match path {
            [] => {
                let reply = self
                    .command("admin", doc! { "listDatabases": 1, "nameOnly": false, "authorizedDatabases": true })
                    .await?;
                let mut nodes: Vec<Json> = reply
                    .get_array("databases")
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(Bson::as_document)
                    .filter_map(|database| {
                        let name = database.get_str("name").ok()?;
                        let mut node = node(vec![name], "catalog", name, true);
                        node["system"] = json!(matches!(name, "admin" | "local" | "config"));
                        if let Some(size) = database.get("sizeOnDisk").and_then(number) {
                            node["size"] = json!(size);
                        }
                        Some(node)
                    })
                    .collect();
                if !nodes.iter().any(|node| node["label"] == self.database.as_str()) {
                    nodes.push(node(vec![self.database.as_str()], "catalog", &self.database, true));
                }
                Ok(nodes)
            }
            [database] => {
                let command =
                    doc! { "listCollections": 1, "authorizedCollections": true, "nameOnly": false, "cursor": {} };
                let documents = self.all(database, command).await?;
                let mut nodes: Vec<Json> = documents
                    .iter()
                    .filter_map(|collection| {
                        let name = collection.get_str("name").ok()?;
                        let kind = match collection.get_str("type") {
                            Ok("view") => "view",
                            _ => "collection",
                        };
                        let mut node = node(vec![database, name], kind, name, kind == "collection");
                        node["system"] = json!(name.starts_with("system."));
                        if let Ok(on) = collection.get_document("options").and_then(|options| options.get_str("viewOn"))
                        {
                            node["detail"] = json!(format!("on {on}"));
                        }
                        Some(node)
                    })
                    .collect();
                nodes.sort_by(|a, b| a["label"].as_str().cmp(&b["label"].as_str()));
                Ok(nodes)
            }
            [database, collection] => {
                let indexes = self.all(database, doc! { "listIndexes": collection.as_str(), "cursor": {} }).await?;
                Ok(indexes
                    .iter()
                    .filter_map(|index| {
                        let name = index.get_str("name").ok()?;
                        let mut node = node(vec![database, collection, name], "index", name, false);
                        if let Ok(keys) = index.get_document("key") {
                            let mut detail = values::json_text(&Bson::Document(keys.clone()));
                            if index.get_bool("unique").unwrap_or(false) {
                                detail.push_str(" unique");
                            }
                            node["detail"] = json!(detail);
                        }
                        Some(node)
                    })
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn all(&mut self, database: &str, command: Document) -> Result<Vec<Document>, Failure> {
        let reply = self.command(database, command).await?;
        let (mut cursor, _) = open_cursor(database, &reply, None)?;
        let mut documents: Vec<Document> = cursor.buffer.drain(..).collect();
        while cursor.id != 0 && documents.len() < 100_000 {
            let command = doc! { "getMore": cursor.id, "collection": cursor.collection.clone() };
            let reply = self.command(database, command).await?;
            let found = reply.get_document("cursor").map_err(|_| broken())?;
            cursor.id = found.get("id").and_then(number).unwrap_or(0);
            documents.extend(
                found
                    .get_array("nextBatch")
                    .map(Vec::as_slice)
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|d| d.as_document().cloned()),
            );
        }
        if cursor.id != 0 {
            self.kill(&cursor).await;
        }
        Ok(documents)
    }
}

impl Session for Server {
    fn run(self: Box<Self>, inbox: Inbox) -> BoxFuture<'static, ()> {
        Box::pin(self.serve(inbox))
    }
}

fn broken() -> Failure {
    WireError::Lost("the server's reply has no cursor".into()).failure()
}

fn check_database(name: &str) -> Result<(), Failure> {
    if name.is_empty() || name.len() > 63 || name.contains(['/', '\\', '.', ' ', '"', '$', '\0']) {
        return Err(Failure::params(format!("`{name}` is not a database name MongoDB takes")));
    }
    Ok(())
}

fn cursor_batch(command: &mut Document, batch: i64) {
    if command.contains_key("find") {
        if !command.contains_key("batchSize") {
            command.insert("batchSize", batch);
        }
    } else if let Ok(cursor) = command.get_document_mut("cursor")
        && !cursor.contains_key("batchSize")
    {
        cursor.insert("batchSize", batch);
    }
}

fn open_cursor(database: &str, reply: &Document, limit: Option<usize>) -> Result<(Cursor, Vec<Document>), Failure> {
    let found = reply.get_document("cursor").map_err(|_| broken())?;
    let first: Vec<Document> = found
        .get_array("firstBatch")
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|document| document.as_document().cloned())
        .collect();
    let namespace = found.get_str("ns").unwrap_or_default();
    let collection = namespace.split_once('.').map_or(namespace, |(_, collection)| collection).to_owned();
    let cursor = Cursor {
        database: database.to_owned(),
        collection,
        id: found.get("id").and_then(number).unwrap_or(0),
        buffer: first.iter().cloned().collect(),
        columns: Columns::default(),
        left: limit,
    };
    Ok((cursor, first))
}

enum Outcome {
    Affected(i64),
    Documents(Vec<Document>),
}

fn outcome(run: &Run, reply: Document) -> Outcome {
    let documents: Vec<Document> = match &run.shape {
        Shape::Written => return Outcome::Affected(reply.get("n").and_then(number).unwrap_or(0)),
        Shape::Count => vec![doc! { "count": reply.get("n").cloned().unwrap_or(Bson::Int32(0)) }],
        Shape::CountFromCursor => {
            let n = reply
                .get_document("cursor")
                .ok()
                .and_then(|cursor| cursor.get_array("firstBatch").ok())
                .and_then(|batch| batch.first())
                .and_then(Bson::as_document)
                .and_then(|first| first.get("n").cloned())
                .unwrap_or(Bson::Int32(0));
            vec![doc! { "count": n }]
        }
        Shape::Distinct(field) => reply
            .get_array("values")
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .map(|value| doc! { field.as_str(): value.clone() })
            .collect(),
        Shape::Databases => reply
            .get_array("databases")
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(|database| database.as_document().cloned())
            .collect(),
        Shape::Value => match reply.get("value") {
            Some(Bson::Document(document)) => vec![document.clone()],
            _ => Vec::new(),
        },
        Shape::Reply | Shape::Cursor => {
            Some(values::reply_row(reply)).filter(|row| !row.is_empty()).into_iter().collect()
        }
    };
    Outcome::Documents(documents)
}

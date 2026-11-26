use std::path::PathBuf;
use std::time::Duration;

use serde_json::{Value as Json, json};
use zen_db_driver_kit::{Answer, BoxFuture, Failure, Inbox, Request, Session, value};

use crate::process::{self, Printed, Process};
use crate::statement::{self, Checked, literal, quote};

const ENDS: Duration = Duration::from_secs(2);

#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) program: PathBuf,
    pub(crate) file: String,
    pub(crate) read_only: bool,
}

pub(crate) struct Server {
    target: Target,
    process: Option<Process>,
    in_transaction: bool,
    auto_commit: bool,
    using: Option<String>,
}

fn verb_opens(verb: &str) -> bool {
    matches!(verb, "BEGIN" | "START")
}

fn verb_ends(verb: &str) -> bool {
    matches!(verb, "COMMIT" | "END" | "ROLLBACK" | "ABORT")
}

impl Server {
    pub(crate) fn new(target: Target, process: Process) -> Self {
        Self { target, process: Some(process), in_transaction: false, auto_commit: true, using: None }
    }

    pub(crate) fn version(&self) -> String {
        let version = self.process.as_ref().map(|process| process.version.clone()).unwrap_or_default();
        format!("DuckDB {version}").trim_end().to_owned()
    }

    async fn process(&mut self) -> Result<&mut Process, Failure> {
        if self.process.as_ref().is_none_or(Process::ended) {
            if let Some(old) = self.process.take() {
                old.close().await;
            }
            self.in_transaction = false;
            let mut started = Process::start(&self.target.program, &self.target.file, self.target.read_only).await?;
            if let Some(using) = self.using.clone() {
                started.run(&statement::check(&format!("USE {using}"))?, 0).await?;
            }
            self.process = Some(started);
        }
        self.process.as_mut().ok_or_else(|| Failure::database("connection", "duckdb has ended"))
    }

    async fn plain(&mut self, text: &str) -> Result<Printed, Failure> {
        let checked = statement::check(text)?;
        let printed = self.process().await?.run(&checked, usize::MAX).await?;
        if !printed.errors.is_empty() {
            return Err(process::error(&printed.errors, text));
        }
        Ok(printed)
    }

    async fn run(&mut self, inbox: &Inbox, checked: &Checked, max_rows: usize, timeout: Option<u64>) -> Answer {
        if !self.auto_commit && !self.in_transaction && !checked.reads() && !verb_opens(&checked.verb) {
            self.plain("BEGIN TRANSACTION").await?;
            self.in_transaction = true;
        }
        let pid = self.process().await?.id();
        let mut stop = inbox.stop(timeout);
        let outcome = {
            let process = self.process.as_mut().ok_or_else(|| Failure::database("connection", "duckdb has ended"))?;
            let run = process.run(checked, max_rows);
            tokio::pin!(run);
            tokio::select! {
                printed = &mut run => Ok(printed),
                failure = stop.fired() => {
                    process::interrupt(pid);
                    let _ = tokio::time::timeout(ENDS, &mut run).await;
                    Err(failure)
                }
            }
        };
        let printed = match outcome {
            Ok(printed) => printed?,
            Err(failure) => {
                if let Some(old) = self.process.take() {
                    old.close().await;
                }
                self.in_transaction = false;
                return Err(failure);
            }
        };
        if !printed.errors.is_empty() {
            return Err(process::error(&printed.errors, &checked.text));
        }
        if verb_opens(&checked.verb) {
            self.in_transaction = true;
        } else if verb_ends(&checked.verb) {
            self.in_transaction = false;
        }
        if checked.verb == "USE" {
            let text = checked.text.trim();
            if text.get(..3).is_some_and(|word| word.eq_ignore_ascii_case("use")) {
                self.using = text.get(3..).map(str::trim).filter(|target| !target.is_empty()).map(str::to_owned);
            }
        }
        let columns: Vec<Json> = printed
            .columns
            .iter()
            .map(|column| {
                value::column(&column.name, &column.type_name.as_deref().unwrap_or_default().to_ascii_lowercase())
            })
            .collect();
        Ok(json!({
            "columns": columns,
            "rows": printed.rows,
            "truncated": printed.truncated,
            "inTransaction": self.in_transaction,
        }))
    }

    async fn execute(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        let checked = statement::check(request.statement()?)?;
        let max_rows = usize::try_from(request.number("maxRows").unwrap_or(200)).unwrap_or(usize::MAX);
        self.run(inbox, &checked, max_rows, request.number("timeoutMs")).await
    }

    pub(crate) async fn start_in(&mut self, state: &Json) -> Result<(), Failure> {
        self.apply(state).await.map(|_| ())
    }

    async fn apply(&mut self, state: &Json) -> Answer {
        if let Some(auto_commit) = state.get("autoCommit").and_then(Json::as_bool) {
            self.auto_commit = auto_commit;
        }
        let database = state.get("database").and_then(Json::as_str).filter(|name| !name.is_empty());
        let schema = state.get("schema").and_then(Json::as_str).filter(|name| !name.is_empty());
        let target = match (database, schema) {
            (Some(database), Some(schema)) => Some(format!("{}.{}", quote(database), quote(schema))),
            (Some(name), None) | (None, Some(name)) => Some(quote(name)),
            (None, None) => None,
        };
        if let Some(target) = target {
            self.plain(&format!("USE {target}")).await?;
            self.using = Some(target);
        }
        Ok(json!({ "inTransaction": self.in_transaction }))
    }

    async fn end(&mut self, commit: bool) -> Answer {
        if self.in_transaction {
            self.plain(if commit { "COMMIT" } else { "ROLLBACK" }).await?;
        }
        self.in_transaction = false;
        Ok(json!({ "inTransaction": false }))
    }

    async fn rows(&mut self, text: &str) -> Result<Vec<Vec<Json>>, Failure> {
        Ok(self.plain(text).await?.rows)
    }

    async fn children(&mut self, path: &[String]) -> Result<Vec<Json>, Failure> {
        let text = |row: &[Json], at: usize| {
            row.get(at).and_then(|cell| cell["value"].as_str()).unwrap_or_default().to_owned()
        };
        let flag = |row: &[Json], at: usize| row.get(at).and_then(|cell| cell["value"].as_bool()).unwrap_or(false);
        let node = |path: Vec<String>, kind: &str, has_children: bool| {
            let label = path.last().cloned().unwrap_or_default();
            json!({ "path": path, "kind": kind, "label": label, "hasChildren": has_children })
        };
        match path {
            [] => {
                let rows = self
                    .rows("SELECT database_name, internal, coalesce(path, '') FROM duckdb_databases() ORDER BY internal, database_name")
                    .await?;
                Ok(rows
                    .iter()
                    .map(|row| {
                        let mut found = node(vec![text(row, 0)], "catalog", true);
                        found["system"] = json!(flag(row, 1));
                        let file = text(row, 2);
                        if !file.is_empty() {
                            found["detail"] = json!(file);
                        }
                        found
                    })
                    .collect())
            }
            [database] => {
                let rows = self
                    .rows(&format!(
                        "SELECT schema_name, internal FROM duckdb_schemas() WHERE database_name = {} ORDER BY internal, schema_name",
                        literal(database)
                    ))
                    .await?;
                Ok(rows
                    .iter()
                    .map(|row| {
                        let mut found = node(vec![database.clone(), text(row, 0)], "schema", true);
                        found["system"] = json!(flag(row, 1));
                        found
                    })
                    .collect())
            }
            [database, schema] => {
                let rows = self
                    .rows(&format!(
                        "SELECT table_name, 'table', estimated_size, coalesce(comment, '') FROM duckdb_tables() \
                         WHERE database_name = {0} AND schema_name = {1} \
                         UNION ALL SELECT view_name, 'view', NULL, coalesce(comment, '') FROM duckdb_views() \
                         WHERE database_name = {0} AND schema_name = {1} AND NOT internal ORDER BY 1",
                        literal(database),
                        literal(schema)
                    ))
                    .await?;
                Ok(rows
                    .iter()
                    .map(|row| {
                        let path = vec![database.clone(), schema.clone(), text(row, 0)];
                        let mut found = node(path, &text(row, 1), true);
                        if let Some(count) = row.get(2).and_then(|cell| cell["value"].as_i64()) {
                            found["rows"] = json!(count);
                        }
                        let comment = text(row, 3);
                        if !comment.is_empty() {
                            found["comment"] = json!(comment);
                        }
                        found
                    })
                    .collect())
            }
            [database, schema, table] => {
                let rows = self
                    .rows(&format!(
                        "SELECT column_name, data_type, is_nullable FROM duckdb_columns() \
                         WHERE database_name = {} AND schema_name = {} AND table_name = {} ORDER BY column_index",
                        literal(database),
                        literal(schema),
                        literal(table)
                    ))
                    .await?;
                Ok(rows
                    .iter()
                    .map(|row| {
                        let path = vec![database.clone(), schema.clone(), table.clone(), text(row, 0)];
                        let mut found = node(path, "column", false);
                        let nullable = if flag(row, 2) { "" } else { " NOT NULL" };
                        found["detail"] = json!(format!("{}{nullable}", text(row, 1)));
                        found
                    })
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }

    async fn handle(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        match request.method.as_str() {
            "ping" => self.plain("SELECT 1").await.map(|_| json!({})),
            "execute" => self.execute(inbox, request).await,
            "apply" => self.apply(request.params.get("state").unwrap_or(&Json::Null)).await,
            "commit" => self.end(true).await,
            "rollback" => self.end(false).await,
            "children" => {
                let path: Vec<String> = request
                    .params
                    .get("path")
                    .and_then(Json::as_array)
                    .map(|path| path.iter().filter_map(Json::as_str).map(str::to_owned).collect())
                    .unwrap_or_default();
                Ok(json!({ "nodes": self.children(&path).await? }))
            }
            other => Err(Failure::method(other)),
        }
    }

    async fn serve(mut self, mut inbox: Inbox) {
        while let Some(request) = inbox.next().await {
            let answer = self.handle(&inbox, &request).await;
            inbox.answer(&request.id, answer);
        }
        if let Some(process) = self.process.take() {
            process.close().await;
        }
    }
}

impl Session for Server {
    fn run(self: Box<Self>, inbox: Inbox) -> BoxFuture<'static, ()> {
        Box::pin(self.serve(inbox))
    }
}

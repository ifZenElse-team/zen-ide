use std::time::Duration;

use serde_json::{Value as Json, json};
use zen_db_driver_kit::{Answer, BoxFuture, Failure, Inbox, Request, Session, value};

use crate::catalog;
use crate::output::{self, Printed};
use crate::process::{self, Process};
use crate::statement::{self, Checked, Kind, literal, quote};
use crate::target::Target;

const STOPS: Duration = Duration::from_secs(3);

pub(crate) const ISOLATION_LEVELS: &[&str] = &["READ COMMITTED", "SERIALIZABLE"];

const NLS: &str = "ALTER SESSION SET NLS_DATE_FORMAT = 'YYYY-MM-DD HH24:MI:SS' \
                   NLS_TIMESTAMP_FORMAT = 'YYYY-MM-DD HH24:MI:SS.FF' \
                   NLS_TIMESTAMP_TZ_FORMAT = 'YYYY-MM-DD HH24:MI:SS.FF TZH:TZM' \
                   NLS_NUMERIC_CHARACTERS = '.,'";

const VERSION: &str = "SELECT product || ' ' || version AS version FROM product_component_version \
                       WHERE product LIKE 'Oracle%' AND ROWNUM = 1";

pub(crate) struct Server {
    target: Target,
    process: Option<Process>,
    auto_commit: bool,
    in_transaction: bool,
    schema: Option<String>,
    isolation: Option<String>,
    version: String,
    held: bool,
}

fn redacted(target: &Target, mut failure: Failure) -> Failure {
    failure.message = target.redact(&failure.message);
    failure
}

fn text_at(row: &[Json], at: usize) -> String {
    row.get(at)
        .and_then(|cell| {
            cell["value"].as_str().map(str::to_owned).or_else(|| cell["value"].as_i64().map(|n| n.to_string()))
        })
        .unwrap_or_default()
}

impl Server {
    pub(crate) async fn start(target: Target, state: &Json) -> Result<Self, Failure> {
        let auto_commit = state.get("autoCommit").and_then(Json::as_bool).unwrap_or(true);
        let mut server = Server {
            target,
            process: None,
            auto_commit,
            in_transaction: false,
            schema: None,
            isolation: None,
            version: String::new(),
            held: false,
        };
        server.open().await?;
        server.apply(state).await?;
        Ok(server)
    }

    pub(crate) fn version(&self) -> String {
        self.version.clone()
    }

    async fn open(&mut self) -> Result<(), Failure> {
        let (mut started, printed) = Process::start(&self.target, self.auto_commit).await?;
        if let Some(failure) = output::failure(&printed, "") {
            let mut failure = redacted(&self.target, failure);
            if failure.kind() == Some("server") {
                failure = Failure::database("connection", failure.message);
            }
            started.close().await;
            return Err(failure);
        }
        let probe = started.statement("", VERSION).await?;
        let read = output::read(&probe.lines, VERSION, 1);
        if let Some(failure) = read.failure {
            let failure = redacted(&self.target, failure);
            started.close().await;
            return Err(match failure.kind() {
                Some("server") => Failure::database("connection", failure.message),
                _ => failure,
            });
        }
        if !read.has_result {
            started.close().await;
            return Err(Failure::database("connection", "SQLcl did not answer as expected: is it SQLcl 22 or newer?"));
        }
        self.version = read.rows.first().map(|row| text_at(row, 0)).unwrap_or_default();
        started.statement("", NLS).await?;
        let module = format!("BEGIN DBMS_APPLICATION_INFO.SET_MODULE('{}', NULL); END;", self.target.application);
        started.statement("", &module).await?;
        if let Some(schema) = &self.schema {
            started.statement("", &format!("ALTER SESSION SET CURRENT_SCHEMA = {}", quote(schema))).await?;
        }
        if let Some(level) = &self.isolation {
            started.statement("", &format!("ALTER SESSION SET ISOLATION_LEVEL = {level}")).await?;
        }
        self.process = Some(started);
        self.in_transaction = false;
        Ok(())
    }

    async fn process(&mut self) -> Result<&mut Process, Failure> {
        if self.process.as_ref().is_none_or(Process::ended) {
            if let Some(old) = self.process.take() {
                old.close().await;
            }
            self.open().await?;
        }
        self.process.as_mut().ok_or_else(|| Failure::database("connection", "SQLcl has ended"))
    }

    async fn forget(&mut self) {
        if let Some(old) = self.process.take() {
            process::kill(old.id());
            old.close().await;
        }
        self.in_transaction = false;
    }

    async fn printed(&mut self, commands: &str, text: &str, max_rows: usize) -> Result<Printed, Failure> {
        let block =
            self.process().await?.statement(commands, text).await.map_err(|failure| redacted(&self.target, failure))?;
        let printed = output::read(&block.lines, text, max_rows);
        if block.overflowed && printed.failure.is_none() {
            return Err(Failure::database("server", "the result is too large to read at once: limit its rows"));
        }
        if let Some(failure) = printed.failure.clone() {
            if failure.kind() == Some("connection") {
                self.forget().await;
            }
            return Err(redacted(&self.target, failure));
        }
        Ok(printed)
    }

    pub(crate) async fn rows(&mut self, text: &str) -> Result<Vec<Vec<Json>>, Failure> {
        Ok(self.printed("SET ROWLIMIT OFF\n", text, usize::MAX).await?.rows)
    }

    async fn run(&mut self, inbox: &Inbox, checked: &Checked, max_rows: usize, timeout: Option<u64>) -> Answer {
        let commands = if checked.kind == Kind::Query {
            match max_rows.checked_add(1) {
                Some(limit) => format!("SET ROWLIMIT {limit}\n"),
                None => "SET ROWLIMIT OFF\n".to_owned(),
            }
        } else {
            String::new()
        };
        let pid = self.process().await?.id();
        let mut stop = inbox.stop(timeout);
        let outcome = {
            let process = self.process.as_mut().ok_or_else(|| Failure::database("connection", "SQLcl has ended"))?;
            let run = process.statement(&commands, &checked.text);
            tokio::pin!(run);
            tokio::select! {
                block = &mut run => Ok(block),
                failure = stop.fired() => {
                    process::interrupt(pid);
                    let finished = tokio::time::timeout(STOPS, &mut run).await;
                    Err((failure, finished.is_ok_and(|block| block.is_ok())))
                }
            }
        };
        let block = match outcome {
            Ok(block) => block.map_err(|failure| redacted(&self.target, failure))?,
            Err((failure, survived)) => {
                if !survived || self.process.as_ref().is_none_or(Process::ended) {
                    self.forget().await;
                }
                return Err(failure);
            }
        };
        let (block, cut) = if block.overflowed && checked.reads() {
            let fitted: usize = block.lines.iter().map(|line| line.matches("},{").count()).sum::<usize>() + 1;
            let limit = (fitted / 2).clamp(1, max_rows.max(1));
            let process = self.process.as_mut().ok_or_else(|| Failure::database("connection", "SQLcl has ended"))?;
            let again = process
                .statement(&format!("SET ROWLIMIT {limit}\n"), &checked.text)
                .await
                .map_err(|failure| redacted(&self.target, failure))?;
            (again, true)
        } else {
            (block, false)
        };
        let mut printed = output::read(&block.lines, &checked.text, max_rows);
        if block.overflowed && printed.failure.is_none() {
            return Err(Failure::database("server", "the result is too large to read at once: limit its rows"));
        }
        printed.truncated |= cut;
        if let Some(failure) = printed.failure {
            if failure.kind() == Some("connection") {
                self.forget().await;
            }
            return Err(redacted(&self.target, failure));
        }
        self.follow(checked);
        let columns: Vec<Json> = printed
            .columns
            .iter()
            .map(|column| value::column(&column.name, &column.type_name.to_ascii_lowercase()))
            .collect();
        let mut columns = columns;
        if printed.has_result && !printed.columns.is_empty() {
            self.sources(&checked.text, &mut columns).await;
        }
        let notices: Vec<Json> =
            printed.notes.iter().map(|note| json!({ "severity": "INFO", "message": note })).collect();
        let mut answer = json!({
            "columns": columns,
            "rows": printed.rows,
            "truncated": printed.truncated,
            "affected": printed.affected,
            "inTransaction": self.in_transaction,
        });
        if !notices.is_empty() {
            answer["notices"] = json!(notices);
        }
        Ok(answer)
    }

    fn follow(&mut self, checked: &Checked) {
        if self.auto_commit {
            self.in_transaction = false;
            return;
        }
        match checked.kind {
            Kind::Change | Kind::Block => self.in_transaction = true,
            Kind::Query if !checked.reads() => self.in_transaction = true,
            Kind::Other if checked.verb == "SET" => self.in_transaction = true,
            Kind::Commit | Kind::Rollback | Kind::Definition => self.in_transaction = false,
            Kind::Query | Kind::Other => {}
        }
    }

    async fn sources(&mut self, text: &str, columns: &mut [Json]) {
        let Some((owner, table)) = catalog::plain_table(text) else { return };
        let owner =
            owner.map_or_else(|| "SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA')".to_owned(), |owner| literal(&owner));
        let query = format!(
            "SELECT o.object_id, c.column_name FROM all_objects o \
             JOIN all_tab_columns c ON c.owner = o.owner AND c.table_name = o.object_name \
             WHERE o.owner = {owner} AND o.object_name = {} AND o.object_type = 'TABLE'",
            literal(&table)
        );
        let Ok(rows) = self.rows(&query).await else { return };
        for (position, column) in columns.iter_mut().enumerate() {
            let name = column["name"].as_str().unwrap_or_default().to_owned();
            let found = rows.iter().find(|row| text_at(row, 1) == name);
            if let Some(row) = found
                && let Some(oid) =
                    row.first().and_then(|cell| cell["value"].as_u64()).and_then(|oid| u32::try_from(oid).ok())
            {
                column["origin"] = json!(name);
                column["source"] = json!({ "table": oid, "position": position });
            }
        }
    }

    async fn hold(&mut self, held: bool) -> Result<(), Failure> {
        let line = if held { "SET AUTOCOMMIT OFF\n" } else { "SET AUTOCOMMIT ON\n" };
        self.process().await?.command(line).await?;
        self.auto_commit = !held;
        self.held = held;
        Ok(())
    }

    async fn execute(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        let text = request.statement()?;
        let control = text.trim().trim_end_matches(';').trim_end().to_ascii_uppercase();
        if control == "BEGIN" {
            if self.auto_commit {
                self.hold(true).await?;
            }
            self.in_transaction = true;
            return Ok(json!({ "columns": [], "rows": [], "truncated": false, "inTransaction": true }));
        }
        if control.starts_with("RELEASE SAVEPOINT") {
            return Ok(json!({ "columns": [], "rows": [], "truncated": false, "inTransaction": self.in_transaction }));
        }
        let checked = statement::check(text)?;
        let max_rows = usize::try_from(request.number("maxRows").unwrap_or(200)).unwrap_or(usize::MAX);
        let answer = self.run(inbox, &checked, max_rows, request.number("timeoutMs")).await;
        if self.held && matches!(checked.kind, Kind::Commit | Kind::Rollback) {
            self.hold(false).await?;
        }
        answer
    }

    async fn apply(&mut self, state: &Json) -> Answer {
        if let Some(auto_commit) = state.get("autoCommit").and_then(Json::as_bool)
            && auto_commit != self.auto_commit
        {
            let line = if auto_commit { "SET AUTOCOMMIT ON\n" } else { "SET AUTOCOMMIT OFF\n" };
            self.process().await?.command(line).await?;
            self.auto_commit = auto_commit;
            self.held = false;
        }
        if let Some(schema) = state.get("schema").and_then(Json::as_str).filter(|name| !name.is_empty()) {
            self.printed("", &format!("ALTER SESSION SET CURRENT_SCHEMA = {}", quote(schema)), 0).await?;
            self.schema = Some(schema.to_owned());
        }
        if let Some(level) = state.get("isolation").and_then(Json::as_str).filter(|level| !level.is_empty()) {
            let level = level.to_ascii_uppercase().replace('_', " ");
            if !ISOLATION_LEVELS.contains(&level.as_str()) {
                return Err(Failure::unsupported(format!("Oracle has no isolation level {level}")));
            }
            self.printed("", &format!("ALTER SESSION SET ISOLATION_LEVEL = {level}"), 0).await?;
            self.isolation = Some(level);
        }
        Ok(json!({ "inTransaction": self.in_transaction }))
    }

    async fn end(&mut self, commit: bool) -> Answer {
        self.printed("", if commit { "COMMIT" } else { "ROLLBACK" }, 0).await?;
        self.in_transaction = false;
        Ok(json!({ "inTransaction": false }))
    }

    async fn handle(&mut self, inbox: &Inbox, request: &Request) -> Answer {
        match request.method.as_str() {
            "ping" => self.printed("", "SELECT 1 FROM dual", 1).await.map(|_| json!({})),
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
                Ok(json!({ "nodes": catalog::children(self, &path).await? }))
            }
            "introspect" => catalog::introspect(self, &request.params).await,
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

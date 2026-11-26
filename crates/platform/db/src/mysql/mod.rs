mod ddl;
mod decode;
mod introspect;
mod sessions;
mod tls;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mysql_async::prelude::Queryable;
use mysql_async::{Conn, Opts, OptsBuilder};

use crate::driver::{
    BoxFuture, Canceller, ConnectSpec, ContextKind, DbError, Driver, DriverSession, ErrorKind, ExecOptions, Param,
    RowSink, StreamEnd, StreamOptions, TrackedState,
};
use crate::explain::{ExplainOption, ExplainOptions, ExplainRequest};
use crate::model::{Capabilities, DataModel, Node, NodeKind, SettingField, StatementKind};
use crate::registry::{DataSource, TlsMode};
use crate::schema::{DdlOptions, IntrospectRequest, Introspection, LARGE_SCHEMA, Snapshot, TreeOptions};
use crate::value::{Column, ColumnSource, Execution, Notice, Value};

pub use introspect::CATALOG;

pub const ID: &str = "mysql";

pub const MARIADB_ID: &str = "mariadb";

pub const KEEP_ALIVE: &str = "keepAlive";

const ISOLATION_LEVELS: [&str; 4] = ["read uncommitted", "read committed", "repeatable read", "serializable"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flavour {
    MySql,
    MariaDb,
}

pub struct MySql {
    flavour: Flavour,
}

impl MySql {
    pub fn for_mysql() -> Self {
        Self { flavour: Flavour::MySql }
    }

    pub fn for_mariadb() -> Self {
        Self { flavour: Flavour::MariaDb }
    }
}

fn capabilities(flavour: Flavour) -> &'static Capabilities {
    static MYSQL: OnceLock<Capabilities> = OnceLock::new();
    static MARIADB: OnceLock<Capabilities> = OnceLock::new();
    let (cell, language, scheme) = match flavour {
        Flavour::MySql => (&MYSQL, "sql:mysql", "mysql"),
        Flavour::MariaDb => (&MARIADB, "sql:mariadb", "mariadb"),
    };
    cell.get_or_init(|| Capabilities {
        models: vec![DataModel::Relational],
        introspection: true,
        language: language.to_owned(),
        transactions: true,
        isolation_levels: ISOLATION_LEVELS.map(str::to_owned).to_vec(),
        catalogs: false,
        schemas: true,
        node_kinds: vec![
            NodeKind::Catalog,
            NodeKind::Schema,
            NodeKind::Folder,
            NodeKind::Table,
            NodeKind::View,
            NodeKind::Column,
            NodeKind::Index,
            NodeKind::Other("function".to_owned()),
            NodeKind::Other("procedure".to_owned()),
            NodeKind::Other("event".to_owned()),
            NodeKind::Other("constraint".to_owned()),
            NodeKind::Other("foreignKey".to_owned()),
            NodeKind::Other("trigger".to_owned()),
            NodeKind::Other("reference".to_owned()),
        ],
        explain: true,
        server_cancel: true,
        bind_parameters: true,
        row_identity: Some("primaryKey".to_owned()),
        ddl_preview: false,
        server_paging: false,
        sessions: true,
        read_only_sessions: true,
        uri_schemes: vec![scheme.to_owned()],
        shared_connection: false,
        password: true,
        settings: vec![
            SettingField {
                key: "sslmode".to_owned(),
                kind: "choice".to_owned(),
                choices: ["disable", "prefer", "require", "verify-ca", "verify-full"].map(str::to_owned).to_vec(),
                default: Some("prefer".to_owned()),
            },
            SettingField {
                key: KEEP_ALIVE.to_owned(),
                kind: "number".to_owned(),
                choices: Vec::new(),
                default: Some("30".to_owned()),
            },
        ],
    })
}

impl Driver for MySql {
    fn id(&self) -> &str {
        match self.flavour {
            Flavour::MySql => ID,
            Flavour::MariaDb => MARIADB_ID,
        }
    }

    fn name(&self) -> &str {
        match self.flavour {
            Flavour::MySql => "MySQL",
            Flavour::MariaDb => "MariaDB",
        }
    }

    fn capabilities(&self) -> &Capabilities {
        capabilities(self.flavour)
    }

    fn default_port(&self) -> Option<u16> {
        Some(3306)
    }

    fn classify(&self, statement: &str) -> StatementKind {
        classify(statement)
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> BoxFuture<'a, Result<Box<dyn DriverSession>, DbError>> {
        let driver = self.id().to_owned();
        Box::pin(async move { Ok(Box::new(Session::open(spec, driver).await?) as Box<dyn DriverSession>) })
    }

    fn changes_structure(&self, statement: &str) -> bool {
        changes_structure(statement)
    }

    fn ddl(&self, snapshot: &Snapshot, path: &[String], options: &DdlOptions) -> Result<String, DbError> {
        ddl::generate(snapshot, path, options)
    }

    fn explain_options(&self) -> Vec<ExplainOption> {
        vec![ExplainOption::Analyze]
    }

    fn explain(&self, statement: &str, options: &ExplainOptions) -> Result<ExplainRequest, DbError> {
        Ok(crate::explain::mysql(statement, options, self.flavour == Flavour::MariaDb))
    }
}

fn words(statement: &str) -> Vec<String> {
    crate::sql::split(statement, crate::sql::Dialect::MySql)
        .first()
        .map_or(statement, |found| &statement[found.start..found.end])
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

pub fn changes_structure(statement: &str) -> bool {
    matches!(
        crate::sql::first_keyword(&strip_hash_comments(statement)).as_deref(),
        Some(
            "CREATE"
                | "ALTER"
                | "DROP"
                | "RENAME"
                | "TRUNCATE"
                | "GRANT"
                | "REVOKE"
                | "INSTALL"
                | "UNINSTALL"
                | "IMPORT"
        )
    )
}

pub fn classify(statement: &str) -> StatementKind {
    let text = strip_hash_comments(statement);
    let words = words(&text);
    let reads = matches!(
        words.first().map(String::as_str),
        Some("select" | "show" | "describe" | "desc" | "explain" | "with" | "table" | "values" | "help")
    );
    let writes = words.iter().any(|word| {
        matches!(
            word.as_str(),
            "insert"
                | "update"
                | "delete"
                | "replace"
                | "into"
                | "analyze"
                | "get_lock"
                | "release_lock"
                | "sleep"
                | "nextval"
                | "setval"
                | "load_file"
        )
    }) || (words.iter().any(|word| word == "for")
        && words.iter().any(|word| word == "update" || word == "share"))
        || words.windows(2).any(|pair| pair[0] == "share" && pair[1] == "mode");
    if reads && !writes { StatementKind::Read } else { StatementKind::Write }
}

fn strip_hash_comments(statement: &str) -> String {
    let found = crate::sql::split(statement, crate::sql::Dialect::MySql);
    let Some(first) = found.first() else { return String::new() };
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut chars = statement[first.start..first.end].chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(open) => {
                out.push(c);
                if c == '\\' && open != '`' {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                } else if c == open {
                    quote = None;
                }
            }
            None if matches!(c, '\'' | '"' | '`') => {
                quote = Some(c);
                out.push(c);
            }
            None if c == '#' || (c == '-' && chars.peek() == Some(&'-')) => {
                for skipped in chars.by_ref() {
                    if skipped == '\n' {
                        break;
                    }
                }
                out.push('\n');
            }
            None if c == '/' && chars.peek() == Some(&'*') => {
                chars.next();
                let mut last = ' ';
                for skipped in chars.by_ref() {
                    if last == '*' && skipped == '/' {
                        break;
                    }
                    last = skipped;
                }
                out.push(' ');
            }
            None => out.push(c),
        }
    }
    out
}

pub(crate) fn quote_ident(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Dialect {
    mariadb: bool,
    backslashes: bool,
}

impl Dialect {
    pub(crate) fn literal(self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 2);
        out.push('\'');
        for c in text.chars() {
            match c {
                '\'' => out.push_str("''"),
                '\\' if self.backslashes => out.push_str("\\\\"),
                '\0' if self.backslashes => out.push_str("\\0"),
                c => out.push(c),
            }
        }
        out.push('\'');
        out
    }

    pub(crate) fn list(self, names: &[String]) -> String {
        names.iter().map(|name| self.literal(name)).collect::<Vec<_>>().join(", ")
    }
}

fn server_error_kind(code: u16) -> ErrorKind {
    match code {
        1044 | 1045 | 1251 | 1524 | 1698 | 1862 | 2059 | 3118 => ErrorKind::Authentication,
        3024 | 1969 => ErrorKind::Timeout,
        1317 => ErrorKind::Cancelled,
        _ => ErrorKind::Server,
    }
}

pub(crate) fn error(failed: &mysql_async::Error) -> DbError {
    use mysql_async::{DriverError, IoError};
    match failed {
        mysql_async::Error::Server(server) => {
            DbError::new(server_error_kind(server.code), &server.message).with_code(server.code.to_string())
        }
        mysql_async::Error::Io(IoError::Tls(tls)) => DbError::new(ErrorKind::Tls, tls.to_string()),
        mysql_async::Error::Driver(DriverError::NoClientSslFlagFromServer) => {
            DbError::new(ErrorKind::Tls, "the server does not offer TLS")
        }
        mysql_async::Error::Url(url) => DbError::new(ErrorKind::Invalid, url.to_string()),
        other => {
            let mut detail = other.to_string();
            let mut source = std::error::Error::source(other);
            while let Some(cause) = source {
                let text = cause.to_string();
                if !detail.contains(&text) {
                    detail.push_str(": ");
                    detail.push_str(&text);
                }
                source = cause.source();
            }
            let lower = detail.to_ascii_lowercase();
            let kind = if lower.contains("timed out") {
                ErrorKind::Timeout
            } else if lower.contains("certificate") || lower.contains("tls") || lower.contains("handshake") {
                ErrorKind::Tls
            } else {
                ErrorKind::Connection
            };
            DbError::new(kind, detail)
        }
    }
}

fn position(statement: &str, message: &str) -> Option<usize> {
    let start = message.find("near '")? + "near '".len();
    let end = message.rfind("' at line ")?;
    let snippet = message.get(start..end).filter(|snippet| !snippet.is_empty())?;
    let line: usize = message.get(end + "' at line ".len()..)?.trim().parse().ok()?;
    let line_start: usize = statement.split_inclusive('\n').take(line.saturating_sub(1)).map(str::len).sum();
    let at = statement
        .get(line_start..)
        .and_then(|rest| rest.find(snippet))
        .map(|offset| line_start + offset)
        .or_else(|| statement.find(snippet))?;
    Some(statement[..at].chars().count() + 1)
}

fn failed_statement(failed: &mysql_async::Error, statement: &str) -> DbError {
    let mut error = error(failed);
    if let mysql_async::Error::Server(server) = failed
        && let Some(at) = position(statement, &server.message)
    {
        error = error.at(at);
    }
    error
}

fn keep_alive(source: &DataSource) -> Option<Duration> {
    let seconds = source.settings.get(KEEP_ALIVE).and_then(|value| match value {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    });
    match seconds {
        Some(0) => None,
        Some(seconds) => Some(Duration::from_secs(seconds)),
        None => Some(Duration::from_secs(30)),
    }
}

fn whole_seconds(duration: Duration) -> u64 {
    duration.as_secs() + u64::from(duration.subsec_nanos() > 0)
}

fn cells(row: mysql_async::Row) -> Vec<Option<String>> {
    row.unwrap_raw()
        .into_iter()
        .map(|value| match value {
            None | Some(mysql_async::Value::NULL) => None,
            Some(mysql_async::Value::Bytes(bytes)) => Some(String::from_utf8_lossy(&bytes).into_owned()),
            Some(other) => Some(other.as_sql(true).trim_matches('\'').to_owned()),
        })
        .collect()
}

pub(crate) async fn sets(conn: &mut Conn, statements: &[String]) -> Result<Vec<Vec<Vec<Option<String>>>>, DbError> {
    if statements.is_empty() {
        return Ok(Vec::new());
    }
    let read = async {
        let mut result = conn.query_iter(statements.join(";\n")).await?;
        let mut sets = Vec::with_capacity(statements.len());
        loop {
            let rows: Vec<mysql_async::Row> = result.collect().await?;
            sets.push(rows.into_iter().map(cells).collect());
            if result.is_empty() {
                break;
            }
        }
        Ok::<_, mysql_async::Error>(sets)
    };
    read.await.map_err(|failed| error(&failed))
}

fn matched_rows(info: &str) -> Option<u64> {
    let rest = &info[info.find("Rows matched:")? + "Rows matched:".len()..];
    rest.split_whitespace().next()?.parse().ok()
}

struct ResultColumns {
    columns: Arc<[mysql_async::Column]>,
    names: Vec<String>,
}

impl ResultColumns {
    fn of(columns: Arc<[mysql_async::Column]>) -> Self {
        let names = columns.iter().map(decode::type_name).collect();
        Self { columns, names }
    }

    fn columns(&self) -> Vec<Column> {
        self.columns
            .iter()
            .zip(&self.names)
            .map(|(column, type_name)| {
                let (table, origin) = (column.org_table_str(), column.org_name_str());
                let known = !table.is_empty() && !origin.is_empty();
                Column {
                    source: known.then(|| ColumnSource {
                        table: crate::dialect::table_id(&column.schema_str(), &table),
                        position: 0,
                    }),
                    origin: known.then(|| origin.into_owned()),
                    ..Column::new(column.name_str().into_owned(), type_name.clone())
                }
            })
            .collect()
    }

    fn binary_values(&self, row: mysql_async::Row) -> Vec<Value> {
        row.unwrap_raw()
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let (Some(column), Some(type_name)) = (self.columns.get(index), self.names.get(index)) else {
                    return Value::Null;
                };
                match value {
                    Some(mysql_async::Value::Bytes(bytes)) => decode::decode(column, type_name, Some(&bytes)),
                    None | Some(mysql_async::Value::NULL) => Value::Null,
                    Some(other) => {
                        let text = other.as_sql(true);
                        decode::decode(column, type_name, Some(text.trim_matches('\'').as_bytes()))
                    }
                }
            })
            .collect()
    }

    fn values(&self, row: mysql_async::Row) -> Vec<Value> {
        row.unwrap_raw()
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let (Some(column), Some(type_name)) = (self.columns.get(index), self.names.get(index)) else {
                    return Value::Null;
                };
                match value {
                    Some(mysql_async::Value::Bytes(bytes)) => decode::decode(column, type_name, Some(&bytes)),
                    None | Some(mysql_async::Value::NULL) => Value::Null,
                    Some(other) => decode::decode(column, type_name, Some(other.as_sql(true).as_bytes())),
                }
            })
            .collect()
    }
}

struct Cancel {
    opts: Opts,
    id: u32,
}

impl Canceller for Cancel {
    fn cancel(&self) -> BoxFuture<'static, Result<(), DbError>> {
        let (opts, id) = (self.opts.clone(), self.id);
        Box::pin(async move {
            let mut conn = Conn::new(opts).await.map_err(|failed| error(&failed))?;
            let killed = conn.query_drop(format!("KILL QUERY {id}")).await.map_err(|failed| error(&failed));
            let _ = conn.disconnect().await;
            killed
        })
    }
}

struct Session {
    conn: Conn,
    driver: String,
    dialect: Dialect,
    version: Option<String>,
    cancel: Arc<Cancel>,
    state: TrackedState,
    in_transaction: bool,
    select_limit: Option<u64>,
    notices: Vec<Notice>,
    busy: bool,
    lost: bool,
    show_system_objects: bool,
    raw_version: String,
    identified: bool,
    fingerprints: bool,
    named_profile: bool,
    host: Option<String>,
}

fn options(spec: &ConnectSpec, tls: bool) -> Opts {
    let source = &spec.source;
    let host = source.host.clone().unwrap_or_else(|| "localhost".to_owned());
    let (address, port) = match spec.via {
        Some(via) => (via.ip().to_string(), via.port()),
        None => (host.clone(), source.port.unwrap_or(3306)),
    };
    let builder = OptsBuilder::default()
        .ip_or_hostname(address)
        .tcp_port(port)
        .prefer_socket(false)
        .user(Some(source.user.clone().unwrap_or_else(|| "root".to_owned())))
        .pass(spec.password.as_ref().map(|password| password.expose().to_owned()))
        .db_name(source.database.clone())
        .tcp_keepalive(keep_alive(source))
        .tcp_nodelay(true)
        .stmt_cache_size(0)
        .connect_attribute("program_name", spec.guards.application_name.clone())
        .ssl_opts(if tls { tls::options(&source.tls, &host) } else { None });
    Opts::from(builder)
}

async fn connect(opts: Opts, timeout: Duration) -> Result<Conn, mysql_async::Error> {
    match tokio::time::timeout(timeout, Conn::new(opts)).await {
        Ok(connected) => connected,
        Err(_) => Err(mysql_async::Error::Io(mysql_async::IoError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "connecting timed out",
        )))),
    }
}

pub fn shown_version(version: &str, comment: &str) -> String {
    let shown = server_name(version);
    match crate::profile::marked_in(ID, &format!("{version} {comment}"), &shown) {
        Some(name) => format!("{shown} ({name})"),
        None => shown,
    }
}

fn server_name(version: &str) -> String {
    match version.find("-MariaDB") {
        Some(end) => {
            let version = version[..end].rsplit('-').next().unwrap_or(&version[..end]);
            format!("MariaDB {version}")
        }
        None => format!("MySQL {}", version.split('-').next().unwrap_or(version)),
    }
}

impl Session {
    async fn open(spec: &ConnectSpec, driver: String) -> Result<Self, DbError> {
        tls::install_provider();
        let source = &spec.source;
        let mut opts = options(spec, source.tls.mode != TlsMode::Disable);
        let conn = match connect(opts.clone(), spec.timeout).await {
            Err(mysql_async::Error::Driver(mysql_async::DriverError::NoClientSslFlagFromServer))
                if source.tls.mode == TlsMode::Prefer =>
            {
                opts = options(spec, false);
                connect(opts.clone(), spec.timeout).await
            }
            other => other,
        };
        let conn = conn.map_err(|failed| error(&failed))?;
        let cancel = Arc::new(Cancel { opts, id: conn.id() });
        let mut session = Session {
            conn,
            driver,
            dialect: Dialect { mariadb: false, backslashes: true },
            version: None,
            cancel,
            state: TrackedState { auto_commit: true, ..TrackedState::default() },
            in_transaction: false,
            select_limit: None,
            notices: Vec::new(),
            busy: false,
            lost: false,
            show_system_objects: source.navigator.show_system_objects,
            raw_version: String::new(),
            identified: false,
            fingerprints: crate::profile::of_source(source)
                .or_else(|| source.host.as_deref().and_then(|host| crate::profile::for_host(Some(ID), host)))
                .is_none_or(|profile| profile.quirks.fingerprints),
            named_profile: crate::profile::of_source(source).is_some(),
            host: source.host.clone(),
        };
        let about = sets(&mut session.conn, &["SELECT VERSION(), @@SESSION.sql_mode".to_owned()]).await?;
        let row = about.first().and_then(|rows| rows.first()).cloned().unwrap_or_default();
        let version = row.first().cloned().flatten().unwrap_or_default();
        let mode = row.get(1).cloned().flatten().unwrap_or_default().to_ascii_uppercase();
        session.dialect =
            Dialect { mariadb: version.contains("MariaDB"), backslashes: !mode.contains("NO_BACKSLASH_ESCAPES") };
        session.version = (!version.is_empty()).then(|| server_name(&version));
        session.raw_version = version;
        let mut guards = Vec::new();
        if let Some(timeout) = spec.guards.statement_timeout {
            guards.push(if session.dialect.mariadb {
                format!("SET SESSION max_statement_time = {}", timeout.as_secs_f64())
            } else {
                format!("SET SESSION max_execution_time = {}", timeout.as_millis())
            });
        }
        if let Some(timeout) = spec.guards.lock_timeout {
            let seconds = whole_seconds(timeout).max(1);
            guards.push(format!("SET SESSION lock_wait_timeout = {seconds}, innodb_lock_wait_timeout = {seconds}"));
        }
        if let Some(timeout) = spec.guards.idle_in_transaction_timeout
            && session.dialect.mariadb
        {
            guards.push(format!("SET SESSION idle_transaction_timeout = {}", whole_seconds(timeout).max(1)));
        }
        if spec.state.read_only && spec.context != ContextKind::Metadata {
            guards.push("SET SESSION TRANSACTION READ ONLY".to_owned());
        }
        sets(&mut session.conn, &guards).await?;
        for statement in &source.bootstrap.init_statements {
            session.conn.query_drop(statement.as_str()).await.map_err(|failed| failed_statement(&failed, statement))?;
        }
        session.apply_state(&spec.state).await?;
        Ok(session)
    }

    fn enter(&mut self) -> Result<(), DbError> {
        if self.busy || self.lost {
            self.lost = true;
            return Err(DbError::new(ErrorKind::Connection, "the connection was left in the middle of an operation"));
        }
        self.busy = true;
        Ok(())
    }

    fn leave<T>(&mut self, result: Result<T, mysql_async::Error>, statement: &str) -> Result<T, DbError> {
        self.busy = false;
        self.sync_transaction();
        result.map_err(|failed| {
            if failed.is_fatal() {
                self.lost = true;
            }
            if let mysql_async::Error::Server(server) = &failed
                && server.code == 1213
            {
                self.in_transaction = false;
            }
            failed_statement(&failed, statement)
        })
    }

    async fn settled<T>(&mut self, left: Result<T, DbError>) -> Result<T, DbError> {
        if left.is_err() && self.in_transaction && !self.lost && self.conn.query_drop("DO 0").await.is_ok() {
            self.sync_transaction();
        }
        left
    }

    fn sync_transaction(&mut self) {
        if let Some(ok) = self.conn.last_ok_packet() {
            self.in_transaction = ok.status_flags().contains(mysql_async::consts::StatusFlags::SERVER_STATUS_IN_TRANS);
        }
    }

    async fn limit(conn: &mut Conn, current: &mut Option<u64>, wanted: Option<u64>) -> Result<(), mysql_async::Error> {
        if *current == wanted {
            return Ok(());
        }
        let sql = match wanted {
            Some(rows) => format!("SET SESSION sql_select_limit = {rows}"),
            None => "SET SESSION sql_select_limit = DEFAULT".to_owned(),
        };
        conn.query_drop(sql).await?;
        *current = wanted;
        Ok(())
    }

    fn select_limit(statement: &str, rows: u64) -> Option<u64> {
        (crate::sql::pageable(&strip_hash_comments(statement)) && rows < (1 << 62)).then_some(rows)
    }

    async fn warnings(&mut self, count: u16) {
        if count == 0 {
            return;
        }
        let Ok(found) = sets(&mut self.conn, &["SHOW WARNINGS".to_owned()]).await else { return };
        for row in found.into_iter().flatten().take(50) {
            self.notices.push(Notice {
                severity: row.first().cloned().flatten().unwrap_or_default().to_ascii_uppercase(),
                message: row.get(2).cloned().flatten().unwrap_or_default(),
                code: row.get(1).cloned().flatten(),
            });
        }
    }

    async fn apply_state(&mut self, state: &TrackedState) -> Result<(), DbError> {
        let mut statements = Vec::new();
        if let Some(schema) = state.schema.as_ref().or(state.database.as_ref()) {
            statements.push(format!("USE {}", quote_ident(schema)));
        }
        if let Some(isolation) = &state.isolation {
            let level = ISOLATION_LEVELS
                .iter()
                .find(|level| level.eq_ignore_ascii_case(isolation))
                .ok_or_else(|| DbError::new(ErrorKind::Invalid, isolation))?;
            statements.push(format!("SET SESSION TRANSACTION ISOLATION LEVEL {}", level.to_uppercase()));
        }
        statements
            .push(format!("SET SESSION TRANSACTION {}", if state.read_only { "READ ONLY" } else { "READ WRITE" }));
        statements.push(format!("SET autocommit = {}", u8::from(state.auto_commit)));
        sets(&mut self.conn, &statements).await?;
        self.state = state.clone();
        self.sync_transaction();
        if state.auto_commit {
            self.in_transaction = false;
        }
        Ok(())
    }

    async fn run(&mut self, statement: &str, options: &ExecOptions) -> Result<Execution, DbError> {
        self.enter()?;
        let limit = Self::select_limit(statement, (options.max_rows as u64).saturating_add(1));
        let (conn, current) = (&mut self.conn, &mut self.select_limit);
        let read = async {
            Self::limit(conn, current, limit).await?;
            let mut result = conn.query_iter(statement).await?;
            let mut execution = Execution::default();
            let mut taken = false;
            loop {
                let typed = result.columns().filter(|columns| !columns.is_empty() && !taken).map(ResultColumns::of);
                if let Some(typed) = &typed {
                    execution.columns = typed.columns();
                    taken = true;
                }
                while let Some(row) = result.next().await? {
                    let Some(typed) = &typed else { continue };
                    if execution.rows.len() >= options.max_rows {
                        execution.truncated = true;
                        continue;
                    }
                    execution.rows.push(typed.values(row));
                }
                if result.is_empty() {
                    break;
                }
            }
            if !taken {
                execution.affected = Some(result.affected_rows());
            }
            Ok::<_, mysql_async::Error>((execution, result.warnings()))
        };
        let read = read.await;
        let left = self.leave(read, statement);
        let (mut execution, warnings) = self.settled(left).await?;
        self.warnings(warnings).await;
        execution.notices = self.take_notices();
        Ok(execution)
    }

    async fn streamed(
        &mut self,
        statement: &str,
        params: &[Value],
        options: &StreamOptions,
        sink: &mut dyn RowSink,
    ) -> Result<StreamEnd, DbError> {
        self.enter()?;
        let wanted_rows = options.skip.saturating_add(sink.cap().max(1) as u64).saturating_add(1);
        let limit = Self::select_limit(statement, wanted_rows);
        let (conn, current) = (&mut self.conn, &mut self.select_limit);
        let read = async {
            Self::limit(conn, current, limit).await?;
            if params.is_empty() {
                let mut result = conn.query_iter(statement).await?;
                let end = read_rows(&mut result, options, sink, false).await?;
                Ok::<_, mysql_async::Error>((end, result.warnings()))
            } else {
                let values: Vec<mysql_async::Value> = params.iter().map(bound_value).collect();
                let mut result = conn.exec_iter(statement, mysql_async::Params::Positional(values)).await?;
                let end = read_rows(&mut result, options, sink, true).await?;
                Ok((end, result.warnings()))
            }
        };
        let read = read.await;
        let left = self.leave(read, statement);
        let (mut end, warnings) = self.settled(left).await?;
        self.warnings(warnings).await;
        end.notices = self.take_notices();
        Ok(end)
    }

    async fn bound(&mut self, statement: &str, params: &[Param]) -> Result<Execution, DbError> {
        self.enter()?;
        let (conn, current) = (&mut self.conn, &mut self.select_limit);
        let values: Vec<mysql_async::Value> = params
            .iter()
            .map(|param| match param {
                Some(text) => mysql_async::Value::Bytes(text.clone().into_bytes()),
                None => mysql_async::Value::NULL,
            })
            .collect();
        let params =
            if values.is_empty() { mysql_async::Params::Empty } else { mysql_async::Params::Positional(values) };
        let read = async {
            Self::limit(conn, current, None).await?;
            let mut result = conn.exec_iter(statement, params).await?;
            let mut execution = Execution::default();
            let mut taken = false;
            let mut affected = None;
            loop {
                let typed = result.columns().filter(|columns| !columns.is_empty() && !taken).map(ResultColumns::of);
                if let Some(typed) = &typed {
                    execution.columns = typed.columns();
                    taken = true;
                }
                while let Some(row) = result.next().await? {
                    if let Some(typed) = &typed {
                        execution.rows.push(typed.binary_values(row));
                    }
                }
                if affected.is_none() {
                    affected = Some(matched_rows(&result.info()).unwrap_or_else(|| result.affected_rows()));
                }
                if result.is_empty() {
                    break;
                }
            }
            if !taken {
                execution.affected = affected;
            }
            Ok::<_, mysql_async::Error>((execution, result.warnings()))
        };
        let read = read.await;
        let left = self.leave(read, statement);
        let (mut execution, warnings) = self.settled(left).await?;
        self.warnings(warnings).await;
        execution.notices = self.take_notices();
        Ok(execution)
    }

    async fn begin_internal(&mut self) -> Result<(), DbError> {
        self.enter()?;
        let reset = Self::limit(&mut self.conn, &mut self.select_limit, None).await;
        self.busy = reset.is_ok();
        reset.map_err(|failed| {
            self.lost |= failed.is_fatal();
            error(&failed)
        })
    }

    fn end_internal<T>(&mut self, result: Result<T, DbError>) -> Result<T, DbError> {
        self.busy = false;
        self.sync_transaction();
        if result.as_ref().is_err_and(|failed| matches!(failed.kind, ErrorKind::Connection | ErrorKind::Tls)) {
            self.lost = true;
        }
        result
    }

    async fn identify(&mut self) {
        if std::mem::replace(&mut self.identified, true) || self.raw_version.is_empty() {
            return;
        }
        let comment = sets(&mut self.conn, &["SELECT @@version_comment".to_owned()])
            .await
            .ok()
            .and_then(|read| read.into_iter().next())
            .and_then(|rows| rows.into_iter().next())
            .and_then(|row| row.into_iter().next().flatten())
            .unwrap_or_default();
        let shown = shown_version(&self.raw_version, &comment);
        if !self.named_profile
            && let Some(profile) = crate::profile::detect(&self.driver, &shown, self.host.as_deref())
        {
            self.fingerprints = profile.quirks.fingerprints;
        }
        self.version = Some(shown);
    }

    async fn read_structure(
        &mut self,
        request: &IntrospectRequest,
        previous: Option<&Snapshot>,
    ) -> Result<Introspection, DbError> {
        self.identify().await;
        self.begin_internal().await?;
        let read = introspect::introspect(&mut self.conn, self.dialect, request, previous, self.fingerprints).await;
        self.end_internal(read)
    }

    async fn nodes(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        let mut path = path.to_vec();
        if let Some(first) = path.first_mut() {
            CATALOG.clone_into(first);
        }
        let request = IntrospectRequest {
            schemas: path.get(1).cloned().into_iter().collect(),
            objects: match path.as_slice() {
                [_, schema, _, key, ..] => vec![(schema.clone(), key.clone())],
                _ => Vec::new(),
            },
            force: true,
            system: true,
            large_schema: LARGE_SCHEMA,
            outline: path.len() < 2,
        };
        let read = self.read_structure(&request, None).await?;
        let mut snapshot = Snapshot::new("", self.driver.clone());
        snapshot.catalogs = read.catalogs;
        let options = TreeOptions { show_system: self.show_system_objects, ..TreeOptions::default() };
        Ok(crate::schema::children(&snapshot, &path, &options).unwrap_or_default())
    }

    async fn control(&mut self, statement: &str) -> Result<(), DbError> {
        self.enter()?;
        let done = self.conn.query_drop(statement).await;
        self.leave(done, statement)?;
        if !self.state.auto_commit {
            self.in_transaction = false;
        }
        Ok(())
    }
}

fn bound_value(value: &Value) -> mysql_async::Value {
    match value {
        Value::Null => mysql_async::Value::NULL,
        Value::Bool(value) => mysql_async::Value::Int(i64::from(*value)),
        Value::Integer(value) => mysql_async::Value::Int(*value),
        Value::Float(value) => mysql_async::Value::Double(*value),
        Value::Bytes(bytes) => mysql_async::Value::Bytes(bytes.clone()),
        other => mysql_async::Value::Bytes(crate::edit::param_text(other).unwrap_or_default().into_bytes()),
    }
}

async fn read_rows<P: mysql_async::prelude::Protocol>(
    result: &mut mysql_async::QueryResult<'_, '_, P>,
    options: &StreamOptions,
    sink: &mut dyn RowSink,
    binary: bool,
) -> Result<StreamEnd, mysql_async::Error> {
    let page = options.page.max(1);
    let mut taken = false;
    let mut dropped = false;
    loop {
        let typed = result.columns().filter(|columns| !columns.is_empty() && !taken).map(ResultColumns::of);
        let mut wanted = 0;
        if let Some(typed) = &typed {
            sink.columns(typed.columns());
            taken = true;
            wanted = sink.want().await;
        }
        let mut skip = options.skip;
        let mut batch = Vec::new();
        let mut keeping = true;
        while let Some(row) = result.next().await? {
            let Some(typed) = &typed else { continue };
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if !keeping || wanted == 0 {
                dropped = true;
                continue;
            }
            batch.push(if binary { typed.binary_values(row) } else { typed.values(row) });
            wanted -= 1;
            if batch.len() >= page || wanted == 0 {
                keeping = sink.push(std::mem::take(&mut batch));
                if keeping && wanted == 0 {
                    wanted = sink.want().await;
                }
            }
        }
        if !batch.is_empty() && keeping {
            sink.push(batch);
        }
        if result.is_empty() {
            break;
        }
    }
    let affected = (!taken).then(|| result.affected_rows());
    Ok(StreamEnd { affected, complete: !dropped, notices: Vec::new() })
}

impl DriverSession for Session {
    fn execute<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a ExecOptions,
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(self.run(statement, options))
    }

    fn execute_bound<'a>(
        &'a mut self,
        statement: &'a str,
        params: &'a [Param],
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(self.bound(statement, params))
    }

    fn stream<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a StreamOptions,
        sink: &'a mut dyn RowSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        Box::pin(self.streamed(statement, &[], options, sink))
    }

    fn stream_bound<'a>(
        &'a mut self,
        statement: &'a str,
        params: &'a [Value],
        options: &'a StreamOptions,
        sink: &'a mut dyn RowSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        Box::pin(self.streamed(statement, params, options, sink))
    }

    fn children<'a>(&'a mut self, path: &'a [String]) -> BoxFuture<'a, Result<Vec<Node>, DbError>> {
        Box::pin(self.nodes(path))
    }

    fn introspect<'a>(
        &'a mut self,
        request: &'a IntrospectRequest,
        previous: Option<&'a Snapshot>,
    ) -> BoxFuture<'a, Result<Introspection, DbError>> {
        Box::pin(self.read_structure(request, previous))
    }

    fn sessions<'a>(
        &'a mut self,
        request: &'a crate::sessions::SessionsRequest,
    ) -> BoxFuture<'a, Result<crate::sessions::ServerSessions, DbError>> {
        Box::pin(async move {
            self.begin_internal().await?;
            let listed = sessions::list(&mut self.conn, self.dialect.mariadb, request).await;
            self.end_internal(listed)
        })
    }

    fn end_session<'a>(
        &'a mut self,
        id: &'a str,
        end: crate::sessions::SessionEnd,
    ) -> BoxFuture<'a, Result<crate::sessions::Ended, DbError>> {
        Box::pin(async move {
            self.begin_internal().await?;
            let ended = sessions::end(&mut self.conn, id, end).await;
            self.end_internal(ended)
        })
    }

    fn server_id(&self) -> Option<String> {
        Some(self.cancel.id.to_string())
    }

    fn ping(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            self.enter()?;
            let answered = self.conn.ping().await;
            self.leave(answered, "")
        })
    }

    fn server_version(&mut self) -> BoxFuture<'_, Result<Option<String>, DbError>> {
        Box::pin(async move {
            self.identify().await;
            Ok(self.version.clone())
        })
    }

    fn apply<'a>(&'a mut self, state: &'a TrackedState) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            self.enter()?;
            let applied = self.apply_state(state).await;
            self.busy = false;
            applied
        })
    }

    fn commit(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(self.control("COMMIT"))
    }

    fn rollback(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(self.control("ROLLBACK"))
    }

    fn in_transaction(&self) -> bool {
        self.in_transaction
    }

    fn is_closed(&self) -> bool {
        self.busy || self.lost
    }

    fn canceller(&self) -> Option<Arc<dyn Canceller>> {
        Some(Arc::clone(&self.cancel) as Arc<dyn Canceller>)
    }

    fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_reads_are_told_from_writes() {
        assert_eq!(classify("SELECT * FROM t"), StatementKind::Read);
        assert_eq!(classify("# note\nSHOW TABLES"), StatementKind::Read);
        assert_eq!(classify("/* x */ DESCRIBE t"), StatementKind::Read);
        assert_eq!(classify("SELECT '#not a comment', a FROM t"), StatementKind::Read);
        assert_eq!(classify("SELECT * FROM t FOR UPDATE"), StatementKind::Write);
        assert_eq!(classify("SELECT * FROM t LOCK IN SHARE MODE"), StatementKind::Write);
        assert_eq!(classify("SELECT a INTO @x FROM t"), StatementKind::Write);
        assert_eq!(classify("SELECT GET_LOCK('a', 1)"), StatementKind::Write);
        assert_eq!(classify("REPLACE INTO t VALUES (1)"), StatementKind::Write);
        assert_eq!(classify("SET @a = 1"), StatementKind::Write);
        assert!(changes_structure("-- x\nALTER TABLE t ADD c int"));
        assert!(changes_structure("RENAME TABLE a TO b"));
        assert!(!changes_structure("SELECT 'create'"));
    }

    #[test]
    fn literals_and_versions_are_written_for_the_server() {
        let escaping = Dialect { mariadb: false, backslashes: true };
        assert_eq!(escaping.literal("it's a \\ b"), "'it''s a \\\\ b'");
        let plain = Dialect { mariadb: false, backslashes: false };
        assert_eq!(plain.literal("a\\b"), "'a\\b'");
        assert_eq!(plain.list(&["a".into(), "b".into()]), "'a', 'b'");
        assert_eq!(server_name("8.4.3"), "MySQL 8.4.3");
        assert_eq!(server_name("11.4.4-MariaDB-ubu2404"), "MariaDB 11.4.4");
        assert_eq!(server_name("5.5.5-10.11.6-MariaDB-log"), "MariaDB 10.11.6");
        assert_eq!(shown_version("8.4.3", "MySQL Community Server - GPL"), "MySQL 8.4.3");
        for (version, comment, shown, profile) in [
            ("8.0.11-TiDB-v7.5.1", "", "MySQL 8.0.11 (TiDB)", "tidb"),
            (
                "8.0.36-28",
                "Percona Server (GPL), Release 28, Revision 47601f19",
                "MySQL 8.0.36 (Percona Server for MySQL)",
                "percona",
            ),
            ("8.0.30-Vitess", "", "MySQL 8.0.30 (Vitess)", "vitess"),
            ("5.1.0", "StarRocks version 3.3.5-6d81f75", "MySQL 5.1.0 (StarRocks)", "starrocks"),
            ("5.7.99", "Doris version doris-2.1.7-rc03-443e87e203", "MySQL 5.7.99 (Apache Doris)", "doris"),
            ("8.0.32", "SingleStore Helios", "MySQL 8.0.32 (SingleStore)", "singlestore"),
        ] {
            let shown_now = shown_version(version, comment);
            assert_eq!(shown_now, shown);
            assert_eq!(crate::profile::detect(ID, &shown_now, None).map(|found| found.id), Some(profile));
        }
        assert_eq!(quote_ident("we`ird"), "`we``ird`");
        assert_eq!(matched_rows("Rows matched: 1  Changed: 0  Warnings: 0"), Some(1));
        assert_eq!(matched_rows("Records: 2  Duplicates: 0  Warnings: 0"), None);
    }

    #[test]
    fn syntax_errors_point_at_their_place() {
        let statement = "SELECT 1\nFROM t WHERE x = = 2";
        let message = "You have an error in your SQL syntax; check the manual that corresponds to your MySQL server version for the right syntax to use near '= 2' at line 2";
        assert_eq!(position(statement, message), Some(statement.find("= 2").unwrap() + 1));
        assert_eq!(position(statement, "near '' at line 1"), None);
        assert_eq!(server_error_kind(1045), ErrorKind::Authentication);
        assert_eq!(server_error_kind(3024), ErrorKind::Timeout);
    }
}

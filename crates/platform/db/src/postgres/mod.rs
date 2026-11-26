mod catalogue;
mod ddl;
mod decode;
mod introspect;
mod sessions;
mod tls;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio_postgres::config::{SslMode, SslNegotiation};
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{AsyncMessage, CancelToken, Client, Config};
use tokio_postgres_rustls::MakeRustlsConnect;

use crate::driver::{
    BoxFuture, ByteSink, Canceller, ConnectSpec, ContextKind, DbError, Driver, DriverSession, ErrorKind, ExecOptions,
    Param, RowSink, StreamEnd, StreamOptions, TrackedState,
};
use crate::explain::{ExplainOption, ExplainOptions, ExplainRequest};
use crate::model::{Capabilities, DataModel, Node, NodeKind, SettingField, StatementKind};
use crate::profile::{Catalogue, Quirks};
use crate::registry::{DataSource, POOLER, TlsMode};
use crate::schema::{DdlOptions, IntrospectRequest, Introspection, LARGE_SCHEMA, Snapshot, TreeOptions};
use crate::value::{Column, ColumnSource, Execution, Notice, Value};

pub const ROLLED_BACK: &str = "rolledBack";

pub const ID: &str = "postgresql";

pub const KEEP_ALIVE: &str = "keepAlive";

#[derive(Default)]
pub struct Postgres;

fn capabilities() -> &'static Capabilities {
    static CAPABILITIES: OnceLock<Capabilities> = OnceLock::new();
    CAPABILITIES.get_or_init(|| Capabilities {
        models: vec![DataModel::Relational],
        introspection: true,
        language: "sql:postgres".to_owned(),
        transactions: true,
        isolation_levels: ["read committed", "repeatable read", "serializable"].map(str::to_owned).to_vec(),
        catalogs: true,
        schemas: true,
        node_kinds: vec![
            NodeKind::Catalog,
            NodeKind::Schema,
            NodeKind::Folder,
            NodeKind::Table,
            NodeKind::View,
            NodeKind::Column,
            NodeKind::Index,
            NodeKind::Other("foreignTable".to_owned()),
            NodeKind::Other("materializedView".to_owned()),
            NodeKind::Other("function".to_owned()),
            NodeKind::Other("procedure".to_owned()),
            NodeKind::Other("sequence".to_owned()),
            NodeKind::Other("type".to_owned()),
            NodeKind::Other("constraint".to_owned()),
            NodeKind::Other("foreignKey".to_owned()),
            NodeKind::Other("trigger".to_owned()),
            NodeKind::Other("reference".to_owned()),
            NodeKind::Other("policy".to_owned()),
        ],
        explain: true,
        server_cancel: true,
        bind_parameters: true,
        row_identity: Some("primaryKey".to_owned()),
        ddl_preview: true,
        server_paging: true,
        sessions: true,
        read_only_sessions: true,
        uri_schemes: vec!["postgresql".to_owned(), "postgres".to_owned()],
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
                key: POOLER.to_owned(),
                kind: "bool".to_owned(),
                choices: Vec::new(),
                default: Some("false".to_owned()),
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

impl Driver for Postgres {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "PostgreSQL"
    }

    fn capabilities(&self) -> &Capabilities {
        capabilities()
    }

    fn default_port(&self) -> Option<u16> {
        Some(5432)
    }

    fn classify(&self, statement: &str) -> StatementKind {
        classify(statement)
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> BoxFuture<'a, Result<Box<dyn DriverSession>, DbError>> {
        Box::pin(async move { Ok(Box::new(Session::open(spec).await?) as Box<dyn DriverSession>) })
    }

    fn changes_structure(&self, statement: &str) -> bool {
        changes_structure(statement)
    }

    fn ddl(&self, snapshot: &Snapshot, path: &[String], options: &DdlOptions) -> Result<String, DbError> {
        ddl::generate(snapshot, path, options)
    }
    fn explain_options(&self) -> Vec<ExplainOption> {
        crate::explain::postgres_options()
    }

    fn explain(&self, statement: &str, options: &ExplainOptions) -> Result<ExplainRequest, DbError> {
        Ok(crate::explain::postgres(statement, options))
    }
}

pub fn changes_structure(statement: &str) -> bool {
    let words = first_words(statement);
    matches!(
        words.first().map(String::as_str),
        Some(
            "create"
                | "alter"
                | "drop"
                | "comment"
                | "grant"
                | "revoke"
                | "truncate"
                | "reindex"
                | "cluster"
                | "security"
                | "import"
        )
    ) || words.first().is_some_and(|word| word == "refresh") && words.get(1).is_some_and(|word| word == "materialized")
}

pub fn classify(statement: &str) -> StatementKind {
    let text = strip_comments(statement).to_ascii_lowercase();
    let words: Vec<&str> =
        text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|word| !word.is_empty()).collect();
    let reads = matches!(words.first().copied(), Some("select" | "show" | "values" | "table" | "with" | "explain"));
    let writes = words.iter().any(|word| {
        matches!(
            *word,
            "insert"
                | "update"
                | "delete"
                | "merge"
                | "into"
                | "nextval"
                | "setval"
                | "analyze"
                | "lock"
                | "copy"
                | "pg_advisory_lock"
                | "pg_advisory_xact_lock"
                | "pg_terminate_backend"
                | "pg_cancel_backend"
                | "set_config"
                | "dblink_exec"
                | "lo_import"
                | "lo_export"
                | "lo_unlink"
        )
    }) || text.contains("for update")
        || text.contains("for share")
        || text.contains("for no key update")
        || text.contains("for key share");
    if reads && !writes { StatementKind::Read } else { StatementKind::Write }
}

fn strip_comments(statement: &str) -> String {
    let mut out = String::with_capacity(statement.len());
    let mut rest = statement;
    loop {
        let line = rest.find("--");
        let block = rest.find("/*");
        match (line, block) {
            (Some(at), block) if block.is_none_or(|block| at < block) => {
                out.push_str(&rest[..at]);
                rest = rest[at..].find('\n').map_or("", |end| &rest[at + end..]);
            }
            (_, Some(at)) => {
                out.push_str(&rest[..at]);
                out.push(' ');
                rest = rest[at + 2..].find("*/").map_or("", |end| &rest[at + 2 + end + 2..]);
            }
            _ => {
                out.push_str(rest);
                return out;
            }
        }
    }
}

fn first_words(statement: &str) -> Vec<String> {
    strip_comments(statement)
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(2)
        .map(str::to_ascii_lowercase)
        .collect()
}

fn error(error: &tokio_postgres::Error) -> DbError {
    if let Some(db) = error.as_db_error() {
        let code = db.code().code();
        let kind = if matches!(code, "28P01" | "28000") { ErrorKind::Authentication } else { ErrorKind::Server };
        let mut failed = DbError::new(kind, db.message()).with_code(code);
        if let Some(tokio_postgres::error::ErrorPosition::Original(at)) = db.position() {
            failed = failed.at(*at as usize);
        }
        return failed;
    }
    let mut detail = error.to_string();
    let statement = (detail.starts_with("expected ") && detail.contains(" parameters but got "))
        || detail.starts_with("error serializing parameter")
        || detail.starts_with("error deserializing column")
        || detail.starts_with("invalid column")
        || detail == "query returned an unexpected number of columns";
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        let text = cause.to_string();
        if !detail.contains(&text) {
            detail.push_str(": ");
            detail.push_str(&text);
        }
        source = cause.source();
    }
    let lower = detail.to_ascii_lowercase();
    let kind = if statement {
        ErrorKind::Server
    } else if lower.contains("timed out") || lower.contains("timeout") {
        ErrorKind::Timeout
    } else if lower.contains("tls") || lower.contains("certificate") || lower.contains("ssl") {
        ErrorKind::Tls
    } else if lower.contains("password") || lower.contains("authentication") || lower.contains("scram") {
        ErrorKind::Authentication
    } else {
        ErrorKind::Connection
    };
    DbError::new(kind, detail)
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

fn column_types(columns: &[tokio_postgres::Column]) -> Vec<(String, Type)> {
    columns.iter().map(|column| (column.name().to_owned(), column.type_().clone())).collect()
}

fn as_columns(columns: &[tokio_postgres::Column]) -> Vec<Column> {
    columns
        .iter()
        .map(|column| Column {
            name: column.name().to_owned(),
            type_name: column.type_().name().to_owned(),
            source: column
                .table_oid()
                .filter(|table| *table != 0)
                .zip(column.column_id())
                .map(|(table, position)| ColumnSource { table, position }),
            origin: None,
        })
        .collect()
}

struct TextParam<'a>(Option<&'a str>);

impl std::fmt::Debug for TextParam<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "TextParam(..)" } else { "TextParam(NULL)" })
    }
}

impl ToSql for TextParam<'_> {
    fn to_sql(
        &self,
        _ty: &Type,
        out: &mut bytes::BytesMut,
    ) -> Result<tokio_postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self.0 {
            Some(text) => {
                out.extend_from_slice(text.as_bytes());
                Ok(tokio_postgres::types::IsNull::No)
            }
            None => Ok(tokio_postgres::types::IsNull::Yes),
        }
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }

    fn encode_format(&self, _ty: &Type) -> tokio_postgres::types::Format {
        tokio_postgres::types::Format::Text
    }

    tokio_postgres::types::to_sql_checked!();
}

fn values(row: &tokio_postgres::Row, types: &[(String, Type)], zone: &decode::Zone) -> Vec<Value> {
    (0..row.len())
        .map(|index| {
            let raw = row.try_get::<_, Option<decode::Raw<'_>>>(index).ok().flatten().map(|raw| raw.0);
            match types.get(index) {
                Some((_, ty)) => decode::decode(ty, raw, zone),
                None => Value::Null,
            }
        })
        .collect()
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn seconds(duration: Duration) -> String {
    format!("{}ms", duration.as_millis())
}

struct Cancel {
    token: CancelToken,
    tls: MakeRustlsConnect,
}

impl Canceller for Cancel {
    fn cancel(&self) -> BoxFuture<'static, Result<(), DbError>> {
        let (token, tls) = (self.token.clone(), self.tls.clone());
        Box::pin(async move { token.cancel_query(tls).await.map_err(|failed| error(&failed)) })
    }
}

#[derive(Default)]
struct Capped {
    columns: Vec<Column>,
    rows: Vec<Vec<Value>>,
    cap: usize,
    truncated: bool,
}

impl RowSink for Capped {
    fn columns(&mut self, columns: Vec<Column>) {
        self.columns = columns;
    }

    fn portal(&mut self, _in_transaction: bool) {}

    fn want(&mut self) -> BoxFuture<'_, usize> {
        let left = if self.truncated { 0 } else { self.cap.saturating_add(1).saturating_sub(self.rows.len()) };
        Box::pin(async move { left })
    }

    fn push(&mut self, rows: Vec<Vec<Value>>) -> bool {
        self.rows.extend(rows);
        if self.rows.len() > self.cap {
            self.rows.truncate(self.cap);
            self.truncated = true;
        }
        !self.truncated
    }

    fn cap(&self) -> usize {
        self.cap
    }
}

struct Session {
    client: Client,
    cancel: Arc<Cancel>,
    notices: Arc<Mutex<Vec<Notice>>>,
    zone: Arc<Mutex<decode::Zone>>,
    closed: Arc<AtomicBool>,
    state: TrackedState,
    in_transaction: bool,
    settings_in_transaction: bool,
    pooler: bool,
    database: String,
    show_all_databases: bool,
    show_system_objects: bool,
    backend: Option<String>,
    quirks: Quirks,
    named_profile: bool,
    host: Option<String>,
    version: Option<String>,
}

impl Session {
    async fn open(spec: &ConnectSpec) -> Result<Self, DbError> {
        let source = &spec.source;
        let mut config = Config::new();
        config
            .host(source.host.as_deref().unwrap_or("localhost"))
            .port(spec.via.map_or(source.port.unwrap_or(5432), |via| via.port()))
            .user(source.user.as_deref().unwrap_or("postgres"))
            .dbname(source.database.as_deref().unwrap_or("postgres"))
            .application_name(&spec.guards.application_name)
            .connect_timeout(spec.timeout)
            .keepalives(keep_alive(source).is_some())
            .keepalives_idle(keep_alive(source).unwrap_or(Duration::from_secs(30)))
            .keepalives_interval(Duration::from_secs(10))
            .keepalives_retries(3);
        if let Some(password) = &spec.password {
            config.password(password.expose());
        }
        if let Some(via) = spec.via {
            config.hostaddr(via.ip());
        }
        config.ssl_mode(match source.tls.mode {
            TlsMode::Disable => SslMode::Disable,
            TlsMode::Prefer => SslMode::Prefer,
            TlsMode::Require | TlsMode::VerifyCa | TlsMode::VerifyFull => SslMode::Require,
        });
        if source.tls.direct {
            config.ssl_negotiation(SslNegotiation::Direct);
        }
        let tls = tls::connector(&source.tls)?;
        let connecting = config.connect(tls.clone());
        let (client, mut connection) = match tokio::time::timeout(spec.timeout, connecting).await {
            Ok(connected) => connected.map_err(|failed| error(&failed))?,
            Err(_) => return Err(DbError::new(ErrorKind::Timeout, "")),
        };
        let notices: Arc<Mutex<Vec<Notice>>> = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let zone = Arc::new(Mutex::new(decode::Zone::named(connection.parameter("TimeZone").unwrap_or("UTC"))));
        let (kept, ended, followed) = (Arc::clone(&notices), Arc::clone(&closed), Arc::clone(&zone));
        tokio::spawn(async move {
            loop {
                let message = std::future::poll_fn(|cx| {
                    let polled = connection.poll_message(cx);
                    if let Some(name) = connection.parameter("TimeZone") {
                        let mut zone = followed.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        if zone.name() != name {
                            *zone = decode::Zone::named(name);
                        }
                    }
                    polled
                })
                .await;
                match message {
                    Some(Ok(AsyncMessage::Notice(notice))) => {
                        kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(Notice {
                            severity: notice.severity().to_owned(),
                            message: notice.message().to_owned(),
                            code: Some(notice.code().code().to_owned()),
                        });
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break,
                }
            }
            ended.store(true, Ordering::SeqCst);
        });
        let cancel = Arc::new(Cancel { token: client.cancel_token(), tls });
        let pooler = source.flag(POOLER);
        let named = crate::profile::of_source(source);
        let mut session = Session {
            client,
            cancel,
            notices,
            zone,
            closed,
            state: TrackedState { auto_commit: true, ..TrackedState::default() },
            in_transaction: false,
            settings_in_transaction: false,
            pooler,
            database: source.database.clone().unwrap_or_else(|| "postgres".to_owned()),
            show_all_databases: source.navigator.show_all_databases,
            show_system_objects: source.navigator.show_system_objects,
            backend: None,
            quirks: named
                .or_else(|| source.host.as_deref().and_then(|host| crate::profile::for_host(Some(ID), host)))
                .map_or(Quirks::NATIVE, |profile| profile.quirks),
            named_profile: named.is_some(),
            host: source.host.clone(),
            version: None,
        };
        if !pooler {
            let mut guards = Vec::new();
            if let Some(timeout) = spec.guards.statement_timeout {
                guards.push(format!("SET statement_timeout = '{}'", seconds(timeout)));
            }
            if let Some(timeout) = spec.guards.lock_timeout.filter(|_| session.quirks.guards) {
                guards.push(format!("SET lock_timeout = '{}'", seconds(timeout)));
            }
            if let Some(timeout) = spec.guards.idle_in_transaction_timeout.filter(|_| session.quirks.guards) {
                guards.push(format!("SET idle_in_transaction_session_timeout = '{}'", seconds(timeout)));
            }
            if spec.state.read_only && spec.context != ContextKind::Metadata && session.quirks.guards {
                guards.push("SET default_transaction_read_only = on".to_owned());
            }
            guards.push("SELECT pg_backend_pid()".to_owned());
            let messages = session.client.simple_query(&guards.join("; ")).await.map_err(|failed| error(&failed))?;
            session.backend = messages.iter().find_map(|message| match message {
                tokio_postgres::SimpleQueryMessage::Row(row) => row.get(0).map(str::to_owned),
                _ => None,
            });
        }
        for statement in &source.bootstrap.init_statements {
            session.client.batch_execute(statement).await.map_err(|failed| error(&failed))?;
        }
        let state = spec.state.clone();
        session.apply_state(&state).await?;
        Ok(session)
    }

    async fn apply_state(&mut self, state: &TrackedState) -> Result<(), DbError> {
        let statements = self.settings(state)?;
        if !statements.is_empty() {
            self.client.batch_execute(&statements.join("; ")).await.map_err(|failed| error(&failed))?;
        }
        let was_auto = self.state.auto_commit;
        self.state = TrackedState { auto_commit: state.auto_commit || self.pooler, ..state.clone() };
        if self.state.auto_commit && !was_auto && self.in_transaction {
            self.client.batch_execute("COMMIT").await.map_err(|failed| error(&failed))?;
            self.in_transaction = false;
        }
        self.settings_in_transaction |= self.in_transaction && !statements.is_empty();
        if !self.in_transaction {
            self.settings_in_transaction = false;
        }
        Ok(())
    }

    async fn restore_settings(&mut self) -> Result<(), DbError> {
        if !self.settings_in_transaction {
            return Ok(());
        }
        let state = self.state.clone();
        let statements = self.settings(&state)?;
        self.client.batch_execute(&statements.join("; ")).await.map_err(|failed| error(&failed))?;
        self.settings_in_transaction = self.in_transaction;
        Ok(())
    }

    async fn rolled_back(&mut self) -> Result<(), DbError> {
        self.in_transaction = false;
        self.restore_settings().await
    }

    async fn refuse_aborted(&mut self) -> Result<(), DbError> {
        match self.client.batch_execute("SELECT 1").await {
            Ok(()) => Ok(()),
            Err(failed) if failed.code() == Some(&tokio_postgres::error::SqlState::IN_FAILED_SQL_TRANSACTION) => {
                self.client.batch_execute("ROLLBACK").await.map_err(|failed| error(&failed))?;
                self.rolled_back().await?;
                Err(error(&failed).with_code(ROLLED_BACK))
            }
            Err(failed) => Err(error(&failed)),
        }
    }

    fn settings(&self, state: &TrackedState) -> Result<Vec<String>, DbError> {
        let mut statements = Vec::new();
        if let Some(schema) = &state.schema {
            statements.push(format!("SET search_path TO {}", quote_ident(schema)));
        }
        if !self.pooler {
            if let Some(isolation) = &state.isolation {
                let level = capabilities()
                    .isolation_levels
                    .iter()
                    .find(|level| level.eq_ignore_ascii_case(isolation))
                    .ok_or_else(|| DbError::new(ErrorKind::Invalid, isolation))?;
                statements.push(format!(
                    "SET SESSION CHARACTERISTICS AS TRANSACTION ISOLATION LEVEL {}",
                    level.to_uppercase()
                ));
            }
            if self.quirks.guards {
                statements.push(format!(
                    "SET default_transaction_read_only = {}",
                    if state.read_only { "on" } else { "off" }
                ));
            }
        }
        Ok(statements)
    }

    async fn run(&mut self, statement: &str, options: &ExecOptions) -> Result<Execution, DbError> {
        self.run_with(statement, &[], options).await
    }

    async fn run_with(
        &mut self,
        statement: &str,
        params: &[Param],
        options: &ExecOptions,
    ) -> Result<Execution, DbError> {
        if self.pooler && !params.is_empty() {
            return Err(DbError::new(ErrorKind::Unsupported, "bound parameters behind a transaction pooler"));
        }
        let words = first_words(statement);
        let first = words.first().map(String::as_str);
        let control = matches!(first, Some("begin" | "start" | "commit" | "end" | "rollback" | "abort"));
        if !self.state.auto_commit && !self.in_transaction && !control {
            self.client.batch_execute("BEGIN").await.map_err(|failed| error(&failed))?;
            self.in_transaction = true;
        }
        if matches!(first, Some("commit" | "end")) && self.in_transaction {
            self.refuse_aborted().await?;
        }
        if !self.pooler && options.max_rows < usize::MAX && crate::sql::pageable(statement) {
            let bound: Vec<TextParam<'_>> = params.iter().map(|param| TextParam(param.as_deref())).collect();
            let bound: Vec<&(dyn ToSql + Sync)> = bound.iter().map(|param| param as _).collect();
            let mut capped = Capped { cap: options.max_rows, ..Capped::default() };
            let page = StreamOptions { page: options.max_rows.saturating_add(1), skip: 0 };
            self.paged(statement, &bound, &page, &mut capped).await?;
            return Ok(Execution {
                columns: capped.columns,
                rows: capped.rows,
                truncated: capped.truncated,
                affected: None,
                notices: self.take_notices(),
            });
        }
        let no_parameters: Vec<(&(dyn ToSql + Sync), Type)> = Vec::new();
        let mut described: Option<Vec<Column>> = None;
        let (columns, stream) = if self.pooler {
            (None, self.client.query_typed_raw(statement, no_parameters).await)
        } else {
            match self.client.prepare(statement).await {
                Ok(prepared) => {
                    let columns: Vec<(String, Type)> = column_types(prepared.columns());
                    described = Some(as_columns(prepared.columns()));
                    let bound: Vec<TextParam<'_>> = params.iter().map(|param| TextParam(param.as_deref())).collect();
                    let bound: Vec<&(dyn ToSql + Sync)> = bound.iter().map(|param| param as _).collect();
                    (Some(columns), self.client.query_raw(&prepared, bound).await)
                }
                Err(failed) => (None, Err(failed)),
            }
        };
        let stream = stream.map_err(|failed| error(&failed))?;
        futures_util::pin_mut!(stream);
        let mut types: Option<Vec<(String, Type)>> = columns;
        let mut execution = Execution::default();
        while let Some(row) = stream.next().await {
            let row = row.map_err(|failed| error(&failed))?;
            if types.is_none() {
                types = Some(column_types(row.columns()));
                described = Some(as_columns(row.columns()));
            }
            if execution.rows.len() >= options.max_rows {
                execution.truncated = true;
                continue;
            }
            let types = types.as_deref().unwrap_or_default();
            let zone = self.zone();
            execution.rows.push(values(&row, types, &zone));
        }
        execution.columns = described.unwrap_or_default();
        execution.affected = stream.rows_affected();
        self.track_control(&words).await?;
        if self.pooler && self.in_transaction {
            self.client.batch_execute("ROLLBACK").await.map_err(|failed| error(&failed))?;
            self.in_transaction = false;
            return Err(DbError::new(ErrorKind::Unsupported, "transactions are not held behind a transaction pooler"));
        }
        execution.notices = self.take_notices();
        Ok(execution)
    }

    fn zone(&self) -> decode::Zone {
        self.zone.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn begin_if_manual(&mut self, control: bool) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            if !self.state.auto_commit && !self.in_transaction && !control {
                self.client.batch_execute("BEGIN").await.map_err(|failed| error(&failed))?;
                self.in_transaction = true;
            }
            Ok(())
        })
    }

    async fn track_control(&mut self, words: &[String]) -> Result<(), DbError> {
        match (words.first().map(String::as_str), words.get(1).map(String::as_str)) {
            (Some("begin" | "start"), _) => self.in_transaction = true,
            (Some("rollback"), Some("to")) => self.restore_settings().await?,
            (Some("rollback" | "abort"), _) => self.rolled_back().await?,
            (Some("commit" | "end"), _) => {
                self.in_transaction = false;
                self.settings_in_transaction = false;
            }
            _ => {}
        }
        Ok(())
    }

    async fn streamed(
        &mut self,
        statement: &str,
        params: &[Value],
        options: &StreamOptions,
        sink: &mut dyn RowSink,
    ) -> Result<StreamEnd, DbError> {
        if self.pooler && !params.is_empty() {
            return Err(DbError::new(ErrorKind::Unsupported, "bound parameters behind a transaction pooler"));
        }
        let texts: Vec<Option<String>> =
            params.iter().map(|value| crate::dialect::SqlDialect::Postgres.param(value)).collect();
        let bound: Vec<TextParam<'_>> = texts.iter().map(|text| TextParam(text.as_deref())).collect();
        let bound: Vec<&(dyn ToSql + Sync)> = bound.iter().map(|param| param as _).collect();
        let words = first_words(statement);
        let first = words.first().map(String::as_str);
        let control = matches!(first, Some("begin" | "start" | "commit" | "end" | "rollback" | "abort"));
        self.begin_if_manual(control).await?;
        if matches!(first, Some("commit" | "end")) && self.in_transaction {
            self.refuse_aborted().await?;
        }
        if !self.pooler && crate::sql::pageable(statement) {
            let end = self.paged(statement, &bound, options, sink).await;
            let mut end = end?;
            end.notices = self.take_notices();
            return Ok(end);
        }
        let no_parameters: Vec<(&(dyn ToSql + Sync), Type)> = Vec::new();
        let (columns, stream) = if self.pooler {
            (None, self.client.query_typed_raw(statement, no_parameters).await)
        } else {
            match self.client.prepare(statement).await {
                Ok(prepared) => {
                    sink.columns(as_columns(prepared.columns()));
                    (
                        Some(column_types(prepared.columns())),
                        self.client.query_raw(&prepared, bound.iter().copied()).await,
                    )
                }
                Err(failed) => (None, Err(failed)),
            }
        };
        let stream = stream.map_err(|failed| error(&failed))?;
        futures_util::pin_mut!(stream);
        let mut types: Option<Vec<(String, Type)>> = columns;
        let zone = self.zone();
        let page = options.page.max(1);
        let mut skip = options.skip;
        let mut batch = Vec::new();
        let mut keeping = true;
        let mut wanted = if types.as_ref().is_some_and(|types| !types.is_empty()) { sink.want().await } else { 0 };
        let mut dropped = false;
        while let Some(row) = stream.next().await {
            let row = row.map_err(|failed| error(&failed))?;
            if types.is_none() {
                let found = column_types(row.columns());
                sink.columns(as_columns(row.columns()));
                types = Some(found);
                wanted = sink.want().await;
            }
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if !keeping || wanted == 0 {
                dropped = true;
                continue;
            }
            batch.push(values(&row, types.as_deref().unwrap_or_default(), &zone));
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
        let affected = stream.rows_affected();
        self.track_control(&words).await?;
        if self.pooler && self.in_transaction {
            self.client.batch_execute("ROLLBACK").await.map_err(|failed| error(&failed))?;
            self.in_transaction = false;
            return Err(DbError::new(ErrorKind::Unsupported, "transactions are not held behind a transaction pooler"));
        }
        Ok(StreamEnd { affected, complete: !dropped, notices: self.take_notices() })
    }

    async fn copied(
        &mut self,
        statement: &str,
        options: &crate::export::CopyOptions,
        sink: &mut dyn ByteSink,
    ) -> Result<StreamEnd, DbError> {
        if self.pooler || !crate::sql::pageable(statement) {
            return Err(DbError::new(ErrorKind::Unsupported, "copy"));
        }
        let prepared = self.client.prepare(statement).await.map_err(|failed| error(&failed))?;
        sink.columns(as_columns(prepared.columns()));
        self.begin_if_manual(false).await?;
        let copy = options.statement(statement);
        let stream = self.client.copy_out(&copy).await.map_err(|failed| error(&failed))?;
        futures_util::pin_mut!(stream);
        let mut complete = true;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|failed| error(&failed))?;
            if !sink.want().await || !sink.push(chunk.to_vec()) {
                complete = false;
                break;
            }
        }
        Ok(StreamEnd { affected: None, complete, notices: self.take_notices() })
    }

    async fn copied_in(&mut self, statement: &str, data: Vec<u8>) -> Result<u64, DbError> {
        self.begin_if_manual(false).await?;
        let sink = self.client.copy_in::<_, bytes::Bytes>(statement).await.map_err(|failed| error(&failed))?;
        futures_util::pin_mut!(sink);
        sink.send(bytes::Bytes::from(data)).await.map_err(|failed| error(&failed))?;
        sink.as_mut().finish().await.map_err(|failed| error(&failed))
    }

    async fn paged(
        &mut self,
        statement: &str,
        params: &[&(dyn ToSql + Sync)],
        options: &StreamOptions,
        sink: &mut dyn RowSink,
    ) -> Result<StreamEnd, DbError> {
        let prepared = self.client.prepare(statement).await.map_err(|failed| error(&failed))?;
        let types = column_types(prepared.columns());
        sink.columns(as_columns(prepared.columns()));
        let own = !self.in_transaction;
        let zone = self.zone();
        let before = self.notices.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len();
        let transaction = self.client.transaction().await.map_err(|failed| error(&failed))?;
        if !own {
            let mut notices = self.notices.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut index = 0;
            notices.retain(|notice| {
                index += 1;
                index <= before || notice.code.as_deref() != Some("25001")
            });
        }
        sink.portal(!own);
        let read = async {
            let portal = transaction.bind(&prepared, params).await.map_err(|failed| error(&failed))?;
            let mut skip = options.skip;
            while skip > 0 {
                let chunk = skip.min(10_000);
                let stream = transaction
                    .query_portal_raw(&portal, i32::try_from(chunk).unwrap_or(i32::MAX))
                    .await
                    .map_err(|failed| error(&failed))?;
                futures_util::pin_mut!(stream);
                let mut read = 0_u64;
                while let Some(row) = stream.next().await {
                    row.map_err(|failed| error(&failed))?;
                    read += 1;
                }
                if read < chunk {
                    return Ok(true);
                }
                skip -= chunk;
            }
            loop {
                let wanted = sink.want().await;
                if wanted == 0 {
                    return Ok(false);
                }
                let stream = transaction
                    .query_portal_raw(&portal, i32::try_from(wanted).unwrap_or(i32::MAX))
                    .await
                    .map_err(|failed| error(&failed))?;
                futures_util::pin_mut!(stream);
                let mut rows = Vec::with_capacity(wanted.min(10_000));
                while let Some(row) = stream.next().await {
                    let row = row.map_err(|failed| error(&failed))?;
                    rows.push(values(&row, &types, &zone));
                }
                let short = rows.len() < wanted;
                let keep = sink.push(rows);
                if short {
                    return Ok(true);
                }
                if !keep {
                    return Ok(false);
                }
            }
        };
        let read: Result<bool, DbError> = read.await;
        if own {
            match &read {
                Ok(_) => transaction.commit().await.map_err(|failed| error(&failed))?,
                Err(_) => transaction.rollback().await.map_err(|failed| error(&failed))?,
            }
        } else {
            std::mem::forget(transaction);
        }
        let complete = read?;
        Ok(StreamEnd { affected: None, complete, notices: Vec::new() })
    }

    async fn identify(&mut self) -> Result<String, DbError> {
        if let Some(version) = &self.version {
            return Ok(version.clone());
        }
        let raw = first_cell(&self.client.simple_query("SELECT version()").await.map_err(|failed| error(&failed))?)
            .unwrap_or_default();
        let mut extensions = Vec::new();
        if raw.starts_with("PostgreSQL") && crate::profile::marked_in(ID, &raw, "").is_none() {
            let listed = self.client.simple_query(EXTENSIONS).await.unwrap_or_default();
            extensions = listed
                .iter()
                .filter_map(|message| match message {
                    tokio_postgres::SimpleQueryMessage::Row(row) => row.get(0).map(str::to_owned),
                    _ => None,
                })
                .collect();
        }
        let shown = shown_version(&raw, &extensions);
        if !self.named_profile
            && let Some(profile) = crate::profile::detect(ID, &shown, self.host.as_deref())
        {
            self.quirks = profile.quirks;
        }
        self.version = Some(shown.clone());
        Ok(shown)
    }

    async fn structure(
        &mut self,
        request: &IntrospectRequest,
        previous: Option<&Snapshot>,
    ) -> Result<Introspection, DbError> {
        if !self.named_profile && self.identify().await.is_err() {
            self.version = Some(String::new());
        }
        match self.quirks.catalogue {
            Catalogue::Native => {
                introspect::introspect(&self.client, &self.database, request, previous, self.quirks.fingerprints).await
            }
            Catalogue::InformationSchema => {
                catalogue::information_schema(&self.client, &self.database, request, previous).await
            }
            Catalogue::MetaFunctions => {
                catalogue::meta_functions(&self.client, &self.database, request, previous).await
            }
        }
    }

    async fn nodes(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        if let Some(database) = path.first()
            && *database != self.database
        {
            return Err(DbError::new(ErrorKind::Unsupported, format!("{database} is read through its own connection")));
        }
        let request = IntrospectRequest {
            schemas: path.get(1).cloned().into_iter().collect(),
            objects: match path {
                [_, schema, _, key, ..] => vec![(schema.clone(), key.clone())],
                _ => Vec::new(),
            },
            force: true,
            system: true,
            large_schema: LARGE_SCHEMA,
            outline: path.len() < 2,
        };
        let read = self.structure(&request, None).await?;
        let mut snapshot = Snapshot::new("", ID);
        snapshot.catalogs = read.catalogs;
        if !self.show_all_databases {
            let current = self.database.clone();
            snapshot.catalogs.retain(|catalog| catalog.name == current);
        }
        let options = TreeOptions { show_system: self.show_system_objects, ..TreeOptions::default() };
        Ok(crate::schema::children(&snapshot, path, &options).unwrap_or_default())
    }
}

impl DriverSession for Session {
    fn execute<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a ExecOptions,
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(self.run(statement, options))
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

    fn copy_out<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a crate::export::CopyOptions,
        sink: &'a mut dyn ByteSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        Box::pin(self.copied(statement, options, sink))
    }

    fn copy_in<'a>(&'a mut self, statement: &'a str, data: Vec<u8>) -> BoxFuture<'a, Result<u64, DbError>> {
        Box::pin(self.copied_in(statement, data))
    }

    fn execute_bound<'a>(
        &'a mut self,
        statement: &'a str,
        params: &'a [Param],
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(async move {
            let options = ExecOptions { max_rows: usize::MAX, timeout: None };
            self.run_with(statement, params, &options).await
        })
    }

    fn children<'a>(&'a mut self, path: &'a [String]) -> BoxFuture<'a, Result<Vec<Node>, DbError>> {
        Box::pin(self.nodes(path))
    }

    fn introspect<'a>(
        &'a mut self,
        request: &'a IntrospectRequest,
        previous: Option<&'a Snapshot>,
    ) -> BoxFuture<'a, Result<Introspection, DbError>> {
        Box::pin(async move {
            let mut read = self.structure(request, previous).await?;
            if !self.show_all_databases {
                let current = self.database.clone();
                read.catalogs.retain(|catalog| catalog.name == current);
            }
            Ok(read)
        })
    }

    fn sessions<'a>(
        &'a mut self,
        request: &'a crate::sessions::SessionsRequest,
    ) -> BoxFuture<'a, Result<crate::sessions::ServerSessions, DbError>> {
        Box::pin(sessions::list(&self.client, request))
    }

    fn end_session<'a>(
        &'a mut self,
        id: &'a str,
        end: crate::sessions::SessionEnd,
    ) -> BoxFuture<'a, Result<crate::sessions::Ended, DbError>> {
        Box::pin(sessions::end(&self.client, id, end))
    }

    fn server_id(&self) -> Option<String> {
        self.backend.clone()
    }

    fn ping(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move { self.client.simple_query("").await.map(|_| ()).map_err(|failed| error(&failed)) })
    }

    fn server_version(&mut self) -> BoxFuture<'_, Result<Option<String>, DbError>> {
        Box::pin(async move {
            let version = self.identify().await?;
            Ok((!version.is_empty()).then_some(version))
        })
    }

    fn apply<'a>(&'a mut self, state: &'a TrackedState) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(self.apply_state(state))
    }

    fn commit(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            if self.in_transaction {
                self.refuse_aborted().await?;
            }
            self.client.batch_execute("COMMIT").await.map_err(|failed| error(&failed))?;
            self.in_transaction = false;
            self.settings_in_transaction = false;
            Ok(())
        })
    }

    fn rollback(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            self.client.batch_execute("ROLLBACK").await.map_err(|failed| error(&failed))?;
            self.rolled_back().await
        })
    }

    fn in_transaction(&self) -> bool {
        self.in_transaction
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst) || self.client.is_closed()
    }

    fn canceller(&self) -> Option<Arc<dyn Canceller>> {
        Some(Arc::clone(&self.cancel) as Arc<dyn Canceller>)
    }

    fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut *self.notices.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
    }
}

const EXTENSIONS: &str =
    "SELECT extname || ' ' || extversion FROM pg_extension WHERE extname IN ('timescaledb', 'citus') ORDER BY 1";

fn first_cell(messages: &[tokio_postgres::SimpleQueryMessage]) -> Option<String> {
    messages.iter().find_map(|message| match message {
        tokio_postgres::SimpleQueryMessage::Row(row) => row.get(0).map(str::to_owned),
        _ => None,
    })
}

pub fn shown_version(raw: &str, extensions: &[String]) -> String {
    let raw = raw.trim();
    let cut = [" on ", ", compiled", " ("].iter().filter_map(|stop| raw.find(stop)).min().unwrap_or(raw.len());
    let mut shown = raw[..cut].trim().to_owned();
    if shown.starts_with(|c: char| c.is_ascii_digit()) {
        shown = format!("PostgreSQL {shown}");
    }
    let mut notes: Vec<String> = crate::profile::marked_in(ID, raw, &shown).map(str::to_owned).into_iter().collect();
    notes.extend(extensions.iter().cloned());
    if !notes.is_empty() {
        shown = format!("{shown} ({})", notes.join(", "));
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shown_version_names_the_server_a_profile_recognises() {
        let plain = "PostgreSQL 18.0 (Debian 18.0-1.pgdg13+3) on aarch64-unknown-linux-gnu, compiled by gcc (Debian 14.2.0-19) 14.2.0, 64-bit";
        assert_eq!(shown_version(plain, &[]), "PostgreSQL 18.0");
        assert_eq!(
            shown_version("PostgreSQL 16.4 on aarch64-unknown-linux-musl, compiled by gcc", &[]),
            "PostgreSQL 16.4"
        );
        let cockroach = "CockroachDB CCL v24.1.0 (aarch64-unknown-linux-gnu, built 2024/05/15 21:28:29, go1.22.2 X:nocoverageredesign)";
        assert_eq!(shown_version(cockroach, &[]), "CockroachDB CCL v24.1.0");
        let redshift = "PostgreSQL 8.0.2 on i686-pc-linux-gnu, compiled by GCC gcc (GCC) 3.4.2 20041017 (Red Hat 3.4.2-6.fc3), Redshift 1.0.77467";
        assert_eq!(shown_version(redshift, &[]), "PostgreSQL 8.0.2 (Amazon Redshift)");
        let greenplum = "PostgreSQL 12.12 (Greenplum Database 7.1.0 build commit:e7c2b1f14bb42a1018ac57d14f4436880e0a0515) on x86_64-pc-linux-gnu";
        assert_eq!(shown_version(greenplum, &[]), "PostgreSQL 12.12 (Greenplum)");
        let yugabyte = "PostgreSQL 11.2-YB-2.20.1.0-b0 on x86_64-pc-linux-gnu, compiled by clang version 16.0.6";
        assert_eq!(shown_version(yugabyte, &[]), "PostgreSQL 11.2-YB-2.20.1.0-b0");
        let timescale = shown_version("PostgreSQL 16.4 on x86_64", &["timescaledb 2.17.0".to_owned()]);
        assert_eq!(timescale, "PostgreSQL 16.4 (timescaledb 2.17.0)");
        for (version, profile) in [
            (shown_version(cockroach, &[]), "cockroachdb"),
            (shown_version(redshift, &[]), "redshift"),
            (shown_version(greenplum, &[]), "greenplum"),
            (shown_version(yugabyte, &[]), "yugabytedb"),
            (timescale, "timescaledb"),
            (shown_version("PostgreSQL 15.6 on x86_64", &["citus 12.1-1".to_owned()]), "citus"),
        ] {
            assert_eq!(crate::profile::detect(ID, &version, None).map(|found| found.id), Some(profile), "{version}");
        }
        assert_eq!(crate::profile::detect(ID, &shown_version(plain, &[]), Some("db.example.com")), None);
    }

    #[test]
    fn plain_reads_are_told_from_writes() {
        assert_eq!(classify("SELECT * FROM t"), StatementKind::Read);
        assert_eq!(classify("  -- note\n select 1"), StatementKind::Read);
        assert_eq!(classify("/* x */ WITH a AS (SELECT 1) SELECT * FROM a"), StatementKind::Read);
        assert_eq!(classify("SHOW search_path"), StatementKind::Read);
        assert_eq!(classify("WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d"), StatementKind::Write);
        assert_eq!(classify("SELECT * INTO copy FROM t"), StatementKind::Write);
        assert_eq!(classify("SELECT * FROM t FOR UPDATE"), StatementKind::Write);
        assert_eq!(classify("SELECT nextval('s')"), StatementKind::Write);
        assert_eq!(classify("EXPLAIN ANALYZE DELETE FROM t"), StatementKind::Write);
        assert_eq!(classify("UPDATE t SET a = 1"), StatementKind::Write);
        assert_eq!(classify("SET search_path TO x"), StatementKind::Write);
    }

    #[test]
    fn identifiers_are_quoted() {
        assert_eq!(quote_ident("we\"ird"), "\"we\"\"ird\"");
    }
}

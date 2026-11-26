use std::io::{BufRead, BufReader, Read};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value as Json;

use crate::driver::{
    BoxFuture, Canceller, ConnectSpec, DbError, Driver, DriverSession, ErrorKind, ExecOptions, RowSink, StreamEnd,
    StreamOptions, TrackedState,
};
use crate::model::{Capabilities, DataModel, Node, NodeKind, SettingField, StatementKind};
use crate::registry::{DataSource, TlsMode};
use crate::value::{Column, Execution, Notice, Typed, Value};

pub const ID: &str = "clickhouse";

pub const FORMAT: &str = "JSONCompactEachRowWithNamesAndTypes";

pub const PROTOCOL: &str = "protocol";

const ROWS_PER_MESSAGE: usize = 200;

const ERROR_BYTES: u64 = 64 << 10;

#[derive(Default)]
pub struct ClickHouse;

fn capabilities() -> &'static Capabilities {
    static CAPABILITIES: OnceLock<Capabilities> = OnceLock::new();
    CAPABILITIES.get_or_init(|| Capabilities {
        models: vec![DataModel::Relational],
        language: "sql:clickhouse".to_owned(),
        transactions: false,
        catalogs: true,
        node_kinds: vec![
            NodeKind::Catalog,
            NodeKind::Folder,
            NodeKind::Table,
            NodeKind::View,
            NodeKind::Column,
            NodeKind::Other("dictionary".to_owned()),
        ],
        server_cancel: true,
        server_paging: true,
        sessions: false,
        read_only_sessions: true,
        uri_schemes: ["clickhouse", "clickhouse+http", "clickhouse+https"].map(str::to_owned).to_vec(),
        password: true,
        settings: vec![SettingField {
            key: PROTOCOL.to_owned(),
            kind: "choice".to_owned(),
            choices: ["http", "https"].map(str::to_owned).to_vec(),
            default: Some("http".to_owned()),
        }],
        ..Capabilities::default()
    })
}

fn words(statement: &str) -> Vec<String> {
    let mut rest = statement.trim_start();
    loop {
        if let Some(line) = rest.strip_prefix("--") {
            rest = line.split_once('\n').map_or("", |(_, next)| next).trim_start();
        } else if let Some(block) = rest.strip_prefix("/*") {
            rest = block.split_once("*/").map_or("", |(_, next)| next).trim_start();
        } else {
            break;
        }
    }
    rest.split(|c: char| c.is_whitespace() || c == '(' || c == ';')
        .filter(|word| !word.is_empty())
        .take(2)
        .map(str::to_ascii_lowercase)
        .collect()
}

pub fn classify(statement: &str) -> StatementKind {
    match words(statement).first().map(String::as_str) {
        Some("select" | "with" | "show" | "describe" | "desc" | "explain" | "exists") => StatementKind::Read,
        _ => StatementKind::Write,
    }
}

pub fn quote(name: &str) -> String {
    format!("`{}`", name.replace('\\', "\\\\").replace('`', "\\`"))
}

impl Driver for ClickHouse {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "ClickHouse"
    }

    fn capabilities(&self) -> &Capabilities {
        capabilities()
    }

    fn default_port(&self) -> Option<u16> {
        Some(8123)
    }

    fn classify(&self, statement: &str) -> StatementKind {
        classify(statement)
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> BoxFuture<'a, Result<Box<dyn DriverSession>, DbError>> {
        Box::pin(async move { Ok(Box::new(Session::open(spec).await?) as Box<dyn DriverSession>) })
    }

    fn changes_structure(&self, statement: &str) -> bool {
        matches!(
            words(statement).first().map(String::as_str),
            Some("create" | "drop" | "alter" | "rename" | "truncate" | "attach" | "detach" | "exchange" | "undrop")
        )
    }

    fn open_text(&self, path: &[String], kind: &NodeKind) -> Option<String> {
        match (path, kind) {
            ([database, _, name], NodeKind::Table | NodeKind::View) => {
                Some(format!("SELECT * FROM {}.{} LIMIT 200;\n", quote(database), quote(name)))
            }
            ([database, _, name], NodeKind::Other(other)) if other == "dictionary" => {
                Some(format!("SELECT * FROM dictionary({}.{}) LIMIT 200;\n", quote(database), quote(name)))
            }
            _ => None,
        }
    }
}

#[derive(Clone)]
struct Endpoint {
    base: String,
    user: String,
    password: Option<String>,
    agent: ureq::Agent,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Endpoint").field("base", &self.base).field("user", &self.user).finish_non_exhaustive()
    }
}

fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn https(source: &DataSource) -> bool {
    let scheme =
        source.url.as_deref().and_then(|url| url.split_once("://")).map(|(scheme, _)| scheme).unwrap_or_default();
    scheme.ends_with("https")
        || source.settings.get(PROTOCOL).and_then(Json::as_str) == Some("https")
        || matches!(source.tls.mode, TlsMode::Require | TlsMode::VerifyCa | TlsMode::VerifyFull)
}

fn tls_config(source: &DataSource, unchecked: bool) -> Result<ureq::tls::TlsConfig, DbError> {
    let failed = |error: &dyn std::fmt::Display| DbError::new(ErrorKind::Tls, error.to_string());
    let mut builder = ureq::tls::TlsConfig::builder()
        .provider(ureq::tls::TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(Arc::new(rustls_rustcrypto::provider()));
    if unchecked {
        builder = builder.disable_verification(true);
    }
    if let Some(file) = &source.tls.ca_file {
        let pem = std::fs::read(file).map_err(|error| failed(&error))?;
        let roots = ureq::tls::parse_pem(&pem)
            .filter_map(|item| match item {
                Ok(ureq::tls::PemItem::Certificate(certificate)) => Some(Ok(certificate)),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| failed(&error))?;
        builder = builder.root_certs(ureq::tls::RootCerts::Specific(Arc::new(roots)));
    }
    if let (Some(cert), Some(key)) = (&source.tls.client_cert, &source.tls.client_key) {
        let chain = std::fs::read(cert).map_err(|error| failed(&error))?;
        let chain = ureq::tls::parse_pem(&chain)
            .filter_map(|item| match item {
                Ok(ureq::tls::PemItem::Certificate(certificate)) => Some(Ok(certificate)),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| failed(&error))?;
        let key = std::fs::read(key).map_err(|error| failed(&error))?;
        let key = ureq::tls::PrivateKey::from_pem(&key).map_err(|error| failed(&error))?;
        builder = builder.client_cert(Some(ureq::tls::ClientCert::new_with_certs(&chain, key)));
    }
    Ok(builder.build())
}

#[derive(Debug)]
struct Tunnel(std::net::SocketAddr);

impl ureq::unversioned::resolver::Resolver for Tunnel {
    fn resolve(
        &self,
        _: &ureq::http::Uri,
        _: &ureq::config::Config,
        _: ureq::unversioned::transport::NextTimeout,
    ) -> Result<ureq::unversioned::resolver::ResolvedSocketAddrs, ureq::Error> {
        let mut addresses = self.empty();
        addresses.push(self.0);
        Ok(addresses)
    }
}

fn endpoint(spec: &ConnectSpec) -> Result<Endpoint, DbError> {
    let source = &spec.source;
    let secure = https(source);
    let host = source.host.clone().unwrap_or_else(|| "localhost".to_owned());
    let port = source.port.unwrap_or(if secure { 8443 } else { 8123 });
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host };
    let config = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .https_only(false)
        .max_redirects(0)
        .proxy(None)
        .timeout_connect(Some(spec.timeout))
        .tls_config(tls_config(source, source.tls.mode == TlsMode::Require)?)
        .user_agent(concat!("ZenIDE/", env!("CARGO_PKG_VERSION")))
        .build();
    let agent = match spec.via {
        Some(via) => {
            ureq::Agent::with_parts(config, ureq::unversioned::transport::DefaultConnector::default(), Tunnel(via))
        }
        None => config.into(),
    };
    Ok(Endpoint {
        base: format!("{}://{host}:{port}/", if secure { "https" } else { "http" }),
        user: source.user.clone().filter(|user| !user.is_empty()).unwrap_or_else(|| "default".to_owned()),
        password: spec.password.as_ref().map(|secret| secret.expose().to_owned()),
        agent,
    })
}

fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher
        .write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    let high = hasher.finish();
    hasher.write_u64(high);
    format!("zen-{high:016x}{:016x}", hasher.finish())
}

#[derive(Clone, Debug, Default)]
struct Call {
    params: Vec<(String, String)>,
}

impl Call {
    fn with(mut self, key: &str, value: impl Into<String>) -> Self {
        self.params.push((key.to_owned(), value.into()));
        self
    }

    fn url(&self, base: &str) -> String {
        let query: Vec<String> =
            self.params.iter().map(|(key, value)| format!("{}={}", encode(key), encode(value))).collect();
        format!("{base}?{}", query.join("&"))
    }
}

fn server_error(status: u16, body: &str, code: Option<String>) -> DbError {
    let text = body.trim();
    let code = code.or_else(|| {
        let rest = text.strip_prefix("Code: ")?;
        Some(rest.split(|c: char| !c.is_ascii_digit()).next()?.to_owned())
    });
    let kind = match code.as_deref() {
        Some("516" | "192" | "193" | "194") => ErrorKind::Authentication,
        Some("394") => ErrorKind::Cancelled,
        Some("159") => ErrorKind::Timeout,
        _ if status == 401 || status == 403 => ErrorKind::Authentication,
        _ => ErrorKind::Server,
    };
    let error = DbError::new(kind, if text.is_empty() { format!("HTTP {status}") } else { text.to_owned() });
    match code {
        Some(code) => error.with_code(code),
        None => error,
    }
}

fn transport(error: &ureq::Error) -> DbError {
    let detail = error.to_string();
    let lower = detail.to_ascii_lowercase();
    let kind = if matches!(error, ureq::Error::Timeout(_)) || lower.contains("timed out") {
        ErrorKind::Timeout
    } else if matches!(error, ureq::Error::Tls(_) | ureq::Error::Rustls(_)) || lower.contains("certificate") {
        ErrorKind::Tls
    } else {
        ErrorKind::Connection
    };
    DbError::new(kind, detail)
}

enum Message {
    Columns(Vec<Column>),
    Rows(Vec<Vec<Value>>),
    Failed(DbError),
}

fn send(endpoint: &Endpoint, call: &Call, body: &str) -> Result<ureq::http::Response<ureq::Body>, DbError> {
    let mut request = endpoint.agent.post(call.url(&endpoint.base)).header("X-ClickHouse-User", endpoint.user.as_str());
    if let Some(password) = &endpoint.password {
        request = request.header("X-ClickHouse-Key", password.as_str());
    }
    let mut response = request.send(body).map_err(|error| transport(&error))?;
    let status = response.status().as_u16();
    if status != 200 {
        let code = response
            .headers()
            .get("X-ClickHouse-Exception-Code")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let text = response.body_mut().with_config().limit(ERROR_BYTES).read_to_string().unwrap_or_default();
        return Err(server_error(status, &text, code));
    }
    Ok(response)
}

fn read_lines(reader: impl Read, sender: &tokio::sync::mpsc::Sender<Message>) {
    let mut lines = BufReader::new(reader);
    let mut line = String::new();
    let mut header: Vec<Vec<String>> = Vec::new();
    let mut types: Vec<String> = Vec::new();
    let mut batch = Vec::new();
    loop {
        line.clear();
        match lines.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                let _ = sender.blocking_send(Message::Failed(DbError::new(ErrorKind::Connection, error.to_string())));
                return;
            }
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        let parsed: Result<Vec<Json>, _> = serde_json::from_str(trimmed);
        let Ok(items) = parsed else {
            let mut rest = String::new();
            let _ = lines.take(ERROR_BYTES).read_to_string(&mut rest);
            let _ = sender.blocking_send(Message::Failed(server_error(200, &format!("{trimmed}\n{rest}"), None)));
            return;
        };
        if header.len() < 2 {
            header.push(items.iter().map(|item| item.as_str().unwrap_or_default().to_owned()).collect());
            if header.len() == 2 {
                types.clone_from(&header[1]);
                let columns = header[0]
                    .iter()
                    .zip(&types)
                    .map(|(name, type_name)| Column::new(name.clone(), type_name.clone()))
                    .collect();
                if sender.blocking_send(Message::Columns(columns)).is_err() {
                    return;
                }
            }
            continue;
        }
        batch.push(items.into_iter().zip(&types).map(|(item, type_name)| value(item, type_name)).collect());
        if batch.len() >= ROWS_PER_MESSAGE && sender.blocking_send(Message::Rows(std::mem::take(&mut batch))).is_err() {
            return;
        }
    }
    if !batch.is_empty() {
        let _ = sender.blocking_send(Message::Rows(batch));
    }
}

fn inner(type_name: &str) -> &str {
    let mut name = type_name.trim();
    loop {
        let unwrapped = ["Nullable(", "LowCardinality(", "SimpleAggregateFunction("]
            .iter()
            .find_map(|wrapper| name.strip_prefix(wrapper).and_then(|rest| rest.strip_suffix(')')));
        match unwrapped {
            Some(rest) if name.starts_with("SimpleAggregateFunction(") => {
                name = rest.split_once(',').map_or(rest, |(_, kind)| kind).trim();
            }
            Some(rest) => name = rest.trim(),
            None => return name,
        }
    }
}

fn arguments(type_name: &str, wrapper: &str) -> Option<Vec<String>> {
    let rest = type_name.strip_prefix(wrapper)?.strip_prefix('(')?.strip_suffix(')')?;
    let mut parts = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (at, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(rest[start..at].trim().to_owned());
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(rest[start..].trim().to_owned());
    Some(parts)
}

pub fn value(item: Json, type_name: &str) -> Value {
    let kind = inner(type_name);
    if item.is_null() {
        return Value::Null;
    }
    if let Some(items) = arguments(kind, "Array") {
        let element = items.first().cloned().unwrap_or_default();
        return match item {
            Json::Array(items) => Value::Array(items.into_iter().map(|item| value(item, &element)).collect()),
            other => Value::Json(other.to_string()),
        };
    }
    if let Some(elements) = arguments(kind, "Tuple") {
        return match item {
            Json::Array(items) => Value::Array(
                items
                    .into_iter()
                    .zip(elements.iter().chain(std::iter::repeat(&String::new())))
                    .map(|(item, element)| {
                        let element = element
                            .rsplit_once(' ')
                            .filter(|(name, _)| !name.contains('('))
                            .map_or(element.as_str(), |(_, kind)| kind);
                        value(item, element)
                    })
                    .collect(),
            ),
            Json::Object(fields) => {
                Value::Document(fields.into_iter().map(|(key, item)| (key, value(item, ""))).collect())
            }
            other => Value::Json(other.to_string()),
        };
    }
    if let Some(parts) = arguments(kind, "Map") {
        let element = parts.get(1).cloned().unwrap_or_default();
        return match item {
            Json::Object(fields) => {
                Value::Map(fields.into_iter().map(|(key, item)| (key, value(item, &element))).collect())
            }
            other => Value::Json(other.to_string()),
        };
    }
    let base = kind.split('(').next().unwrap_or(kind);
    match (base, item) {
        ("Bool", Json::Bool(flag)) => Value::Bool(flag),
        (
            "Int8" | "Int16" | "Int32" | "Int64" | "UInt8" | "UInt16" | "UInt32" | "UInt64" | "Int128" | "Int256"
            | "UInt128" | "UInt256",
            item,
        ) => {
            let text = match &item {
                Json::String(text) => text.clone(),
                other => other.to_string(),
            };
            text.parse().map_or(Value::Decimal(text), Value::Integer)
        }
        ("Float32" | "Float64" | "BFloat16", Json::Number(number)) => Value::Float(number.as_f64().unwrap_or(f64::NAN)),
        ("Float32" | "Float64" | "BFloat16", Json::String(text)) => Value::Float(match text.as_str() {
            "inf" | "+inf" => f64::INFINITY,
            "-inf" => f64::NEG_INFINITY,
            other => other.parse().unwrap_or(f64::NAN),
        }),
        ("Decimal" | "Decimal32" | "Decimal64" | "Decimal128" | "Decimal256", item) => Value::Decimal(match item {
            Json::String(text) => text,
            other => other.to_string(),
        }),
        ("Date" | "Date32" | "DateTime" | "DateTime64" | "Time" | "Time64", Json::String(text)) => {
            Value::Temporal(Typed { text, type_name: kind.to_owned() })
        }
        ("UUID", Json::String(text)) => Value::Uuid(text),
        ("JSON" | "Object" | "Dynamic" | "Variant", item) => Value::Json(item.to_string()),
        ("String" | "FixedString" | "Enum8" | "Enum16" | "Enum", Json::String(text)) => Value::Text(text),
        ("IPv4" | "IPv6", Json::String(text)) => Value::Other(Typed { text, type_name: kind.to_owned() }),
        (_, Json::String(text)) => {
            if kind.is_empty() {
                Value::Text(text)
            } else {
                Value::Other(Typed { text, type_name: kind.to_owned() })
            }
        }
        (_, Json::Bool(flag)) => Value::Bool(flag),
        (_, Json::Number(number)) => {
            number.as_i64().map_or_else(|| Value::Float(number.as_f64().unwrap_or(f64::NAN)), Value::Integer)
        }
        (_, other) => Value::Json(other.to_string()),
    }
}

pub struct Session {
    endpoint: Endpoint,
    session_id: String,
    database: String,
    read_only: bool,
    max_execution_time: Option<u64>,
    running: Arc<Mutex<Option<String>>>,
    version: Option<String>,
}

impl Session {
    async fn open(spec: &ConnectSpec) -> Result<Self, DbError> {
        let endpoint = endpoint(spec)?;
        let mut session = Session {
            endpoint,
            session_id: new_id(),
            database: spec
                .state
                .schema
                .clone()
                .or_else(|| spec.source.database.clone())
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "default".to_owned()),
            read_only: spec.state.read_only,
            max_execution_time: spec.guards.statement_timeout.map(|limit| limit.as_secs().max(1)),
            running: Arc::default(),
            version: None,
        };
        let probe = session.collect("SELECT version()", 1);
        let execution =
            tokio::time::timeout(spec.timeout, probe).await.map_err(|_| DbError::new(ErrorKind::Timeout, ""))??;
        session.version = execution
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(Value::display)
            .map(|version| format!("ClickHouse {version}"));
        Ok(session)
    }

    fn call(&self, query_id: &str) -> Call {
        let mut call = Call::default()
            .with("session_id", self.session_id.as_str())
            .with("session_timeout", "600")
            .with("database", self.database.as_str())
            .with("query_id", query_id)
            .with("default_format", FORMAT)
            .with("output_format_json_quote_64bit_integers", "1")
            .with("output_format_json_quote_decimals", "1")
            .with("output_format_decimal_trailing_zeros", "1")
            .with("output_format_json_quote_denormals", "1");
        if self.read_only {
            call = call.with("readonly", "2");
        }
        if let Some(seconds) = self.max_execution_time {
            call = call.with("max_execution_time", seconds.to_string());
        }
        call
    }

    fn start(&self, statement: &str, params: &[(&str, &str)]) -> (tokio::sync::mpsc::Receiver<Message>, String) {
        let query_id = new_id();
        let mut call = self.call(&query_id);
        for (key, value) in params {
            call = call.with(&format!("param_{key}"), *value);
        }
        let (sender, receiver) = tokio::sync::mpsc::channel(4);
        let endpoint = self.endpoint.clone();
        let body = statement.trim().trim_end_matches(';').to_owned();
        *self.running.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(query_id.clone());
        tokio::task::spawn_blocking(move || match send(&endpoint, &call, &body) {
            Ok(response) => read_lines(response.into_body().into_reader(), &sender),
            Err(error) => {
                let _ = sender.blocking_send(Message::Failed(error));
            }
        });
        (receiver, query_id)
    }

    fn finished(&self) {
        *self.running.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    async fn collect_with(
        &mut self,
        statement: &str,
        max_rows: usize,
        params: &[(&str, &str)],
    ) -> Result<Execution, DbError> {
        let (mut receiver, _) = self.start(statement, params);
        let mut execution = Execution::default();
        let mut failure = None;
        while let Some(message) = receiver.recv().await {
            match message {
                Message::Columns(columns) => execution.columns = columns,
                Message::Rows(rows) => {
                    let room = max_rows.saturating_sub(execution.rows.len());
                    if rows.len() > room {
                        execution.truncated = true;
                    }
                    execution.rows.extend(rows.into_iter().take(room));
                    if execution.truncated {
                        break;
                    }
                }
                Message::Failed(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        self.finished();
        match failure {
            Some(error) => Err(error),
            None => {
                self.track(statement);
                Ok(execution)
            }
        }
    }

    async fn collect(&mut self, statement: &str, max_rows: usize) -> Result<Execution, DbError> {
        self.collect_with(statement, max_rows, &[]).await
    }

    fn track(&mut self, statement: &str) {
        let words = words(statement);
        if words.first().map(String::as_str) == Some("use")
            && let Some(name) = statement.trim().trim_end_matches(';').split_whitespace().nth(1)
        {
            self.database = name.trim_matches(['`', '"']).to_owned();
        }
    }

    async fn streamed(
        &mut self,
        statement: &str,
        options: &StreamOptions,
        sink: &mut dyn RowSink,
    ) -> Result<StreamEnd, DbError> {
        let (mut receiver, _) = self.start(statement, &[]);
        let page = options.page.max(1);
        let mut skip = options.skip;
        let mut wanted = 0usize;
        let mut keeping = true;
        let mut dropped = false;
        let mut batch = Vec::new();
        let mut failure = None;
        while let Some(message) = receiver.recv().await {
            match message {
                Message::Columns(columns) => {
                    let any = !columns.is_empty();
                    sink.columns(columns);
                    wanted = if any { sink.want().await } else { 0 };
                }
                Message::Rows(rows) => {
                    for row in rows {
                        if skip > 0 {
                            skip -= 1;
                            continue;
                        }
                        if !keeping || wanted == 0 {
                            dropped = true;
                            continue;
                        }
                        batch.push(row);
                        wanted -= 1;
                        if batch.len() >= page || wanted == 0 {
                            keeping = sink.push(std::mem::take(&mut batch));
                            if keeping && wanted == 0 {
                                wanted = sink.want().await;
                            }
                        }
                    }
                    if dropped && !keeping {
                        break;
                    }
                }
                Message::Failed(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if !batch.is_empty() && keeping {
            sink.push(batch);
        }
        self.finished();
        if let Some(error) = failure {
            return Err(error);
        }
        self.track(statement);
        Ok(StreamEnd { affected: None, complete: !dropped, notices: Vec::new() })
    }

    async fn nodes(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        let text = |row: &[Value], at: usize| row.get(at).and_then(Value::display).unwrap_or_default();
        let number = |row: &[Value], at: usize| match row.get(at) {
            Some(Value::Integer(number)) => Some(*number),
            Some(Value::Decimal(text)) => text.parse().ok(),
            _ => None,
        };
        match path {
            [] => {
                let read = self.collect("SELECT name, comment FROM system.databases ORDER BY name", 100_000).await?;
                Ok(read
                    .rows
                    .iter()
                    .map(|row| {
                        let name = text(row, 0);
                        let mut node = Node::new(vec![name.clone()], NodeKind::Catalog, name.clone(), true);
                        node.system = matches!(name.as_str(), "system" | "INFORMATION_SCHEMA" | "information_schema");
                        node.comment = Some(text(row, 1)).filter(|comment| !comment.is_empty());
                        node
                    })
                    .collect())
            }
            [database] => Ok(["tables", "views", "dictionaries"]
                .iter()
                .map(|folder| {
                    let label = match *folder {
                        "tables" => "Tables",
                        "views" => "Views",
                        _ => "Dictionaries",
                    };
                    Node::new(vec![database.clone(), (*folder).to_owned()], NodeKind::Folder, label, true)
                })
                .collect()),
            [database, folder] => {
                let filter = match folder.as_str() {
                    "views" => "engine IN ('View', 'MaterializedView', 'LiveView', 'WindowView')",
                    "dictionaries" => "engine = 'Dictionary'",
                    _ => "engine NOT IN ('View', 'MaterializedView', 'LiveView', 'WindowView', 'Dictionary')",
                };
                let query = format!(
                    "SELECT name, engine, total_rows, total_bytes, comment, is_temporary FROM system.tables \
                     WHERE database = {{database:String}} AND {filter} ORDER BY name"
                );
                let read = self.collect_with(&query, 100_000, &[("database", database)]).await?;
                Ok(read
                    .rows
                    .iter()
                    .map(|row| {
                        let name = text(row, 0);
                        let kind = match folder.as_str() {
                            "views" => NodeKind::View,
                            "dictionaries" => NodeKind::Other("dictionary".to_owned()),
                            _ => NodeKind::Table,
                        };
                        let mut node =
                            Node::new(vec![database.clone(), folder.clone(), name.clone()], kind, name, true)
                                .with_detail(Some(text(row, 1)));
                        node.rows = number(row, 2);
                        node.size = number(row, 3).and_then(|size| u64::try_from(size).ok());
                        node.comment = Some(text(row, 4)).filter(|comment| !comment.is_empty());
                        node
                    })
                    .collect())
            }
            [database, folder, table] => {
                let read = self
                    .collect_with(
                        "SELECT name, type, comment FROM system.columns WHERE database = {database:String} \
                         AND table = {table:String} ORDER BY position",
                        100_000,
                        &[("database", database), ("table", table)],
                    )
                    .await?;
                Ok(read
                    .rows
                    .iter()
                    .map(|row| {
                        let name = text(row, 0);
                        let mut node = Node::new(
                            vec![database.clone(), folder.clone(), table.clone(), name.clone()],
                            NodeKind::Column,
                            name,
                            false,
                        )
                        .with_detail(Some(text(row, 1)));
                        node.comment = Some(text(row, 2)).filter(|comment| !comment.is_empty());
                        node
                    })
                    .collect())
            }
            _ => Ok(Vec::new()),
        }
    }
}

struct Cancel {
    endpoint: Endpoint,
    running: Arc<Mutex<Option<String>>>,
}

impl Canceller for Cancel {
    fn cancel(&self) -> BoxFuture<'static, Result<(), DbError>> {
        let endpoint = self.endpoint.clone();
        let running = self.running.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        Box::pin(async move {
            let Some(query_id) = running else { return Ok(()) };
            tokio::task::spawn_blocking(move || {
                let call = Call::default().with("param_id", query_id.as_str());
                let response = send(&endpoint, &call, "KILL QUERY WHERE query_id = {id:String} ASYNC FORMAT Null")?;
                let _ = response.into_body().read_to_vec();
                Ok(())
            })
            .await
            .map_err(|error| DbError::new(ErrorKind::Connection, error.to_string()))?
        })
    }
}

impl DriverSession for Session {
    fn execute<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a ExecOptions,
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(self.collect(statement, options.max_rows))
    }

    fn stream<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a StreamOptions,
        sink: &'a mut dyn RowSink,
    ) -> BoxFuture<'a, Result<StreamEnd, DbError>> {
        Box::pin(self.streamed(statement, options, sink))
    }

    fn children<'a>(&'a mut self, path: &'a [String]) -> BoxFuture<'a, Result<Vec<Node>, DbError>> {
        Box::pin(self.nodes(path))
    }

    fn ping(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move { self.collect("SELECT 1", 1).await.map(|_| ()) })
    }

    fn server_version(&mut self) -> BoxFuture<'_, Result<Option<String>, DbError>> {
        Box::pin(async move { Ok(self.version.clone()) })
    }

    fn apply<'a>(&'a mut self, state: &'a TrackedState) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            self.read_only = state.read_only;
            if let Some(schema) = state.schema.as_ref().filter(|schema| !schema.is_empty()) {
                self.database.clone_from(schema);
            }
            Ok(())
        })
    }

    fn commit(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async { Ok(()) })
    }

    fn rollback(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async { Ok(()) })
    }

    fn in_transaction(&self) -> bool {
        false
    }

    fn is_closed(&self) -> bool {
        false
    }

    fn canceller(&self) -> Option<Arc<dyn Canceller>> {
        Some(Arc::new(Cancel { endpoint: self.endpoint.clone(), running: Arc::clone(&self.running) }))
    }

    fn take_notices(&mut self) -> Vec<Notice> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests;

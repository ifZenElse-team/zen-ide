pub mod line;

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use ::redis::aio::MultiplexedConnection;
use ::redis::cluster_async::ClusterConnection;
use ::redis::cluster_routing::{RoutingInfo, SingleNodeRoutingInfo};
use ::redis::{AsyncConnectionConfig, Cmd, ConnectionAddr, IntoConnectionInfo, ProtocolVersion, RedisConnectionInfo};
use ::redis::{RedisError, Value as Reply};

use crate::driver::{
    BoxFuture, Canceller, ConnectSpec, DbError, Driver, DriverSession, ErrorKind, ExecOptions, KeyChange, KeyRequest,
    TrackedState,
};
use crate::model::{Capabilities, DataModel, Node, NodeKind, SettingField, StatementKind};
use crate::registry::{DataSource, TlsMode};
use crate::value::{Column, Execution, KeyValue, Notice, Value};

pub const ID: &str = "redis";

pub const MODE: &str = "mode";

pub const SEPARATOR: &str = "separator";

pub const MASTER: &str = "master";

pub const KEYS_PER_LEVEL: usize = 10_000;

const SCAN_CALLS: usize = 200;

const SCAN_COUNT: usize = 1000;

const TYPED_KEYS: usize = 1000;

pub const STRING_BYTES: usize = 1 << 20;

#[derive(Default)]
pub struct Redis;

fn capabilities() -> &'static Capabilities {
    static CAPABILITIES: OnceLock<Capabilities> = OnceLock::new();
    CAPABILITIES.get_or_init(|| Capabilities {
        models: vec![DataModel::KeyValue],
        language: "redis".to_owned(),
        transactions: false,
        node_kinds: vec![
            NodeKind::Catalog,
            NodeKind::KeyPrefix,
            NodeKind::Key,
            NodeKind::Other("node".to_owned()),
            NodeKind::Other("more".to_owned()),
        ],
        server_cancel: true,
        row_identity: Some("key".to_owned()),
        uri_schemes: ["redis", "rediss", "redis+sentinel", "rediss+sentinel", "valkey", "valkeys"]
            .map(str::to_owned)
            .to_vec(),
        password: true,
        settings: vec![
            SettingField {
                key: MODE.to_owned(),
                kind: "choice".to_owned(),
                choices: ["standalone", "sentinel", "cluster"].map(str::to_owned).to_vec(),
                default: Some("standalone".to_owned()),
            },
            SettingField {
                key: SEPARATOR.to_owned(),
                kind: "text".to_owned(),
                choices: Vec::new(),
                default: Some(":".to_owned()),
            },
            SettingField {
                key: MASTER.to_owned(),
                kind: "text".to_owned(),
                choices: Vec::new(),
                default: Some("mymaster".to_owned()),
            },
        ],
        ..Capabilities::default()
    })
}

const READS: &[&str] = &[
    "get",
    "mget",
    "getrange",
    "strlen",
    "exists",
    "type",
    "ttl",
    "pttl",
    "expiretime",
    "pexpiretime",
    "keys",
    "scan",
    "dbsize",
    "info",
    "ping",
    "echo",
    "time",
    "hget",
    "hmget",
    "hgetall",
    "hkeys",
    "hvals",
    "hlen",
    "hexists",
    "hscan",
    "hstrlen",
    "hrandfield",
    "lrange",
    "lindex",
    "llen",
    "lpos",
    "smembers",
    "sismember",
    "smismember",
    "scard",
    "sscan",
    "srandmember",
    "sinter",
    "sunion",
    "sdiff",
    "sintercard",
    "zrange",
    "zrangebyscore",
    "zrangebylex",
    "zrevrange",
    "zrevrangebyscore",
    "zrevrangebylex",
    "zscore",
    "zmscore",
    "zrank",
    "zrevrank",
    "zcard",
    "zcount",
    "zlexcount",
    "zscan",
    "zrandmember",
    "zinter",
    "zunion",
    "zdiff",
    "xrange",
    "xrevrange",
    "xlen",
    "xread",
    "xinfo",
    "xpending",
    "json.get",
    "json.mget",
    "json.type",
    "json.strlen",
    "json.arrlen",
    "json.objkeys",
    "json.objlen",
    "memory",
    "object",
    "dump",
    "randomkey",
    "lcs",
    "bitcount",
    "bitpos",
    "getbit",
    "pfcount",
    "geopos",
    "geodist",
    "geohash",
    "geosearch",
    "georadius_ro",
    "georadiusbymember_ro",
    "eval_ro",
    "evalsha_ro",
    "fcall_ro",
    "select",
    "command",
    "client",
    "slowlog",
    "latency",
    "role",
    "lastsave",
    "sort_ro",
    "touch",
    "substr",
    "hello",
    "auth",
];

const ADMIN: &[&str] = &[
    "shutdown",
    "debug",
    "module",
    "replicaof",
    "slaveof",
    "failover",
    "save",
    "bgsave",
    "bgrewriteaof",
    "monitor",
    "sync",
    "psync",
    "swapdb",
    "flushdb",
    "flushall",
    "eval",
    "evalsha",
    "fcall",
];

const ADMIN_SUBCOMMANDS: &[(&str, &[&str])] = &[
    ("config", &["set", "rewrite", "resetstat"]),
    ("client", &["kill", "pause", "unpause", "no-evict", "no-touch"]),
    ("acl", &["setuser", "deluser", "load", "save"]),
    ("script", &["flush", "kill", "load"]),
    ("function", &["load", "delete", "flush", "restore", "kill"]),
    ("memory", &["purge"]),
    ("latency", &["reset"]),
    ("slowlog", &["reset"]),
    (
        "cluster",
        &[
            "addslots",
            "addslotsrange",
            "delslots",
            "delslotsrange",
            "failover",
            "forget",
            "meet",
            "replicate",
            "reset",
            "setslot",
            "flushslots",
            "saveconfig",
            "bumpepoch",
            "set-config-epoch",
        ],
    ),
];

const UNSUPPORTED: &[&str] = &["subscribe", "psubscribe", "ssubscribe", "monitor", "sync", "psync", "quit"];

const STRUCTURE: &[&str] =
    &["del", "unlink", "rename", "renamenx", "flushdb", "flushall", "move", "copy", "swapdb", "restore"];

fn verb_of(statement: &str) -> String {
    statement.split_whitespace().next().unwrap_or_default().to_ascii_lowercase()
}

pub fn classify(statement: &str) -> StatementKind {
    if READS.contains(&verb_of(statement).as_str()) { StatementKind::Read } else { StatementKind::Write }
}

impl Driver for Redis {
    fn id(&self) -> &str {
        ID
    }

    fn name(&self) -> &str {
        "Redis"
    }

    fn capabilities(&self) -> &Capabilities {
        capabilities()
    }

    fn default_port(&self) -> Option<u16> {
        Some(6379)
    }

    fn classify(&self, statement: &str) -> StatementKind {
        classify(statement)
    }

    fn guards_read_only(&self) -> bool {
        true
    }

    fn connect<'a>(&'a self, spec: &'a ConnectSpec) -> BoxFuture<'a, Result<Box<dyn DriverSession>, DbError>> {
        Box::pin(async move { Ok(Box::new(Session::open(spec).await?) as Box<dyn DriverSession>) })
    }

    fn changes_structure(&self, statement: &str) -> bool {
        STRUCTURE.contains(&verb_of(statement).as_str())
    }

    fn open_text(&self, path: &[String], kind: &NodeKind) -> Option<String> {
        let key: String = path.get(1..)?.concat();
        match kind {
            NodeKind::Key => Some(format!("TYPE {}\n", line::quote(key.as_bytes()))),
            NodeKind::KeyPrefix => {
                Some(format!("SCAN 0 MATCH {}* COUNT 100\n", line::quote(&line::glob_escape(key.as_bytes()))))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Standalone,
    Sentinel,
    Cluster,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub mode: Mode,
    pub nodes: Vec<(String, u16)>,
    pub db: i64,
    pub master: String,
    pub tls: bool,
    pub separator: String,
}

fn scheme(source: &DataSource) -> String {
    source
        .url
        .as_deref()
        .and_then(|url| url.split_once("://"))
        .map(|(scheme, _)| scheme.to_ascii_lowercase())
        .unwrap_or_default()
}

fn setting(source: &DataSource, key: &str) -> Option<String> {
    match source.settings.get(key)? {
        serde_json::Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    }
}

pub fn target(source: &DataSource) -> Result<Target, DbError> {
    let scheme = scheme(source);
    let mode = match setting(source, MODE).as_deref() {
        Some("cluster") => Mode::Cluster,
        Some("sentinel") => Mode::Sentinel,
        _ if scheme.ends_with("+sentinel") => Mode::Sentinel,
        _ => Mode::Standalone,
    };
    let default_port = if mode == Mode::Sentinel { 26379 } else { 6379 };
    let hosts = source.host.clone().unwrap_or_else(|| "localhost".to_owned());
    let mut nodes = Vec::new();
    for part in hosts.split(',').map(str::trim).filter(|part| !part.is_empty()) {
        let (host, port) = match part.rsplit_once(':') {
            Some((host, port)) if !host.contains(':') || host.starts_with('[') => {
                let port = port.parse().map_err(|_| DbError::new(ErrorKind::Invalid, format!("port of {part}")))?;
                (host.trim_matches(['[', ']']).to_owned(), port)
            }
            _ => (part.trim_matches(['[', ']']).to_owned(), source.port.unwrap_or(default_port)),
        };
        nodes.push((host, port));
    }
    if nodes.len() == 1 && source.port.is_some() && !hosts.contains(':') {
        nodes[0].1 = source.port.unwrap_or(default_port);
    }
    let database = source.database.clone().unwrap_or_default();
    let (master, db) = match mode {
        Mode::Sentinel => {
            let (named, db) = database.split_once('/').map_or((database.as_str(), ""), |(named, db)| (named, db));
            let (named, db) = if named.parse::<i64>().is_ok() && db.is_empty() { ("", named) } else { (named, db) };
            let master =
                setting(source, MASTER).filter(|master| !master.is_empty()).unwrap_or_else(|| named.to_owned());
            (if master.is_empty() { "mymaster".to_owned() } else { master }, db.to_owned())
        }
        _ => (String::new(), database),
    };
    let db = match db.trim().trim_start_matches("db") {
        "" => 0,
        number => number.parse().map_err(|_| DbError::new(ErrorKind::Invalid, format!("database {number}")))?,
    };
    let tls = scheme.starts_with("rediss")
        || scheme == "valkeys"
        || matches!(source.tls.mode, TlsMode::Require | TlsMode::VerifyCa | TlsMode::VerifyFull);
    let separator = setting(source, SEPARATOR).unwrap_or_else(|| ":".to_owned());
    Ok(Target { mode, nodes, db: if mode == Mode::Cluster { 0 } else { db }, master, tls, separator })
}

fn install_tls() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::CryptoProvider::install_default(rustls_rustcrypto::provider());
    });
}

fn error(failed: &RedisError) -> DbError {
    let detail = failed.to_string();
    let lower = detail.to_ascii_lowercase();
    let kind = match failed.kind() {
        ::redis::ErrorKind::AuthenticationFailed => ErrorKind::Authentication,
        _ if lower.contains("wrongpass") || lower.contains("noauth") || lower.contains("invalid password") => {
            ErrorKind::Authentication
        }
        _ if failed.is_timeout() => ErrorKind::Timeout,
        _ if lower.contains("certificate") || lower.contains("tls") || lower.contains("handshake") => ErrorKind::Tls,
        ::redis::ErrorKind::Io
        | ::redis::ErrorKind::ClusterConnectionNotFound
        | ::redis::ErrorKind::MasterNameNotFoundBySentinel
        | ::redis::ErrorKind::NoValidReplicasFoundBySentinel
        | ::redis::ErrorKind::EmptySentinelList => ErrorKind::Connection,
        ::redis::ErrorKind::InvalidClientConfig => ErrorKind::Invalid,
        _ if failed.is_connection_refusal() || failed.is_io_error() => ErrorKind::Connection,
        _ => ErrorKind::Server,
    };
    let error = DbError::new(kind, detail);
    match failed.code() {
        Some(code) => error.with_code(code),
        None => error,
    }
}

fn certificates(source: &DataSource) -> Result<Option<::redis::TlsCertificates>, DbError> {
    let read = |file: &std::path::Path| {
        std::fs::read(file).map_err(|failed| DbError::new(ErrorKind::Tls, format!("{}: {failed}", file.display())))
    };
    let root_cert = source.tls.ca_file.as_deref().map(read).transpose()?;
    let client_tls = match (&source.tls.client_cert, &source.tls.client_key) {
        (Some(cert), Some(key)) => Some(::redis::ClientTlsConfig { client_cert: read(cert)?, client_key: read(key)? }),
        _ => None,
    };
    Ok((root_cert.is_some() || client_tls.is_some()).then_some(::redis::TlsCertificates { client_tls, root_cert }))
}

fn connection_info(spec: &ConnectSpec, protocol: ProtocolVersion, db: i64) -> RedisConnectionInfo {
    let mut info = RedisConnectionInfo::default().set_db(db).set_protocol(protocol);
    if let Some(user) = spec.source.user.as_deref().filter(|user| !user.is_empty()) {
        info = info.set_username(user);
    }
    if let Some(password) = &spec.password {
        info = info.set_password(password.expose());
    }
    info
}

fn tls_mode(source: &DataSource) -> ::redis::TlsMode {
    if source.tls.mode == TlsMode::Require { ::redis::TlsMode::Insecure } else { ::redis::TlsMode::Secure }
}

fn address(source: &DataSource, target: &Target, host: &str, port: u16) -> ConnectionAddr {
    if !target.tls {
        return ConnectionAddr::Tcp(host.to_owned(), port);
    }
    let mut addr = ConnectionAddr::TcpTls {
        host: host.to_owned(),
        port,
        insecure: source.tls.mode == TlsMode::Require,
        tls_params: None,
    };
    if source.tls.mode == TlsMode::VerifyCa {
        addr.set_danger_accept_invalid_hostnames(true);
    }
    addr
}

struct Tunnel(std::net::SocketAddr);

impl ::redis::io::AsyncDNSResolver for Tunnel {
    fn resolve<'a, 'b: 'a>(
        &'a self,
        _: &'b str,
        _: u16,
    ) -> ::redis::RedisFuture<'a, Box<dyn Iterator<Item = std::net::SocketAddr> + Send + 'a>> {
        let address = self.0;
        Box::pin(async move { Ok(Box::new(std::iter::once(address)) as Box<dyn Iterator<Item = _> + Send>) })
    }
}

fn config(spec: &ConnectSpec) -> AsyncConnectionConfig {
    AsyncConnectionConfig::new().set_connection_timeout(Some(spec.timeout)).set_response_timeout(None)
}

enum Opener {
    Client(::redis::Client, AsyncConnectionConfig),
    Sentinel(tokio::sync::Mutex<::redis::sentinel::SentinelClient>, AsyncConnectionConfig),
}

impl Opener {
    async fn open(&self) -> Result<MultiplexedConnection, RedisError> {
        match self {
            Opener::Client(client, config) => client.get_multiplexed_async_connection_with_config(config).await,
            Opener::Sentinel(client, config) => client.lock().await.get_async_connection_with_config(config).await,
        }
    }
}

enum Link {
    Single(MultiplexedConnection),
    Cluster(ClusterConnection),
}

impl Link {
    async fn query(&mut self, cmd: &Cmd) -> Result<Reply, RedisError> {
        match self {
            Link::Single(connection) => cmd.query_async(connection).await,
            Link::Cluster(connection) => cmd.query_async(connection).await,
        }
    }

    async fn query_at(&mut self, node: Option<&(String, u16)>, cmd: Cmd) -> Result<Reply, RedisError> {
        match (self, node) {
            (Link::Cluster(connection), Some((host, port))) => {
                let routing =
                    RoutingInfo::SingleNode(SingleNodeRoutingInfo::ByAddress { host: host.clone(), port: *port });
                connection.route_command(cmd, routing).await
            }
            (link, _) => link.query(&cmd).await,
        }
    }
}

pub struct Session {
    link: Link,
    opener: Option<Arc<Opener>>,
    client_id: Option<i64>,
    target: Target,
    db: i64,
    read_only: bool,
    in_multi: bool,
    closed: bool,
    writes: HashMap<String, bool>,
    version: Option<String>,
}

impl Session {
    async fn open(spec: &ConnectSpec) -> Result<Self, DbError> {
        install_tls();
        let target = target(&spec.source)?;
        let opening = Self::connect(spec, &target, ProtocolVersion::RESP3);
        let opened = match tokio::time::timeout(spec.timeout, opening).await {
            Ok(Err(failed)) if resp2_only(&failed) => {
                tokio::time::timeout(spec.timeout, Self::connect(spec, &target, ProtocolVersion::RESP2)).await
            }
            other => other,
        };
        let (link, opener) = match opened {
            Ok(opened) => opened.map_err(|failed| error(&failed))?,
            Err(_) => return Err(DbError::new(ErrorKind::Timeout, "")),
        };
        let mut session = Session {
            link,
            opener,
            client_id: None,
            db: target.db,
            target,
            read_only: spec.state.read_only,
            in_multi: false,
            closed: false,
            writes: HashMap::new(),
            version: None,
        };
        let name: String =
            spec.guards.application_name.chars().map(|c| if c.is_ascii_graphic() { c } else { '-' }).collect();
        if !name.is_empty() && session.target.mode != Mode::Cluster {
            let _ = session.link.query(::redis::cmd("CLIENT").arg("SETNAME").arg(&name)).await;
        }
        if session.target.mode != Mode::Cluster {
            session.client_id = session
                .link
                .query(&::redis::cmd("CLIENT").arg("ID").clone())
                .await
                .ok()
                .as_ref()
                .and_then(line::integer);
        }
        if let Some(db) = spec.state.database.as_deref().and_then(parse_db) {
            session.select(db).await?;
        }
        Ok(session)
    }

    async fn connect(
        spec: &ConnectSpec,
        target: &Target,
        protocol: ProtocolVersion,
    ) -> Result<(Link, Option<Arc<Opener>>), RedisError> {
        let source = &spec.source;
        let config = config(spec);
        let certificates = certificates(source).map_err(|failed| {
            RedisError::from((::redis::ErrorKind::InvalidClientConfig, "certificates", failed.detail))
        })?;
        match target.mode {
            Mode::Standalone => {
                let (host, port) = target.nodes.first().cloned().unwrap_or_else(|| ("localhost".to_owned(), 6379));
                let config = match spec.via {
                    Some(via) => config.set_dns_resolver(Tunnel(via)),
                    None => config,
                };
                let info = address(source, target, &host, port)
                    .into_connection_info()?
                    .set_redis_settings(connection_info(spec, protocol, target.db));
                let client = match certificates {
                    Some(certificates) if target.tls => ::redis::Client::build_with_tls(info, certificates)?,
                    _ => ::redis::Client::open(info)?,
                };
                let connection = client.get_multiplexed_async_connection_with_config(&config).await?;
                Ok((Link::Single(connection), Some(Arc::new(Opener::Client(client, config)))))
            }
            Mode::Sentinel => {
                let sentinels = target.nodes.iter().map(|(host, port)| address(source, target, host, *port));
                let mut builder = ::redis::sentinel::SentinelClientBuilder::new(
                    sentinels,
                    &target.master,
                    ::redis::sentinel::SentinelServerType::Master,
                )?
                .set_client_to_redis_db(target.db)
                .set_client_to_redis_protocol(protocol)
                .set_client_to_sentinel_protocol(protocol);
                if let Some(user) = source.user.as_deref().filter(|user| !user.is_empty()) {
                    builder = builder.set_client_to_redis_username(user);
                }
                if let Some(password) = &spec.password {
                    builder = builder.set_client_to_redis_password(password.expose());
                }
                if target.tls {
                    builder = builder
                        .set_client_to_redis_tls_mode(tls_mode(source))
                        .set_client_to_sentinel_tls_mode(tls_mode(source));
                    if let Some(certificates) = certificates {
                        builder = builder
                            .set_client_to_redis_certificates(certificates.clone())
                            .set_client_to_sentinel_certificates(certificates);
                    }
                }
                let mut client = builder.build()?;
                let connection = client.get_async_connection_with_config(&config).await?;
                Ok((
                    Link::Single(connection),
                    Some(Arc::new(Opener::Sentinel(tokio::sync::Mutex::new(client), config))),
                ))
            }
            Mode::Cluster => {
                let seeds: Vec<ConnectionAddr> =
                    target.nodes.iter().map(|(host, port)| address(source, target, host, *port)).collect();
                let mut builder = ::redis::cluster::ClusterClientBuilder::new(seeds)
                    .use_protocol(protocol)
                    .connection_timeout(spec.timeout)
                    .response_timeout(Duration::from_secs(24 * 3600))
                    .overall_response_timeout(None)
                    .retries(1);
                if let Some(user) = source.user.as_deref().filter(|user| !user.is_empty()) {
                    builder = builder.username(user);
                }
                if let Some(password) = &spec.password {
                    builder = builder.password(password.expose());
                }
                if target.tls {
                    builder = builder.tls(tls_mode(source));
                    if let Some(certificates) = certificates {
                        builder = builder.certs(certificates);
                    }
                }
                let connection = builder.build()?.get_async_connection().await?;
                Ok((Link::Cluster(connection), None))
            }
        }
    }

    fn fail(&mut self, failed: &RedisError) -> DbError {
        if failed.is_unrecoverable_error() || failed.is_io_error() || failed.is_connection_dropped() {
            self.closed = true;
        }
        error(failed)
    }

    async fn run(&mut self, node: Option<&(String, u16)>, cmd: Cmd) -> Result<Reply, DbError> {
        match self.link.query_at(node, cmd).await {
            Ok(reply) => Ok(reply),
            Err(failed) => Err(self.fail(&failed)),
        }
    }

    async fn select(&mut self, db: i64) -> Result<(), DbError> {
        if self.target.mode == Mode::Cluster || db == self.db {
            return Ok(());
        }
        self.run(None, ::redis::cmd("SELECT").arg(db).clone()).await?;
        self.db = db;
        Ok(())
    }

    async fn writes(&mut self, args: &[Vec<u8>]) -> bool {
        let lower = |arg: &[u8]| String::from_utf8_lossy(arg).to_ascii_lowercase();
        let verb = lower(&args[0]);
        let sub = args.get(1).map(|arg| lower(arg));
        if ADMIN.contains(&verb.as_str()) {
            return true;
        }
        if let (Some((_, subs)), Some(sub)) = (ADMIN_SUBCOMMANDS.iter().find(|(name, _)| *name == verb), &sub) {
            return subs.contains(&sub.as_str());
        }
        let name = match &sub {
            Some(sub) if ADMIN_SUBCOMMANDS.iter().any(|(name, _)| *name == verb) => format!("{verb}|{sub}"),
            _ => verb.clone(),
        };
        if let Some(known) = self.writes.get(&name) {
            return *known;
        }
        let flags = self.command_flags(&name).await;
        let writes = match flags {
            Some(flags) => flags.iter().any(|flag| flag == "write" || flag == "may_replicate"),
            None => !READS.contains(&verb.as_str()),
        };
        self.writes.insert(name, writes);
        writes
    }

    async fn command_flags(&mut self, name: &str) -> Option<Vec<String>> {
        let reply = self.link.query(::redis::cmd("COMMAND").arg("INFO").arg(name)).await.ok()?;
        let Reply::Array(mut entries) = reply else { return None };
        let Some(Reply::Array(entry)) = entries.pop() else { return None };
        match entry.into_iter().nth(2)? {
            Reply::Array(flags) | Reply::Set(flags) => Some(flags.into_iter().map(line::name).collect()),
            _ => None,
        }
    }

    fn locate(&self, path: &[String]) -> Result<Located, DbError> {
        let (first, rest) = path.split_first().ok_or_else(|| DbError::new(ErrorKind::Invalid, "empty path"))?;
        let key = rest.concat();
        match self.target.mode {
            Mode::Cluster => {
                let (host, port) = first
                    .rsplit_once(':')
                    .and_then(|(host, port)| Some((host.to_owned(), port.parse().ok()?)))
                    .ok_or_else(|| DbError::new(ErrorKind::Invalid, format!("node {first}")))?;
                Ok((Some((host, port)), 0, key))
            }
            _ => {
                let db =
                    parse_db(first).ok_or_else(|| DbError::new(ErrorKind::Invalid, format!("database {first}")))?;
                Ok((None, db, key))
            }
        }
    }

    async fn roots(&mut self) -> Result<Vec<Node>, DbError> {
        if self.target.mode == Mode::Cluster {
            let reply = self.run(None, ::redis::cmd("CLUSTER").arg("NODES").clone()).await?;
            let text = line::name(reply);
            let mut nodes: Vec<Node> = text
                .lines()
                .filter_map(|row| {
                    let fields: Vec<&str> = row.split_whitespace().collect();
                    let flags = fields.get(2)?;
                    if !flags.contains("master") || flags.contains("fail") {
                        return None;
                    }
                    let address = fields.get(1)?.split(['@', ',']).next()?.to_owned();
                    Some(Node::new(vec![address.clone()], NodeKind::Other("node".to_owned()), address, true))
                })
                .collect();
            nodes.sort_by(|a, b| a.label.cmp(&b.label));
            return Ok(nodes);
        }
        let reply = self.run(None, ::redis::cmd("INFO").arg("keyspace").clone()).await?;
        let mut counts: Vec<(i64, Option<u64>)> = line::name(reply)
            .lines()
            .filter_map(|row| {
                let (db, rest) = row.split_once(':')?;
                let db = db.strip_prefix("db")?.parse().ok()?;
                let keys = rest.split(',').find_map(|part| part.strip_prefix("keys="))?.parse().ok();
                Some((db, keys))
            })
            .collect();
        if !counts.iter().any(|(db, _)| *db == self.target.db) {
            counts.push((self.target.db, Some(0)));
        }
        counts.sort_by_key(|(db, _)| *db);
        Ok(counts
            .into_iter()
            .map(|(db, keys)| {
                let name = format!("db{db}");
                Node::new(vec![name.clone()], NodeKind::Catalog, name, true)
                    .with_detail(keys.map(|keys| keys.to_string()))
            })
            .collect())
    }

    async fn scan(&mut self, node: Option<&(String, u16)>, prefix: &str) -> Result<(Vec<String>, bool), DbError> {
        let pattern = [line::glob_escape(prefix.as_bytes()), b"*".to_vec()].concat();
        let mut cursor: u64 = 0;
        let mut keys = Vec::new();
        for _ in 0..SCAN_CALLS {
            let mut cmd = ::redis::cmd("SCAN");
            cmd.arg(cursor).arg("MATCH").arg(&pattern).arg("COUNT").arg(SCAN_COUNT);
            let reply = self.run(node, cmd).await?;
            let Reply::Array(mut parts) = reply else {
                return Err(DbError::new(ErrorKind::Server, "SCAN answered without a cursor"));
            };
            let found = parts.pop();
            let next = parts.pop().as_ref().and_then(line::integer).and_then(|next| u64::try_from(next).ok());
            if let Some(Reply::Array(found)) = found {
                keys.extend(found.into_iter().map(line::name));
            }
            cursor = next.unwrap_or(0);
            if cursor == 0 {
                return Ok((keys, false));
            }
            if keys.len() >= KEYS_PER_LEVEL {
                break;
            }
        }
        Ok((keys, true))
    }

    async fn types(&mut self, node: Option<&(String, u16)>, keys: &[String]) -> Vec<Option<String>> {
        let keys = &keys[..keys.len().min(TYPED_KEYS)];
        if keys.is_empty() {
            return Vec::new();
        }
        if let (Link::Single(connection), None) = (&mut self.link, node) {
            let mut pipe = ::redis::pipe();
            for key in keys {
                pipe.cmd("TYPE").arg(key);
            }
            let replies: Result<Vec<Reply>, _> = pipe.query_async(connection).await;
            return match replies {
                Ok(replies) => replies.into_iter().map(|reply| Some(line::name(reply))).collect(),
                Err(_) => vec![None; keys.len()],
            };
        }
        let mut types = Vec::with_capacity(keys.len());
        for key in keys.iter().take(200) {
            let reply = self.link.query(::redis::cmd("TYPE").arg(key)).await.ok();
            types.push(reply.map(line::name));
        }
        types
    }

    async fn keys_under(&mut self, path: &[String]) -> Result<Vec<Node>, DbError> {
        let (node, db, prefix) = self.locate(path)?;
        self.select(db).await?;
        let (keys, more) = self.scan(node.as_ref(), &prefix).await?;
        let separator = self.target.separator.clone();
        let grouped = line::group(&prefix, keys.iter().map(String::as_str), &separator);
        let mut nodes = Vec::with_capacity(grouped.prefixes.len() + grouped.keys.len() + 1);
        for (segment, count) in &grouped.prefixes {
            let label = segment.strip_suffix(separator.as_str()).unwrap_or(segment).to_owned();
            let mut node_path = path.to_vec();
            node_path.push(segment.clone());
            nodes.push(Node::new(node_path, NodeKind::KeyPrefix, label, true).with_detail(Some(count.to_string())));
        }
        let full: Vec<String> = grouped.keys.iter().map(|segment| format!("{prefix}{segment}")).collect();
        let types = self.types(node.as_ref(), &full).await;
        for (index, segment) in grouped.keys.into_iter().enumerate() {
            let mut node_path = path.to_vec();
            node_path.push(segment.clone());
            let detail = types.get(index).cloned().flatten().map(|kind| shown_type(&kind).to_owned());
            nodes.push(Node::new(node_path, NodeKind::Key, segment, false).with_detail(detail));
        }
        if more {
            let mut node_path = path.to_vec();
            node_path.push("\u{2026}".to_owned());
            nodes.push(
                Node::new(node_path, NodeKind::Other("more".to_owned()), "\u{2026}", false)
                    .with_detail(Some(keys.len().to_string())),
            );
        }
        Ok(nodes)
    }

    async fn read_key(&mut self, path: &[String], request: &KeyRequest) -> Result<KeyValue, DbError> {
        let (_, db, key) = self.locate(path)?;
        self.select(db).await?;
        if let Some(change) = request.change {
            if self.read_only {
                return Err(read_only(if change == KeyChange::Persist { "PERSIST" } else { "EXPIRE" }));
            }
            let cmd = match change {
                KeyChange::Expire(seconds) => ::redis::cmd("EXPIRE").arg(&key).arg(seconds).clone(),
                KeyChange::Persist => ::redis::cmd("PERSIST").arg(&key).clone(),
            };
            self.run(None, cmd).await?;
        }
        let limit = request.limit.max(1);
        let kind = line::name(self.run(None, ::redis::cmd("TYPE").arg(&key).clone()).await?);
        let ttl = self.run(None, ::redis::cmd("PTTL").arg(&key).clone()).await.ok().as_ref().and_then(line::integer);
        let size = self
            .link
            .query(::redis::cmd("MEMORY").arg("USAGE").arg(&key))
            .await
            .ok()
            .as_ref()
            .and_then(line::integer)
            .and_then(|size| u64::try_from(size).ok());
        let encoding = match self.link.query(::redis::cmd("OBJECT").arg("ENCODING").arg(&key)).await {
            Ok(Reply::Nil) | Err(_) => None,
            Ok(reply) => Some(line::name(reply)),
        };
        let mut value = KeyValue {
            key: key.clone(),
            database: path.first().cloned(),
            type_name: shown_type(&kind).to_owned(),
            ttl_ms: ttl.filter(|ttl| *ttl >= 0),
            size,
            encoding,
            ..KeyValue::default()
        };
        let column = |name: &str, kind: &str| Column::new(name.to_owned(), kind.to_owned());
        let count = |reply: Reply| line::integer(&reply).and_then(|count| u64::try_from(count).ok());
        match kind.as_str() {
            "string" => {
                value.length = count(self.run(None, ::redis::cmd("STRLEN").arg(&key).clone()).await?);
                let end = i64::try_from(STRING_BYTES).unwrap_or(i64::MAX) - 1;
                let reply = self.run(None, ::redis::cmd("GETRANGE").arg(&key).arg(0).arg(end).clone()).await?;
                let read = match reply {
                    Reply::BulkString(bytes) => bytes,
                    other => line::name(other).into_bytes(),
                };
                value.truncated = value.length.is_some_and(|length| length > read.len() as u64);
                let shown = match line::text(read) {
                    Value::Text(text) if !value.truncated && looks_like_json(&text) => Value::Json(text),
                    other => other,
                };
                value.columns = vec![column("value", "string")];
                value.rows = vec![vec![shown]];
            }
            "hash" => {
                value.length = count(self.run(None, ::redis::cmd("HLEN").arg(&key).clone()).await?);
                let items = self.scan_items("HSCAN", &key, limit).await?;
                value.columns = vec![column("field", "string"), column("value", "string")];
                value.rows = line::pairs(Reply::Array(items))
                    .into_iter()
                    .take(limit)
                    .map(|(field, item)| vec![line::text(bytes(field)), line::text(bytes(item))])
                    .collect();
            }
            "list" => {
                value.length = count(self.run(None, ::redis::cmd("LLEN").arg(&key).clone()).await?);
                let end = i64::try_from(limit).unwrap_or(i64::MAX) - 1;
                let reply = self.run(None, ::redis::cmd("LRANGE").arg(&key).arg(0).arg(end).clone()).await?;
                value.columns = vec![column("index", "integer"), column("value", "string")];
                if let Reply::Array(items) = reply {
                    value.rows = items
                        .into_iter()
                        .enumerate()
                        .map(|(index, item)| {
                            vec![Value::Integer(i64::try_from(index).unwrap_or(i64::MAX)), line::text(bytes(item))]
                        })
                        .collect();
                }
            }
            "set" => {
                value.length = count(self.run(None, ::redis::cmd("SCARD").arg(&key).clone()).await?);
                let items = self.scan_items("SSCAN", &key, limit).await?;
                value.columns = vec![column("member", "string")];
                value.rows = items.into_iter().take(limit).map(|item| vec![line::text(bytes(item))]).collect();
            }
            "zset" => {
                value.length = count(self.run(None, ::redis::cmd("ZCARD").arg(&key).clone()).await?);
                let end = i64::try_from(limit).unwrap_or(i64::MAX) - 1;
                let reply =
                    self.run(None, ::redis::cmd("ZRANGE").arg(&key).arg(0).arg(end).arg("WITHSCORES").clone()).await?;
                value.columns = vec![column("member", "string"), column("score", "double")];
                value.rows = line::pairs(reply)
                    .into_iter()
                    .map(|(member, score)| vec![line::text(bytes(member)), score_of(score)])
                    .collect();
            }
            "stream" => {
                value.length = count(self.run(None, ::redis::cmd("XLEN").arg(&key).clone()).await?);
                let reply = self
                    .run(None, ::redis::cmd("XRANGE").arg(&key).arg("-").arg("+").arg("COUNT").arg(limit).clone())
                    .await?;
                value.columns = vec![column("id", "string"), column("fields", "map")];
                value.rows = line::pairs(reply)
                    .into_iter()
                    .map(|(id, fields)| {
                        let fields = line::pairs(fields)
                            .into_iter()
                            .map(|(field, item)| (line::name(field), line::text(bytes(item))))
                            .collect();
                        vec![Value::Text(line::name(id)), Value::Map(fields)]
                    })
                    .collect();
            }
            "ReJSON-RL" => {
                let reply = self.run(None, ::redis::cmd("JSON.GET").arg(&key).clone()).await?;
                value.columns = vec![column("value", "json")];
                value.rows = vec![vec![Value::Json(line::name(reply))]];
            }
            _ => {}
        }
        value.truncated |= value.length.is_some_and(|length| length > value.rows.len() as u64) && kind != "string";
        Ok(value)
    }

    async fn scan_items(&mut self, verb: &str, key: &str, limit: usize) -> Result<Vec<Reply>, DbError> {
        let mut cursor: u64 = 0;
        let mut items = Vec::new();
        let wanted = if verb == "HSCAN" { limit.saturating_mul(2) } else { limit };
        loop {
            let reply = self.run(None, ::redis::cmd(verb).arg(key).arg(cursor).arg("COUNT").arg(500).clone()).await?;
            let Reply::Array(mut parts) = reply else { break };
            let found = parts.pop();
            cursor =
                parts.pop().as_ref().and_then(line::integer).and_then(|next| u64::try_from(next).ok()).unwrap_or(0);
            match found {
                Some(Reply::Array(found)) => items.extend(found),
                Some(Reply::Map(found)) => items.extend(found.into_iter().flat_map(|(field, item)| [field, item])),
                _ => {}
            }
            if cursor == 0 || items.len() >= wanted {
                break;
            }
        }
        Ok(items)
    }
}

type Located = (Option<(String, u16)>, i64, String);

fn bytes(reply: Reply) -> Vec<u8> {
    match reply {
        Reply::BulkString(bytes) => bytes,
        other => line::name(other).into_bytes(),
    }
}

fn score_of(reply: Reply) -> Value {
    match reply {
        Reply::Double(score) => Value::Float(score),
        other => {
            let text = line::name(other);
            text.parse().map_or(Value::Text(text), Value::Float)
        }
    }
}

fn looks_like_json(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with('{') || trimmed.starts_with('[')) && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
}

fn shown_type(kind: &str) -> &str {
    if kind == "ReJSON-RL" { "json" } else { kind }
}

fn parse_db(name: &str) -> Option<i64> {
    name.trim().trim_start_matches("db").parse().ok()
}

fn resp2_only(failed: &RedisError) -> bool {
    failed.kind() == ::redis::ErrorKind::RESP3NotSupported || failed.to_string().to_ascii_lowercase().contains("hello")
}

fn read_only(verb: &str) -> DbError {
    DbError::new(ErrorKind::Server, format!("the connection is read-only; {verb} changes data")).with_code("readOnly")
}

struct Cancel {
    opener: Arc<Opener>,
    client_id: i64,
}

impl Canceller for Cancel {
    fn cancel(&self) -> BoxFuture<'static, Result<(), DbError>> {
        let (opener, id) = (Arc::clone(&self.opener), self.client_id);
        Box::pin(async move {
            let mut connection = opener.open().await.map_err(|failed| error(&failed))?;
            let _: Reply = ::redis::cmd("CLIENT")
                .arg("UNBLOCK")
                .arg(id)
                .arg("ERROR")
                .query_async(&mut connection)
                .await
                .map_err(|failed| error(&failed))?;
            Ok(())
        })
    }
}

impl DriverSession for Session {
    fn execute<'a>(
        &'a mut self,
        statement: &'a str,
        options: &'a ExecOptions,
    ) -> BoxFuture<'a, Result<Execution, DbError>> {
        Box::pin(async move {
            let args = line::split(statement.trim())
                .ok_or_else(|| DbError::new(ErrorKind::Invalid, "a quote is not closed"))?;
            let Some(first) = args.first() else { return Ok(Execution::default()) };
            let verb = String::from_utf8_lossy(first).to_ascii_uppercase();
            if UNSUPPORTED.contains(&verb.to_ascii_lowercase().as_str()) {
                return Err(DbError::new(ErrorKind::Unsupported, format!("{verb} does not answer as one reply")));
            }
            if self.read_only && self.writes(&args).await {
                return Err(read_only(&verb));
            }
            let mut cmd = ::redis::cmd(&verb);
            for arg in &args[1..] {
                cmd.arg(arg.as_slice());
            }
            let reply = self.run(None, cmd).await;
            match verb.as_str() {
                "MULTI" if reply.is_ok() => self.in_multi = true,
                "EXEC" | "DISCARD" => self.in_multi = false,
                "SELECT" if reply.is_ok() => {
                    if let Some(db) = args.get(1).and_then(|arg| std::str::from_utf8(arg).ok()).and_then(parse_db) {
                        self.db = db;
                    }
                }
                _ => {}
            }
            let (columns, rows, truncated) = line::rows(reply?, options.max_rows);
            Ok(Execution { columns, rows, truncated, affected: None, notices: Vec::new() })
        })
    }

    fn children<'a>(&'a mut self, path: &'a [String]) -> BoxFuture<'a, Result<Vec<Node>, DbError>> {
        Box::pin(async move { if path.is_empty() { self.roots().await } else { self.keys_under(path).await } })
    }

    fn key<'a>(&'a mut self, path: &'a [String], request: &'a KeyRequest) -> BoxFuture<'a, Result<KeyValue, DbError>> {
        Box::pin(self.read_key(path, request))
    }

    fn ping(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move { self.run(None, ::redis::cmd("PING")).await.map(|_| ()) })
    }

    fn server_version(&mut self) -> BoxFuture<'_, Result<Option<String>, DbError>> {
        Box::pin(async move {
            if self.version.is_none() {
                let reply = self.run(None, ::redis::cmd("INFO").arg("server").clone()).await?;
                self.version = version_of(&line::name(reply));
            }
            Ok(self.version.clone())
        })
    }

    fn selected_database(&self) -> Option<String> {
        (self.target.mode != Mode::Cluster).then(|| self.db.to_string())
    }

    fn apply<'a>(&'a mut self, state: &'a TrackedState) -> BoxFuture<'a, Result<(), DbError>> {
        Box::pin(async move {
            self.read_only = state.read_only;
            if let Some(db) = state.database.as_deref().and_then(parse_db) {
                self.select(db).await?;
            }
            Ok(())
        })
    }

    fn commit(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            if self.in_multi {
                self.in_multi = false;
                self.run(None, ::redis::cmd("EXEC")).await?;
            }
            Ok(())
        })
    }

    fn rollback(&mut self) -> BoxFuture<'_, Result<(), DbError>> {
        Box::pin(async move {
            if self.in_multi {
                self.in_multi = false;
                self.run(None, ::redis::cmd("DISCARD")).await?;
            }
            Ok(())
        })
    }

    fn in_transaction(&self) -> bool {
        self.in_multi
    }

    fn is_closed(&self) -> bool {
        self.closed
    }

    fn canceller(&self) -> Option<Arc<dyn Canceller>> {
        let opener = Arc::clone(self.opener.as_ref()?);
        Some(Arc::new(Cancel { opener, client_id: self.client_id? }))
    }

    fn take_notices(&mut self) -> Vec<Notice> {
        Vec::new()
    }
}

pub fn version_of(info: &str) -> Option<String> {
    let field = |name: &str| {
        info.lines()
            .find_map(|line| line.trim().strip_prefix(name).and_then(|rest| rest.strip_prefix(':')))
            .map(str::to_owned)
    };
    for (name, product) in
        [("valkey_version", "Valkey"), ("dragonfly_version", "Dragonfly"), ("garnet_version", "Garnet")]
    {
        if let Some(version) = field(name) {
            return Some(format!("{product} {version}"));
        }
    }
    let redis = field("redis_version")?;
    if info.to_ascii_lowercase().contains("keydb") {
        return Some(format!("KeyDB {redis}"));
    }
    Some(format!("Redis {redis}"))
}

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use crate::types::DEVELOPMENT;

pub const POOLER: &str = "pooler";

/// Where a connection is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// In the IDE's data folder, usable in every project.
    #[default]
    Global,
    /// In the project's `.zen/data-sources.json`, shared through Git, never with a secret.
    Project,
}

/// How a connection proves who it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AuthKind {
    /// A user name and a password.
    #[default]
    Password,
    /// Nothing beyond the user name (trust, peer, a client certificate).
    None,
}

pub fn passwordless_by_default(driver: &str, user: Option<&str>) -> bool {
    match driver {
        "redis" | "sqlite" => true,
        "clickhouse" => user.is_none_or(|user| user.is_empty() || user == "default"),
        _ => false,
    }
}

/// How TLS is used, as libpq names the modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TlsMode {
    /// Never.
    Disable,
    /// When the server offers it, without checking its certificate.
    #[default]
    Prefer,
    /// Always, without checking the certificate.
    Require,
    /// Always, checking the certificate's chain but not its host name.
    VerifyCa,
    /// Always, checking the chain and the host name.
    VerifyFull,
}

pub const TLS_MODE_KEYS: [&str; 3] = ["sslmode", "ssl-mode", "sslMode"];

pub fn tls_mode_named(value: &str) -> Option<&'static str> {
    Some(match value.to_ascii_lowercase().replace('_', "-").as_str() {
        "disable" | "disabled" => "disable",
        "prefer" | "preferred" => "prefer",
        "require" | "required" => "require",
        "verify-ca" => "verify-ca",
        "verify-full" | "verify-identity" => "verify-full",
        _ => return None,
    })
}

/// TLS settings of a connection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TlsSettings {
    /// The mode.
    pub mode: TlsMode,
    /// A PEM file of the certificate authorities to trust; the system's web roots when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_file: Option<PathBuf>,
    /// A PEM file with the client's certificate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_cert: Option<PathBuf>,
    /// A PEM file with the client certificate's key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_key: Option<PathBuf>,
    /// Whether TLS starts at once, without asking the server first (PostgreSQL 17 `sslnegotiation=direct`).
    pub direct: bool,
}

/// An SSH tunnel through the system's OpenSSH (ADR-0165): the database is reached through `ssh -N -L` from this host.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SshSettings {
    /// A host alias of `~/.ssh/config`, or a host name.
    pub host: String,
    /// The SSH user, when not set by the alias.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// The SSH port, when not 22 or set by the alias.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Jump hosts to go through first (`ssh -J`, comma-separated), when not set by the alias's ProxyJump.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jump: Option<String>,
}

/// How the navigator shows a connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NavigatorOptions {
    /// The navigator view: `simple`, `advanced` or `custom`.
    pub view: String,
    /// Whether every database of the server is shown, not only the connection's.
    pub show_all_databases: bool,
    /// Whether system schemas and objects are shown.
    pub show_system_objects: bool,
}

impl Default for NavigatorOptions {
    fn default() -> Self {
        Self { view: "advanced".to_owned(), show_all_databases: true, show_system_objects: false }
    }
}

/// A filter of the navigator's objects of one kind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ObjectFilter {
    /// The kind of objects it filters (`catalog`, `schema`, `table`).
    pub kind: String,
    /// Name masks to show (`*` and `?`); all when empty.
    pub include: Vec<String>,
    /// Name masks to hide.
    pub exclude: Vec<String>,
}

/// What runs after connecting.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Bootstrap {
    /// The schema (or key namespace) made current after connecting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_schema: Option<String>,
    /// Statements run after connecting, on every execution context.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub init_statements: Vec<String>,
    /// The isolation level set after connecting, by the driver's name for it; the server's default when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isolation: Option<String>,
}

/// Settings of one connection that override its connection type's.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TypeOverrides {
    /// Auto-commit, instead of the type's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_commit: Option<bool>,
    /// Confirm data-changing statements, instead of the type's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm_execute: Option<bool>,
    /// Confirm saving edited data, instead of the type's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirm_data_change: Option<bool>,
    /// Seconds after which an idle transaction ends, instead of the type's; 0 never.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_idle_transactions_after: Option<u64>,
    /// Seconds after which an idle connection closes, instead of the type's; 0 never.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_idle_after: Option<u64>,
    /// The statement timeout in seconds, instead of the type's; 0 none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement_timeout: Option<u64>,
    /// What agents may run, instead of the type's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<crate::types::AgentAccess>,
}

/// A connection as the registry keeps it: never a secret.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataSource {
    /// Its stable id, which also names its secrets (`<id>/password`) and its consoles' folder.
    pub id: String,
    /// Its name as shown.
    pub name: String,
    /// The folder it is shown in, a path such as `Team/Reporting`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The driver's id (`postgresql`).
    pub driver: String,
    /// The database profile over the driver (`cockroachdb`, ADR-0173); the driver's own server when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// The connection type's id.
    #[serde(rename = "type", default = "development")]
    pub connection_type: String,
    /// The server's host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The server's port; the driver's default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The database to connect to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// The user name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// A URI the settings were taken from, without its password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// How it authenticates.
    #[serde(default)]
    pub auth: AuthKind,
    /// Whether its password is kept in the system's secret store; otherwise it is asked at each connect.
    #[serde(default = "yes")]
    pub save_password: bool,
    /// Whether a password was saved in the secret store, so that `hasPassword` is known without reading the store.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub password_saved: bool,
    /// Whether it refuses statements that change data.
    #[serde(default)]
    pub read_only: bool,
    /// TLS settings.
    #[serde(default)]
    pub tls: TlsSettings,
    /// An SSH tunnel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh: Option<SshSettings>,
    /// How the navigator shows it.
    #[serde(default)]
    pub navigator: NavigatorOptions,
    /// Filters of the navigator's objects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<ObjectFilter>,
    /// What runs after connecting.
    #[serde(default)]
    pub bootstrap: Bootstrap,
    /// Settings that override the connection type's.
    #[serde(default)]
    pub overrides: TypeOverrides,
    /// The driver's own settings (`pooler` for PostgreSQL behind pgbouncer, Redis's `mode`).
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub settings: Map<String, Json>,
    /// Where it was found or imported from, when it did not start in the connection dialog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Fields this version does not know, kept as they were.
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Map<String, Json>,
}

/// What a connection was made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OriginKind {
    /// A `.env` file of the project.
    Env,
    /// A Docker Compose file of the project.
    Compose,
    /// DBeaver's `data-sources.json`.
    Dbeaver,
    /// A JetBrains project's `.idea/dataSources.xml`.
    Jetbrains,
}

/// Where a connection was found or imported from (ADR-0165, decisions 5 and 6).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    /// The kind of file.
    pub kind: OriginKind,
    /// The file: relative to the project for a detection, the file's name for an import.
    pub file: String,
    /// The key, service or entry of the file it came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The detection it was added from (`db.detect.list`'s `id`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detection: Option<String>,
    /// The detection's settings when it was added, so that a change in the file is offered again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
}

fn development() -> String {
    DEVELOPMENT.to_owned()
}

fn yes() -> bool {
    true
}

impl DataSource {
    pub fn flag(&self, key: &str) -> bool {
        self.settings.get(key).and_then(Json::as_bool).unwrap_or(false)
    }

    pub fn forget_secrets(&mut self) {
        if let Some(url) = &self.url {
            self.url =
                Some(parse_uri(url).map_or_else(|| without_secret_params(url), |parsed| parsed.without_password));
        }
        self.settings.retain(|key, _| !is_secret_param(key));
    }
}

/// A person's own settings of a project connection, kept outside the project.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PersonalOverride {
    /// The user name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// A local port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Read-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    /// The connection type's id.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub connection_type: Option<String>,
    /// Whether the password is saved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub save_password: Option<bool>,
    /// Whether the person saved a password in the secret store.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_saved: Option<bool>,
}

impl PersonalOverride {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    pub fn apply(&self, mut source: DataSource) -> DataSource {
        if let Some(user) = &self.user {
            source.user = Some(user.clone());
        }
        if let Some(port) = self.port {
            source.port = Some(port);
        }
        if let Some(read_only) = self.read_only {
            source.read_only = read_only;
        }
        if let Some(kind) = &self.connection_type {
            source.connection_type = kind.clone();
        }
        if let Some(save) = self.save_password {
            source.save_password = save;
        }
        if let Some(saved) = self.password_saved {
            source.password_saved = saved;
        }
        source
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourcesFile {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<String>,
    #[serde(default)]
    pub connections: Vec<DataSource>,
    #[serde(flatten)]
    pub extra: Map<String, Json>,
}

fn one() -> u32 {
    1
}

impl Default for SourcesFile {
    fn default() -> Self {
        Self { version: 1, folders: Vec::new(), connections: Vec::new(), extra: Map::new() }
    }
}

impl Default for OverridesFile {
    fn default() -> Self {
        Self { version: 1, connections: BTreeMap::new(), extra: Map::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OverridesFile {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub connections: BTreeMap<String, PersonalOverride>,
    #[serde(flatten)]
    pub extra: Map<String, Json>,
}

/// A connection as the registry answers it: the settings in effect and where they come from.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Where it is kept.
    pub scope: Scope,
    /// The connection with the person's overrides applied.
    #[serde(flatten)]
    pub source: DataSource,
    /// The person's overrides of a project connection, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub personal: Option<PersonalOverride>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    pub global: Option<PathBuf>,
    pub project: Option<PathBuf>,
    pub overrides: Option<PathBuf>,
}

impl Files {
    pub fn new(data: Option<&Path>, project: Option<(&Path, &str)>) -> Self {
        let databases = data.map(|data| data.join("databases"));
        Self {
            global: databases.as_ref().map(|folder| folder.join("data-sources.json")),
            project: project.map(|(root, _)| root.join(".zen").join("data-sources.json")),
            overrides: databases.zip(project).map(|(folder, (_, key))| folder.join(key).join("overrides.json")),
        }
    }

    pub fn of(&self, scope: Scope) -> Option<&Path> {
        match scope {
            Scope::Global => self.global.as_deref(),
            Scope::Project => self.project.as_deref(),
        }
    }

    pub fn read(&self, scope: Scope) -> std::io::Result<SourcesFile> {
        match self.of(scope) {
            Some(file) => read_json::<SourcesFile>(file).map(Option::unwrap_or_default).map(|mut sources| {
                sources.connections.iter_mut().for_each(DataSource::forget_secrets);
                sources
            }),
            None => Ok(SourcesFile::default()),
        }
    }

    pub fn write(&self, scope: Scope, sources: &SourcesFile) -> std::io::Result<()> {
        let Some(file) = self.of(scope) else { return Err(std::io::Error::other("no file for this scope")) };
        let mut sources = sources.clone();
        sources.connections.iter_mut().for_each(DataSource::forget_secrets);
        if read_json::<SourcesFile>(file).ok().flatten().as_ref() == Some(&sources) {
            return Ok(());
        }
        write_json(file, &sources)
    }

    pub fn read_overrides(&self) -> OverridesFile {
        self.overrides.as_deref().and_then(|file| read_json(file).ok().flatten()).unwrap_or_default()
    }

    pub fn write_overrides(&self, overrides: &OverridesFile) -> std::io::Result<()> {
        let Some(file) = &self.overrides else { return Ok(()) };
        if read_json::<OverridesFile>(file).ok().flatten().as_ref() == Some(overrides) {
            return Ok(());
        }
        write_json(file, overrides)
    }

    pub fn entries(&self) -> std::io::Result<Vec<Entry>> {
        let overrides = self.read_overrides();
        let mut entries = Vec::new();
        for scope in [Scope::Project, Scope::Global] {
            for source in self.read(scope)?.connections {
                let personal =
                    (scope == Scope::Project).then(|| overrides.connections.get(&source.id).cloned()).flatten();
                let source =
                    personal.as_ref().map_or_else(|| source.clone(), |personal| personal.apply(source.clone()));
                entries.push(Entry { scope, source, personal });
            }
        }
        Ok(entries)
    }

    pub fn folders(&self) -> std::io::Result<Vec<(Scope, String)>> {
        let mut folders = Vec::new();
        for scope in [Scope::Project, Scope::Global] {
            let file = self.read(scope)?;
            let mut names: Vec<String> = file.folders.clone();
            names.extend(file.connections.iter().filter_map(|source| source.folder.clone()));
            names.sort();
            names.dedup();
            folders.extend(names.into_iter().map(|name| (scope, name)));
        }
        Ok(folders)
    }
}

pub fn new_id(name: &str) -> String {
    use std::hash::{BuildHasher, Hasher};
    let slug: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug.chars().take(24).collect();
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher
        .write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    let suffix = format!("{:08x}", hasher.finish() as u32);
    if slug.is_empty() { suffix } else { format!("{slug}-{suffix}") }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedUri {
    pub scheme: String,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub database: Option<String>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub params: Vec<(String, String)>,
    pub without_password: String,
}

pub fn parse_uri(uri: &str) -> Option<ParsedUri> {
    if let Some(parsed) = parse_file_uri(uri) {
        return Some(parsed);
    }
    let (scheme, rest) = uri.split_once("://")?;
    let (rest, query) = rest.split_once('?').map_or((rest, None), |(rest, query)| (rest, Some(query)));
    let (authority, path) = rest.split_once('/').map_or((rest, None), |(authority, path)| (authority, Some(path)));
    let (credentials, address) =
        authority.rsplit_once('@').map_or((None, authority), |(credentials, address)| (Some(credentials), address));
    let (user, password) = match credentials {
        Some(credentials) => match credentials.split_once(':') {
            Some((user, password)) => (Some(decode(user)), Some(decode(password))),
            None => (Some(decode(credentials)), None),
        },
        None => (None, None),
    };
    let (host, port) = if address.contains(',') || scheme.ends_with("+srv") {
        (address.to_owned(), None)
    } else if let Some(bracketed) = address.strip_prefix('[') {
        let (host, rest) = bracketed.split_once(']')?;
        (host.to_owned(), rest.strip_prefix(':'))
    } else {
        match address.rsplit_once(':') {
            Some((host, port)) => (host.to_owned(), Some(port)),
            None => (address.to_owned(), None),
        }
    };
    let port = match port {
        Some(port) if !port.is_empty() => Some(port.parse().ok()?),
        _ => None,
    };
    let (params, secrets) = split_params(query);
    let without_password = match (credentials, &password) {
        (Some(credentials), Some(_)) => {
            let user = credentials.split_once(':').map_or(credentials, |(user, _)| user);
            uri.replacen(&format!("{credentials}@"), &format!("{user}@"), 1)
        }
        _ => uri.to_owned(),
    };
    let without_password = without_secret_params(&without_password);
    let password = password.or_else(|| secrets.into_iter().find(|(key, _)| key == "password").map(|(_, value)| value));
    Some(ParsedUri {
        scheme: scheme.to_ascii_lowercase(),
        host: (!host.is_empty()).then(|| decode(&host)),
        port,
        database: path.filter(|path| !path.is_empty()).map(decode),
        user: user.filter(|user| !user.is_empty()),
        password,
        params,
        without_password,
    })
}

fn parse_file_uri(uri: &str) -> Option<ParsedUri> {
    let (scheme, rest) = uri.split_once(':')?;
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "sqlite" | "sqlite3" | "file" => "sqlite",
        "duckdb" => "duckdb",
        _ => return None,
    };
    let rest = rest.strip_prefix("file:").unwrap_or(rest);
    let (rest, query) = rest.split_once('?').map_or((rest, None), |(rest, query)| (rest, Some(query)));
    let path = match rest.strip_prefix("//") {
        Some(after) if after.starts_with('/') => format!("/{}", after.trim_start_matches('/')),
        Some(after) => match after.strip_prefix("localhost/") {
            Some(absolute) => format!("/{}", absolute.trim_start_matches('/')),
            None => after.to_owned(),
        },
        None if rest.starts_with('/') => format!("/{}", rest.trim_start_matches('/')),
        None => rest.to_owned(),
    };
    let (params, _) = split_params(query);
    Some(ParsedUri {
        scheme: scheme.to_owned(),
        database: (!path.is_empty()).then(|| decode(&path)),
        params,
        without_password: without_secret_params(uri),
        ..ParsedUri::default()
    })
}

pub const SECRET_PARAMS: [&str; 2] = ["password", "sslpassword"];

type Params = Vec<(String, String)>;

fn split_params(query: Option<&str>) -> (Params, Params) {
    query
        .into_iter()
        .flat_map(|query| query.split('&'))
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            (decode(key), decode(value))
        })
        .partition(|(key, _)| !is_secret_param(key))
}

fn is_secret_param(key: &str) -> bool {
    SECRET_PARAMS.iter().any(|secret| secret.eq_ignore_ascii_case(key))
}

pub fn without_secret_params(uri: &str) -> String {
    let Some((head, query)) = uri.split_once('?') else { return uri.to_owned() };
    let kept: Vec<&str> = query
        .split('&')
        .filter(|pair| {
            !pair.is_empty() && !is_secret_param(&decode(pair.split_once('=').map_or(*pair, |(key, _)| key)))
        })
        .collect();
    if kept.is_empty() { head.to_owned() } else { format!("{head}?{}", kept.join("&")) }
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(byte) = text.get(at + 1..at + 3).and_then(|pair| u8::from_str_radix(pair, 16).ok())
        {
            out.push(byte);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail.find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'').unwrap_or(tail.len());
        let authority = &tail[..end];
        match authority.rsplit_once('@').and_then(|(credentials, host)| Some((credentials.split_once(':')?, host))) {
            Some(((user, _), host)) => {
                out.push_str(user);
                out.push_str(":***@");
                out.push_str(host);
            }
            None => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

pub fn read_json<T: DeserializeOwned>(file: &Path) -> std::io::Result<Option<T>> {
    match std::fs::read(file) {
        Ok(bytes) => serde_json::from_slice(&bytes).map(Some).map_err(std::io::Error::other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

pub fn write_json<T: Serialize>(file: &Path, value: &T) -> std::io::Result<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    text.push('\n');
    let temporary = file.with_extension("json.tmp");
    std::fs::write(&temporary, text)?;
    std::fs::rename(&temporary, file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_split_and_lose_their_password() {
        let parsed =
            parse_uri("postgresql://ann:p%40ss@db.example.com:6543/shop?sslmode=verify-full&pooler=true").unwrap();
        assert_eq!(parsed.scheme, "postgresql");
        assert_eq!((parsed.host.as_deref(), parsed.port), (Some("db.example.com"), Some(6543)));
        assert_eq!((parsed.user.as_deref(), parsed.password.as_deref()), (Some("ann"), Some("p@ss")));
        assert_eq!(parsed.database.as_deref(), Some("shop"));
        assert_eq!(
            parsed.without_password,
            "postgresql://ann@db.example.com:6543/shop?sslmode=verify-full&pooler=true"
        );
        let ipv6 = parse_uri("postgres://[::1]:5432").unwrap();
        assert_eq!((ipv6.host.as_deref(), ipv6.port, ipv6.database), (Some("::1"), Some(5432), None));
        assert!(parse_uri("not a uri").is_none());
        for (uri, scheme, path) in [
            ("sqlite:///abs/path.db", "sqlite", "/abs/path.db"),
            ("sqlite:////abs/path.db", "sqlite", "/abs/path.db"),
            ("sqlite://localhost/abs/path.db", "sqlite", "/abs/path.db"),
            ("sqlite:/abs/path.db", "sqlite", "/abs/path.db"),
            ("sqlite:data/shop.db", "sqlite", "data/shop.db"),
            ("sqlite://data/shop.db", "sqlite", "data/shop.db"),
            ("sqlite:file:/abs/my%20shop.db?mode=ro", "sqlite", "/abs/my shop.db"),
            ("file:///abs/path.db", "sqlite", "/abs/path.db"),
            ("file:shop.db", "sqlite", "shop.db"),
            ("duckdb:///abs/lake.duckdb", "duckdb", "/abs/lake.duckdb"),
        ] {
            let parsed = parse_uri(uri).unwrap();
            assert_eq!(
                (parsed.scheme.as_str(), parsed.database.as_deref(), parsed.host),
                (scheme, Some(path), None),
                "{uri}"
            );
        }
        assert_eq!(
            redact("failed: postgresql://ann:secret@host/db refused"),
            "failed: postgresql://ann:***@host/db refused"
        );
        assert_eq!(redact("no uri here"), "no uri here");
        assert_eq!(tls_mode_named("VERIFY_IDENTITY"), Some("verify-full"));
        assert_eq!((tls_mode_named("REQUIRED"), tls_mode_named("verify-ca")), (Some("require"), Some("verify-ca")));
        assert_eq!(tls_mode_named("sometimes"), None);
    }

    #[test]
    fn a_password_given_as_a_parameter_is_taken_out_of_the_uri() {
        let parsed = parse_uri("postgresql://ann@h/db?password=p%26w&sslmode=require&sslpassword=k&x=1").unwrap();
        assert_eq!(parsed.password.as_deref(), Some("p&w"));
        assert_eq!(parsed.without_password, "postgresql://ann@h/db?sslmode=require&x=1");
        assert_eq!(parsed.params, vec![("sslmode".to_owned(), "require".to_owned()), ("x".to_owned(), "1".to_owned())]);
        let both = parse_uri("mysql://ann:inline@h/db?password=param").unwrap();
        assert_eq!((both.password.as_deref(), both.without_password.as_str()), (Some("inline"), "mysql://ann@h/db"));
        let file = parse_uri("sqlite:/tmp/a.db?mode=ro&password=x").unwrap();
        assert_eq!(file.without_password, "sqlite:/tmp/a.db?mode=ro");
    }

    #[test]
    fn redis_and_mongodb_uris_split_too() {
        let redis = parse_uri("rediss://default:pw@cache.example.com:6380/2").unwrap();
        assert_eq!(
            (redis.scheme.as_str(), redis.host.as_deref(), redis.port),
            ("rediss", Some("cache.example.com"), Some(6380))
        );
        assert_eq!(
            (redis.user.as_deref(), redis.password.as_deref(), redis.database.as_deref()),
            (Some("default"), Some("pw"), Some("2"))
        );
        assert_eq!(redis.without_password, "rediss://default@cache.example.com:6380/2");
        let srv = parse_uri("mongodb+srv://ann:pw@cluster0.example.net/app?retryWrites=true").unwrap();
        assert_eq!(
            (srv.scheme.as_str(), srv.host.as_deref(), srv.port),
            ("mongodb+srv", Some("cluster0.example.net"), None)
        );
        assert_eq!(srv.params, vec![("retryWrites".to_owned(), "true".to_owned())]);
        let replicas = parse_uri("mongodb://a.local:27017,b.local:27018/app?replicaSet=rs0").unwrap();
        assert_eq!((replicas.host.as_deref(), replicas.port), (Some("a.local:27017,b.local:27018"), None));
    }

    #[test]
    fn only_servers_that_commonly_need_none_go_without_a_password() {
        assert!(passwordless_by_default(crate::redis::ID, Some("default")));
        assert!(passwordless_by_default(crate::sqlite::ID, None));
        assert!(passwordless_by_default(crate::clickhouse::ID, None));
        assert!(passwordless_by_default(crate::clickhouse::ID, Some("default")));
        assert!(!passwordless_by_default(crate::clickhouse::ID, Some("analyst")));
        assert!(!passwordless_by_default(crate::postgres::ID, None));
        assert!(!passwordless_by_default(crate::mysql::ID, Some("root")));
    }

    #[test]
    fn unknown_fields_are_kept_and_unchanged_files_are_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let files = Files::new(Some(dir.path()), None);
        let file = files.global.clone().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, r#"{"version":1,"future":{"x":1},"connections":[{"id":"a","name":"A","driver":"postgresql","later":true}]}"#).unwrap();
        let read = files.read(Scope::Global).unwrap();
        assert_eq!(read.connections[0].connection_type, "dev");
        assert_eq!(read.connections[0].extra.get("later"), Some(&Json::Bool(true)));
        let mut read = read;
        files.write(Scope::Global, &read).unwrap();
        assert!(
            std::fs::read_to_string(&file).unwrap().starts_with("{\"version\":1"),
            "an unchanged file is not written"
        );
        read.folders.push("Team".to_owned());
        files.write(Scope::Global, &read).unwrap();
        let written = std::fs::read_to_string(&file).unwrap();
        assert!(written.contains("\"future\"") && written.contains("\"later\": true"), "{written}");
        let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        files.write(Scope::Global, &read).unwrap();
        assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), modified);
        assert!(new_id("My DB!").starts_with("my-db-"));
    }
}

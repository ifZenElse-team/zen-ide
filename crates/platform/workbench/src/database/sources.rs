use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};
use zen_core::{Command, CommandError, Session};
use zen_db::engine::OperationKind;
use zen_db::registry::{
    AuthKind, Bootstrap, DataSource, Entry, Files, NavigatorOptions, ObjectFilter, PersonalOverride, Scope,
    SshSettings, TlsSettings, TypeOverrides, new_id, parse_uri,
};
use zen_db::types::{COLORS, ConnectionType, ConnectionTypes};
use zen_i18n::t;
use zen_secrets::database::Kind;
use zen_secrets::{Secret, SecretError, SecretId, SecretStore};

use super::{Secrets, project_key};
use crate::Workbench;
use crate::workspaces::Workspaces;

const LISTED: usize = 50;

pub struct Database {
    engine: Option<zen_db::engine::Engine>,
    drivers: Option<Vec<Arc<dyn zen_db::driver::Driver>>>,
    more_drivers: zen_db::engine::MoreDrivers,
    application_name: String,
    waker: Option<zen_db::Waker>,
}

impl Default for Database {
    fn default() -> Self {
        Self {
            engine: None,
            drivers: None,
            more_drivers: Arc::new(Vec::new),
            application_name: "Zen IDE".to_owned(),
            waker: None,
        }
    }
}

impl Database {
    pub fn set_application_name(&mut self, name: impl Into<String>) {
        self.application_name = name.into();
    }

    pub fn set_drivers(&mut self, drivers: Vec<Arc<dyn zen_db::driver::Driver>>) {
        self.drivers = Some(drivers);
    }

    pub fn set_more_drivers(&mut self, more: zen_db::engine::MoreDrivers) {
        if let Some(engine) = &self.engine {
            engine.set_more_drivers(Some(Arc::clone(&more)));
        }
        self.more_drivers = more;
    }

    pub fn set_waker(&mut self, waker: zen_db::Waker) {
        if let Some(engine) = &self.engine {
            engine.set_waker(Arc::clone(&waker));
        }
        self.waker = Some(waker);
    }

    pub fn engine(&mut self) -> zen_db::engine::Engine {
        if let Some(engine) = &self.engine {
            return engine.clone();
        }
        let drivers = self.drivers.clone().unwrap_or_else(zen_db::drivers);
        let engine = zen_db::engine::Engine::new(drivers, self.application_name.clone());
        engine.set_more_drivers(Some(Arc::clone(&self.more_drivers)));
        if let Some(waker) = &self.waker {
            engine.set_waker(Arc::clone(waker));
        }
        self.engine = Some(engine.clone());
        engine
    }

    pub fn drivers(&self) -> Vec<Arc<dyn zen_db::driver::Driver>> {
        if let Some(engine) = &self.engine {
            return engine.drivers();
        }
        let mut drivers = self.drivers.clone().unwrap_or_else(zen_db::drivers);
        let more: Vec<_> = (self.more_drivers)()
            .into_iter()
            .filter(|driver| !drivers.iter().any(|own| own.id() == driver.id()))
            .collect();
        drivers.extend(more);
        drivers
    }

    pub fn waker(&self) -> Option<zen_db::Waker> {
        self.waker.clone()
    }

    pub fn started(&self) -> Option<&zen_db::engine::Engine> {
        self.engine.as_ref()
    }
}

pub fn files(session: &Session) -> Files {
    let data = session.state::<Workspaces>().and_then(Workspaces::data_dir).map(PathBuf::from);
    let project =
        session.state::<Workbench>().and_then(Workbench::project).map(|root| (root.to_owned(), project_key(root)));
    Files::new(data.as_deref(), project.as_ref().map(|(root, key)| (root.as_path(), key.as_str())))
}

pub fn secret_project(session: &Session, scope: Scope) -> Option<String> {
    (scope == Scope::Project)
        .then(|| session.state::<Workbench>().and_then(Workbench::project).map(project_key))
        .flatten()
}

type Stamp = Option<(SystemTime, u64)>;

type Kept = (Vec<Stamp>, Arc<dyn Any + Send + Sync>);

#[derive(Default)]
pub struct DiskCache(Mutex<HashMap<(TypeId, Vec<PathBuf>), Kept>>);

fn stamp(path: &Path) -> Stamp {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

pub fn cached<T, E>(session: &Session, paths: &[PathBuf], read: impl FnOnce() -> Result<T, E>) -> Result<T, E>
where
    T: Any + Clone + Send + Sync,
{
    let Some(cache) = session.state::<DiskCache>() else { return read() };
    let key = (TypeId::of::<T>(), paths.to_vec());
    let stamps: Vec<Stamp> = paths.iter().map(|path| stamp(path)).collect();
    let mut kept = cache.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((known, value)) = kept.get(&key)
        && *known == stamps
        && let Some(value) = value.downcast_ref::<T>()
    {
        return Ok(value.clone());
    }
    let value = read()?;
    kept.insert(key, (stamps, Arc::new(value.clone())));
    Ok(value)
}

pub fn entries(session: &Session) -> std::io::Result<Vec<Entry>> {
    let files = files(session);
    let paths: Vec<PathBuf> =
        [&files.global, &files.project, &files.overrides].into_iter().flatten().cloned().collect();
    cached(session, &paths, || files.entries())
}

pub fn connection_types(session: &Session) -> (ConnectionTypes, Option<PathBuf>) {
    let file = session
        .state::<Workspaces>()
        .and_then(Workspaces::data_dir)
        .map(|data| data.join("databases").join("connection-types.json"));
    let paths: Vec<PathBuf> = file.iter().cloned().collect();
    let types = cached::<_, std::convert::Infallible>(session, &paths, || {
        Ok(file.as_deref().map(ConnectionTypes::load).unwrap_or_else(|| ConnectionTypes::load(Path::new(""))))
    });
    (types.unwrap_or_else(|never| match never {}), file)
}

fn io_failed(error: &std::io::Error) -> CommandError {
    CommandError::new(t!("database-error-files", error = error.to_string()))
}

pub fn find(session: &Session, id: &str) -> Result<Entry, CommandError> {
    entries(session)
        .map_err(|error| io_failed(&error))?
        .into_iter()
        .find(|entry| entry.source.id == id)
        .ok_or_else(|| CommandError::with_code("unknownConnection", t!("database-error-unknown-connection", id = id)))
}

pub fn secret_store(session: &Session) -> Result<Arc<dyn SecretStore>, CommandError> {
    session
        .state::<Secrets>()
        .map(|secrets| Arc::clone(&secrets.0))
        .ok_or_else(|| CommandError::new(t!("database-error-secret-no-store")))
}

pub fn secret_id(session: &Session, entry: &Entry, kind: Kind) -> Result<SecretId, CommandError> {
    let project = secret_project(session, entry.scope);
    zen_secrets::database::id(project.as_deref(), &entry.source.id, kind)
        .map_err(|error| secret_error(&error, &entry.source.id))
}

pub fn has_password(entry: &Entry) -> bool {
    entry.source.save_password && entry.source.password_saved
}

pub fn secret_error(error: &SecretError, connection: &str) -> CommandError {
    CommandError::new(secret_text(error, connection))
}

pub fn secret_text(error: &SecretError, connection: &str) -> String {
    match error {
        SecretError::Denied => t!("database-error-secret-denied"),
        SecretError::Unsupported => t!("database-error-secret-unsupported"),
        SecretError::InvalidId => t!("database-error-secret-id", connection = connection),
        SecretError::Failed(reason) => t!("database-error-secret-failed", reason = reason.as_str()),
    }
}

/// A connection as the commands answer it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceAnswer {
    /// The connection in effect, with its scope and personal overrides.
    #[serde(flatten)]
    pub entry: Entry,
    /// Whether a password is stored for it (the password itself is never answered).
    pub has_password: bool,
    /// Its connection state: `disconnected`, `connecting`, `connected`, `closed` or `failed`.
    pub state: zen_db::engine::ConnectionState,
    /// What it waits for while it connects: `keychain`, `tunnel` or `network`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting_for: Option<zen_db::engine::Waiting>,
    /// The operation storing or deleting its password in the background, when this call changed it
    /// (the Keychain is never written on the window's thread); `db.operation.state` tells how it
    /// went, and a failure is also a notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_operation: Option<u64>,
}

fn answer(session: &Session, entry: Entry) -> SourceAnswer {
    answer_storing(session, entry, None)
}

fn answer_storing(session: &Session, entry: Entry, secret_operation: Option<u64>) -> SourceAnswer {
    let status = session
        .state::<Database>()
        .and_then(Database::started)
        .map(|engine| engine.status(&entry.source.id))
        .unwrap_or_else(|| zen_db::engine::ConnectionStatus::disconnected(&entry.source.id));
    SourceAnswer {
        has_password: has_password(&entry),
        state: status.state,
        waiting_for: status.waiting_for,
        secret_operation,
        entry,
    }
}

// WHY: a Keychain prompt waits for the person, so this work never runs on the window's thread.
pub fn secret_work(
    session: &mut Session,
    kind: OperationKind,
    id: &str,
    work: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> u64 {
    session.state_mut::<Database>().engine().secret_work(kind, id, work)
}

pub fn password_not_saved(session: &mut Session, id: &str) {
    let Ok(entry) = find(session, id) else { return };
    let files = files(session);
    if entry.scope == Scope::Project {
        let mut overrides = files.read_overrides();
        if let Some(personal) = overrides.connections.get_mut(id) {
            personal.password_saved = None;
            let _ = files.write_overrides(&overrides);
        }
    } else if let Ok(mut file) = files.read(entry.scope)
        && let Some(source) = file.connections.iter_mut().find(|source| source.id == id)
    {
        source.password_saved = false;
        let _ = files.write(entry.scope, &file);
    }
    session.state_mut::<Workbench>().revision += 1;
}

/// Which connections to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// Only this scope.
    pub scope: Option<Scope>,
    /// Only connections in this folder or below it.
    pub folder: Option<String>,
    /// The most connections to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// A folder of connections.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FolderAnswer {
    /// Its scope.
    pub scope: Scope,
    /// Its path, such as `Team/Reporting`.
    pub path: String,
}

/// The connections of the window.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAnswer {
    /// The connections, the project's first.
    pub connections: Vec<SourceAnswer>,
    /// The folders.
    pub folders: Vec<FolderAnswer>,
    /// How many connections matched.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct ListSources;

impl Command for ListSources {
    type Params = ListParams;
    type Output = ListAnswer;
    const ID: &'static str = "db.dataSource.list";
    const TITLE: &'static str = "List Database Connections";
    const DESCRIPTION: &'static str = "Lists the database connections of the open project (`.zen/data-sources.json`) and the global ones (the IDE's data folder), project ones first: settings in effect, scope, folder, connection type, `hasPassword` and the connection state. Never answers a secret. `scope`, `folder` and `limit` (50) narrow it.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ListAnswer, CommandError> {
        let files = files(session);
        let entries = files.entries().map_err(|error| io_failed(&error))?;
        let inside = |folder: &Option<String>| match &params.folder {
            None => true,
            Some(wanted) => {
                folder.as_deref().is_some_and(|folder| folder == wanted || folder.starts_with(&format!("{wanted}/")))
            }
        };
        let matched: Vec<Entry> = entries
            .into_iter()
            .filter(|entry| params.scope.is_none_or(|scope| scope == entry.scope) && inside(&entry.source.folder))
            .collect();
        let total = matched.len();
        let limit = params.limit.unwrap_or(LISTED);
        let connections = matched.into_iter().take(limit).map(|entry| answer(session, entry)).collect();
        let folders = files
            .folders()
            .map_err(|error| io_failed(&error))?
            .into_iter()
            .filter(|(scope, _)| params.scope.is_none_or(|wanted| wanted == *scope))
            .map(|(scope, path)| FolderAnswer { scope, path })
            .collect();
        Ok(ListAnswer { connections, folders, total, truncated: total > limit })
    }
}

/// Names one connection.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IdParams {
    /// The connection's id.
    pub id: String,
}

pub struct GetSource;

impl Command for GetSource {
    type Params = IdParams;
    type Output = SourceAnswer;
    const ID: &'static str = "db.dataSource.get";
    const TITLE: &'static str = "Get Database Connection";
    const DESCRIPTION: &'static str = "Answers one database connection by `id`: every setting in effect, its scope, the person's overrides of a project connection, `hasPassword` and its state. Never answers a secret.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<SourceAnswer, CommandError> {
        let entry = find(session, &params.id)?;
        Ok(answer(session, entry))
    }
}

/// The settings of a connection to add or change; absent fields are left as they are.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SourceFields {
    /// Its name.
    pub name: Option<String>,
    /// Its folder, such as `Team/Reporting`; an empty text takes it out of its folder.
    pub folder: Option<String>,
    /// The driver's id (`postgresql`).
    pub driver: Option<String>,
    /// The database profile (`cockroachdb`, from db.driver.list `profiles`), which also sets the driver; on a new connection it fills the port, database, TLS mode and settings not given (a saved one keeps its own); an empty text takes it off.
    pub profile: Option<String>,
    /// A URI (`postgresql://user:password@host:5432/db?sslmode=require`) split into the fields; its password is stored as a secret, never in the file.
    pub url: Option<String>,
    /// The host.
    pub host: Option<String>,
    /// The port.
    pub port: Option<u16>,
    /// The database.
    pub database: Option<String>,
    /// The user name; for a project connection kept as the person's own unless `shared`.
    pub user: Option<String>,
    /// `password` or `none`.
    pub auth: Option<AuthKind>,
    /// The connection type's id.
    #[serde(rename = "type")]
    pub connection_type: Option<String>,
    /// Read-only.
    pub read_only: Option<bool>,
    /// Whether the password is stored in the secret store; otherwise it is passed at each connect.
    pub save_password: Option<bool>,
    /// A password to store as the connection's secret (only with `savePassword`); never written to a file or answered.
    pub password: Option<String>,
    /// TLS settings.
    pub tls: Option<TlsSettings>,
    /// An SSH tunnel; `null` keeps it, an empty host removes it.
    pub ssh: Option<SshSettings>,
    /// How the navigator shows it.
    pub navigator: Option<NavigatorOptions>,
    /// Filters of the navigator's objects.
    pub filters: Option<Vec<ObjectFilter>>,
    /// What runs after connecting.
    pub bootstrap: Option<Bootstrap>,
    /// Settings that override the connection type's.
    pub overrides: Option<TypeOverrides>,
    /// The driver's own settings, merged key by key (`{"pooler": true}`).
    pub settings: Option<Map<String, Json>>,
    /// For a project connection: write the user name, port, read-only, type and save-password to the shared file instead of the person's overrides.
    pub shared: bool,
}

/// A connection to add.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct AddParams {
    /// Where to keep it: `global` (default) or `project`.
    pub scope: Scope,
    /// Its settings.
    #[serde(flatten)]
    pub fields: SourceFields,
}

/// A connection to change.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateParams {
    /// The connection's id.
    pub id: String,
    /// The settings to change.
    #[serde(flatten)]
    pub fields: SourceFields,
}

fn refused(message: String) -> CommandError {
    CommandError::with_code("invalidConnection", message)
}

pub fn driver_for_scheme(drivers: &[Arc<dyn zen_db::driver::Driver>], scheme: &str) -> Option<String> {
    let base = scheme.strip_suffix("+srv").unwrap_or(scheme);
    let base = if base == "rediss" { "redis" } else { base };
    drivers
        .iter()
        .find(|driver| driver.capabilities().uri_schemes.iter().any(|known| known == scheme || known == base))
        .map(|driver| driver.id().to_owned())
        .or_else(|| {
            zen_db::profile::for_scheme(scheme)
                .map(|(profile, _)| profile.driver)
                .filter(|driver| drivers.iter().any(|known| known.id() == *driver))
                .map(str::to_owned)
        })
}

fn unknown_profile(id: &str) -> CommandError {
    CommandError::with_code("unknownProfile", t!("database-error-unknown-profile", profile = id))
}

fn take_profile(
    source: &mut DataSource,
    personal: &PersonalOverride,
    fields: &SourceFields,
    profile: &'static zen_db::profile::Profile,
    tls_given: bool,
) {
    let fresh = source.id.is_empty() && source.profile.as_deref() != Some(profile.id);
    source.profile = Some(profile.id.to_owned());
    if fields.driver.is_none() || zen_db::profile::family(&source.driver) != zen_db::profile::family(profile.driver) {
        source.driver = profile.driver.to_owned();
    }
    if !fresh {
        return;
    }
    if source.port.is_none() && personal.port.is_none() && fields.port.is_none() {
        source.port = profile.port;
    }
    if source.database.is_none() && fields.database.is_none() {
        source.database = profile.database.map(str::to_owned);
    }
    if !tls_given {
        source.tls.mode = profile.tls;
    }
    for setting in profile.settings {
        let given = fields.settings.as_ref().is_some_and(|settings| settings.contains_key(*setting));
        if !given {
            source.settings.entry((*setting).to_owned()).or_insert(Json::Bool(true));
        }
    }
}

pub fn apply_fields(
    source: &mut DataSource,
    personal: &mut PersonalOverride,
    fields: &SourceFields,
    project: bool,
    drivers: &[Arc<dyn zen_db::driver::Driver>],
) -> Result<Option<String>, CommandError> {
    let mut password = fields.password.clone();
    let mut tls_given = fields.tls.is_some();
    let mut url_profile = None;
    if let Some(url) = fields.url.as_deref().map(str::trim).filter(|url| !url.is_empty()) {
        let parsed = parse_uri(url).ok_or_else(|| refused(t!("database-error-bad-url")))?;
        source.url = Some(parsed.without_password.clone());
        if source.driver.is_empty() && fields.driver.is_none() {
            source.driver = driver_for_scheme(drivers, &parsed.scheme).unwrap_or_else(|| parsed.scheme.clone());
        }
        let driver = fields.driver.as_deref().unwrap_or(&source.driver);
        url_profile = zen_db::profile::for_url(&parsed.scheme, Some(driver), parsed.host.as_deref());
        if parsed.scheme == "rediss" || zen_db::profile::for_scheme(&parsed.scheme).is_some_and(|(_, tls)| tls) {
            source.tls.mode = zen_db::registry::TlsMode::Require;
            tls_given = true;
        }
        tls_given |= parsed.params.iter().any(|(key, _)| zen_db::registry::TLS_MODE_KEYS.contains(&key.as_str()));
        source.host = parsed.host.or(source.host.take());
        source.port = parsed.port.or(source.port);
        source.database = parsed.database.or(source.database.take());
        if parsed.user.is_some() {
            source.user = parsed.user;
        }
        for (key, value) in parsed.params {
            if zen_db::registry::TLS_MODE_KEYS.contains(&key.as_str()) {
                let mode =
                    zen_db::registry::tls_mode_named(&value).ok_or_else(|| refused(t!("database-error-bad-url")))?;
                source.tls.mode = serde_json::from_value(Json::String(mode.to_owned()))
                    .map_err(|_| refused(t!("database-error-bad-url")))?;
            } else if key == "sslnegotiation" {
                source.tls.direct = value == "direct";
            } else if key == "application_name" {
                continue;
            } else {
                source.settings.insert(key, Json::String(value));
            }
        }
        password = parsed.password.or(password);
    }
    if let Some(name) = &fields.name {
        source.name = name.trim().to_owned();
    }
    if let Some(folder) = &fields.folder {
        let folder = folder.trim().trim_matches('/');
        source.folder = (!folder.is_empty()).then(|| folder.to_owned());
    }
    if let Some(driver) = &fields.driver {
        source.driver = driver.clone();
    }
    match fields.profile.as_deref().map(str::trim) {
        Some("") => source.profile = None,
        Some(id) => {
            let profile = zen_db::profile::get(id).ok_or_else(|| unknown_profile(id))?;
            take_profile(source, personal, fields, profile, tls_given);
        }
        None => {
            if let Some(profile) = url_profile.filter(|_| source.profile.is_none()) {
                take_profile(source, personal, fields, profile, tls_given);
            }
        }
    }
    if fields.host.is_some() {
        source.host = fields.host.clone().filter(|host| !host.is_empty());
    }
    if fields.database.is_some() {
        source.database = fields.database.clone().filter(|database| !database.is_empty());
    }
    if let Some(auth) = fields.auth {
        source.auth = auth;
    }
    if let Some(tls) = &fields.tls {
        source.tls = tls.clone();
    }
    if let Some(ssh) = &fields.ssh {
        source.ssh = (!ssh.host.is_empty()).then(|| ssh.clone());
    }
    if let Some(navigator) = &fields.navigator {
        source.navigator = navigator.clone();
    }
    if let Some(filters) = &fields.filters {
        source.filters = filters.clone();
    }
    if let Some(bootstrap) = &fields.bootstrap {
        source.bootstrap = bootstrap.clone();
    }
    if let Some(overrides) = &fields.overrides {
        source.overrides = overrides.clone();
    }
    if let Some(settings) = &fields.settings {
        for (key, value) in settings {
            if value.is_null() {
                source.settings.remove(key);
            } else {
                source.settings.insert(key.clone(), value.clone());
            }
        }
    }
    let personal_fields = project && !fields.shared;
    let user = fields.user.clone().filter(|user| !user.is_empty());
    if personal_fields {
        if fields.user.is_some() {
            personal.user = user;
            if fields.url.is_some() {
                personal.user = source.user.take().or(personal.user.take());
            }
        } else if fields.url.is_some() {
            personal.user = source.user.take().or(personal.user.take());
        }
        if let Some(port) = fields.port {
            personal.port = Some(port);
        }
        if let Some(read_only) = fields.read_only {
            personal.read_only = Some(read_only);
        }
        if let Some(kind) = &fields.connection_type {
            personal.connection_type = Some(kind.clone());
        }
        if let Some(save) = fields.save_password {
            personal.save_password = Some(save);
        }
    } else {
        if fields.user.is_some() {
            source.user = user;
        }
        if let Some(port) = fields.port {
            source.port = Some(port);
        }
        if let Some(read_only) = fields.read_only {
            source.read_only = read_only;
        }
        if let Some(kind) = &fields.connection_type {
            source.connection_type = kind.clone();
        }
        if let Some(save) = fields.save_password {
            source.save_password = save;
        }
    }
    if source.name.is_empty() {
        return Err(refused(t!("database-error-no-name")));
    }
    source.forget_secrets();
    Ok(password.filter(|password| !password.is_empty()))
}

pub fn check_driver(session: &mut Session, driver: &str) -> Result<(), CommandError> {
    let known = session.state_mut::<Database>().drivers().iter().any(|known| known.id() == driver);
    if known {
        Ok(())
    } else {
        Err(CommandError::with_code("unknownDriver", t!("database-error-unknown-driver", driver = driver)))
    }
}

fn keep_password(
    session: &mut Session,
    scope: Scope,
    source: &mut DataSource,
    personal: &mut PersonalOverride,
    password: Option<String>,
) -> Result<Option<u64>, CommandError> {
    let entry = Entry { scope, source: personal.apply(source.clone()), personal: None };
    let saved = |personal: &mut PersonalOverride, source: &mut DataSource, saved: bool| {
        if scope == Scope::Project {
            personal.password_saved = Some(saved).filter(|saved| *saved);
            source.password_saved = false;
        } else {
            source.password_saved = saved;
        }
    };
    let connection = entry.source.id.clone();
    if !entry.source.save_password || entry.source.auth == AuthKind::None {
        if entry.source.password_saved {
            let store = secret_store(session)?;
            let id = secret_id(session, &entry, Kind::Password)?;
            saved(personal, source, false);
            let name = connection.clone();
            let work = move || store.delete(&id).map(drop).map_err(|error| secret_text(&error, &name));
            return Ok(Some(secret_work(session, OperationKind::DeleteSecrets, &connection, work)));
        }
        return Ok(None);
    }
    let Some(password) = password else { return Ok(None) };
    let store = secret_store(session)?;
    let id = secret_id(session, &entry, Kind::Password)?;
    saved(personal, source, true);
    let secret = Secret::new(password);
    let name = connection.clone();
    let work = move || store.set(&id, &secret).map_err(|error| secret_text(&error, &name));
    Ok(Some(secret_work(session, OperationKind::StoreSecret, &connection, work)))
}

fn save(
    session: &Session,
    scope: Scope,
    source: DataSource,
    personal: PersonalOverride,
) -> Result<Entry, CommandError> {
    let files = files(session);
    if files.of(scope).is_none() {
        let message = if scope == Scope::Project {
            t!("workbench-error-no-project")
        } else {
            t!("database-error-no-data-folder")
        };
        return Err(CommandError::new(message));
    }
    let mut file = files.read(scope).map_err(|error| io_failed(&error))?;
    match file.connections.iter_mut().find(|known| known.id == source.id) {
        Some(known) => *known = source.clone(),
        None => file.connections.push(source.clone()),
    }
    files.write(scope, &file).map_err(|error| io_failed(&error))?;
    if scope == Scope::Project {
        let mut overrides = files.read_overrides();
        if personal.is_empty() {
            overrides.connections.remove(&source.id);
        } else {
            overrides.connections.insert(source.id.clone(), personal.clone());
        }
        files.write_overrides(&overrides).map_err(|error| io_failed(&error))?;
    }
    let personal = (scope == Scope::Project && !personal.is_empty()).then_some(personal);
    let source = personal.as_ref().map_or_else(|| source.clone(), |personal| personal.apply(source.clone()));
    Ok(Entry { scope, source, personal })
}

pub struct AddSource;

impl Command for AddSource {
    type Params = AddParams;
    type Output = SourceAnswer;
    const ID: &'static str = "db.dataSource.add";
    const TITLE: &'static str = "Add Database Connection";
    const DESCRIPTION: &'static str = "Adds a database connection, `global` (the IDE's data folder, every project) or `project` (`.zen/data-sources.json`, shared through Git, never with a secret; the user name, port, read-only, type and save-password go to the person's overrides unless `shared`). Takes the fields or a `url` (its password is stored as a secret and removed). New connections are Development. Without `auth`, one given no password gets `none` when its server commonly needs none (Redis, Valkey, SQLite, ClickHouse's `default` user), else `password` (PostgreSQL, MySQL and MariaDB ask for it when connecting). `password` is stored in the secret store when `savePassword` (default on). An agent may not name the program its driver runs (`settings.program`, `settings.javaHome`: code `agentNotAllowed`). Connects nowhere. Answers the connection without secrets.";

    fn run(&self, session: &mut Session, params: AddParams) -> Result<SourceAnswer, CommandError> {
        add(session, params.scope, &[&params.fields], None)
    }
}

pub fn add(
    session: &mut Session,
    scope: Scope,
    layers: &[&SourceFields],
    origin: Option<zen_db::registry::Origin>,
) -> Result<SourceAnswer, CommandError> {
    agent_names_program(session, None, layers)?;
    let project = scope == Scope::Project;
    let mut source = DataSource { driver: String::new(), save_password: true, ..DataSource::default() };
    source.connection_type = zen_db::types::DEVELOPMENT.to_owned();
    let mut personal = PersonalOverride::default();
    let drivers = session.state_mut::<Database>().drivers();
    let mut password = None;
    for fields in layers {
        password = apply_fields(&mut source, &mut personal, fields, project, &drivers)?.or(password);
    }
    if source.driver.is_empty() {
        source.driver = zen_db::postgres::ID.to_owned();
    }
    check_driver(session, &source.driver)?;
    if password.is_none() && layers.iter().all(|fields| fields.auth.is_none()) {
        let user = personal.user.as_deref().or(source.user.as_deref());
        let no_password = drivers
            .iter()
            .find(|driver| driver.id() == source.driver)
            .is_some_and(|driver| !driver.capabilities().password);
        if no_password || zen_db::registry::passwordless_by_default(&source.driver, user) {
            source.auth = AuthKind::None;
        }
    }
    source.id = new_id(&source.name);
    source.origin = origin;
    let storing = keep_password(session, scope, &mut source, &mut personal, password)?;
    let entry = save(session, scope, source, personal)?;
    session.state_mut::<Workbench>().revision += 1;
    Ok(answer_storing(session, entry, storing))
}

fn agent_names_program(
    session: &Session,
    current: Option<&Map<String, Json>>,
    layers: &[&SourceFields],
) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    for key in super::connections::PROGRAM_SETTINGS {
        let given = layers.iter().filter_map(|fields| fields.settings.as_ref()?.get(key)).next_back();
        let kept = current.and_then(|settings| settings.get(key));
        if let Some(given) = given
            && Some(given) != kept
            && !(given.is_null() || given.as_str().is_some_and(|text| text.trim().is_empty()))
        {
            return Err(CommandError::with_code(
                super::safety::AGENT_NOT_ALLOWED,
                t!("database-error-agent-program", setting = key),
            ));
        }
    }
    Ok(())
}

pub struct UpdateSource;

impl Command for UpdateSource {
    type Params = UpdateParams;
    type Output = SourceAnswer;
    const ID: &'static str = "db.dataSource.update";
    const TITLE: &'static str = "Update Database Connection";
    const DESCRIPTION: &'static str = "Changes the settings of database connection `id`; absent fields stay. An agent may not give `settings.program` or `settings.javaHome` a new value, nor make it less safe (read-only off, a looser type or overrides: code `agentNotAllowed`). For a project connection the user name, port, read-only, type and save-password are the person's own overrides unless `shared`. A `password` is stored as a secret (with `savePassword`). A connected connection takes read-only, its type and the type's overrides at once (each open session gets the commit mode and read-only setting they make; a session with uncommitted changes keeps manual commit until they end), the other settings at its next connect. Answers it without secrets.";

    fn run(&self, session: &mut Session, params: UpdateParams) -> Result<SourceAnswer, CommandError> {
        let current = find(session, &params.id)?;
        super::safety::agent_loosens_source(session, &current, &params.fields)?;
        let answer = update(session, &params.id, &[&params.fields], None)?;
        if params.fields.read_only.is_some()
            && let Some(engine) = session.state::<Database>().and_then(Database::started)
        {
            let entry = find(session, &params.id)?;
            engine.set_read_only(&params.id, entry.source.read_only);
        }
        let changed = find(session, &params.id).is_ok_and(|entry| {
            entry.source.connection_type != current.source.connection_type
                || entry.source.overrides != current.source.overrides
        });
        if changed {
            reapply_type(session, &params.id);
        }
        Ok(answer)
    }
}

pub fn reapply_type(session: &mut Session, id: &str) {
    let Ok(entry) = find(session, id) else { return };
    let (types, _) = connection_types(session);
    let kind = types.resolve(&entry.source.connection_type);
    let Some(engine) = session.state::<Database>().and_then(Database::started) else { return };
    let (_, kept) = engine.set_kind(id, kind, entry.source.overrides.clone());
    if kept.is_empty() {
        return;
    }
    let body = t!("database-type-kept-manual", name = entry.source.name.as_str(), count = kept.len());
    crate::notifications::post(
        session,
        crate::notifications::NewNotification::message(
            crate::notifications::Group::General,
            crate::notifications::Kind::Warning,
            body,
        ),
    );
}

pub fn update(
    session: &mut Session,
    id: &str,
    layers: &[&SourceFields],
    origin: Option<zen_db::registry::Origin>,
) -> Result<SourceAnswer, CommandError> {
    let current = find(session, id)?;
    agent_names_program(session, Some(&current.source.settings), layers)?;
    let files = files(session);
    let stored = files
        .read(current.scope)
        .map_err(|error| io_failed(&error))?
        .connections
        .into_iter()
        .find(|source| source.id == id)
        .ok_or_else(|| CommandError::new(t!("database-error-unknown-connection", id = id)))?;
    let mut source = stored;
    let mut personal = current.personal.clone().unwrap_or_default();
    let drivers = session.state_mut::<Database>().drivers();
    let mut password = None;
    for fields in layers {
        password =
            apply_fields(&mut source, &mut personal, fields, current.scope == Scope::Project, &drivers)?.or(password);
    }
    check_driver(session, &source.driver)?;
    if origin.is_some() {
        source.origin = origin;
    }
    let storing = keep_password(session, current.scope, &mut source, &mut personal, password)?;
    let entry = save(session, current.scope, source, personal)?;
    session.state_mut::<Workbench>().revision += 1;
    Ok(answer_storing(session, entry, storing))
}

/// What `db.dataSource.remove` removed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Removed {
    /// The connection's id.
    pub id: String,
    /// Its scope.
    pub scope: Scope,
    /// The operation deleting its secrets in the background; `db.operation.state` tells how it
    /// went, and a failure is also a notification.
    pub secret_operation: u64,
    /// How many of its tabs closed: object, plan, sessions and key tabs and its consoles' result tabs.
    pub closed_tabs: usize,
    /// The open consoles that stay without a connection.
    pub unbound_consoles: Vec<PathBuf>,
}

pub struct RemoveSource;

impl Command for RemoveSource {
    type Params = IdParams;
    type Output = Removed;
    const ID: &'static str = "db.dataSource.remove";
    const TITLE: &'static str = "Remove Database Connection";
    const DESCRIPTION: &'static str = "Removes database connection `id` from its registry file, with the person's overrides and its stored secrets (deleted in the background: `secretOperation`, read with db.operation.state); a connected one is disconnected first (open transactions rolled back). Its object, plan, sessions and key tabs and its consoles' result tabs close (`closedTabs`); its open consoles stay, without a connection, saying it was removed (`unboundConsoles`). Unsaved data or structure changes in a tab that would close refuse it first (codes `pendingChanges`, `pendingStructureChanges`); db.dataSource.removal tells beforehand what closes. Touches no other connection and no database.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<Removed, CommandError> {
        let entry = find(session, &params.id)?;
        let removal = super::removal::of(session, &params.id);
        super::removal::refuse_unsaved(session, &removal, serde_json::json!({ "id": params.id }))?;
        let files = files(session);
        let mut file = files.read(entry.scope).map_err(|error| io_failed(&error))?;
        file.connections.retain(|source| source.id != params.id);
        files.write(entry.scope, &file).map_err(|error| io_failed(&error))?;
        if entry.scope == Scope::Project {
            let mut overrides = files.read_overrides();
            if overrides.connections.remove(&params.id).is_some() {
                files.write_overrides(&overrides).map_err(|error| io_failed(&error))?;
            }
        }
        if let Some(engine) = session.state::<Database>().and_then(Database::started) {
            engine.forget(&params.id);
        }
        let secret_operation = forget_secrets(session, entry.scope, &params.id)?;
        super::removal::apply(session, &removal, &entry.source.name);
        session.state_mut::<Workbench>().revision += 1;
        Ok(Removed {
            id: params.id,
            scope: entry.scope,
            secret_operation,
            closed_tabs: removal.closing(),
            unbound_consoles: removal.consoles().to_vec(),
        })
    }
}

pub struct DuplicateSource;

impl Command for DuplicateSource {
    type Params = IdParams;
    type Output = SourceAnswer;
    const ID: &'static str = "db.dataSource.duplicate";
    const TITLE: &'static str = "Duplicate Database Connection";
    const DESCRIPTION: &'static str = "Copies database connection `id` into a new connection of the same scope and folder named `<name> (copy)` (a number added while that name is taken): every setting, the person's overrides of a project connection, and its saved secrets, copied in the background (`secretOperation`, read with db.operation.state). Connects nowhere. Answers the new connection without secrets.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<SourceAnswer, CommandError> {
        let entry = find(session, &params.id)?;
        let files = files(session);
        let mut source = files
            .read(entry.scope)
            .map_err(|error| io_failed(&error))?
            .connections
            .into_iter()
            .find(|source| source.id == params.id)
            .ok_or_else(|| CommandError::new(t!("database-error-unknown-connection", id = params.id.as_str())))?;
        let taken: Vec<String> =
            files.entries().unwrap_or_default().into_iter().map(|known| known.source.name).collect();
        let base = t!("database-connection-copy-name", name = entry.source.name.as_str());
        let name = (1..)
            .map(|number| if number == 1 { base.clone() } else { format!("{base} {number}") })
            .find(|name| !taken.contains(name))
            .unwrap_or(base);
        source.id = new_id(&name);
        source.name = name;
        source.origin = None;
        let personal = entry.personal.clone().unwrap_or_default();
        let copied = save(session, entry.scope, source, personal)?;
        let project = secret_project(session, entry.scope);
        let store = secret_store(session)?;
        let (from, to) = (params.id.clone(), copied.source.id.clone());
        let work = move || {
            for kind in Kind::ALL {
                let text = |error: SecretError| secret_text(&error, &from);
                let source = zen_secrets::database::id(project.as_deref(), &from, kind).map_err(text)?;
                let target = zen_secrets::database::id(project.as_deref(), &to, kind).map_err(text)?;
                if let Some(secret) = store.get(&source).map_err(text)? {
                    store.set(&target, &secret).map_err(text)?;
                }
            }
            Ok(())
        };
        let operation = secret_work(session, OperationKind::StoreSecret, &copied.source.id, work);
        session.state_mut::<Workbench>().revision += 1;
        Ok(answer_storing(session, copied, Some(operation)))
    }
}

pub fn forget_secrets(session: &mut Session, scope: Scope, id: &str) -> Result<u64, CommandError> {
    let project = secret_project(session, scope);
    let store = secret_store(session)?;
    let connection = id.to_owned();
    let work = move || {
        zen_secrets::database::delete_all(store.as_ref(), project.as_deref(), &connection)
            .map(drop)
            .map_err(|error| secret_text(&error, &connection))
    };
    Ok(secret_work(session, OperationKind::DeleteSecrets, id, work))
}

/// What `db.connection.setSecret` stores.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetSecretParams {
    /// The connection's id.
    pub connection: String,
    /// Which of its secrets: `password`, `ssh` or `token`.
    pub kind: Kind,
    /// The secret; stored in the Keychain only, never echoed or logged.
    #[schemars(with = "String")]
    pub value: Secret,
    /// The connection's scope, checked against the connection's own when given.
    #[serde(default)]
    pub scope: Option<Scope>,
}

/// A secret stored, without its value.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretStored {
    /// The connection's id.
    pub connection: String,
    /// Which of its secrets.
    pub kind: Kind,
    /// The connection's scope.
    pub scope: Scope,
    /// The operation storing it in the background (the Keychain is never written on the window's
    /// thread); `db.operation.state` tells how it went, and a failure is also a notification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
}

pub struct SetSecret;

impl Command for SetSecret {
    type Params = SetSecretParams;
    type Output = SecretStored;
    const ID: &'static str = "db.connection.setSecret";
    const TITLE: &'static str = "Set Connection Secret";
    const DESCRIPTION: &'static str = "Stores a secret (`kind`: `password`, `ssh` or `token`) of the existing connection `connection` in the macOS Keychain as the item `<connection>/<kind>` (`<project key>/<connection>/<kind>` for a project connection) of the service `space.ifzenelse.zen.database`, replacing the one there, in the background (`operation`: db.operation.state tells how it went; a failure is also a notification); a password also turns on the connection's Save password. Never in a file; the answer never echoes the value, and no command reads it back. Connects nowhere. A denied Keychain access is reported in the operation, not retried.";

    fn run(&self, session: &mut Session, params: SetSecretParams) -> Result<SecretStored, CommandError> {
        if params.value.is_empty() {
            return Err(CommandError::new(t!("database-error-secret-empty")));
        }
        let connection = params.connection.trim();
        if zen_secrets::database::id(None, connection, params.kind).is_err() {
            return Err(secret_error(&SecretError::InvalidId, connection));
        }
        let entry = find(session, connection)?;
        if params.scope.is_some_and(|scope| scope != entry.scope) {
            return Err(CommandError::with_code(
                "unknownConnection",
                t!("database-error-unknown-connection", id = connection),
            ));
        }
        let operation = if params.kind == Kind::Password {
            update_password(session, &entry, params.value.expose().to_owned())?
        } else {
            let store = secret_store(session)?;
            let id = secret_id(session, &entry, params.kind)?;
            let (value, name) = (params.value, connection.to_owned());
            let work = move || store.set(&id, &value).map_err(|error| secret_text(&error, &name));
            Some(secret_work(session, OperationKind::StoreSecret, connection, work))
        };
        Ok(SecretStored { connection: connection.to_owned(), kind: params.kind, scope: entry.scope, operation })
    }
}

fn update_password(session: &mut Session, entry: &Entry, password: String) -> Result<Option<u64>, CommandError> {
    let files = files(session);
    let mut source = files
        .read(entry.scope)
        .map_err(|error| io_failed(&error))?
        .connections
        .into_iter()
        .find(|source| source.id == entry.source.id)
        .ok_or_else(|| CommandError::new(t!("database-error-unknown-connection", id = entry.source.id.as_str())))?;
    let mut personal = entry.personal.clone().unwrap_or_default();
    if entry.scope == Scope::Project {
        personal.save_password = Some(true);
    } else {
        source.save_password = true;
    }
    source.auth = AuthKind::Password;
    let storing = keep_password(session, entry.scope, &mut source, &mut personal, Some(password))?;
    save(session, entry.scope, source, personal)?;
    session.state_mut::<Workbench>().revision += 1;
    Ok(storing)
}

/// The connection types.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TypesAnswer {
    /// Every type, the built-in ones first.
    pub types: Vec<ConnectionType>,
    /// The theme colours a type can take.
    pub colors: Vec<String>,
}

pub struct ListTypes;

impl Command for ListTypes {
    type Params = zen_core::NoParams;
    type Output = TypesAnswer;
    const ID: &'static str = "db.connectionType.list";
    const TITLE: &'static str = "List Connection Types";
    const DESCRIPTION: &'static str = "Lists the connection types: Development, Test and Production with their built-in defaults (auto-commit, confirmations, idle transaction end and idle close in seconds, statement timeout) and the person's own, and the theme colours a type can take.";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<TypesAnswer, CommandError> {
        let (types, _) = connection_types(session);
        Ok(TypesAnswer {
            types: types.all().to_vec(),
            colors: COLORS.iter().map(|color| (*color).to_owned()).collect(),
        })
    }
}

/// The settings of a connection type; absent fields keep their value (or the Development default for a new type).
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TypeFields {
    /// Its name.
    pub name: Option<String>,
    /// One of the theme's colours.
    pub color: Option<String>,
    /// Auto-commit.
    pub auto_commit: Option<bool>,
    /// Confirm data-changing statements.
    pub confirm_execute: Option<bool>,
    /// Confirm saving edited data.
    pub confirm_data_change: Option<bool>,
    /// Smart commit.
    pub smart_commit: Option<bool>,
    /// Return to auto-commit after a commit or rollback.
    pub auto_close_transactions: Option<bool>,
    /// Seconds after which an idle transaction ends; 0 never.
    pub end_idle_transactions_after: Option<u64>,
    /// Seconds after which an idle connection closes; 0 never.
    pub close_idle_after: Option<u64>,
    /// The statement timeout in seconds; 0 none.
    pub statement_timeout: Option<u64>,
    /// Read-only: every connection of the type refuses changes.
    pub read_only: Option<bool>,
    /// What agents may run: `full`, `readOnly` or `none`.
    pub agents: Option<zen_db::types::AgentAccess>,
}

/// A connection type to create.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateTypeParams {
    /// Its id; made from the name when absent.
    pub id: Option<String>,
    /// Its settings.
    #[serde(flatten)]
    pub fields: TypeFields,
}

/// A connection type to change.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateTypeParams {
    /// Its id.
    pub id: String,
    /// The settings to change.
    #[serde(flatten)]
    pub fields: TypeFields,
}

fn seconds(value: Option<u64>, current: Option<u64>) -> Option<u64> {
    match value {
        Some(0) => None,
        Some(value) => Some(value),
        None => current,
    }
}

fn apply_type(kind: &mut ConnectionType, fields: &TypeFields) -> Result<(), CommandError> {
    if let Some(name) = &fields.name {
        kind.name = name.trim().to_owned();
    }
    if let Some(color) = &fields.color {
        if !COLORS.contains(&color.as_str()) {
            return Err(CommandError::new(t!(
                "database-error-color",
                color = color.as_str(),
                colors = COLORS.join(", ")
            )));
        }
        kind.color = color.clone();
    }
    let flags = [
        (fields.auto_commit, &mut kind.auto_commit),
        (fields.confirm_execute, &mut kind.confirm_execute),
        (fields.confirm_data_change, &mut kind.confirm_data_change),
        (fields.smart_commit, &mut kind.smart_commit),
        (fields.auto_close_transactions, &mut kind.auto_close_transactions),
        (fields.read_only, &mut kind.read_only),
    ];
    for (value, field) in flags {
        if let Some(value) = value {
            *field = value;
        }
    }
    kind.end_idle_transactions_after = seconds(fields.end_idle_transactions_after, kind.end_idle_transactions_after);
    kind.close_idle_after = seconds(fields.close_idle_after, kind.close_idle_after);
    kind.statement_timeout = seconds(fields.statement_timeout, kind.statement_timeout);
    if let Some(agents) = fields.agents {
        kind.agents = Some(agents);
    }
    if kind.name.is_empty() {
        return Err(CommandError::new(t!("database-error-no-name")));
    }
    Ok(())
}

fn save_types(types: &ConnectionTypes, file: Option<PathBuf>) -> Result<(), CommandError> {
    let file = file.ok_or_else(|| CommandError::new(t!("database-error-no-data-folder")))?;
    types.save(&file).map_err(|error| io_failed(&error))
}

pub struct CreateType;

impl Command for CreateType {
    type Params = CreateTypeParams;
    type Output = ConnectionType;
    const ID: &'static str = "db.connectionType.create";
    const TITLE: &'static str = "Create Connection Type";
    const DESCRIPTION: &'static str = "Creates a connection type, starting from Development's settings, kept in the IDE's data folder (`databases/connection-types.json`). `color` is one of the theme's colours (`db.connectionType.list` names them). Answers the type.";

    fn run(&self, session: &mut Session, params: CreateTypeParams) -> Result<ConnectionType, CommandError> {
        let (mut types, file) = connection_types(session);
        let mut kind = types.resolve(zen_db::types::DEVELOPMENT);
        kind.color = "blue".to_owned();
        apply_type(&mut kind, &params.fields)?;
        kind.id = params.id.filter(|id| !id.trim().is_empty()).unwrap_or_else(|| new_id(&kind.name));
        if types.get(&kind.id).is_some() {
            return Err(CommandError::new(t!("database-error-type-exists", id = kind.id.as_str())));
        }
        kind.built_in = false;
        types.put(kind.clone());
        save_types(&types, file)?;
        Ok(kind)
    }
}

pub struct UpdateType;

impl Command for UpdateType {
    type Params = UpdateTypeParams;
    type Output = ConnectionType;
    const ID: &'static str = "db.connectionType.update";
    const TITLE: &'static str = "Update Connection Type";
    const DESCRIPTION: &'static str = "Changes connection type `id`, a built-in one too; absent fields stay, 0 seconds means never. `readOnly` makes every connection of the type read-only; `agents` (`full`, `readOnly`, `none`) is what agents may run on them. Connected connections of the type take it at once, as db.dataSource.update does with a type. An agent may make a type safer, never looser (code `agentNotAllowed`). Answers the type.";

    fn run(&self, session: &mut Session, params: UpdateTypeParams) -> Result<ConnectionType, CommandError> {
        let (mut types, file) = connection_types(session);
        let mut kind = types
            .get(&params.id)
            .cloned()
            .ok_or_else(|| CommandError::new(t!("database-error-unknown-type", id = params.id.as_str())))?;
        let before = kind.clone();
        apply_type(&mut kind, &params.fields)?;
        super::safety::agent_loosens_type(session, &before, &kind)?;
        types.put(kind.clone());
        save_types(&types, file)?;
        if before != kind {
            let users: Vec<String> = files(session)
                .entries()
                .map_err(|error| io_failed(&error))?
                .into_iter()
                .filter(|entry| entry.source.connection_type == params.id)
                .map(|entry| entry.source.id)
                .collect();
            for id in users {
                reapply_type(session, &id);
            }
        }
        Ok(types.get(&params.id).cloned().unwrap_or(kind))
    }
}

pub struct DeleteType;

impl Command for DeleteType {
    type Params = IdParams;
    type Output = TypesAnswer;
    const ID: &'static str = "db.connectionType.delete";
    const TITLE: &'static str = "Delete Connection Type";
    const DESCRIPTION: &'static str = "Deletes the person's connection type `id`. The built-in types are never deleted, and a type that connections use is refused with their names. Answers the types left.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<TypesAnswer, CommandError> {
        let (mut types, file) = connection_types(session);
        let kind = types
            .get(&params.id)
            .cloned()
            .ok_or_else(|| CommandError::new(t!("database-error-unknown-type", id = params.id.as_str())))?;
        if kind.built_in {
            return Err(CommandError::new(t!("database-error-type-built-in", name = kind.name.as_str())));
        }
        let users: Vec<String> = files(session)
            .entries()
            .map_err(|error| io_failed(&error))?
            .into_iter()
            .filter(|entry| entry.source.connection_type == params.id)
            .map(|entry| entry.source.name)
            .collect();
        if !users.is_empty() {
            return Err(CommandError::new(t!(
                "database-error-type-used",
                name = kind.name.as_str(),
                connections = users.join(", ")
            )));
        }
        types.remove(&params.id);
        save_types(&types, file)?;
        Ok(TypesAnswer {
            types: types.all().to_vec(),
            colors: COLORS.iter().map(|color| (*color).to_owned()).collect(),
        })
    }
}

/// A driver as the connection dialog and agents see it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriverAnswer {
    /// Its id (`postgresql`).
    pub id: String,
    /// Its name as shown.
    pub name: String,
    /// The family the connection dialog lists it in (`mysql` for MariaDB).
    pub family: String,
    /// Its default port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_port: Option<u16>,
    /// What it can do and the settings its connections have.
    pub capabilities: zen_db::model::Capabilities,
    /// For a driver a plug-in's program serves: the plug-in, whether the program is installed, how it runs (ADR-0174); absent for one compiled in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub external: Option<zen_db::external::ExternalInfo>,
}

/// Which drivers and profiles to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DriverListParams {
    /// Only those whose id or name holds this text, without case.
    pub query: Option<String>,
    /// Only one family (`postgresql`, `mysql`, `redis`).
    pub family: Option<String>,
    /// The most profiles answered; all by default.
    pub limit: Option<usize>,
    /// Whether each profile carries its capabilities; true by default.
    pub capabilities: Option<bool>,
}

/// The drivers and the database profiles over them.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriversAnswer {
    /// Every driver asked for, in the order the connection dialog lists them.
    pub drivers: Vec<DriverAnswer>,
    /// The database profiles asked for (ADR-0173), by family, over drivers the IDE has.
    pub profiles: Vec<zen_db::profile::ProfileAnswer>,
    /// How many profiles matched.
    pub total_profiles: usize,
    /// Whether `limit` left profiles out.
    pub truncated: bool,
}

pub struct ListDrivers;

impl Command for ListDrivers {
    type Params = DriverListParams;
    type Output = DriversAnswer;
    const ID: &'static str = "db.driver.list";
    const TITLE: &'static str = "List Database Drivers";
    const DESCRIPTION: &'static str = "Lists the database drivers of the IDE, those compiled in and those installed plug-ins add as driver programs: id, name, family, default port and capabilities (data models, query language, transactions, isolation levels, URI schemes, whether a password is asked, and the settings their connections have, which the connection dialog shows as fields); a plug-in's driver also has `external` (its plug-in, whether its program is installed, its icon, `perConnection` or `shared`, the protocol version and how many of its programs run). Also the database profiles over the drivers (ADR-0173: CockroachDB, TiDB, Valkey…): id, name, base `driver`, family, default port, database and user, URI schemes, managed-service hosts, TLS default, settings turned on, how the structure is read (`catalogue`), navigator folders hidden, docs link and capabilities. `query` filters both by id or name, `family` by family; `limit` caps the profiles (`truncated` says so) and `capabilities: false` leaves the profiles' capabilities out. Connects nowhere and starts nothing.";

    fn run(&self, session: &mut Session, params: DriverListParams) -> Result<DriversAnswer, CommandError> {
        let drivers = session.state_mut::<Database>().drivers();
        let query = params.query.as_deref().map(str::trim).unwrap_or_default().to_lowercase();
        let wanted = |id: &str, name: &str, family: &str| {
            params.family.as_deref().is_none_or(|wanted| wanted == family)
                && (query.is_empty() || id.to_lowercase().contains(&query) || name.to_lowercase().contains(&query))
        };
        let listed: Vec<DriverAnswer> = drivers
            .iter()
            .filter(|driver| wanted(driver.id(), driver.name(), zen_db::profile::family(driver.id())))
            .map(|driver| DriverAnswer {
                id: driver.id().to_owned(),
                name: driver.name().to_owned(),
                family: zen_db::profile::family(driver.id()).to_owned(),
                default_port: driver.default_port(),
                capabilities: driver.capabilities().clone(),
                external: driver.external().map(zen_db::external::ExternalDriver::info),
            })
            .collect();
        let with_capabilities = params.capabilities.unwrap_or(true);
        let profiles: Vec<zen_db::profile::ProfileAnswer> = zen_db::profile::PROFILES
            .iter()
            .filter(|profile| wanted(profile.id, profile.name, zen_db::profile::family(profile.driver)))
            .filter_map(|profile| {
                let base = drivers.iter().find(|driver| driver.id() == profile.driver)?;
                Some(profile.answer(with_capabilities.then(|| base.capabilities())))
            })
            .collect();
        let total_profiles = profiles.len();
        let limit = params.limit.unwrap_or(usize::MAX);
        Ok(DriversAnswer {
            drivers: listed,
            truncated: total_profiles > limit,
            profiles: profiles.into_iter().take(limit).collect(),
            total_profiles,
        })
    }
}

/// Parameters of `db.driver.log`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriverLogParams {
    /// The driver's id.
    pub id: String,
    /// The most lines answered, newest; 50 when absent.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// The log of an external driver.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DriverLogAnswer {
    /// Its lines, oldest first: programs started and stopped, what they wrote to standard error, why they failed; passwords removed.
    pub lines: Vec<String>,
    /// Whether older lines were left out.
    pub truncated: bool,
}

pub struct DriverLog;

impl Command for DriverLog {
    type Params = DriverLogParams;
    type Output = DriverLogAnswer;
    const ID: &'static str = "db.driver.log";
    const TITLE: &'static str = "Show Database Driver Log";
    const DESCRIPTION: &'static str = "The log of an external database driver (a plug-in's driver program, ADR-0174), newest `limit` lines (default 50): its programs started and stopped, what they wrote to standard error and why they failed, with passwords removed; never the statements or rows it carried. Fails with code unknownDriver for a driver that is not external.";

    fn run(&self, session: &mut Session, params: DriverLogParams) -> Result<DriverLogAnswer, CommandError> {
        let drivers = session.state_mut::<Database>().drivers();
        let driver = drivers.iter().find(|driver| driver.id() == params.id).and_then(|driver| driver.external());
        let Some(driver) = driver else {
            return Err(CommandError::with_code(
                "unknownDriver",
                t!("database-error-not-external-driver", id = params.id.as_str()),
            ));
        };
        let (lines, truncated) = driver.log().tail(params.limit.unwrap_or(LISTED));
        Ok(DriverLogAnswer { lines, truncated })
    }
}

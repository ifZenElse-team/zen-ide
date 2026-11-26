use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_lsp::ServerDeclaration;
use zen_lsp::connection::ContentSource;
use zen_syntax::{Feature, SchemaDeclaration, TextFeatures};

use crate::notifications::{Group, Kind, NewNotification};

pub const SCHEME: &str = "zen-schema://";

pub type PluginSource = Arc<dyn Fn() -> Vec<Arc<dyn TextFeatures>> + Send + Sync>;

fn source() -> &'static RwLock<Option<PluginSource>> {
    static SOURCE: std::sync::OnceLock<RwLock<Option<PluginSource>>> = std::sync::OnceLock::new();
    SOURCE.get_or_init(|| RwLock::new(None))
}

pub fn set_plugins(plugins: Option<PluginSource>) {
    if let Ok(mut slot) = source().write() {
        *slot = plugins;
    }
}

fn plugins() -> Vec<Arc<dyn TextFeatures>> {
    source().read().ok().and_then(|slot| slot.clone()).map(|source| source()).unwrap_or_default()
}

pub fn uri(plugin: &str, path: &str) -> String {
    format!("{SCHEME}{plugin}/{path}")
}

pub fn shipped(server: &str) -> Vec<(String, SchemaDeclaration)> {
    plugins()
        .into_iter()
        .filter(|plugin| plugin.supports(Feature::Resources))
        .filter_map(|plugin| Some((plugin.plugin_id()?.to_owned(), plugin.schemas())))
        .flat_map(|(id, schemas)| schemas.into_iter().map(move |schema| (id.clone(), schema)))
        .filter(|(_, schema)| schema.server == server && !schema.entry.is_empty())
        .collect()
}

pub fn mapping(server: &str) -> Map<String, Value> {
    mapping_in(server, None)
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Mapping {
    /// A shipped schema as `<plugin>/<id>` (from `schemas.catalog.list`), a JSON Schema file (relative to
    /// the project, or absolute), or an `http(s)` address, which is downloaded only when the person
    /// allowed downloads.
    pub schema: String,
    /// The file name patterns it applies to, matched anywhere in the project (`deploy/*.yaml`).
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    mappings: Vec<Mapping>,
    downloads: bool,
}

#[derive(Default)]
struct State {
    loaded: bool,
    saved: Saved,
    announced: Vec<(Option<PathBuf>, String)>,
    told: Vec<String>,
    kept: BTreeMap<String, String>,
}

const SETTINGS_FILE: &str = "schemas.json";

const ANNOUNCE_WAIT: Duration = Duration::from_secs(10);

fn state() -> &'static (Mutex<State>, Condvar) {
    static STATE: std::sync::OnceLock<(Mutex<State>, Condvar)> = std::sync::OnceLock::new();
    STATE.get_or_init(|| (Mutex::new(State::default()), Condvar::new()))
}

fn settings_file() -> Option<PathBuf> {
    crate::build_tools::BuildEnv::global().data_dir().map(|data| data.join(SETTINGS_FILE))
}

fn with_state<T>(run: impl FnOnce(&mut State) -> T) -> T {
    let mut guard = state().0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !guard.loaded {
        guard.loaded = true;
        guard.saved = settings_file()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
    }
    run(&mut guard)
}

fn save(saved: &Saved) -> Result<(), CommandError> {
    let Some(file) = settings_file() else { return Ok(()) };
    let bytes = serde_json::to_vec_pretty(saved).map_err(|error| CommandError::new(error.to_string()))?;
    zen_vfs::write_atomic(&file, &bytes).map_err(|error| CommandError::new(error.to_string()))
}

fn web(uri: &str) -> bool {
    uri.starts_with("http://") || uri.starts_with("https://")
}

fn download(uri: &str, root: Option<&Path>) -> Result<String, String> {
    let refused = || Err(t!("workbench-schema-download-off", uri = uri));
    if !with_state(|state| state.saved.downloads) {
        return refused();
    }
    if root.is_some_and(|root| !crate::trust::is_trusted(&crate::build_tools::BuildEnv::global(), root)) {
        return Err(t!("workbench-schema-download-untrusted", uri = uri));
    }
    if let Some(kept) = with_state(|state| state.kept.get(uri).cloned()) {
        return Ok(kept);
    }
    let (lock, posted) = state();
    {
        let mut guard = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.announced.push((root.map(Path::to_owned), uri.to_owned()));
        let (guard, waited) = posted
            .wait_timeout_while(guard, ANNOUNCE_WAIT, |state| !state.told.iter().any(|told| told == uri))
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if waited.timed_out() {
            drop(guard);
            return Err(t!("workbench-schema-download-unannounced", uri = uri));
        }
    }
    let text = fetch(uri).map_err(|error| t!("workbench-schema-download-failed", uri = uri, error = error))?;
    with_state(|state| state.kept.insert(uri.to_owned(), text.clone()));
    Ok(text)
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch(uri: &str) -> Result<String, String> {
    const MAX_DOWNLOAD: u64 = 16 * 1024 * 1024;
    let response = zen_net::agent_for(uri).get(uri).call().map_err(|error| error.to_string())?;
    response.into_body().into_with_config().limit(MAX_DOWNLOAD).read_to_string().map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
fn fetch(_uri: &str) -> Result<String, String> {
    Err("no network here".to_owned())
}

pub fn content_in(uri: &str, root: Option<&Path>) -> Result<String, String> {
    if let Some(rest) = uri.strip_prefix(SCHEME) {
        let (plugin, path) = rest.split_once('/').ok_or_else(|| format!("{uri}: no such schema"))?;
        let path = path.split(['#', '?']).next().unwrap_or_default();
        let found = plugins()
            .into_iter()
            .find(|candidate| candidate.plugin_id() == Some(plugin))
            .and_then(|candidate| candidate.resource(path))
            .ok_or_else(|| format!("{uri}: no such schema"))?;
        return String::from_utf8(found).map_err(|_| format!("{uri}: not UTF-8"));
    }
    if web(uri) {
        return download(uri, root);
    }
    Err(format!("{uri}: no such schema"))
}

pub fn content(uri: &str) -> Result<String, String> {
    content_in(uri, None)
}

pub fn content_source() -> ContentSource {
    Arc::new(content)
}

fn content_source_in(root: Option<PathBuf>) -> ContentSource {
    Arc::new(move |uri| content_in(uri, root.as_deref()))
}

pub(crate) fn announce(session: &mut Session) {
    let project = session.state::<crate::Workbench>().and_then(|workbench| workbench.project().map(Path::to_owned));
    let waiting = with_state(|state| {
        let (mine, others): (Vec<_>, Vec<_>) =
            std::mem::take(&mut state.announced).into_iter().partition(|(root, _)| root.is_none() || *root == project);
        state.announced = others;
        mine
    });
    if waiting.is_empty() {
        return;
    }
    for (_, uri) in waiting {
        let body = t!("workbench-schema-downloading", uri = uri.as_str());
        crate::notifications::post(session, NewNotification::message(Group::General, Kind::Info, body));
        with_state(|state| state.told.push(uri));
    }
    state().1.notify_all();
}

fn mapped_uri(schema: &str, root: Option<&Path>) -> Option<String> {
    let schema = schema.trim();
    if schema.is_empty() {
        return None;
    }
    if schema.starts_with(SCHEME) || web(schema) {
        return Some(schema.to_owned());
    }
    if let Some((plugin, id)) = schema.split_once('/')
        && let Some((owner, shipped)) =
            plugins().into_iter().filter(|candidate| candidate.plugin_id() == Some(plugin)).find_map(|candidate| {
                let entry = candidate.schemas().into_iter().find(|declared| declared.id == id)?;
                Some((plugin.to_owned(), entry))
            })
    {
        return Some(uri(&owner, &shipped.entry));
    }
    let path = Path::new(schema);
    let path = if path.is_absolute() { path.to_owned() } else { root?.join(path) };
    Some(zen_lsp::position::file_uri(&path))
}

pub fn mapping_in(server: &str, root: Option<&Path>) -> Map<String, Value> {
    let own = with_state(|state| state.saved.mappings.clone());
    let taken: Vec<&String> = own.iter().flat_map(|mapping| &mapping.files).collect();
    let mut map = Map::new();
    for (plugin, schema) in shipped(server) {
        let files: Vec<String> = schema.files.into_iter().filter(|file| !taken.contains(&file)).collect();
        if !files.is_empty() {
            map.insert(uri(&plugin, &schema.entry), Value::from(files));
        }
    }
    for mapping in &own {
        let Some(uri) = mapped_uri(&mapping.schema, root) else { continue };
        let entry = map.entry(uri).or_insert_with(|| Value::Array(Vec::new()));
        if let Some(files) = entry.as_array_mut() {
            files.extend(mapping.files.iter().cloned().map(Value::from));
        }
    }
    map
}

pub(crate) fn serve(
    declaration: &ServerDeclaration,
    configuration: &mut BTreeMap<String, Value>,
    root: Option<&Path>,
) -> (Option<ContentSource>, Vec<String>) {
    let Some(support) = &declaration.schema_support else { return (None, Vec::new()) };
    let content = Some(content_source_in(root.map(Path::to_owned)));
    if support.setting.is_empty() {
        return (content, support.register.iter().cloned().collect());
    }
    let (section, key) = support.setting.split_once('.').unwrap_or((support.setting.as_str(), ""));
    let section_value = configuration.entry(section.to_owned()).or_insert_with(|| Value::Object(Map::new()));
    if !section_value.is_object() {
        *section_value = Value::Object(Map::new());
    }
    if let Some(object) = section_value.as_object_mut()
        && !key.is_empty()
    {
        let mut map = object.get(key).and_then(Value::as_object).cloned().unwrap_or_default();
        map.extend(mapping_in(&declaration.id, root));
        object.insert(key.to_owned(), Value::Object(map));
    }
    (content, support.register.iter().cloned().collect())
}

/// A shipped schema, as `schemas.catalog.list` answers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Shipped {
    /// `<plugin>/<id>`, what a mapping names it by.
    pub schema: String,
    /// Its name.
    pub name: String,
    /// The language server that uses it.
    pub server: String,
    /// The file patterns it applies to by itself.
    pub files: Vec<String>,
    /// The address a file's comment names it by (`# yaml-language-server: $schema=...`).
    pub uri: String,
}

/// The schemas, a person's own mappings and whether downloads are allowed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemaList {
    /// The schemas the plug-ins ship.
    pub shipped: Vec<Shipped>,
    /// A person's own mappings, which win over a shipped pattern for the same file.
    pub mappings: Vec<Mapping>,
    /// Whether a schema a file names with a web address may be downloaded.
    pub downloads: bool,
}

fn list() -> SchemaList {
    let shipped = plugins()
        .into_iter()
        .filter(|plugin| plugin.supports(Feature::Resources))
        .filter_map(|plugin| Some((plugin.plugin_id()?.to_owned(), plugin.schemas())))
        .flat_map(|(id, schemas)| {
            schemas.into_iter().map(move |schema| Shipped {
                schema: format!("{id}/{}", schema.id),
                uri: uri(&id, &schema.entry),
                name: schema.name,
                server: schema.server,
                files: schema.files,
            })
        })
        .collect();
    let (mappings, downloads) = with_state(|state| (state.saved.mappings.clone(), state.saved.downloads));
    SchemaList { shipped, mappings, downloads }
}

fn restart(session: &mut Session) {
    let languages = session.state_mut::<crate::language::Languages>();
    languages.restart_schema_servers();
}

pub struct ListSchemas;

impl Command for ListSchemas {
    type Params = NoParams;
    type Output = SchemaList;
    const ID: &'static str = "schemas.catalog.list";
    const TITLE: &'static str = "Schemas";
    const DESCRIPTION: &'static str = "The schemas the plug-ins ship (with the patterns they apply to and the address a \
         file's comment names them by), a person's own mappings and whether schema downloads are allowed.";

    fn run(&self, _session: &mut Session, _: NoParams) -> Result<SchemaList, CommandError> {
        Ok(list())
    }
}

/// The parameters of `schemas.mappings.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MappingsParams {
    /// Every mapping, replacing the ones there were.
    pub mappings: Vec<Mapping>,
}

pub struct SetMappings;

impl Command for SetMappings {
    type Params = MappingsParams;
    type Output = SchemaList;
    const ID: &'static str = "schemas.mappings.set";
    const TITLE: &'static str = "Set Schema Mappings";
    const DESCRIPTION: &'static str = "Replaces a person's own schema mappings ({schema, files}); the language servers \
         with schemas restart to take them.";

    fn run(&self, session: &mut Session, params: MappingsParams) -> Result<SchemaList, CommandError> {
        let mappings: Vec<Mapping> = params
            .mappings
            .into_iter()
            .map(|mapping| Mapping {
                schema: mapping.schema.trim().to_owned(),
                files: mapping
                    .files
                    .into_iter()
                    .map(|file| file.trim().to_owned())
                    .filter(|file| !file.is_empty())
                    .collect(),
            })
            .collect();
        if let Some(empty) = mappings.iter().find(|mapping| mapping.schema.is_empty() || mapping.files.is_empty()) {
            return Err(CommandError::new(t!("workbench-schema-mapping-incomplete", schema = empty.schema.as_str())));
        }
        let saved = with_state(|state| {
            state.saved.mappings = mappings;
            state.saved.clone()
        });
        save(&saved)?;
        restart(session);
        Ok(list())
    }
}

/// The parameters of `schemas.downloads.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadsParams {
    /// Whether a schema a file names with a web address may be downloaded.
    pub allowed: bool,
}

pub struct SetDownloads;

impl Command for SetDownloads {
    type Params = DownloadsParams;
    type Output = SchemaList;
    const ID: &'static str = "schemas.downloads.set";
    const TITLE: &'static str = "Allow Schema Downloads";
    const DESCRIPTION: &'static str = "Allows or forbids downloading a schema a file names with a web address; the \
         person's alone. Never in a project nobody trusted; a notification names each address before it downloads.";

    fn run(&self, session: &mut Session, params: DownloadsParams) -> Result<SchemaList, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(
                crate::trust::AGENT_NOT_ALLOWED,
                t!("workbench-schema-downloads-person"),
            ));
        }
        let saved = with_state(|state| {
            state.saved.downloads = params.allowed;
            if !params.allowed {
                state.kept.clear();
            }
            state.saved.clone()
        });
        save(&saved)?;
        restart(session);
        Ok(list())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListSchemas)?;
    registry.register(SetMappings)?;
    registry.register(SetDownloads)?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn reset_for_tests() {
    with_state(|state| *state = State { loaded: true, ..State::default() });
}

#[cfg(test)]
mod tests;

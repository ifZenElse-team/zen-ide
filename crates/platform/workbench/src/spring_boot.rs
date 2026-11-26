pub mod beans;
pub mod config;
mod index;
pub mod metadata;
pub mod run;
pub mod source;
pub mod starters;
pub mod support;
#[cfg(test)]
mod tests;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::{Suggestion, Suggestions};

use crate::Workbench;
use crate::build_tools::BuildEnv;
use crate::language::{Problem, byte_offset, char_offset};
use crate::project_model::ProjectModels;
use config::{ConfigKey, Format, Place};
use metadata::{Metadata, Property};

pub const PLUGIN: &str = "spring-boot";

const MAX_WAIT: Duration = Duration::from_secs(120);

pub type JarSource = Arc<dyn Fn(&Path) -> Vec<PathBuf> + Send + Sync>;

#[derive(Clone, Debug, PartialEq)]
struct Key {
    root: PathBuf,
    revision: u64,
    own: Vec<PathBuf>,
    stamps: Vec<Option<(Option<std::time::SystemTime>, u64)>>,
}

#[derive(Default)]
struct Shared {
    loaded: Option<(Key, Arc<Metadata>, usize)>,
    loading: Option<Key>,
    generation: u64,
}

type Cell = Arc<(Mutex<Shared>, Condvar)>;

#[derive(Default)]
pub struct SpringBoot {
    jars: Option<JarSource>,
    shared: Cell,
    problems: HashMap<PathBuf, (u64, u64, Vec<Problem>)>,
    index: index::Index,
}

impl SpringBoot {
    pub fn set_jars(&mut self, source: JarSource) {
        self.jars = Some(source);
    }
}

fn lock(cell: &Cell) -> std::sync::MutexGuard<'_, Shared> {
    cell.0.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn is_on(session: &Session) -> bool {
    session.state::<crate::plugin_projects::PluginProjects>().is_some_and(|projects| projects.installed(PLUGIN))
}

pub(crate) fn serves(session: &Session, path: &Path) -> Option<Format> {
    config::format_of(path).filter(|_| is_on(session))
}

fn key_of(session: &Session) -> Option<Key> {
    let root = crate::run::project_root(session).ok()?;
    let models = session.state::<ProjectModels>();
    let modules: Vec<PathBuf> = models
        .map(|models| models.model().modules.iter().map(|module| module.root.clone()).collect())
        .unwrap_or_default();
    let own = metadata::own_files(&root, &modules);
    let stamps = metadata::stamps(&own);
    Some(Key { root, revision: models.map_or(0, ProjectModels::revision), own, stamps })
}

fn jar_source(session: &Session) -> JarSource {
    if let Some(source) = session.state::<SpringBoot>().and_then(|spring| spring.jars.clone()) {
        return source;
    }
    let env = session.state::<BuildEnv>().cloned().unwrap_or_default();
    Arc::new(move |root| crate::jvm_libraries::JvmLibraries::new(env.clone()).jars(root))
}

fn ensure(session: &mut Session) -> (Cell, Option<Key>) {
    let key = key_of(session);
    let source = jar_source(session);
    let cell = Arc::clone(&session.state_mut::<SpringBoot>().shared);
    let Some(key) = key else { return (cell, None) };
    let started = {
        let mut shared = lock(&cell);
        let fresh = shared.loaded.as_ref().is_some_and(|(loaded, _, _)| *loaded == key);
        let start = !fresh && shared.loading.as_ref() != Some(&key);
        if start {
            shared.loading = Some(key.clone());
        }
        start
    };
    if !started {
        return (cell, Some(key));
    }
    let work = {
        let (cell, key) = (Arc::clone(&cell), key.clone());
        move || {
            let jars = source(&key.root);
            let read = Arc::new(metadata::load(&jars, &key.own));
            let mut shared = lock(&cell);
            if shared.loading.as_ref() == Some(&key) {
                shared.loading = None;
                shared.loaded = Some((key, read, jars.len()));
                shared.generation += 1;
            }
            drop(shared);
            cell.1.notify_all();
        }
    };
    #[cfg(not(target_arch = "wasm32"))]
    std::thread::spawn(work);
    #[cfg(target_arch = "wasm32")]
    work();
    (cell, Some(key))
}

fn loaded(session: &mut Session) -> Option<(Arc<Metadata>, u64)> {
    let (cell, key) = ensure(session);
    let key = key?;
    let shared = lock(&cell);
    let (seen, metadata, _) = shared.loaded.as_ref()?;
    (seen.root == key.root).then(|| (Arc::clone(metadata), shared.generation))
}

fn wait_loaded(session: &mut Session, wait: Duration) -> (Option<Arc<Metadata>>, bool, usize) {
    let (cell, key) = ensure(session);
    let Some(key) = key else { return (None, false, 0) };
    let deadline = Instant::now() + wait.min(MAX_WAIT);
    let mut shared = lock(&cell);
    loop {
        if let Some((seen, metadata, jars)) = shared.loaded.as_ref()
            && *seen == key
        {
            return (Some(Arc::clone(metadata)), false, *jars);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            let stale = shared.loaded.as_ref().filter(|(seen, _, _)| seen.root == key.root);
            return (stale.map(|(_, metadata, _)| Arc::clone(metadata)), true, stale.map_or(0, |(_, _, jars)| *jars));
        }
        shared = cell.1.wait_timeout(shared, left).unwrap_or_else(PoisonError::into_inner).0;
    }
}

fn document(session: &Session, path: &Path) -> Option<(u64, String)> {
    let workbench = session.state::<Workbench>()?;
    let document = workbench.documents().iter().find(|document| document.path() == path)?;
    document.image().is_none().then(|| (document.version(), document.text().to_owned()))
}

fn short_type(ty: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    for c in ty.chars() {
        if c.is_alphanumeric() || c == '.' || c == '_' || c == '$' {
            word.push(c);
        } else {
            out.push_str(word.rsplit('.').next().unwrap_or(&word));
            word.clear();
            out.push(c);
        }
    }
    out.push_str(word.rsplit('.').next().unwrap_or(&word));
    out
}

pub(crate) fn completes_in(session: &Session, path: &Path) -> bool {
    serves(session, path).is_some()
}

fn present(keys: &[ConfigKey]) -> BTreeSet<String> {
    keys.iter().filter(|key| key.leaf).map(|key| metadata::canonical(&key.name)).collect()
}

fn value_items(property: &Property) -> Vec<(Suggestion, Option<String>)> {
    let mut items: Vec<(Suggestion, Option<String>)> = property
        .values
        .iter()
        .map(|hint| {
            let text = metadata::value_text(&hint.value);
            let suggestion = Suggestion {
                label: text.clone(),
                kind: "value".to_owned(),
                insert: text,
                caret: None,
                detail: property.value_type.as_deref().map(short_type),
            };
            (suggestion, hint.description.clone())
        })
        .collect();
    if items.is_empty() && property.is_boolean() {
        for value in ["true", "false"] {
            let suggestion = Suggestion {
                label: value.to_owned(),
                kind: "value".to_owned(),
                insert: value.to_owned(),
                caret: None,
                detail: Some("Boolean".to_owned()),
            };
            items.push((suggestion, None));
        }
    }
    items
}

pub(crate) fn complete(
    session: &mut Session,
    path: &Path,
    text: &str,
    caret: usize,
) -> Option<(Suggestions, Vec<Option<String>>)> {
    let format = serves(session, path)?;
    let (metadata, _) = loaded(session)?;
    let place = config::place_at(format, text, caret)?;
    let items: Vec<(Suggestion, Option<String>)> = match &place {
        Place::Value { key, .. } => metadata.find(key).map(value_items).unwrap_or_default(),
        Place::Key { parent, indent, .. } => {
            let keys = config::keys(format, text);
            let present = present(&keys);
            let rest_of_line = text[caret..].split(['\n', '\r']).next().unwrap_or("").trim();
            let unit = config::indent_unit(&keys);
            let prefix = if parent.is_empty() { String::new() } else { format!("{}.", metadata::canonical(parent)) };
            let mut items = Vec::new();
            for property in metadata.properties() {
                let canonical = metadata::canonical(&property.name);
                if present.contains(&canonical) || !canonical.starts_with(&prefix) {
                    continue;
                }
                let relative = if parent.is_empty() {
                    property.name.clone()
                } else {
                    let dots = parent.matches('.').count() + 1;
                    property.name.splitn(dots + 1, '.').nth(dots).unwrap_or(&property.name).to_owned()
                };
                let insert = match format {
                    Format::Properties if rest_of_line.is_empty() => format!("{relative}="),
                    Format::Yaml if rest_of_line.is_empty() => config::yaml_insert(&relative, *indent, unit, "\n"),
                    _ => relative.clone(),
                };
                let suggestion = Suggestion {
                    label: relative,
                    kind: "property".to_owned(),
                    insert,
                    caret: None,
                    detail: property.value_type.as_deref().map(short_type),
                };
                items.push((suggestion, Some(markdown(property))));
            }
            items
        }
    };
    let from = match place {
        Place::Key { from, .. } | Place::Value { from, .. } => from,
    };
    let (items, docs): (Vec<Suggestion>, Vec<Option<String>>) = items.into_iter().unzip();
    Some((Suggestions { from, items }, docs))
}

pub fn markdown(property: &Property) -> String {
    let mut out = format!("**{}**", property.name);
    if let Some(ty) = &property.value_type {
        out.push_str(&format!("\n\n`{ty}`"));
    }
    if let Some(description) = property.description.as_deref().filter(|text| !text.trim().is_empty()) {
        out.push_str(&format!("\n\n{}", description.trim()));
    }
    if let Some(default) = property.default_text() {
        out.push_str(&format!("\n\n{} `{default}`", t!("workbench-spring-boot-default")));
    }
    if let Some(deprecation) = &property.deprecation {
        out.push_str(&format!("\n\n**{}**", t!("workbench-spring-boot-deprecated")));
        if let Some(reason) = deprecation.reason.as_deref().filter(|text| !text.trim().is_empty()) {
            out.push_str(&format!(" {}", reason.trim()));
        }
        if let Some(replacement) = &deprecation.replacement {
            out.push_str(&format!(
                "\n\n{}",
                t!("workbench-spring-boot-replacement", replacement = format!("`{replacement}`"))
            ));
        }
    }
    if let Some(source) = &property.source_type {
        out.push_str(&format!("\n\n{}", t!("workbench-spring-boot-declared-by", source = format!("`{source}`"))));
    }
    out
}

pub(crate) struct Documented {
    pub markdown: String,
    pub range: [usize; 2],
}

pub(crate) fn documentation(session: &mut Session, path: &Path, offset: usize) -> Option<Documented> {
    let format = serves(session, path)?;
    let (_, text) = document(session, path)?;
    let byte = byte_offset(&text, offset);
    let keys = config::keys(format, &text);
    let key = keys.iter().find(|key| key.range.start <= byte && byte <= key.range.end)?;
    let (metadata, _) = loaded(session)?;
    let property = metadata.find(&key.name)?;
    Some(Documented {
        markdown: markdown(property),
        range: [char_offset(&text, key.range.start), char_offset(&text, key.range.end)],
    })
}

fn finding(
    start: usize,
    end: usize,
    severity: zen_project::inspect::FindingSeverity,
    message: String,
) -> zen_project::inspect::Finding {
    zen_project::inspect::Finding {
        start,
        end,
        severity,
        message,
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        source: PLUGIN.to_owned(),
        code: String::new(),
        deprecated: false,
        unused: false,
    }
}

pub fn check(format: Format, text: &str, metadata: &Metadata) -> Vec<zen_project::inspect::Finding> {
    use zen_project::inspect::FindingSeverity;
    if metadata.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for key in config::keys(format, text).iter().filter(|key| key.leaf && !key.listed) {
        let (start, end) = (key.range.start, key.range.end);
        match metadata.find(&key.name) {
            None => found.push(zen_project::inspect::Finding {
                code: "unknown".to_owned(),
                ..finding(
                    start,
                    end,
                    FindingSeverity::WeakWarning,
                    t!("workbench-spring-boot-unknown", name = key.name.clone()),
                )
            }),
            Some(property) => {
                let Some(deprecation) = &property.deprecation else { continue };
                let message = match &deprecation.replacement {
                    Some(replacement) => t!(
                        "workbench-spring-boot-deprecated-use",
                        name = key.name.clone(),
                        replacement = replacement.clone()
                    ),
                    None => t!("workbench-spring-boot-deprecated-key", name = key.name.clone()),
                };
                let severity = if deprecation.is_error() { FindingSeverity::Error } else { FindingSeverity::Warning };
                found.push(zen_project::inspect::Finding {
                    code: "deprecated".to_owned(),
                    deprecated: true,
                    ..finding(start, end, severity, message)
                });
            }
        }
    }
    found
}

pub(crate) fn problems(session: &mut Session, path: &Path) -> Vec<Problem> {
    let Some(format) = serves(session, path) else { return Vec::new() };
    let Some((version, text)) = document(session, path) else { return Vec::new() };
    let Some((metadata, generation)) = loaded(session) else { return Vec::new() };
    if let Some((seen, at, problems)) = session.state::<SpringBoot>().and_then(|spring| spring.problems.get(path))
        && (*seen, *at) == (version, generation)
    {
        return problems.clone();
    }
    let problems: Vec<Problem> =
        check(format, &text, &metadata).iter().map(|finding| crate::inspections::problem(&text, finding)).collect();
    let open: BTreeSet<PathBuf> = session
        .state::<Workbench>()
        .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
        .unwrap_or_default();
    let spring = session.state_mut::<SpringBoot>();
    spring.problems.retain(|kept, _| open.contains(kept));
    spring.problems.insert(path.to_owned(), (version, generation, problems.clone()));
    problems
}

pub(crate) fn known(session: &Session, path: &Path) -> Vec<Problem> {
    let version = document(session, path).map(|(version, _)| version);
    session
        .state::<SpringBoot>()
        .and_then(|spring| spring.problems.get(path))
        .filter(|(seen, _, _)| Some(*seen) == version)
        .map(|(_, _, problems)| problems.clone())
        .unwrap_or_default()
}

/// The parameters of `springBoot.properties.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PropertiesParams {
    /// Only the properties whose name contains this text (ignoring case, `-` and `_`).
    pub query: Option<String>,
    /// At most this many properties (100 by default).
    pub limit: Option<usize>,
    /// Waits up to this long for the project's metadata to be read (at most two minutes).
    pub wait_ms: Option<u64>,
}

/// The configuration properties the project's jars declare.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PropertyList {
    /// Whether the metadata is still being read; what is listed may then be older.
    pub loading: bool,
    /// How many jars of the project were read.
    pub jars: usize,
    /// How many properties are known in all.
    pub total: usize,
    /// The matching properties, by name.
    pub properties: Vec<Property>,
}

pub struct ListProperties;

impl Command for ListProperties {
    type Params = PropertiesParams;
    type Output = PropertyList;
    const ID: &'static str = "springBoot.properties.list";
    const TITLE: &'static str = "Spring Boot Configuration Properties";
    const DESCRIPTION: &'static str = "The configuration properties that application.properties and application.yml \
         can set, read from META-INF/spring-configuration-metadata.json and \
         additional-spring-configuration-metadata.json in the project's jars and its own build output: name, type, \
         description, default, deprecation and known values. Needs the Spring Boot plug-in.";

    fn run(&self, session: &mut Session, params: PropertiesParams) -> Result<PropertyList, CommandError> {
        if !is_on(session) {
            return Err(CommandError::new(t!("workbench-spring-boot-off")));
        }
        let (metadata, loading, jars) = wait_loaded(session, Duration::from_millis(params.wait_ms.unwrap_or(0)));
        let Some(metadata) = metadata else { return Ok(PropertyList { loading, ..PropertyList::default() }) };
        let query = params.query.as_deref().map(metadata::canonical).unwrap_or_default();
        let properties = metadata
            .properties()
            .filter(|property| metadata::canonical(&property.name).contains(&query))
            .take(params.limit.unwrap_or(100))
            .cloned()
            .collect();
        Ok(PropertyList { loading, jars, total: metadata.len(), properties })
    }
}

/// A Spring Boot application of the project.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Application {
    /// The file with its `@SpringBootApplication` class.
    pub path: PathBuf,
    /// The line of the class that has its `main`, one-based.
    pub line: u32,
    /// The run configuration that starts it, for `run.configuration.create` or `run.process.start`.
    pub configuration: zen_run::Configuration,
}

/// What `springBoot.applications.list` answers.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationList {
    /// The applications, by file.
    pub applications: Vec<Application>,
}

const SKIPPED: &[&str] = &["target", "build", "out", "node_modules", ".git", ".gradle", ".idea", ".zen"];

fn sources(root: &Path, found: &mut Vec<PathBuf>, depth: usize) {
    if depth > 24 || found.len() > 20_000 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(root) else { return };
    let mut entries: Vec<_> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if path.is_dir() {
            if !SKIPPED.contains(&name.as_str()) {
                sources(&path, found, depth + 1);
            }
        } else if name.ends_with(".java") || name.ends_with(".kt") {
            found.push(path);
        }
    }
}

pub struct ListApplications;

impl Command for ListApplications {
    type Params = zen_core::NoParams;
    type Output = ApplicationList;
    const ID: &'static str = "springBoot.applications.list";
    const TITLE: &'static str = "Spring Boot Applications";
    const DESCRIPTION: &'static str = "The project's @SpringBootApplication classes (Java and Kotlin) that have a main, \
         each with the Spring Boot run configuration that starts it. Needs the Spring Boot plug-in.";

    fn run(&self, session: &mut Session, _params: zen_core::NoParams) -> Result<ApplicationList, CommandError> {
        if !is_on(session) {
            return Err(CommandError::new(t!("workbench-spring-boot-off")));
        }
        let root = crate::run::project_root(session)?;
        let mut files = Vec::new();
        sources(&root, &mut files, 0);
        let provider = run::SpringBootRun::default();
        let mut applications = Vec::new();
        for path in files {
            let text = match document(session, &path) {
                Some((_, text)) => text,
                None => std::fs::read_to_string(&path).unwrap_or_default(),
            };
            if !text.contains("SpringBootApplication") {
                continue;
            }
            for (line, configuration) in provider.applications(&root, &path, &text) {
                applications.push(Application { path: path.clone(), line: line + 1, configuration });
            }
        }
        Ok(ApplicationList { applications })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListProperties)?;
    registry.register(ListApplications)?;
    registry.register(beans::ListMarkers)?;
    registry.register(beans::NavigateMarker)?;
    registry.register(beans::ListEndpoints)?;
    registry.register(beans::OpenEndpoint)?;
    registry.register(beans::RefreshEndpoints)?;
    registry.register(starters::GetStarters)?;
    registry.register(starters::ApplyStarters)?;
    Ok(())
}

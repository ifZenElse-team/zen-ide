use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::{IndexDeclaration, ProjectIndex, TextFeatures};

use crate::Workbench;
use crate::hidden_files;

pub type PluginSource = Arc<dyn Fn() -> Vec<Arc<dyn TextFeatures>> + Send + Sync>;

pub const AFTER_CHANGE: Duration = Duration::from_millis(500);

const SKIPPED_FOLDERS: [&str; 1] = ["node_modules"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub files: usize,
    pub file_size: u64,
    pub total: u64,
    pub paths: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { files: 5_000, file_size: 1 << 20, total: 64 << 20, paths: 100_000 }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Built {
    pub index: ProjectIndex,
    pub files: usize,
    pub digests: BTreeMap<String, u64>,
}

pub fn digest(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

struct Running {
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Built>>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Entry {
    built: Option<Arc<Built>>,
    running: Option<Running>,
    stale: bool,
}

#[derive(Default)]
pub struct Indexes {
    source: Option<PluginSource>,
    root: Option<PathBuf>,
    entries: BTreeMap<String, Entry>,
    changed_at: Option<Instant>,
    limits: Limits,
    reveal: Option<PathBuf>,
}

impl Indexes {
    pub fn set_plugins(&mut self, source: PluginSource) {
        self.source = Some(source);
        for entry in self.entries.values_mut() {
            entry.stale = true;
        }
    }

    pub fn set_limits(&mut self, limits: Limits) {
        self.limits = limits;
    }

    pub fn indexing(&self) -> bool {
        self.entries.values().any(|entry| entry.running.is_some() || entry.stale)
    }

    pub fn built(&self, plugin: &str) -> Option<Arc<Built>> {
        self.entries.get(plugin).and_then(|entry| entry.built.clone())
    }

    pub fn all(&self) -> Vec<(String, Arc<Built>)> {
        self.entries.iter().filter_map(|(id, entry)| Some((id.clone(), entry.built.clone()?))).collect()
    }

    fn plugins(&self) -> Vec<(String, Arc<dyn TextFeatures>, IndexDeclaration)> {
        let Some(source) = &self.source else { return Vec::new() };
        source()
            .into_iter()
            .filter_map(|plugin| {
                let declaration = plugin.index_declaration()?;
                let id = plugin.plugin_id()?.to_owned();
                Some((id, plugin, declaration))
            })
            .collect()
    }
}

pub fn files_changed(session: &mut Session, paths: &[PathBuf], now: Instant) {
    let indexes = session.state_mut::<Indexes>();
    let declarations: Vec<IndexDeclaration> =
        indexes.plugins().into_iter().map(|(_, _, declaration)| declaration).collect();
    let touched = paths.iter().any(|path| {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        !path.is_file() || declarations.iter().any(|declaration| zen_syntax::index_matches(declaration, &name))
    });
    if touched {
        for entry in indexes.entries.values_mut() {
            entry.stale = true;
        }
        indexes.changed_at = Some(now);
    }
}

pub fn poll(session: &mut Session, now: Instant) {
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf);
    let file_index = session.state::<Workbench>().and_then(Workbench::file_index).cloned();
    let indexes = session.state_mut::<Indexes>();
    if indexes.root != root {
        indexes.root = root.clone();
        indexes.entries.clear();
        indexes.changed_at = None;
    }
    let Some(root) = root else { return };
    let plugins = indexes.plugins();
    indexes.entries.retain(|id, _| plugins.iter().any(|(plugin, _, _)| plugin == id));
    for (id, _, _) in &plugins {
        indexes.entries.entry(id.clone()).or_insert_with(|| Entry { stale: true, ..Entry::default() });
    }
    let waiting = indexes.changed_at.is_some_and(|changed| now.saturating_duration_since(changed) < AFTER_CHANGE);
    let limits = indexes.limits;
    for (id, plugin, declaration) in plugins {
        let Some(entry) = indexes.entries.get_mut(&id) else { continue };
        if let Some(running) = &entry.running {
            let finished = running.result.lock().unwrap_or_else(PoisonError::into_inner).take();
            if let Some(built) = finished {
                entry.built = Some(Arc::new(built));
                entry.running = None;
            }
        }
        if !entry.stale || waiting {
            continue;
        }
        entry.stale = false;
        let cancel = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        entry.running = Some(Running { cancel: Arc::clone(&cancel), result: Arc::clone(&result) });
        let (root, file_index) = (root.clone(), file_index.clone());
        let spawned = std::thread::Builder::new().name(format!("zen-index-{id}")).spawn(move || {
            let built = build(&root, file_index.as_ref(), plugin.as_ref(), &declaration, limits, &cancel);
            if let Some(built) = built {
                *result.lock().unwrap_or_else(PoisonError::into_inner) = Some(built);
            }
        });
        if spawned.is_err() {
            entry.running = None;
        }
    }
}

#[derive(Serialize)]
struct Input<'a> {
    files: Vec<InputFile<'a>>,
    paths: Vec<&'a str>,
    remotes: Vec<String>,
}

pub fn remotes(root: &Path) -> Vec<String> {
    let mut urls: Vec<String> = zen_vcs::Repository::discover(root)
        .and_then(|repository| repository.remote_urls().ok())
        .unwrap_or_default()
        .into_iter()
        .map(|(_, url)| url)
        .collect();
    urls.sort();
    urls.dedup();
    urls
}

#[derive(Serialize)]
struct InputFile<'a> {
    path: &'a str,
    text: &'a str,
}

pub struct Walked {
    pub files: Vec<(String, String)>,
    pub paths: Vec<String>,
    pub truncated: bool,
}

pub fn walk(
    root: &Path,
    file_index: Option<&zen_index::FileIndex>,
    declaration: &IndexDeclaration,
    limits: Limits,
    cancel: &AtomicBool,
) -> Option<Walked> {
    let exclusions = file_index.map(|index| index.exclusions().clone()).unwrap_or_default();
    let mut walked = Walked { files: Vec::new(), paths: Vec::new(), truncated: false };
    let mut total = 0u64;
    let mut queue = VecDeque::from([root.to_path_buf()]);
    while let Some(folder) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if exclusions.excludes_entry(&path) || file_index.is_some_and(|index| index.git_ignores(&path)) {
                continue;
            }
            if kind.is_dir() {
                if zen_vfs::HIDDEN.contains(&name.as_str())
                    || SKIPPED_FOLDERS.contains(&name.as_str())
                    || zen_vfs::TOOL_FOLDERS.contains(&name.as_str())
                    || zen_vfs::excluded_folders::is_excluded(&path)
                {
                    continue;
                }
                queue.push_back(path.clone());
            }
            if walked.paths.len() >= limits.paths {
                walked.truncated = true;
                return Some(walked);
            }
            let relative = path.strip_prefix(root).unwrap_or(&path).to_string_lossy().replace('\\', "/");
            walked.paths.push(relative.clone());
            if !kind.is_file() || !zen_syntax::index_matches(declaration, &name) {
                continue;
            }
            let size = entry.metadata().map(|meta| meta.len()).unwrap_or(u64::MAX);
            if walked.files.len() >= limits.files || size > limits.file_size || total + size > limits.total {
                walked.truncated = true;
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            total += size;
            walked.files.push((relative, text));
        }
    }
    Some(walked)
}

pub fn build(
    root: &Path,
    file_index: Option<&zen_index::FileIndex>,
    plugin: &dyn TextFeatures,
    declaration: &IndexDeclaration,
    limits: Limits,
    cancel: &AtomicBool,
) -> Option<Built> {
    let walked = walk(root, file_index, declaration, limits, cancel)?;
    let input = Input {
        files: walked.files.iter().map(|(path, text)| InputFile { path, text }).collect(),
        paths: walked.paths.iter().map(String::as_str).collect(),
        remotes: remotes(root),
    };
    let bytes = serde_json::to_vec(&input).ok()?;
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    let files: Vec<(&str, &str)> = walked.files.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
    let mut index = plugin.index(&bytes).unwrap_or_default().checked(&files);
    index.truncated |= walked.truncated;
    let digests = walked.files.iter().map(|(path, text)| (path.clone(), digest(text))).collect();
    Some(Built { index, files: files.len(), digests })
}

/// The parameters of `plugins.index.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GetParams {
    /// The plug-in whose index to give; all plug-ins with an index when left out.
    pub plugin: Option<String>,
    /// Only nodes of these kinds (`Kustomization`, `Application`...).
    pub kinds: Vec<String>,
    /// Only what is in this file (relative to the project or absolute).
    pub path: Option<String>,
    /// Also the references (spans that name a node).
    pub references: bool,
    /// Wait up to this many milliseconds for an index that is being built.
    pub wait_ms: u64,
}

#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexAnswer {
    pub plugin: String,
    pub indexing: bool,
    pub files: usize,
    #[serde(flatten)]
    pub index: ProjectIndex,
}

#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IndexList {
    pub indexes: Vec<IndexAnswer>,
    pub indexing: bool,
}

fn project_relative(session: &Session, path: &str) -> String {
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf);
    let path = Path::new(path);
    match root {
        Some(root) if path.is_absolute() => {
            path.strip_prefix(&root).unwrap_or(path).to_string_lossy().replace('\\', "/")
        }
        _ => path.to_string_lossy().replace('\\', "/"),
    }
}

pub fn visible(session: &Session, mut index: ProjectIndex) -> ProjectIndex {
    let Some(patterns) = hidden_files::for_caller(session) else { return index };
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf);
    let hidden = |path: &str| {
        let full = root.as_ref().map_or_else(|| PathBuf::from(path), |root| root.join(path));
        hidden_files::is_hidden(&patterns, root.as_deref(), &full)
    };
    let all: std::collections::BTreeSet<String> = index.nodes.iter().map(|node| node.id.clone()).collect();
    index.nodes.retain(|node| !hidden(&node.path));
    let kept: std::collections::BTreeSet<String> = index.nodes.iter().map(|node| node.id.clone()).collect();
    index.edges.retain(|edge| kept.contains(&edge.from) && (kept.contains(&edge.to) || !all.contains(&edge.to)));
    index.references.retain(|reference| kept.contains(&reference.target) || !all.contains(&reference.target));
    index.references.retain(|reference| !hidden(&reference.path));
    index.problems.retain(|problem| !hidden(&problem.path));
    for completion in &mut index.completions {
        completion.files.retain(|path| !hidden(path));
    }
    index.completions.retain(|completion| !completion.files.is_empty());
    index
}

pub struct GetIndex;

impl Command for GetIndex {
    type Params = GetParams;
    type Output = IndexList;
    const ID: &'static str = "plugins.index.get";
    const TITLE: &'static str = "Project Index";
    const DESCRIPTION: &'static str = "The project index of the plug-ins that declare one (the GitOps plug-in's Flux and \
         Argo CD objects, for example): `nodes` (id, kind, group, name, namespace, path, start and end byte offsets, \
         detail), `edges` (from, to, kind), `problems` and, with `references`, the spans that name a node. Narrowed by \
         `plugin`, `kinds` and `path`; `indexing` says an index is being built (`waitMs` waits for it), `truncated` that \
         the project passed the limits. Read-only: built from the project's files, nothing runs.";

    fn run(&self, session: &mut Session, params: GetParams) -> Result<IndexList, CommandError> {
        let deadline = Instant::now() + Duration::from_millis(params.wait_ms.min(30_000));
        poll(session, Instant::now());
        while session.state::<Indexes>().is_some_and(Indexes::indexing) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
            poll(session, Instant::now());
        }
        let path = params.path.as_deref().map(|path| project_relative(session, path));
        let indexes = session.state_mut::<Indexes>();
        let indexing = indexes.indexing();
        if let Some(plugin) = &params.plugin
            && !indexes.entries.contains_key(plugin)
        {
            return Err(CommandError::new(t!("workbench-index-unknown", plugin = plugin.as_str())));
        }
        let built: Vec<(String, Arc<Built>, bool)> = indexes
            .entries
            .iter()
            .filter(|(id, _)| params.plugin.as_ref().is_none_or(|plugin| plugin == *id))
            .map(|(id, entry)| {
                let busy = entry.running.is_some() || entry.stale;
                (id.clone(), entry.built.clone().unwrap_or_default(), busy)
            })
            .collect();
        let mut answers = Vec::new();
        for (plugin, built, busy) in built {
            let mut index = visible(session, built.index.clone());
            if !params.kinds.is_empty() {
                index.nodes.retain(|node| params.kinds.contains(&node.kind));
            }
            if let Some(path) = &path {
                index.nodes.retain(|node| &node.path == path);
                index.problems.retain(|problem| &problem.path == path);
                index.references.retain(|reference| &reference.path == path);
            }
            if !params.kinds.is_empty() || path.is_some() {
                let ids: std::collections::BTreeSet<String> = index.nodes.iter().map(|node| node.id.clone()).collect();
                index.edges.retain(|edge| ids.contains(&edge.from) || ids.contains(&edge.to));
            }
            if !params.references {
                index.references.clear();
            }
            answers.push(IndexAnswer { plugin, indexing: busy, files: built.files, index });
        }
        Ok(IndexList { indexes: answers, indexing })
    }
}

/// The parameters of `plugins.index.refresh`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RefreshParams {
    /// The plug-in whose index to build again; every one when left out.
    pub plugin: Option<String>,
}

pub struct RefreshIndex;

impl Command for RefreshIndex {
    type Params = RefreshParams;
    type Output = IndexList;
    const ID: &'static str = "plugins.index.refresh";
    const TITLE: &'static str = "Refresh Project Index";
    const DESCRIPTION: &'static str = "Builds the project index again now (all plug-ins', or `plugin`'s), in the \
         background; `plugins.index.get` with `waitMs` gives the result.";

    fn run(&self, session: &mut Session, params: RefreshParams) -> Result<IndexList, CommandError> {
        poll(session, Instant::now());
        let indexes = session.state_mut::<Indexes>();
        indexes.changed_at = None;
        for (id, entry) in indexes.entries.iter_mut() {
            if params.plugin.as_ref().is_none_or(|plugin| plugin == id) {
                entry.stale = true;
                entry.running = None;
            }
        }
        poll(session, Instant::now());
        let indexing = session.state::<Indexes>().is_some_and(Indexes::indexing);
        Ok(IndexList { indexes: Vec::new(), indexing })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetIndex)?;
    registry.register(RefreshIndex)?;
    Ok(())
}

mod editor;

pub(crate) use editor::{complete, completes_in, find_at, go_to, problems};

#[cfg(test)]
mod tests;

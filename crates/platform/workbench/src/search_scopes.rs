use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_index::FileFilter;
use zen_index::scope::{Pattern, ScopeError, ScopeSet};

use crate::Workbench;
use crate::local_history::{LocalHistory, VersionKind};
use crate::project_model::ProjectModels;
use crate::project_settings::ProjectSettingsState;
use zen_project::settings::Scope;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListScopes)?;
    registry.register(SelectScope)?;
    registry.register(ScopeFiles)?;
    registry.register(FilterPaths)?;
    edit::register(registry)?;
    Ok(())
}

pub const ALL_PLACES: &str = "allPlaces";
pub const PROJECT_FILES: &str = "projectFiles";
pub const PROJECT_AND_LIBRARIES: &str = "projectAndLibraries";
pub const NOT_IGNORED: &str = "projectNotIgnored";
pub const GIT_TRACKED: &str = "gitTracked";
pub const SCRATCHES: &str = "scratches";
pub const RECENTLY_VIEWED: &str = "recentlyViewed";
pub const RECENTLY_CHANGED: &str = "recentlyChanged";
pub const CURRENT_FILE: &str = "currentFile";
pub const ALL_CHANGED: &str = "allChanged";
pub const PRODUCTION: &str = "production";
pub const TESTS: &str = "tests";
pub const MODULE_PREFIX: &str = "module:";
pub const NAMED_PREFIX: &str = "named:";

pub const TEST_FILES: &[&str] = &["*_test.go", "*.test.*", "*.spec.*", "test_*.py", "*_test.py"];

const BEFORE_MODULES: &[&str] = &[
    ALL_PLACES,
    PROJECT_FILES,
    PROJECT_AND_LIBRARIES,
    NOT_IGNORED,
    GIT_TRACKED,
    SCRATCHES,
    RECENTLY_VIEWED,
    RECENTLY_CHANGED,
];

const AFTER_MODULES: &[&str] = &[CURRENT_FILE, ALL_CHANGED, PRODUCTION, TESTS];

fn predefined_name(id: &str) -> String {
    match id {
        ALL_PLACES => t!("workbench-scope-all-places"),
        PROJECT_FILES => t!("workbench-scope-project-files"),
        PROJECT_AND_LIBRARIES => t!("workbench-scope-project-and-libraries"),
        NOT_IGNORED => t!("workbench-scope-not-ignored"),
        GIT_TRACKED => t!("workbench-scope-git-tracked"),
        SCRATCHES => t!("workbench-scope-scratches"),
        RECENTLY_VIEWED => t!("workbench-scope-recently-viewed"),
        RECENTLY_CHANGED => t!("workbench-scope-recently-changed"),
        CURRENT_FILE => t!("workbench-scope-current-file"),
        ALL_CHANGED => t!("workbench-scope-all-changed"),
        PRODUCTION => t!("workbench-scope-production"),
        _ => t!("workbench-scope-tests"),
    }
}

/// Which part of the list a scope belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ScopeGroup {
    /// A scope every project has.
    Predefined,
    /// A module's files.
    Module,
    /// A named scope the project keeps.
    Named,
}

/// One scope offered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopeInfo {
    /// The id `search.text.start` takes as `scope`.
    pub id: String,
    /// The name shown.
    pub name: String,
    /// Which part of the list it is in.
    pub group: ScopeGroup,
    /// The pattern of a named scope, as written.
    pub pattern: Option<String>,
    /// Why a named scope's pattern cannot be used, if it cannot.
    pub error: Option<String>,
    /// Whether it is one of the project's own named scopes (`.zen/scopes.json`), which can be changed.
    pub own: bool,
}

/// One module offered by the Module area.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleScope {
    /// The module's name.
    pub name: String,
    /// Its folder, relative to the project (empty for the project's folder).
    pub folder: String,
}

/// The area of Find in Files' scope row that is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ScopeArea {
    /// In Project.
    #[default]
    Project,
    /// Module, with the module chosen.
    Module,
    /// Directory, with the folder and whether its subfolders count.
    Directory,
    /// Scope, with the scope chosen.
    Scope,
}

/// Find in Files' scope row: the area chosen and what each area holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ScopeSelection {
    /// The area chosen.
    pub area: ScopeArea,
    /// The module of the Module area.
    pub module: Option<String>,
    /// The folder of the Directory area, relative to the project.
    pub directory: String,
    /// Whether the Directory area searches the subfolders too.
    pub recursive: bool,
    /// The scope id of the Scope area.
    pub scope: String,
    /// The scope id the tabs of Search Everywhere search in.
    pub everywhere: String,
}

impl Default for ScopeSelection {
    fn default() -> Self {
        Self {
            area: ScopeArea::Project,
            module: None,
            directory: String::new(),
            recursive: true,
            scope: PROJECT_FILES.to_owned(),
            everywhere: PROJECT_FILES.to_owned(),
        }
    }
}

impl ScopeSelection {
    pub fn search_params(&self) -> serde_json::Value {
        match self.area {
            ScopeArea::Project => serde_json::json!({ "scope": PROJECT_FILES, "directory": "", "recursive": true }),
            ScopeArea::Module => serde_json::json!({
                "scope": self.module.as_ref().map_or_else(|| PROJECT_FILES.to_owned(), |name| format!("{MODULE_PREFIX}{name}")),
                "directory": "",
                "recursive": true,
            }),
            ScopeArea::Directory => serde_json::json!({
                "scope": PROJECT_FILES,
                "directory": self.directory,
                "recursive": self.recursive,
            }),
            ScopeArea::Scope => serde_json::json!({ "scope": self.scope, "directory": "", "recursive": true }),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SearchScopes {
    selection: ScopeSelection,
    scratches: Option<PathBuf>,
    recent: Option<(String, Instant, ResolvedScope)>,
}

impl SearchScopes {
    pub fn selection(&self) -> &ScopeSelection {
        &self.selection
    }

    pub fn set_scratches(&mut self, folder: PathBuf) {
        self.scratches = Some(folder);
    }
}

#[derive(Clone, Debug)]
pub struct ResolvedScope {
    pub filter: FileFilter,
    pub outside: Vec<(PathBuf, String)>,
}

impl ResolvedScope {
    pub fn extra_files(&self) -> Vec<(PathBuf, String)> {
        self.outside
            .iter()
            .flat_map(|(folder, shown)| {
                zen_index::list_files(folder)
                    .into_iter()
                    .map(move |(path, relative)| (path, format!("{shown}/{relative}")))
            })
            .collect()
    }

    pub fn contains(&self, root: Option<&Path>, path: &Path) -> bool {
        match root.and_then(|root| path.strip_prefix(root).ok()) {
            Some(relative) => self.filter.accepts(&slashed(relative)),
            None => self.outside.iter().any(|(folder, _)| path.starts_with(folder)),
        }
    }
}

fn slashed(relative: &Path) -> String {
    relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

fn root_of(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

pub fn modules(session: &Session) -> Vec<ModuleScope> {
    let Some(root) = root_of(session) else { return Vec::new() };
    let found: Vec<ModuleScope> = session
        .state::<ProjectModels>()
        .map(|models| {
            models
                .model()
                .modules
                .iter()
                .filter_map(|module| {
                    let folder = module.root.strip_prefix(&root).ok()?;
                    Some(ModuleScope { name: module.name.clone(), folder: slashed(folder) })
                })
                .collect()
        })
        .unwrap_or_default();
    if !found.is_empty() {
        return found;
    }
    let name = root.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    vec![ModuleScope { name, folder: String::new() }]
}

fn scope_set(session: &Session) -> ScopeSet {
    let modules = modules(session).into_iter().map(|module| (module.name, module.folder)).collect();
    let mut set = ScopeSet::new(modules);
    for (scope, _) in named_scopes(session) {
        set.add(&scope.name, &scope.pattern);
    }
    set
}

pub fn named_scopes(session: &Session) -> Vec<(Scope, bool)> {
    let own = root_of(session).map(|root| own_scopes(&root)).unwrap_or_default();
    let mut all: Vec<(Scope, bool)> = session
        .state::<ProjectSettingsState>()
        .map(|state| state.settings().scopes.clone())
        .unwrap_or_default()
        .into_iter()
        .filter(|scope| !own.iter().any(|mine| mine.name == scope.name))
        .map(|scope| (scope, false))
        .collect();
    all.extend(own.into_iter().map(|scope| (scope, true)));
    all
}

pub fn error_message(error: &ScopeError) -> String {
    match error {
        ScopeError::Empty => t!("workbench-scope-error-empty"),
        ScopeError::Unexpected(at) => t!("workbench-scope-error-unexpected", column = at + 1),
        ScopeError::Unsupported(kind) => t!("workbench-scope-error-unsupported", kind = kind.clone()),
        ScopeError::UnknownScope(name) => t!("workbench-scope-error-unknown", name = name.clone()),
        ScopeError::Cycle(name) => t!("workbench-scope-error-cycle", name = name.clone()),
    }
}

pub fn scopes(session: &Session, current_file: bool) -> Vec<ScopeInfo> {
    let predefined = |id: &&str| ScopeInfo {
        id: (*id).to_owned(),
        name: predefined_name(id),
        group: ScopeGroup::Predefined,
        pattern: None,
        error: None,
        own: false,
    };
    let mut list: Vec<ScopeInfo> = BEFORE_MODULES.iter().map(predefined).collect();
    list.extend(modules(session).into_iter().map(|module| ScopeInfo {
        id: format!("{MODULE_PREFIX}{}", module.name),
        name: t!("workbench-scope-module", name = module.name),
        group: ScopeGroup::Module,
        pattern: None,
        error: None,
        own: false,
    }));
    list.extend(AFTER_MODULES.iter().filter(|id| current_file || **id != CURRENT_FILE).map(predefined));
    let set = scope_set(session);
    for (scope, own) in named_scopes(session) {
        let error = Pattern::parse(&scope.pattern).and_then(|pattern| set.check(&pattern)).err();
        list.push(ScopeInfo {
            id: format!("{NAMED_PREFIX}{}", scope.name),
            name: scope.name,
            group: ScopeGroup::Named,
            pattern: Some(scope.pattern),
            error: error.as_ref().map(error_message),
            own,
        });
    }
    list
}

pub(crate) fn is_test(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or(relative).to_lowercase();
    TEST_FILES.iter().any(|pattern| zen_index::scope::glob(pattern, &name))
}

fn relative_set(root: &Path, paths: impl IntoIterator<Item = PathBuf>) -> HashSet<String> {
    paths.into_iter().filter_map(|path| path.strip_prefix(root).ok().map(slashed)).collect()
}

fn only(files: HashSet<String>) -> FileFilter {
    FileFilter::new(move |relative| files.contains(relative))
}

fn libraries(session: &Session) -> Vec<(PathBuf, String)> {
    session
        .state::<ProjectModels>()
        .map(|models| {
            models
                .model()
                .libraries
                .iter()
                .filter(|library| library.exists)
                .map(|library| (library.root.clone(), library.name.clone()))
                .collect()
        })
        .unwrap_or_default()
}

fn scratches(session: &Session) -> Vec<(PathBuf, String)> {
    let folder = session.state::<SearchScopes>().and_then(|scopes| scopes.scratches.clone());
    let buffers = session.state::<crate::scratch::Scratches>().and_then(crate::scratch::Scratches::buffers_folder);
    let title = t!("workbench-scope-scratches");
    let buffers = buffers.map(|buffers| (buffers, format!("{title}/{}", t!("workbench-scratch-buffers"))));
    folder.map(|folder| (folder, title)).into_iter().chain(buffers).filter(|(folder, _)| folder.is_dir()).collect()
}

struct GitPaths {
    ignored: HashSet<String>,
    unversioned: HashSet<String>,
    changed: HashSet<String>,
}

fn git_paths(session: &mut Session, root: &Path) -> Option<GitPaths> {
    crate::vcs::repository(session).ok()?;
    let report = crate::vcs::StatusList.run(session, zen_core::NoParams::default()).ok()?;
    let mut paths = GitPaths {
        ignored: report.ignored.iter().filter_map(|path| path.strip_prefix(root).ok().map(slashed)).collect(),
        unversioned: HashSet::new(),
        changed: HashSet::new(),
    };
    for change in report.changes {
        let Ok(relative) = change.path.strip_prefix(root) else { continue };
        let relative = slashed(relative);
        if change.status == "unversioned" {
            paths.unversioned.insert(relative);
        } else {
            paths.changed.insert(relative);
        }
    }
    Some(paths)
}

fn under(paths: &HashSet<String>, relative: &str) -> bool {
    !paths.is_empty()
        && (paths.contains(relative) || relative.match_indices('/').any(|(at, _)| paths.contains(&relative[..at])))
}

fn recently_changed(session: &Session, root: &Path) -> HashSet<String> {
    let mut files: HashSet<String> = session
        .state::<Workbench>()
        .map(|workbench| {
            relative_set(
                root,
                workbench
                    .documents()
                    .iter()
                    .filter(|document| document.is_dirty())
                    .map(|document| document.path().to_owned()),
            )
        })
        .unwrap_or_default();
    if let Some(history) = session.state::<LocalHistory>().filter(|history| history.is_enabled()) {
        let project = root.to_owned();
        let saved = history.query(move |store| {
            store
                .entries(&project)
                .iter()
                .filter(|entry| matches!(entry.kind, VersionKind::Save | VersionKind::Action))
                .filter_map(|entry| entry.path.clone())
                .collect::<Vec<_>>()
        });
        files.extend(saved.unwrap_or_default());
    }
    files
}

pub fn resolve(session: &mut Session, id: &str) -> Result<ResolvedScope, CommandError> {
    let root = root_of(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let inside = |filter| Ok(ResolvedScope { filter, outside: Vec::new() });
    match id {
        PROJECT_FILES => inside(FileFilter::all()),
        ALL_PLACES => {
            let mut outside = libraries(session);
            outside.extend(scratches(session));
            Ok(ResolvedScope { filter: FileFilter::all(), outside })
        }
        PROJECT_AND_LIBRARIES => Ok(ResolvedScope { filter: FileFilter::all(), outside: libraries(session) }),
        NOT_IGNORED => {
            let paths = git_paths(session, &root).map(|paths| paths.ignored).unwrap_or_default();
            inside(FileFilter::new(move |relative| !under(&paths, relative)))
        }
        GIT_TRACKED => match git_paths(session, &root) {
            Some(paths) => inside(FileFilter::new(move |relative| {
                !under(&paths.unversioned, relative) && !under(&paths.ignored, relative)
            })),
            None => inside(FileFilter::new(|_| false)),
        },
        SCRATCHES => Ok(ResolvedScope { filter: FileFilter::new(|_| false), outside: scratches(session) }),
        RECENTLY_VIEWED => {
            let recent =
                session.state::<Workbench>().map(|workbench| workbench.recent_files().to_vec()).unwrap_or_default();
            inside(only(relative_set(&root, recent)))
        }
        RECENTLY_CHANGED => inside(only(recently_changed(session, &root))),
        CURRENT_FILE => {
            let active = session
                .state::<Workbench>()
                .and_then(Workbench::active_document)
                .map(|document| document.path().to_owned());
            inside(only(relative_set(&root, active)))
        }
        ALL_CHANGED => inside(only(git_paths(session, &root).map(|paths| paths.changed).unwrap_or_default())),
        PRODUCTION => inside(FileFilter::new(|relative| !is_test(relative))),
        TESTS => inside(FileFilter::new(is_test)),
        _ => {
            if let Some(name) = id.strip_prefix(MODULE_PREFIX) {
                let module = modules(session)
                    .into_iter()
                    .find(|module| module.name == name)
                    .ok_or_else(|| CommandError::new(t!("workbench-scope-unknown", id = id.to_owned())))?;
                let folder = module.folder;
                return inside(FileFilter::new(move |relative| {
                    folder.is_empty()
                        || relative.strip_prefix(folder.as_str()).is_some_and(|rest| rest.starts_with('/'))
                }));
            }
            if let Some(name) = id.strip_prefix(NAMED_PREFIX) {
                let set = scope_set(session);
                let pattern = match set.named(name) {
                    Some(pattern) => pattern.clone().map_err(|error| CommandError::new(error_message(&error)))?,
                    None => return Err(CommandError::new(t!("workbench-scope-unknown", id = id.to_owned()))),
                };
                return pattern_scope(set, pattern);
            }
            Err(CommandError::new(t!("workbench-scope-unknown", id = id.to_owned())))
        }
    }
}

fn pattern_scope(set: ScopeSet, pattern: Pattern) -> Result<ResolvedScope, CommandError> {
    set.check(&pattern).map_err(|error| CommandError::new(error_message(&error)))?;
    Ok(ResolvedScope { filter: FileFilter::new(move |relative| set.matches(&pattern, relative)), outside: Vec::new() })
}

pub fn resolve_pattern(session: &Session, pattern: &str) -> Result<ResolvedScope, CommandError> {
    let parsed = Pattern::parse(pattern).map_err(|error| CommandError::new(error_message(&error)))?;
    pattern_scope(scope_set(session), parsed)
}

pub fn resolve_in(
    session: &mut Session,
    id: Option<&str>,
    directory: &str,
    recursive: bool,
) -> Result<ResolvedScope, CommandError> {
    let directory = project_folder(session.state::<Workbench>(), directory)?;
    let scope = resolve(session, id.filter(|id| !id.is_empty()).unwrap_or(PROJECT_FILES))?;
    let mut scope = narrowed(scope, &directory, recursive);
    if let Some(patterns) = crate::hidden_files::for_caller(session) {
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf);
        let shown = FileFilter::new(move |relative| {
            !crate::hidden_files::is_hidden(&patterns, root.as_deref(), Path::new(relative))
        });
        scope.filter = scope.filter.and(shown);
    }
    Ok(scope)
}

fn project_folder(workbench: Option<&Workbench>, directory: &str) -> Result<String, CommandError> {
    let given = directory.trim();
    let Some(root) = workbench.and_then(Workbench::project).map(Path::to_path_buf) else {
        return Ok(given.trim_matches('/').to_owned());
    };
    if given.is_empty() {
        return Ok(String::new());
    }
    let path = Path::new(given);
    let relative = if path.is_absolute() {
        let spelled = workbench.map_or_else(|| path.to_path_buf(), |workbench| workbench.spelled(path));
        match spelled.strip_prefix(&root) {
            Ok(inside) => inside.to_path_buf(),
            Err(_) => {
                let (directory, project) = (given.to_owned(), root.display().to_string());
                return Err(CommandError::new(t!(
                    "workbench-scope-directory-outside",
                    directory = directory,
                    project = project
                )));
            }
        }
    } else {
        path.to_path_buf()
    };
    let parts: Vec<String> = relative
        .components()
        .filter(|part| !matches!(part, std::path::Component::CurDir))
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.iter().any(|part| part == "..") {
        let (directory, project) = (given.to_owned(), root.display().to_string());
        return Err(CommandError::new(t!(
            "workbench-scope-directory-outside",
            directory = directory,
            project = project
        )));
    }
    let relative = parts.join("/");
    if !root.join(&relative).is_dir() {
        let (directory, project) = (given.to_owned(), root.display().to_string());
        return Err(CommandError::new(t!(
            "workbench-scope-directory-missing",
            directory = directory,
            project = project
        )));
    }
    Ok(relative)
}

fn narrowed(scope: ResolvedScope, directory: &str, recursive: bool) -> ResolvedScope {
    let directory = directory.trim_matches('/').to_owned();
    if directory.is_empty() && recursive {
        return scope;
    }
    let folder = FileFilter::new(move |relative| {
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        if recursive {
            directory.is_empty() || parent == directory || parent.starts_with(&format!("{directory}/"))
        } else {
            parent == directory
        }
    });
    ResolvedScope { filter: scope.filter.and(folder), outside: Vec::new() }
}

/// What `search.scope.list` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// The list Search Everywhere offers, without Current File.
    pub everywhere: bool,
}

/// What `search.scope.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopeList {
    /// The scopes, in the order the lists show them.
    pub scopes: Vec<ScopeInfo>,
    /// The modules the Module area offers.
    pub modules: Vec<ModuleScope>,
    /// Find in Files' scope row as last chosen.
    pub selection: ScopeSelection,
}

pub struct ListScopes;

impl Command for ListScopes {
    type Params = ListParams;
    type Output = ScopeList;
    const ID: &'static str = "search.scope.list";
    const TITLE: &'static str = "Search Scopes";
    const DESCRIPTION: &'static str = "Lists the scopes Find in Files and Search Everywhere search in: the predefined ones \
         (All Places, Project Files, Project and Libraries, Git-ignored and Git-tracked, Scratches and Consoles, Recently \
         Viewed and Changed, Current File, All Changed Files, Production and Test files), one per module and the \
         project's named scopes with their patterns; with the modules and Find in Files' scope row as last chosen. \
         `everywhere` gives Search Everywhere's list, without Current File.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ScopeList, CommandError> {
        Ok(ScopeList {
            scopes: scopes(session, !params.everywhere),
            modules: modules(session),
            selection: session.state::<SearchScopes>().map(|scopes| scopes.selection.clone()).unwrap_or_default(),
        })
    }
}

pub struct SelectScope;

impl Command for SelectScope {
    type Params = ScopeSelection;
    type Output = ();
    const ID: &'static str = "search.scope.select";
    const TITLE: &'static str = "Select Search Scope";
    const DESCRIPTION: &'static str = "Sets Find in Files' scope row: the area (`project`, `module`, `directory` or \
         `scope`) with the module, the folder and whether its subfolders count, and the scope id from \
         search.scope.list; and `everywhere`, the scope of Search Everywhere's tabs. The window remembers it with the \
         project.";

    fn run(&self, session: &mut Session, params: ScopeSelection) -> Result<(), CommandError> {
        session.state_mut::<SearchScopes>().selection = params;
        Ok(())
    }
}

/// What `search.scope.files` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FilesParams {
    /// A scope id from search.scope.list; Project Files when absent.
    pub scope: Option<String>,
    /// A pattern in the scope language instead, e.g. `file:internal//*&&!file:*_test.go`.
    pub pattern: Option<String>,
    /// How many files to list at most; 1000 when absent.
    pub limit: Option<usize>,
}

/// What `search.scope.files` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilesInScope {
    /// The files, relative to the project (outside it, under the library's name), sorted.
    pub files: Vec<String>,
    /// How many files the scope holds.
    pub total: usize,
}

pub struct ScopeFiles;

impl Command for ScopeFiles {
    type Params = FilesParams;
    type Output = FilesInScope;
    const ID: &'static str = "search.scope.files";
    const TITLE: &'static str = "Files in Scope";
    const DESCRIPTION: &'static str = "Lists the files a scope from search.scope.list holds, or the files a pattern in the \
         scope language matches (`file:` patterns with `*` and `//*`, `&&`, `||`, `!`, parentheses and `$Name` for a \
         named scope); an unusable pattern is an error that says why.";

    fn run(&self, session: &mut Session, params: FilesParams) -> Result<FilesInScope, CommandError> {
        let scope = match params.pattern.as_deref() {
            Some(pattern) => resolve_pattern(session, pattern)?,
            None => resolve(session, params.scope.as_deref().unwrap_or(PROJECT_FILES))?,
        };
        let index = session.state::<Workbench>().and_then(|workbench| workbench.index.clone());
        let mut files: Vec<String> = index
            .map(|index| index.file_list())
            .unwrap_or_default()
            .into_iter()
            .map(|(_, relative)| relative)
            .filter(|relative| scope.filter.accepts(relative))
            .chain(scope.extra_files().into_iter().map(|(_, shown)| shown))
            .collect();
        files.sort();
        let total = files.len();
        files.truncate(params.limit.unwrap_or(1000));
        Ok(FilesInScope { files, total })
    }
}

/// What `search.scope.filter` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterParams {
    /// A scope id from search.scope.list; Project Files when absent.
    pub scope: Option<String>,
    /// Absolute paths of files.
    pub paths: Vec<PathBuf>,
}

pub struct FilterPaths;

impl Command for FilterPaths {
    type Params = FilterParams;
    type Output = Vec<PathBuf>;
    const ID: &'static str = "search.scope.filter";
    const TITLE: &'static str = "Filter by Search Scope";
    const DESCRIPTION: &'static str = "Answers which of the given files (absolute paths) the scope from search.scope.list \
         holds, in their order; Search Everywhere keeps its results to its scope with it.";

    fn run(&self, session: &mut Session, params: FilterParams) -> Result<Vec<PathBuf>, CommandError> {
        let id = params.scope.filter(|id| !id.is_empty()).unwrap_or_else(|| PROJECT_FILES.to_owned());
        let fresh = session
            .state::<SearchScopes>()
            .and_then(|scopes| scopes.recent.clone())
            .filter(|(recent, at, _)| *recent == id && at.elapsed() < Duration::from_secs(1));
        let scope = match fresh {
            Some((_, _, scope)) => scope,
            None => {
                let scope = resolve(session, &id)?;
                session.state_mut::<SearchScopes>().recent = Some((id, Instant::now(), scope.clone()));
                scope
            }
        };
        let root = root_of(session);
        Ok(params.paths.into_iter().filter(|path| scope.contains(root.as_deref(), path)).collect())
    }
}

mod edit;
pub use edit::{DeleteParams, DeleteScope, OWN_SCOPES, SaveParams, SaveScope, own_scopes};

#[cfg(test)]
mod tests;

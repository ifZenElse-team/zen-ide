use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::model::{Library, ModelEnv, ModelProblem, ModelProvider, Module, ProjectModel, WorkspaceFile};

use crate::Workbench;
use crate::language::{Problem, Severity, char_offset};

#[derive(Default)]
pub struct ProjectModels {
    providers: Vec<Arc<dyn ModelProvider>>,
    env: ModelEnv,
    model: ProjectModel,
    revision: u64,
    built_for: Option<Option<PathBuf>>,
    generations: Vec<u64>,
    read: BTreeMap<PathBuf, String>,
}

pub const MODEL_SOURCE: &str = "project model";

impl ProjectModels {
    pub fn set_providers(&mut self, providers: Vec<Arc<dyn ModelProvider>>, env: ModelEnv) {
        self.providers = providers;
        self.env = env;
        self.built_for = None;
    }

    pub fn set_env(&mut self, env: ModelEnv) {
        if env != self.env {
            self.env = env;
            self.built_for = None;
        }
    }

    pub fn providers(&self) -> &[Arc<dyn ModelProvider>] {
        &self.providers
    }

    pub fn env(&self) -> &ModelEnv {
        &self.env
    }

    pub fn model(&self) -> &ProjectModel {
        &self.model
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn watches(&self, path: &Path) -> bool {
        let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else { return false };
        self.providers.iter().any(|provider| provider.watches(&name))
    }

    pub fn is_built_for(&self, root: Option<&Path>) -> bool {
        self.built_for.as_ref().is_some_and(|built| built.as_deref() == root)
    }

    pub fn ensure(&mut self, root: Option<&Path>) -> bool {
        let moved = root.is_some_and(|root| self.generations_of(root) != self.generations);
        (!self.is_built_for(root) || moved) && self.refresh(root)
    }

    fn generations_of(&self, root: &Path) -> Vec<u64> {
        self.providers.iter().map(|provider| provider.generation(root)).collect()
    }

    pub fn refresh_after(&mut self, root: Option<&Path>, changed: Option<&[PathBuf]>) -> bool {
        let wanted = changed.is_none_or(|paths| paths.iter().any(|path| self.watches(path)));
        wanted && self.refresh(root)
    }

    pub fn refresh(&mut self, root: Option<&Path>) -> bool {
        self.generations = root.map(|root| self.generations_of(root)).unwrap_or_default();
        let mut model = ProjectModel::default();
        if let Some(root) = root {
            let safe_mode = !crate::trust::is_trusted(&crate::build_tools::BuildEnv::global(), root);
            for provider in self.providers.iter().filter(|provider| !(safe_mode && provider.runs_programs())) {
                let Some(part) = provider.load(root, &self.env) else { continue };
                model.modules.extend(part.modules);
                model.workspace = model.workspace.or(part.workspace);
                model.libraries.extend(part.libraries);
                model.problems.extend(part.problems);
            }
        }
        if let Some(root) = root {
            within(&mut model.problems, root);
        }
        self.read = model
            .problems
            .iter()
            .map(|problem| (problem.file.clone(), std::fs::read_to_string(&problem.file).unwrap_or_default()))
            .collect();
        self.built_for = Some(root.map(Path::to_owned));
        self.replace(model)
    }

    pub fn problem_files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self.model.problems.iter().map(|problem| problem.file.clone()).collect();
        files.sort();
        files.dedup();
        files
    }

    pub fn checks(&self, path: &Path) -> bool {
        let model = &self.model;
        self.built_for.as_ref().is_some_and(Option::is_some)
            && (model.modules.iter().any(|module| module.file == path)
                || model.workspace.as_ref().is_some_and(|workspace| workspace.file == path)
                || model.problems.iter().any(|problem| problem.file == path))
    }

    pub fn recheck_libraries(&mut self) -> bool {
        if self.model.libraries.iter().all(|library| library.exists == library.root.is_dir()) {
            return false;
        }
        for library in &mut self.model.libraries {
            library.exists = library.root.is_dir();
        }
        self.revision += 1;
        true
    }

    fn replace(&mut self, model: ProjectModel) -> bool {
        if model == self.model {
            return false;
        }
        self.model = model;
        self.revision += 1;
        true
    }
}

fn within(problems: &mut [ModelProblem], root: &Path) {
    let Ok(real) = root.canonicalize() else { return };
    if real == root {
        return;
    }
    for problem in problems {
        if let Ok(rest) = problem.file.strip_prefix(&real) {
            problem.file = root.join(rest);
        }
    }
}

pub(crate) fn problems_of(session: &Session, path: &Path) -> Vec<Problem> {
    let Some(models) = session.state::<ProjectModels>() else { return Vec::new() };
    let found: Vec<&ModelProblem> = models.model.problems.iter().filter(|problem| problem.file == path).collect();
    if found.is_empty() {
        return Vec::new();
    }
    let read = models.read.get(path).map_or("", String::as_str);
    let document = session.state::<Workbench>().and_then(|workbench| {
        workbench.documents().iter().find(|document| document.path() == path).map(|document| document.text().to_owned())
    });
    let text = document.as_deref().unwrap_or(read);
    let hunks = if text == read { Vec::new() } else { zen_vcs::diff::hunks(read, text) };
    let last = text.split('\n').count() - 1;
    found
        .into_iter()
        .map(|problem| {
            let index = moved(&hunks, problem.line.max(1) as usize - 1).min(last);
            as_problem(text, index, problem)
        })
        .collect()
}

fn moved(hunks: &[zen_vcs::diff::Hunk], line: usize) -> usize {
    let mut shift = 0isize;
    for hunk in hunks {
        if line < hunk.before.start {
            break;
        }
        if line < hunk.before.end {
            let within = (line - hunk.before.start).min(hunk.after.len().saturating_sub(1));
            return hunk.after.start + within;
        }
        shift += hunk.after.len() as isize - hunk.before.len() as isize;
    }
    line.saturating_add_signed(shift)
}

fn as_problem(text: &str, index: usize, problem: &ModelProblem) -> Problem {
    let lines: Vec<&str> = text.split('\n').collect();
    let line_start: usize = lines[..index].iter().map(|line| line.len() + 1).sum();
    let line = lines[index].trim_end();
    let indent = line.len() - line.trim_start().len();
    Problem {
        start: char_offset(text, line_start + indent),
        end: char_offset(text, line_start + line.len()),
        line: index as u32 + 1,
        column: line[..indent].chars().count() as u32 + 1,
        severity: if problem.warning { Severity::Warning } else { Severity::Error },
        message: problem.message.clone(),
        source: Some(MODEL_SOURCE.to_owned()),
        code: None,
        note: None,
        rows: Vec::new(),
        details: None,
        fixes: Vec::new(),
        deprecated: false,
        unused: false,
    }
}

fn project_root(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

/// The parameters of `project.model.refresh`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct RefreshParams {
    /// The files that changed; the model is rebuilt only when a provider watches one. All when absent.
    #[serde(default)]
    pub paths: Option<Vec<PathBuf>>,
}

/// What `project.model.refresh` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefreshAnswer {
    /// The model's revision after the refresh.
    pub revision: u64,
    /// Whether the model changed.
    pub changed: bool,
}

pub struct RefreshModel;

impl Command for RefreshModel {
    type Params = RefreshParams;
    type Output = RefreshAnswer;
    const ID: &'static str = "project.model.refresh";
    const TITLE: &'static str = "Reload Project Model";
    const DESCRIPTION: &'static str =
        "Rereads the project's module files (go.mod, go.work...) when one of the given files changed, or always.";

    fn run(&self, session: &mut Session, params: RefreshParams) -> Result<RefreshAnswer, CommandError> {
        let root = project_root(session);
        let models = session.state_mut::<ProjectModels>();
        let changed = models.refresh_after(root.as_deref(), params.paths.as_deref());
        Ok(RefreshAnswer { revision: models.revision(), changed })
    }
}

/// Which modules `project.module.info` gives in full.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ModuleInfoParams {
    /// A module by its name or its folder (relative to the project or absolute): it alone in full, with its
    /// dependencies, folders and details.
    pub module: Option<String>,
    /// Every module in full; on a large workspace this is long.
    pub all: bool,
}

/// What `project.module.info` answers.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleInfo {
    /// How many modules the project has.
    pub count: usize,
    /// Every module as `name folder`, the folder relative to the project when inside it; `module` or
    /// `all` gives them in full.
    pub summary: Vec<String>,
    /// The names of the module files, `Cargo.toml` or `go.mod`.
    pub files: Vec<String>,
    /// The modules asked for in full: `module`'s, or every one with `all`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<Module>,
    /// The workspace file, when there is one; with neither `module` nor `all`, without its members, replacements
    /// and details.
    pub workspace: Option<WorkspaceFile>,
    /// What could not be read.
    pub problems: Vec<ModelProblem>,
}

pub struct ShowModuleInfo;

impl Command for ShowModuleInfo {
    type Params = ModuleInfoParams;
    type Output = ModuleInfo;
    const ID: &'static str = "project.module.info";
    const TITLE: &'static str = "Module Info";
    const DESCRIPTION: &'static str = "The project's modules in a few words (name, folder, module file), its workspace \
         file and the problems found in them, as last read; `module` gives one module in full (dependencies, folders, \
         details), `all` every one.";

    fn run(&self, session: &mut Session, params: ModuleInfoParams) -> Result<ModuleInfo, CommandError> {
        let root = project_root(session);
        let models = session.state_mut::<ProjectModels>();
        models.ensure(root.as_deref());
        let model = models.model().clone();
        let relative =
            |path: &Path| root.as_deref().and_then(|root| path.strip_prefix(root).ok()).unwrap_or(path).to_owned();
        let summary = model
            .modules
            .iter()
            .map(|module| format!("{} {}", module.name, relative(&module.root).display()))
            .collect();
        let mut files: Vec<String> = model
            .modules
            .iter()
            .filter_map(|module| module.file.file_name().map(|name| name.to_string_lossy().into_owned()))
            .collect();
        files.sort();
        files.dedup();
        let full = params.all || params.module.as_deref().is_some_and(|wanted| !wanted.trim().is_empty());
        let workspace = model.workspace.map(|workspace| {
            if full {
                return workspace;
            }
            WorkspaceFile {
                file: relative(&workspace.file),
                uses: Vec::new(),
                replacements: Vec::new(),
                details: serde_json::Value::Null,
                ..workspace
            }
        });
        let modules = match params.module.as_deref().map(str::trim).filter(|wanted| !wanted.is_empty()) {
            Some(wanted) => {
                let found: Vec<Module> = model
                    .modules
                    .iter()
                    .filter(|module| {
                        module.name == wanted
                            || module.root == Path::new(wanted)
                            || relative(&module.root) == Path::new(wanted)
                    })
                    .cloned()
                    .collect();
                if found.is_empty() {
                    return Err(CommandError::new(t!("workbench-module-unknown", module = wanted)));
                }
                found
            }
            None if params.all => model.modules.clone(),
            None => Vec::new(),
        };
        Ok(ModuleInfo { count: model.modules.len(), summary, files, modules, workspace, problems: model.problems })
    }
}

/// What `project.libraries.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LibrariesAnswer {
    /// The model's revision.
    pub revision: u64,
    /// The SDK and the dependencies, as External Libraries shows them.
    pub libraries: Vec<Library>,
}

pub struct ListLibraries;

impl Command for ListLibraries {
    type Params = NoParams;
    type Output = LibrariesAnswer;
    const ID: &'static str = "project.libraries.list";
    const TITLE: &'static str = "External Libraries";
    const DESCRIPTION: &'static str = "The SDK and the dependencies the project uses, with their folders.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<LibrariesAnswer, CommandError> {
        let root = project_root(session);
        let models = session.state_mut::<ProjectModels>();
        models.ensure(root.as_deref());
        models.recheck_libraries();
        Ok(LibrariesAnswer { revision: models.revision(), libraries: models.model().libraries.clone() })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(RefreshModel)?;
    registry.register(ShowModuleInfo)?;
    registry.register(ListLibraries)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn setup() -> (Registry, Session, tempfile::TempDir) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        (registry, session, dir)
    }

    #[test]
    fn without_providers_the_model_is_empty() {
        let (registry, mut session, _dir) = setup();
        let refreshed = registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        assert_eq!(refreshed, json!({ "revision": 0, "changed": false }));
        let info = registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        assert_eq!(info, json!({ "count": 0, "summary": [], "files": [], "workspace": null, "problems": [] }));
        let libraries = registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(libraries, json!({ "revision": 0, "libraries": [] }));
    }

    struct OneModule;

    impl ModelProvider for OneModule {
        fn id(&self) -> &str {
            "test"
        }

        fn watches(&self, file_name: &str) -> bool {
            file_name == "test.mod"
        }

        fn load(&self, root: &Path, _: &ModelEnv) -> Option<ProjectModel> {
            Some(ProjectModel {
                modules: vec![Module { name: "m".to_owned(), root: root.to_owned(), ..Module::default() }],
                ..ProjectModel::default()
            })
        }
    }

    struct RunsCargo;

    impl ModelProvider for RunsCargo {
        fn id(&self) -> &str {
            "cargo"
        }

        fn watches(&self, _: &str) -> bool {
            false
        }

        fn runs_programs(&self) -> bool {
            true
        }

        fn load(&self, root: &Path, env: &ModelEnv) -> Option<ProjectModel> {
            OneModule.load(root, env)
        }
    }

    #[test]
    fn a_provider_that_runs_programs_waits_for_trust_and_one_that_reads_files_does_not() {
        let (registry, mut session, dir) = setup();
        let modules = |session: &Session| session.state::<ProjectModels>().unwrap().model().modules.len();
        session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(RunsCargo)], ModelEnv::default());
        registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        assert_eq!(modules(&session), 0, "safe mode");
        session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(OneModule)], ModelEnv::default());
        registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        assert_eq!(modules(&session), 1);
        session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(RunsCargo)], ModelEnv::default());
        crate::trust::trust_in_tests(&crate::build_tools::BuildEnv::global(), dir.path());
        registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        assert_eq!(modules(&session), 1, "trusted");
    }

    #[test]
    fn a_provider_fills_the_model_once() {
        let (registry, mut session, dir) = setup();
        session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(OneModule)], ModelEnv::default());
        let other = json!({ "paths": [dir.path().join("main.go")] });
        assert_eq!(registry.dispatch(&mut session, "project.model.refresh", other).unwrap()["changed"], false);
        let watched = json!({ "paths": [dir.path().join("test.mod")] });
        let refreshed = registry.dispatch(&mut session, "project.model.refresh", watched).unwrap();
        assert_eq!(refreshed, json!({ "revision": 1, "changed": true }));
        assert_eq!(registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap()["changed"], false);
        let info = registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        assert_eq!(info["modules"][0]["name"], "m");
    }

    struct ManyModules;

    impl ModelProvider for ManyModules {
        fn id(&self) -> &str {
            "many"
        }

        fn watches(&self, _: &str) -> bool {
            false
        }

        fn load(&self, root: &Path, _: &ModelEnv) -> Option<ProjectModel> {
            let module = |at: usize| {
                let folder = root.join(format!("crates/part-{at}"));
                let dependencies = (0..40)
                    .map(|dep| zen_project::model::Dependency {
                        name: format!("dependency-{dep}"),
                        version: "1.0.0".to_owned(),
                        ..zen_project::model::Dependency::default()
                    })
                    .collect();
                Module {
                    name: format!("part-{at}"),
                    file: folder.join("Cargo.toml"),
                    root: folder,
                    dependencies,
                    ..Module::default()
                }
            };
            Some(ProjectModel { modules: (0..49).map(module).collect(), ..ProjectModel::default() })
        }
    }

    #[test]
    fn module_info_is_a_short_summary_and_one_module_in_full_on_request() {
        let (registry, mut session, _dir) = setup();
        session.state_mut::<ProjectModels>().set_providers(vec![Arc::new(ManyModules)], ModelEnv::default());
        registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        let summary = registry.dispatch(&mut session, "project.module.info", json!({})).unwrap();
        let size = summary.to_string().len();
        assert!(size < 4 * 1024, "{size} bytes");
        assert_eq!(summary["count"], 49);
        assert_eq!(summary["summary"][3], "part-3 crates/part-3");
        assert_eq!(summary["files"], json!(["Cargo.toml"]));
        assert!(summary.get("modules").is_none());
        let one = registry.dispatch(&mut session, "project.module.info", json!({ "module": "crates/part-3" })).unwrap();
        assert_eq!(one["modules"].as_array().unwrap().len(), 1);
        assert_eq!(one["modules"][0]["dependencies"].as_array().unwrap().len(), 40);
        let by_name = registry.dispatch(&mut session, "project.module.info", json!({ "module": "part-3" })).unwrap();
        assert_eq!(by_name["modules"], one["modules"]);
        let all = registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        assert!(all.to_string().len() > 10 * size, "every module in full is what the summary saves");
        assert!(registry.dispatch(&mut session, "project.module.info", json!({ "module": "none" })).is_err());
    }

    #[derive(Default)]
    struct Fake {
        model: std::sync::Mutex<ProjectModel>,
        loads: std::sync::atomic::AtomicUsize,
        generation: std::sync::atomic::AtomicU64,
    }

    impl Fake {
        fn set(&self, model: ProjectModel) {
            *self.model.lock().unwrap() = model;
        }

        fn loads(&self) -> usize {
            self.loads.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl ModelProvider for Fake {
        fn id(&self) -> &str {
            "fake"
        }

        fn watches(&self, file_name: &str) -> bool {
            file_name == "fake.mod"
        }

        fn generation(&self, _root: &Path) -> u64 {
            self.generation.load(std::sync::atomic::Ordering::SeqCst)
        }

        fn load(&self, root: &Path, env: &ModelEnv) -> Option<ProjectModel> {
            self.loads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut model = self.model.lock().unwrap().clone();
            for module in &mut model.modules {
                module.root = root.to_owned();
            }
            if let Some(version) = &env.sdk_version {
                model.libraries.push(Library {
                    kind: zen_project::model::LibraryKind::Sdk,
                    name: "Fake".to_owned(),
                    version: Some(version.clone()),
                    root: env.sdk_root.clone().unwrap_or_default(),
                    exists: true,
                    group: None,
                    label: None,
                });
            }
            Some(model)
        }
    }

    fn module(name: &str) -> ProjectModel {
        ProjectModel { modules: vec![Module { name: name.to_owned(), ..Module::default() }], ..ProjectModel::default() }
    }

    fn with_fake(session: &mut Session, model: ProjectModel) -> Arc<Fake> {
        let fake = Arc::new(Fake::default());
        fake.set(model);
        session.state_mut::<ProjectModels>().set_providers(vec![fake.clone()], ModelEnv::default());
        fake
    }

    #[test]
    fn the_model_is_read_on_the_first_query() {
        let (registry, mut session, dir) = setup();
        let fake = with_fake(&mut session, module("lazy"));
        assert_eq!(fake.loads(), 0);
        let info = registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        assert_eq!(info["modules"][0]["name"], "lazy");
        assert_eq!(info["modules"][0]["root"], json!(dir.path()));
        registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        let libraries = registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(libraries["revision"], 1);
        assert_eq!(fake.loads(), 1);
    }

    #[test]
    fn a_provider_whose_generation_moves_is_read_again_when_its_libraries_are_asked_for() {
        let (registry, mut session, _dir) = setup();
        let fake = with_fake(&mut session, module("a"));
        registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(fake.loads(), 1);
        fake.set(module("b"));
        fake.generation.store(7, std::sync::atomic::Ordering::SeqCst);
        let libraries = registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(fake.loads(), 2);
        assert_eq!(libraries["revision"], 2);
        registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(fake.loads(), 2, "read once per generation");
    }

    #[test]
    fn only_a_watched_change_rebuilds_and_the_revision_rises_only_on_a_difference() {
        let (registry, mut session, dir) = setup();
        let fake = with_fake(&mut session, module("a"));
        let refresh = |registry: &Registry, session: &mut Session, name: &str| {
            let params = json!({ "paths": [dir.path().join(name)] });
            registry.dispatch(session, "project.model.refresh", params).unwrap()
        };
        assert_eq!(refresh(&registry, &mut session, "fake.mod"), json!({ "revision": 1, "changed": true }));
        assert_eq!(refresh(&registry, &mut session, "fake.mod"), json!({ "revision": 1, "changed": false }));
        assert_eq!(fake.loads(), 2);
        fake.set(module("b"));
        assert_eq!(refresh(&registry, &mut session, "main.go"), json!({ "revision": 1, "changed": false }));
        assert_eq!(fake.loads(), 2);
        assert_eq!(refresh(&registry, &mut session, "fake.mod"), json!({ "revision": 2, "changed": true }));
        fake.set(module("c"));
        let all = registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        assert_eq!(all, json!({ "revision": 3, "changed": true }));
        let none = registry.dispatch(&mut session, "project.model.refresh", json!({ "paths": [] })).unwrap();
        assert_eq!(none["changed"], false);
        let info = registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        assert_eq!(info["modules"][0]["name"], "c");
    }

    #[test]
    fn listing_the_libraries_checks_again_whether_they_are_downloaded() {
        let (registry, mut session, dir) = setup();
        let cache = dir.path().join("cache").join("example.com").join("lib@v1.0.0");
        let model = ProjectModel {
            libraries: vec![Library {
                name: "example.com/lib".to_owned(),
                version: Some("v1.0.0".to_owned()),
                root: cache.clone(),
                exists: false,
                ..Library::default()
            }],
            ..ProjectModel::default()
        };
        with_fake(&mut session, model);
        let listed = registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(listed["revision"], 1);
        assert_eq!(listed["libraries"][0]["exists"], false);
        std::fs::create_dir_all(&cache).unwrap();
        let listed = registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(listed["revision"], 2);
        assert_eq!(listed["libraries"][0]["exists"], true);
        let again = registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(again["revision"], 2);
        std::fs::remove_dir_all(&cache).unwrap();
        let listed = registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(listed["revision"], 3);
        assert_eq!(listed["libraries"][0]["exists"], false);
    }

    #[test]
    fn problems_are_shown_with_the_modules() {
        let (registry, mut session, dir) = setup();
        let file = dir.path().join("fake.mod");
        let problem =
            ModelProblem { file: file.clone(), line: 2, message: "unknown directive".to_owned(), warning: false };
        with_fake(&mut session, ProjectModel { problems: vec![problem], ..module("broken") });
        let info = registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        assert_eq!(info["problems"], json!([{ "file": file, "line": 2, "message": "unknown directive" }]));
    }

    #[test]
    fn the_models_problems_are_listed_and_marked_until_the_file_is_fixed() {
        let (registry, mut session, dir) = setup();
        let file = dir.path().join("fake.mod");
        std::fs::write(&file, "module m\n\n  frobnicate x  \n").unwrap();
        let broken = ProjectModel {
            problems: vec![ModelProblem {
                file: file.clone(),
                line: 3,
                message: "unknown directive: frobnicate".to_owned(),
                warning: false,
            }],
            ..module("broken")
        };
        let fake = with_fake(&mut session, broken);
        registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        let expected = json!([{
            "start": 12, "end": 24, "line": 3, "column": 3, "severity": "error",
            "message": "unknown directive: frobnicate", "source": "project model", "code": null
        }]);
        let listed = registry.dispatch(&mut session, "problems.file.list", json!({ "path": file })).unwrap();
        assert_eq!(listed, expected, "a closed file's problems are on its line on disk");
        let project = registry.dispatch(&mut session, "problems.project.list", json!({})).unwrap();
        assert_eq!(project, json!([{ "path": file, "problems": expected }]));

        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let next = registry.dispatch(&mut session, "editor.problem.next", json!({})).unwrap();
        assert_eq!(next["line"], 3, "F2 goes to it");

        let fixed = Module { name: "fixed".to_owned(), file: file.clone(), ..Module::default() };
        fake.set(ProjectModel { modules: vec![fixed], ..ProjectModel::default() });
        let refreshed = registry.dispatch(&mut session, "project.model.refresh", json!({ "paths": [file] })).unwrap();
        assert_eq!(refreshed["changed"], true);
        let listed = registry.dispatch(&mut session, "problems.file.list", json!({ "path": file })).unwrap();
        assert_eq!(listed, json!({ "problems": [], "checked": true }), "fixed, and known to be clean");
        assert_eq!(registry.dispatch(&mut session, "problems.project.list", json!({})).unwrap(), json!([]));
    }

    #[test]
    fn a_models_problem_moves_with_unsaved_edits() {
        let (registry, mut session, dir) = setup();
        let file = dir.path().join("fake.mod");
        std::fs::write(&file, "module m\n\nbad x\n\ngo 1.23\n").unwrap();
        let problem = |line| ModelProblem { file: file.clone(), line, message: "m".to_owned(), warning: false };
        with_fake(&mut session, ProjectModel { problems: vec![problem(3), problem(5)], ..module("m") });
        registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let lines = |registry: &Registry, session: &mut Session| -> Vec<(u64, String)> {
            let listed = registry.dispatch(session, "problems.file.list", json!({ "path": file })).unwrap();
            crate::language::listed_problems(&listed)
                .iter()
                .map(|problem| (problem["line"].as_u64().unwrap(), problem["source"].as_str().unwrap().to_owned()))
                .collect()
        };
        let edit = |registry: &Registry, session: &mut Session, text: &str| {
            registry.dispatch(session, "editor.text.write", json!({ "path": file, "text": text })).unwrap();
        };
        assert_eq!(lines(&registry, &mut session), [(3, MODEL_SOURCE.to_owned()), (5, MODEL_SOURCE.to_owned())]);

        edit(&registry, &mut session, "// a\n// b\nmodule m\n\nbad x\n\ngo 1.23\n");
        let moved: Vec<u64> = lines(&registry, &mut session).into_iter().map(|(line, _)| line).collect();
        assert_eq!(moved, [5, 7], "two lines added above: both move down");

        edit(&registry, &mut session, "module m\nbad x\n\ngo 1.23\n// end\n");
        let moved: Vec<u64> = lines(&registry, &mut session).into_iter().map(|(line, _)| line).collect();
        assert_eq!(moved, [2, 4], "a line removed above, one added below");

        edit(&registry, &mut session, "module m\n\nbad y\n");
        let moved: Vec<u64> = lines(&registry, &mut session).into_iter().map(|(line, _)| line).collect();
        assert_eq!(moved, [3, 3], "the edited line keeps its mark, a removed line's goes to where it was");
    }

    #[test]
    fn a_model_read_all_the_same_warns_and_a_line_with_an_error_is_not_told_twice() {
        let (registry, mut session, dir) = setup();
        let file = dir.path().join("fake.mod");
        let text = "module m\nbad\n";
        std::fs::write(&file, text).unwrap();
        let problem = |line, warning| ModelProblem { file: file.clone(), line, message: "m".to_owned(), warning };
        with_fake(&mut session, ProjectModel { problems: vec![problem(1, true), problem(2, false)], ..module("m") });
        registry.dispatch(&mut session, "project.model.refresh", json!({})).unwrap();
        let listed = registry.dispatch(&mut session, "problems.file.list", json!({ "path": file })).unwrap();
        let severities: Vec<(u64, &str)> = listed
            .as_array()
            .unwrap()
            .iter()
            .map(|problem| (problem["line"].as_u64().unwrap(), problem["severity"].as_str().unwrap()))
            .collect();
        assert_eq!(severities, [(1, "warning"), (2, "error")]);

        let diagnostic = json!({
            "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 3 } },
            "severity": 1, "message": "unknown directive: bad", "source": "server"
        });
        session.state_mut::<crate::language::Languages>().publish_for_test(&file, text, &[diagnostic]);
        let listed = registry.dispatch(&mut session, "problems.file.list", json!({ "path": file })).unwrap();
        let messages: Vec<String> = crate::language::listed_problems(&listed)
            .into_iter()
            .map(|problem| problem["message"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(messages, ["m", "unknown directive: bad"], "the server's error is told once, on line 2");
    }

    #[cfg(unix)]
    #[test]
    fn a_problem_named_by_the_real_path_is_the_projects_file() {
        let real = tempfile::tempdir().unwrap();
        let link = tempfile::tempdir().unwrap();
        let root = link.path().join("project");
        std::os::unix::fs::symlink(real.path(), &root).unwrap();
        let mut problems = vec![ModelProblem {
            file: real.path().canonicalize().unwrap().join("a/Cargo.toml"),
            ..ModelProblem::default()
        }];
        within(&mut problems, &root);
        assert_eq!(problems[0].file, root.join("a/Cargo.toml"));
    }

    #[test]
    fn another_project_or_environment_is_read_again() {
        let (registry, mut session, _dir) = setup();
        let fake = with_fake(&mut session, module("m"));
        registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        let other = tempfile::tempdir().unwrap();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": other.path() })).unwrap();
        let info = registry.dispatch(&mut session, "project.module.info", json!({ "all": true })).unwrap();
        assert_eq!(info["modules"][0]["root"], json!(other.path()));
        assert_eq!(fake.loads(), 2);
        let sdk = other.path().join("sdk");
        let env = ModelEnv { sdk_root: Some(sdk.clone()), sdk_version: Some("1.0".to_owned()), package_cache: None };
        session.state_mut::<ProjectModels>().set_env(env.clone());
        session.state_mut::<ProjectModels>().set_env(env);
        std::fs::create_dir_all(&sdk).unwrap();
        let listed = registry.dispatch(&mut session, "project.libraries.list", json!({})).unwrap();
        assert_eq!(listed["libraries"][0]["kind"], "sdk");
        assert_eq!(listed["libraries"][0]["root"], json!(sdk));
        assert_eq!(fake.loads(), 3);
    }
}

pub mod edits;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::build_tools::{BuildEnv, LoadState};
use crate::gradle::analysis::GradleAnalyzer;
use crate::maven::analysis::MavenAnalyzer;
use crate::run::project_root;
use crate::text_replace::{WriteParams, WriteText};
use edits::Exclusion;

/// Why a dependency is marked in the analysis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// Another version won a version conflict.
    Conflict,
    /// The same version is already there through another path.
    Duplicate,
    /// It would depend on itself.
    Cycle,
    /// The build's dependency management sets the version.
    Managed,
    /// The build forces the version.
    Forced,
    /// A dependency constraint chose the version.
    Constraint,
    /// A resolution rule of the build chose the version.
    Rule,
    /// It could not be resolved.
    Failed,
}

/// A resolved dependency in a module's tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencyNode {
    /// `groupId`.
    pub group_id: String,
    /// `artifactId`, or the Gradle module's name.
    pub artifact_id: String,
    /// The classifier, e.g. `tests`, when it has one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub classifier: String,
    /// The version asked for at this place; empty when the build asked for none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub requested_version: String,
    /// The version that is used; empty when none could be resolved.
    pub selected_version: String,
    /// The Maven scope or the Gradle configuration.
    pub scope: String,
    /// Whether the version asked for here lost to another one.
    pub conflict: bool,
    /// Whether this place adds nothing to the classpath.
    pub omitted: bool,
    /// Why it is marked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// What the build tool says about it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// The Gradle project it is, as `:lib`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    /// What it brings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<DependencyNode>,
}

impl DependencyNode {
    pub fn name(&self) -> String {
        format!("{}:{}", self.group_id, self.artifact_id)
    }

    fn coordinates(&self) -> String {
        match self.selected_version.as_str() {
            "" => self.name(),
            version => format!("{}:{version}", self.name()),
        }
    }

    fn is(&self, group: &str, artifact: &str) -> bool {
        self.group_id == group && self.artifact_id == artifact
    }

    fn brings(&self, group: &str, artifact: &str) -> bool {
        self.children.iter().any(|child| child.is(group, artifact) || child.brings(group, artifact))
    }
}

pub fn lost_conflict(requested: &str, selected: &str) -> bool {
    let dynamic = requested.contains(['+', '[', ']', '(', ')']) || requested.starts_with("latest.");
    !requested.is_empty() && !selected.is_empty() && requested != selected && !dynamic
}

/// A module of the build and what it resolves.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzedModule {
    /// Its id for the commands: `group:artifact` for Maven, `:app` for Gradle.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its build file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_file: Option<PathBuf>,
    /// Its direct dependencies, with what they bring.
    pub nodes: Vec<DependencyNode>,
}

pub trait DependencyAnalyzer: Send + Sync {
    fn id(&self) -> &'static str;
    fn applies(&self, root: &Path) -> bool;
    fn key(&self, root: &Path) -> String;
    fn by_configuration(&self) -> bool;
    fn start(&self, session: &mut Session, env: &BuildEnv, root: &Path) -> Result<(), CommandError>;
    fn modules(&self, root: &Path, output: &str) -> Vec<AnalyzedModule>;
    fn declaring_files(&self, root: &Path, module: &AnalyzedModule) -> Vec<PathBuf>;
    fn declaration(&self, root: &Path, file: &Path, text: &str, direct: &DependencyNode) -> Option<usize>;
    fn exclusion(
        &self,
        root: &Path,
        file: &Path,
        text: &str,
        direct: &DependencyNode,
        group: &str,
        artifact: &str,
    ) -> Result<Exclusion, String>;
    fn reload(&self, session: &mut Session) -> Result<(), CommandError>;
}

const ANALYZERS: [&dyn DependencyAnalyzer; 2] = [&MavenAnalyzer, &GradleAnalyzer];

fn env(session: &Session) -> BuildEnv {
    session.state::<BuildEnv>().cloned().unwrap_or_default()
}

fn analyzer(
    session: &Session,
    system: Option<&str>,
) -> Result<(&'static dyn DependencyAnalyzer, PathBuf), CommandError> {
    let root = project_root(session)?;
    ANALYZERS
        .into_iter()
        .find(|analyzer| match system {
            Some(system) => analyzer.id() == system,
            None => analyzer.applies(&root),
        })
        .filter(|analyzer| analyzer.applies(&root))
        .map(|analyzer| (analyzer, root))
        .ok_or_else(|| CommandError::new(t!("workbench-dependency-no-build")))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filter {
    pub scope: Option<String>,
    pub conflicts_only: bool,
    pub search: String,
}

impl Filter {
    fn own(&self, node: &DependencyNode) -> bool {
        (!self.conflicts_only || node.conflict) && self.named(&node.coordinates())
    }

    fn named(&self, name: &str) -> bool {
        self.search.is_empty() || name.to_lowercase().contains(&self.search.to_lowercase())
    }

    fn in_scope(&self, node: &DependencyNode) -> bool {
        self.scope.as_ref().is_none_or(|scope| *scope == node.scope)
    }
}

pub fn filtered_tree(nodes: &[DependencyNode], filter: &Filter) -> Vec<DependencyNode> {
    nodes
        .iter()
        .filter(|node| filter.in_scope(node))
        .filter_map(|node| {
            let children = filtered_tree(&node.children, filter);
            (filter.own(node) || !children.is_empty()).then(|| DependencyNode { children, ..node.clone() })
        })
        .collect()
}

/// A place a dependency is used from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// The coordinates from the module's direct dependency down to this one.
    pub path: Vec<String>,
    /// The Maven scope or the Gradle configuration.
    pub scope: String,
    /// The version asked for here.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub requested_version: String,
    /// Whether the version asked for here lost.
    pub conflict: bool,
    /// Whether this place adds nothing to the classpath.
    pub omitted: bool,
    /// Why it is marked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// What the build tool says about it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// A library of the module, once however many times it is used.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListedDependency {
    /// `groupId`.
    pub group_id: String,
    /// `artifactId`, or the Gradle module's name.
    pub artifact_id: String,
    /// The classifier, when it has one.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub classifier: String,
    /// The version that won.
    pub version: String,
    /// Every version asked for.
    pub requested: Vec<String>,
    /// The scopes or configurations it is in.
    pub scopes: Vec<String>,
    /// Whether some place asked for another version than the one that won.
    pub conflict: bool,
    /// Why it is marked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    /// Whether the module declares it itself.
    pub direct: bool,
    /// Where it is used from.
    pub usages: Vec<Usage>,
}

fn add_to_list(list: &mut BTreeMap<(String, String), ListedDependency>, node: &DependencyNode, path: &mut Vec<String>) {
    path.push(node.coordinates());
    let key = (node.name(), node.classifier.clone());
    let entry = list.entry(key).or_insert_with(|| ListedDependency {
        group_id: node.group_id.clone(),
        artifact_id: node.artifact_id.clone(),
        classifier: node.classifier.clone(),
        version: String::new(),
        requested: Vec::new(),
        scopes: Vec::new(),
        conflict: false,
        reason: None,
        direct: false,
        usages: Vec::new(),
    });
    if entry.version.is_empty() || !node.omitted {
        entry.version.clone_from(&node.selected_version);
    }
    if !node.requested_version.is_empty() && !entry.requested.contains(&node.requested_version) {
        entry.requested.push(node.requested_version.clone());
    }
    if !entry.scopes.contains(&node.scope) {
        entry.scopes.push(node.scope.clone());
    }
    entry.conflict |= node.conflict;
    if entry.reason.is_none() || node.conflict {
        entry.reason = node.reason.filter(|reason| *reason != Reason::Duplicate).or(entry.reason);
    }
    entry.direct |= path.len() == 1;
    entry.usages.push(Usage {
        path: path.clone(),
        scope: node.scope.clone(),
        requested_version: node.requested_version.clone(),
        conflict: node.conflict,
        omitted: node.omitted,
        reason: node.reason,
        note: node.note.clone(),
    });
    for child in &node.children {
        add_to_list(list, child, path);
    }
    path.pop();
}

pub fn listed(nodes: &[DependencyNode], filter: &Filter) -> Vec<ListedDependency> {
    let mut list = BTreeMap::new();
    let mut path = Vec::new();
    for node in nodes.iter().filter(|node| filter.in_scope(node)) {
        add_to_list(&mut list, node, &mut path);
    }
    list.into_values()
        .filter(|entry| {
            let name = format!("{}:{}:{}", entry.group_id, entry.artifact_id, entry.version);
            (!filter.conflicts_only || entry.conflict) && filter.named(&name)
        })
        .collect()
}

/// A module of the analysis, without its dependencies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ModuleSummary {
    /// Its id for the commands.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Its build file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_file: Option<PathBuf>,
}

/// What `build.dependencies.analyze` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DependencyAnalysis {
    /// The build system: `maven` or `gradle`.
    pub system: String,
    /// Where its loading is; absent before it was ever asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<LoadState>,
    /// Why it did not load.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The background task that loads it (`tasks.log.read`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
    /// The modules of the build.
    pub modules: Vec<ModuleSummary>,
    /// The module shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    /// Every scope or configuration of the module.
    pub scopes: Vec<String>,
    /// Whether the tree has a tree per scope, as Gradle's configurations.
    pub by_configuration: bool,
    /// The libraries, by name, as the filters leave them.
    pub dependencies: Vec<ListedDependency>,
    /// The tree of direct dependencies, as the filters leave it.
    pub tree: Vec<DependencyNode>,
}

/// The parameters of `build.dependencies.analyze`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzeParams {
    /// The build system, `maven` or `gradle`; the project's first when absent.
    #[serde(default)]
    pub system: Option<String>,
    /// The module's id; the first with dependencies when absent.
    #[serde(default)]
    pub module: Option<String>,
    /// Only this scope or configuration.
    #[serde(default)]
    pub scope: Option<String>,
    /// Only conflicts, in the tree with what leads to them.
    #[serde(default)]
    pub conflicts_only: bool,
    /// Only what has this text in its coordinates.
    #[serde(default)]
    pub search: Option<String>,
    /// Load it (again) with the build tool, which may download.
    #[serde(default)]
    pub load: bool,
    /// Answer only where its loading is, without reading the result.
    #[serde(default)]
    pub brief: bool,
    /// Waits up to this long for it to load (at most 10 minutes).
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

fn wait_load(env: &BuildEnv, key: &str, wait: Duration) {
    let end = Instant::now() + wait.min(crate::run::MAX_RUN_WAIT);
    while Instant::now() < end && env.load(key).is_some_and(|load| load.state == LoadState::Loading) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn loaded_modules(analyzer: &dyn DependencyAnalyzer, env: &BuildEnv, root: &Path) -> Option<Vec<AnalyzedModule>> {
    let load = env.load(&analyzer.key(root)).filter(|load| load.state == LoadState::Loaded)?;
    Some(analyzer.modules(root, &load.output))
}

fn chosen<'a>(modules: &'a [AnalyzedModule], id: Option<&str>) -> Result<Option<&'a AnalyzedModule>, CommandError> {
    match id {
        Some(id) => modules
            .iter()
            .find(|module| module.id == id)
            .map(Some)
            .ok_or_else(|| CommandError::new(t!("workbench-dependency-no-module", module = id))),
        None => Ok(modules.iter().find(|module| !module.nodes.is_empty()).or(modules.first())),
    }
}

fn scopes(nodes: &[DependencyNode]) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for node in nodes {
        if !found.contains(&node.scope) {
            found.push(node.scope.clone());
        }
    }
    found
}

pub struct Analyze;

impl Command for Analyze {
    type Params = AnalyzeParams;
    type Output = DependencyAnalysis;
    const ID: &'static str = "build.dependencies.analyze";
    const TITLE: &'static str = "Analyze Dependencies";
    const DESCRIPTION: &'static str = "Why each version is on a module's classpath, from Maven or Gradle themselves: \
         mvn dependency:tree -Dverbose, or Gradle's resolved graph per configuration through an init script. Answers \
         the modules, and for one of them the libraries (the version that won, every version asked for, the scopes, \
         each usage with its path from the direct dependency) and the tree. conflict marks a version that lost, \
         reason says why (conflict, managed, forced, constraint, rule; duplicate, cycle and failed for what adds \
         nothing). Filters: scope, conflictsOnly, search. With load it is loaded (again) in a background task, which \
         may download; with waitMs the answer waits for it. state is absent before it was ever loaded.";

    fn run(&self, session: &mut Session, params: AnalyzeParams) -> Result<DependencyAnalysis, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let (analyzer, root) = analyzer(session, params.system.as_deref())?;
        let env = env(session);
        if params.load {
            crate::trust::guard_build_load(session, &root)?;
            analyzer.start(session, &env, &root)?;
        }
        if let Some(wait) = params.wait_ms {
            wait_load(&env, &analyzer.key(&root), Duration::from_millis(wait));
        }
        let load = env.load(&analyzer.key(&root));
        let mut analysis = DependencyAnalysis {
            system: analyzer.id().to_owned(),
            state: load.as_ref().map(|load| load.state.clone()),
            error: load.as_ref().and_then(|load| load.error.clone()),
            task: load.as_ref().and_then(|load| load.task),
            by_configuration: analyzer.by_configuration(),
            ..DependencyAnalysis::default()
        };
        if params.brief {
            return Ok(analysis);
        }
        let Some(modules) = loaded_modules(analyzer, &env, &root) else { return Ok(analysis) };
        analysis.modules = modules
            .iter()
            .map(|module| ModuleSummary {
                id: module.id.clone(),
                name: module.name.clone(),
                build_file: module.build_file.clone(),
            })
            .collect();
        if let Some(module) = chosen(&modules, params.module.as_deref())? {
            let filter = Filter {
                scope: params.scope.filter(|scope| !scope.is_empty()),
                conflicts_only: params.conflicts_only,
                search: params.search.unwrap_or_default().trim().to_owned(),
            };
            analysis.module = Some(module.id.clone());
            analysis.scopes = scopes(&module.nodes);
            analysis.dependencies = listed(&module.nodes, &filter);
            analysis.tree = filtered_tree(&module.nodes, &filter);
        }
        Ok(analysis)
    }
}

/// The parameters of `build.dependencies.declaration`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationParams {
    /// The build system, `maven` or `gradle`; the project's first when absent.
    #[serde(default)]
    pub system: Option<String>,
    /// The module's id; the first with dependencies when absent.
    #[serde(default)]
    pub module: Option<String>,
    /// The dependency's `groupId`.
    pub group_id: String,
    /// The dependency's `artifactId`.
    pub artifact_id: String,
    /// The direct dependency that brings it, as `group:artifact`; the first that does when absent.
    #[serde(default)]
    pub via: Option<String>,
    /// Only answer the place, without opening it.
    #[serde(default)]
    pub no_open: bool,
}

/// What `build.dependencies.declaration` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Declaration {
    /// The file.
    pub path: PathBuf,
    /// The character offset in it, for navigate.place.goTo.
    pub offset: usize,
    /// The one-based line.
    pub line: usize,
    /// The direct dependency declared there, as `group:artifact`.
    pub direct: String,
}

fn loaded(
    session: &Session,
    system: Option<&str>,
) -> Result<(&'static dyn DependencyAnalyzer, PathBuf, Vec<AnalyzedModule>), CommandError> {
    let (analyzer, root) = analyzer(session, system)?;
    let modules = loaded_modules(analyzer, &env(session), &root)
        .ok_or_else(|| CommandError::new(t!("workbench-dependency-not-loaded")))?;
    Ok((analyzer, root, modules))
}

fn direct_dependencies<'a>(
    module: &'a AnalyzedModule,
    group: &str,
    artifact: &str,
    via: Option<&str>,
) -> Vec<&'a DependencyNode> {
    let mut found: Vec<&DependencyNode> = Vec::new();
    for node in &module.nodes {
        let wanted = match via {
            Some(via) => node.name() == via,
            None => node.brings(group, artifact),
        };
        if wanted && !found.iter().any(|known| known.name() == node.name()) {
            found.push(node);
        }
    }
    found
}

pub struct GoToDeclaration;

impl Command for GoToDeclaration {
    type Params = DeclarationParams;
    type Output = Declaration;
    const ID: &'static str = "build.dependencies.declaration";
    const TITLE: &'static str = "Go to Declaration";
    const DESCRIPTION: &'static str = "Opens the place in the build file that declares a dependency of the analysis \
         (build.dependencies.analyze): the module's own dependency, or for one that comes with another the direct \
         dependency that brings it (via), in the module's build file, a parent POM, the root build script or the \
         version catalog. noOpen only answers the place. Refused when no build file of the project declares it.";

    fn run(&self, session: &mut Session, params: DeclarationParams) -> Result<Declaration, CommandError> {
        let (analyzer, root, modules) = loaded(session, params.system.as_deref())?;
        let module = chosen(&modules, params.module.as_deref())?
            .ok_or_else(|| CommandError::new(t!("workbench-dependency-not-loaded")))?;
        let (group, artifact) = (params.group_id.as_str(), params.artifact_id.as_str());
        let own = module.nodes.iter().find(|node| node.is(group, artifact)).filter(|_| params.via.is_none());
        let bringing = direct_dependencies(module, group, artifact, params.via.as_deref());
        let directs: Vec<&DependencyNode> = own.into_iter().chain(bringing).collect();
        for direct in directs {
            for file in analyzer.declaring_files(&root, module) {
                let Ok((text, _)) = crate::run::file_text(session, &file) else { continue };
                let Some(at) = analyzer.declaration(&root, &file, &text, direct) else { continue };
                let offset = text[..at].chars().count();
                let line = text[..at].matches('\n').count() + 1;
                let declaration = Declaration { path: file, offset, line, direct: direct.name() };
                if !params.no_open {
                    let place = crate::history::NavigateToParams { path: declaration.path.clone(), offset, end: None };
                    crate::history::NavigateTo.run(session, place)?;
                }
                return Ok(declaration);
            }
        }
        Err(CommandError::new(t!("workbench-dependency-no-declaration", dependency = format!("{group}:{artifact}"))))
    }
}

/// The parameters of `build.dependencies.exclude`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExcludeParams {
    /// The build system, `maven` or `gradle`; the project's first when absent.
    #[serde(default)]
    pub system: Option<String>,
    /// The module's id; the first with dependencies when absent.
    #[serde(default)]
    pub module: Option<String>,
    /// The `groupId` to exclude.
    pub group_id: String,
    /// The `artifactId` to exclude.
    pub artifact_id: String,
    /// The direct dependency to exclude it from, as `group:artifact`; every one that brings it when absent.
    #[serde(default)]
    pub via: Option<String>,
    /// Leave the project as it is loaded: Load Changes shows instead.
    #[serde(default)]
    pub no_reload: bool,
}

/// A direct dependency an exclusion went to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedFrom {
    /// The direct dependency, as `group:artifact`.
    pub direct: String,
    /// The build file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    /// Whether the file changed; false when it had the exclusion already or does not declare it.
    pub added: bool,
}

/// What `build.dependencies.exclude` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Excluded {
    /// Each direct dependency.
    pub from: Vec<ExcludedFrom>,
    /// Whether the project and the analysis are loading again.
    pub reloaded: bool,
    /// Why they are not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload_error: Option<String>,
}

fn exclude_from(
    session: &mut Session,
    analyzer: &dyn DependencyAnalyzer,
    root: &Path,
    module: &AnalyzedModule,
    direct: &DependencyNode,
    params: &ExcludeParams,
) -> Result<ExcludedFrom, CommandError> {
    for file in analyzer.declaring_files(root, module) {
        let Ok((text, _)) = crate::run::file_text(session, &file) else { continue };
        let edit = analyzer
            .exclusion(root, &file, &text, direct, &params.group_id, &params.artifact_id)
            .map_err(|error| CommandError::new(t!("workbench-dependency-edit-failed", error = error)))?;
        match edit {
            Exclusion::NotDeclared => {}
            Exclusion::Present => {
                return Ok(ExcludedFrom { direct: direct.name(), file: Some(file), added: false });
            }
            Exclusion::Edited(text) => {
                let write = WriteParams { path: file.clone(), text, save: Some(true), ..WriteParams::default() };
                WriteText.run(session, write)?;
                return Ok(ExcludedFrom { direct: direct.name(), file: Some(file), added: true });
            }
        }
    }
    Ok(ExcludedFrom { direct: direct.name(), file: None, added: false })
}

pub struct Exclude;

impl Command for Exclude {
    type Params = ExcludeParams;
    type Output = Excluded;
    const ID: &'static str = "build.dependencies.exclude";
    const TITLE: &'static str = "Exclude Dependency";
    const DESCRIPTION: &'static str = "Excludes a library that comes with a direct dependency of the analysis \
         (build.dependencies.analyze), in the build file and keeping it as written: Maven <exclusions> under the \
         direct dependency, Gradle `exclude` in the script's own DSL. via names one direct dependency; without it \
         every one that brings the library gets it. Then the project and the analysis load again, unless noReload. \
         Refused for a library only the module itself declares, and when no build file declares what brings it.";

    fn run(&self, session: &mut Session, params: ExcludeParams) -> Result<Excluded, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let dependency = format!("{}:{}", params.group_id, params.artifact_id);
        if !edits::valid_name(&params.group_id) || !edits::valid_name(&params.artifact_id) {
            return Err(CommandError::new(t!("workbench-dependency-bad-name", dependency = dependency.as_str())));
        }
        let (analyzer, root, modules) = loaded(session, params.system.as_deref())?;
        let module = chosen(&modules, params.module.as_deref())?
            .ok_or_else(|| CommandError::new(t!("workbench-dependency-not-loaded")))?;
        let directs = direct_dependencies(module, &params.group_id, &params.artifact_id, params.via.as_deref());
        let brought: Vec<&DependencyNode> = directs
            .into_iter()
            .filter(|direct| !direct.is(&params.group_id, &params.artifact_id))
            .filter(|direct| direct.brings(&params.group_id, &params.artifact_id))
            .collect();
        if brought.is_empty() {
            return Err(CommandError::new(t!("workbench-dependency-exclude-direct", dependency = dependency.as_str())));
        }
        let mut from = Vec::new();
        for direct in brought {
            from.push(exclude_from(session, analyzer, &root, module, direct, &params)?);
        }
        if from.iter().all(|edit| edit.file.is_none()) {
            let direct = from.first().map(|edit| edit.direct.clone()).unwrap_or_default();
            return Err(CommandError::new(t!("workbench-dependency-no-declaration", dependency = direct)));
        }
        let mut excluded = Excluded { reloaded: false, reload_error: None, from };
        if !params.no_reload && excluded.from.iter().any(|edit| edit.added) {
            let again =
                crate::trust::guard_build_load(session, &root).and_then(|()| analyzer.reload(session)).and_then(|()| {
                    let env = env(session);
                    analyzer.start(session, &env, &root)
                });
            match again {
                Ok(()) => excluded.reloaded = true,
                Err(error) => excluded.reload_error = Some(error.to_string()),
            }
        }
        Ok(excluded)
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(Analyze)?;
    registry.register(GoToDeclaration)?;
    registry.register(Exclude)
}

#[cfg(test)]
mod tests;

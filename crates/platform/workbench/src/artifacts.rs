pub mod jar;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_project::model::{FolderKind, Module, ProjectModel};
use zen_run::{
    BeforeLaunchTask, CommandLine, Configuration, FieldKind, FieldSpec, KindSpec, RunOutput, RunState, SharedOutput,
};

use crate::Workbench;
use crate::before_launch::{Started, TaskStart, TaskType};
use crate::build_tools::BuildEnv;
use crate::build_triggers::When;
use crate::java_run::{BuildTool, GRADLE_INIT, argument_file, build_tool};
use crate::project_model::ProjectModels;
use crate::run::{Driver, RunControl, RunSummary, Runs, wait_for_run};

pub const FILE: &str = ".zen/artifacts.json";

pub const RUN_KIND: &str = "artifacts.build";

pub const TASK: &str = "buildArtifacts";

const MAIN_SEARCH_LIMIT: usize = 300;

/// What an artifact is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ArtifactKind {
    /// A jar the IDE writes from the modules' compiled classes and resources and their libraries.
    #[default]
    Jar,
    /// The build's own packaging: a Maven phase or a Gradle task (`package`, `jar`, `bootJar`).
    Build,
}

/// Where a jar artifact puts the libraries of its modules.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LibraryPlacement {
    /// Extracted into the jar, so that it runs alone.
    #[default]
    Inside,
    /// Copied next to the jar and named in its manifest's Class-Path.
    NextTo,
}

/// An artifact the project builds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Artifact {
    /// Its name, unique in the project, e.g. `shop:jar`.
    pub name: String,
    pub kind: ArtifactKind,
    /// The modules by name (project.structure.modules): a jar takes their compiled classes and
    /// resources; a build artifact runs the build of the first. Empty: the project's root module.
    pub modules: Vec<String>,
    /// A jar's Main-Class; empty for a library jar.
    pub main_class: String,
    /// A jar's libraries: extracted inside it or copied next to it.
    pub libraries: LibraryPlacement,
    /// The folder a jar is written to, relative to the project; `out/artifacts/<name>` when empty.
    pub output: String,
    /// A build artifact's phase or task with its arguments: `package`, `jar`, `bootJar`.
    pub task: String,
}

/// What `project.structure.artifacts` answers: the Artifacts section.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactsSection {
    /// The artifacts kept in `.zen/artifacts.json`.
    pub artifacts: Vec<Artifact>,
    /// What can be added: for Maven and Gradle the build's own packaging first, then a jar of the
    /// modules. Build Artifacts offers them while no artifact is kept.
    pub suggestions: Vec<Artifact>,
    /// The file of each jar artifact, kept or suggested, by name.
    pub jars: Vec<ArtifactFile>,
}

/// Where an artifact is written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactFile {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Stored {
    artifacts: Vec<Artifact>,
}

pub fn saved(root: &Path) -> Vec<Artifact> {
    std::fs::read(root.join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .map(|stored| stored.artifacts)
        .unwrap_or_default()
}

fn write_saved(root: &Path, artifacts: Vec<Artifact>) -> Result<(), CommandError> {
    let failed =
        |error: &dyn std::fmt::Display| CommandError::new(t!("artifacts-error-write", error = error.to_string()));
    let file = root.join(FILE);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| failed(&error))?;
    }
    let mut text = serde_json::to_vec_pretty(&Stored { artifacts }).unwrap_or_default();
    text.push(b'\n');
    zen_vfs::write_atomic(&file, &text).map_err(|error| failed(&error))
}

fn require_project(session: &Session) -> Result<PathBuf, CommandError> {
    session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_owned)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

fn model(session: &mut Session, root: &Path) -> ProjectModel {
    let models = session.state_mut::<ProjectModels>();
    models.ensure(Some(root));
    models.model().clone()
}

fn jvm_modules(model: &ProjectModel) -> Vec<Module> {
    model
        .modules
        .iter()
        .filter(|module| {
            matches!(module.details.get("build").and_then(serde_json::Value::as_str), Some("Maven" | "Gradle"))
        })
        .cloned()
        .collect()
}

fn plain_module(root: &Path) -> Module {
    Module {
        name: root.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
        root: root.to_owned(),
        ..Module::default()
    }
}

fn pick(root: &Path, modules: &[Module], names: &[String]) -> Result<Vec<Module>, String> {
    if names.is_empty() {
        let module = modules.iter().find(|module| module.root == root).or_else(|| modules.first());
        return Ok(vec![module.cloned().unwrap_or_else(|| plain_module(root))]);
    }
    names
        .iter()
        .map(|name| {
            modules
                .iter()
                .find(|module| module.name == *name)
                .cloned()
                .or_else(|| (modules.is_empty() && plain_module(root).name == *name).then(|| plain_module(root)))
                .ok_or_else(|| t!("artifacts-error-no-module", module = name.as_str()))
        })
        .collect()
}

pub fn file_stem(artifact: &Artifact) -> String {
    let name = artifact.name.strip_suffix(":jar").unwrap_or(&artifact.name);
    let stem: String =
        name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    if stem.trim_matches(['_', '.']).is_empty() { "artifact".to_owned() } else { stem }
}

pub fn jar_path(root: &Path, artifact: &Artifact) -> PathBuf {
    let stem = file_stem(artifact);
    let output = artifact.output.trim();
    let folder = if output.is_empty() {
        root.join("out").join("artifacts").join(&stem)
    } else if Path::new(output).is_absolute() {
        PathBuf::from(output)
    } else {
        root.join(output)
    };
    folder.join(format!("{stem}.jar"))
}

fn gradle_task(module: &Module) -> &'static str {
    let spring = ["build.gradle", "build.gradle.kts"].iter().any(|name| {
        std::fs::read_to_string(module.root.join(name)).is_ok_and(|text| text.contains("org.springframework.boot"))
    });
    if spring { "bootJar" } else { "jar" }
}

fn java_files(folder: &Path, found: &mut Vec<PathBuf>, limit: usize) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        if found.len() >= limit {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if !name.starts_with('.') && !matches!(name.as_str(), "target" | "build" | "out" | "node_modules") {
                java_files(&entry.path(), found, limit);
            }
        } else if kind.is_file() && name.ends_with(".java") {
            found.push(entry.path());
        }
    }
}

fn source_folders(module: &Module) -> Vec<PathBuf> {
    let folders: Vec<PathBuf> = module
        .folders
        .iter()
        .filter(|folder| folder.kind == FolderKind::Sources)
        .map(|folder| folder.path.clone())
        .collect();
    if folders.is_empty() { vec![module.root.clone()] } else { folders }
}

pub fn main_class_of(module: &Module) -> Option<String> {
    let mut files = Vec::new();
    for folder in source_folders(module) {
        java_files(&folder, &mut files, MAIN_SEARCH_LIMIT);
    }
    files.iter().find_map(|file| {
        let text = std::fs::read_to_string(file).ok()?;
        crate::java_run::main_methods(file, &text).into_iter().next().map(|main| main.class)
    })
}

pub fn suggestions(root: &Path, model: &ProjectModel) -> Vec<Artifact> {
    let modules = jvm_modules(model);
    let mut found = Vec::new();
    let first = modules
        .iter()
        .find(|module| module.root == root)
        .or_else(|| modules.first())
        .cloned()
        .or_else(|| (build_tool(root) != BuildTool::Javac).then(|| plain_module(root)));
    match (&first, build_tool(root)) {
        (Some(module), BuildTool::Maven) => found.push(Artifact {
            name: format!("{}:mvn package", module.name),
            kind: ArtifactKind::Build,
            modules: vec![module.name.clone()],
            task: "package".to_owned(),
            ..Artifact::default()
        }),
        (Some(module), BuildTool::Gradle) => {
            let task = gradle_task(module);
            found.push(Artifact {
                name: format!("{}:gradle {task}", module.name),
                kind: ArtifactKind::Build,
                modules: vec![module.name.clone()],
                task: task.to_owned(),
                ..Artifact::default()
            });
        }
        _ => {}
    }
    let with_sources = modules
        .iter()
        .find(|module| module.folders.iter().any(|folder| folder.kind == FolderKind::Sources && folder.path.is_dir()));
    let module = match (with_sources.or(first.as_ref()), crate::jdk::is_jvm_project(root)) {
        (Some(module), _) => Some(module.clone()),
        (None, true) => Some(plain_module(root)),
        (None, false) => {
            let mut files = Vec::new();
            java_files(root, &mut files, 1);
            (!files.is_empty()).then(|| plain_module(root))
        }
    };
    if let Some(module) = module {
        found.push(Artifact {
            name: format!("{}:jar", module.name),
            kind: ArtifactKind::Jar,
            modules: if modules.is_empty() { Vec::new() } else { vec![module.name.clone()] },
            main_class: main_class_of(&module).unwrap_or_default(),
            ..Artifact::default()
        });
    }
    found
}

pub fn section(session: &mut Session, root: &Path) -> ArtifactsSection {
    let model = model(session, root);
    let artifacts = saved(root);
    let suggestions = suggestions(root, &model);
    let jars = artifacts
        .iter()
        .chain(&suggestions)
        .filter(|artifact| artifact.kind == ArtifactKind::Jar)
        .map(|artifact| ArtifactFile { name: artifact.name.clone(), path: jar_path(root, artifact) })
        .collect();
    ArtifactsSection { artifacts, suggestions, jars }
}

pub fn buildable(section: &ArtifactsSection) -> &[Artifact] {
    if section.artifacts.is_empty() { &section.suggestions } else { &section.artifacts }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Build,
    Rebuild,
    Clean,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Folder(PathBuf),
    Classpath(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    pub name: String,
    pub jar: PathBuf,
    pub main_class: String,
    pub extract: bool,
    pub sources: Vec<Source>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub lines: Vec<CommandLine>,
    pub packs: Vec<Pack>,
    pub clean: Vec<PathBuf>,
}

fn line(program: PathBuf, args: Vec<String>, cwd: &Path) -> CommandLine {
    let name = program.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let display = std::iter::once(name).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
    CommandLine { program, args, cwd: cwd.to_owned(), env: Vec::new(), display }
}

fn tool(env: &BuildEnv, tool: BuildTool, module: &Path, root: &Path) -> Result<PathBuf, String> {
    let found = match tool {
        BuildTool::Maven => {
            let settings = crate::maven::settings::MavenSettings::read(env, root);
            crate::maven::settings::executable(&settings, module, root, env, &env.folders("maven"))
        }
        BuildTool::Gradle => {
            let settings = crate::gradle::settings::GradleSettings::read(env, root);
            crate::gradle::settings::executable(&settings, module, root, env, &env.folders("gradle"))
        }
        BuildTool::Javac => None,
    };
    let program = if tool == BuildTool::Maven { "mvn" } else { "gradle" };
    found.map(|found| found.path).ok_or_else(|| t!("workbench-java-tool-missing", program = program))
}

struct Planner<'a> {
    root: &'a Path,
    env: &'a BuildEnv,
    scratch: &'a Path,
    clean: bool,
    plan: Plan,
    compiled: Vec<(PathBuf, Vec<Source>)>,
}

impl Planner<'_> {
    fn write(&self, name: &str, text: &str) -> Result<PathBuf, String> {
        std::fs::create_dir_all(self.scratch).map_err(|error| format!("{}: {error}", self.scratch.display()))?;
        let path = self.scratch.join(name);
        std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
        Ok(path)
    }

    fn compile(&mut self, module: &Module) -> Result<Vec<Source>, String> {
        if let Some((_, sources)) = self.compiled.iter().find(|(root, _)| *root == module.root) {
            return Ok(sources.clone());
        }
        let index = self.compiled.len();
        let folder = module.root.as_path();
        let sources = match build_tool(folder) {
            BuildTool::Maven => {
                let mvn = tool(self.env, BuildTool::Maven, folder, self.root)?;
                let deps = self.scratch.join(format!("dependencies-{index}.txt"));
                let mut args = vec!["-B".to_owned()];
                if self.clean {
                    args.push("clean".to_owned());
                }
                args.extend([
                    "compile".to_owned(),
                    "dependency:build-classpath".to_owned(),
                    format!("-Dmdep.outputFile={}", deps.display()),
                    "-Dmdep.includeScope=runtime".to_owned(),
                ]);
                self.plan.lines.push(line(mvn, args, folder));
                let classes = module.output.clone().unwrap_or_else(|| folder.join("target/classes"));
                vec![Source::Folder(classes), Source::Classpath(deps)]
            }
            BuildTool::Gradle => {
                let gradle = tool(self.env, BuildTool::Gradle, folder, self.root)?;
                let init = self.write("zen-classpath.gradle", GRADLE_INIT)?;
                let classpath = self.scratch.join(format!("classpath-{index}.args"));
                let mut args = vec!["--init-script".to_owned(), init.display().to_string()];
                args.push(format!("-PzenClasspathOut={}", classpath.display()));
                args.extend(["-p".to_owned(), folder.display().to_string()]);
                if self.clean {
                    args.push("clean".to_owned());
                }
                args.push("zenClasspath".to_owned());
                self.plan.lines.push(line(gradle, args, folder));
                vec![Source::Classpath(classpath)]
            }
            BuildTool::Javac => {
                let home = self.env.java_home(self.root).ok_or_else(|| t!("workbench-java-no-jdk"))?;
                let javac = home.join("bin").join(if cfg!(windows) { "javac.exe" } else { "javac" });
                let mut files = Vec::new();
                for folder in source_folders(module) {
                    java_files(&folder, &mut files, usize::MAX);
                }
                if files.is_empty() {
                    return Err(t!("workbench-java-no-sources", path = folder.display().to_string()));
                }
                let list: String = files.iter().map(|file| argument_file(&file.display().to_string())).collect();
                let list = self.write(&format!("sources-{index}.args"), &list)?;
                let classes = self.scratch.join(format!("classes-{index}"));
                let mut args =
                    vec!["-d".to_owned(), classes.display().to_string(), "-encoding".to_owned(), "UTF-8".to_owned()];
                if let Some(level) = crate::jdk::project_jdk(self.root).map(|project| project.language_level)
                    && level > 0
                {
                    args.extend(["--release".to_owned(), level.to_string()]);
                }
                args.push(format!("@{}", list.display()));
                self.plan.lines.push(line(javac, args, folder));
                vec![Source::Folder(classes)]
            }
        };
        self.compiled.push((module.root.clone(), sources.clone()));
        Ok(sources)
    }

    fn build_line(&mut self, module: &Module, task: &str) -> Result<(), String> {
        let folder = module.root.as_path();
        let tool_kind = build_tool(folder);
        if tool_kind == BuildTool::Javac {
            return Err(t!("artifacts-error-no-build", module = module.name.as_str()));
        }
        let program = tool(self.env, tool_kind, folder, self.root)?;
        let mut args = Vec::new();
        if tool_kind == BuildTool::Maven {
            args.push("-B".to_owned());
        } else {
            args.extend(["-p".to_owned(), folder.display().to_string()]);
        }
        args.extend(zen_run::args::split(task));
        self.plan.lines.push(line(program, args, folder));
        Ok(())
    }
}

pub fn plan(
    root: &Path,
    model: &ProjectModel,
    artifacts: &[Artifact],
    action: Action,
    env: &BuildEnv,
    scratch: &Path,
) -> Result<Plan, String> {
    std::fs::create_dir_all(scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let modules = jvm_modules(model);
    let mut planner =
        Planner { root, env, scratch, clean: action == Action::Rebuild, plan: Plan::default(), compiled: Vec::new() };
    for artifact in artifacts {
        let picked = pick(root, &modules, &artifact.modules)?;
        match artifact.kind {
            ArtifactKind::Build => {
                let module = &picked[0];
                let task = match (action, artifact.task.trim()) {
                    (Action::Clean, _) => "clean".to_owned(),
                    (Action::Rebuild, task) => format!("clean {}", default_task(module, task)),
                    (Action::Build, task) => default_task(module, task),
                };
                planner.build_line(module, &task)?;
            }
            ArtifactKind::Jar => {
                let jar = jar_path(root, artifact);
                if action != Action::Build {
                    planner.plan.clean.push(jar.clone());
                }
                if action == Action::Clean {
                    continue;
                }
                let mut sources = Vec::new();
                for module in &picked {
                    for source in planner.compile(module)? {
                        if !sources.contains(&source) {
                            sources.push(source);
                        }
                    }
                }
                planner.plan.packs.push(Pack {
                    name: artifact.name.clone(),
                    jar,
                    main_class: artifact.main_class.trim().to_owned(),
                    extract: artifact.libraries == LibraryPlacement::Inside,
                    sources,
                });
            }
        }
    }
    Ok(planner.plan)
}

fn default_task(module: &Module, task: &str) -> String {
    if !task.is_empty() {
        return task.to_owned();
    }
    match build_tool(&module.root) {
        BuildTool::Gradle => gradle_task(module).to_owned(),
        _ => "package".to_owned(),
    }
}

pub fn contents(sources: &[Source]) -> jar::Contents {
    let mut contents = jar::Contents::default();
    for source in sources {
        match source {
            Source::Folder(folder) => {
                if !contents.folders.contains(folder) {
                    contents.folders.push(folder.clone());
                }
            }
            Source::Classpath(file) => {
                let text = std::fs::read_to_string(file).unwrap_or_default();
                let found = jar::split(jar::classpath_file(&text));
                for folder in found.folders {
                    if !contents.folders.contains(&folder) {
                        contents.folders.push(folder);
                    }
                }
                for library in found.libraries {
                    if !contents.libraries.contains(&library) {
                        contents.libraries.push(library);
                    }
                }
            }
        }
    }
    contents
}

pub fn pack(packs: &[Pack], output: &SharedOutput) -> Result<(), String> {
    let seconds =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_secs()).unwrap_or(0);
    for pack in packs {
        let contents = contents(&pack.sources);
        let written = jar::write(&pack.jar, &pack.main_class, &contents, pack.extract, seconds)?;
        output.update(|output| {
            output.system(&t!(
                "artifacts-built",
                name = pack.name.as_str(),
                path = written.jar.display().to_string(),
                count = written.entries
            ))
        });
        for copied in &written.copied {
            output.update(|output| output.system(&t!("artifacts-copied", path = copied.display().to_string())));
        }
    }
    Ok(())
}

/// Which artifacts to build, rebuild or clean.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BuildParams {
    /// The artifacts by name; every one Build Artifacts lists when empty (the kept ones, else the
    /// suggestions).
    pub names: Vec<String>,
    /// Waits up to this long for the run to end (at most 10 minutes) and answers its summary.
    pub wait_ms: Option<u64>,
}

/// What `build.artifacts.*` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BuildAnswer {
    /// The run in the Run tool window that compiles and packs, if one was needed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// With `waitMs`: where the run is once it ended or the time was up.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<RunSummary>,
    /// The jars the run writes.
    pub jars: Vec<PathBuf>,
    /// The files removed before building: jars and the libraries copied next to them.
    pub removed: Vec<PathBuf>,
}

fn chosen(session: &mut Session, root: &Path, names: &[String]) -> Result<Vec<Artifact>, CommandError> {
    let section = section(session, root);
    let listed = buildable(&section);
    if names.is_empty() {
        if listed.is_empty() {
            return Err(CommandError::new(t!("artifacts-error-none")));
        }
        return Ok(listed.to_vec());
    }
    names
        .iter()
        .map(|name| {
            section
                .artifacts
                .iter()
                .chain(&section.suggestions)
                .find(|artifact| artifact.name == *name)
                .cloned()
                .ok_or_else(|| CommandError::new(t!("artifacts-error-unknown", name = name.as_str())))
        })
        .collect()
}

pub fn start(session: &mut Session, params: &BuildParams, action: Action) -> Result<BuildAnswer, CommandError> {
    let root = require_project(session)?;
    crate::trust::guard_project_build(session, &root)?;
    let artifacts = chosen(session, &root, &params.names)?;
    let model = model(session, &root);
    let env = session.state::<BuildEnv>().cloned().unwrap_or_default();
    let scratch = crate::run::scratch_folder();
    let plan = plan(&root, &model, &artifacts, action, &env, &scratch).map_err(|error| {
        let _ = std::fs::remove_dir_all(&scratch);
        CommandError::new(t!("artifacts-error-cannot-build", error = error))
    })?;
    let mut plan = plan;
    let around = match action {
        Action::Build => Some((When::BeforeBuild, When::AfterBuild)),
        Action::Rebuild => Some((When::BeforeRebuild, When::AfterRebuild)),
        Action::Clean => None,
    };
    if let Some((before, after)) = around {
        let triggers = |session: &mut Session, when| {
            crate::build_triggers::lines(session, &root, when).map_err(|error| {
                let _ = std::fs::remove_dir_all(&scratch);
                CommandError::new(t!("artifacts-error-cannot-build", error = error))
            })
        };
        let mut lines = triggers(session, before)?;
        lines.append(&mut plan.lines);
        lines.extend(triggers(session, after)?);
        plan.lines = lines;
    }
    let removed: Vec<PathBuf> = plan.clean.iter().flat_map(|jar| jar::remove(jar)).collect();
    let jars: Vec<PathBuf> = plan.packs.iter().map(|pack| pack.jar.clone()).collect();
    if plan.lines.is_empty() {
        let _ = std::fs::remove_dir_all(&scratch);
        return Ok(BuildAnswer { run_id: None, summary: None, jars, removed });
    }
    let name = match artifacts.as_slice() {
        [one] => one.name.clone(),
        _ => t!("artifacts-run-all"),
    };
    let configuration = Configuration { name, kind: RUN_KIND.to_owned(), temporary: true, ..Configuration::default() };
    let runs = session.state_mut::<Runs>();
    let mut environment = runs.env().env.clone();
    for (key, value) in env.environment(&root, &env.folders("maven")) {
        environment.retain(|(known, _)| *known != key);
        environment.push((key, value));
    }
    let output = SharedOutput::new(RunOutput::new(Box::new(zen_run::PlainParser)), runs.waker().cloned());
    let driver = Driver::new(plan.lines.clone(), environment, output.clone(), scratch.clone());
    let control: Arc<dyn RunControl> = driver.clone();
    let run_id = runs.insert_task(configuration, RUN_KIND.to_owned(), plan.lines.clone(), output.clone(), control);
    let packs = plan.packs;
    driver.then(Box::new(move || {
        let worker = output.clone();
        let spawned = std::thread::Builder::new().name("zen-artifacts".to_owned()).spawn(move || {
            let result = pack(&packs, &worker);
            let _ = std::fs::remove_dir_all(&scratch);
            worker.update(|output| match result {
                Ok(()) => {
                    output.system(&t!("workbench-run-process-finished", code = 0));
                    output.end(RunState::Finished, Some(0));
                }
                Err(error) => {
                    output.system(&error);
                    output.fail(error);
                }
            });
        });
        if let Err(error) = spawned {
            output.update(|output| output.fail(error.to_string()));
        }
    }));
    driver.start();
    let summary = match params.wait_ms.map(Duration::from_millis) {
        Some(wait) => Some(wait_for_run(session, run_id, wait)?),
        None => None,
    };
    Ok(BuildAnswer { run_id: Some(run_id), summary, jars, removed })
}

pub struct ArtifactsCommand;

impl Command for ArtifactsCommand {
    type Params = NoParams;
    type Output = ArtifactsSection;
    const ID: &'static str = "project.structure.artifacts";
    const TITLE: &'static str = "Project Structure: Artifacts";
    const DESCRIPTION: &'static str = "The Artifacts section of Project Structure: the artifacts kept in \
         .zen/artifacts.json (a `jar` the IDE writes from modules' compiled classes and resources with a Main-Class, \
         its libraries extracted `inside` or copied `nextTo` it; or a `build` artifact, the build's own packaging), \
         the suggestions to add (for Maven and Gradle the build's packaging, `mvn package`, `gradle jar` or \
         `bootJar`, first, then a jar of the modules), and where each jar is written.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ArtifactsSection, CommandError> {
        let root = require_project(session)?;
        Ok(section(session, &root))
    }
}

/// What `project.artifact.save` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveParams {
    pub artifact: Artifact,
    /// The name it had before, when it is renamed or changed.
    pub previous: Option<String>,
}

pub struct SaveArtifact;

impl Command for SaveArtifact {
    type Params = SaveParams;
    type Output = ArtifactsSection;
    const ID: &'static str = "project.artifact.save";
    const TITLE: &'static str = "Save Artifact";
    const DESCRIPTION: &'static str = "Adds or changes an artifact kept in .zen/artifacts.json (see \
         project.structure.artifacts for its fields); `previous` names the one it replaces. The name must be \
         unique and the modules must be the project's. Answers the Artifacts section.";

    fn run(&self, session: &mut Session, params: SaveParams) -> Result<ArtifactsSection, CommandError> {
        let root = require_project(session)?;
        let mut artifact = params.artifact;
        artifact.name = artifact.name.trim().to_owned();
        artifact.main_class = artifact.main_class.trim().to_owned();
        if artifact.name.is_empty() {
            return Err(CommandError::new(t!("artifacts-error-no-name")));
        }
        let model = model(session, &root);
        pick(&root, &jvm_modules(&model), &artifact.modules).map_err(CommandError::new)?;
        let mut artifacts = saved(&root);
        let previous = params.previous.unwrap_or_else(|| artifact.name.clone());
        if artifact.name != previous && artifacts.iter().any(|kept| kept.name == artifact.name) {
            return Err(CommandError::new(t!("artifacts-error-taken", name = artifact.name.as_str())));
        }
        let at = artifacts.iter().position(|kept| kept.name == previous);
        artifacts.retain(|kept| kept.name != previous);
        artifacts.insert(at.unwrap_or(artifacts.len()), artifact);
        write_saved(&root, artifacts)?;
        Ok(section(session, &root))
    }
}

/// What `project.artifact.delete` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DeleteParams {
    pub name: String,
}

pub struct DeleteArtifact;

impl Command for DeleteArtifact {
    type Params = DeleteParams;
    type Output = ArtifactsSection;
    const ID: &'static str = "project.artifact.delete";
    const TITLE: &'static str = "Remove Artifact";
    const DESCRIPTION: &'static str = "Removes an artifact kept in .zen/artifacts.json; its built files stay. \
         Answers the Artifacts section.";

    fn run(&self, session: &mut Session, params: DeleteParams) -> Result<ArtifactsSection, CommandError> {
        let root = require_project(session)?;
        let mut artifacts = saved(&root);
        let before = artifacts.len();
        artifacts.retain(|kept| kept.name != params.name);
        if artifacts.len() == before {
            return Err(CommandError::new(t!("artifacts-error-unknown", name = params.name.as_str())));
        }
        write_saved(&root, artifacts)?;
        Ok(section(session, &root))
    }
}

pub struct BuildArtifacts;

impl Command for BuildArtifacts {
    type Params = BuildParams;
    type Output = BuildAnswer;
    const ID: &'static str = "build.artifacts.build";
    const TITLE: &'static str = "Build Artifacts";
    const DESCRIPTION: &'static str = "Builds artifacts (`names`; every one Build Artifacts lists when empty) as a \
         run of the Run tool window: a jar's modules compile through their build (Maven compile with the runtime \
         classpath, Gradle classes with the runtime classpath) or javac for a project without one, then the IDE \
         writes the jar with its manifest (Main-Class, Class-Path for libraries copied next to it); a build \
         artifact runs its phase or task. Answers the run's id and the jars, with waitMs the run's summary.";

    fn run(&self, session: &mut Session, params: BuildParams) -> Result<BuildAnswer, CommandError> {
        start(session, &params, Action::Build)
    }
}

pub struct RebuildArtifacts;

impl Command for RebuildArtifacts {
    type Params = BuildParams;
    type Output = BuildAnswer;
    const ID: &'static str = "build.artifacts.rebuild";
    const TITLE: &'static str = "Rebuild Artifacts";
    const DESCRIPTION: &'static str = "Cleans, then builds artifacts as build.artifacts.build does: a jar's old \
         file and the libraries copied next to it are removed and its modules compile from clean (Maven and Gradle \
         `clean` first); a build artifact runs `clean` before its phase or task.";

    fn run(&self, session: &mut Session, params: BuildParams) -> Result<BuildAnswer, CommandError> {
        start(session, &params, Action::Rebuild)
    }
}

pub struct CleanArtifacts;

impl Command for CleanArtifacts {
    type Params = BuildParams;
    type Output = BuildAnswer;
    const ID: &'static str = "build.artifacts.clean";
    const TITLE: &'static str = "Clean Artifacts";
    const DESCRIPTION: &'static str = "Removes what artifacts built: a jar and the libraries its manifest names \
         next to it, at once; a build artifact runs the build's `clean` as a run of the Run tool window. Answers \
         the removed files and the run's id, if any.";

    fn run(&self, session: &mut Session, params: BuildParams) -> Result<BuildAnswer, CommandError> {
        start(session, &params, Action::Clean)
    }
}

fn task_names(task: &BeforeLaunchTask) -> Vec<String> {
    task.options
        .get("artifacts")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

struct BuildBeforeLaunch;

impl TaskType for BuildBeforeLaunch {
    fn spec(&self) -> KindSpec {
        KindSpec {
            id: TASK.to_owned(),
            name_message: "artifacts-before-launch".to_owned(),
            fields: vec![FieldSpec {
                key: "artifacts".to_owned(),
                label_message: "artifacts-before-launch-field".to_owned(),
                kind: FieldKind::Artifacts,
            }],
            defaults: serde_json::json!({ "artifacts": [] }),
        }
    }

    fn start(&self, session: &mut Session, task: &BeforeLaunchTask, _: &TaskStart<'_>) -> Result<Started, String> {
        let names = task_names(task);
        if names.is_empty() {
            return Err(t!("artifacts-before-launch-none"));
        }
        let answer =
            start(session, &BuildParams { names, wait_ms: None }, Action::Build).map_err(|error| error.message)?;
        Ok(answer.run_id.map_or(Started::Done, Started::Run))
    }

    fn summary(&self, _: &mut Session, task: &BeforeLaunchTask) -> String {
        task_names(task).join(", ")
    }
}

pub fn task_type() -> Arc<dyn TaskType> {
    Arc::new(BuildBeforeLaunch)
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ArtifactsCommand)?;
    registry.register(SaveArtifact)?;
    registry.register(DeleteArtifact)?;
    registry.register(BuildArtifacts)?;
    registry.register(RebuildArtifacts)?;
    registry.register(CleanArtifacts)?;
    Ok(())
}

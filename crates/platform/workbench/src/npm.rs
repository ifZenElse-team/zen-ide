pub mod model;

use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{
    CommandLine, Configuration, FieldKind, FieldSpec, KindSpec, OutputParser, PlainParser, RunContext, RunProvider,
    RunTarget, TargetKind,
};

use crate::run::{RunStarted, project_root, wait_for_run};

pub const KIND: &str = "npm";

pub const COMMANDS: [&str; 3] = ["run", "exec", "install"];

pub const PACKAGE_MANAGERS: [&str; 4] = ["npm", "pnpm", "yarn", "bun"];

pub const SETTINGS_FILE: &str = ".zen/npm.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Settings {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    offline: bool,
}

fn load(root: &Path) -> Settings {
    std::fs::read(root.join(SETTINGS_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(session: &mut Session, root: &Path, settings: &Settings) -> Result<(), CommandError> {
    crate::trust::guard_agent_build(session, root)?;
    let path = root.join(SETTINGS_FILE);
    crate::hidden_files::guard_change(session, root, std::slice::from_ref(&path))?;
    if crate::guard::refuses_in(Some(root), &path) {
        return Err(CommandError::new(crate::guard::refusal(&path)));
    }
    if !crate::hidden_files::real_path(&path).starts_with(crate::hidden_files::real_path(root)) {
        return Err(CommandError::new(t!("workbench-npm-settings-outside", path = path.display().to_string())));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| CommandError::new(error.to_string()))?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|error| CommandError::new(error.to_string()))?;
    crate::disk::edit_file(session, &path, |_| Some(format!("{text}\n")))?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NpmOptions {
    pub package_json: String,
    pub command: String,
    pub script: String,
    pub package_manager: String,
}

impl Default for NpmOptions {
    fn default() -> Self {
        Self {
            package_json: "package.json".to_owned(),
            command: "run".to_owned(),
            script: String::new(),
            package_manager: "auto".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NpmRun {
    folders: Option<Vec<PathBuf>>,
}

impl NpmRun {
    pub fn in_folders(folders: Vec<PathBuf>) -> Self {
        Self { folders: Some(folders) }
    }

    fn folders(&self) -> Vec<PathBuf> {
        self.folders.clone().unwrap_or_else(zen_lsp::servers::search_folders)
    }

    pub fn script(package_json: &str, script: &str) -> Configuration {
        let options =
            NpmOptions { package_json: package_json.to_owned(), script: script.to_owned(), ..NpmOptions::default() };
        Configuration {
            name: script.to_owned(),
            kind: KIND.to_owned(),
            options: serde_json::to_value(options).unwrap_or_default(),
            ..Configuration::default()
        }
    }

    pub fn install(package_json: &str, manager: &str) -> Configuration {
        let options = NpmOptions {
            package_json: package_json.to_owned(),
            command: "install".to_owned(),
            ..NpmOptions::default()
        };
        Configuration {
            name: format!("{manager} install"),
            kind: KIND.to_owned(),
            options: serde_json::to_value(options).unwrap_or_default(),
            ..Configuration::default()
        }
    }
}

fn options(configuration: &Configuration) -> NpmOptions {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

fn choice(values: &[&str]) -> FieldKind {
    FieldKind::Choice(values.iter().map(|value| (*value).to_owned()).collect())
}

pub fn package_manager(folder: &Path) -> &'static str {
    let declared = std::fs::read_to_string(folder.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|manifest| manifest.get("packageManager")?.as_str().map(str::to_owned));
    if let Some(declared) = declared {
        let name = declared.split('@').next().unwrap_or_default();
        if let Some(known) = PACKAGE_MANAGERS.iter().find(|known| **known == name) {
            return known;
        }
    }
    let locks: [(&str, &'static str); 5] = [
        ("pnpm-lock.yaml", "pnpm"),
        ("yarn.lock", "yarn"),
        ("bun.lock", "bun"),
        ("bun.lockb", "bun"),
        ("package-lock.json", "npm"),
    ];
    locks.iter().find(|(lock, _)| folder.join(lock).is_file()).map_or("npm", |(_, manager)| manager)
}

pub fn arguments(manager: &str, command: &str, script: &str, arguments: &[String]) -> (String, Vec<String>) {
    let mut args: Vec<String> = Vec::new();
    let program = match (command, manager) {
        ("install", _) => {
            args.push("install".to_owned());
            manager
        }
        ("exec", "npm") => {
            args.extend(["--no".to_owned(), script.to_owned()]);
            "npx"
        }
        ("exec", "pnpm") => {
            args.extend(["exec".to_owned(), script.to_owned()]);
            manager
        }
        _ => {
            args.extend(["run".to_owned(), script.to_owned()]);
            if manager == "npm" && !arguments.is_empty() {
                args.push("--".to_owned());
            }
            manager
        }
    };
    args.extend(arguments.iter().cloned());
    (program.to_owned(), args)
}

fn offline_install(
    manager: &str,
    folder: &Path,
    args: &mut Vec<String>,
    env: &mut Vec<(String, String)>,
) -> Result<(), String> {
    match manager {
        "bun" => return Err(t!("workbench-npm-offline-unsupported", manager = manager)),
        "yarn" if folder.join(".yarnrc.yml").is_file() => {
            env.push(("YARN_ENABLE_NETWORK".to_owned(), "0".to_owned()));
        }
        _ => args.push("--offline".to_owned()),
    }
    Ok(())
}

impl RunProvider for NpmRun {
    fn id(&self) -> &str {
        KIND
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: KIND.to_owned(),
            name_message: "workbench-npm-kind".to_owned(),
            fields: vec![
                field("packageJson", "workbench-npm-field-package-json", FieldKind::Path),
                field("command", "workbench-npm-field-command", choice(&COMMANDS)),
                field("script", "workbench-npm-field-script", FieldKind::Text),
                field(
                    "packageManager",
                    "workbench-npm-field-package-manager",
                    choice(&["auto", "npm", "pnpm", "yarn", "bun"]),
                ),
            ],
            defaults: serde_json::to_value(NpmOptions::default()).unwrap_or_default(),
        }]
    }

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
        if path.file_name().is_none_or(|name| name != "package.json") {
            return Vec::new();
        }
        let relative = path.strip_prefix(&context.root).unwrap_or(path).to_string_lossy().replace('\\', "/");
        scripts(text)
            .into_iter()
            .map(|(name, line)| RunTarget {
                line,
                kind: TargetKind::Main,
                name: name.clone(),
                package: String::new(),
                configuration: Self::script(&relative, &name),
            })
            .collect()
    }

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
        let context = RunContext { root: root.to_owned(), ..RunContext::default() };
        let line = text.get(..offset.min(text.len()))?.matches('\n').count();
        let line = u32::try_from(line).ok()?;
        self.targets(&context, path, text)
            .into_iter()
            .find(|target| target.line == line)
            .map(|target| target.configuration)
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        let options = options(configuration);
        if !COMMANDS.contains(&options.command.as_str()) {
            return Err(t!("workbench-npm-unknown-command", command = options.command.as_str()));
        }
        if options.command != "install" && options.script.trim().is_empty() {
            return Err(t!("workbench-npm-no-script"));
        }
        let package_json = if options.package_json.trim().is_empty() { "package.json" } else { &options.package_json };
        let folder = context.root.join(package_json).parent().map_or_else(|| context.root.clone(), Path::to_owned);
        let manager = match options.package_manager.as_str() {
            "" | "auto" => package_manager(&folder),
            other => PACKAGE_MANAGERS
                .iter()
                .find(|known| **known == other)
                .ok_or_else(|| t!("workbench-npm-unknown-manager", manager = other))?,
        };
        let extra = zen_run::args::split(&configuration.common.program_arguments);
        let (program, mut args) = arguments(manager, &options.command, options.script.trim(), &extra);
        let folders = self.folders();
        let found = zen_lsp::servers::find_executable(&program, &folders)
            .ok_or_else(|| t!("workbench-npm-not-found", program = program.as_str()))?;
        let cwd = match &configuration.common.working_directory {
            Some(folder) if !folder.as_os_str().is_empty() => context.root.join(folder),
            _ => folder,
        };
        let mut env = Vec::new();
        if let Ok(path) = std::env::join_paths(&folders) {
            env.push(("PATH".to_owned(), path.to_string_lossy().into_owned()));
        }
        let offline = load(&context.root).offline;
        if offline {
            env.push(("COREPACK_ENABLE_NETWORK".to_owned(), "0".to_owned()));
        }
        if options.command == "install" && offline {
            offline_install(manager, &cwd, &mut args, &mut env)?;
        }
        env.extend(configuration.common.env.iter().cloned());
        let display = std::iter::once(program.clone()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        Ok(vec![CommandLine { program: found, args, cwd, env, display }])
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }
}

pub fn scripts(text: &str) -> Vec<(String, u32)> {
    let Ok(serde_json::Value::Object(manifest)) = serde_json::from_str::<serde_json::Value>(text) else {
        return Vec::new();
    };
    let Some(scripts) = manifest.get("scripts").and_then(serde_json::Value::as_object) else { return Vec::new() };
    let Some(start) = key_at(text, "scripts", 0) else { return Vec::new() };
    let mut found: Vec<(String, u32, usize)> = scripts
        .keys()
        .filter_map(|name| {
            let at = key_at(text, name, start)?;
            let line = u32::try_from(text[..at].matches('\n').count()).ok()?;
            Some((name.clone(), line, at))
        })
        .collect();
    found.sort_by_key(|(_, _, at)| *at);
    found.into_iter().map(|(name, line, _)| (name, line)).collect()
}

fn key_at(text: &str, key: &str, from: usize) -> Option<usize> {
    let quoted = serde_json::to_string(key).ok()?;
    let mut at = from;
    while let Some(found) = text.get(at..)?.find(&quoted) {
        let start = at + found;
        let after = &text[start + quoted.len()..];
        if after.trim_start().starts_with(':') {
            return Some(start);
        }
        at = start + quoted.len();
    }
    None
}

/// What `npm.project.get` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NpmProject {
    /// The `package.json` files of the project.
    pub packages: Vec<model::Package>,
    /// Whether installs stay off the network.
    pub offline: bool,
}

pub fn read_project(root: &Path) -> NpmProject {
    NpmProject { packages: model::packages(root), offline: load(root).offline }
}

fn project(session: &Session) -> Result<NpmProject, CommandError> {
    let root = project_root(session)?;
    Ok(NpmProject { packages: visible_packages(session, &root), offline: load(&root).offline })
}

fn visible_packages(session: &Session, root: &Path) -> Vec<model::Package> {
    let mut packages = model::packages(root);
    if let Some(patterns) = crate::hidden_files::for_caller(session) {
        packages.retain(|package| !crate::hidden_files::is_hidden(&patterns, Some(root), &package.path));
    }
    packages
}

fn listed(session: &Session, root: &Path, wanted: Option<&Path>) -> Result<model::Package, CommandError> {
    let packages = visible_packages(session, root);
    let found = match wanted {
        Some(path) => {
            let path = root.join(path);
            packages.into_iter().find(|package| package.path == path)
        }
        None => packages.into_iter().next(),
    };
    found.ok_or_else(|| {
        let path = wanted.map(|path| path.display().to_string()).unwrap_or_default();
        CommandError::new(t!("workbench-npm-not-listed", path = path))
    })
}

fn finish(session: &mut Session, started: RunStarted, wait_ms: Option<u64>) -> Result<RunStarted, CommandError> {
    match wait_ms.map(Duration::from_millis) {
        Some(wait) => Ok(RunStarted { summary: Some(wait_for_run(session, started.run_id, wait)?), ..started }),
        None => Ok(started),
    }
}

pub struct GetNpmProject;

impl Command for GetNpmProject {
    type Params = NoParams;
    type Output = NpmProject;
    const ID: &'static str = "npm.project.get";
    const TITLE: &'static str = "npm Packages";
    const DESCRIPTION: &'static str = "Lists the package.json files of the project, as the npm tool window shows them: \
         the root's, its workspaces' and those in folders of the root, each with its scripts, dependencies, installed \
         versions and package manager, and whether installs stay offline. It reads files and runs nothing.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<NpmProject, CommandError> {
        project(session)
    }
}

/// The parameters of `npm.script.run`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunScriptParams {
    /// The script's name.
    pub script: String,
    /// The `package.json`, absolute or relative to the project; the first one listed when absent.
    #[serde(default)]
    pub package_json: Option<PathBuf>,
    /// Waits up to this long for the run to end (at most 10 minutes) and answers its summary.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct RunScript;

impl Command for RunScript {
    type Params = RunScriptParams;
    type Output = RunStarted;
    const ID: &'static str = "npm.script.run";
    const TITLE: &'static str = "Run npm Script";
    const DESCRIPTION: &'static str = "Runs a script of a package.json with the project's package manager, as a \
         double-click in the npm tool window does: a temporary run configuration of kind npm, its output in the Run \
         tool window; returns the run's id, and with waitMs its summary once it ended. Refused in safe mode.";

    fn run(&self, session: &mut Session, params: RunScriptParams) -> Result<RunStarted, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let root = project_root(session)?;
        let package = listed(session, &root, params.package_json.as_deref())?;
        if !package.scripts.iter().any(|script| script.name == params.script) {
            let path = package.relative.as_str();
            return Err(CommandError::new(t!(
                "workbench-npm-no-such-script",
                script = params.script.as_str(),
                path = path
            )));
        }
        let mut configuration = NpmRun::script(&package.relative, &params.script);
        configuration.temporary = true;
        crate::trust::guard_run(session, &root, &configuration)?;
        let started = crate::run::start(session, configuration)?;
        finish(session, started, params.wait_ms)
    }
}

/// The parameters of `npm.dependencies.install`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InstallParams {
    /// The `package.json`, absolute or relative to the project; the first one listed when absent.
    #[serde(default)]
    pub package_json: Option<PathBuf>,
    /// Waits up to this long for the run to end (at most 10 minutes) and answers its summary.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

pub struct InstallDependencies;

impl Command for InstallDependencies {
    type Params = InstallParams;
    type Output = RunStarted;
    const ID: &'static str = "npm.dependencies.install";
    const TITLE: &'static str = "Install npm Dependencies";
    const DESCRIPTION: &'static str = "Installs the dependencies of a package.json with the project's package manager, \
         in the Run tool window. This is the only thing that uses the network here, and only when it runs: with \
         npm.options.set offline it stays off the network. Refused in safe mode, as the packages' own install scripts run.";

    fn run(&self, session: &mut Session, params: InstallParams) -> Result<RunStarted, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let root = project_root(session)?;
        let package = listed(session, &root, params.package_json.as_deref())?;
        let mut configuration = NpmRun::install(&package.relative, &package.manager);
        configuration.temporary = true;
        crate::trust::guard_run(session, &root, &configuration)?;
        let started = crate::run::start(session, configuration)?;
        finish(session, started, params.wait_ms)
    }
}

/// The parameters of `npm.options.set`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OptionsParams {
    /// Offline mode: installs use the package manager's cache and never the network.
    pub offline: bool,
}

pub struct SetOptions;

impl Command for SetOptions {
    type Params = OptionsParams;
    type Output = NpmProject;
    const ID: &'static str = "npm.options.set";
    const TITLE: &'static str = "Toggle npm Offline Mode";
    const DESCRIPTION: &'static str = "Turns the npm tool window's offline mode on or off for this project, in \
         .zen/npm.json: on, installs pass the package manager's offline switch, and a package manager without one \
         refuses to install. Answers the packages.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<NpmProject, CommandError> {
        let root = project_root(session)?;
        if !params.offline && zen_core::macros::by_agent(session) && load(&root).offline {
            return Err(CommandError::new(t!("workbench-npm-online-person")));
        }
        save(session, &root, &Settings { offline: params.offline })?;
        project(session)
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetNpmProject)?;
    registry.register(RunScript)?;
    registry.register(InstallDependencies)?;
    registry.register(SetOptions)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};

    use super::*;

    const MANIFEST: &str = r#"{
  "name": "shop",
  "scripts": {
    "build": "nuxt build",
    "dev": "nuxt dev",
    "postinstall": "nuxt prepare"
  },
  "dependencies": { "dev": "1.0.0", "nuxt": "^3.13.0" }
}
"#;

    fn context(root: &Path) -> RunContext {
        RunContext { root: root.to_owned(), ..RunContext::default() }
    }

    fn fake_tools(dir: &Path) -> PathBuf {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for tool in ["npm", "npx", "pnpm", "yarn", "bun"] {
            let path = bin.join(tool);
            std::fs::write(&path, format!("#!/bin/sh\necho {tool} \"$@\"\n")).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        bin
    }

    #[test]
    fn the_scripts_of_package_json_are_run_targets_on_their_lines() {
        let root = Path::new("/p");
        let targets = NpmRun::default().targets(&context(root), &root.join("web/package.json"), MANIFEST);
        let found: Vec<(&str, u32)> = targets.iter().map(|target| (target.name.as_str(), target.line)).collect();
        assert_eq!(found, [("build", 3), ("dev", 4), ("postinstall", 5)]);
        assert_eq!(targets[1].configuration, NpmRun::script("web/package.json", "dev"));
        assert!(NpmRun::default().targets(&context(root), &root.join("other.json"), MANIFEST).is_empty());
        assert!(NpmRun::default().targets(&context(root), &root.join("package.json"), "{ broken").is_empty());
        let offset = MANIFEST.find("nuxt dev").unwrap();
        let at = NpmRun::default().context(root, &root.join("package.json"), MANIFEST, offset).unwrap();
        assert_eq!(at.name, "dev");
        assert_eq!(NpmRun::default().context(root, &root.join("package.json"), MANIFEST, 0), None);
    }

    #[test]
    fn the_package_manager_is_the_projects_own() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(package_manager(dir.path()), "npm");
        std::fs::write(dir.path().join("yarn.lock"), "").unwrap();
        assert_eq!(package_manager(dir.path()), "yarn");
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").unwrap();
        assert_eq!(package_manager(dir.path()), "pnpm");
        std::fs::write(dir.path().join("package.json"), r#"{ "packageManager": "bun@1.1.0" }"#).unwrap();
        assert_eq!(package_manager(dir.path()), "bun");
        std::fs::write(dir.path().join("package.json"), r#"{ "packageManager": "deno@2" }"#).unwrap();
        assert_eq!(package_manager(dir.path()), "pnpm", "an unknown one falls back to the lock files");
    }

    #[test]
    fn each_package_manager_runs_scripts_and_programs_its_own_way() {
        let args = |list: &[&str]| list.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
        type Case<'a> = (&'a str, &'a str, &'a [&'a str], &'a str, &'a [&'a str]);
        let cases: [Case<'_>; 8] = [
            ("npm", "run", &[], "npm", &["run", "dev"]),
            ("npm", "run", &["--port", "4000"], "npm", &["run", "dev", "--", "--port", "4000"]),
            ("pnpm", "run", &["--port", "4000"], "pnpm", &["run", "dev", "--port", "4000"]),
            ("npm", "exec", &["prepare"], "npx", &["--no", "dev", "prepare"]),
            ("pnpm", "exec", &["prepare"], "pnpm", &["exec", "dev", "prepare"]),
            ("yarn", "exec", &["prepare"], "yarn", &["run", "dev", "prepare"]),
            ("bun", "exec", &[], "bun", &["run", "dev"]),
            ("yarn", "install", &[], "yarn", &["install"]),
        ];
        for (manager, command, extra, program, expected) in cases {
            assert_eq!(
                arguments(manager, command, "dev", &args(extra)),
                (program.to_owned(), args(expected)),
                "{manager} {command}"
            );
        }
    }

    struct Agentless {
        registry: Registry,
        session: Session,
        project: PathBuf,
        _dir: tempfile::TempDir,
    }

    fn session(trusted: bool) -> Agentless {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_tools(dir.path());
        let project = dir.path().canonicalize().unwrap().join("shop");
        std::fs::create_dir_all(project.join("web")).unwrap();
        std::fs::write(project.join("package.json"), MANIFEST).unwrap();
        std::fs::write(project.join("web/package.json"), r#"{ "name": "web", "scripts": { "start": "serve" } }"#)
            .unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(crate::build_tools::BuildEnv::default());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        if trusted {
            crate::trust::trust_session_in_tests(&mut session, &project);
        }
        session
            .state_mut::<crate::run::Runs>()
            .set_providers(vec![Arc::new(NpmRun::in_folders(vec![bin]))], zen_run::RunEnv::default());
        Agentless { registry, session, project, _dir: dir }
    }

    impl Agentless {
        fn run(&mut self, id: &str, params: Value) -> Value {
            self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
        }

        fn output(&mut self, answer: &Value) -> String {
            let read = self.run("run.output.get", json!({ "runId": answer["runId"] }));
            let lines = read["lines"].as_array().unwrap();
            lines.iter().map(|line| line["text"].as_str().unwrap_or_default()).collect::<Vec<_>>().join("\n")
        }
    }

    #[test]
    fn the_project_lists_its_packages_and_scripts_for_agents() {
        let mut fixture = session(true);
        let project = fixture.run("npm.project.get", json!({}));
        assert_eq!(project["offline"], false);
        assert_eq!(project["packages"][0]["name"], "shop");
        assert_eq!(project["packages"][0]["scripts"][1]["command"], "nuxt dev");
        assert_eq!(project["packages"][0]["dependencies"][1]["name"], "nuxt");
        assert_eq!(project["packages"][1]["relative"], "web/package.json");
    }

    #[test]
    fn a_script_runs_in_its_package_and_an_unknown_one_is_refused() {
        let mut fixture = session(true);
        let done = fixture
            .run("npm.script.run", json!({ "script": "start", "packageJson": "web/package.json", "waitMs": 60000 }));
        assert!(fixture.output(&done).contains("npm run start"), "{done}");
        let error = fixture.registry.dispatch(&mut fixture.session, "npm.script.run", json!({ "script": "nope" }));
        assert!(error.unwrap_err().to_string().contains("no script"));
        let error = fixture.registry.dispatch(
            &mut fixture.session,
            "npm.script.run",
            json!({ "script": "dev", "packageJson": "other/package.json" }),
        );
        assert!(error.unwrap_err().to_string().contains("is not a package.json"));
    }

    #[test]
    fn install_stays_off_the_network_in_offline_mode() {
        let mut fixture = session(true);
        let online = fixture.run("npm.dependencies.install", json!({ "waitMs": 60000 }));
        let printed = fixture.output(&online);
        assert!(printed.contains("npm install") && !printed.contains("--offline"), "{printed}");
        assert_eq!(fixture.run("npm.options.set", json!({ "offline": true }))["offline"], true);
        let offline = fixture.run("npm.dependencies.install", json!({ "waitMs": 60000 }));
        assert!(fixture.output(&offline).contains("npm install --offline"));
        assert!(fixture.project.join(SETTINGS_FILE).is_file());
    }

    #[test]
    fn in_offline_mode_corepack_fetches_no_package_manager_for_any_command() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_tools(dir.path());
        std::fs::write(dir.path().join("package.json"), MANIFEST).unwrap();
        let provider = NpmRun::in_folders(vec![bin]);
        let corepack = |configuration: &Configuration| {
            let lines = provider.command_lines(configuration, &context(dir.path())).unwrap();
            lines[0].env.iter().any(|(name, value)| name == "COREPACK_ENABLE_NETWORK" && value == "0")
        };
        let (script, install) = (NpmRun::script("package.json", "dev"), NpmRun::install("package.json", "npm"));
        assert!(!corepack(&script) && !corepack(&install), "online nothing is said");
        std::fs::create_dir_all(dir.path().join(".zen")).unwrap();
        std::fs::write(dir.path().join(SETTINGS_FILE), r#"{ "offline": true }"#).unwrap();
        assert!(corepack(&script) && corepack(&install));
    }

    #[test]
    fn offline_mode_goes_the_way_of_each_package_manager() {
        let dir = tempfile::tempdir().unwrap();
        let (mut args, mut env) = (Vec::new(), Vec::new());
        offline_install("pnpm", dir.path(), &mut args, &mut env).unwrap();
        offline_install("yarn", dir.path(), &mut args, &mut env).unwrap();
        assert_eq!(args, ["--offline", "--offline"]);
        assert!(env.is_empty());
        std::fs::write(dir.path().join(".yarnrc.yml"), "").unwrap();
        offline_install("yarn", dir.path(), &mut args, &mut env).unwrap();
        assert_eq!(args.len(), 2, "Yarn with .yarnrc.yml is told through its environment");
        assert_eq!(env, [("YARN_ENABLE_NETWORK".to_owned(), "0".to_owned())]);
        assert!(offline_install("bun", dir.path(), &mut args, &mut env).is_err(), "never a silent download");
    }

    #[test]
    fn an_agent_is_not_shown_a_package_json_hidden_from_it_and_the_settings_file_keeps_its_guards() {
        let mut fixture = session(true);
        fixture
            .session
            .state_mut::<crate::hidden_files::HiddenFromAgents>()
            .set(&["web/**".to_owned(), ".zen/".to_owned()]);
        let (listed, script, set) = zen_core::macros::as_agent(&mut fixture.session, |session| {
            (
                fixture.registry.dispatch(session, "npm.project.get", json!({})).unwrap(),
                fixture.registry.dispatch(
                    session,
                    "npm.script.run",
                    json!({ "script": "start", "packageJson": "web/package.json" }),
                ),
                fixture.registry.dispatch(session, "npm.options.set", json!({ "offline": true })),
            )
        });
        assert_eq!(listed["packages"].as_array().unwrap().len(), 1, "{listed}");
        assert!(script.is_err(), "{script:?}");
        assert!(format!("{set:?}").contains(crate::hidden_files::REDACTED_CODE), "{set:?}");
        assert!(!fixture.project.join(SETTINGS_FILE).exists());
        assert_eq!(fixture.run("npm.project.get", json!({}))["packages"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn in_safe_mode_the_scripts_are_listed_and_nothing_runs() {
        let mut fixture = session(false);
        assert_eq!(fixture.run("npm.project.get", json!({}))["packages"][0]["name"], "shop", "reading runs nothing");
        for (command, params) in
            [("npm.script.run", json!({ "script": "dev" })), ("npm.dependencies.install", json!({}))]
        {
            let error = fixture.registry.dispatch(&mut fixture.session, command, params).unwrap_err();
            assert!(error.to_string().contains("safe mode"), "{command}: {error}");
        }
        assert!(!fixture.project.join(SETTINGS_FILE).exists());
    }

    #[test]
    fn an_agent_does_not_take_a_project_back_online() {
        let mut fixture = session(true);
        fixture.run("npm.options.set", json!({ "offline": true }));
        let (registry, session) = (&fixture.registry, &mut fixture.session);
        let back = zen_core::macros::as_agent(session, |session| {
            assert!(registry.dispatch(session, "npm.options.set", json!({ "offline": true })).is_ok());
            registry.dispatch(session, "npm.options.set", json!({ "offline": false }))
        });
        assert!(back.is_err());
        assert_eq!(fixture.run("npm.project.get", json!({}))["offline"], true);
        assert_eq!(fixture.run("npm.options.set", json!({ "offline": false }))["offline"], false);
    }

    #[test]
    fn offline_mode_is_kept_in_local_history() {
        let mut fixture = session(true);
        let history = fixture.project.parent().unwrap().join("local-history");
        let limits = crate::local_history::Limits::default();
        fixture.session.set_state(crate::local_history::LocalHistory::open(history, limits));
        fixture.run("npm.options.set", json!({ "offline": true }));
        let path = fixture.project.join(SETTINGS_FILE);
        let versions = fixture.run("localHistory.versions.list", json!({ "path": path }));
        assert!(!versions["versions"].as_array().unwrap().is_empty(), "{versions}");
    }

    #[cfg(unix)]
    #[test]
    fn a_settings_folder_linked_out_of_the_project_is_not_written() {
        let mut fixture = session(true);
        let outside = fixture.project.parent().unwrap().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, fixture.project.join(".zen")).unwrap();
        let error = fixture.registry.dispatch(&mut fixture.session, "npm.options.set", json!({ "offline": true }));
        assert!(error.is_err());
        assert!(!outside.join("npm.json").exists());
    }

    #[test]
    fn a_configuration_runs_in_the_folder_of_its_package_json() {
        let dir = tempfile::tempdir().unwrap();
        let bin = fake_tools(dir.path());
        let web = dir.path().join("web");
        std::fs::create_dir_all(&web).unwrap();
        std::fs::write(web.join("pnpm-lock.yaml"), "").unwrap();
        let provider = NpmRun::in_folders(vec![bin.clone()]);
        let mut configuration = NpmRun::script("web/package.json", "dev");
        configuration.common.program_arguments = "--host 'a b'".to_owned();
        let lines = provider.command_lines(&configuration, &context(dir.path())).unwrap();
        assert_eq!(lines[0].program, bin.join("pnpm"));
        assert_eq!(lines[0].args, ["run", "dev", "--host", "a b"]);
        assert_eq!(lines[0].cwd, web);
        assert_eq!(lines[0].display, "pnpm run dev --host a b");
        assert!(lines[0].env.iter().any(|(name, value)| name == "PATH" && value.contains(&*bin.to_string_lossy())));

        configuration.options["script"] = serde_json::json!(" ");
        assert!(provider.command_lines(&configuration, &context(dir.path())).is_err(), "a script is needed");
        configuration.options["command"] = serde_json::json!("publish");
        assert!(provider.command_lines(&configuration, &context(dir.path())).is_err());
        let missing = NpmRun::in_folders(vec![dir.path().join("nowhere")]);
        let error = missing.command_lines(&NpmRun::script("package.json", "dev"), &context(dir.path())).unwrap_err();
        assert!(error.contains("npm"), "{error}");
    }
}

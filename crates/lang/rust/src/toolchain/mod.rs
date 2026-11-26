pub mod rust;

use std::collections::HashMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::SystemTime;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

pub use rust::{Environment, RustAnalyzer, RustToolchain};

use crate::analyzer::{self, AnalyzerSettings};

#[derive(Clone, Default)]
pub struct Toolchain {
    inner: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    file: Option<PathBuf>,
    chosen: Option<PathBuf>,
    rust_analyzer_path: Option<PathBuf>,
    analyzer: Value,
    found: Option<Found>,
    env: Option<Environment>,
    generation: u64,
    projects: HashMap<PathBuf, (ProjectKey, ProjectRust)>,
    install: Install,
    stdlib_install: Install,
}

#[derive(Clone)]
struct Found {
    toolchains: Vec<RustToolchain>,
    sysroot: Option<PathBuf>,
    rust_analyzer: Option<RustAnalyzer>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    toolchain: Option<PathBuf>,
    rust_analyzer_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "is_empty")]
    rust_analyzer_settings: Value,
}

fn is_empty(settings: &Value) -> bool {
    settings.as_object().is_none_or(serde_json::Map::is_empty)
}

/// An installation with rustup.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Install {
    /// Not started.
    #[default]
    Idle,
    /// `rustup component add` is running; the last line it printed, if any.
    Running {
        /// The last line rustup printed.
        line: Option<String>,
    },
    /// Installed.
    Done,
    /// It failed: what rustup said.
    Failed {
        /// The end of what rustup printed.
        message: String,
    },
}

/// The Rust toolchain as the IDE sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RustInfo {
    /// Every toolchain found: rustup's `~/.cargo/bin`, Homebrew's, those on `PATH`, and the one
    /// chosen by hand.
    pub toolchains: Vec<RustToolchain>,
    /// The toolchain in use: the one chosen, else the first found.
    pub toolchain: Option<RustToolchain>,
    /// The toolchain folder chosen before that is gone, when it is: `toolchain` is then the first found.
    pub missing_toolchain: Option<PathBuf>,
    /// rustup, when the toolchain in use is rustup's.
    pub rustup: Option<PathBuf>,
    /// The standard library's sources of the toolchain in use (rustup's `rust-src`), whether or not installed.
    pub stdlib: Option<PathBuf>,
    /// Whether those sources are installed.
    pub stdlib_installed: bool,
    /// The installation of the standard library's sources started from the IDE.
    pub stdlib_install: Install,
    /// rust-analyzer, if found.
    pub rust_analyzer: Option<RustAnalyzer>,
    /// The rust-analyzer chosen by hand, when one is.
    pub rust_analyzer_path: Option<PathBuf>,
    /// The installation of rust-analyzer started from the IDE.
    pub install: Install,
    /// The user's rust-analyzer settings over the plug-in's defaults (`toolchain.rustAnalyzer.settings`
    /// tells them all, with the project's).
    pub rust_analyzer_settings: Value,
}

/// What a project's folder makes of the toolchain in use: a `rust-toolchain.toml` there may choose
/// another rustup toolchain.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRust {
    /// `cargo`, when a toolchain is found.
    pub cargo: Option<PathBuf>,
    /// The version of `rustc` in the project, e.g. `1.99.1`.
    pub version: Option<String>,
    /// The sysroot of `rustc` in the project.
    pub sysroot: Option<PathBuf>,
    /// The host's target triple, e.g. `aarch64-apple-darwin`.
    pub host: Option<String>,
    /// The environment that `cargo` runs with: the toolchain's folder first on `PATH`.
    pub env: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectKey {
    generation: u64,
    files: Vec<Option<SystemTime>>,
}

impl ProjectKey {
    fn of(generation: u64, root: &Path) -> Self {
        let files = ["rust-toolchain.toml", "rust-toolchain"]
            .iter()
            .map(|name| std::fs::metadata(root.join(name)).and_then(|meta| meta.modified()).ok())
            .collect();
        Self { generation, files }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Component {
    RustAnalyzer,
    RustSrc,
}

impl Component {
    fn name(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust-analyzer",
            Self::RustSrc => "rust-src",
        }
    }

    fn state(self, state: &mut State) -> &mut Install {
        match self {
            Self::RustAnalyzer => &mut state.install,
            Self::RustSrc => &mut state.stdlib_install,
        }
    }

    fn installed(self, info: &RustInfo) -> bool {
        match self {
            Self::RustAnalyzer => info.rust_analyzer.is_some(),
            Self::RustSrc => info.stdlib_installed,
        }
    }
}

impl Toolchain {
    pub fn load(file: Option<PathBuf>) -> Self {
        let saved: Saved = file
            .as_ref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            inner: Arc::new(Mutex::new(State {
                file,
                chosen: saved.toolchain,
                rust_analyzer_path: saved.rust_analyzer_path,
                analyzer: saved.rust_analyzer_settings,
                ..State::default()
            })),
        }
    }

    pub fn with_environment(file: Option<PathBuf>, env: Environment) -> Self {
        let toolchain = Self::load(file);
        toolchain.state().env = Some(env);
        toolchain
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn cargo_home(&self) -> Option<PathBuf> {
        let env = self.environment();
        env.cargo_home.or_else(|| env.home.map(|home| home.join(".cargo")))
    }

    fn environment(&self) -> Environment {
        self.state().env.clone().unwrap_or_else(Environment::of_process)
    }

    fn found(&self) -> Found {
        if let Some(found) = self.state().found.clone() {
            return found;
        }
        let env = self.environment();
        let mut toolchains = rust::find_toolchains(&env);
        let (chosen, analyzer_path) = {
            let state = self.state();
            (state.chosen.clone(), state.rust_analyzer_path.clone())
        };
        if let Some(chosen) = &chosen
            && !toolchains.iter().any(|known| known.bin == *chosen)
            && let Some(toolchain) = rust::toolchain_at(chosen)
        {
            toolchains.push(toolchain);
        }
        let in_use = pick(&toolchains, chosen.as_deref());
        let sysroot = in_use.and_then(|toolchain| rust::sysroot(&toolchain.rustc(), env.home.as_deref()));
        let rust_analyzer = match analyzer_path.filter(|path| path.is_file()) {
            Some(path) => Some(RustAnalyzer { version: rust::rust_analyzer_version(&path), path }),
            None => rust::find_rust_analyzer(in_use, &env.folders(), env.home.as_deref()),
        };
        let found = Found { toolchains, sysroot, rust_analyzer };
        self.state().found = Some(found.clone());
        found
    }

    pub fn rust(&self) -> RustInfo {
        let found = self.found();
        let state = self.state();
        let toolchain = pick(&found.toolchains, state.chosen.as_deref()).cloned();
        let missing_toolchain =
            state.chosen.clone().filter(|chosen| !found.toolchains.iter().any(|known| known.bin == *chosen));
        let stdlib = found.sysroot.as_deref().map(rust::library_sources);
        RustInfo {
            rustup: toolchain.as_ref().and_then(RustToolchain::rustup),
            stdlib_installed: stdlib.as_ref().is_some_and(|stdlib| stdlib.is_dir()),
            stdlib,
            stdlib_install: state.stdlib_install.clone(),
            toolchains: found.toolchains,
            toolchain,
            missing_toolchain,
            rust_analyzer: found.rust_analyzer,
            rust_analyzer_path: state.rust_analyzer_path.clone(),
            install: state.install.clone(),
            rust_analyzer_settings: state.analyzer.clone(),
        }
    }

    pub fn in_use(&self) -> Option<RustToolchain> {
        self.rust().toolchain
    }

    pub fn generation(&self) -> u64 {
        self.state().generation
    }

    fn tools_in_use(&self) -> (Option<RustToolchain>, Option<RustAnalyzer>) {
        let info = self.rust();
        (info.toolchain, info.rust_analyzer)
    }

    fn tracked<T>(&self, change: impl FnOnce(&Self) -> T) -> T {
        let cached = self.state().found.is_some();
        let before = cached.then(|| self.tools_in_use());
        let result = change(self);
        if before.is_some_and(|before| before != self.tools_in_use()) {
            self.state().generation += 1;
        }
        result
    }

    pub fn refresh(&self) {
        self.tracked(|toolchain| {
            let mut state = toolchain.state();
            state.found = None;
            state.projects.clear();
        });
    }

    pub fn choose(&self, bin: &Path) -> Result<(), String> {
        if !rust::is_toolchain(bin) {
            return Err(t!("rust-toolchain-error-not-toolchain", path = bin.display().to_string()));
        }
        self.tracked(|toolchain| {
            let mut state = toolchain.state();
            state.chosen = Some(bin.to_owned());
            state.found = None;
            state.projects.clear();
            save(&state)
        })
    }

    pub fn choose_rust_analyzer(&self, path: Option<&Path>) -> Result<(), String> {
        if let Some(path) = path {
            let shown = path.display().to_string();
            if !path.is_file() {
                return Err(t!("rust-toolchain-error-no-file", path = shown));
            }
            if rust::rust_analyzer_version(path).is_none() {
                return Err(t!("rust-toolchain-error-not-rust-analyzer", path = shown));
            }
        }
        self.tracked(|toolchain| {
            let mut state = toolchain.state();
            state.rust_analyzer_path = path.map(Path::to_owned);
            state.found = None;
            save(&state)
        })
    }

    pub fn install_rust_analyzer(&self) -> Result<(), String> {
        self.install(Component::RustAnalyzer)
    }

    pub fn install_stdlib(&self) -> Result<(), String> {
        self.install(Component::RustSrc)
    }

    fn install(&self, component: Component) -> Result<(), String> {
        let rustup = self.rust().rustup.ok_or_else(|| t!("rust-toolchain-error-no-rustup"))?;
        {
            let mut state = self.state();
            let install = component.state(&mut state);
            if matches!(install, Install::Running { .. }) {
                return Ok(());
            }
            *install = Install::Running { line: None };
        }
        let home = self.environment().home;
        let toolchain = self.clone();
        std::thread::spawn(move || {
            let mut command = std::process::Command::new(&rustup);
            command.args(["component", "add", component.name()]);
            if let Some(home) = &home {
                command.current_dir(home);
            }
            let outcome = run_install(command, |line| {
                *component.state(&mut toolchain.state()) = Install::Running { line: Some(line.to_owned()) };
            });
            toolchain.refresh();
            let installed = component.installed(&toolchain.rust());
            *component.state(&mut toolchain.state()) = match outcome {
                Ok(()) if installed => Install::Done,
                Ok(()) => Install::Failed {
                    message: t!("rust-toolchain-error-installed-not-found", component = component.name()),
                },
                Err(message) => Install::Failed { message },
            };
        });
        Ok(())
    }

    pub fn analyzer_settings(&self, defaults: &Value, root: Option<&Path>) -> AnalyzerSettings {
        let ide = self.state().analyzer.clone();
        let ide = if ide.is_object() { ide } else { Value::Object(serde_json::Map::new()) };
        let (project, problem) = match root.map(analyzer::read_project) {
            Some(Ok(project)) => (project, None),
            Some(Err(problem)) => (Value::Object(serde_json::Map::new()), Some(problem)),
            None => (Value::Object(serde_json::Map::new()), None),
        };
        AnalyzerSettings {
            effective: analyzer::effective(defaults, &ide, &project),
            defaults: defaults.clone(),
            ide,
            project_file: root.map(|root| root.join(analyzer::PROJECT_FILE)),
            project,
            problem,
        }
    }

    pub fn configure_analyzer(&self, key: &str, value: Option<Value>, root: Option<&Path>) -> Result<(), String> {
        let value = value.map(analyzer::coerce);
        match root {
            Some(root) => {
                let mut project = analyzer::read_project(root)?;
                analyzer::set(&mut project, key, value)?;
                analyzer::write_project(root, &project)?;
                self.state().generation += 1;
                Ok(())
            }
            None => {
                let mut state = self.state();
                analyzer::set(&mut state.analyzer, key, value)?;
                state.generation += 1;
                save(&state)
            }
        }
    }

    pub fn project(&self, root: &Path) -> ProjectRust {
        let key = ProjectKey::of(self.generation(), root);
        if let Some((known, project)) = self.state().projects.get(root)
            && *known == key
        {
            return project.clone();
        }
        let project = match self.in_use() {
            Some(toolchain) => {
                let rustc = toolchain.rustc();
                ProjectRust {
                    cargo: Some(toolchain.cargo()),
                    version: rust::rustc_version(&rustc, Some(root)),
                    sysroot: rust::sysroot(&rustc, Some(root)),
                    host: rust::host(&rustc, Some(root)),
                    env: command_env(&toolchain.bin, std::env::var_os("PATH").as_deref()),
                }
            }
            None => ProjectRust::default(),
        };
        self.state().projects.insert(root.to_owned(), (key, project.clone()));
        project
    }
}

impl zen_lsp::ServerTools for Toolchain {
    fn locate(&self, server: &zen_lsp::ServerDeclaration) -> Option<PathBuf> {
        if server.command == "rust-analyzer" {
            return self.rust().rust_analyzer.map(|found| found.path);
        }
        let mut folders: Vec<PathBuf> = self.in_use().map(|toolchain| toolchain.bin).into_iter().collect();
        folders.extend(self.environment().folders());
        zen_lsp::servers::find_executable(&server.command, &folders)
            .or_else(|| zen_lsp::servers::find_executable(&server.command, &zen_lsp::servers::search_folders()))
    }

    fn environment(&self, _server: &zen_lsp::ServerDeclaration) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        let Some(toolchain) = self.in_use() else { return zen_lsp::servers::default_environment() };
        let inherited = std::env::join_paths(zen_lsp::servers::search_folders()).ok();
        command_env(&toolchain.bin, inherited.as_deref())
            .into_iter()
            .map(|(name, value)| (name.into(), value.into()))
            .collect()
    }

    fn install_command(
        &self,
        server: &zen_lsp::ServerDeclaration,
    ) -> Result<std::process::Command, zen_lsp::NotInstallable> {
        let Some(recipe) = server.install.as_ref().filter(|recipe| !recipe.package.is_empty()) else {
            return zen_lsp::servers::default_install_command(server);
        };
        let info = self.rust();
        let (program, args): (PathBuf, Vec<&str>) = match recipe.kind {
            zen_lsp::InstallKind::Rustup => {
                let rustup = info.rustup.ok_or(zen_lsp::NotInstallable::ToolMissing(zen_lsp::InstallKind::Rustup))?;
                (rustup, vec!["component", "add", recipe.package.as_str()])
            }
            zen_lsp::InstallKind::Cargo => {
                let toolchain =
                    info.toolchain.ok_or(zen_lsp::NotInstallable::ToolMissing(zen_lsp::InstallKind::Cargo))?;
                (toolchain.cargo(), vec!["install", "--locked", recipe.package.as_str()])
            }
            _ => return zen_lsp::servers::default_install_command(server),
        };
        let mut command = std::process::Command::new(program);
        command.args(args).envs(zen_lsp::ServerTools::environment(self, server));
        if let Some(home) = self.environment().home {
            command.current_dir(home);
        }
        Ok(command)
    }

    fn refresh(&self) {
        Toolchain::refresh(self);
    }

    fn initialization_options(&self, server: &zen_lsp::ServerDeclaration, root: &Path) -> Value {
        if server.command != "rust-analyzer" {
            return server.initialization_options.clone();
        }
        self.analyzer_settings(&server.initialization_options, Some(root)).effective
    }
}

fn pick<'a>(toolchains: &'a [RustToolchain], chosen: Option<&Path>) -> Option<&'a RustToolchain> {
    chosen.and_then(|chosen| toolchains.iter().find(|toolchain| toolchain.bin == chosen)).or_else(|| toolchains.first())
}

fn save(state: &State) -> Result<(), String> {
    let Some(file) = &state.file else { return Ok(()) };
    let saved = Saved {
        toolchain: state.chosen.clone(),
        rust_analyzer_path: state.rust_analyzer_path.clone(),
        rust_analyzer_settings: state.analyzer.clone(),
    };
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    std::fs::write(file, serde_json::to_vec_pretty(&saved).unwrap_or_default())
        .map_err(|error| t!("rust-toolchain-error-save", file = file.display().to_string(), error = error.to_string()))
}

fn run_install(mut command: std::process::Command, mut line: impl FnMut(&str)) -> Result<(), String> {
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut last = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        for text in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
            line(&text);
            last.push(text);
            if last.len() > 20 {
                last.remove(0);
            }
        }
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() { Ok(()) } else { Err(last.join("\n")) }
}

pub fn command_env(bin: &Path, inherited: Option<&std::ffi::OsStr>) -> Vec<(String, String)> {
    let mut folders = vec![bin.to_owned()];
    folders.extend(inherited.map(|value| std::env::split_paths(value).collect::<Vec<_>>()).unwrap_or_default());
    folders.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    let mut unique: Vec<PathBuf> = Vec::new();
    for folder in folders {
        if !unique.contains(&folder) {
            unique.push(folder);
        }
    }
    let path = std::env::join_paths(unique).map(|path| path.to_string_lossy().into_owned()).unwrap_or_default();
    vec![("PATH".to_owned(), path)]
}

const STARTS_A_PROGRAM: [&str; 9] = [
    "overrideCommand",
    "extraEnv",
    "server",
    "\"path\"",
    "runnables.command",
    "\"command\"",
    "sysroot",
    "invocation",
    "rustfmt",
];

fn person_only(session: &Session) -> Result<(), CommandError> {
    if zen_core::macros::by_agent(session) {
        return Err(CommandError::with_code("agentNotAllowed", t!("rust-toolchain-error-agent")));
    }
    Ok(())
}

fn starts_a_program(key: &str, value: &Value) -> bool {
    let text = format!("{key} {value}");
    STARTS_A_PROGRAM.iter().any(|mark| text.contains(mark))
}

fn toolchain(session: &Session) -> Result<&Toolchain, CommandError> {
    session.state::<Toolchain>().ok_or_else(|| CommandError::new(t!("rust-toolchain-error-none")))
}

pub struct RustInfoCommand;

impl Command for RustInfoCommand {
    type Params = NoParams;
    type Output = RustInfo;
    const ID: &'static str = "toolchain.rust.info";
    const TITLE: &'static str = "Rust Toolchain";
    const DESCRIPTION: &'static str = "The Rust toolchains found on this computer (folders holding cargo and rustc, with \
         rustc's version), the one in use, rustup, the standard library's sources, rust-analyzer and the state of \
         their installations.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RustInfo, CommandError> {
        Ok(toolchain(session)?.rust())
    }
}

pub struct RustRefresh;

impl Command for RustRefresh {
    type Params = NoParams;
    type Output = RustInfo;
    const ID: &'static str = "toolchain.rust.refresh";
    const TITLE: &'static str = "Refresh Rust Toolchain";
    const DESCRIPTION: &'static str =
        "Looks for Rust toolchains and rust-analyzer again, after installing or removing one.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RustInfo, CommandError> {
        let toolchain = toolchain(session)?;
        toolchain.refresh();
        Ok(toolchain.rust())
    }
}

/// The parameters of `toolchain.rust.choose`.
#[derive(Deserialize, JsonSchema)]
pub struct ChooseParams {
    /// The toolchain's folder, holding cargo and rustc, e.g. ~/.cargo/bin.
    pub bin: PathBuf,
}

pub struct RustChoose;

impl Command for RustChoose {
    type Params = ChooseParams;
    type Output = RustInfo;
    const ID: &'static str = "toolchain.rust.choose";
    const TITLE: &'static str = "Choose Rust Toolchain";
    const DESCRIPTION: &'static str = "Uses the Rust toolchain in a folder holding cargo and rustc (one found, or any \
         other) and remembers the choice.";

    fn run(&self, session: &mut Session, params: ChooseParams) -> Result<RustInfo, CommandError> {
        person_only(session)?;
        let toolchain = toolchain(session)?;
        toolchain.choose(&params.bin).map_err(CommandError::new)?;
        Ok(toolchain.rust())
    }
}

/// The parameters of `toolchain.rustAnalyzer.choose`.
#[derive(Deserialize, JsonSchema)]
pub struct RustAnalyzerChooseParams {
    /// The rust-analyzer executable to use; absent or null goes back to the one found.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

pub struct RustAnalyzerChoose;

impl Command for RustAnalyzerChoose {
    type Params = RustAnalyzerChooseParams;
    type Output = RustInfo;
    const ID: &'static str = "toolchain.rustAnalyzer.choose";
    const TITLE: &'static str = "Choose rust-analyzer";
    const DESCRIPTION: &'static str = "Uses the rust-analyzer at a path instead of the one found, once it answers \
         `rust-analyzer --version`, and remembers the choice; null goes back to the one found.";

    fn run(&self, session: &mut Session, params: RustAnalyzerChooseParams) -> Result<RustInfo, CommandError> {
        person_only(session)?;
        let toolchain = toolchain(session)?;
        toolchain.choose_rust_analyzer(params.path.as_deref()).map_err(CommandError::new)?;
        Ok(toolchain.rust())
    }
}

pub struct RustAnalyzerInstall;

impl Command for RustAnalyzerInstall {
    type Params = NoParams;
    type Output = RustInfo;
    const ID: &'static str = "toolchain.rustAnalyzer.install";
    const TITLE: &'static str = "Install rust-analyzer";
    const DESCRIPTION: &'static str = "Installs rust-analyzer into the toolchain in use with `rustup component add \
         rust-analyzer` in the background, only when called; toolchain.rust.info reports the progress as install.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RustInfo, CommandError> {
        let toolchain = toolchain(session)?;
        toolchain.install_rust_analyzer().map_err(CommandError::new)?;
        Ok(toolchain.rust())
    }
}

pub struct RustSrcInstall;

impl Command for RustSrcInstall {
    type Params = NoParams;
    type Output = RustInfo;
    const ID: &'static str = "toolchain.rustSrc.install";
    const TITLE: &'static str = "Download Standard Library Sources";
    const DESCRIPTION: &'static str = "Installs the standard library's sources into the toolchain in use with `rustup \
         component add rust-src` in the background, only when called; toolchain.rust.info reports the progress as \
         stdlibInstall.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RustInfo, CommandError> {
        let toolchain = toolchain(session)?;
        toolchain.install_stdlib().map_err(CommandError::new)?;
        Ok(toolchain.rust())
    }
}

pub fn analyzer_defaults() -> Value {
    zen_lsp::ServerDeclaration::parse_list(crate::LANGUAGE_SERVERS)
        .ok()
        .and_then(|servers| servers.into_iter().find(|server| server.id == "rust-analyzer"))
        .map(|server| server.initialization_options)
        .unwrap_or_default()
}

/// The parameters of `toolchain.rustAnalyzer.settings`.
#[derive(Deserialize, JsonSchema)]
pub struct AnalyzerSettingsParams {
    /// A project's root folder, absolute, to tell its own settings too.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

pub struct RustAnalyzerSettings;

impl Command for RustAnalyzerSettings {
    type Params = AnalyzerSettingsParams;
    type Output = AnalyzerSettings;
    const ID: &'static str = "toolchain.rustAnalyzer.settings";
    const TITLE: &'static str = "rust-analyzer Settings";
    const DESCRIPTION: &'static str = "rust-analyzer's settings, as its manual names them without the `rust-analyzer.` \
         prefix, nested as objects: the plug-in's defaults (tuned for large workspaces), the user's, the project's own \
         .zen/rust-analyzer.json when `project` is given, and what rust-analyzer starts with there (`effective`: the \
         project's over the user's over the defaults).";

    fn run(&self, session: &mut Session, params: AnalyzerSettingsParams) -> Result<AnalyzerSettings, CommandError> {
        Ok(toolchain(session)?.analyzer_settings(&analyzer_defaults(), params.project.as_deref()))
    }
}

/// The parameters of `toolchain.rustAnalyzer.configure`.
#[derive(Deserialize, JsonSchema)]
pub struct AnalyzerConfigureParams {
    /// The setting, dotted, e.g. `check.workspace`, `cachePriming.enable` or `files.exclude`.
    pub key: String,
    /// Its value; null removes it, back to what is under it (the user's, then the defaults). Text that
    /// reads as true, false or a number is taken as that.
    #[serde(default)]
    pub value: Value,
    /// A project's root folder, absolute: the setting goes into its .zen/rust-analyzer.json, which
    /// the repository carries, instead of the user's settings.
    #[serde(default)]
    pub project: Option<PathBuf>,
}

pub struct RustAnalyzerConfigure;

impl Command for RustAnalyzerConfigure {
    type Params = AnalyzerConfigureParams;
    type Output = AnalyzerSettings;
    const ID: &'static str = "toolchain.rustAnalyzer.configure";
    const TITLE: &'static str = "Configure rust-analyzer";
    const DESCRIPTION: &'static str = "Sets one of rust-analyzer's settings (a dotted key, e.g. check.workspace) for the \
         user, or with `project` in that project's .zen/rust-analyzer.json; null removes it. rust-analyzer restarts \
         with it in every window. Answers the settings as toolchain.rustAnalyzer.settings does.";

    fn run(&self, session: &mut Session, params: AnalyzerConfigureParams) -> Result<AnalyzerSettings, CommandError> {
        if starts_a_program(&params.key, &params.value) {
            person_only(session)?;
        }
        let toolchain = toolchain(session)?;
        let value = Some(params.value).filter(|value| !value.is_null());
        toolchain.configure_analyzer(&params.key, value, params.project.as_deref()).map_err(CommandError::new)?;
        Ok(toolchain.analyzer_settings(&analyzer_defaults(), params.project.as_deref()))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(RustInfoCommand)?;
    registry.register(RustRefresh)?;
    registry.register(RustChoose)?;
    registry.register(RustAnalyzerChoose)?;
    registry.register(RustAnalyzerInstall)?;
    registry.register(RustSrcInstall)?;
    registry.register(RustAnalyzerSettings)?;
    registry.register(RustAnalyzerConfigure)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use zen_lsp::ServerTools;

    use super::*;

    #[cfg(unix)]
    fn script(path: &Path, text: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{text}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    fn fake_toolchain(bin: &Path, version: &str, sysroot: &Path) {
        script(&bin.join("cargo"), "echo cargo");
        script(
            &bin.join("rustc"),
            &format!(
                "case \"$1\" in\n--version) echo 'rustc {version} (abc 2026-10-13)';;\n-vV) echo 'rustc {version}'; echo 'host: aarch64-apple-darwin';;\n--print) echo '{}';;\nesac",
                sysroot.display()
            ),
        );
    }

    #[cfg(unix)]
    fn fake_rustup(bin: &Path, installed: &Path) {
        let analyzer = installed.display();
        script(
            &bin.join("rustup"),
            &format!(
                "case \"$1\" in\nwhich) if [ -f '{analyzer}' ]; then echo '{analyzer}'; else echo 'error: unknown binary' >&2; exit 1; fi;;\ncomponent) echo 'info: downloading component' >&2; mkdir -p \"$(dirname '{analyzer}')\"; printf '#!/bin/sh\\necho \"rust-analyzer 1.99.1 (58cf8f9 2026-10-13)\"\\n' > '{analyzer}'; chmod +x '{analyzer}';;\nesac"
            ),
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_agent_never_picks_or_starts_a_program_of_a_folder_it_names() {
        let temp = tempfile::tempdir().unwrap();
        let mark = temp.path().join("ran");
        let bin = temp.path().join("project/bin");
        script(&bin.join("cargo"), &format!("touch '{}'", mark.display()));
        script(&bin.join("rustc"), &format!("touch '{}'", mark.display()));
        let analyzer = temp.path().join("project/rust-analyzer");
        script(&analyzer, &format!("touch '{}'\necho 'rust-analyzer 1.97.0'", mark.display()));
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        let file = temp.path().join("data/toolchain-rust.json");
        session.set_state(Toolchain::load(Some(file.clone())));
        for (command, params) in [
            ("toolchain.rust.choose", serde_json::json!({ "bin": bin })),
            ("toolchain.rustAnalyzer.choose", serde_json::json!({ "path": analyzer })),
            (
                "toolchain.rustAnalyzer.configure",
                serde_json::json!({ "key": "check.overrideCommand", "value": ["evil"] }),
            ),
            (
                "toolchain.rustAnalyzer.configure",
                serde_json::json!({ "key": "check", "value": { "overrideCommand": ["evil"] } }),
            ),
            ("toolchain.rustAnalyzer.configure", serde_json::json!({ "key": "server.path", "value": "/evil" })),
            (
                "toolchain.rustAnalyzer.configure",
                serde_json::json!({ "key": "server.extraEnv", "value": { "PATH": "/evil" } }),
            ),
        ] {
            let answer =
                zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, command, params.clone()));
            assert!(format!("{answer:?}").contains("agentNotAllowed"), "{command} {params}: {answer:?}");
            assert!(!mark.exists(), "{command} started the folder's program");
        }
        assert!(!std::fs::read_to_string(&file).unwrap_or_default().contains("evil"));
        let plain = serde_json::json!({ "key": "check.workspace", "value": true });
        let answer = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "toolchain.rustAnalyzer.configure", plain)
        });
        assert!(answer.is_ok(), "a setting that starts no program stays open: {answer:?}");
        registry
            .dispatch(&mut session, "toolchain.rustAnalyzer.choose", serde_json::json!({ "path": analyzer }))
            .unwrap();
        assert!(mark.exists(), "choosing does start the program to read its version");
    }

    fn rust_analyzer_declaration() -> zen_lsp::ServerDeclaration {
        zen_lsp::ServerDeclaration::parse_list(crate::LANGUAGE_SERVERS).unwrap().remove(0)
    }

    #[cfg(unix)]
    #[test]
    fn the_toolchain_in_use_and_its_standard_library_are_found() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let sysroot = home.join("rustup/toolchains/stable");
        std::fs::create_dir_all(rust::library_sources(&sysroot)).unwrap();
        fake_toolchain(&home.join(".cargo/bin"), "1.99.1", &sysroot);
        script(&home.join(".cargo/bin/rustup"), "exit 1");
        let env = Environment { home: Some(home.clone()), ..Environment::default() };
        let toolchain = Toolchain::with_environment(None, env);
        let info = toolchain.rust();
        assert_eq!(info.toolchain.as_ref().map(|found| found.bin.clone()), Some(home.join(".cargo/bin")));
        assert_eq!(info.toolchain.as_ref().and_then(|found| found.version.as_deref()), Some("1.99.1"));
        assert_eq!(info.rustup, Some(home.join(".cargo/bin/rustup")));
        assert_eq!(info.stdlib, Some(rust::library_sources(&sysroot)));
        assert!(info.stdlib_installed);
        assert_eq!(info.rust_analyzer, None);
        let project = toolchain.project(&home);
        assert_eq!(project.version.as_deref(), Some("1.99.1"));
        assert_eq!(project.host.as_deref(), Some("aarch64-apple-darwin"));
        assert_eq!(project.sysroot, Some(sysroot));
        assert_eq!(project.cargo, Some(home.join(".cargo/bin/cargo")));
        assert!(project.env[0].1.starts_with(&home.join(".cargo/bin").display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn rust_analyzer_is_installed_with_rustup_and_then_used() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let bin = home.join(".cargo/bin");
        let sysroot = home.join("rustup/toolchains/stable");
        fake_toolchain(&bin, "1.99.1", &sysroot);
        let installed = sysroot.join("bin/rust-analyzer");
        fake_rustup(&bin, &installed);
        let env = Environment { home: Some(home.clone()), ..Environment::default() };
        let toolchain = Toolchain::with_environment(Some(home.join("data/toolchain-rust.json")), env);
        let server = rust_analyzer_declaration();
        assert_eq!(toolchain.locate(&server), None, "not installed yet");
        let command = toolchain.install_command(&server).unwrap();
        assert_eq!(command.get_program(), bin.join("rustup"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["component", "add", "rust-analyzer"]);

        let generation = toolchain.generation();
        toolchain.install_rust_analyzer().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while matches!(toolchain.rust().install, Install::Running { .. }) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let info = toolchain.rust();
        assert_eq!(info.install, Install::Done);
        assert_eq!(info.rust_analyzer.as_ref().map(|found| found.path.clone()), Some(installed.clone()));
        assert_eq!(toolchain.locate(&server), Some(installed.clone()));
        assert!(toolchain.generation() > generation, "windows restart rust-analyzer");
        let env = ServerTools::environment(&toolchain, &server);
        assert!(env[0].1.to_string_lossy().starts_with(&bin.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn choices_are_checked_and_saved() {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().canonicalize().unwrap();
        let sysroot = home.join("sysroot");
        fake_toolchain(&home.join(".cargo/bin"), "1.99.1", &sysroot);
        let other = home.join("other/bin");
        fake_toolchain(&other, "1.90.0", &sysroot);
        let file = home.join("data/toolchain-rust.json");
        let env = Environment { home: Some(home.clone()), ..Environment::default() };
        let toolchain = Toolchain::with_environment(Some(file.clone()), env.clone());
        assert!(toolchain.choose(&home.join("nothing")).is_err());
        toolchain.choose(&other).unwrap();
        let info = toolchain.rust();
        assert_eq!(info.toolchain.as_ref().and_then(|found| found.version.as_deref()), Some("1.90.0"));
        assert_eq!(info.toolchains.len(), 2, "the one chosen by hand is listed");
        assert!(toolchain.choose_rust_analyzer(Some(&home.join("missing"))).is_err());
        let fake = home.join("ra");
        script(&fake, "echo 'rust-analyzer 2.0.0'");
        toolchain.choose_rust_analyzer(Some(&fake)).unwrap();
        let again = Toolchain::with_environment(Some(file), env);
        let info = again.rust();
        assert_eq!(info.toolchain.map(|found| found.bin), Some(other));
        assert_eq!(info.rust_analyzer.map(|found| (found.path, found.version)), Some((fake, Some("2.0.0".to_owned()))));
    }

    #[test]
    fn without_a_toolchain_nothing_is_in_use() {
        let temp = tempfile::tempdir().unwrap();
        let env = Environment { home: Some(temp.path().to_owned()), ..Environment::default() };
        let toolchain = Toolchain::with_environment(Some(temp.path().join("toolchain-rust.json")), env);
        let info = toolchain.rust();
        assert!(info.toolchains.is_empty() && info.toolchain.is_none() && info.stdlib.is_none());
        assert_eq!(toolchain.project(temp.path()), ProjectRust::default());
        assert!(toolchain.install_rust_analyzer().is_err(), "no rustup");
        let missing = toolchain.install_command(&rust_analyzer_declaration()).unwrap_err();
        assert_eq!(missing, zen_lsp::NotInstallable::ToolMissing(zen_lsp::InstallKind::Rustup));
    }

    #[test]
    fn the_command_environment_puts_the_toolchain_first() {
        let env = command_env(Path::new("/t/bin"), Some(std::ffi::OsStr::new("/usr/bin:/x")));
        assert_eq!(env[0].0, "PATH");
        let folders: Vec<PathBuf> = std::env::split_paths(&env[0].1).collect();
        assert_eq!(folders[0], PathBuf::from("/t/bin"));
        assert_eq!(folders.iter().filter(|folder| **folder == Path::new("/usr/bin")).count(), 1);
    }

    #[test]
    fn rust_analyzer_starts_with_the_defaults_the_users_settings_and_the_projects() {
        use serde_json::json;
        zen_i18n::register(crate::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("toolchain-rust.json");
        let toolchain = Toolchain::load(Some(file.clone()));
        let server = rust_analyzer_declaration();
        let project = tempfile::tempdir().unwrap();
        let options = toolchain.initialization_options(&server, project.path());
        assert_eq!(options["check"], json!({ "command": "check", "workspace": false }));
        assert_eq!(options["cargo"]["targetDir"], true);
        assert_eq!(options["files"]["exclude"], json!([".claude/worktrees"]));
        assert_eq!(options["workspace"]["symbol"]["search"]["kind"], "all_symbols");
        assert_eq!(options["workspace"]["symbol"]["search"]["limit"], 2000);

        let generation = toolchain.generation();
        toolchain.configure_analyzer("check.command", Some(json!("clippy")), None).unwrap();
        assert!(toolchain.generation() > generation, "rust-analyzer restarts with the change");
        toolchain.configure_analyzer("check.workspace", Some(json!("true")), Some(project.path())).unwrap();
        let options = toolchain.initialization_options(&server, project.path());
        assert_eq!(options["check"], json!({ "command": "clippy", "workspace": true }));
        assert_eq!(options["cargo"]["targetDir"], true, "the rest of the defaults stay");
        let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(saved["rustAnalyzerSettings"], json!({ "check": { "command": "clippy" } }));
        assert_eq!(Toolchain::load(Some(file)).state().analyzer, json!({ "check": { "command": "clippy" } }));
        let carried = project.path().join(analyzer::PROJECT_FILE);
        let carried_settings: Value = serde_json::from_slice(&std::fs::read(&carried).unwrap()).unwrap();
        assert_eq!(carried_settings, json!({ "check": { "workspace": true } }));

        let mut other = server.clone();
        other.command = "other-server".to_owned();
        other.initialization_options = json!({ "a": 1 });
        assert_eq!(toolchain.initialization_options(&other, project.path()), json!({ "a": 1 }));

        toolchain.configure_analyzer("check.workspace", None, Some(project.path())).unwrap();
        assert!(!carried.exists());
        std::fs::write(&carried, "{ broken").unwrap();
        let settings = toolchain.analyzer_settings(&analyzer_defaults(), Some(project.path()));
        assert!(settings.problem.as_deref().is_some_and(|problem| problem.contains(".zen")), "{settings:?}");
        assert_eq!(settings.effective["check"], json!({ "command": "clippy", "workspace": false }));
        assert!(toolchain.configure_analyzer("check..command", Some(json!(1)), None).is_err());
    }
}

pub mod go;

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

#[derive(Clone, Default)]
pub struct Toolchain {
    inner: Arc<Mutex<State>>,
}

#[derive(Default)]
struct State {
    file: Option<PathBuf>,
    chosen: Option<PathBuf>,
    found: Option<Found>,
    install: Install,
    custom: Vec<PathBuf>,
    gopls_path: Option<PathBuf>,
    dlv_install: Install,
    dlv_path: Option<PathBuf>,
    env: Option<go::Environment>,
    generation: u64,
    marks: std::collections::HashMap<PathBuf, Mark>,
    checked: Option<Instant>,
    checking: bool,
    projects: crate::project_env::GoProjectEnv,
}

const CHECK_EVERY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Mark {
    real: Option<PathBuf>,
    has_go: bool,
    version: Option<String>,
}

impl Mark {
    fn of(root: &Path) -> Self {
        Self { real: root.canonicalize().ok(), has_go: go::has_go(root), version: go::sdk_version(root) }
    }
}

#[derive(Clone)]
struct Found {
    sdks: Vec<go::GoSdk>,
    custom: Vec<go::GoSdk>,
    gopls: Option<Gopls>,
    dlv: Option<Tool>,
    module_cache: Option<PathBuf>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    go_sdk: Option<PathBuf>,
    custom_sdks: Vec<PathBuf>,
    gopls_path: Option<PathBuf>,
    dlv_path: Option<PathBuf>,
}

/// gopls, found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Gopls {
    /// Its executable.
    pub path: PathBuf,
    /// Its version, e.g. `v0.18.1`, when it answered.
    pub version: Option<String>,
}

/// A tool with a minimum version, such as Delve, found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Tool {
    /// Its executable.
    pub path: PathBuf,
    /// Its version, e.g. `1.23.1`, when it answered.
    pub version: Option<String>,
    /// Whether its version is older than the IDE needs.
    pub outdated: bool,
}

/// The installation of gopls or Delve.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Install {
    /// Not started.
    #[default]
    Idle,
    /// `go install` is running; the last line it printed, if any.
    Running {
        /// The last line `go` printed.
        line: Option<String>,
    },
    /// Installed.
    Done,
    /// It failed: what `go` said.
    Failed {
        /// The end of what `go` printed.
        message: String,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GoLocations {
    pub sdk: Option<go::GoSdk>,
    pub module_cache: Option<PathBuf>,
}

/// The Go toolchain as the IDE sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoInfo {
    /// Every Go SDK found.
    pub sdks: Vec<go::GoSdk>,
    /// The SDK in use: the one chosen, else the first found.
    pub sdk: Option<go::GoSdk>,
    /// The SDK chosen before whose folder is gone (an upgrade removed it), when it is: `sdk` is then
    /// the one now at its stable path, else the first found, and `toolchain.go.choose` with its root
    /// makes it the choice.
    pub missing_sdk: Option<PathBuf>,
    /// gopls, if found.
    pub gopls: Option<Gopls>,
    /// The state of a gopls installation started from the IDE.
    pub install: Install,
    /// The roots of the SDKs added by hand, which can be removed.
    pub custom_sdks: Vec<PathBuf>,
    /// The gopls chosen by hand, when one is.
    pub gopls_path: Option<PathBuf>,
    /// Delve's `dlv`, if found.
    pub dlv: Option<Tool>,
    /// The state of a Delve installation started from the IDE.
    pub dlv_install: Install,
    /// The Delve chosen by hand, when one is.
    pub dlv_path: Option<PathBuf>,
}

impl Toolchain {
    pub fn load(file: Option<PathBuf>) -> Self {
        let saved: Saved = file
            .as_ref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let personal =
            file.as_ref().and_then(|file| file.parent()).map(|data| data.join(crate::project_env::PERSONAL_FILE));
        Self {
            inner: Arc::new(Mutex::new(State {
                projects: crate::project_env::GoProjectEnv::new(personal),
                file,
                chosen: saved.go_sdk,
                custom: saved.custom_sdks,
                gopls_path: saved.gopls_path,
                dlv_path: saved.dlv_path,
                ..State::default()
            })),
        }
    }

    pub fn with_environment(file: Option<PathBuf>, env: go::Environment) -> Self {
        let toolchain = Self::load(file);
        toolchain.state().env = Some(env);
        toolchain
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn environment(&self) -> go::Environment {
        self.state().env.clone().unwrap_or_else(go::Environment::of_process)
    }

    fn found(&self) -> Found {
        let found = self.state().found.clone();
        found.unwrap_or_else(|| {
            let (custom, gopls_path, dlv_path) = {
                let state = self.state();
                (state.custom.clone(), state.gopls_path.clone(), state.dlv_path.clone())
            };
            let found = discover(&self.environment(), &custom, gopls_path.as_deref(), dlv_path.as_deref());
            let listed = all_sdks(&found);
            let marks = listed.iter().map(|sdk| (sdk.root.clone(), Mark::of(&sdk.root))).collect();
            let mut state = self.state();
            state.marks = marks;
            state.checked = Some(Instant::now());
            if let Some(chosen) = state.chosen.clone()
                && !listed.iter().any(|sdk| sdk.root == chosen)
                && let Some(sdk) = listed.iter().find(|sdk| go::same_folder(&sdk.root, &chosen))
            {
                state.chosen = Some(sdk.root.clone());
                let _ = save(&state);
            }
            state.found = Some(found.clone());
            found
        })
    }

    fn chosen_sdk(&self, sdks: &[go::GoSdk]) -> Option<go::GoSdk> {
        let state = self.state();
        let Some(chosen) = &state.chosen else { return sdks.first().cloned() };
        let stable = go::homebrew_alias(chosen);
        sdks.iter()
            .find(|sdk| &sdk.root == chosen)
            .or_else(|| sdks.iter().find(|sdk| stable.as_ref() == Some(&sdk.root)))
            .or_else(|| sdks.first())
            .cloned()
    }

    pub fn go(&self) -> GoInfo {
        let found = self.found();
        let sdks = all_sdks(&found);
        let sdk = self.chosen_sdk(&sdks);
        let state = self.state();
        let missing_sdk = state.chosen.clone().filter(|chosen| !sdks.iter().any(|sdk| &sdk.root == chosen));
        GoInfo {
            sdks,
            sdk,
            missing_sdk,
            gopls: found.gopls,
            install: state.install.clone(),
            custom_sdks: found.custom.into_iter().map(|sdk| sdk.root).collect(),
            gopls_path: state.gopls_path.clone(),
            dlv: found.dlv,
            dlv_install: state.dlv_install.clone(),
            dlv_path: state.dlv_path.clone(),
        }
    }

    pub fn staticcheck(&self) -> Option<PathBuf> {
        go::find_tool(&self.environment(), "staticcheck")
    }

    pub fn go_locations(&self) -> GoLocations {
        let found = self.found();
        GoLocations { sdk: self.chosen_sdk(&all_sdks(&found)), module_cache: found.module_cache }
    }

    pub fn generation(&self) -> u64 {
        self.state().generation
    }

    pub fn check(&self) -> bool {
        let Some(found) = self.state().found.clone() else { return false };
        let now: Vec<(PathBuf, Mark)> =
            all_sdks(&found).into_iter().map(|sdk| (sdk.root.clone(), Mark::of(&sdk.root))).collect();
        let changed = {
            let mut state = self.state();
            state.checked = Some(Instant::now());
            let changed = now.iter().any(|(root, mark)| state.marks.get(root).is_some_and(|seen| seen != mark));
            if !changed {
                state.marks.extend(now);
            }
            changed
        };
        if changed {
            self.refresh();
        }
        changed
    }

    pub fn check_soon(&self) {
        {
            let mut state = self.state();
            let due = state.found.is_some()
                && !state.checking
                && state.checked.is_none_or(|checked| checked.elapsed() >= CHECK_EVERY);
            if !due {
                return;
            }
            state.checking = true;
        }
        let toolchain = self.clone();
        std::thread::spawn(move || {
            toolchain.check();
            toolchain.state().checking = false;
        });
    }

    fn in_use(&self) -> (Option<go::GoSdk>, Option<Gopls>, Option<Tool>) {
        let info = self.go();
        (info.sdk, info.gopls, info.dlv)
    }

    fn tracked<T>(&self, change: impl FnOnce(&Self) -> T) -> T {
        let cached = self.state().found.is_some();
        let before = cached.then(|| self.in_use());
        let result = change(self);
        if before.is_some_and(|before| before != self.in_use()) {
            self.state().generation += 1;
        }
        result
    }

    pub fn refresh(&self) {
        self.tracked(|toolchain| toolchain.state().found = None);
    }

    pub fn choose(&self, root: &Path) -> Result<(), String> {
        self.found();
        self.tracked(|toolchain| toolchain.use_listed(root))
    }

    fn use_listed(&self, root: &Path) -> Result<(), String> {
        if !self.go().sdks.iter().any(|sdk| sdk.root == root) {
            return Err(t!("toolchain-error-unknown-sdk", root = root.display().to_string()));
        }
        let mut state = self.state();
        state.chosen = Some(root.to_owned());
        save(&state)
    }

    pub fn add_sdk(&self, root: &Path) -> Result<go::GoSdk, String> {
        let shown = root.display().to_string();
        if !go::has_go(root) {
            return Err(t!("toolchain-error-no-go", root = shown));
        }
        let sdk = go::sdk_at(root).ok_or_else(|| t!("toolchain-error-no-version", root = shown))?;
        let found = self.found();
        self.tracked(|toolchain| {
            if !all_sdks(&found).iter().any(|known| known.root == sdk.root) {
                let mut state = toolchain.state();
                if !state.custom.contains(&sdk.root) {
                    state.custom.push(sdk.root.clone());
                }
                if let Some(found) = &mut state.found {
                    found.custom.push(sdk.clone());
                }
            }
            toolchain.use_listed(&sdk.root)?;
            Ok(sdk)
        })
    }

    pub fn remove_sdk(&self, root: &Path) -> Result<(), String> {
        let canonical = root.canonicalize().unwrap_or_else(|_| root.to_owned());
        self.found();
        self.tracked(|toolchain| {
            let mut state = toolchain.state();
            let before = state.custom.len();
            state.custom.retain(|custom| custom != root && custom != &canonical);
            if state.custom.len() == before {
                return Err(t!("toolchain-error-not-added", root = root.display().to_string()));
            }
            if let Some(found) = &mut state.found {
                found.custom.retain(|sdk| sdk.root != root && sdk.root != canonical);
            }
            let listed = state.found.as_ref().map(all_sdks).unwrap_or_default();
            if state.chosen.as_ref().is_some_and(|chosen| !listed.iter().any(|sdk| &sdk.root == chosen)) {
                state.chosen = None;
            }
            save(&state)
        })
    }

    pub fn choose_gopls(&self, path: Option<&Path>) -> Result<(), String> {
        let chosen = match path {
            Some(path) => {
                let shown = path.display().to_string();
                if !path.is_file() {
                    return Err(t!("toolchain-error-no-gopls", path = shown));
                }
                let version = go::gopls_version(path).ok_or_else(|| t!("toolchain-error-not-gopls", path = shown))?;
                Some(Gopls { path: path.to_owned(), version: Some(version) })
            }
            None => None,
        };
        self.found();
        self.tracked(|toolchain| {
            let gopls = chosen.or_else(|| found_gopls(&toolchain.environment()));
            let mut state = toolchain.state();
            state.gopls_path = path.map(Path::to_owned);
            if let Some(found) = &mut state.found {
                found.gopls = gopls;
            }
            save(&state)
        })
    }

    pub fn choose_dlv(&self, path: Option<&Path>) -> Result<(), String> {
        let chosen = match path {
            Some(path) => {
                let shown = path.display().to_string();
                if !path.is_file() {
                    return Err(t!("toolchain-error-no-dlv", path = shown));
                }
                let version = go::dlv_version(path).ok_or_else(|| t!("toolchain-error-not-dlv", path = shown))?;
                Some(dlv_tool(path.to_owned(), Some(version)))
            }
            None => None,
        };
        self.found();
        self.tracked(|toolchain| {
            let dlv = chosen.or_else(|| found_dlv(&toolchain.environment()));
            let mut state = toolchain.state();
            state.dlv_path = path.map(Path::to_owned);
            if let Some(found) = &mut state.found {
                found.dlv = dlv;
            }
            save(&state)
        })
    }

    pub fn install_gopls(&self, env: Vec<(String, String)>) -> Result<(), String> {
        self.install(Installed::Gopls, env)
    }

    pub fn install_dlv(&self, env: Vec<(String, String)>) -> Result<(), String> {
        self.install(Installed::Dlv, env)
    }

    fn install(&self, tool: Installed, env: Vec<(String, String)>) -> Result<(), String> {
        let sdk = self.go().sdk.ok_or_else(|| t!("toolchain-error-no-sdk-install"))?;
        {
            let mut state = self.state();
            let install = tool.state(&mut state);
            if matches!(install, Install::Running { .. }) {
                return Ok(());
            }
            *install = Install::Running { line: None };
        }
        let toolchain = self.clone();
        std::thread::spawn(move || {
            let mut command = tool.command(&sdk);
            command.envs(env);
            let outcome = run_install(&sdk, command, |line| {
                *tool.state(&mut toolchain.state()) = Install::Running { line: Some(line.to_owned()) };
            });
            toolchain.refresh();
            let found = tool.found(&toolchain.go());
            *tool.state(&mut toolchain.state()) = match outcome {
                Ok(()) if found => Install::Done,
                Ok(()) => Install::Failed { message: tool.not_found() },
                Err(message) => Install::Failed { message },
            };
        });
        Ok(())
    }

    pub fn go_debugger(&self) -> Option<PathBuf> {
        self.go().dlv.filter(|dlv| !dlv.outdated).map(|dlv| dlv.path)
    }
}

#[derive(Clone, Copy)]
enum Installed {
    Gopls,
    Dlv,
}

impl Installed {
    fn state(self, state: &mut State) -> &mut Install {
        match self {
            Self::Gopls => &mut state.install,
            Self::Dlv => &mut state.dlv_install,
        }
    }

    fn command(self, sdk: &go::GoSdk) -> std::process::Command {
        match self {
            Self::Gopls => go::install_gopls_command(sdk),
            Self::Dlv => go::install_dlv_command(sdk),
        }
    }

    fn found(self, info: &GoInfo) -> bool {
        match self {
            Self::Gopls => info.gopls.is_some(),
            Self::Dlv => info.dlv.is_some(),
        }
    }

    fn not_found(self) -> String {
        match self {
            Self::Gopls => t!("toolchain-error-installed-not-found"),
            Self::Dlv => t!("toolchain-error-dlv-installed-not-found"),
        }
    }
}

impl Toolchain {
    pub fn go_run_env(&self) -> Option<zen_run::RunEnv> {
        let sdk = self.go().sdk?;
        Some(go_run_env_for(&sdk, std::env::var_os("PATH").as_deref()))
    }

    pub fn project_env(&self) -> crate::project_env::GoProjectEnv {
        self.state().projects.clone()
    }

    pub fn set_project_env(
        &self,
        root: &Path,
        scope: crate::project_env::Scope,
        variables: &std::collections::BTreeMap<String, String>,
    ) -> Result<(), String> {
        let before = self.project_env().variables(root, true);
        self.project_env().set(root, scope, variables)?;
        if self.project_env().variables(root, true) != before {
            self.state().generation += 1;
        }
        Ok(())
    }
}

impl zen_lsp::ServerTools for Toolchain {
    fn locate(&self, server: &zen_lsp::ServerDeclaration) -> Option<PathBuf> {
        if is_go(server) && !Path::new(&server.command).is_absolute() {
            if server.command == "gopls"
                && let Some(gopls) = self.go().gopls
            {
                return Some(gopls.path);
            }
            if let Some(found) = go::find_tool(&Toolchain::environment(self), &server.command) {
                return Some(found);
            }
        }
        zen_lsp::servers::find_executable(&server.command, &zen_lsp::servers::search_folders())
    }

    fn environment(&self, server: &zen_lsp::ServerDeclaration) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        let sdk = if is_go(server) { self.go().sdk } else { None };
        let Some(sdk) = sdk else { return zen_lsp::servers::default_environment() };
        let inherited = std::env::join_paths(zen_lsp::servers::search_folders()).ok();
        let Ok(path) = std::env::join_paths(go_path(&sdk.root, inherited.as_deref())) else {
            return zen_lsp::servers::default_environment();
        };
        vec![("PATH".into(), path), ("GOROOT".into(), sdk.root.into_os_string())]
    }

    fn project_environment(&self, root: &Path) -> Vec<(String, String)> {
        self.project_env().variables(root, zen_project::trust::trusted(root))
    }

    fn install_command(
        &self,
        server: &zen_lsp::ServerDeclaration,
    ) -> Result<std::process::Command, zen_lsp::NotInstallable> {
        match &server.install {
            Some(recipe) if recipe.kind == zen_lsp::InstallKind::Go && !recipe.package.is_empty() => {
                let sdk = self.go().sdk.ok_or(zen_lsp::NotInstallable::ToolMissing(zen_lsp::InstallKind::Go))?;
                Ok(go::install_command(&sdk, &recipe.package))
            }
            _ => zen_lsp::servers::default_install_command(server),
        }
    }

    fn refresh(&self) {
        Toolchain::refresh(self);
    }
}

fn is_go(server: &zen_lsp::ServerDeclaration) -> bool {
    server.install.as_ref().is_some_and(|recipe| recipe.kind == zen_lsp::InstallKind::Go)
}

fn go_path(sdk_root: &Path, inherited: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    let mut path = vec![sdk_root.join("bin")];
    path.extend(inherited.map(|value| std::env::split_paths(value).collect::<Vec<_>>()).unwrap_or_default());
    path.extend(["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from));
    path
}

fn go_run_env_for(sdk: &go::GoSdk, inherited: Option<&std::ffi::OsStr>) -> zen_run::RunEnv {
    let folders: Vec<String> =
        go_path(&sdk.root, inherited).iter().map(|folder| folder.to_string_lossy().into_owned()).collect();
    let separator = if cfg!(windows) { ";" } else { ":" };
    zen_run::RunEnv {
        tool: sdk.go(),
        env: vec![
            ("PATH".to_owned(), folders.join(separator)),
            ("GOROOT".to_owned(), sdk.root.to_string_lossy().into_owned()),
        ],
    }
}

fn discover(env: &go::Environment, custom: &[PathBuf], gopls_path: Option<&Path>, dlv_path: Option<&Path>) -> Found {
    let gopls = match gopls_path.filter(|path| path.is_file()) {
        Some(path) => Some(Gopls { version: go::gopls_version(path), path: path.to_owned() }),
        None => found_gopls(env),
    };
    let dlv = match dlv_path.filter(|path| path.is_file()) {
        Some(path) => Some(dlv_tool(path.to_owned(), go::dlv_version(path))),
        None => found_dlv(env),
    };
    Found {
        sdks: go::find_sdks(env),
        custom: custom.iter().filter_map(|root| go::sdk_at(root)).collect(),
        gopls,
        dlv,
        module_cache: go::module_cache(env),
    }
}

fn found_gopls(env: &go::Environment) -> Option<Gopls> {
    go::find_gopls(env).map(|path| Gopls { version: go::gopls_version(&path), path })
}

fn found_dlv(env: &go::Environment) -> Option<Tool> {
    go::find_dlv(env).map(|path| {
        let version = go::dlv_version(&path);
        dlv_tool(path, version)
    })
}

fn dlv_tool(path: PathBuf, version: Option<String>) -> Tool {
    let outdated = version.as_deref().is_some_and(go::dlv_outdated);
    Tool { path, version, outdated }
}

fn all_sdks(found: &Found) -> Vec<go::GoSdk> {
    let mut sdks = found.sdks.clone();
    for sdk in &found.custom {
        if !sdks.iter().any(|known| known.root == sdk.root) {
            sdks.push(sdk.clone());
        }
    }
    sdks
}

fn save(state: &State) -> Result<(), String> {
    let Some(file) = &state.file else { return Ok(()) };
    let saved = Saved {
        go_sdk: state.chosen.clone(),
        custom_sdks: state.custom.clone(),
        gopls_path: state.gopls_path.clone(),
        dlv_path: state.dlv_path.clone(),
    };
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    std::fs::write(file, serde_json::to_vec_pretty(&saved).unwrap_or_default())
        .map_err(|error| t!("toolchain-error-save", file = file.display().to_string(), error = error.to_string()))
}

pub(crate) fn open_project_env(session: &Session, toolchain: &Toolchain) -> Vec<(String, String)> {
    match session.state::<zen_project::trust::OpenRoot>() {
        Some(open) => toolchain.project_env().variables(&open.0, zen_project::trust::trusted(&open.0)),
        None => Vec::new(),
    }
}

pub(crate) fn run_install(
    sdk: &go::GoSdk,
    mut command: std::process::Command,
    mut line: impl FnMut(&str),
) -> Result<(), String> {
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| {
            t!("toolchain-error-run", program = sdk.go().display().to_string(), error = error.to_string())
        })?;
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

pub(crate) fn person_only(session: &Session) -> Result<(), CommandError> {
    if zen_core::macros::by_agent(session) {
        return Err(CommandError::with_code("agentNotAllowed", t!("toolchain-error-agent")));
    }
    Ok(())
}

fn toolchain(session: &Session) -> Result<&Toolchain, CommandError> {
    session.state::<Toolchain>().ok_or_else(|| CommandError::new(t!("toolchain-error-none")))
}

pub struct GoInfoCommand;

impl Command for GoInfoCommand {
    type Params = NoParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.go.info";
    const TITLE: &'static str = "Go Toolchain";
    const DESCRIPTION: &'static str = "The Go SDKs found on this computer, the one in use, gopls and Delve (dlv) with \
         their versions, and the state of their installations; an SDK changed on disk since (an upgrade) is \
         looked up again first, and missingSdk names a chosen SDK whose folder is gone.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<GoInfo, CommandError> {
        let toolchain = toolchain(session)?;
        toolchain.check();
        Ok(toolchain.go())
    }
}

pub struct GoRefresh;

impl Command for GoRefresh {
    type Params = NoParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.go.refresh";
    const TITLE: &'static str = "Refresh Go Toolchain";
    const DESCRIPTION: &'static str = "Looks for Go SDKs, gopls and Delve again, after installing or removing one.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<GoInfo, CommandError> {
        let toolchain = toolchain(session)?;
        toolchain.refresh();
        Ok(toolchain.go())
    }
}

/// The parameters of `toolchain.go.choose`.
#[derive(Deserialize, JsonSchema)]
pub struct ChooseParams {
    /// The SDK's root folder, as `toolchain.go.info` lists it.
    pub root: PathBuf,
}

pub struct GoChoose;

impl Command for GoChoose {
    type Params = ChooseParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.go.choose";
    const TITLE: &'static str = "Choose Go SDK";
    const DESCRIPTION: &'static str = "Uses one of the Go SDKs found (its root folder) and remembers the choice.";

    fn run(&self, session: &mut Session, params: ChooseParams) -> Result<GoInfo, CommandError> {
        person_only(session)?;
        let toolchain = toolchain(session)?;
        toolchain.choose(&params.root).map_err(CommandError::new)?;
        Ok(toolchain.go())
    }
}

/// The parameters of `toolchain.go.add` and `toolchain.go.remove`.
#[derive(Deserialize, JsonSchema)]
pub struct SdkParams {
    /// The SDK's root folder, the one holding `bin/go`.
    pub root: PathBuf,
}

pub struct GoAdd;

impl Command for GoAdd {
    type Params = SdkParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.go.add";
    const TITLE: &'static str = "Add Go SDK";
    const DESCRIPTION: &'static str = "Adds a Go SDK from a folder (its root, holding bin/go) to those found, uses it and \
         remembers both; the version comes from its VERSION file or `go version`.";

    fn run(&self, session: &mut Session, params: SdkParams) -> Result<GoInfo, CommandError> {
        person_only(session)?;
        let toolchain = toolchain(session)?;
        toolchain.add_sdk(&params.root).map_err(CommandError::new)?;
        Ok(toolchain.go())
    }
}

pub struct GoRemove;

impl Command for GoRemove {
    type Params = SdkParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.go.remove";
    const TITLE: &'static str = "Remove Go SDK";
    const DESCRIPTION: &'static str =
        "Removes a Go SDK added with toolchain.go.add from the list; the first SDK found is used if it was in use.";

    fn run(&self, session: &mut Session, params: SdkParams) -> Result<GoInfo, CommandError> {
        person_only(session)?;
        let toolchain = toolchain(session)?;
        toolchain.remove_sdk(&params.root).map_err(CommandError::new)?;
        Ok(toolchain.go())
    }
}

/// The parameters of `toolchain.gopls.choose`.
#[derive(Deserialize, JsonSchema)]
pub struct GoplsChooseParams {
    /// The gopls executable to use; absent or null goes back to the one found.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

pub struct GoplsChoose;

impl Command for GoplsChoose {
    type Params = GoplsChooseParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.gopls.choose";
    const TITLE: &'static str = "Choose gopls";
    const DESCRIPTION: &'static str = "Uses the gopls at a path instead of the one found, once it answers `gopls version`, \
         and remembers the choice; null goes back to the gopls found.";

    fn run(&self, session: &mut Session, params: GoplsChooseParams) -> Result<GoInfo, CommandError> {
        person_only(session)?;
        let toolchain = toolchain(session)?;
        toolchain.choose_gopls(params.path.as_deref()).map_err(CommandError::new)?;
        Ok(toolchain.go())
    }
}

pub struct GoplsInstall;

impl Command for GoplsInstall {
    type Params = NoParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.gopls.install";
    const TITLE: &'static str = "Install gopls";
    const DESCRIPTION: &'static str = "Installs the latest gopls with `go install golang.org/x/tools/gopls@latest` in the \
         background; toolchain.go.info reports the progress.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<GoInfo, CommandError> {
        let toolchain = toolchain(session)?;
        toolchain.install_gopls(open_project_env(session, toolchain)).map_err(CommandError::new)?;
        Ok(toolchain.go())
    }
}

/// The parameters of `toolchain.dlv.choose`.
#[derive(Deserialize, JsonSchema)]
pub struct DlvChooseParams {
    /// The dlv executable to use; absent or null goes back to the one found.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

pub struct DlvChoose;

impl Command for DlvChoose {
    type Params = DlvChooseParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.dlv.choose";
    const TITLE: &'static str = "Choose Delve";
    const DESCRIPTION: &'static str = "Uses the Delve (dlv) at a path instead of the one found, once it answers \
         `dlv version`, and remembers the choice; null goes back to the dlv found.";

    fn run(&self, session: &mut Session, params: DlvChooseParams) -> Result<GoInfo, CommandError> {
        person_only(session)?;
        let toolchain = toolchain(session)?;
        toolchain.choose_dlv(params.path.as_deref()).map_err(CommandError::new)?;
        Ok(toolchain.go())
    }
}

pub struct DlvInstall;

impl Command for DlvInstall {
    type Params = NoParams;
    type Output = GoInfo;
    const ID: &'static str = "toolchain.dlv.install";
    const TITLE: &'static str = "Install Delve";
    const DESCRIPTION: &'static str = "Downloads and installs the latest Delve with `go install \
         github.com/go-delve/delve/cmd/dlv@latest` in the background, only when called; toolchain.go.info \
         reports the progress as dlvInstall.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<GoInfo, CommandError> {
        let toolchain = toolchain(session)?;
        toolchain.install_dlv(open_project_env(session, toolchain)).map_err(CommandError::new)?;
        Ok(toolchain.go())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GoInfoCommand)?;
    registry.register(GoRefresh)?;
    registry.register(GoChoose)?;
    registry.register(GoAdd)?;
    registry.register(GoRemove)?;
    registry.register(GoplsChoose)?;
    registry.register(GoplsInstall)?;
    registry.register(DlvChoose)?;
    registry.register(DlvInstall)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_is_saved_and_checked() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("go1");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin/go"), "").unwrap();
        let root = root.canonicalize().unwrap();
        let file = temp.path().join("data/toolchain.json");
        let toolchain = Toolchain::load(Some(file.clone()));
        toolchain.state().found = Some(Found {
            sdks: vec![go::GoSdk { root: root.clone(), version: None }],
            custom: Vec::new(),
            gopls: None,
            dlv: None,
            module_cache: None,
        });
        assert!(toolchain.choose(&temp.path().join("elsewhere")).is_err());
        toolchain.choose(&root).unwrap();
        assert_eq!(toolchain.go().sdk.map(|sdk| sdk.root), Some(root.clone()));
        let again = Toolchain::load(Some(file));
        assert_eq!(again.state().chosen, Some(root));
    }

    #[cfg(unix)]
    fn script(path: &Path, output: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\necho '{output}'\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    fn brew_go(brew: &Path, version: &str) {
        use std::os::unix::fs::symlink;
        let keg = brew.join("Cellar/go").join(version);
        let libexec = keg.join("libexec");
        script(&libexec.join("bin/go"), &libexec.display().to_string());
        std::fs::create_dir_all(libexec.join("src/runtime")).unwrap();
        std::fs::write(libexec.join("VERSION"), format!("go{version}\ntime 2026-10-01T00:00:00Z\n")).unwrap();
        std::fs::create_dir_all(keg.join("bin")).unwrap();
        symlink("../libexec/bin/go", keg.join("bin/go")).unwrap();
        for (link, target) in
            [("opt/go", format!("../Cellar/go/{version}")), ("bin/go", format!("../Cellar/go/{version}/bin/go"))]
        {
            let link = brew.join(link);
            std::fs::create_dir_all(link.parent().unwrap()).unwrap();
            let _ = std::fs::remove_file(&link);
            symlink(target, &link).unwrap();
        }
    }

    #[cfg(unix)]
    fn brew_environment(temp: &Path, brew: &Path) -> go::Environment {
        go::Environment {
            path: vec![brew.join("bin")],
            install_folders: vec![brew.join("opt/go/libexec")],
            ..empty_environment(temp)
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_homebrew_go_is_kept_by_its_stable_path_and_follows_an_upgrade() {
        let temp = tempfile::tempdir().unwrap();
        let brew = temp.path().canonicalize().unwrap().join("brew");
        brew_go(&brew, "1.27.2");
        let stable = brew.join("opt/go/libexec");
        let toolchain = Toolchain::with_environment(None, brew_environment(temp.path(), &brew));
        let info = toolchain.go();
        let sdk = |version: &str| go::GoSdk { root: stable.clone(), version: Some(version.to_owned()) };
        assert_eq!(info.sdks, [sdk("go1.27.2")], "found on PATH and in opt/go, kept once by the stable path");
        assert_eq!(info.sdk, Some(sdk("go1.27.2")));
        let generation = toolchain.generation();
        assert!(!toolchain.check(), "nothing changed");
        assert_eq!(toolchain.generation(), generation);

        brew_go(&brew, "1.27.3");
        std::fs::remove_dir_all(brew.join("Cellar/go/1.27.2")).unwrap();
        assert!(toolchain.check(), "the upgrade is noticed");
        let info = toolchain.go();
        assert_eq!(info.sdks, [sdk("go1.27.3")]);
        assert_eq!(info.sdk, Some(sdk("go1.27.3")));
        assert_eq!(info.missing_sdk, None);
        assert_eq!(toolchain.generation(), generation + 1, "gopls, runs and the project model follow");
        assert_eq!(toolchain.go_locations().sdk, Some(sdk("go1.27.3")));
        let run = toolchain.go_run_env().unwrap();
        assert_eq!(run.tool, stable.join("bin/go"));
        assert!(run.env.contains(&("GOROOT".to_owned(), stable.display().to_string())));
        assert!(!toolchain.check());
        assert_eq!(toolchain.generation(), generation + 1);
    }

    #[cfg(unix)]
    #[test]
    fn a_chosen_sdk_is_kept_by_its_stable_path_or_reported_missing() {
        let temp = tempfile::tempdir().unwrap();
        let brew = temp.path().canonicalize().unwrap().join("brew");
        brew_go(&brew, "1.27.2");
        let stable = brew.join("opt/go/libexec");
        let file = temp.path().join("data/toolchain.json");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();

        let cellar = brew.join("Cellar/go/1.27.2/libexec");
        std::fs::write(&file, serde_json::json!({ "goSdk": cellar }).to_string()).unwrap();
        let toolchain = Toolchain::with_environment(Some(file.clone()), brew_environment(temp.path(), &brew));
        let info = toolchain.go();
        assert_eq!(info.sdk.map(|sdk| sdk.root), Some(stable.clone()));
        assert_eq!(info.missing_sdk, None);
        assert_eq!(saved(&file)["goSdk"], serde_json::json!(stable));

        let gone = brew.join("Cellar/go/1.27.1/libexec");
        std::fs::write(&file, serde_json::json!({ "goSdk": gone }).to_string()).unwrap();
        let toolchain = Toolchain::with_environment(Some(file.clone()), brew_environment(temp.path(), &brew));
        let info = toolchain.go();
        assert_eq!(info.missing_sdk, Some(gone));
        assert_eq!(info.sdk.map(|sdk| sdk.root), Some(stable.clone()));
        toolchain.choose(&stable).unwrap();
        assert_eq!(toolchain.go().missing_sdk, None);
        assert_eq!(saved(&file)["goSdk"], serde_json::json!(stable));
    }

    fn empty_environment(temp: &Path) -> go::Environment {
        go::Environment { home: Some(temp.join("home")), ..go::Environment::default() }
    }

    fn saved(file: &Path) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
    }

    #[cfg(unix)]
    #[test]
    fn an_sdk_added_by_hand_is_listed_used_saved_and_removed() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("sdks/go-first");
        let second = temp.path().join("sdks/go-second");
        script(&first.join("bin/go"), "go version go1.99.1 darwin/arm64");
        script(&second.join("bin/go"), "go version go1.98.0 linux/amd64");
        let first = first.canonicalize().unwrap();
        let second = second.canonicalize().unwrap();
        let file = temp.path().join("data/toolchain.json");
        let toolchain = Toolchain::with_environment(Some(file.clone()), empty_environment(temp.path()));
        assert_eq!(toolchain.go().sdk, None);
        let generation = toolchain.generation();

        let sdk = toolchain.add_sdk(&first).unwrap();
        assert_eq!(sdk, go::GoSdk { root: first.clone(), version: Some("go1.99.1".to_owned()) });
        let info = toolchain.go();
        assert_eq!(info.sdk, Some(sdk.clone()));
        assert_eq!(info.sdks, [sdk]);
        assert_eq!(info.custom_sdks, vec![first.clone()]);
        assert_eq!(toolchain.generation(), generation + 1);
        assert_eq!(saved(&file)["customSdks"], serde_json::json!([first]));
        assert_eq!(saved(&file)["goSdk"], serde_json::json!(first));

        toolchain.add_sdk(&second).unwrap();
        assert_eq!(toolchain.go().sdk.map(|sdk| sdk.root), Some(second.clone()));
        assert_eq!(toolchain.generation(), generation + 2);
        toolchain.choose(&first).unwrap();
        toolchain.choose(&first).unwrap();
        assert_eq!(toolchain.generation(), generation + 3, "choosing the SDK in use changes nothing");
        toolchain.add_sdk(&first).unwrap();
        assert_eq!(toolchain.go().custom_sdks, [first.clone(), second.clone()], "added once");

        let again = Toolchain::with_environment(Some(file.clone()), empty_environment(temp.path()));
        let info = again.go();
        assert_eq!(info.sdk.map(|sdk| sdk.root), Some(first.clone()));
        assert_eq!(info.custom_sdks, [first.clone(), second.clone()]);

        toolchain.remove_sdk(&first).unwrap();
        let info = toolchain.go();
        assert_eq!(info.sdk.map(|sdk| sdk.root), Some(second.clone()));
        assert_eq!(info.custom_sdks, vec![second.clone()]);
        assert_eq!(toolchain.generation(), generation + 4);
        assert_eq!(saved(&file)["goSdk"], serde_json::Value::Null);
        assert!(toolchain.remove_sdk(&first).is_err(), "removed already");
        assert!(toolchain.choose(&first).is_err(), "no longer listed");
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_without_a_working_go_is_not_added() {
        let temp = tempfile::tempdir().unwrap();
        let toolchain = Toolchain::with_environment(None, empty_environment(temp.path()));
        std::fs::create_dir_all(temp.path().join("empty")).unwrap();
        assert!(toolchain.add_sdk(&temp.path().join("empty")).is_err(), "no bin/go");
        script(&temp.path().join("silent/bin/go"), "hello");
        assert!(toolchain.add_sdk(&temp.path().join("silent")).is_err(), "no version");
        assert!(toolchain.go().sdks.is_empty());
        assert!(toolchain.remove_sdk(&temp.path().join("silent")).is_err(), "not added");
    }

    #[cfg(unix)]
    fn program_leaving_a_mark(path: &Path, mark: &Path, said: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\ntouch '{}'\necho '{said}'\n", mark.display())).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_agent_never_picks_or_starts_a_program_of_a_folder_it_names() {
        let temp = tempfile::tempdir().unwrap();
        let mark = temp.path().join("ran");
        let root = temp.path().join("project/go");
        program_leaving_a_mark(&root.join("bin/go"), &mark, "go version go1.97.3 darwin/arm64");
        let gopls = temp.path().join("project/gopls");
        program_leaving_a_mark(&gopls, &mark, "golang.org/x/tools/gopls v0.97.0");
        let dlv = temp.path().join("project/dlv");
        program_leaving_a_mark(&dlv, &mark, "Delve Debugger\nVersion: 1.97.0");
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Toolchain::with_environment(None, empty_environment(temp.path())));
        for (command, params) in [
            ("toolchain.go.add", serde_json::json!({ "root": root })),
            ("toolchain.go.choose", serde_json::json!({ "root": root })),
            ("toolchain.go.remove", serde_json::json!({ "root": root })),
            ("toolchain.gopls.choose", serde_json::json!({ "path": gopls })),
            ("toolchain.dlv.choose", serde_json::json!({ "path": dlv })),
        ] {
            let answer =
                zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, command, params));
            assert!(format!("{answer:?}").contains("agentNotAllowed"), "{command}: {answer:?}");
            assert!(!mark.exists(), "{command} started the folder's program");
        }
        let info = zen_core::macros::as_agent(&mut session, |session| {
            registry.dispatch(session, "toolchain.go.info", serde_json::json!({}))
        });
        assert_eq!(info.unwrap()["sdks"], serde_json::json!([]), "listing stays open and nothing was added");
        registry.dispatch(&mut session, "toolchain.go.add", serde_json::json!({ "root": root })).unwrap();
        assert!(mark.exists(), "add does start the folder's go to read its version, so an agent is refused before");
    }

    #[cfg(unix)]
    #[test]
    fn the_commands_add_and_remove_sdks_and_choose_gopls() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("go");
        script(&root.join("bin/go"), "go version go1.97.3 darwin/arm64");
        let gopls = temp.path().join("bin/gopls");
        script(&gopls, "golang.org/x/tools/gopls v0.97.0");
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Toolchain::with_environment(None, empty_environment(temp.path())));
        let info = registry.dispatch(&mut session, "toolchain.go.add", serde_json::json!({ "root": root })).unwrap();
        assert_eq!(info["sdk"]["version"], "go1.97.3");
        assert_eq!(info["customSdks"].as_array().map(Vec::len), Some(1));
        let info =
            registry.dispatch(&mut session, "toolchain.gopls.choose", serde_json::json!({ "path": gopls })).unwrap();
        assert_eq!(info["gopls"]["version"], "v0.97.0");
        assert_eq!(info["goplsPath"], serde_json::json!(gopls));
        let info =
            registry.dispatch(&mut session, "toolchain.gopls.choose", serde_json::json!({ "path": null })).unwrap();
        assert_eq!(info["goplsPath"], serde_json::Value::Null);
        let info = registry.dispatch(&mut session, "toolchain.go.remove", serde_json::json!({ "root": root })).unwrap();
        assert_eq!(info["sdks"], serde_json::json!([]));
        assert!(registry.dispatch(&mut session, "toolchain.go.remove", serde_json::json!({ "root": root })).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_gopls_chosen_by_hand_is_checked_used_and_saved() {
        let temp = tempfile::tempdir().unwrap();
        let gopls = temp.path().join("tools/gopls");
        script(&gopls, "golang.org/x/tools/gopls v0.99.0");
        let other = temp.path().join("tools/not-gopls");
        script(&other, "something else");
        let file = temp.path().join("data/toolchain.json");
        let toolchain = Toolchain::with_environment(Some(file.clone()), empty_environment(temp.path()));
        toolchain.state().found =
            Some(Found { sdks: Vec::new(), custom: Vec::new(), gopls: None, dlv: None, module_cache: None });
        assert!(toolchain.choose_gopls(Some(&temp.path().join("missing"))).is_err());
        assert!(toolchain.choose_gopls(Some(&other)).is_err());
        assert_eq!(toolchain.generation(), 0);

        toolchain.choose_gopls(Some(&gopls)).unwrap();
        let info = toolchain.go();
        assert_eq!(info.gopls, Some(Gopls { path: gopls.clone(), version: Some("v0.99.0".to_owned()) }));
        assert_eq!(info.gopls_path, Some(gopls.clone()));
        assert_eq!(toolchain.generation(), 1);
        assert_eq!(saved(&file)["goplsPath"], serde_json::json!(gopls));

        let again = Toolchain::with_environment(Some(file.clone()), empty_environment(temp.path()));
        assert_eq!(again.go().gopls.map(|found| found.path), Some(gopls.clone()));

        toolchain.choose_gopls(None).unwrap();
        let info = toolchain.go();
        assert_eq!(info.gopls_path, None);
        assert_ne!(info.gopls.map(|found| found.path), Some(gopls));
        assert_eq!(toolchain.generation(), 2);
        assert_eq!(saved(&file)["goplsPath"], serde_json::Value::Null);
    }

    #[cfg(unix)]
    #[test]
    fn a_delve_chosen_by_hand_is_checked_used_and_saved() {
        let temp = tempfile::tempdir().unwrap();
        let dlv = temp.path().join("tools/dlv");
        script(&dlv, "Delve Debugger\nVersion: 1.23.1\nBuild: $Id: abc $");
        let old = temp.path().join("tools/old-dlv");
        script(&old, "Delve Debugger\nVersion: 1.19.0");
        let other = temp.path().join("tools/not-dlv");
        script(&other, "something else");
        let file = temp.path().join("data/toolchain.json");
        let toolchain = Toolchain::with_environment(Some(file.clone()), empty_environment(temp.path()));
        toolchain.state().found =
            Some(Found { sdks: Vec::new(), custom: Vec::new(), gopls: None, dlv: None, module_cache: None });
        assert!(toolchain.choose_dlv(Some(&temp.path().join("missing"))).is_err());
        assert!(toolchain.choose_dlv(Some(&other)).is_err());
        assert_eq!(toolchain.generation(), 0);

        toolchain.choose_dlv(Some(&dlv)).unwrap();
        let info = toolchain.go();
        assert_eq!(info.dlv, Some(Tool { path: dlv.clone(), version: Some("1.23.1".to_owned()), outdated: false }));
        assert_eq!(info.dlv_path, Some(dlv.clone()));
        assert_eq!(toolchain.go_debugger(), Some(dlv.clone()));
        assert_eq!(toolchain.generation(), 1);
        assert_eq!(saved(&file)["dlvPath"], serde_json::json!(dlv));
        toolchain.choose_dlv(Some(&dlv)).unwrap();
        assert_eq!(toolchain.generation(), 1, "choosing the Delve in use changes nothing");

        let again = Toolchain::with_environment(Some(file.clone()), empty_environment(temp.path()));
        assert_eq!(again.go().dlv.map(|found| found.path), Some(dlv.clone()));

        toolchain.choose_dlv(Some(&old)).unwrap();
        assert_eq!(toolchain.go().dlv.map(|found| found.outdated), Some(true));
        assert_eq!(toolchain.go_debugger(), None, "a Delve too old is not started");
        assert_eq!(toolchain.generation(), 2);

        toolchain.choose_dlv(None).unwrap();
        let info = toolchain.go();
        assert_eq!(info.dlv_path, None);
        assert!(info.dlv.is_none_or(|found| !found.path.starts_with(temp.path())));
        assert_eq!(toolchain.generation(), 3);
        assert_eq!(saved(&file)["dlvPath"], serde_json::Value::Null);
    }

    #[cfg(unix)]
    #[test]
    fn the_commands_choose_delve_and_report_it() {
        let temp = tempfile::tempdir().unwrap();
        let dlv = temp.path().join("bin/dlv");
        script(&dlv, "Delve Debugger\nVersion: 1.24.0");
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Toolchain::with_environment(None, empty_environment(temp.path())));
        let info = registry.dispatch(&mut session, "toolchain.go.info", serde_json::json!({})).unwrap();
        assert_eq!(info["dlvInstall"]["state"], "idle");
        assert!(info.get("dlv").is_some());
        assert!(info.get("dlvPath").is_some());
        let info = registry.dispatch(&mut session, "toolchain.dlv.choose", serde_json::json!({ "path": dlv })).unwrap();
        assert_eq!(info["dlv"], serde_json::json!({ "path": dlv, "version": "1.24.0", "outdated": false }));
        assert_eq!(info["dlvPath"], serde_json::json!(dlv));
        let info = registry.dispatch(&mut session, "toolchain.dlv.choose", serde_json::json!({})).unwrap();
        assert_eq!(info["dlvPath"], serde_json::Value::Null);
        assert!(
            registry
                .dispatch(&mut session, "toolchain.dlv.choose", serde_json::json!({ "path": "/none/dlv" }))
                .is_err()
        );
    }

    #[test]
    fn toolchain_files_written_before_custom_sdks_still_load() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("toolchain.json");
        std::fs::write(&file, r#"{"goSdk": "/sdk/b"}"#).unwrap();
        let toolchain = Toolchain::load(Some(file.clone()));
        {
            let state = toolchain.state();
            assert_eq!(state.chosen, Some(PathBuf::from("/sdk/b")));
            assert!(state.custom.is_empty());
            assert_eq!(state.gopls_path, None);
            assert_eq!(state.dlv_path, None);
        }
        std::fs::write(
            &file,
            r#"{"goSdk": null, "customSdks": ["/sdk/c"], "goplsPath": "/bin/gopls", "dlvPath": "/bin/dlv", "later": 1}"#,
        )
        .unwrap();
        let toolchain = Toolchain::load(Some(file));
        let state = toolchain.state();
        assert_eq!(state.chosen, None);
        assert_eq!(state.custom, [PathBuf::from("/sdk/c")]);
        assert_eq!(state.gopls_path, Some(PathBuf::from("/bin/gopls")));
        assert_eq!(state.dlv_path, Some(PathBuf::from("/bin/dlv")));
    }

    #[test]
    fn locations_give_the_chosen_sdk_and_the_module_cache() {
        let toolchain = Toolchain::default();
        let first = go::GoSdk { root: PathBuf::from("/sdk/a"), version: Some("go1.23.4".to_owned()) };
        let second = go::GoSdk { root: PathBuf::from("/sdk/b"), version: None };
        toolchain.state().found = Some(Found {
            sdks: vec![first.clone(), second.clone()],
            custom: Vec::new(),
            gopls: None,
            dlv: None,
            module_cache: Some(PathBuf::from("/cache")),
        });
        assert_eq!(
            toolchain.go_locations(),
            GoLocations { sdk: Some(first), module_cache: Some(PathBuf::from("/cache")) }
        );
        toolchain.state().chosen = Some(PathBuf::from("/sdk/b"));
        assert_eq!(toolchain.go_locations().sdk, Some(second));
    }

    #[test]
    fn go_runs_with_the_sdk_first_on_the_path() {
        let sdk = go::GoSdk { root: PathBuf::from("/sdk/go"), version: None };
        let inherited = std::env::join_paths(["/a", "/b"]).unwrap();
        let path = go_path(&sdk.root, Some(&inherited));
        assert_eq!(
            path,
            ["/sdk/go/bin", "/a", "/b", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].map(PathBuf::from)
        );
        assert_eq!(go_path(&sdk.root, None).len(), 5);
        let env = go_run_env_for(&sdk, Some(&inherited));
        assert_eq!(env.tool, sdk.go());
        let separator = if cfg!(windows) { ";" } else { ":" };
        let expected =
            ["/sdk/go/bin", "/a", "/b", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"].join(separator);
        assert_eq!(env.env, [("PATH".to_owned(), expected), ("GOROOT".to_owned(), "/sdk/go".to_owned())]);
        let toolchain = Toolchain::default();
        toolchain.state().found =
            Some(Found { sdks: Vec::new(), custom: Vec::new(), gopls: None, dlv: None, module_cache: None });
        assert_eq!(toolchain.go_run_env(), None);
        toolchain.state().found =
            Some(Found { sdks: vec![sdk.clone()], custom: Vec::new(), gopls: None, dlv: None, module_cache: None });
        assert_eq!(toolchain.go_run_env().map(|env| env.tool), Some(sdk.go()));
    }

    #[test]
    fn commands_need_the_toolchain_in_the_session() {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        assert!(registry.dispatch(&mut session, "toolchain.go.info", serde_json::json!({})).is_err());
        let toolchain = Toolchain::default();
        toolchain.state().found =
            Some(Found { sdks: Vec::new(), custom: Vec::new(), gopls: None, dlv: None, module_cache: None });
        session.set_state(toolchain);
        let info = registry.dispatch(&mut session, "toolchain.go.info", serde_json::json!({})).unwrap();
        assert_eq!(info["sdks"], serde_json::json!([]));
        assert_eq!(info["install"]["state"], "idle");
        assert!(registry.dispatch(&mut session, "toolchain.gopls.install", serde_json::json!({})).is_err(), "no SDK");
        assert!(registry.dispatch(&mut session, "toolchain.dlv.install", serde_json::json!({})).is_err(), "no SDK");
        assert_eq!(info["dlvInstall"]["state"], "idle");
    }

    #[test]
    fn go_servers_use_the_sdk_in_use() {
        use zen_lsp::ServerTools;
        let server = zen_lsp::ServerDeclaration::parse_list(
            r#"[{ "id": "gopls", "name": "gopls", "languages": ["go"], "extensions": ["go"], "command": "gopls",
                 "install": { "kind": "go", "package": "golang.org/x/tools/gopls@latest" } }]"#,
        )
        .unwrap()
        .remove(0);
        let toolchain = Toolchain::default();
        let sdk = go::GoSdk { root: PathBuf::from("/sdk/go"), version: None };
        let gopls = Gopls { path: PathBuf::from("/home/u/go/bin/gopls"), version: None };
        toolchain.state().found =
            Some(Found { sdks: Vec::new(), custom: Vec::new(), gopls: None, dlv: None, module_cache: None });
        assert_eq!(
            toolchain.install_command(&server).unwrap_err(),
            zen_lsp::NotInstallable::ToolMissing(zen_lsp::InstallKind::Go)
        );
        toolchain.state().found = Some(Found {
            sdks: vec![sdk.clone()],
            custom: Vec::new(),
            gopls: Some(gopls.clone()),
            dlv: None,
            module_cache: None,
        });
        assert_eq!(toolchain.locate(&server), Some(gopls.path));
        let command = toolchain.install_command(&server).unwrap();
        assert_eq!(command.get_program(), sdk.go().as_os_str());
        assert_eq!(command.get_args().collect::<Vec<_>>(), ["install", "golang.org/x/tools/gopls@latest"]);
        let env = ServerTools::environment(&toolchain, &server);
        assert_eq!(env[1], ("GOROOT".into(), "/sdk/go".into()));
        assert!(std::env::split_paths(&env[0].1).next() == Some(PathBuf::from("/sdk/go/bin")));
    }
}

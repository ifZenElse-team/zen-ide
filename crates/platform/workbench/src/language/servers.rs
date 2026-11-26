use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
pub use zen_lsp::servers::file_matches;
pub use zen_lsp::{
    InstallKind, InstallRecipe, NotInstallable, ServerDeclaration, ServerLog, ServerPlugin, ServerTools, SystemTools,
    VirtualDocuments,
};

use super::{Languages, Server, with_languages};
use crate::java::processing::{self, JavaLaunch};

pub const INSTALL_COMMAND: &str = "lsp.server.install";

const LOOK_AGAIN_AFTER: Duration = Duration::from_secs(30);

const JAVA_LOOK_AGAIN: Duration = Duration::from_secs(3);

const FAILURE_LINES: usize = 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    Running(Option<String>),
    Done,
    Failed(String),
}

/// Where a server's program runs from (ADR-0128).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ServerSource {
    /// Installed locally: in the IDE's own folder for it, `<data>/language-servers/<id>`.
    Local,
    /// Found on `PATH` or in the usual tool folders (installed by the user, or by a toolchain).
    Path,
    /// The program the user set in the server's settings.
    Custom,
    /// The project's own program, from its `node_modules/.bin` (`fromProject`, ADR-0136).
    Project,
    /// Not found.
    Missing,
}

pub fn project_program(command: &str, root: &Path) -> Option<PathBuf> {
    root.ancestors()
        .find_map(|folder| zen_lsp::servers::find_executable(command, &[folder.join("node_modules").join(".bin")]))
}

fn plugin_installed(folder: &Path, name: &str) -> bool {
    folder.join("node_modules").join(name).join("package.json").is_file()
}

pub fn with_plugins(options: Value, plugins: &[(ServerPlugin, PathBuf)]) -> Value {
    if plugins.is_empty() {
        return options;
    }
    let mut options = if options.is_object() { options } else { Value::Object(serde_json::Map::new()) };
    let Some(object) = options.as_object_mut() else { return options };
    let list = object.entry("plugins").or_insert_with(|| Value::Array(Vec::new()));
    if !list.is_array() {
        *list = Value::Array(Vec::new());
    }
    if let Some(list) = list.as_array_mut() {
        for (plugin, location) in plugins {
            let mut entry = serde_json::json!({ "name": plugin.name, "location": location.to_string_lossy() });
            if !plugin.languages.is_empty() {
                entry["languages"] = serde_json::json!(plugin.languages);
            }
            list.push(entry);
        }
    }
    options
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Configured {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    dismissed: BTreeSet<String>,
    servers: BTreeMap<String, Configured>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    chosen: BTreeMap<String, String>,
}

struct Inner {
    file: Option<PathBuf>,
    contributions: Vec<(String, ServerPlugin)>,
    plugin_stamps: BTreeMap<String, u64>,
    own_folder: Option<PathBuf>,
    tools: Arc<dyn ServerTools>,
    declarations: Vec<ServerDeclaration>,
    saved: Saved,
    installs: BTreeMap<String, Install>,
    located: BTreeMap<String, (Option<PathBuf>, Instant)>,
    versions: BTreeMap<PathBuf, Option<Option<String>>>,
    logs: BTreeMap<String, ServerLog>,
    java_finder: Option<JavaFinder>,
    java_homes: BTreeMap<(Option<PathBuf>, u32), (Option<PathBuf>, Instant)>,
    java_launches: BTreeMap<PathBuf, LaunchSlot>,
    java_reader: Option<JavaLaunchReader>,
    launched: BTreeMap<String, JavaLaunch>,
    launched_jdks: BTreeMap<String, Option<PathBuf>>,
    generation: u64,
}

pub type JavaFinder = Arc<dyn Fn(Option<&Path>, u32) -> Option<PathBuf> + Send + Sync>;

pub type JavaLaunchReader = Arc<dyn Fn(&Path) -> JavaLaunch + Send + Sync>;

#[derive(Default)]
struct LaunchSlot {
    found: Option<JavaLaunch>,
    read_at: Option<Instant>,
    reading: bool,
    generation: u64,
}

#[derive(Clone)]
pub struct LanguageServers {
    inner: Arc<Mutex<Inner>>,
}

impl Default for LanguageServers {
    fn default() -> Self {
        Self::load(None, Arc::new(SystemTools))
    }
}

impl LanguageServers {
    pub fn load(file: Option<PathBuf>, tools: Arc<dyn ServerTools>) -> Self {
        let saved: Saved = file
            .as_ref()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        let own_folder = file.as_ref().and_then(|file| Some(file.parent()?.join("language-servers")));
        let inner = Inner {
            file,
            contributions: Vec::new(),
            plugin_stamps: BTreeMap::new(),
            own_folder,
            tools,
            declarations: Vec::new(),
            saved,
            installs: BTreeMap::new(),
            located: BTreeMap::new(),
            versions: BTreeMap::new(),
            logs: BTreeMap::new(),
            java_finder: None,
            java_homes: BTreeMap::new(),
            java_launches: BTreeMap::new(),
            java_reader: None,
            launched: BTreeMap::new(),
            launched_jdks: BTreeMap::new(),
            generation: 0,
        };
        Self { inner: Arc::new(Mutex::new(inner)) }
    }

    pub(crate) fn same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn declare(&self, servers: impl IntoIterator<Item = ServerDeclaration>) {
        let mut inner = self.inner();
        for server in servers {
            if !inner.declarations.iter().any(|known| known.id == server.id) {
                inner.declarations.push(server);
            }
        }
        inner.generation += 1;
    }

    pub fn declarations(&self) -> Vec<ServerDeclaration> {
        self.inner().declarations.clone()
    }

    pub fn declaration(&self, id: &str) -> Option<ServerDeclaration> {
        self.inner().declarations.iter().find(|server| server.id == id).cloned()
    }

    pub fn generation(&self) -> u64 {
        self.inner().generation
    }

    pub fn configured(&self, id: &str) -> Configured {
        self.inner().saved.servers.get(id).cloned().unwrap_or_default()
    }

    pub fn located(&self, id: &str) -> Option<PathBuf> {
        let (server, tools) = {
            let inner = self.inner();
            if let Some((found, at)) = inner.located.get(id)
                && (found.as_ref().is_some_and(|path| path.is_file()) || at.elapsed() < LOOK_AGAIN_AFTER)
            {
                return found.clone();
            }
            (inner.declarations.iter().find(|server| server.id == id).cloned()?, Arc::clone(&inner.tools))
        };
        let found = if server.has_placeholders() {
            self.own_prefix(&server).and_then(|prefix| server.unpacked_marker(&prefix))
        } else {
            let own = self.own_prefix(&server).and_then(|prefix| zen_lsp::servers::locate_in_prefix(&server, &prefix));
            own.or_else(|| tools.locate(&server))
        };
        let mut inner = self.inner();
        let before = inner.located.insert(id.to_owned(), (found.clone(), Instant::now()));
        if before.is_some_and(|(was, _)| was != found) {
            inner.generation += 1;
        }
        found
    }

    pub fn own_prefix(&self, server: &ServerDeclaration) -> Option<PathBuf> {
        let folder = self.inner().own_folder.clone()?;
        zen_lsp::servers::npm_prefix(&folder, server)
    }

    fn install_command(&self, server: &ServerDeclaration) -> Result<std::process::Command, NotInstallable> {
        let tools = Arc::clone(&self.inner().tools);
        match self.own_prefix(server) {
            Some(prefix) => tools.install_command_into(server, &prefix),
            None => tools.install_command(server),
        }
    }

    pub fn executable(&self, id: &str) -> Option<PathBuf> {
        self.executable_in(id, None)
    }

    pub fn executable_in(&self, id: &str, root: Option<&Path>) -> Option<PathBuf> {
        if let Some(path) = self.configured(id).path {
            return zen_lsp::servers::find_executable(&path.to_string_lossy(), &zen_lsp::servers::search_folders());
        }
        let server = self.declaration(id)?;
        if server.from_project {
            return root.and_then(|root| project_program(&server.command, root));
        }
        self.located(id)
    }

    pub fn source(&self, id: &str) -> ServerSource {
        let Some(found) = self.executable(id) else { return ServerSource::Missing };
        if self.configured(id).path.is_some() {
            return ServerSource::Custom;
        }
        let own = self.declaration(id).and_then(|server| self.own_prefix(&server));
        if own.is_some_and(|prefix| found.starts_with(prefix)) { ServerSource::Local } else { ServerSource::Path }
    }

    pub fn is_enabled(&self, id: &str) -> bool {
        !self.configured(id).disabled && self.alternatives(id).first().is_none_or(|chosen| chosen == id)
    }

    pub fn alternatives(&self, id: &str) -> Vec<String> {
        let inner = self.inner();
        let Some(language) = language_of(&inner.declarations, id) else { return Vec::new() };
        let mut found: Vec<String> = inner
            .declarations
            .iter()
            .filter(|server| !server.alongside && server.languages.first() == Some(&language))
            .map(|server| server.id.clone())
            .collect();
        if found.len() < 2 {
            return Vec::new();
        }
        if let Some(at) = inner.saved.chosen.get(&language).and_then(|chosen| found.iter().position(|id| id == chosen))
        {
            let chosen = found.remove(at);
            found.insert(0, chosen);
        }
        found
    }

    pub fn choose(&self, id: &str) -> Result<(), String> {
        let mut inner = self.inner();
        let language = language_of(&inner.declarations, id).ok_or_else(|| t!("workbench-lsp-unknown", id = id))?;
        inner.saved.chosen.insert(language, id.to_owned());
        inner.generation += 1;
        save(&inner)
    }

    pub fn args(&self, id: &str) -> Vec<String> {
        self.configured(id).args.or_else(|| self.declaration(id).map(|server| server.args)).unwrap_or_default()
    }

    pub fn log(&self, id: &str) -> ServerLog {
        self.inner().logs.entry(id.to_owned()).or_default().clone()
    }

    pub fn logged_ids(&self) -> Vec<String> {
        self.inner().logs.keys().cloned().collect()
    }

    pub fn launcher(&self, id: &str, root: &Path) -> Result<zen_lsp::Launcher, String> {
        let server = self.declaration(id).ok_or_else(|| t!("workbench-lsp-unknown", id = id))?;
        let program = self
            .executable_in(id, Some(root))
            .ok_or_else(|| t!("workbench-lsp-missing", server = server.name.as_str()))?;
        let jdk = crate::jdk::for_server(&server, root);
        let launch = processing::names_launch(&server).then(|| self.read_java_launch(root));
        let agents = launch.as_ref().map(JavaLaunch::agents).unwrap_or_default();
        let mut args: Vec<String> = self
            .args(id)
            .iter()
            .flat_map(|arg| match arg.as_str() {
                processing::AGENTS => agents.clone(),
                _ => vec![crate::jdk::expand_text(arg, jdk.as_ref())],
            })
            .collect();
        if let Some(launch) = launch {
            self.inner().launched.insert(id.to_owned(), launch);
        }
        self.inner().launched_jdks.insert(id.to_owned(), jdk.as_ref().map(|jdk| jdk.jdk.home.clone()));
        let mut program = program;
        if self.configured(id).path.is_none() && server.has_placeholders() {
            (program, args) = self.launch_line(&server, root, args)?;
        }
        let mut env = self.environment(&server);
        crate::jdk::with_project_jdk(&mut env, jdk.as_ref());
        env.extend(self.project_environment(root).into_iter().map(|(name, value)| (name.into(), value.into())));
        if !crate::trust::is_trusted(&crate::build_tools::BuildEnv::global(), root) {
            env.extend(crate::trust::safe_environment());
        }
        let log = self.log(id);
        log.push(format!("{} {}", program.display(), args.join(" ")));
        Ok(zen_lsp::logged_process_launcher(program, args, root.to_owned(), env, Some(log)))
    }

    fn launch_line(
        &self,
        server: &ServerDeclaration,
        root: &Path,
        args: Vec<String>,
    ) -> Result<(PathBuf, Vec<String>), String> {
        let caches = self.inner().own_folder.as_ref().and_then(|folder| Some(folder.parent()?.join("caches")));
        let places = zen_lsp::LaunchPlaces {
            server_folder: self.own_prefix(server),
            java_home: server.java.and_then(|version| self.java_home(Some(root), version)),
            workspace_data: caches.map(|caches| archive::workspace_data(&caches, &server.id, root)),
        };
        let declared = ServerDeclaration { args, ..server.clone() };
        declared.launch_line(&places).map_err(|missing| match missing {
            zen_lsp::LaunchMissing::Java(version) => {
                t!("workbench-lsp-java-missing", server = server.name.as_str(), version = version.to_string())
            }
            zen_lsp::LaunchMissing::Server | zen_lsp::LaunchMissing::Other(_) => {
                t!("workbench-lsp-missing", server = server.name.as_str())
            }
        })
    }

    pub fn set_java_finder(&self, finder: JavaFinder) {
        let mut inner = self.inner();
        inner.java_finder = Some(finder);
        inner.java_homes.clear();
        inner.generation += 1;
    }

    pub fn java_home(&self, root: Option<&Path>, version: u32) -> Option<PathBuf> {
        let key = (root.map(Path::to_owned), version);
        if let Some((found, at)) = self.inner().java_homes.get(&key)
            && at.elapsed() < JAVA_LOOK_AGAIN
        {
            return found.clone();
        }
        let finder = self.inner().java_finder.clone();
        let new_enough = |home: &PathBuf| archive::java_version(home).is_some_and(|found| found >= version);
        let found = finder
            .and_then(|finder| finder(root, version))
            .filter(new_enough)
            .or_else(|| root.and_then(crate::jdk::project_home).filter(new_enough))
            .or_else(|| crate::jdk::newest_at_least(version).filter(new_enough));
        self.inner().java_homes.insert(key, (found.clone(), Instant::now()));
        found
    }

    pub fn set_java_launch_reader(&self, reader: Option<JavaLaunchReader>) {
        let mut inner = self.inner();
        inner.java_reader = reader;
        inner.java_launches.clear();
    }

    fn java_reader(&self) -> JavaLaunchReader {
        self.inner().java_reader.clone().unwrap_or_else(|| {
            Arc::new(|root: &Path| processing::launch(&crate::build_tools::BuildEnv::global(), root))
        })
    }

    pub fn read_java_launch(&self, root: &Path) -> JavaLaunch {
        let generation = self.inner().java_launches.get(root).map_or(0, |slot| slot.generation);
        let found = (self.java_reader())(root);
        self.keep_java_launch(root, generation, found.clone());
        found
    }

    fn keep_java_launch(&self, root: &Path, generation: u64, found: JavaLaunch) {
        let mut inner = self.inner();
        let slot = inner.java_launches.entry(root.to_owned()).or_default();
        slot.reading = false;
        if slot.generation == generation {
            slot.found = Some(found);
            slot.read_at = Some(Instant::now());
        }
    }

    pub fn known_java_launch(&self, root: &Path) -> Option<JavaLaunch> {
        let (found, generation) = {
            let mut inner = self.inner();
            let slot = inner.java_launches.entry(root.to_owned()).or_default();
            let due = slot.read_at.is_none_or(|at| at.elapsed() >= JAVA_LOOK_AGAIN);
            if !due || slot.reading {
                return slot.found.clone();
            }
            slot.reading = true;
            (slot.found.clone(), slot.generation)
        };
        let servers = self.clone();
        let root = root.to_owned();
        std::thread::spawn(move || {
            let read = (servers.java_reader())(&root);
            servers.keep_java_launch(&root, generation, read);
        });
        found
    }

    pub fn forget_java_launches(&self) {
        for slot in self.inner().java_launches.values_mut() {
            slot.read_at = None;
            slot.generation += 1;
        }
    }

    pub fn java_launch_change(&self, id: &str, root: &Path) -> Option<String> {
        let before = self.inner().launched.get(id).cloned()?;
        let now = self.known_java_launch(root)?;
        let notice = now.change_notice(&before, &self.declaration(id)?.name)?;
        self.inner().launched.insert(id.to_owned(), now);
        Some(notice)
    }

    pub fn project_jdk_change(&self, id: &str, root: &Path) -> Option<String> {
        let before = self.inner().launched_jdks.get(id).cloned()?;
        let server = self.declaration(id)?;
        let now = crate::jdk::for_server(&server, root);
        let home = now.as_ref().map(|now| now.jdk.home.clone());
        if home == before {
            return None;
        }
        self.inner().launched_jdks.insert(id.to_owned(), home);
        Some(t!("processing-restart-jdk", server = server.name.as_str(), version = now?.jdk.version.as_str()))
    }

    pub fn initialization_options(&self, id: &str, root: &Path) -> Value {
        let Some(server) = self.declaration(id) else { return Value::Null };
        let tools = Arc::clone(&self.inner().tools);
        let options = tools.initialization_options(&server, root);
        let options = zen_lsp::servers::expand_options(&options, root, self.executable(id).as_deref());
        let options = crate::jdk::expand(&options, crate::jdk::for_server(&server, root).as_ref());
        let options = if processing::names_launch(&server) {
            let options = processing::expand_options(&options, self.read_java_launch(root).processing);
            if crate::trust::is_trusted(&crate::build_tools::BuildEnv::global(), root) {
                options
            } else {
                processing::without_build_scripts(options)
            }
        } else {
            options
        };
        let options = if server.has_placeholders() {
            let places = zen_lsp::LaunchPlaces { server_folder: self.own_prefix(&server), ..Default::default() };
            zen_lsp::servers::expand_launch_options(&options, &places, server.java)
        } else {
            options
        };
        with_plugins(options, &self.installed_plugins(id))
    }

    fn installed_plugins(&self, id: &str) -> Vec<(ServerPlugin, PathBuf)> {
        let Some(folder) = self.plugins_folder(id) else { return Vec::new() };
        self.server_plugins(id)
            .into_iter()
            .filter(|plugin| plugin_installed(&folder, &plugin.name))
            .map(|plugin| (plugin, folder.clone()))
            .collect()
    }

    fn environment(&self, server: &ServerDeclaration) -> Vec<(OsString, OsString)> {
        let tools = Arc::clone(&self.inner().tools);
        tools.environment(server)
    }

    pub fn project_environment(&self, root: &Path) -> Vec<(String, String)> {
        let tools = Arc::clone(&self.inner().tools);
        tools.project_environment(root)
    }

    pub fn version(&self, id: &str) -> Option<String> {
        let server = self.declaration(id)?;
        let program = self.executable(id)?;
        if server.has_placeholders() && self.configured(id).path.is_none() {
            return archive::installed_version(&self.own_prefix(&server)?);
        }
        {
            let mut inner = self.inner();
            match inner.versions.get(&program) {
                Some(Some(version)) => return version.clone(),
                Some(None) => return None,
                None => {
                    inner.versions.insert(program.clone(), None);
                }
            }
        }
        let env = self.environment(&server);
        let servers = self.clone();
        std::thread::spawn(move || {
            let version = zen_lsp::servers::read_version(&program, &server.version_args, &env);
            servers.inner().versions.insert(program, Some(version));
        });
        None
    }

    pub fn installable(&self, id: &str) -> bool {
        self.install_check(id).is_ok()
    }

    pub fn install_check(&self, id: &str) -> Result<(), NotInstallable> {
        let server = self.declaration(id).ok_or_else(|| NotInstallable::Manual(String::new()))?;
        if let Some(recipe) = archive_recipe(&server) {
            return match self.own_prefix(&server) {
                Some(_) if recipe.archive_for(&zen_lsp::servers::platform()).is_some() => Ok(()),
                _ => Err(NotInstallable::Manual(recipe.hint.clone())),
            };
        }
        self.install_command(&server).map(drop)
    }

    pub fn install_line(&self, id: &str) -> Result<String, NotInstallable> {
        let server = self.declaration(id).ok_or_else(|| NotInstallable::Manual(String::new()))?;
        if let Some(recipe) = archive_recipe(&server) {
            self.install_check(id)?;
            let platform = zen_lsp::servers::platform();
            return Ok(recipe.archive_for(&platform).map(|(url, _)| url.to_owned()).unwrap_or_default());
        }
        let command = self.install_command(&server)?;
        let program = Path::new(command.get_program());
        let program = program.file_name().unwrap_or(program.as_os_str()).to_string_lossy().into_owned();
        Ok(std::iter::once(program)
            .chain(command.get_args().map(|arg| arg.to_string_lossy().into_owned()))
            .collect::<Vec<_>>()
            .join(" "))
    }

    pub fn install_state(&self, id: &str) -> Option<Install> {
        self.inner().installs.get(id).cloned()
    }

    pub fn install(&self, id: &str) -> Result<(), String> {
        let server = self.declaration(id).ok_or_else(|| t!("workbench-lsp-unknown", id = id))?;
        if archive_recipe(&server).is_some() {
            self.install_check(id).map_err(|why| not_installable(&server, &why))?;
            return self.run_archive_installation(&server);
        }
        if self.located(id).is_some() && !self.missing_plugins(id).is_empty() {
            return self.install_plugins(id);
        }
        let with_plugins = self.with_plugin_packages(&server, &self.server_plugins(id));
        let command = self.install_command(&with_plugins).map_err(|why| not_installable(&server, &why))?;
        self.run_installation(&server, command, false)
    }

    pub fn install_plugins(&self, id: &str) -> Result<(), String> {
        let server = self.declaration(id).ok_or_else(|| t!("workbench-lsp-unknown", id = id))?;
        let missing = self.missing_plugins(id);
        if missing.is_empty() {
            return Ok(());
        }
        let packages = missing.iter().map(ServerPlugin::package).collect::<Vec<_>>().join(" ");
        let only_plugins = ServerDeclaration {
            install: Some(InstallRecipe {
                kind: InstallKind::Npm,
                url: String::new(),
                sha256: String::new(),
                files: Vec::new(),
                package: packages,
                hint: String::new(),
                platforms: Default::default(),
            }),
            ..server.clone()
        };
        let command = self.install_command(&only_plugins).map_err(|why| not_installable(&server, &why))?;
        self.run_installation(&server, command, true)
    }

    pub fn plugins_installable(&self, id: &str) -> Result<(), NotInstallable> {
        let server = self.declaration(id).ok_or_else(|| NotInstallable::Manual(String::new()))?;
        let packages = self.server_plugins(id).iter().map(ServerPlugin::package).collect::<Vec<_>>().join(" ");
        let only_plugins = ServerDeclaration {
            install: Some(InstallRecipe {
                kind: InstallKind::Npm,
                url: String::new(),
                sha256: String::new(),
                files: Vec::new(),
                package: packages,
                hint: String::new(),
                platforms: Default::default(),
            }),
            ..server
        };
        self.install_command(&only_plugins).map(drop)
    }

    fn with_plugin_packages(&self, server: &ServerDeclaration, plugins: &[ServerPlugin]) -> ServerDeclaration {
        let mut server = server.clone();
        if let Some(recipe) = server.install.as_mut().filter(|recipe| recipe.kind == InstallKind::Npm) {
            for plugin in plugins {
                recipe.package.push(' ');
                recipe.package.push_str(plugin.package());
            }
        }
        server
    }

    fn run_archive_installation(&self, server: &ServerDeclaration) -> Result<(), String> {
        let id = server.id.clone();
        let platform = zen_lsp::servers::platform();
        let archive = archive_recipe(server).and_then(|recipe| recipe.archive_for(&platform));
        let files = archive_recipe(server).map(|recipe| recipe.files.clone()).unwrap_or_default();
        let (Some((url, sha256)), Some(folder)) =
            (archive.map(|(u, s)| (u.to_owned(), s.to_owned())), self.own_prefix(server))
        else {
            return Err(t!("workbench-lsp-missing", server = server.name.as_str()));
        };
        {
            let mut inner = self.inner();
            if matches!(inner.installs.get(&id), Some(Install::Running(_))) {
                return Ok(());
            }
            inner.installs.insert(id.clone(), Install::Running(None));
            inner.generation += 1;
        }
        let log = self.log(&id);
        log.push(t!("workbench-lsp-install-downloading-from", url = url.as_str()));
        if let Some(notice) = &server.notice {
            log.push(notice.text_in(&zen_i18n::language()));
        }
        let servers = self.clone();
        let name = server.name.clone();
        std::thread::spawn(move || {
            let mut shown = 0;
            let outcome = archive::download(&url, |done, total| {
                let megabytes = done >> 20;
                if megabytes != shown {
                    shown = megabytes;
                    let line = if total > 0 {
                        t!(
                            "workbench-lsp-install-downloaded-of",
                            done = megabytes.to_string(),
                            total = (total >> 20).to_string()
                        )
                    } else {
                        t!("workbench-lsp-install-downloaded", done = megabytes.to_string())
                    };
                    servers.inner().installs.insert(id.clone(), Install::Running(Some(line)));
                }
            })
            .and_then(|bytes| {
                let line = t!("workbench-lsp-install-unpacking", folder = folder.display().to_string());
                log.push(line.clone());
                servers.inner().installs.insert(id.clone(), Install::Running(Some(line)));
                archive::unpack(&bytes, &sha256, &url, &folder)
            })
            .and_then(|()| {
                for file in &files {
                    log.push(t!("workbench-lsp-install-downloading-from", url = file.url.as_str()));
                    let bytes = archive::download(&file.url, |_, _| {})?;
                    archive::place_file(&bytes, &file.sha256, &file.url, &archive::inside(&folder, &file.path)?)?;
                }
                Ok(())
            });
            servers.refresh();
            let state = match outcome {
                Ok(()) if servers.located(&id).is_some() => Install::Done,
                Ok(()) => Install::Failed(t!("workbench-lsp-install-not-found", server = name.as_str())),
                Err(message) => Install::Failed(t!("workbench-lsp-install-failed", detail = message)),
            };
            if let Install::Failed(message) = &state {
                log.push(message.clone());
            }
            let mut inner = servers.inner();
            inner.installs.insert(id.clone(), state);
            inner.generation += 1;
        });
        Ok(())
    }

    fn run_installation(
        &self,
        server: &ServerDeclaration,
        mut command: std::process::Command,
        plugins_only: bool,
    ) -> Result<(), String> {
        let id = server.id.clone();
        if let Some(prefix) = self.own_prefix(server) {
            std::fs::create_dir_all(&prefix)
                .map_err(|error| t!("workbench-lsp-install-cannot-run", detail = error.to_string()))?;
        }
        {
            let mut inner = self.inner();
            if matches!(inner.installs.get(&id), Some(Install::Running(_))) {
                return Ok(());
            }
            inner.installs.insert(id.clone(), Install::Running(None));
            inner.generation += 1;
        }
        let log = self.log(&id);
        let args: Vec<String> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        log.push(format!("{} {}", command.get_program().to_string_lossy(), args.join(" ")));
        let servers = self.clone();
        let name = server.name.clone();
        std::thread::spawn(move || {
            let outcome = run_install(&mut command, |line| {
                log.push(line);
                servers.inner().installs.insert(id.clone(), Install::Running(Some(line.to_owned())));
            });
            servers.refresh();
            let installed =
                if plugins_only { servers.missing_plugins(&id).is_empty() } else { servers.located(&id).is_some() };
            let state = match outcome {
                Ok(()) if installed => Install::Done,
                Ok(()) => Install::Failed(t!("workbench-lsp-install-not-found", server = name.as_str())),
                Err(message) => Install::Failed(message),
            };
            if let Install::Failed(message) = &state {
                log.push(message.clone());
            }
            let mut inner = servers.inner();
            inner.installs.insert(id.clone(), state);
            *inner.plugin_stamps.entry(id).or_default() += 1;
            inner.generation += 1;
        });
        Ok(())
    }

    pub fn contribute(&self, plugin: &str, plugins: impl IntoIterator<Item = ServerPlugin>) {
        let plugins: Vec<ServerPlugin> = plugins.into_iter().collect();
        let mut inner = self.inner();
        let mut touched: BTreeSet<String> = inner
            .contributions
            .iter()
            .filter(|(from, _)| from == plugin)
            .map(|(_, contributed)| contributed.server.clone())
            .collect();
        inner.contributions.retain(|(from, _)| from != plugin);
        for contributed in plugins {
            touched.insert(contributed.server.clone());
            inner.contributions.push((plugin.to_owned(), contributed));
        }
        for server in touched {
            *inner.plugin_stamps.entry(server).or_default() += 1;
        }
        inner.generation += 1;
    }

    pub fn withdraw(&self, plugin: &str) {
        self.contribute(plugin, Vec::new());
    }

    pub fn server_plugins(&self, id: &str) -> Vec<ServerPlugin> {
        let mut found: Vec<ServerPlugin> = Vec::new();
        for (_, contributed) in self.inner().contributions.iter().filter(|(_, contributed)| contributed.server == id) {
            if !found.iter().any(|known| known.name == contributed.name) {
                found.push(contributed.clone());
            }
        }
        found
    }

    pub fn plugins_folder(&self, id: &str) -> Option<PathBuf> {
        let server = self.declaration(id)?;
        self.own_prefix(&server)
    }

    pub fn missing_plugins(&self, id: &str) -> Vec<ServerPlugin> {
        let plugins = self.server_plugins(id);
        if plugins.is_empty() {
            return plugins;
        }
        let Some(folder) = self.plugins_folder(id) else { return Vec::new() };
        plugins.into_iter().filter(|plugin| !plugin_installed(&folder, &plugin.name)).collect()
    }

    pub fn missing_plugins_for(&self, id: &str, root: Option<&Path>) -> Vec<ServerPlugin> {
        self.missing_plugins(id)
            .into_iter()
            .filter(|plugin| match root {
                Some(root) => plugin.applies(root),
                None => plugin.when.is_none(),
            })
            .collect()
    }

    pub fn plugins_stamp(&self, id: &str) -> u64 {
        self.inner().plugin_stamps.get(id).copied().unwrap_or_default()
    }

    pub fn refresh(&self) {
        let tools = {
            let mut inner = self.inner();
            inner.located.clear();
            inner.versions.clear();
            inner.generation += 1;
            Arc::clone(&inner.tools)
        };
        tools.refresh();
    }

    pub fn forget(&self, id: &str) {
        let mut inner = self.inner();
        inner.located.remove(id);
        inner.generation += 1;
    }

    pub fn is_dismissed(&self, id: &str) -> bool {
        self.inner().saved.dismissed.contains(id)
    }

    pub fn set_dismissed(&self, id: &str, dismissed: bool) -> Result<(), String> {
        let mut inner = self.inner();
        if dismissed {
            inner.saved.dismissed.insert(id.to_owned());
        } else {
            inner.saved.dismissed.remove(id);
        }
        save(&inner)
    }

    pub fn configure(
        &self,
        id: &str,
        path: Option<Option<PathBuf>>,
        args: Option<Option<Vec<String>>>,
        enabled: Option<bool>,
    ) -> Result<(), String> {
        let mut inner = self.inner();
        let configured = inner.saved.servers.entry(id.to_owned()).or_default();
        if let Some(path) = path {
            configured.path = path.filter(|path| !path.as_os_str().is_empty());
        }
        if let Some(args) = args {
            configured.args = args;
        }
        if let Some(enabled) = enabled {
            configured.disabled = !enabled;
        }
        if *configured == Configured::default() {
            inner.saved.servers.remove(id);
        }
        inner.located.remove(id);
        inner.generation += 1;
        save(&inner)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeededServer {
    pub id: String,
    pub program: Option<PathBuf>,
    pub install: Option<Result<String, String>>,
}

impl LanguageServers {
    pub fn needed_by(&self, root: &Path) -> Vec<NeededServer> {
        let files = scan(root);
        self.declarations()
            .into_iter()
            .filter(|server| files.has_files_of(server) && self.is_enabled(&server.id))
            .map(|server| {
                let program = self.executable(&server.id);
                let install = program
                    .is_none()
                    .then(|| self.install_line(&server.id).map_err(|why| not_installable(&server, &why)));
                NeededServer { id: server.id, program, install }
            })
            .collect()
    }
}

fn language_of(declarations: &[ServerDeclaration], id: &str) -> Option<String> {
    declarations.iter().find(|server| server.id == id).and_then(|server| server.languages.first().cloned())
}

fn archive_recipe(server: &ServerDeclaration) -> Option<&InstallRecipe> {
    server.install.as_ref().filter(|recipe| recipe.kind == InstallKind::Archive)
}

pub fn not_installable(server: &ServerDeclaration, why: &NotInstallable) -> String {
    match why {
        NotInstallable::Manual(hint) if !hint.is_empty() => hint.clone(),
        NotInstallable::Manual(_) => t!("workbench-lsp-install-manual", server = server.name.as_str()),
        NotInstallable::ToolMissing(kind) => {
            t!("workbench-lsp-tool-missing", tool = kind.tool().unwrap_or_default(), server = server.name.as_str())
        }
    }
}

fn save(inner: &Inner) -> Result<(), String> {
    let Some(file) = &inner.file else { return Ok(()) };
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let json = serde_json::to_vec_pretty(&inner.saved).unwrap_or_default();
    zen_vfs::write_atomic(file, &json).map_err(|error| t!("workbench-lsp-save-failed", detail = error.to_string()))
}

fn run_install(command: &mut std::process::Command, mut line: impl FnMut(&str)) -> Result<(), String> {
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| t!("workbench-lsp-install-cannot-run", detail = error.to_string()))?;
    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        let sender = sender.clone();
        readers.push(std::thread::spawn(move || {
            for text in std::io::BufReader::new(stdout).lines().map_while(Result::ok) {
                let _ = sender.send(text);
            }
        }));
    }
    if let Some(stderr) = child.stderr.take() {
        let sender = sender.clone();
        readers.push(std::thread::spawn(move || {
            for text in std::io::BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = sender.send(text);
            }
        }));
    }
    drop(sender);
    let mut last = Vec::new();
    for text in receiver {
        line(&text);
        last.push(text);
        if last.len() > FAILURE_LINES {
            last.remove(0);
        }
    }
    for reader in readers {
        let _ = reader.join();
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    if status.success() { Ok(()) } else { Err(last.join("\n")) }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProjectFiles {
    extensions: BTreeSet<String>,
    names: BTreeSet<String>,
}

impl ProjectFiles {
    pub(super) fn has_files_of(&self, server: &ServerDeclaration) -> bool {
        server.extensions.iter().any(|extension| self.extensions.contains(&extension.to_ascii_lowercase()))
            || (!server.names.is_empty() && self.names.iter().any(|name| file_matches(&[], &server.names, name)))
    }
}

const SCAN_LIMIT: usize = 20_000;

const SKIPPED: &[&str] = &["node_modules", "target", "build", "dist", "out"];

pub(crate) fn scan(root: &Path) -> ProjectFiles {
    let mut files = ProjectFiles::default();
    let mut folders = vec![root.to_owned()];
    let mut seen = 0;
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            seen += 1;
            if seen > SCAN_LIMIT {
                return files;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                    folders.push(entry.path());
                }
                continue;
            }
            let lower = name.to_ascii_lowercase();
            if let Some((_, extension)) = lower.rsplit_once('.') {
                files.extensions.insert(extension.to_owned());
            }
            files.names.insert(lower);
        }
    }
    files
}

#[derive(Default)]
pub(crate) enum Scan {
    #[default]
    None,
    Running(PathBuf, std::sync::mpsc::Receiver<ProjectFiles>),
    Done(PathBuf, ProjectFiles),
}

/// The state of a language server, as `lsp.servers.list` gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Running.
    Running,
    /// Being started.
    Starting,
    /// It could not start, or stopped for good.
    Failed,
    /// Stopped by the user; `lsp.server.restart` starts it again.
    Stopped,
    /// Not installed (or the configured program does not exist).
    Missing,
    /// Installed, but no file it serves is open.
    NotStarted,
    /// Turned off by the user (`lsp.server.configure` with `enabled: false`).
    Disabled,
}

/// The program that installs a server, when it is missing, and where the user gets it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Installer {
    /// The program, e.g. `go`, `npm` or `cargo`.
    pub tool: String,
    /// The page to download it from, e.g. `https://go.dev/dl/`.
    pub download: String,
}

/// A language server, as `lsp.servers.list` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// The server's id, e.g. `gopls`.
    pub id: String,
    /// Its name, e.g. `gopls`.
    pub name: String,
    /// Its state.
    pub state: State,
    /// Its version, once its program has answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Its program: the one configured, else the one found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Where it runs from: `local` (installed in the IDE's own folder), `path` (found on `PATH`),
    /// `custom` (the path set in its settings) or `missing`; `path` for a server given in code.
    pub source: ServerSource,
    /// The arguments it starts with.
    pub args: Vec<String>,
    /// How many open files it serves.
    pub files: u32,
    /// How many messages wait to be written to it: they are written by a thread of their own, so
    /// a server that reads slowly never holds up the window (0 when it does not run).
    pub queued: u32,
    /// The language ids of its files.
    pub languages: Vec<String>,
    /// The file extensions it serves, without the dot.
    pub extensions: Vec<String>,
    /// The file name patterns it serves, where `*` stands for any characters.
    pub names: Vec<String>,
    /// Whether it may start: the user has not turned it off.
    pub enabled: bool,
    /// What to tell the user: why it failed, how an installation goes, how to install it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Whether `lsp.server.install` can install it.
    pub installable: bool,
    /// The missing program that would install it, with where to download it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installer: Option<Installer>,
    /// Whether an installation started with `lsp.server.install` runs.
    pub installing: bool,
    /// The plug-ins other plug-ins contribute to it that are not installed yet and are for this
    /// project (their `when`, ADR-0134), by package name: `lsp.server.install` installs them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_plugins: Vec<String>,
    /// Whether the project has files it serves, or the user opened one.
    pub needed: bool,
    /// Whether the user said "Don't ask again" about installing it.
    pub dismissed: bool,
    /// Whether it runs and is busy: it reports work in progress, or says it has not finished
    /// loading the project (rust-analyzer). Its answers may be incomplete until it is not.
    pub busy: bool,
    /// The work it reports in progress (`$/progress`), oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub progress: Vec<super::activity::ServerProgress>,
    /// How long after it started it was first not busy, in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ready_after_ms: Option<u64>,
    /// The servers that serve its language in its place, the chosen one first (`lsp.server.choose`);
    /// empty when it has no alternative.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alternatives: Vec<String>,
    /// What the user is told where it is offered and installed, in the interface language (ADR-0151).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    /// Where to read more about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notice_url: Option<String>,
    /// The Java version it needs to run and no JDK found has (Eclipse JDT LS: 25): download one
    /// with `toolchain.jdk.download` (ADR-0153).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub java_needed: Option<u32>,
}

impl Languages {
    pub fn server_list(&mut self, root: Option<&Path>) -> Vec<ServerInfo> {
        if root.is_some() {
            self.set_root(root);
        }
        self.refresh_entries();
        let project = root.and_then(|root| self.project_files(root));
        let registry = self.registry.clone();
        self.entries
            .iter()
            .map(|entry| {
                let id = entry.declaration.id.as_str();
                let declared = entry.declared;
                let install = if declared { registry.install_state(id) } else { None };
                let installing = matches!(install, Some(Install::Running(_)));
                let enabled = !declared || registry.is_enabled(id);
                let check =
                    if declared { registry.install_check(id) } else { Err(NotInstallable::Manual(String::new())) };
                let activity = self.activity.get(id);
                let queued = match self.servers.get(id) {
                    Some(Server::Running { client, .. }) => u32::try_from(client.queued()).unwrap_or(u32::MAX),
                    _ => 0,
                };
                let (state, mut message, files) = match self.servers.get(id) {
                    _ if !enabled => (State::Disabled, None, 0),
                    Some(Server::Starting(_)) => (State::Starting, None, 0),
                    Some(Server::Running { client, files }) if client.is_running() => {
                        (State::Running, None, files.len() as u32)
                    }
                    Some(Server::Running { .. }) => (State::Failed, Some(t!("workbench-language-not-running")), 0),
                    Some(Server::Failed { reason, .. }) => (State::Failed, Some(reason.text()), 0),
                    Some(Server::Stopped) => (State::Stopped, None, 0),
                    None if entry.available => (State::NotStarted, None, 0),
                    None => {
                        let message = match &check {
                            Err(why) => not_installable(&entry.declaration, why),
                            Ok(()) => String::new(),
                        };
                        (State::Missing, Some(message), 0)
                    }
                };
                match &install {
                    Some(Install::Running(line)) => {
                        message = Some(t!("workbench-lsp-installing", detail = line.clone().unwrap_or_default()));
                    }
                    Some(Install::Failed(failure)) if state == State::Missing => {
                        let last = failure.lines().last().unwrap_or_default();
                        message = Some(t!("workbench-lsp-install-failed", detail = last));
                    }
                    _ => {}
                }
                let configured = if declared { registry.configured(id) } else { Configured::default() };
                let path =
                    if declared { configured.path.clone().or_else(|| registry.executable_in(id, root)) } else { None };
                let source = match (declared, entry.available) {
                    (true, true) if entry.declaration.from_project && registry.configured(id).path.is_none() => {
                        ServerSource::Project
                    }
                    (true, _) => registry.source(id),
                    (false, true) => ServerSource::Path,
                    (false, false) => ServerSource::Missing,
                };
                let missing_plugins = if declared {
                    registry.missing_plugins_for(id, root).into_iter().map(|plugin| plugin.name).collect()
                } else {
                    Vec::new()
                };
                ServerInfo {
                    id: id.to_owned(),
                    name: entry.declaration.name.clone(),
                    missing_plugins,
                    source,
                    state,
                    version: if declared && enabled && entry.available { registry.version(id) } else { None },
                    path,
                    args: if declared { registry.args(id) } else { entry.declaration.args.clone() },
                    files,
                    queued,
                    languages: entry.declaration.languages.clone(),
                    extensions: entry.declaration.extensions.clone(),
                    names: entry.declaration.names.clone(),
                    enabled,
                    message: message
                        .or_else(|| {
                            let running = state == State::Running;
                            activity.filter(|_| running).and_then(super::activity::Activity::status).map(str::to_owned)
                        })
                        .filter(|message| !message.is_empty()),
                    installable: declared && check.is_ok(),
                    installer: match check {
                        Err(NotInstallable::ToolMissing(kind)) if state == State::Missing => Some(Installer {
                            tool: kind.tool().unwrap_or_default().to_owned(),
                            download: kind.download().unwrap_or_default().to_owned(),
                        }),
                        _ => None,
                    },
                    installing,
                    needed: self.opened.contains(id)
                        || project.as_ref().is_some_and(|files| files.has_files_of(&entry.declaration)),
                    dismissed: declared && registry.is_dismissed(id),
                    busy: state == State::Running && activity.is_some_and(super::activity::Activity::busy),
                    progress: if state == State::Running {
                        activity.map(super::activity::Activity::progress).unwrap_or_default()
                    } else {
                        Vec::new()
                    },
                    ready_after_ms: activity.and_then(super::activity::Activity::ready_after),
                    alternatives: if declared { registry.alternatives(id) } else { Vec::new() },
                    notice: entry.declaration.notice.as_ref().map(|notice| notice.text_in(&zen_i18n::language())),
                    notice_url: entry.declaration.notice.as_ref().and_then(|notice| notice.link()).map(str::to_owned),
                    java_needed: if declared && entry.available && registry.configured(id).path.is_none() {
                        entry.declaration.java.filter(|&version| registry.java_home(root, version).is_none())
                    } else {
                        None
                    },
                }
            })
            .collect()
    }

    pub(super) fn project_servers(&mut self, root: &Path) -> Vec<String> {
        self.refresh_entries();
        let project = self.project_files(root);
        self.entries
            .iter()
            .filter(|entry| entry.available && entry.enabled)
            .filter(|entry| {
                !entry.declared || project.as_ref().is_some_and(|files| files.has_files_of(&entry.declaration))
            })
            .map(|entry| entry.id().to_owned())
            .collect()
    }

    pub(super) fn project_files(&mut self, root: &Path) -> Option<ProjectFiles> {
        match std::mem::take(&mut self.scan) {
            Scan::Done(folder, files) if folder == root => {
                self.scan = Scan::Done(folder, files.clone());
                Some(files)
            }
            Scan::Running(folder, receiver) if folder == root => match receiver.try_recv() {
                Ok(files) => {
                    self.scan = Scan::Done(folder, files.clone());
                    Some(files)
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.scan = Scan::Running(folder, receiver);
                    None
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.scan = Scan::Done(folder, ProjectFiles::default());
                    None
                }
            },
            _ => {
                let (sender, receiver) = std::sync::mpsc::channel();
                let folder = root.to_owned();
                std::thread::spawn(move || {
                    let _ = sender.send(scan(&folder));
                });
                self.scan = Scan::Running(root.to_owned(), receiver);
                None
            }
        }
    }

    fn known(&mut self, id: &str) -> Result<(), CommandError> {
        self.refresh_entries();
        if self.entries.iter().any(|entry| entry.declaration.id == id) {
            Ok(())
        } else {
            Err(CommandError::new(t!("workbench-lsp-unknown", id = id)))
        }
    }

    pub fn stop_server(&mut self, id: &str) {
        let startable = self.entries.iter().any(|entry| entry.id() == id && entry.available && entry.enabled);
        if !startable && !self.servers.contains_key(id) {
            return;
        }
        self.activity.remove(id);
        if let Some(server) = self.servers.insert(id.to_owned(), Server::Stopped) {
            std::thread::spawn(move || drop(server));
        }
        self.registry.log(id).push(t!("workbench-lsp-stopped-by-user"));
    }

    pub fn restart_server(&mut self, id: &str) {
        self.activity.remove(id);
        if let Some(server) = self.servers.remove(id) {
            std::thread::spawn(move || drop(server));
        }
        self.progress.remove(id);
        self.registry.forget(id);
    }

    pub(super) fn scanning(&self, root: &Path) -> bool {
        matches!(&self.scan, Scan::Running(folder, _) if folder == root)
    }
}

/// A server's id.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct IdParams {
    /// The server's id, as `lsp.servers.list` gives it, e.g. `gopls`.
    pub id: String,
}

/// What `lsp.servers.list` gives.
#[derive(Serialize, JsonSchema)]
pub struct ServerList {
    /// Every server the IDE knows, in the order declared.
    pub servers: Vec<ServerInfo>,
}

pub struct ListServers;

impl Command for ListServers {
    type Params = NoParams;
    type Output = ServerList;
    const ID: &'static str = "lsp.servers.list";
    const TITLE: &'static str = "Language Servers";
    const DESCRIPTION: &'static str = "Every language server the IDE knows (declared by language modules and plug-ins) with its \
         state, version, program, arguments, open files, whether the project needs it, whether it can be installed \
         and whether the user dismissed the offer to install it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ServerList, CommandError> {
        Ok(with_languages(session, |languages, workbench| ServerList {
            servers: languages.server_list(workbench.project()),
        }))
    }
}

/// What `lsp.server.install` gives.
#[derive(Serialize, JsonSchema)]
pub struct Started {
    /// Always true: the installation runs in the background; `lsp.servers.list` follows it.
    pub started: bool,
}

pub struct InstallServer;

impl Command for InstallServer {
    type Params = IdParams;
    type Output = Started;
    const ID: &'static str = "lsp.server.install";
    const TITLE: &'static str = "Install Language Server";
    const DESCRIPTION: &'static str = "Installs a language server with the tool its declaration names (go install; npm install \
         --prefix into the IDE's own folder for it, ADR-0127; cargo install) in the background; only when asked, never on its own. lsp.servers.list shows the progress.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<Started, CommandError> {
        let registry = session.state_mut::<LanguageServers>().clone();
        registry.install(&params.id).map_err(CommandError::new)?;
        Ok(Started { started: true })
    }
}

pub struct RestartServer;

impl Command for RestartServer {
    type Params = IdParams;
    type Output = NoParams;
    const ID: &'static str = "lsp.server.restart";
    const TITLE: &'static str = "Restart Language Server";
    const DESCRIPTION: &'static str = "Stops a language server of this window and starts it again (with its current program and \
         arguments) when a file it serves is open.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<NoParams, CommandError> {
        with_languages(session, |languages, workbench| {
            languages.known(&params.id)?;
            languages.restart_server(&params.id);
            languages.sync(workbench);
            Ok(NoParams {})
        })
    }
}

pub struct StopServer;

impl Command for StopServer {
    type Params = IdParams;
    type Output = NoParams;
    const ID: &'static str = "lsp.server.stop";
    const TITLE: &'static str = "Stop Language Server";
    const DESCRIPTION: &'static str =
        "Stops a language server of this window; it stays stopped until lsp.server.restart.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<NoParams, CommandError> {
        with_languages(session, |languages, _| {
            languages.known(&params.id)?;
            languages.stop_server(&params.id);
            Ok(NoParams {})
        })
    }
}

/// The parameters of `lsp.server.log`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct LogParams {
    /// The server's id.
    pub id: String,
    /// The number of the first line wanted: `next` of the previous call; from the oldest kept when absent.
    #[serde(default)]
    pub since: Option<u64>,
}

/// What `lsp.server.log` gives.
#[derive(Serialize, JsonSchema)]
pub struct LogLines {
    /// The lines: the server's standard error, the JSON-RPC trace and installations.
    pub lines: Vec<String>,
    /// The number of the next line, for the next call's `since`.
    pub next: u64,
}

pub struct ServerLogCommand;

impl Command for ServerLogCommand {
    type Params = LogParams;
    type Output = LogLines;
    const ID: &'static str = "lsp.server.log";
    const TITLE: &'static str = "Show Language Server Log";
    const DESCRIPTION: &'static str = "The last lines a language server wrote to its standard error, the JSON-RPC trace of its \
         messages and the output of its installations; `since` reads only the lines after an earlier call.";

    fn run(&self, session: &mut Session, params: LogParams) -> Result<LogLines, CommandError> {
        with_languages(session, |languages, _| languages.known(&params.id))?;
        let (lines, next) = session.state_mut::<LanguageServers>().log(&params.id).since(params.since.unwrap_or(0));
        Ok(LogLines { lines, next })
    }
}

pub struct DismissServer<const DISMISS: bool>;

impl<const DISMISS: bool> Command for DismissServer<DISMISS> {
    type Params = IdParams;
    type Output = NoParams;
    const ID: &'static str = if DISMISS { "lsp.server.dismiss" } else { "lsp.server.undismiss" };
    const TITLE: &'static str = if DISMISS { "Don't Ask Again" } else { "Offer to Install Again" };
    const DESCRIPTION: &'static str = if DISMISS {
        "Stops offering to install a language server (\"Don't ask again\"), remembered in the IDE's settings."
    } else {
        "Offers to install a language server again after lsp.server.dismiss."
    };

    fn run(&self, session: &mut Session, params: IdParams) -> Result<NoParams, CommandError> {
        with_languages(session, |languages, _| languages.known(&params.id))?;
        let registry = session.state_mut::<LanguageServers>().clone();
        registry.set_dismissed(&params.id, DISMISS).map_err(CommandError::new)?;
        Ok(NoParams {})
    }
}

fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// The parameters of `lsp.server.configure`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ConfigureParams {
    /// The server's id.
    pub id: String,
    /// The program to start instead of the one found; `null` or `""` goes back to the one found; absent keeps it.
    #[serde(default, deserialize_with = "present")]
    pub path: Option<Option<PathBuf>>,
    /// The arguments to pass instead of the declared ones; `null` goes back to them; absent keeps them.
    #[serde(default, deserialize_with = "present")]
    pub args: Option<Option<Vec<String>>>,
    /// Whether the server may start; absent keeps it.
    #[serde(default)]
    pub enabled: Option<bool>,
}

pub struct ConfigureServer;

impl Command for ConfigureServer {
    type Params = ConfigureParams;
    type Output = NoParams;
    const ID: &'static str = "lsp.server.configure";
    const TITLE: &'static str = "Configure Language Server";
    const DESCRIPTION: &'static str = "Sets the program and the arguments a language server starts with and whether it may start \
         at all (enabled), remembered in the IDE's settings; the server restarts in this window with them, other windows \
         use them from its next start.";

    fn run(&self, session: &mut Session, params: ConfigureParams) -> Result<NoParams, CommandError> {
        let registry = session.state_mut::<LanguageServers>().clone();
        if registry.declaration(&params.id).is_none() {
            return Err(CommandError::new(t!("workbench-lsp-not-configurable", id = params.id.as_str())));
        }
        registry.configure(&params.id, params.path, params.args, params.enabled).map_err(CommandError::new)?;
        with_languages(session, |languages, workbench| {
            if languages.servers.get(&params.id).is_some_and(|server| !matches!(server, Server::Stopped)) {
                languages.restart_server(&params.id);
            }
            languages.sync(workbench);
        });
        Ok(NoParams {})
    }
}

pub struct ChooseServer;

impl Command for ChooseServer {
    type Params = IdParams;
    type Output = NoParams;
    const ID: &'static str = "lsp.server.choose";
    const TITLE: &'static str = "Choose Language Server";
    const DESCRIPTION: &'static str = "Chooses the server that serves a language several declared servers serve (Kotlin: \
         kotlin-lsp, the default, or kotlin-language-server, fwcd's), as Settings → Language Servers does; \
         remembered in the IDE's settings. The other one stops, the chosen one starts (or is offered for install). \
         lsp.servers.list gives each server's alternatives, the chosen one first.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<NoParams, CommandError> {
        let registry = session.state_mut::<LanguageServers>().clone();
        let others = registry.alternatives(&params.id);
        if others.is_empty() {
            return Err(CommandError::new(t!("workbench-lsp-no-alternative", id = params.id.as_str())));
        }
        registry.choose(&params.id).map_err(CommandError::new)?;
        with_languages(session, |languages, workbench| {
            for id in &others {
                if languages.servers.get(id).is_some_and(|server| !matches!(server, Server::Stopped)) {
                    languages.restart_server(id);
                }
            }
            languages.sync(workbench);
        });
        Ok(NoParams {})
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListServers)?;
    registry.register(InstallServer)?;
    registry.register(RestartServer)?;
    registry.register(StopServer)?;
    registry.register(ServerLogCommand)?;
    registry.register(DismissServer::<true>)?;
    registry.register(DismissServer::<false>)?;
    registry.register(ConfigureServer)?;
    registry.register(ChooseServer)?;
    Ok(())
}

pub mod archive;

#[cfg(test)]
mod tests;

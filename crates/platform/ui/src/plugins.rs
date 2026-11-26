use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use zen_i18n::t;
use zen_plugins::{Catalog, CatalogPlugin, CatalogVersion, Installed, InstalledPlugin, WasmHighlighter};
use zen_syntax::{Highlighter, TextFeatures};
use zen_workbench::language::servers::{
    Install, InstallKind, LanguageServers, NotInstallable, ServerDeclaration, ServerSource,
};
use zen_workbench::plugin_manager::{
    CatalogStatus, PluginDetails, PluginList, PluginManager, PluginSummary, Queued, VersionNotes, clip_notes, sort,
};

#[derive(Clone, Debug)]
pub struct PluginConfig {
    pub folder: PathBuf,
    pub catalog_url: Option<String>,
    pub public_key: String,
    pub bundled: Option<PathBuf>,
    pub optional: Vec<String>,
}

pub fn bundled_folder() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("ZEN_BUNDLED_PLUGINS").map(PathBuf::from) {
        return Some(folder);
    }
    let exe = std::env::current_exe().ok()?;
    let folder = exe.parent()?.parent()?.join("Resources").join("plugins");
    folder.is_dir().then_some(folder)
}

pub trait PluginSource: Send + Sync {
    fn catalog(&self) -> Result<Catalog, String>;
    fn module(&self, plugin: &CatalogPlugin, version: &CatalogVersion) -> Result<Vec<u8>, String>;
    fn file(&self, url: &str) -> Result<Vec<u8>, String> {
        Err(format!("{url} cannot be downloaded here"))
    }
}

const MAX_DRIVER_PROGRAM: u64 = 1 << 30;

struct ReleaseServer {
    url: String,
    public_key: String,
}

impl PluginSource for ReleaseServer {
    #[cfg(not(target_arch = "wasm32"))]
    fn catalog(&self) -> Result<Catalog, String> {
        zen_plugins::fetch::catalog(&self.url, &self.public_key).map_err(|error| error.to_string())
    }

    #[cfg(target_arch = "wasm32")]
    fn catalog(&self) -> Result<Catalog, String> {
        Err(format!("{} cannot be fetched in the browser", self.url))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn module(&self, plugin: &CatalogPlugin, version: &CatalogVersion) -> Result<Vec<u8>, String> {
        zen_plugins::fetch::module(&self.url, plugin, version).map_err(|error| error.to_string())
    }

    #[cfg(target_arch = "wasm32")]
    fn module(&self, plugin: &CatalogPlugin, _version: &CatalogVersion) -> Result<Vec<u8>, String> {
        Err(format!("{} cannot be downloaded in the browser", plugin.id))
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn file(&self, url: &str) -> Result<Vec<u8>, String> {
        zen_plugins::fetch::file(&self.url, url, MAX_DRIVER_PROGRAM).map_err(|error| error.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PluginEntry {
    #[serde(flatten)]
    pub plugin: InstalledPlugin,
    pub bundled: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct CatalogEntry {
    #[serde(flatten)]
    pub plugin: CatalogPlugin,
    pub bundled: bool,
    pub installed: Option<String>,
}

#[derive(Debug)]
enum CatalogState {
    NotFetched,
    Fetching(Option<Arc<Catalog>>),
    Ready(Arc<Catalog>),
    Failed,
}

#[derive(Clone, Debug)]
pub enum CatalogView {
    Loading,
    Ready(Arc<Catalog>),
    Unavailable,
}

fn tool_name(kind: InstallKind) -> &'static str {
    match kind {
        InstallKind::Npm => "Node.js (npm)",
        InstallKind::Go => "Go",
        InstallKind::Cargo | InstallKind::Rustup => "Rust (rustup)",
        InstallKind::Archive | InstallKind::Manual => "",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Offer {
    Install { plugin: Box<CatalogPlugin>, extension: String },
    Installing { name: String },
}

#[derive(Clone, Debug)]
struct Job {
    plugin: CatalogPlugin,
    version: CatalogVersion,
    servers: bool,
}

struct State {
    catalog: CatalogState,
    highlighters: Vec<(String, Arc<WasmHighlighter>)>,
    ignored: BTreeSet<String>,
    queue: VecDeque<Job>,
    working: bool,
    update_when_fetched: bool,
    check_failed: Option<String>,
    restart: BTreeSet<String>,
    messages: Vec<String>,
    file_types: BTreeSet<String>,
    ignored_plugins: BTreeSet<String>,
    generation: u64,
    installed: BTreeSet<String>,
    loading: usize,
}

pub struct Plugins {
    config: Option<PluginConfig>,
    source: Option<Arc<dyn PluginSource>>,
    this: Weak<Plugins>,
    repaint: Mutex<Option<egui::Context>>,
    state: Mutex<State>,
    servers: Mutex<Option<LanguageServers>>,
    generator_archive: zen_plugins::generator::ArchiveMemo,
    templates: Mutex<Option<zen_workbench::templates::LiveTemplates>>,
    #[cfg(not(target_arch = "wasm32"))]
    drivers: zen_db::external::Declared,
}

impl Plugins {
    pub fn new(config: Option<PluginConfig>) -> Arc<Self> {
        let source = config.as_ref().and_then(|config| {
            let url = config.catalog_url.clone()?;
            Some(Arc::new(ReleaseServer { url, public_key: config.public_key.clone() }) as Arc<dyn PluginSource>)
        });
        Self::with_source(config, source)
    }

    pub fn with_source(config: Option<PluginConfig>, source: Option<Arc<dyn PluginSource>>) -> Arc<Self> {
        let mut highlighters: Vec<(String, Arc<WasmHighlighter>)> = Vec::new();
        let mut messages = Vec::new();
        let mut ignored = BTreeSet::new();
        let mut ignored_plugins = BTreeSet::new();
        let mut installed = BTreeSet::new();
        if let Some(config) = &config {
            let switches = Switches::read(config);
            let all = entries(config, &switches);
            installed = all.iter().map(|entry| entry.plugin.id.clone()).collect();
            for entry in all.into_iter().filter(|entry| entry.enabled) {
                match load(config, &entry.plugin) {
                    Ok(highlighter) => highlighters.push((entry.plugin.id.clone(), Arc::new(highlighter))),
                    Err(problem) => messages.push(problem.to_string()),
                }
            }
            ignored = read_set(&ignored_file(config));
            ignored_plugins = read_set(&ignored_plugins_file(config));
        }
        Arc::new_cyclic(|this| Self {
            config,
            source,
            this: this.clone(),
            repaint: Mutex::new(None),
            generator_archive: zen_plugins::generator::ArchiveMemo::default(),
            servers: Mutex::new(None),
            templates: Mutex::new(None),
            #[cfg(not(target_arch = "wasm32"))]
            drivers: zen_db::external::Declared::default(),
            state: Mutex::new(State {
                catalog: CatalogState::NotFetched,
                highlighters,
                ignored,
                queue: VecDeque::new(),
                working: false,
                update_when_fetched: false,
                check_failed: None,
                restart: BTreeSet::new(),
                messages,
                file_types: BTreeSet::new(),
                ignored_plugins,
                generation: 0,
                installed,
                loading: 0,
            }),
        })
        .with_file_types()
    }

    fn with_file_types(self: Arc<Self>) -> Arc<Self> {
        self.sync_file_types();
        let problems = self.sync_drivers();
        self.state().messages.extend(problems);
        self
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn test_supports(&self) -> Vec<Arc<dyn zen_run::test_support::TestSupport>> {
        self.entries()
            .into_iter()
            .filter(|entry| entry.enabled && !entry.plugin.tests.is_empty())
            .flat_map(|entry| {
                zen_run::test_support::DeclaredTests::parse_list(&serde_json::Value::Array(entry.plugin.tests))
            })
            .map(|support| Arc::new(support) as Arc<dyn zen_run::test_support::TestSupport>)
            .collect()
    }

    pub fn terminal_agents(&self) -> Vec<(serde_json::Value, bool)> {
        self.entries()
            .into_iter()
            .filter(|entry| entry.enabled)
            .flat_map(|entry| {
                let bundled = entry.bundled;
                entry.plugin.terminal_agents.into_iter().map(move |adapter| (adapter, bundled))
            })
            .collect()
    }

    pub fn database_drivers(&self) -> Vec<Arc<dyn zen_db::driver::Driver>> {
        self.drivers.as_drivers()
    }

    fn sync_drivers(&self) -> Vec<String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(config) = &self.config else { return Vec::new() };
            let (specs, problems) = driver_specs(config, &self.entries());
            self.drivers.set(specs);
            problems
        }
        #[cfg(target_arch = "wasm32")]
        Vec::new()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn install_drivers(&self, plugin: &CatalogPlugin) -> Vec<String> {
        let (Some(config), Some(source)) = (&self.config, &self.source) else { return Vec::new() };
        if plugin.database_drivers.is_empty() {
            return Vec::new();
        }
        let declarations = match zen_db::external::DriverDeclaration::parse_list(&serde_json::Value::Array(
            plugin.database_drivers.clone(),
        )) {
            Ok(declarations) => declarations,
            Err(error) => return vec![t!("ide-plugin-driver-invalid", name = plugin.name.as_str(), error = error)],
        };
        let installed = Installed::new(&config.folder);
        let mut messages = Vec::new();
        for declaration in declarations {
            let Some((binary, file)) = declaration.binary().and_then(|binary| Some((binary, binary.file_name()?)))
            else {
                messages.push(t!(
                    "ide-plugin-driver-no-platform",
                    name = plugin.name.as_str(),
                    driver = declaration.name.as_str(),
                    platform = zen_db::external::platform()
                ));
                continue;
            };
            let path = installed.driver_program(&plugin.id, &declaration.id, &file);
            let current = std::fs::read(&path)
                .is_ok_and(|bytes| zen_update::sha256_hex(&bytes).eq_ignore_ascii_case(binary.sha256.trim()));
            if current {
                continue;
            }
            let done = source.file(&binary.url).and_then(|bytes| {
                installed
                    .install_driver_program(&plugin.id, &declaration.id, &file, &binary.sha256, &bytes)
                    .map_err(|error| error.to_string())
            });
            messages.push(match done {
                Ok(_) => {
                    t!("ide-plugin-driver-installed", name = plugin.name.as_str(), driver = declaration.name.as_str())
                }
                Err(error) => t!(
                    "ide-plugin-driver-failed",
                    name = plugin.name.as_str(),
                    driver = declaration.name.as_str(),
                    error = error
                ),
            });
        }
        messages
    }

    fn sync_file_types(&self) {
        let loaded: BTreeSet<String> = self.state().highlighters.iter().map(|(id, _)| id.clone()).collect();
        let types: Vec<zen_vfs::file_types::FileType> = self
            .installed()
            .into_iter()
            .filter(|plugin| loaded.contains(&plugin.id))
            .filter(|plugin| {
                ![zen_vfs::file_types::TEXT, zen_vfs::file_types::DETECTED, zen_vfs::file_types::NATIVE]
                    .contains(&plugin.id.as_str())
            })
            .filter(|plugin| !plugin.extensions.is_empty() || !plugin.names.is_empty() || !plugin.hashbangs.is_empty())
            .map(|plugin| file_type(&plugin))
            .collect();
        let gone: Vec<String> = {
            let mut state = self.state();
            state.generation += 1;
            let gone = state.file_types.iter().filter(|id| !loaded.contains(*id)).cloned().collect();
            state.file_types = types.iter().map(|ty| ty.id.clone()).collect();
            gone
        };
        for id in gone {
            zen_vfs::file_types::undeclare(&id);
        }
        zen_vfs::file_types::declare(types);
        self.sync_templates();
    }

    pub fn set_live_templates(&self, templates: zen_workbench::templates::LiveTemplates) {
        *self.templates.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(templates);
        self.sync_templates();
    }

    fn sync_templates(&self) {
        let Some(templates) = self.templates.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() else {
            return;
        };
        let loaded: BTreeSet<String> = self.state().highlighters.iter().map(|(id, _)| id.clone()).collect();
        let declared = self
            .installed()
            .into_iter()
            .filter(|plugin| loaded.contains(&plugin.id) && !plugin.live_templates.is_empty())
            .map(|plugin| {
                let live = &plugin.live_templates;
                zen_workbench::templates::PluginTemplates::parse(&plugin.id, &live.templates, &live.postfix)
                    .with_assists(&live.doc_comments, &live.statements)
                    .with_surrounders(&live.surrounders)
            })
            .collect();
        templates.set_plugin_templates(declared);
    }

    pub fn set_language_servers(&self, servers: LanguageServers) {
        *self.servers.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(servers);
    }

    fn language_servers(&self) -> Option<LanguageServers> {
        self.servers.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn declare_servers(&self, servers: &[ServerDeclaration]) -> bool {
        let Some(registry) = self.language_servers() else { return false };
        let fresh = servers.iter().all(|server| registry.declaration(&server.id).is_none());
        if fresh {
            registry.declare(servers.iter().cloned());
        }
        fresh
    }

    fn install_servers(&self, plugin: &CatalogPlugin) {
        let Some(registry) = self.language_servers() else { return };
        let mut missing_tools = BTreeSet::new();
        for server in &plugin.language_servers {
            if registry.declaration(&server.id).is_none() || registry.source(&server.id) != ServerSource::Missing {
                continue;
            }
            match registry.install_check(&server.id) {
                Ok(()) => {
                    if registry.install(&server.id).is_ok() {
                        self.watch_install(registry.clone(), plugin.name.clone(), server.clone());
                    }
                }
                Err(NotInstallable::ToolMissing(kind)) => {
                    if missing_tools.insert(format!("{kind:?}")) {
                        let message = t!(
                            "ide-plugin-server-needs-tool",
                            name = plugin.name.as_str(),
                            server = server.name.as_str(),
                            tool = tool_name(kind),
                            url = kind.download().unwrap_or_default()
                        );
                        self.state().messages.push(message);
                    }
                }
                Err(NotInstallable::Manual(_)) => {}
            }
        }
        let targets: BTreeSet<&str> =
            plugin.server_plugins.iter().map(|contributed| contributed.server.as_str()).collect();
        for target in targets {
            let Some(declaration) = registry.declaration(target) else { continue };
            let missing = registry.missing_plugins(target);
            if missing.is_empty() {
                continue;
            }
            match registry.plugins_installable(target) {
                Ok(()) => {
                    if registry.install_plugins(target).is_ok() {
                        let names =
                            missing.into_iter().map(|contributed| contributed.name).collect::<Vec<_>>().join(", ");
                        self.watch_plugins_install(registry.clone(), plugin.name.clone(), declaration, names);
                    }
                }
                Err(NotInstallable::ToolMissing(kind)) => {
                    if missing_tools.insert(format!("{kind:?}")) {
                        let message = t!(
                            "ide-plugin-server-needs-tool",
                            name = plugin.name.as_str(),
                            server = declaration.name.as_str(),
                            tool = tool_name(kind),
                            url = kind.download().unwrap_or_default()
                        );
                        self.state().messages.push(message);
                    }
                }
                Err(NotInstallable::Manual(_)) => {}
            }
        }
        self.repaint();
    }

    fn watch_plugins_install(
        &self,
        registry: LanguageServers,
        plugin: String,
        server: ServerDeclaration,
        names: String,
    ) {
        #[cfg(not(target_arch = "wasm32"))]
        self.spawn(move |plugins| {
            let started = std::time::Instant::now();
            let state = loop {
                match registry.install_state(&server.id) {
                    Some(Install::Running(_)) if started.elapsed() < std::time::Duration::from_secs(3600) => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    other => break other,
                }
            };
            let message = match state {
                Some(Install::Done) => t!(
                    "ide-plugin-server-plugins-installed",
                    name = plugin.as_str(),
                    server = server.name.as_str(),
                    plugins = names.as_str()
                ),
                Some(Install::Failed(error)) => t!(
                    "ide-plugin-server-failed",
                    name = plugin.as_str(),
                    server = names.as_str(),
                    error = error.lines().last().unwrap_or_default()
                ),
                _ => return,
            };
            plugins.state().messages.push(message);
            plugins.repaint();
        });
        #[cfg(target_arch = "wasm32")]
        let _ = (registry, plugin, server, names);
    }

    fn watch_install(&self, registry: LanguageServers, plugin: String, server: ServerDeclaration) {
        #[cfg(not(target_arch = "wasm32"))]
        self.spawn(move |plugins| {
            let started = std::time::Instant::now();
            let state = loop {
                match registry.install_state(&server.id) {
                    Some(Install::Running(_)) if started.elapsed() < std::time::Duration::from_secs(3600) => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    other => break other,
                }
            };
            let message = match state {
                Some(Install::Done) => {
                    t!("ide-plugin-server-installed", name = plugin.as_str(), server = server.name.as_str())
                }
                Some(Install::Failed(error)) => t!(
                    "ide-plugin-server-failed",
                    name = plugin.as_str(),
                    server = server.name.as_str(),
                    error = error.lines().last().unwrap_or_default()
                ),
                _ => return,
            };
            plugins.state().messages.push(message);
            plugins.repaint();
        });
        #[cfg(target_arch = "wasm32")]
        let _ = (registry, plugin, server);
    }

    fn remove_servers(&self, plugin: &InstalledPlugin) {
        let Some(registry) = self.language_servers() else { return };
        let others: BTreeSet<String> = self
            .entries()
            .into_iter()
            .filter(|entry| entry.plugin.id != plugin.id)
            .flat_map(|entry| entry.plugin.language_servers.into_iter().map(|server| server.id))
            .collect();
        for server in plugin.language_servers.iter().filter(|server| !others.contains(&server.id)) {
            if let Some(prefix) = registry.own_prefix(server)
                && prefix.is_dir()
            {
                let _ = std::fs::remove_dir_all(&prefix);
                registry.forget(&server.id);
            }
        }
    }

    pub fn attach(&self, ctx: &egui::Context) {
        if let Ok(mut repaint) = self.repaint.lock()
            && repaint.is_none()
        {
            *repaint = Some(ctx.clone());
        }
    }

    fn repaint(&self) {
        if let Ok(repaint) = self.repaint.lock()
            && let Some(ctx) = repaint.as_ref()
        {
            ctx.request_repaint();
        }
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn spawn(&self, work: impl FnOnce(Arc<Plugins>) + Send + 'static) {
        let Some(plugins) = self.this.upgrade() else { return };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || work(plugins));
        #[cfg(target_arch = "wasm32")]
        work(plugins);
    }

    pub fn highlighter(&self, builtin: &[Arc<dyn Highlighter>], name: &str) -> Option<Arc<dyn Highlighter>> {
        let name = zen_vfs::file_types::known_name(name, None)?;
        let name = name.as_str();
        builtin
            .iter()
            .find(|highlighter| highlighter.handles(name))
            .cloned()
            .or_else(|| {
                self.highlighters()
                    .into_iter()
                    .filter(|highlighter| highlighter.handles(name))
                    .max_by_key(|highlighter| highlighter.specificity(name))
            })
            .or_else(|| platform_highlighter(name))
    }

    pub fn highlighters(&self) -> Vec<Arc<dyn Highlighter>> {
        self.state().highlighters.iter().map(|(_, plugin)| Arc::clone(plugin) as Arc<dyn Highlighter>).collect()
    }

    pub fn features(&self) -> Vec<Arc<dyn TextFeatures>> {
        self.state().highlighters.iter().map(|(_, plugin)| Arc::clone(plugin) as Arc<dyn TextFeatures>).collect()
    }

    pub(crate) fn take_messages(&self) -> Vec<String> {
        std::mem::take(&mut self.state().messages)
    }

    pub(crate) fn offer(&self, file_name: &str) -> Option<Offer> {
        self.source.as_ref()?;
        let extension = file_name
            .rsplit_once('.')
            .map_or_else(|| file_name.to_owned(), |(_, extension)| extension.to_owned())
            .to_ascii_lowercase();
        let installed = {
            let state = self.state();
            if let Some(job) = state.queue.front() {
                return Some(Offer::Installing { name: job.plugin.name.clone() });
            }
            if state.ignored.contains(&extension) {
                return None;
            }
            state.installed.clone()
        };
        match self.catalog() {
            CatalogView::Ready(catalog) => catalog
                .for_file(file_name)
                .filter(|plugin| plugin.latest().is_some() && !installed.contains(&plugin.id))
                .map(|plugin| Offer::Install { plugin: Box::new(plugin.clone()), extension }),
            CatalogView::Loading | CatalogView::Unavailable => None,
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.state().generation
    }

    pub(crate) fn project_offers(&self, root: &std::path::Path) -> Vec<CatalogPlugin> {
        let Some(catalog) = self.fetched() else { return Vec::new() };
        let installed: BTreeSet<String> = self.entries().into_iter().map(|entry| entry.plugin.id).collect();
        let ignored = self.state().ignored_plugins.clone();
        let offered: Vec<&CatalogPlugin> = catalog
            .for_project(root)
            .into_iter()
            .filter(|plugin| {
                plugin.latest().is_some() && !installed.contains(&plugin.id) && !ignored.contains(&plugin.id)
            })
            .collect();
        let required: BTreeSet<String> = offered
            .iter()
            .flat_map(|plugin| {
                catalog
                    .with_requirements(&plugin.id)
                    .into_iter()
                    .filter(|other| other.id != plugin.id)
                    .map(|other| other.id.clone())
            })
            .collect();
        offered.into_iter().filter(|plugin| !required.contains(&plugin.id)).cloned().collect()
    }

    pub(crate) fn missing_requirements(&self, plugin: &CatalogPlugin) -> Vec<String> {
        self.requirements(plugin).into_iter().map(|job| job.plugin.name).collect()
    }

    pub(crate) fn ignore_for_projects(&self, id: &str) {
        let Some(config) = &self.config else { return };
        let mut state = self.state();
        state.ignored_plugins.insert(id.to_owned());
        write_set(&ignored_plugins_file(config), &state.ignored_plugins);
        state.generation += 1;
    }

    pub fn project_generators(
        &self,
        environment: &zen_project::generators::GeneratorEnvironment,
        client: &str,
    ) -> Vec<Arc<dyn zen_project::generators::ProjectGenerator>> {
        let host = zen_plugins::generator::GeneratorHost {
            cache: self
                .config
                .as_ref()
                .and_then(|config| config.folder.parent())
                .map(|data| data.join("generator-cache")),
            environment: environment.clone(),
            archive: Arc::clone(&self.generator_archive),
            ..zen_plugins::generator::GeneratorHost::new(Arc::new(zen_plugins::generator::WebFetcher), client)
        };
        let modules = self.state().highlighters.clone();
        let mut generators: Vec<Arc<dyn zen_project::generators::ProjectGenerator>> = Vec::new();
        for plugin in self.installed() {
            let module = modules.iter().find(|(id, module)| *id == plugin.id && module.has_project());
            let Some((_, module)) = module else { continue };
            for declaration in plugin.project_generators {
                let tools =
                    zen_workbench::new_project::BuildToolRunner::new(zen_workbench::build_tools::BuildEnv::global());
                generators.push(Arc::new(
                    zen_plugins::generator::PluginGenerator::new(
                        &plugin.id,
                        declaration,
                        Arc::clone(module),
                        host.clone(),
                    )
                    .with_tools(Arc::new(tools)),
                ));
            }
        }
        generators
    }

    pub fn module_builds(&self) -> Vec<Arc<dyn zen_project::generators::ModuleBuild>> {
        let modules = self.state().highlighters.clone();
        let mut builds: Vec<Arc<dyn zen_project::generators::ModuleBuild>> = Vec::new();
        for plugin in self.installed() {
            let module = modules.iter().find(|(id, module)| *id == plugin.id && module.has_project());
            let (Some((_, module)), Some(build)) = (module, plugin.project.build) else { continue };
            if !build.module_files.is_empty() {
                builds.push(Arc::new(zen_plugins::generator::PluginModuleBuild::new(
                    &plugin.id,
                    build,
                    Arc::clone(module),
                )));
            }
        }
        builds
    }

    pub fn project_support(&self) -> Vec<zen_workbench::plugin_projects::PluginProject> {
        let loaded: BTreeSet<String> = self.state().highlighters.iter().map(|(id, _)| id.clone()).collect();
        self.installed()
            .into_iter()
            .filter(|plugin| loaded.contains(&plugin.id) && !plugin.project.is_empty())
            .map(|plugin| zen_workbench::plugin_projects::PluginProject {
                id: plugin.id,
                name: plugin.name,
                support: plugin.project,
            })
            .collect()
    }

    pub fn catalog(&self) -> CatalogView {
        if self.source.is_none() {
            return CatalogView::Unavailable;
        }
        let mut state = self.state();
        match &state.catalog {
            CatalogState::Ready(catalog) | CatalogState::Fetching(Some(catalog)) => {
                CatalogView::Ready(Arc::clone(catalog))
            }
            CatalogState::Fetching(None) => CatalogView::Loading,
            CatalogState::Failed => CatalogView::Unavailable,
            CatalogState::NotFetched => {
                state.catalog = CatalogState::Fetching(None);
                drop(state);
                self.fetch();
                CatalogView::Loading
            }
        }
    }

    fn fetch(&self) {
        let Some(source) = self.source.clone() else { return };
        self.spawn(move |plugins| {
            let result = source.catalog();
            plugins.catalog_fetched(result);
        });
    }

    fn catalog_fetched(&self, result: Result<Catalog, String>) {
        let update = {
            let mut state = self.state();
            let previous = match std::mem::replace(&mut state.catalog, CatalogState::Failed) {
                CatalogState::Fetching(previous) => previous,
                _ => None,
            };
            match (result, previous) {
                (Ok(catalog), _) => {
                    state.catalog = CatalogState::Ready(Arc::new(catalog));
                    state.check_failed = None;
                }
                (Err(error), Some(previous)) => {
                    state.catalog = CatalogState::Ready(previous);
                    state.check_failed = Some(error);
                }
                (Err(_), None) => state.catalog = CatalogState::Failed,
            }
            state.generation += 1;
            std::mem::take(&mut state.update_when_fetched)
        };
        if update {
            self.enqueue(self.start_updates());
        }
        self.repaint();
    }

    pub fn refresh_catalog(&self) {
        if self.source.is_none() {
            return;
        }
        {
            let mut state = self.state();
            let previous = match std::mem::replace(&mut state.catalog, CatalogState::NotFetched) {
                CatalogState::Fetching(previous) => {
                    state.catalog = CatalogState::Fetching(previous);
                    return;
                }
                CatalogState::Ready(catalog) => Some(catalog),
                CatalogState::NotFetched | CatalogState::Failed => None,
            };
            state.catalog = CatalogState::Fetching(previous);
            state.check_failed = None;
        }
        self.fetch();
    }

    fn status(&self) -> CatalogStatus {
        if self.source.is_none() {
            return CatalogStatus::Unavailable;
        }
        match self.state().catalog {
            CatalogState::NotFetched => CatalogStatus::NotFetched,
            CatalogState::Fetching(_) => CatalogStatus::Loading,
            CatalogState::Ready(_) => CatalogStatus::Ready,
            CatalogState::Failed => CatalogStatus::Unavailable,
        }
    }

    fn fetched(&self) -> Option<Arc<Catalog>> {
        match &self.state().catalog {
            CatalogState::Ready(catalog) | CatalogState::Fetching(Some(catalog)) => Some(Arc::clone(catalog)),
            _ => None,
        }
    }

    pub fn update_in_background(&self) {
        if self.source.is_none() || self.entries().is_empty() {
            return;
        }
        self.state().update_when_fetched = true;
        match self.catalog() {
            CatalogView::Ready(_) => {
                self.state().update_when_fetched = false;
                self.enqueue(self.start_updates());
            }
            CatalogView::Loading | CatalogView::Unavailable => {}
        }
    }

    pub fn installed(&self) -> Vec<InstalledPlugin> {
        self.entries().into_iter().map(|entry| entry.plugin).collect()
    }

    pub fn entries(&self) -> Vec<PluginEntry> {
        self.config.as_ref().map(|config| entries(config, &Switches::read(config))).unwrap_or_default()
    }

    pub fn catalog_entries(&self, catalog: &Catalog) -> Vec<CatalogEntry> {
        let entries = self.entries();
        catalog
            .plugins
            .iter()
            .map(|plugin| {
                let entry = entries.iter().find(|entry| entry.plugin.id == plugin.id);
                CatalogEntry {
                    plugin: plugin.clone(),
                    bundled: entry.is_some_and(|entry| entry.bundled),
                    installed: entry.map(|entry| entry.plugin.version.clone()),
                }
            })
            .collect()
    }

    pub fn installing(&self) -> Vec<String> {
        self.state().queue.iter().map(|job| job.plugin.id.clone()).collect()
    }

    pub fn installing_name(&self) -> Option<String> {
        self.state().queue.front().map(|job| job.plugin.name.clone())
    }

    pub fn restart_required(&self) -> Vec<String> {
        self.state().restart.iter().cloned().collect()
    }

    fn needs_restart(&self, plugin: &InstalledPlugin) {
        if !plugin.language_servers.is_empty() {
            self.state().restart.insert(plugin.name.clone());
        }
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let config = self.config.as_ref().ok_or_else(|| t!("plugins-error-none"))?;
        let mut switches = Switches::read(config);
        let entry = entries(config, &switches)
            .into_iter()
            .find(|entry| entry.plugin.id == id)
            .ok_or_else(|| t!("plugins-error-not-installed", id = id))?;
        if entry.enabled == enabled {
            return Ok(());
        }
        switches.set(config, id, enabled);
        switches.write(config);
        self.state().highlighters.retain(|(installed, _)| installed != id);
        if enabled {
            self.load_in_background(config, entry.plugin.clone());
        }
        self.sync_file_types();
        if let Some(registry) = self.language_servers() {
            let contributed = if enabled { entry.plugin.server_plugins.clone() } else { Vec::new() };
            registry.contribute(id, contributed);
        }
        self.sync_drivers();
        self.needs_restart(&entry.plugin);
        Ok(())
    }

    fn load_in_background(&self, config: &PluginConfig, plugin: InstalledPlugin) {
        let config = config.clone();
        self.state().loading += 1;
        self.spawn(move |this| {
            let loaded = load(&config, &plugin);
            let on = Switches::read(&config).on(&config, &plugin.id);
            {
                let mut state = this.state();
                state.loading -= 1;
                match loaded {
                    Ok(highlighter) if on => {
                        state.highlighters.retain(|(id, _)| *id != plugin.id);
                        state.highlighters.push((plugin.id.clone(), Arc::new(highlighter)));
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let message = t!("ide-plugin-failed", name = plugin.name.as_str(), error = error.to_string());
                        state.messages.push(message);
                    }
                }
            }
            this.sync_file_types();
            this.repaint();
        });
    }

    fn note_installed(&self) {
        let installed = self.entries().into_iter().map(|entry| entry.plugin.id).collect();
        self.state().installed = installed;
    }

    pub fn uninstall(&self, id: &str) -> Result<(), String> {
        let config = self.config.as_ref().ok_or_else(|| t!("plugins-error-none"))?;
        let user = Installed::new(&config.folder);
        let bundled = config
            .bundled
            .as_ref()
            .and_then(|folder| Installed::new(folder).list().into_iter().find(|plugin| plugin.id == id));
        let Some(own) = user.list().into_iter().find(|plugin| plugin.id == id) else {
            return Err(if bundled.is_some() {
                t!("settings-plugins-bundled-cannot-uninstall")
            } else {
                t!("plugins-error-not-installed", id = id)
            });
        };
        let taken_out = user.take_out(id).map_err(|error| error.to_string())?;
        self.spawn(move |_| {
            let _ = std::fs::remove_dir_all(taken_out);
        });
        self.note_installed();
        if bundled.is_none() {
            self.remove_servers(&own);
            if let Some(registry) = self.language_servers() {
                registry.withdraw(id);
            }
        }
        let enabled = Switches::read(config).on(config, id);
        {
            let mut state = self.state();
            state.highlighters.retain(|(installed, _)| installed != id);
            if let (Some(bundled), true) = (&bundled, enabled)
                && let Ok(highlighter) = load(config, bundled)
            {
                state.highlighters.push((id.to_owned(), Arc::new(highlighter)));
            }
        }
        self.sync_file_types();
        self.sync_drivers();
        self.needs_restart(&own);
        Ok(())
    }

    pub(crate) fn ignore(&self, extension: &str) {
        let Some(config) = &self.config else { return };
        let mut state = self.state();
        state.ignored.insert(extension.to_owned());
        if let Some(parent) = ignored_file(config).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = serde_json::to_vec_pretty(&state.ignored).unwrap_or_default();
        let _ = zen_vfs::write_atomic(&ignored_file(config), &json);
    }

    pub fn list_plugins(&self) -> serde_json::Value {
        let catalog = match self.catalog() {
            CatalogView::Ready(catalog) => serde_json::to_value(self.catalog_entries(&catalog)).unwrap_or_default(),
            CatalogView::Loading => serde_json::json!("loading; call again"),
            CatalogView::Unavailable => serde_json::json!("unavailable"),
        };
        serde_json::json!({
            "installed": self.entries(),
            "installing": self.installing_name(),
            "installingIds": self.installing(),
            "catalog": catalog,
        })
    }

    pub fn agent_may_install(&self, id: Option<&str>) -> Result<(), String> {
        let mut bringing: Vec<String> = match id {
            Some(id) => self
                .fetched()
                .map(|catalog| {
                    catalog
                        .with_requirements(id)
                        .into_iter()
                        .filter(|plugin| !plugin.database_drivers.is_empty() || !plugin.terminal_agents.is_empty())
                        .map(|plugin| plugin.name.clone())
                        .collect()
                })
                .unwrap_or_default(),
            None => self
                .updates()
                .into_iter()
                .filter(|job| !job.plugin.database_drivers.is_empty() || !job.plugin.terminal_agents.is_empty())
                .map(|job| job.plugin.name)
                .collect(),
        };
        bringing.sort();
        bringing.dedup();
        if bringing.is_empty() {
            return Ok(());
        }
        Err(t!("plugins-error-agent-programs", names = bringing.join(", ")))
    }

    pub fn agent_may_enable(&self, id: &str) -> Result<(), String> {
        let Some(entry) = self.entries().into_iter().find(|entry| entry.plugin.id == id) else { return Ok(()) };
        let brings = !entry.plugin.database_drivers.is_empty() || !entry.plugin.terminal_agents.is_empty();
        if entry.enabled || !brings {
            return Ok(());
        }
        Err(t!("plugins-error-agent-programs", names = entry.plugin.name))
    }

    pub fn install_plugin_by_id(&self, id: &str) -> Result<serde_json::Value, String> {
        self.agent_may_install(Some(id))?;
        let CatalogView::Ready(catalog) = self.catalog() else {
            return Err("the plug-in catalog is loading or unavailable; call again in a moment".to_owned());
        };
        let plugin = catalog
            .plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| format!("no plug-in `{id}` in the catalog"))?;
        let queued = self.install_plugin(plugin.clone());
        Ok(serde_json::json!({
            "installing": plugin.name,
            "queued": queued.queued,
            "installingIds": queued.installing,
        }))
    }

    pub fn install_plugin(&self, plugin: CatalogPlugin) -> Queued {
        let mut jobs = self.requirements(&plugin);
        jobs.extend(plugin.latest().cloned().map(|version| Job { plugin, version, servers: true }));
        self.enqueue(jobs)
    }

    fn requirements(&self, plugin: &CatalogPlugin) -> Vec<Job> {
        if plugin.requires.is_empty() {
            return Vec::new();
        }
        let Some(catalog) = self.fetched() else { return Vec::new() };
        let installed: BTreeSet<String> = self.entries().into_iter().map(|entry| entry.plugin.id).collect();
        catalog
            .with_requirements(&plugin.id)
            .into_iter()
            .filter(|required| required.id != plugin.id && !installed.contains(&required.id))
            .filter_map(|required| {
                Some(Job { plugin: required.clone(), version: required.latest()?.clone(), servers: true })
            })
            .collect()
    }

    fn start_updates(&self) -> Vec<Job> {
        self.updates()
            .into_iter()
            .filter(|job| job.plugin.database_drivers.is_empty())
            .map(|job| Job { servers: false, ..job })
            .collect()
    }

    fn updates(&self) -> Vec<Job> {
        let Some(catalog) = self.fetched() else { return Vec::new() };
        self.entries()
            .into_iter()
            .filter_map(|entry| {
                let plugin = catalog.plugins.iter().find(|plugin| plugin.id == entry.plugin.id)?;
                let version =
                    plugin.latest().filter(|latest| zen_plugins::is_newer(&latest.version, &entry.plugin.version))?;
                Some(Job { plugin: plugin.clone(), version: version.clone(), servers: true })
            })
            .collect()
    }

    fn enqueue(&self, jobs: Vec<Job>) -> Queued {
        let (queued, installing, start) = {
            let mut state = self.state();
            let mut queued = Vec::new();
            for job in jobs {
                if !state.queue.iter().any(|waiting| waiting.plugin.id == job.plugin.id) {
                    queued.push(job.plugin.id.clone());
                    state.queue.push_back(job);
                }
            }
            let start = !state.working && !state.queue.is_empty();
            state.working |= start;
            (queued, state.queue.iter().map(|job| job.plugin.id.clone()).collect(), start)
        };
        if start {
            self.spawn(|plugins| plugins.work());
        }
        self.repaint();
        Queued { queued, installing }
    }

    fn work(&self) {
        loop {
            let Some(job) = ({
                let mut state = self.state();
                let job = state.queue.front().cloned();
                state.working = job.is_some();
                job
            }) else {
                return;
            };
            let message = match self.install_now(&job) {
                Ok(true) => {
                    t!("ide-plugin-updated", name = job.plugin.name.as_str(), version = job.version.version.as_str())
                }
                Ok(false) => t!("ide-plugin-installed", name = job.plugin.name.as_str()),
                Err(error) => t!("ide-plugin-failed", name = job.plugin.name.as_str(), error = error),
            };
            {
                let mut state = self.state();
                state.queue.pop_front();
                state.messages.push(message);
            }
            self.repaint();
        }
    }

    fn install_now(&self, job: &Job) -> Result<bool, String> {
        let config = self.config.as_ref().ok_or_else(|| t!("plugins-error-none"))?;
        let source = self.source.as_ref().ok_or_else(|| t!("plugins-error-no-catalog"))?;
        let previous = self.entries().into_iter().find(|entry| entry.plugin.id == job.plugin.id);
        let wasm = source.module(&job.plugin, &job.version)?;
        let highlighter = Installed::new(&config.folder)
            .install(&job.plugin, &job.version, &wasm)
            .map_err(|error| error.to_string())?;
        self.note_installed();
        let mut switches = Switches::read(config);
        if previous.is_none() && !switches.on(config, &job.plugin.id) {
            switches.set(config, &job.plugin.id, true);
            switches.write(config);
        }
        if switches.on(config, &job.plugin.id) {
            let mut state = self.state();
            state.highlighters.retain(|(id, _)| *id != job.plugin.id);
            state.highlighters.push((job.plugin.id.clone(), Arc::new(highlighter)));
        }
        self.sync_file_types();
        let changed_servers = !job.plugin.language_servers.is_empty()
            || previous.as_ref().is_some_and(|entry| !entry.plugin.language_servers.is_empty());
        let declared_now = previous.is_none() && self.declare_servers(&job.plugin.language_servers);
        if changed_servers && !declared_now {
            self.state().restart.insert(job.plugin.name.clone());
        }
        if switches.on(config, &job.plugin.id)
            && let Some(registry) = self.language_servers()
        {
            registry.contribute(&job.plugin.id, job.plugin.server_plugins.iter().cloned());
        }
        if job.servers && switches.on(config, &job.plugin.id) {
            self.install_servers(&job.plugin);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if job.servers {
            let messages = self.install_drivers(&job.plugin);
            self.state().messages.extend(messages);
        }
        let problems = self.sync_drivers();
        self.state().messages.extend(problems);
        Ok(previous.is_some())
    }

    fn summaries(&self, catalog: Option<&Catalog>) -> Vec<PluginSummary> {
        let installing = self.installing();
        let entries = self.entries();
        let mut summaries: Vec<PluginSummary> = entries
            .iter()
            .map(|entry| {
                let listed =
                    catalog.and_then(|catalog| catalog.plugins.iter().find(|plugin| plugin.id == entry.plugin.id));
                summary(Some(entry), listed, &installing)
            })
            .collect();
        for plugin in catalog.map(|catalog| catalog.plugins.as_slice()).unwrap_or_default() {
            if plugin.latest().is_some() && !entries.iter().any(|entry| entry.plugin.id == plugin.id) {
                summaries.push(summary(None, Some(plugin), &installing));
            }
        }
        sort(&mut summaries);
        summaries
    }

    fn list(&self, status: CatalogStatus, plugins: Vec<PluginSummary>, query: &str) -> PluginList {
        PluginList {
            catalog: status,
            plugins: plugins.into_iter().filter(|plugin| plugin.matches(query)).collect(),
            installing: self.installing(),
            restart_required: self.restart_required(),
            check_failed: self.state().check_failed.clone(),
        }
    }

    fn ready_catalog(&self) -> Result<Arc<Catalog>, String> {
        match self.catalog() {
            CatalogView::Ready(catalog) => Ok(catalog),
            CatalogView::Loading => Err(t!("plugins-error-loading")),
            CatalogView::Unavailable => Err(t!("settings-plugins-unavailable")),
        }
    }
}

fn summary(entry: Option<&PluginEntry>, listed: Option<&CatalogPlugin>, installing: &[String]) -> PluginSummary {
    let installed = entry.map(|entry| &entry.plugin);
    let latest = listed.and_then(CatalogPlugin::latest).map(|version| version.version.clone());
    let update = match (installed, &latest) {
        (Some(installed), Some(latest)) if zen_plugins::is_newer(latest, &installed.version) => Some(latest.clone()),
        _ => None,
    };
    let id = installed
        .map_or_else(|| listed.map(|plugin| plugin.id.clone()).unwrap_or_default(), |plugin| plugin.id.clone());
    let pick = |own: Option<&String>, catalog: Option<&String>| {
        catalog.filter(|text| !text.is_empty()).or(own).cloned().unwrap_or_default()
    };
    PluginSummary {
        installing: installing.contains(&id),
        name: pick(installed.map(|plugin| &plugin.name), listed.map(|plugin| &plugin.name)),
        description: pick(installed.map(|plugin| &plugin.description), listed.map(|plugin| &plugin.description)),
        installed: installed.map(|plugin| plugin.version.clone()),
        latest,
        update,
        bundled: entry.is_some_and(|entry| entry.bundled),
        enabled: entry.is_some_and(|entry| entry.enabled),
        extensions: installed.map_or_else(
            || listed.map(|plugin| plugin.extensions.clone()).unwrap_or_default(),
            |plugin| plugin.extensions.clone(),
        ),
        names: installed.map_or_else(
            || listed.map(|plugin| plugin.names.clone()).unwrap_or_default(),
            |plugin| plugin.names.clone(),
        ),
        language_servers: installed.map_or_else(
            || listed.is_some_and(|plugin| !plugin.language_servers.is_empty()),
            |plugin| !plugin.language_servers.is_empty(),
        ),
        runs: installed
            .map_or_else(|| listed.map(|plugin| plugin.runs.clone()).unwrap_or_default(), |plugin| plugin.runs.clone()),
        person_only: installed
            .map_or_else(
                || listed.map(|plugin| plugin.person_only.clone()).unwrap_or_default(),
                |plugin| plugin.person_only.clone(),
            )
            .iter()
            .map(|rule| format!("{} {}", rule.program, rule.subcommands.join(", ")))
            .collect(),
        tool_windows: installed
            .map_or_else(
                || listed.map(|plugin| plugin.tool_windows.clone()).unwrap_or_default(),
                |plugin| plugin.tool_windows.clone(),
            )
            .iter()
            .map(window_title)
            .collect(),
        id,
    }
}

fn window_title(window: &zen_syntax::ToolWindowDeclaration) -> String {
    use zen_image::icon::IconProblem;
    let title = zen_syntax::localized(&window.title, &zen_i18n::language());
    if !window.icon.trim_start().starts_with('<') {
        return title;
    }
    let why = match zen_image::icon::problem(&window.icon) {
        None => return title,
        Some(IconProblem::TooBig) => t!("settings-plugins-icon-too-big"),
        Some(IconProblem::TooDeep) => t!("settings-plugins-icon-too-deep"),
        Some(IconProblem::NoViewBox) => t!("settings-plugins-icon-no-view-box"),
        Some(IconProblem::Reference) => t!("settings-plugins-icon-reference"),
        Some(IconProblem::Element | IconProblem::Attribute) => t!("settings-plugins-icon-not-plain"),
        Some(IconProblem::NotSvg | IconProblem::Unreadable) => t!("settings-plugins-icon-unreadable"),
    };
    t!("settings-plugins-icon-unused", window = title, why = why)
}

impl PluginManager for Plugins {
    fn catalog(&self, query: &str) -> PluginList {
        let (status, catalog) = match Plugins::catalog(self) {
            CatalogView::Ready(catalog) => (self.status(), Some(catalog)),
            CatalogView::Loading => (CatalogStatus::Loading, None),
            CatalogView::Unavailable => (CatalogStatus::Unavailable, None),
        };
        let listed: Vec<PluginSummary> =
            self.summaries(catalog.as_deref()).into_iter().filter(|plugin| plugin.latest.is_some()).collect();
        self.list(status, listed, query)
    }

    fn installed(&self, query: &str) -> PluginList {
        let status = self.status();
        let installed: Vec<PluginSummary> =
            self.summaries(self.fetched().as_deref()).into_iter().filter(|plugin| plugin.installed.is_some()).collect();
        self.list(status, installed, query)
    }

    fn details(&self, id: &str) -> Result<PluginDetails, String> {
        let catalog = self.fetched();
        let plugin = self
            .summaries(catalog.as_deref())
            .into_iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| t!("plugins-error-unknown", id = id))?;
        let listed = catalog.as_ref().and_then(|catalog| catalog.plugins.iter().find(|plugin| plugin.id == id));
        let own = self.entries().into_iter().find(|entry| entry.plugin.id == id).map(|entry| entry.plugin);
        let mut notes = Vec::new();
        if let Some(newest) = listed.and_then(CatalogPlugin::latest) {
            notes.push(VersionNotes {
                version: newest.version.clone(),
                date: newest.date.clone(),
                notes: clip_notes(&newest.notes),
            });
        }
        if let Some(own) = &own
            && !notes.iter().any(|shown| shown.version == own.version)
        {
            let published = listed.and_then(|plugin| plugin.version(&own.version));
            notes.push(VersionNotes {
                version: own.version.clone(),
                date: published.map(|version| version.date.clone()).unwrap_or_default(),
                notes: clip_notes(
                    published
                        .map(|version| version.notes.as_str())
                        .filter(|notes| !notes.is_empty())
                        .unwrap_or(&own.notes),
                ),
            });
        }
        Ok(PluginDetails { plugin, notes })
    }

    fn install(&self, id: &str) -> Result<Queued, String> {
        let catalog = self.ready_catalog()?;
        let plugin = catalog
            .plugins
            .iter()
            .find(|plugin| plugin.id == id)
            .ok_or_else(|| t!("plugins-error-not-in-catalog", id = id))?;
        let version = plugin.latest().ok_or_else(|| t!("plugins-error-not-in-catalog", id = id))?;
        if let Some(entry) = self.entries().into_iter().find(|entry| entry.plugin.id == id)
            && !zen_plugins::is_newer(&version.version, &entry.plugin.version)
        {
            return Err(t!(
                "plugins-error-up-to-date",
                name = entry.plugin.name.as_str(),
                version = entry.plugin.version.as_str()
            ));
        }
        let mut jobs = self.requirements(plugin);
        jobs.push(Job { plugin: plugin.clone(), version: version.clone(), servers: true });
        Ok(self.enqueue(jobs))
    }

    fn update(&self, id: &str) -> Result<Queued, String> {
        let entry = self
            .entries()
            .into_iter()
            .find(|entry| entry.plugin.id == id)
            .ok_or_else(|| t!("plugins-error-not-installed", id = id))?;
        self.ready_catalog()?;
        let job = self.updates().into_iter().find(|job| job.plugin.id == id).ok_or_else(|| {
            t!("plugins-error-up-to-date", name = entry.plugin.name.as_str(), version = entry.plugin.version.as_str())
        })?;
        Ok(self.enqueue(vec![job]))
    }

    fn update_all(&self) -> Result<Queued, String> {
        self.ready_catalog()?;
        Ok(self.enqueue(self.updates()))
    }

    fn refresh(&self) -> PluginList {
        self.refresh_catalog();
        PluginManager::installed(self, "")
    }

    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        Plugins::set_enabled(self, id, enabled)
    }

    fn uninstall(&self, id: &str) -> Result<(), String> {
        Plugins::uninstall(self, id)
    }

    fn agent_may_install(&self, id: Option<&str>) -> Result<(), String> {
        Plugins::agent_may_install(self, id)
    }

    fn agent_may_enable(&self, id: &str) -> Result<(), String> {
        Plugins::agent_may_enable(self, id)
    }
}

impl zen_modules::AreaPlugins for Plugins {
    fn plugin(&self, id: &str) -> Option<zen_modules::AreaPlugin> {
        let entry = self.entries().into_iter().find(|entry| entry.plugin.id == id)?;
        let restart_required = self.state().restart.contains(&entry.plugin.name);
        Some(zen_modules::AreaPlugin {
            name: entry.plugin.name,
            description: entry.plugin.description,
            enabled: entry.enabled,
            restart_required,
        })
    }

    fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        Plugins::set_enabled(self, id, enabled)
    }
}

fn entries(config: &PluginConfig, switches: &Switches) -> Vec<PluginEntry> {
    let bundled = config.bundled.as_ref().map(|folder| Installed::new(folder).list()).unwrap_or_default();
    let user = Installed::new(&config.folder).list();
    let mut entries: Vec<PluginEntry> = bundled
        .iter()
        .map(|plugin| {
            let chosen = user.iter().find(|own| own.id == plugin.id).cloned().unwrap_or_else(|| plugin.clone());
            PluginEntry { enabled: switches.on(config, &plugin.id), bundled: true, plugin: chosen }
        })
        .collect();
    for plugin in user.into_iter().filter(|own| !bundled.iter().any(|plugin| plugin.id == own.id)) {
        entries.push(PluginEntry { enabled: switches.on(config, &plugin.id), bundled: false, plugin });
    }
    entries.sort_by(|a, b| a.plugin.id.cmp(&b.plugin.id));
    entries
}

fn file_type(plugin: &InstalledPlugin) -> zen_vfs::file_types::FileType {
    let extensions = plugin.extensions.iter().map(|extension| format!("*.{extension}"));
    zen_vfs::file_types::FileType {
        id: plugin.id.clone(),
        name: plugin.name.clone(),
        localized: false,
        patterns: extensions.chain(plugin.names.iter().cloned()).collect(),
        hashbangs: plugin.hashbangs.clone(),
        contents: plugin.contents.iter().map(zen_plugins::ContentDeclaration::marker).collect(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn driver_specs(config: &PluginConfig, entries: &[PluginEntry]) -> (Vec<zen_db::external::ExternalSpec>, Vec<String>) {
    let built_in: Vec<String> = zen_db::drivers().iter().map(|driver| driver.id().to_owned()).collect();
    let mut specs: Vec<zen_db::external::ExternalSpec> = Vec::new();
    let mut problems = Vec::new();
    for entry in entries.iter().filter(|entry| entry.enabled && !entry.plugin.database_drivers.is_empty()) {
        let plugin = &entry.plugin;
        let declarations = match zen_db::external::DriverDeclaration::parse_list(&serde_json::Value::Array(
            plugin.database_drivers.clone(),
        )) {
            Ok(declarations) => declarations,
            Err(error) => {
                problems.push(t!("ide-plugin-driver-invalid", name = plugin.name.as_str(), error = error));
                continue;
            }
        };
        let user = Installed::new(&config.folder);
        let folder = if user.list().iter().any(|own| own.id == plugin.id) {
            user
        } else {
            config.bundled.as_ref().map_or(user, Installed::new)
        };
        for declaration in declarations {
            if built_in.contains(&declaration.id) || specs.iter().any(|known| known.declaration.id == declaration.id) {
                problems.push(t!(
                    "ide-plugin-driver-taken",
                    name = plugin.name.as_str(),
                    driver = declaration.id.as_str()
                ));
                continue;
            }
            let (program, sha256) = match declaration.binary().and_then(|binary| Some((binary.file_name()?, binary))) {
                Some((file, binary)) => {
                    (folder.driver_program(&plugin.id, &declaration.id, &file), Some(binary.sha256.clone()))
                }
                None => (folder.driver_program(&plugin.id, &declaration.id, &declaration.id), Some(String::new())),
            };
            specs.push(zen_db::external::ExternalSpec {
                declaration,
                program,
                sha256,
                plugin: Some(plugin.id.clone()),
            });
        }
    }
    (specs, problems)
}

fn load(config: &PluginConfig, plugin: &InstalledPlugin) -> Result<WasmHighlighter, zen_plugins::PluginError> {
    let user = Installed::new(&config.folder);
    if user.list().iter().any(|own| own.id == plugin.id) {
        return user.load(plugin);
    }
    match &config.bundled {
        Some(folder) => Installed::new(folder).load(plugin),
        None => user.load(plugin),
    }
}

fn read_set(path: &std::path::Path) -> BTreeSet<String> {
    std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn write_set(path: &std::path::Path, set: &BTreeSet<String>) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = zen_vfs::write_atomic(path, &serde_json::to_vec_pretty(set).unwrap_or_default());
}

#[derive(Debug, Default)]
struct Switches {
    disabled: BTreeSet<String>,
    enabled: BTreeSet<String>,
}

impl Switches {
    fn read(config: &PluginConfig) -> Self {
        Self { disabled: read_set(&disabled_file(config)), enabled: read_set(&enabled_file(config)) }
    }

    fn on(&self, config: &PluginConfig, id: &str) -> bool {
        if config.optional.iter().any(|optional| optional == id) {
            self.enabled.contains(id)
        } else {
            !self.disabled.contains(id)
        }
    }

    fn set(&mut self, config: &PluginConfig, id: &str, on: bool) {
        self.disabled.remove(id);
        self.enabled.remove(id);
        if config.optional.iter().any(|optional| optional == id) {
            if on {
                self.enabled.insert(id.to_owned());
            }
        } else if !on {
            self.disabled.insert(id.to_owned());
        }
    }

    fn write(&self, config: &PluginConfig) {
        write_set(&disabled_file(config), &self.disabled);
        if !self.enabled.is_empty() || enabled_file(config).exists() {
            write_set(&enabled_file(config), &self.enabled);
        }
    }
}

fn enabled_file(config: &PluginConfig) -> PathBuf {
    config.folder.join("enabled.json")
}

fn disabled_file(config: &PluginConfig) -> PathBuf {
    config.folder.join("disabled.json")
}

fn ignored_file(config: &PluginConfig) -> PathBuf {
    config.folder.join("ignored-extensions.json")
}

fn ignored_plugins_file(config: &PluginConfig) -> PathBuf {
    config.folder.join("ignored-project-plugins.json")
}

pub fn platform_highlighter(name: &str) -> Option<Arc<dyn Highlighter>> {
    let editor_config = zen_workbench::editor_config::EditorConfigSyntax;
    Highlighter::handles(&editor_config, name).then(|| Arc::new(editor_config) as Arc<dyn Highlighter>)
}

#[cfg(test)]
pub(crate) mod fake {
    use std::sync::Mutex;

    use zen_plugins::{CATALOG_SCHEMA, Catalog, CatalogPlugin, CatalogVersion};

    pub(crate) const EMPTY_MODULE: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x10, 0x03, 0x60, 0x00, 0x01, 0x7f, 0x60, 0x01, 0x7f,
        0x01, 0x7f, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7e, 0x03, 0x04, 0x03, 0x00, 0x01, 0x02, 0x05, 0x03, 0x01, 0x00,
        0x01, 0x07, 0x30, 0x04, 0x06, b'm', b'e', b'm', b'o', b'r', b'y', 0x02, 0x00, 0x07, b'z', b'e', b'n', b'_',
        b'a', b'b', b'i', 0x00, 0x00, 0x09, b'z', b'e', b'n', b'_', b'a', b'l', b'l', b'o', b'c', 0x00, 0x01, 0x0d,
        b'z', b'e', b'n', b'_', b'h', b'i', b'g', b'h', b'l', b'i', b'g', b'h', b't', 0x00, 0x02, 0x0a, 0x11, 0x03,
        0x04, 0x00, 0x41, 0x00, 0x0b, 0x05, 0x00, 0x41, 0x80, 0x08, 0x0b, 0x04, 0x00, 0x42, 0x00, 0x0b,
    ];

    pub(crate) fn repository_plugins(folder: &std::path::Path) -> usize {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins");
        let mut written = 0;
        for entry in std::fs::read_dir(source).unwrap() {
            let path = entry.unwrap().path();
            let Ok(text) = std::fs::read_to_string(path.join("plugin.json")) else { continue };
            let id = path.file_name().unwrap().to_string_lossy().into_owned();
            let mut manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
            manifest["id"] = serde_json::json!(id);
            manifest["version"] = serde_json::json!("0.1.0");
            let target = folder.join(&id);
            std::fs::create_dir_all(&target).unwrap();
            std::fs::write(target.join("plugin.json"), manifest.to_string()).unwrap();
            std::fs::write(target.join("plugin.wasm"), EMPTY_MODULE).unwrap();
            written += 1;
        }
        written
    }

    pub(crate) fn plugin(id: &str, name: &str, extension: &str, versions: &[&str]) -> CatalogPlugin {
        CatalogPlugin {
            id: id.to_owned(),
            name: name.to_owned(),
            description: format!("Supports {name} files."),
            extensions: vec![extension.to_owned()],
            names: Vec::new(),
            hashbangs: Vec::new(),
            contents: Vec::new(),
            language_servers: Vec::new(),
            file_templates: zen_plugins::FileTemplateDeclarations::default(),
            live_templates: Default::default(),
            server_plugins: Vec::new(),
            requires: Vec::new(),
            project: zen_plugins::ProjectSupport::default(),
            database_drivers: Vec::new(),
            project_generators: Vec::new(),
            tests: Vec::new(),
            terminal_agents: Vec::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index: None,
            code_styles: Vec::new(),
            versions: versions
                .iter()
                .map(|version| CatalogVersion {
                    version: (*version).to_owned(),
                    date: "2026-10-19".to_owned(),
                    abi: 0,
                    url: format!("{id}-{version}.wasm"),
                    sha256: String::new(),
                    size: 0,
                    notes: format!("- {name} {version} notes."),
                })
                .collect(),
        }
    }

    pub(crate) struct FakeSource {
        pub(crate) catalog: Mutex<Catalog>,
        pub(crate) fetches: Mutex<usize>,
        pub(crate) downloads: Mutex<Vec<String>>,
        pub(crate) offline: Mutex<bool>,
        pub(crate) files: Mutex<std::collections::HashMap<String, Vec<u8>>>,
    }

    impl FakeSource {
        pub(crate) fn new(plugins: Vec<CatalogPlugin>) -> Self {
            Self {
                catalog: Mutex::new(Catalog { schema: CATALOG_SCHEMA, plugins }),
                fetches: Mutex::new(0),
                downloads: Mutex::new(Vec::new()),
                offline: Mutex::new(false),
                files: Mutex::new(std::collections::HashMap::new()),
            }
        }
    }

    impl super::PluginSource for FakeSource {
        fn catalog(&self) -> Result<Catalog, String> {
            *self.fetches.lock().unwrap() += 1;
            if *self.offline.lock().unwrap() {
                return Err("offline".to_owned());
            }
            Ok(self.catalog.lock().unwrap().clone())
        }

        fn module(&self, plugin: &CatalogPlugin, version: &CatalogVersion) -> Result<Vec<u8>, String> {
            self.downloads.lock().unwrap().push(format!("{}@{}", plugin.id, version.version));
            Ok(EMPTY_MODULE.to_vec())
        }

        fn file(&self, url: &str) -> Result<Vec<u8>, String> {
            self.downloads.lock().unwrap().push(url.to_owned());
            self.files.lock().unwrap().get(url).cloned().ok_or_else(|| format!("{url}: not found"))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::fake::{EMPTY_MODULE, FakeSource, plugin};
    use super::{CatalogState, Offer, PluginConfig, PluginSource, Plugins};
    use zen_plugins::{CATALOG_SCHEMA, Catalog, CatalogPlugin, CatalogVersion};
    use zen_workbench::plugin_manager::{CatalogStatus, PluginManager};

    fn manifest(folder: &std::path::Path, id: &str, version: &str, extension: &str) {
        let target = folder.join(id);
        std::fs::create_dir_all(&target).unwrap();
        let json = serde_json::json!({ "id": id, "name": id, "version": version, "extensions": [extension] });
        std::fs::write(target.join("plugin.json"), serde_json::to_vec(&json).unwrap()).unwrap();
        std::fs::write(target.join("plugin.wasm"), EMPTY_MODULE).unwrap();
    }

    fn catalog() -> Catalog {
        let plugin = |id: &str, extension: &str| CatalogPlugin {
            id: id.to_owned(),
            name: id.to_owned(),
            description: String::new(),
            extensions: vec![extension.to_owned()],
            names: Vec::new(),
            hashbangs: Vec::new(),
            contents: Vec::new(),
            language_servers: Vec::new(),
            file_templates: zen_plugins::FileTemplateDeclarations::default(),
            live_templates: Default::default(),
            server_plugins: Vec::new(),
            requires: Vec::new(),
            project: zen_plugins::ProjectSupport::default(),
            database_drivers: Vec::new(),
            project_generators: Vec::new(),
            tests: Vec::new(),
            terminal_agents: Vec::new(),
            schemas: Vec::new(),
            runs: Vec::new(),
            person_only: Vec::new(),
            target_files: Vec::new(),
            tool_windows: Vec::new(),
            index: None,
            code_styles: Vec::new(),
            versions: vec![CatalogVersion {
                version: "0.2.0".to_owned(),
                date: "2026-10-19".to_owned(),
                abi: 1,
                url: format!("{id}.wasm"),
                sha256: String::new(),
                size: 0,
                notes: String::new(),
            }],
        };
        Catalog { schema: CATALOG_SCHEMA, plugins: vec![plugin("markdown", "md"), plugin("sql", "sql")] }
    }

    fn ide(root: &std::path::Path, bundled: &[&str]) -> Arc<Plugins> {
        let bundle = root.join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        for id in bundled {
            manifest(&bundle, id, "0.1.0", id);
        }
        let plugins = Plugins::new(Some(PluginConfig {
            folder: root.join("user"),
            catalog_url: Some("http://127.0.0.1:1/releases/plugins.json".to_owned()),
            public_key: String::new(),
            bundled: Some(bundle),
            optional: Vec::new(),
        }));
        plugins.state().catalog = CatalogState::Ready(Arc::new(catalog()));
        plugins
    }

    fn with_fake(
        root: &std::path::Path,
        bundled: &[&str],
        downloaded: &[(&str, &str)],
    ) -> (Arc<Plugins>, Arc<FakeSource>) {
        let bundle = root.join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        for id in bundled {
            manifest(&bundle, id, "0.1.0", id);
        }
        for (id, version) in downloaded {
            manifest(&root.join("user"), id, version, id);
        }
        let source = Arc::new(FakeSource::new(vec![
            plugin("markdown", "Markdown", "md", &["0.1.0", "0.3.0"]),
            plugin("sql", "SQL", "sql", &["0.1.0", "0.2.0"]),
            plugin("yaml", "YAML", "yaml", &["0.1.0"]),
        ]));
        let config = PluginConfig {
            folder: root.join("user"),
            catalog_url: None,
            public_key: String::new(),
            bundled: Some(bundle),
            optional: Vec::new(),
        };
        (Plugins::with_source(Some(config), Some(Arc::clone(&source) as Arc<dyn PluginSource>)), source)
    }

    fn with_driver(id: &str, url: &str, sha256: &str) -> CatalogPlugin {
        let mut with = plugin(id, id, id, &["0.1.0", "0.2.0"]);
        with.database_drivers = vec![serde_json::json!({
            "id": format!("{id}db"),
            "name": format!("{id} DB"),
            "process": "shared",
            "capabilities": { "models": ["relational"], "language": "sql:duckdb" },
            "platforms": { zen_db::external::platform(): { "url": url, "sha256": sha256, "file": "driver" } },
        })];
        with
    }

    #[test]
    fn installing_a_plugin_downloads_its_driver_program_checks_it_and_declares_the_driver() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &[], &[]);
        let program = b"#!/bin/sh\nexit 0\n".to_vec();
        let sum = zen_update::sha256_hex(&program);
        let duck = with_driver("duck", "drivers/duck", &sum);
        let broken = with_driver("bad", "drivers/bad", &"0".repeat(64));
        source.files.lock().unwrap().insert("drivers/duck".into(), program.clone());
        source.files.lock().unwrap().insert("drivers/bad".into(), program.clone());
        source.catalog.lock().unwrap().plugins.extend([duck.clone(), broken.clone()]);
        assert!(plugins.database_drivers().is_empty());
        plugins.install_plugin(duck);
        plugins.install_plugin(broken);
        settle(&plugins);
        let path = root.path().join("user/duck/drivers/duckdb/driver");
        assert_eq!(std::fs::read(&path).unwrap(), program);
        assert!(
            !root.path().join("user/bad/drivers/baddb/driver").exists(),
            "a program that does not match is not kept"
        );
        let drivers = plugins.database_drivers();
        let ids: Vec<&str> = drivers.iter().map(|driver| driver.id()).collect();
        assert_eq!(ids, ["baddb", "duckdb"]);
        let infos: Vec<bool> = drivers.iter().map(|driver| driver.external().unwrap().info().installed).collect();
        assert_eq!(infos, [false, true]);
        let messages = plugins.take_messages();
        assert!(messages.iter().any(|message| message.contains("duck DB")), "{messages:?}");
        assert!(
            messages.iter().any(|message| message.contains("bad DB") && message.contains("SHA-256")),
            "{messages:?}"
        );
        plugins.set_enabled("duck", false).unwrap();
        let ids: Vec<String> = plugins.database_drivers().iter().map(|driver| driver.id().to_owned()).collect();
        assert_eq!(ids, ["baddb"], "a plug-in switched off takes its drivers along");
        plugins.uninstall("bad").unwrap();
        assert!(plugins.database_drivers().is_empty());
    }

    #[test]
    fn an_agent_does_not_install_a_plugin_that_brings_driver_programs() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &[], &[]);
        let program = b"#!/bin/sh\nexit 0\n".to_vec();
        let duck = with_driver("duck", "drivers/duck", &zen_update::sha256_hex(&program));
        source.files.lock().unwrap().insert("drivers/duck".into(), program);
        source.catalog.lock().unwrap().plugins.push(duck.clone());
        let _ = plugins.install_plugin_by_id("duck");
        settle(&plugins);
        let refused = plugins.install_plugin_by_id("duck").unwrap_err();
        assert!(refused.contains("duck") && refused.contains("only you"), "{refused}");
        assert_eq!(plugins.agent_may_install(Some("yaml")), Ok(()), "a plug-in without programs installs");
        assert!(!root.path().join("user/duck").exists(), "nothing was downloaded");
        plugins.install_plugin(duck);
        settle(&plugins);
        assert!(root.path().join("user/duck/drivers/duckdb/driver").is_file());
    }

    #[test]
    fn an_agent_does_not_install_a_plugin_that_brings_terminal_agent_adapters() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &[], &[]);
        let mut agents = plugin("agents", "Agents", "agents", &["0.1.0"]);
        agents.terminal_agents = vec![serde_json::json!({
            "id": "evil", "name": "Evil", "programs": ["sh"], "resume": ["sh", "-c", "{session}"]
        })];
        source.catalog.lock().unwrap().plugins.push(agents);
        let _ = plugins.install_plugin_by_id("agents");
        settle(&plugins);
        let refused = plugins.install_plugin_by_id("agents").unwrap_err();
        assert!(refused.contains("Agents") && refused.contains("only you"), "{refused}");
        assert!(!root.path().join("user/agents").exists(), "nothing was installed");

        let mut person = plugin("agents", "Agents", "agents", &["0.1.0"]);
        person.terminal_agents = source.catalog.lock().unwrap().plugins.last().unwrap().terminal_agents.clone();
        plugins.install_plugin(person);
        settle(&plugins);
        plugins.set_enabled("agents", false).unwrap();
        let refused = plugins.agent_may_enable("agents").unwrap_err();
        assert!(refused.contains("Agents") && refused.contains("only you"), "{refused}");
        assert_eq!(plugins.agent_may_enable("yaml"), Ok(()), "a plug-in without programs or adapters switches on");
        plugins.set_enabled("agents", true).unwrap();
        assert_eq!(plugins.agent_may_enable("agents"), Ok(()), "one already on is no change");
    }

    #[test]
    fn updating_a_plugin_replaces_its_driver_program() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &[], &[]);
        let old = b"#!/bin/sh\necho old\n".to_vec();
        let new = b"#!/bin/sh\necho new\n".to_vec();
        let mut first = with_driver("duck", "artifacts/plugin-duck/0.1.0/driver", &zen_update::sha256_hex(&old));
        first.versions.truncate(1);
        let second = with_driver("duck", "artifacts/plugin-duck/0.2.0/driver", &zen_update::sha256_hex(&new));
        source.files.lock().unwrap().insert("artifacts/plugin-duck/0.1.0/driver".into(), old.clone());
        source.files.lock().unwrap().insert("artifacts/plugin-duck/0.2.0/driver".into(), new.clone());
        source.catalog.lock().unwrap().plugins.push(first.clone());
        plugins.install_plugin(first);
        settle(&plugins);
        let path = root.path().join("user/duck/drivers/duckdb/driver");
        assert_eq!(std::fs::read(&path).unwrap(), old);
        source.catalog.lock().unwrap().plugins = vec![second];
        plugins.state().catalog = CatalogState::Ready(Arc::new(source.catalog.lock().unwrap().clone()));
        PluginManager::update(&*plugins, "duck").unwrap();
        settle(&plugins);
        assert_eq!(std::fs::read(&path).unwrap(), new, "the update brings the new program");
        let drivers = plugins.database_drivers();
        assert!(drivers.iter().any(|driver| driver.id() == "duckdb" && driver.external().unwrap().info().installed));
    }

    #[test]
    fn the_update_at_start_leaves_plugins_with_database_drivers_to_the_user() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &[], &[("duck", "0.1.0"), ("sql", "0.1.0")]);
        let duck = with_driver("duck", "drivers/duck", &"0".repeat(64));
        source.catalog.lock().unwrap().plugins.push(duck.clone());
        plugins.state().catalog = CatalogState::Ready(Arc::new(source.catalog.lock().unwrap().clone()));
        let at_start: Vec<String> = plugins.start_updates().into_iter().map(|job| job.plugin.id).collect();
        assert_eq!(at_start, ["sql"], "nothing is downloaded for a driver without the user");
        let asked: Vec<String> = plugins.updates().into_iter().map(|job| job.plugin.id).collect();
        assert!(asked.contains(&"duck".to_owned()), "Update offers it");
    }

    #[test]
    fn a_driver_whose_id_the_ide_has_is_not_declared() {
        let root = tempfile::tempdir().unwrap();
        let user = root.path().join("user");
        std::fs::create_dir_all(user.join("pg")).unwrap();
        let json = serde_json::json!({
            "id": "pg", "name": "PG", "version": "0.1.0", "extensions": [],
            "databaseDrivers": [{ "id": "postgresql", "name": "Other", "capabilities": { "models": ["relational"] } }],
        });
        std::fs::write(user.join("pg/plugin.json"), json.to_string()).unwrap();
        std::fs::write(user.join("pg/plugin.wasm"), EMPTY_MODULE).unwrap();
        let config = PluginConfig {
            folder: user,
            catalog_url: None,
            public_key: String::new(),
            bundled: None,
            optional: Vec::new(),
        };
        let plugins = Plugins::with_source(Some(config), None);
        assert!(plugins.database_drivers().is_empty());
        let messages = plugins.take_messages();
        assert!(messages.iter().any(|message| message.contains("postgresql")), "{messages:?}");
    }

    fn busy(plugins: &Plugins) -> bool {
        let state = plugins.state();
        matches!(state.catalog, CatalogState::Fetching(_))
            || !state.queue.is_empty()
            || state.working
            || state.loading > 0
    }

    fn settle(plugins: &Plugins) {
        let start = Instant::now();
        while busy(plugins) {
            assert!(start.elapsed() < Duration::from_secs(60), "the background work did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_plugin_declaring_generators_without_the_export_offers_none() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &[], &[]);
        let mut with = plugin("vite", "Vite", "vite", &["0.1.0"]);
        with.project_generators =
            zen_plugins::GeneratorDeclaration::parse_list(r#"[{ "id": "app", "name": "App" }]"#).unwrap();
        source.catalog.lock().unwrap().plugins.push(with.clone());
        plugins.install_plugin(with);
        settle(&plugins);
        assert_eq!(plugins.installed()[0].project_generators.len(), 1, "the manifest keeps them");
        let environment = zen_project::generators::GeneratorEnvironment::default();
        assert!(plugins.project_generators(&environment, "zen-ide/0").is_empty());
    }

    #[test]
    fn the_offer_for_a_file_reads_no_plug_in_folder() {
        let root = tempfile::tempdir().unwrap();
        let plugins = ide(root.path(), &["markdown", "sql"]);
        assert_eq!(plugins.offer("query.sql"), None);
        for folder in std::fs::read_dir(root.path()).unwrap().filter_map(Result::ok) {
            std::fs::remove_dir_all(folder.path()).unwrap();
        }
        assert_eq!(plugins.offer("query.sql"), None, "still known to be installed");
    }

    #[test]
    fn an_optional_bundled_plugin_is_off_until_switched_on() {
        use zen_modules::AreaPlugins;

        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("bundle");
        for id in ["markdown", "sql"] {
            manifest(&bundle, id, "0.1.0", id);
        }
        let config = PluginConfig {
            folder: root.path().join("user"),
            catalog_url: None,
            public_key: String::new(),
            bundled: Some(bundle),
            optional: vec!["sql".to_owned()],
        };
        let on = |plugins: &Plugins| -> Vec<(String, bool)> {
            plugins.entries().into_iter().map(|entry| (entry.plugin.id, entry.enabled)).collect()
        };
        let plugins = Plugins::with_source(Some(config.clone()), None);
        assert_eq!(on(&plugins), [("markdown".to_owned(), true), ("sql".to_owned(), false)]);
        assert_eq!(plugins.highlighters().len(), 1, "an optional plug-in that is off is not loaded");
        assert!(!AreaPlugins::plugin(&*plugins, "sql").unwrap().enabled);

        AreaPlugins::set_enabled(&*plugins, "sql", true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while plugins.highlighters().len() < 2 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(plugins.highlighters().len(), 2, "switched on without a restart");
        let enabled: Vec<String> =
            serde_json::from_slice(&std::fs::read(root.path().join("user/enabled.json")).unwrap()).unwrap();
        assert_eq!(enabled, ["sql"]);
        let again = Plugins::with_source(Some(config.clone()), None);
        assert_eq!(on(&again), [("markdown".to_owned(), true), ("sql".to_owned(), true)], "remembered");

        again.set_enabled("sql", false).unwrap();
        again.set_enabled("markdown", false).unwrap();
        let after = Plugins::with_source(Some(config), None);
        assert_eq!(on(&after), [("markdown".to_owned(), false), ("sql".to_owned(), false)]);
        assert!(after.highlighters().is_empty());
        assert!(AreaPlugins::plugin(&*after, "cobol").is_none());
    }

    #[test]
    fn a_plugin_bundled_with_another_ide_installs_from_the_catalog() {
        let with_sql = tempfile::tempdir().unwrap();
        let without_sql = tempfile::tempdir().unwrap();
        let bundling = ide(with_sql.path(), &["markdown", "sql"]);
        let other = ide(without_sql.path(), &["markdown"]);

        let sql = |plugins: &Plugins| {
            plugins.catalog_entries(&catalog()).into_iter().find(|entry| entry.plugin.id == "sql").unwrap()
        };
        assert!(sql(&bundling).bundled);
        assert_eq!(sql(&bundling).installed.as_deref(), Some("0.1.0"));
        assert!(!sql(&other).bundled);
        assert_eq!(sql(&other).installed, None);

        assert_eq!(bundling.offer("query.sql"), None);
        assert!(matches!(other.offer("query.sql"), Some(Offer::Install { plugin, .. }) if plugin.id == "sql"));
    }

    #[test]
    fn a_catalog_update_of_a_bundled_plugin_stays_bundled() {
        let root = tempfile::tempdir().unwrap();
        let plugins = ide(root.path(), &["markdown"]);
        manifest(&root.path().join("user"), "markdown", "0.2.0", "md");
        manifest(&root.path().join("user"), "sql", "0.2.0", "sql");

        let entries = plugins.entries();
        let flags: Vec<_> = entries
            .iter()
            .map(|entry| (entry.plugin.id.as_str(), entry.plugin.version.as_str(), entry.bundled))
            .collect();
        assert_eq!(flags, [("markdown", "0.2.0", true), ("sql", "0.2.0", false)]);
        let markdown =
            plugins.catalog_entries(&catalog()).into_iter().find(|entry| entry.plugin.id == "markdown").unwrap();
        assert!(markdown.bundled);
        assert_eq!(markdown.installed.as_deref(), Some("0.2.0"));
    }

    #[test]
    fn list_plugins_reports_bundled_for_every_plugin() {
        let root = tempfile::tempdir().unwrap();
        let plugins = ide(root.path(), &["markdown"]);
        let installed = serde_json::to_value(plugins.entries()).unwrap();
        assert_eq!(installed[0]["id"], "markdown");
        assert_eq!(installed[0]["bundled"], true);
        let listed = serde_json::to_value(plugins.catalog_entries(&catalog())).unwrap();
        assert_eq!(listed[0]["id"], "markdown");
        assert_eq!(listed[0]["bundled"], true);
        assert_eq!(listed[0]["installed"], "0.1.0");
        assert_eq!(listed[1]["id"], "sql");
        assert_eq!(listed[1]["bundled"], false);
        assert!(listed[1]["installed"].is_null());
        assert_eq!(listed[1]["versions"][0]["version"], "0.2.0");
    }

    #[test]
    fn the_catalog_is_fetched_only_when_asked_for() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &["markdown"], &[]);
        let installed = PluginManager::installed(&*plugins, "");
        assert_eq!(installed.catalog, CatalogStatus::NotFetched);
        assert_eq!(installed.plugins.len(), 1);
        assert_eq!(installed.plugins[0].update, None, "no update is known before the catalog is fetched");
        assert_eq!(*source.fetches.lock().unwrap(), 0, "listing the installed plug-ins does not go online");

        assert_ne!(
            PluginManager::catalog(&*plugins, "").catalog,
            CatalogStatus::NotFetched,
            "the Marketplace asks for it"
        );
        settle(&plugins);
        let marketplace = PluginManager::catalog(&*plugins, "");
        assert_eq!(marketplace.catalog, CatalogStatus::Ready);
        PluginManager::catalog(&*plugins, "");
        assert_eq!(*source.fetches.lock().unwrap(), 1, "once per session");
        let _ = plugins.refresh();
        settle(&plugins);
        assert_eq!(*source.fetches.lock().unwrap(), 2, "Check for Updates fetches it again");
    }

    #[test]
    fn a_failed_check_for_updates_keeps_the_catalog_fetched_before() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &["markdown"], &[]);
        PluginManager::catalog(&*plugins, "");
        settle(&plugins);
        *source.offline.lock().unwrap() = true;
        let _ = plugins.refresh();
        settle(&plugins);
        let marketplace = PluginManager::catalog(&*plugins, "");
        assert_eq!(marketplace.catalog, CatalogStatus::Ready);
        assert_eq!(marketplace.plugins.len(), 3, "the previous catalog is still listed");
        assert_eq!(marketplace.check_failed.as_deref(), Some("offline"));
        assert_eq!(PluginManager::installed(&*plugins, "").plugins[0].update.as_deref(), Some("0.3.0"));
        assert!(plugins.update("markdown").is_ok(), "updates still work from the previous catalog");
        settle(&plugins);

        *source.offline.lock().unwrap() = false;
        let _ = plugins.refresh();
        settle(&plugins);
        assert_eq!(PluginManager::installed(&*plugins, "").check_failed, None, "a good check clears the failure");
    }

    #[test]
    fn a_first_fetch_that_fails_leaves_the_catalog_unavailable() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &["markdown"], &[]);
        *source.offline.lock().unwrap() = true;
        PluginManager::catalog(&*plugins, "");
        settle(&plugins);
        let list = PluginManager::catalog(&*plugins, "");
        assert_eq!((list.catalog, list.plugins.len(), list.check_failed), (CatalogStatus::Unavailable, 0, None));
    }

    #[test]
    fn the_start_update_failing_offline_does_not_update_on_a_later_check() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &["markdown"], &[("sql", "0.1.0")]);
        *source.offline.lock().unwrap() = true;
        plugins.update_in_background();
        settle(&plugins);
        *source.offline.lock().unwrap() = false;
        let _ = plugins.refresh();
        settle(&plugins);
        PluginManager::catalog(&*plugins, "");
        settle(&plugins);
        assert!(source.downloads.lock().unwrap().is_empty(), "only Update or Update All installs the updates");
        assert_eq!(
            PluginManager::installed(&*plugins, "").plugins.iter().filter(|plugin| plugin.update.is_some()).count(),
            2
        );
    }

    #[test]
    fn the_start_update_installs_the_newer_versions() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &["markdown"], &[("sql", "0.1.0")]);
        plugins.update_in_background();
        settle(&plugins);
        assert_eq!(*source.downloads.lock().unwrap(), ["markdown@0.3.0", "sql@0.2.0"]);
    }

    #[test]
    fn the_details_cut_overlong_notes() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &[], &[]);
        source.catalog.lock().unwrap().plugins[2].versions[0].notes =
            "x".repeat(zen_workbench::plugin_manager::NOTES_LIMIT * 2);
        PluginManager::catalog(&*plugins, "");
        settle(&plugins);
        let notes = &plugins.details("yaml").unwrap().notes[0].notes;
        assert!(notes.len() <= zen_workbench::plugin_manager::NOTES_LIMIT + "…".len() && notes.ends_with('…'));
    }

    #[test]
    fn the_mcp_tools_keep_their_fields() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, _) = with_fake(root.path(), &["markdown"], &[]);
        assert!(plugins.install_plugin_by_id("yaml").is_err(), "the catalog is loading");
        settle(&plugins);
        let listed = plugins.list_plugins();
        assert!(listed["installing"].is_null() && listed["installingIds"] == serde_json::json!([]), "{listed}");
        assert_eq!(listed["installed"][0]["id"], "markdown");
        assert_eq!(listed["catalog"][2]["id"], "yaml");
        let installing = plugins.install_plugin_by_id("yaml").unwrap();
        assert_eq!(installing["installing"], "YAML", "the name, as before");
        assert_eq!(installing["queued"], serde_json::json!(["yaml"]));
        settle(&plugins);
        let again = plugins.install_plugin_by_id("yaml").unwrap();
        assert_eq!(again["installing"], "YAML", "an installed plug-in is installed again, as before");
        settle(&plugins);
        assert!(plugins.install_plugin_by_id("nope").is_err());
    }

    #[test]
    fn the_marketplace_searches_and_sorts_by_name() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, _) = with_fake(root.path(), &["markdown"], &[]);
        PluginManager::catalog(&*plugins, "");
        settle(&plugins);
        let names = |query: &str| -> Vec<String> {
            PluginManager::catalog(&*plugins, query).plugins.into_iter().map(|plugin| plugin.name).collect()
        };
        assert_eq!(names(""), ["Markdown", "SQL", "YAML"]);
        assert_eq!(names("sql"), ["SQL"]);
        assert_eq!(names("supports files"), ["Markdown", "SQL", "YAML"]);
        assert_eq!(names("yaml"), ["YAML"]);
        assert!(names("nothing like this").is_empty());
        let markdown = PluginManager::catalog(&*plugins, "markdown").plugins.remove(0);
        assert_eq!(
            (markdown.installed.as_deref(), markdown.latest.as_deref(), markdown.update.as_deref(), markdown.bundled),
            (Some("0.1.0"), Some("0.3.0"), Some("0.3.0"), true)
        );
    }

    #[test]
    fn install_update_and_update_all_go_through_the_queue() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &["markdown"], &[("sql", "0.1.0")]);
        assert!(plugins.install("yaml").is_err(), "the catalog must be fetched first");
        settle(&plugins);

        let queued = plugins.install("yaml").unwrap();
        assert_eq!(queued.queued, ["yaml"]);
        settle(&plugins);
        let yaml = PluginManager::installed(&*plugins, "yaml").plugins.remove(0);
        assert_eq!((yaml.installed.as_deref(), yaml.enabled, yaml.bundled), (Some("0.1.0"), true, false));
        assert!(plugins.highlighter(&[], "a.yaml").is_some(), "it works at once");
        assert!(plugins.install("yaml").is_err(), "it is up to date");
        assert!(plugins.install("nope").is_err());

        let sql = plugins.update("sql").unwrap();
        assert_eq!(sql.queued, ["sql"]);
        settle(&plugins);
        assert!(plugins.update("sql").is_err(), "nothing newer");
        assert!(plugins.update("css").is_err(), "not installed");

        let updates: Vec<_> = PluginManager::installed(&*plugins, "")
            .plugins
            .into_iter()
            .filter_map(|plugin| plugin.update.map(|version| (plugin.id, version)))
            .collect();
        assert_eq!(updates, [("markdown".to_owned(), "0.3.0".to_owned())]);
        let all = plugins.update_all().unwrap();
        assert_eq!(all.queued, ["markdown"]);
        settle(&plugins);
        assert!(plugins.update_all().unwrap().queued.is_empty(), "everything is up to date");
        let markdown = PluginManager::installed(&*plugins, "markdown").plugins.remove(0);
        assert_eq!((markdown.installed.as_deref(), markdown.bundled, markdown.update), (Some("0.3.0"), true, None));
        assert_eq!(*source.downloads.lock().unwrap(), ["yaml@0.1.0", "sql@0.2.0", "markdown@0.3.0"]);
        let messages = plugins.take_messages();
        assert_eq!(messages.len(), 3, "{messages:?}");
        assert!(plugins.restart_required().is_empty(), "highlighting plug-ins change without a restart");
    }

    #[test]
    fn details_show_the_notes_of_the_newest_and_the_installed_version() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, _) = with_fake(root.path(), &["markdown"], &[]);
        let offline = plugins.details("markdown").unwrap();
        assert_eq!(offline.notes.len(), 1, "without the catalog, the installed version");
        assert_eq!(offline.notes[0].version, "0.1.0");
        PluginManager::catalog(&*plugins, "");
        settle(&plugins);
        let details = plugins.details("markdown").unwrap();
        let notes: Vec<_> = details.notes.iter().map(|notes| (notes.version.as_str(), notes.notes.as_str())).collect();
        assert_eq!(notes, [("0.3.0", "- Markdown 0.3.0 notes."), ("0.1.0", "- Markdown 0.1.0 notes.")]);
        assert_eq!(details.plugin.update.as_deref(), Some("0.3.0"));
        let yaml = plugins.details("yaml").unwrap();
        assert_eq!(yaml.notes.len(), 1);
        assert_eq!(yaml.plugin.installed, None);
        assert!(plugins.details("nope").is_err());
    }

    #[test]
    fn a_plugin_cannot_take_the_platforms_own_file_types() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, _) = with_fake(root.path(), &["native", "zt-own"], &[]);
        assert!(plugins.highlighter(&[], "a.zt-own").is_some(), "another plug-in's type is declared");
        let registry = zen_vfs::file_types::current();
        assert_eq!(registry.type_of("a.native").map(|ty| ty.id), None, "nothing opens in an application by a plug-in");
    }

    #[test]
    fn a_bundled_plugin_cannot_be_uninstalled_but_its_update_can() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let root = tempfile::tempdir().unwrap();
        let (plugins, _) = with_fake(root.path(), &["markdown"], &[("sql", "0.1.0")]);
        assert_eq!(
            PluginManager::uninstall(&*plugins, "markdown").unwrap_err(),
            zen_i18n::t!("settings-plugins-bundled-cannot-uninstall")
        );
        PluginManager::catalog(&*plugins, "");
        settle(&plugins);
        plugins.update("markdown").unwrap();
        settle(&plugins);
        PluginManager::uninstall(&*plugins, "markdown").unwrap();
        let markdown = PluginManager::installed(&*plugins, "markdown").plugins.remove(0);
        assert_eq!(
            (markdown.installed.as_deref(), markdown.bundled),
            (Some("0.1.0"), true),
            "the bundled version is back"
        );
        assert!(plugins.highlighter(&[], "a.markdown").is_some());

        PluginManager::set_enabled(&*plugins, "markdown", false).unwrap();
        assert!(plugins.highlighter(&[], "a.markdown").is_none());
        PluginManager::uninstall(&*plugins, "sql").unwrap();
        assert!(PluginManager::installed(&*plugins, "sql").plugins.is_empty());
        assert!(PluginManager::uninstall(&*plugins, "sql").is_err());
    }

    #[test]
    fn a_plugin_with_language_servers_asks_for_a_restart() {
        let root = tempfile::tempdir().unwrap();
        let (plugins, source) = with_fake(root.path(), &[], &[]);
        {
            let mut catalog = source.catalog.lock().unwrap();
            catalog.plugins[2].language_servers = zen_plugins::ServerDeclaration::parse_list(
                r#"[{ "id": "yaml-ls", "name": "YAML LS", "languages": ["yaml"], "extensions": ["yaml"], "command": "yaml-ls" }]"#,
            )
            .unwrap();
        }
        PluginManager::catalog(&*plugins, "");
        settle(&plugins);
        plugins.install("yaml").unwrap();
        settle(&plugins);
        assert_eq!(plugins.restart_required(), ["YAML"]);
        assert!(PluginManager::installed(&*plugins, "").plugins[0].language_servers);
    }

    #[test]
    fn a_required_plugin_is_offered_with_the_one_that_requires_it() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("web");
        std::fs::create_dir_all(&project).unwrap();
        let packages = |package: &str| {
            zen_plugins::ProjectSupport::parse(&format!(r#"{{ "markers": {{ "packages": ["{package}"] }} }}"#)).unwrap()
        };
        let mut react = plugin("react", "React", "react", &["0.1.0"]);
        react.project = packages("react");
        let mut next = plugin("next", "Next.js", "next", &["0.1.0"]);
        next.requires = vec!["react".to_owned()];
        next.project = packages("next");
        let source = Arc::new(FakeSource::new(vec![react, next]));
        let config = PluginConfig {
            folder: root.path().join("user"),
            catalog_url: None,
            public_key: String::new(),
            bundled: None,
            optional: Vec::new(),
        };
        let plugins = Plugins::with_source(Some(config), Some(Arc::clone(&source) as Arc<dyn PluginSource>));
        plugins.catalog();
        settle(&plugins);
        let offers = |plugins: &Plugins| -> Vec<String> {
            plugins.project_offers(&project).into_iter().map(|plugin| plugin.id).collect()
        };
        std::fs::write(project.join("package.json"), r#"{ "dependencies": { "react": "^19" } }"#).unwrap();
        assert_eq!(offers(&plugins), ["react"]);
        std::fs::write(project.join("package.json"), r#"{ "dependencies": { "next": "15", "react": "^19" } }"#)
            .unwrap();
        assert_eq!(offers(&plugins), ["next"]);
        let next = plugins.project_offers(&project).remove(0);
        assert_eq!(plugins.missing_requirements(&next), ["React"]);
    }

    #[test]
    fn a_sveltekit_project_is_offered_sveltekit_with_svelte() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        let mut svelte = plugin("svelte", "Svelte", "svelte", &["0.1.0"]);
        svelte.project = zen_plugins::ProjectSupport::parse(r#"{ "markers": { "packages": ["svelte"] } }"#).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../../plugins/sveltekit/plugin.json")).unwrap();
        let mut kit = plugin("sveltekit", "SvelteKit", "sveltekit", &["0.1.0"]);
        kit.extensions = Vec::new();
        kit.requires = vec!["svelte".to_owned()];
        kit.project = zen_plugins::ProjectSupport::parse(&manifest["project"].to_string()).unwrap();
        let source = Arc::new(FakeSource::new(vec![svelte, kit]));
        let config = PluginConfig {
            folder: root.path().join("user"),
            catalog_url: None,
            public_key: String::new(),
            bundled: None,
            optional: Vec::new(),
        };
        let plugins = Plugins::with_source(Some(config), Some(Arc::clone(&source) as Arc<dyn PluginSource>));
        plugins.catalog();
        settle(&plugins);
        let manifest = r#"{ "devDependencies": { "@sveltejs/kit": "^2", "svelte": "^5" } }"#;
        std::fs::write(project.join("package.json"), manifest).unwrap();
        let offers = plugins.project_offers(&project);
        assert_eq!(offers.iter().map(|plugin| plugin.id.as_str()).collect::<Vec<_>>(), ["sveltekit"]);
        assert_eq!(plugins.missing_requirements(&offers[0]), ["Svelte"]);
        plugins.install_plugin(offers[0].clone());
        settle(&plugins);
        assert_eq!(*source.downloads.lock().unwrap(), ["svelte@0.1.0", "sveltekit@0.1.0"]);
    }

    #[test]
    fn a_project_offers_its_plugins_which_install_with_their_requirements() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("shop");
        std::fs::create_dir_all(&project).unwrap();
        let markers = |file: &str| {
            zen_plugins::ProjectSupport::parse(&format!(r#"{{ "markers": {{ "files": ["{file}"] }} }}"#)).unwrap()
        };
        let mut nuxt = plugin("nuxt", "Nuxt", "nuxt", &["0.1.0"]);
        nuxt.extensions = Vec::new();
        nuxt.requires = vec!["vue".to_owned()];
        nuxt.project = markers("nuxt.config.ts");
        let mut svelte = plugin("svelte", "Svelte", "svelte", &["0.1.0"]);
        svelte.project = markers("svelte.config.js");
        let source = Arc::new(FakeSource::new(vec![nuxt, svelte, plugin("vue", "Vue", "vue", &["0.2.0"])]));
        let config = PluginConfig {
            folder: root.path().join("user"),
            catalog_url: None,
            public_key: String::new(),
            bundled: None,
            optional: Vec::new(),
        };
        let open = || Plugins::with_source(Some(config.clone()), Some(Arc::clone(&source) as Arc<dyn PluginSource>));
        let plugins = open();
        std::fs::write(project.join("nuxt.config.ts"), "").unwrap();
        assert!(plugins.project_offers(&project).is_empty(), "only a catalog fetched already is looked at");
        let before = plugins.generation();
        plugins.catalog();
        settle(&plugins);
        assert!(plugins.generation() > before, "a fetch changes the generation");
        let offers: Vec<String> = plugins.project_offers(&project).into_iter().map(|plugin| plugin.id).collect();
        assert_eq!(offers, ["nuxt"]);
        let nuxt = plugins.project_offers(&project).remove(0);
        assert_eq!(plugins.missing_requirements(&nuxt), ["Vue"]);

        let before = plugins.generation();
        plugins.install_plugin(nuxt);
        settle(&plugins);
        assert_eq!(*source.downloads.lock().unwrap(), ["vue@0.2.0", "nuxt@0.1.0"], "the requirement first");
        assert!(plugins.generation() > before);
        assert!(plugins.project_offers(&project).is_empty(), "installed");
        let support = plugins.project_support();
        assert_eq!(support.iter().map(|plugin| plugin.id.as_str()).collect::<Vec<_>>(), ["nuxt"]);
        assert!(support[0].support.supports(&project));

        std::fs::write(project.join("svelte.config.js"), "").unwrap();
        assert_eq!(plugins.project_offers(&project)[0].id, "svelte");
        plugins.ignore_for_projects("svelte");
        assert!(plugins.project_offers(&project).is_empty());
        let again = open();
        again.catalog();
        settle(&again);
        assert!(again.project_offers(&project).is_empty(), "remembered");
    }
}

#[cfg(all(test, unix))]
mod server_tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use super::fake::{FakeSource, plugin};
    use super::{Offer, PluginConfig, PluginSource, Plugins};
    use zen_workbench::language::servers::{
        InstallKind, InstallRecipe, LanguageServers, NotInstallable, ServerDeclaration, ServerSource, ServerTools,
    };
    use zen_workbench::plugin_manager::PluginManager;

    #[derive(Default)]
    struct FakeNpm {
        on_path: Mutex<BTreeMap<String, PathBuf>>,
        no_npm: Mutex<bool>,
    }

    impl ServerTools for FakeNpm {
        fn locate(&self, server: &ServerDeclaration) -> Option<PathBuf> {
            self.on_path.lock().unwrap().get(&server.id).cloned()
        }

        fn environment(&self, _: &ServerDeclaration) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
            Vec::new()
        }

        fn install_command(&self, _: &ServerDeclaration) -> Result<std::process::Command, NotInstallable> {
            Err(NotInstallable::Manual("only into the IDE's folder".to_owned()))
        }

        fn install_command_into(
            &self,
            server: &ServerDeclaration,
            prefix: &Path,
        ) -> Result<std::process::Command, NotInstallable> {
            if *self.no_npm.lock().unwrap() {
                return Err(NotInstallable::ToolMissing(InstallKind::Npm));
            }
            let bin = prefix.join("node_modules/.bin");
            let packages = server.install.as_ref().map(|recipe| recipe.package.clone()).unwrap_or_default();
            let mut script = format!(
                "echo '{prefix}' >> '{runs}'; echo added 1 package; mkdir -p '{bin}'",
                prefix = prefix.display(),
                runs = prefix.parent().unwrap_or(prefix).join("runs.log").display(),
                bin = bin.display(),
            );
            if packages.split_whitespace().any(|package| package == "fake-ls") {
                script.push_str(&format!(" && touch '{}'", bin.join(&server.command).display()));
            }
            for package in packages.split_whitespace() {
                let name = match package.rfind('@') {
                    Some(at) if at > 0 => &package[..at],
                    _ => package,
                };
                let folder = prefix.join("node_modules").join(name);
                script.push_str(&format!(
                    " && mkdir -p '{folder}' && echo '{{}}' > '{folder}/package.json'",
                    folder = folder.display()
                ));
            }
            let mut command = std::process::Command::new("sh");
            command.args(["-c", &script]);
            Ok(command)
        }
    }

    fn server(id: &str, command: &str) -> ServerDeclaration {
        ServerDeclaration {
            id: id.to_owned(),
            name: format!("{id} server"),
            languages: vec!["vue".to_owned()],
            extensions: vec!["vue".to_owned()],
            names: Vec::new(),
            watches: Vec::new(),
            command: command.to_owned(),
            args: vec!["--stdio".to_owned()],
            version_args: Vec::new(),
            install: Some(InstallRecipe {
                kind: InstallKind::Npm,
                url: String::new(),
                sha256: String::new(),
                files: Vec::new(),
                package: "fake-ls typescript@5".into(),
                hint: String::new(),
                platforms: Default::default(),
            }),
            initialization_options: serde_json::Value::Null,
            inlay_hints: None,
            language_ids: Default::default(),
            workspace_symbols: true,
            optimize_imports: Vec::new(),
            alongside: false,
            formats: false,
            fix_on_save: None,
            settings: Default::default(),
            from_project: false,
            when: None,
            power_save: None,
            java: None,
            notice: None,
            virtual_documents: None,
            generate: None,
            inspections: Vec::new(),
            inspection_settings: None,
            suppression: None,
            diagnostics_settle_ms: 0,
            schema_support: None,
        }
    }

    struct Ide {
        plugins: Arc<Plugins>,
        servers: LanguageServers,
        tools: Arc<FakeNpm>,
        source: Arc<FakeSource>,
        data: PathBuf,
        _root: tempfile::TempDir,
    }

    fn runs(ide: &Ide) -> Vec<PathBuf> {
        std::fs::read_to_string(ide.data.join("language-servers/runs.log"))
            .unwrap_or_default()
            .lines()
            .map(PathBuf::from)
            .collect()
    }

    fn ide() -> Ide {
        zen_i18n::register(crate::toolbox::LOCALES);
        let root = tempfile::tempdir().unwrap();
        let mut vue = plugin("vue", "Vue", "vue", &["0.1.0", "0.2.0"]);
        vue.language_servers = vec![server("vue-ls", "vue-language-server")];
        let source = Arc::new(FakeSource::new(vec![vue, plugin("sql", "SQL", "sql", &["0.1.0"])]));
        let config = PluginConfig {
            folder: root.path().join("user"),
            catalog_url: None,
            public_key: String::new(),
            bundled: None,
            optional: Vec::new(),
        };
        let plugins = Plugins::with_source(Some(config), Some(Arc::clone(&source) as Arc<dyn PluginSource>));
        let data = root.path().join("data");
        let tools = Arc::new(FakeNpm::default());
        let servers = LanguageServers::load(Some(data.join("language-servers.json")), tools.clone());
        plugins.set_language_servers(servers.clone());
        Ide { plugins, servers, tools, source, data, _root: root }
    }

    fn catalog_ready(plugins: &Plugins) {
        let start = Instant::now();
        while !matches!(plugins.catalog(), super::CatalogView::Ready(_)) {
            assert!(start.elapsed() < Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn wait(ide: &Ide, done: impl Fn(&Ide, &[String]) -> bool) -> Vec<String> {
        let start = Instant::now();
        let mut messages = Vec::new();
        loop {
            messages.extend(ide.plugins.take_messages());
            if ide.plugins.installing().is_empty() && done(ide, &messages) {
                return messages;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "{messages:?}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn installing_a_plugin_over_mcp_installs_its_server_into_the_ides_folder() {
        let ide = ide();
        catalog_ready(&ide.plugins);
        ide.plugins.install_plugin_by_id("vue").unwrap();
        let messages = wait(&ide, |ide, messages| {
            ide.servers.source("vue-ls") == ServerSource::Local
                && messages.iter().any(|message| message.contains("vue-ls server"))
        });
        let prefix = ide.data.join("language-servers/vue-ls");
        assert_eq!(runs(&ide), vec![prefix.clone()]);
        assert_eq!(ide.servers.executable("vue-ls"), Some(prefix.join("node_modules/.bin/vue-language-server")));
        assert!(messages.iter().any(|message| message.contains("vue-ls server")), "{messages:?}");
        assert!(ide.plugins.restart_required().is_empty(), "a new plug-in's servers work without a restart");
        assert!(ide.servers.declaration("vue-ls").is_some());

        let newer = plugin("vue", "Vue", "vue", &["0.3.0"]).versions;
        ide.source.catalog.lock().unwrap().plugins[0].versions.extend(newer);
        ide.plugins.refresh_catalog();
        let start = Instant::now();
        while PluginManager::installed(&*ide.plugins, "").plugins.iter().all(|plugin| plugin.update.is_none()) {
            assert!(start.elapsed() < Duration::from_secs(60), "no update offered");
            std::thread::sleep(Duration::from_millis(5));
        }
        let updated = ide.plugins.update("vue").unwrap();
        assert_eq!(updated.queued, ["vue"]);
        wait(&ide, |_, _| true);
        assert_eq!(runs(&ide).len(), 1, "the server installed is kept");

        ide.plugins.uninstall("vue").unwrap();
        assert!(!prefix.exists(), "the IDE's own copy goes with the plug-in");
        assert_eq!(ide.servers.source("vue-ls"), ServerSource::Missing);
    }

    #[test]
    fn a_plugin_adds_its_typescript_plugin_to_another_plugins_server_and_takes_it_back() {
        let ide = ide();
        let mut ts = server("ts-ls", "ts-ls");
        ts.extensions = vec!["ts".to_owned()];
        ide.servers.declare([ts]);
        let contributed = zen_workbench::language::servers::ServerPlugin {
            server: "ts-ls".to_owned(),
            name: "ts-svelte".to_owned(),
            package: "ts-svelte@0.3.0".to_owned(),
            languages: vec!["svelte".to_owned()],
            when: None,
        };
        ide.source.catalog.lock().unwrap().plugins[0].server_plugins = vec![contributed.clone()];
        catalog_ready(&ide.plugins);
        ide.plugins.install_plugin_by_id("vue").unwrap();
        let messages = wait(&ide, |ide, messages| {
            ide.servers.missing_plugins("ts-ls").is_empty()
                && messages.iter().any(|message| message.contains("ts-svelte"))
        });
        assert_eq!(ide.servers.server_plugins("ts-ls"), [contributed]);
        assert!(
            runs(&ide).contains(&ide.data.join("language-servers/ts-ls")),
            "into the server's folder: {messages:?}"
        );
        assert_eq!(ide.servers.source("ts-ls"), ServerSource::Missing, "the server itself is left to its banner");
        assert!(ide.data.join("language-servers/ts-ls/node_modules/ts-svelte/package.json").is_file());

        ide.plugins.uninstall("vue").unwrap();
        assert!(ide.servers.server_plugins("ts-ls").is_empty(), "uninstalled, its contribution goes");
    }

    #[test]
    fn the_editors_offer_installs_the_plugin_and_its_server_in_one_step() {
        let ide = ide();
        catalog_ready(&ide.plugins);
        let Some(Offer::Install { plugin, .. }) = ide.plugins.offer("App.vue") else { panic!("no offer") };
        ide.plugins.install_plugin(*plugin);
        wait(&ide, |ide, _| ide.servers.source("vue-ls") == ServerSource::Local);
        assert_eq!(runs(&ide).len(), 1);
    }

    #[test]
    fn a_server_already_on_path_is_kept_and_not_installed_again() {
        let ide = ide();
        let global = ide.data.join("global/vue-language-server");
        std::fs::create_dir_all(global.parent().unwrap()).unwrap();
        std::fs::write(&global, "").unwrap();
        ide.tools.on_path.lock().unwrap().insert("vue-ls".to_owned(), global.clone());
        catalog_ready(&ide.plugins);
        ide.plugins.install_plugin_by_id("vue").unwrap();
        let messages = wait(&ide, |_, _| true);
        assert!(runs(&ide).is_empty(), "{messages:?}");
        assert_eq!(ide.servers.source("vue-ls"), ServerSource::Path);
        assert_eq!(ide.servers.executable("vue-ls"), Some(global));
    }

    #[test]
    fn the_update_at_start_never_installs_a_server() {
        let ide = ide();
        *ide.tools.no_npm.lock().unwrap() = true;
        catalog_ready(&ide.plugins);
        ide.plugins.install_plugin_by_id("vue").unwrap();
        wait(&ide, |_, _| true);
        *ide.tools.no_npm.lock().unwrap() = false;
        let newer = plugin("vue", "Vue", "vue", &["0.3.0"]).versions;
        ide.source.catalog.lock().unwrap().plugins[0].versions.extend(newer);
        ide.plugins.refresh_catalog();
        let start = Instant::now();
        while PluginManager::installed(&*ide.plugins, "").plugins.iter().all(|plugin| plugin.update.is_none()) {
            assert!(start.elapsed() < Duration::from_secs(60), "no update offered");
            std::thread::sleep(Duration::from_millis(5));
        }
        ide.plugins.update_in_background();
        let start = Instant::now();
        while PluginManager::installed(&*ide.plugins, "").plugins[0].installed.as_deref() != Some("0.3.0") {
            assert!(start.elapsed() < Duration::from_secs(60), "the start update did not run");
            std::thread::sleep(Duration::from_millis(5));
        }
        wait(&ide, |_, _| true);
        assert!(runs(&ide).is_empty(), "updated at start, the server is left to its banner");
        assert_eq!(ide.servers.source("vue-ls"), ServerSource::Missing);
        assert!(ide.servers.installable("vue-ls"), "the banner can still install it");
    }

    #[test]
    fn without_npm_one_message_says_to_get_nodejs_and_the_server_stays_missing() {
        let ide = ide();
        *ide.tools.no_npm.lock().unwrap() = true;
        catalog_ready(&ide.plugins);
        ide.plugins.install_plugin_by_id("vue").unwrap();
        let messages = wait(&ide, |_, _| true);
        let node: Vec<&String> = messages.iter().filter(|message| message.contains("Node.js (npm)")).collect();
        assert_eq!(node.len(), 1, "{messages:?}");
        assert!(node[0].contains("https://nodejs.org/en/download"), "{messages:?}");
        assert_eq!(ide.servers.source("vue-ls"), ServerSource::Missing);
        assert!(ide.servers.install_state("vue-ls").is_none(), "nothing ran");
    }
}

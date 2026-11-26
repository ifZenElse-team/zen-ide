use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Ui, ViewportBuilder, ViewportCommand, ViewportId};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_theme::{ResolvedTheme, ThemeImporter, ThemeStore, VsCodeImporter, ZEN_DARK_ID};
use zen_workbench::Workbench;
use zen_workbench::keymaps::{self, KeymapStore, SharedKeymaps};
use zen_workbench::recent::{RecentProject, RecentProjects};
use zen_workbench::themes::{self, SharedThemes};

use super::settings::IdeSettings;
use super::tool_windows::{Bounds, SavedLayouts, ViewMode};
use super::updates::{UpdateOffer, Updater};
use super::view_modes::{Detached, ViewClass};
use super::welcome::{WelcomeSection, WelcomeView};
use super::workspace::{self, Workspace};
use super::{IdePalette, IdeProduct, IdeView};
use crate::plugins::Plugins;

const PROJECT_SIZE: [f32; 2] = [1280.0, 820.0];
const WELCOME_SIZE: [f32; 2] = [960.0, 650.0];
const EXIT_DELAY: f64 = 0.15;

fn merged(bound: serde_json::Value, given: Option<serde_json::Value>) -> serde_json::Value {
    match (bound, given) {
        (bound, None) => bound,
        (serde_json::Value::Object(mut bound), Some(serde_json::Value::Object(given))) => {
            bound.extend(given);
            serde_json::Value::Object(bound)
        }
        (_, Some(given)) => given,
    }
}

fn welcome_viewport() -> ViewportId {
    ViewportId::from_hash_of("zen-welcome-window")
}

pub(super) fn tool_viewport(project: &Path, id: &str, mode: ViewMode) -> ViewportId {
    ViewportId::from_hash_of(("zen-tool-window", project, id, mode.name()))
}

fn editor_viewport(project: &Path, group: zen_workbench::groups::GroupId) -> ViewportId {
    ViewportId::from_hash_of(("zen-editor-window", project, group))
}

pub(super) fn detached_builder(tool: &Detached, product_id: &str, ide_focused: bool) -> ViewportBuilder {
    let size = tool.bounds.map_or(tool.size, |bounds| egui::vec2(bounds.width, bounds.height));
    let mut builder = ViewportBuilder::default()
        .with_title(tool.title.clone())
        .with_app_id(format!("space.ifzenelse.{product_id}"))
        .with_inner_size(size)
        .with_min_inner_size(Bounds::MIN);
    if let Some(bounds) = tool.bounds {
        builder = builder.with_position([bounds.x, bounds.y]);
    }
    match tool.mode {
        ViewMode::Float => builder
            .with_window_level(if ide_focused { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal })
            .with_taskbar(false)
            .with_minimize_button(false)
            .with_window_type(egui::X11WindowType::Utility),
        _ => builder.with_window_level(egui::WindowLevel::Normal).with_taskbar(true),
    }
}

const CONSOLE_IDLE_SECONDS: f64 = 1.0;

fn exit_clock() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Request {
    Open(PathBuf),
    OpenWithoutAsking(PathBuf),
    OpenWindow(PathBuf),
    AskTrust(PathBuf),
    Focus(PathBuf),
    Close(PathBuf),
    MoveTab(PathBuf, usize),
    RemoveRecent(PathBuf),
    Settings(Box<IdeSettings>),
    Update,
    Restart,
    UpdateInstalled(String),
    RestartAsking(RestartAsk, Option<zen_workbench::maintenance::InvalidateParams>),
    DismissUpdate(String),
    Exit { confirmed: bool },
    Autosave,
    Layouts(Box<SavedLayouts>),
    Welcome(Option<WelcomeSection>),
}

impl Request {
    pub(super) fn open_created(by_agent: bool, path: PathBuf) -> Self {
        if by_agent { Self::OpenWithoutAsking(path) } else { Self::Open(path) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RestartAsk {
    Person,
    Always,
    IfWorkRuns,
    Never,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Overview {
    pub(super) recent: Vec<RecentProject>,
    pub(super) open: Vec<PathBuf>,
    pub(super) automation: Option<String>,
    pub(super) settings: IdeSettings,
    pub(super) update: Option<UpdateOffer>,
    pub(super) tabs: Vec<PathBuf>,
    pub(super) active_tab: Option<PathBuf>,
    pub(super) layouts: SavedLayouts,
    pub(super) trust_question: bool,
}

type KeptFromAgents = Arc<Mutex<Vec<(Option<PathBuf>, Vec<String>)>>>;

const WAITING_STARTS_CHECK: std::time::Duration = std::time::Duration::from_millis(250);

type HandedOver = (Vec<PathBuf>, Option<zen_workbench::handoff::Waiting>);

struct WaitingStart {
    files: Vec<PathBuf>,
    seen: Vec<PathBuf>,
    _held: zen_workbench::handoff::Waiting,
}

struct ProjectWindow {
    session: Session,
    view: IdeView,
}

enum WindowCall {
    Action { action: String, command: String, params: serde_json::Value },
    Keys(super::agent::KeyPress),
}

#[derive(Clone)]
struct Slot {
    path: PathBuf,
    viewport: ViewportId,
    tabbed: bool,
    window: Arc<Mutex<ProjectWindow>>,
}

fn session_overview(registry: &Registry, session: &mut Session, project: &Path) -> serde_json::Value {
    let mut ask = |command: &str| {
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, serde_json::json!({})))
            .unwrap_or_default()
    };
    let repository = ask("vcs.repository.info");
    let changes = ask("vcs.status.list")["changes"].as_array().map_or(0, Vec::len);
    let servers: serde_json::Map<String, serde_json::Value> = ask("language.servers.status")
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, status)| (id.clone(), status["state"].clone()))
        .collect();
    let index = ask("search.index.status");
    serde_json::json!({
        "project": project,
        "branch": repository["branch"],
        "upstream": repository["upstream"],
        "ahead": repository["ahead"],
        "behind": repository["behind"],
        "changes": changes,
        "languageServers": servers,
        "index": { "ready": index["ready"], "indexed": index["indexed"], "total": index["total"] },
    })
}

fn move_tabbed(windows: &mut Vec<Slot>, path: &Path, to: usize) -> bool {
    let Some(from) = windows.iter().position(|slot| slot.tabbed && slot.path == path) else { return false };
    let slot = windows.remove(from);
    let tabs: Vec<usize> = windows.iter().enumerate().filter(|(_, slot)| slot.tabbed).map(|(at, _)| at).collect();
    let at = match tabs.get(to) {
        Some(at) => *at,
        None => tabs.last().map_or(windows.len(), |last| last + 1),
    };
    windows.insert(at, slot);
    true
}

impl Slot {
    fn drawn_in(&self) -> ViewportId {
        if self.tabbed { ViewportId::ROOT } else { self.viewport }
    }
}

struct State {
    windows: Vec<Slot>,
    active_tab: Option<PathBuf>,
    recent: RecentProjects,
    recent_file: Option<PathBuf>,
    trust_question: bool,
    requests: Vec<Request>,
    headless: bool,
}

struct Shared {
    product: Arc<IdeProduct>,
    registry: Arc<Registry>,
    plugins: Arc<Plugins>,
    state: Mutex<State>,
    automation_url: Mutex<Option<String>>,
    settings: Mutex<IdeSettings>,
    settings_file: Option<PathBuf>,
    help: super::productivity::Help,
    layouts: Mutex<SavedLayouts>,
    layouts_file: Option<PathBuf>,
    update_offer: Mutex<Option<UpdateOffer>>,
    toolchain: Option<Arc<dyn zen_modules::LanguageToolchain>>,
    window_states: Vec<zen_modules::WindowState>,
    language_servers: zen_workbench::language::servers::LanguageServers,
    local_history: zen_workbench::local_history::LocalHistory,
    search_history: zen_workbench::search_everywhere::SearchHistory,
    find_history: zen_workbench::find_history::FindHistory,
    templates: zen_workbench::templates::LiveTemplates,
    file_templates: zen_workbench::file_templates::FileTemplates,
    new_projects: zen_workbench::new_project::NewProjects,
    macros: zen_workbench::macros::Macros,
    external_tools: zen_workbench::external_tools::ExternalTools,
    file_watchers: zen_workbench::file_watchers::GlobalWatchers,
    clipboard_history: zen_workbench::clipboard_history::ClipboardHistory,
    file_types: zen_workbench::file_types::FileTypeStore,
    caffeine: zen_workbench::caffeine::Caffeine,
    theme: Mutex<ResolvedTheme>,
    themes: SharedThemes,
    keymaps: SharedKeymaps,
    exiting: std::sync::atomic::AtomicBool,
    handed_over: Mutex<Vec<HandedOver>>,
    system_quit: &'static zen_macos::QuitGate,
    autosave: Mutex<super::autosave::AutosaveClock>,
    consoles: Mutex<super::autosave::AutosaveClock>,
    leaving: Mutex<super::autosave::Leaving>,
}

impl Shared {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn theme(&self) -> ResolvedTheme {
        self.theme.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    fn palette(&self) -> IdePalette {
        IdePalette::from_theme(&self.theme())
    }

    fn is_exiting(&self) -> bool {
        self.exiting.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn stop_work(&self) {
        self.caffeine.shutdown();
        let windows = self.state().windows.clone();
        let mut runs: Vec<zen_workbench::run::Run> = Vec::new();
        let mut programs = Vec::new();
        for slot in &windows {
            let window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            runs.extend(zen_workbench::exit::running_runs(&window.session));
            programs.extend(zen_workbench::exit::running_programs(&window.session));
        }
        zen_workbench::exit::stop_work(&runs, &programs, zen_run::process::STOP_GRACE);
    }

    fn overview(&self) -> Overview {
        let state = self.state();
        let open = state.windows.iter().map(|slot| slot.path.clone());
        let tabs: Vec<PathBuf> =
            state.windows.iter().filter(|slot| slot.tabbed).map(|slot| slot.path.clone()).collect();
        let active_tab = state.active_tab.clone().filter(|path| tabs.contains(path)).or_else(|| tabs.first().cloned());
        let automation = self.automation_url.lock().ok().and_then(|url| url.clone());
        let settings = self.settings.lock().map(|settings| settings.clone()).unwrap_or_default();
        Overview {
            recent: state.recent.projects().to_vec(),
            open: open.collect(),
            automation,
            settings,
            update: self.update_offer.lock().ok().and_then(|offer| offer.clone()),
            tabs,
            active_tab,
            layouts: self.layouts.lock().map(|layouts| layouts.clone()).unwrap_or_default(),
            trust_question: state.trust_question,
        }
    }

    fn observe_input(&self, ctx: &egui::Context) {
        let (input, left) = ctx.input(|input| {
            (input.events.iter().any(super::autosave::is_input), super::autosave::window_left(&input.events))
        });
        if input {
            self.autosave.lock().unwrap_or_else(std::sync::PoisonError::into_inner).input(exit_clock());
            self.consoles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).input(exit_clock());
            let idle = self.settings.lock().ok().filter(|settings| settings.autosave_on_idle);
            if let Some(seconds) = idle.map(|settings| settings.autosave_idle_seconds) {
                ctx.request_repaint_after_for(std::time::Duration::from_secs(u64::from(seconds)), ViewportId::ROOT);
            }
        }
        if left {
            self.leaving.lock().unwrap_or_else(std::sync::PoisonError::into_inner).left();
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }

    fn submit(&self, ctx: &egui::Context, requests: Vec<Request>) {
        if requests.is_empty() {
            return;
        }
        self.state().requests.extend(requests);
        ctx.request_repaint_of(ViewportId::ROOT);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootMode {
    Welcome,
    Project,
}

pub struct IdeApp {
    shared: Arc<Shared>,
    welcome: WelcomeView,
    root_mode: Option<RootMode>,
    root_title: String,
    paths: Vec<PathBuf>,
    tool_viewports: Vec<ViewportId>,
    ui_zoom: Option<(f32, f32)>,
    automation: Option<(zen_automation::Server, std::sync::mpsc::Receiver<zen_automation::Call>)>,
    mcp_stats: Arc<zen_automation::stats::Stats>,
    kept_from_agents: KeptFromAgents,
    no_restore: bool,
    automation_port: Option<u16>,
    automation_for_run: Option<u16>,
    automation_file: Option<PathBuf>,
    updater: Option<Updater>,
    workspace: Workspace,
    workspace_file: Option<PathBuf>,
    workspace_saved: f64,
    splash: super::splash::Splash,
    theme_applied: (u64, String),
    keymap_applied: String,
    file_types_seen: u64,
    pending_notice: Option<String>,
    settings_stuck: bool,
    update_watch: Option<UpdateWatch>,
    internal_errors_watched: bool,
    welcome_session: Session,
    trust_questions: std::collections::VecDeque<super::trust_dialog::Question>,
    trust_answered: std::collections::BTreeSet<PathBuf>,
    asks_trust: bool,
    trust_env: zen_workbench::build_tools::BuildEnv,
    areas: zen_modules::SharedAreas,
    areas_dialog: Option<super::areas_dialog::AreasDialog>,
    areas_checked: bool,
    welcome_exit: bool,
    welcome_window: bool,
    asking_where: Option<PathBuf>,
    kept_notes: Option<Vec<zen_toolbox::ReleaseNote>>,
    start_help_done: bool,
    exit_at: Option<f64>,
    system_quit_hooked: bool,
    runs_stopped: bool,
    headless: bool,
    finished: bool,
    ending: bool,
    agent_seen: Option<std::time::Instant>,
    leases_counted: Option<(std::time::Instant, usize)>,
    caffeine_woken: bool,
    caffeine_was_on: bool,
    waiting_runs: Vec<WaitingRun>,
    waiting_lookups: Vec<WaitingLookup>,
    launchers_wanted: bool,
    launchers: Option<zen_workbench::handoff::Listener>,
    waiting_starts: Vec<WaitingStart>,
}

struct WaitingLookup {
    arguments: serde_json::Value,
    wait: zen_workbench::language::SearchWait,
    reply: super::agent::Reply,
}

struct WaitingRun {
    window: Arc<Mutex<ProjectWindow>>,
    run_id: u64,
    until: std::time::Instant,
    reply: super::agent::Reply,
}

fn settings_data(data: Option<&Path>) -> Option<PathBuf> {
    let data = data?;
    if cfg!(test) && !data.starts_with(std::env::temp_dir()) {
        return None;
    }
    Some(data.to_owned())
}

#[cfg(test)]
thread_local! {
    static RESTARTS_ASKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static RESTART_REFUSED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn restart(shared: &Shared, bundle: Option<&Path>) -> std::io::Result<()> {
    if cfg!(test) {
        #[cfg(test)]
        {
            RESTARTS_ASKED.with(|asked| asked.set(asked.get() + 1));
            if RESTART_REFUSED.with(std::cell::Cell::get) {
                return Err(std::io::Error::other("the shell cannot start"));
            }
        }
        return Ok(());
    }
    let (start, program) = match bundle {
        Some(bundle) => ("open \"$0\"", bundle.to_owned()),
        None => ("exec \"$0\"", std::env::current_exe()?),
    };
    let script = format!("while kill -0 {} 2>/dev/null; do sleep 0.2; done; {start}", std::process::id());
    std::process::Command::new("/bin/sh")
        .args(["-c", &script])
        .arg(&program)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    if let Some(file) = shared.settings_file.as_ref().and_then(|file| Some(file.parent()?.join("automation.json"))) {
        let _ = std::fs::remove_file(file);
    }
    shared.system_quit.allow();
    shared.stop_work();
    shared.local_history.flush_within(zen_workbench::local_history::QUIT_WAIT);
    std::process::exit(0)
}

fn newly_installed(installed: &str, running: &str, asked: &mut Option<String>) -> bool {
    if installed == running || asked.as_deref() == Some(installed) {
        return false;
    }
    *asked = Some(installed.to_owned());
    true
}

struct UpdateWatch {
    info_plist: PathBuf,
    bundle: PathBuf,
}

impl UpdateWatch {
    const INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

    fn spawn(self, shared: Arc<Shared>, ctx: egui::Context) {
        let _ = std::thread::Builder::new().name("zen-update-watch".to_owned()).spawn(move || {
            let mut asked = None;
            loop {
                std::thread::sleep(Self::INTERVAL);
                let Some(installed) = self.installed_version() else { continue };
                if newly_installed(&installed, shared.product.version, &mut asked) {
                    shared.submit(&ctx, vec![Request::UpdateInstalled(installed)]);
                }
            }
        });
    }

    fn for_this_process() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let contents = exe.parent()?.parent()?;
        let bundle = contents.parent()?;
        if exe.parent()?.file_name()? != "MacOS" || bundle.extension()? != "app" {
            return None;
        }
        Some(Self { info_plist: contents.join("Info.plist"), bundle: bundle.to_owned() })
    }

    fn installed_version(&self) -> Option<String> {
        let plist = std::fs::read_to_string(&self.info_plist).ok()?;
        let key = plist.find("<key>CFBundleShortVersionString</key>")?;
        let rest = &plist[key..];
        let start = rest.find("<string>")? + "<string>".len();
        let end = rest[start..].find("</string>")?;
        Some(rest[start..start + end].trim().to_owned())
    }
}

impl Drop for IdeApp {
    fn drop(&mut self) {
        self.shared.caffeine.shutdown();
        self.shared.local_history.flush_within(zen_workbench::local_history::QUIT_WAIT);
    }
}

impl IdeApp {
    pub fn new(product: IdeProduct, registry: Registry, recent_file: Option<PathBuf>, open: Vec<PathBuf>) -> Self {
        let mut app = Self::build(product, registry, recent_file, open, false);
        app.accept_launchers();
        app
    }

    pub fn in_safe_mode(
        product: IdeProduct,
        registry: Registry,
        recent_file: Option<PathBuf>,
        open: Vec<PathBuf>,
    ) -> Self {
        let mut app = Self::build(product, registry, recent_file, Vec::new(), false);
        app.asks_trust = false;
        app.trust_questions.clear();
        for path in open {
            app.open(None, &path);
        }
        app
    }

    pub fn accept_launchers(&mut self) {
        self.launchers_wanted = true;
    }

    pub fn launcher_socket(&self) -> Option<&Path> {
        self.launchers.as_ref().map(zen_workbench::handoff::Listener::path)
    }

    pub(super) fn build(
        product: IdeProduct,
        registry: Registry,
        recent_file: Option<PathBuf>,
        open: Vec<PathBuf>,
        headless: bool,
    ) -> Self {
        Self::build_restoring(product, registry, recent_file, open, headless, true)
    }

    pub(super) fn build_restoring(
        mut product: IdeProduct,
        mut registry: Registry,
        recent_file: Option<PathBuf>,
        open: Vec<PathBuf>,
        headless: bool,
        restore: bool,
    ) -> Self {
        #[cfg(not(test))]
        zen_workbench::trust::install_git_gate(zen_workbench::build_tools::BuildEnv::global());
        if !cfg!(test) {
            zen_workbench::internal_errors::catch_panics();
        }
        let data = recent_file.as_deref().and_then(Path::parent).map(Path::to_owned);
        mark_areas(&mut product);
        let file_templates = zen_workbench::file_templates::FileTemplates::load(data.clone(), product.name);
        let new_projects = zen_workbench::new_project::NewProjects::default();
        let (toolchain, mut window_states, modules) =
            start_modules(&mut product, &mut registry, data.as_deref(), &file_templates, &new_projects);
        let plugins = Plugins::new(product.plugins.clone());
        let style_plugins = Arc::clone(&plugins);
        window_states.push(Arc::new(move |session: &mut Session| {
            let style_plugins = Arc::clone(&style_plugins);
            let styles = session.state::<zen_workbench::code_style_options::CodeStyles>().cloned().unwrap_or_default();
            session.set_state(styles.with_plugins(Arc::new(move || {
                style_plugins
                    .entries()
                    .into_iter()
                    .filter(|entry| entry.enabled)
                    .flat_map(|entry| entry.plugin.code_styles.clone())
                    .collect()
            })));
        }));
        let generator_plugins = Arc::clone(&plugins);
        let generator_client = format!("zen-ide/{}", if product.version.is_empty() { "0" } else { product.version });
        new_projects.set_plugins(Arc::new(move |environment| {
            generator_plugins.project_generators(environment, &generator_client)
        }));
        let build_plugins = Arc::clone(&plugins);
        new_projects.set_builds(Arc::new(move || build_plugins.module_builds()));
        if let Some(data) = &data {
            new_projects.set_data_dir(data.clone());
        }
        let schema_plugins = Arc::clone(&plugins);
        zen_workbench::schemas::set_plugins(Some(Arc::new(move || schema_plugins.features())));
        let run_plugins = Arc::clone(&plugins);
        zen_workbench::plugin_runs::set_plugins(Some(Arc::new(move || run_plugins.features())));
        let jvm_plugins = Arc::clone(&plugins);
        zen_workbench::build_tools::BuildEnv::global().set_jvm_reader(Some(Arc::new(move |path, text| {
            zen_workbench::plugins::jvm_file(&jvm_plugins.features(), path, text)
        })));
        let template_plugins = Arc::clone(&plugins);
        file_templates.set_plugins(Arc::new(move || {
            template_plugins
                .entries()
                .into_iter()
                .filter(|entry| entry.enabled)
                .map(|entry| {
                    let declared = &entry.plugin.file_templates;
                    zen_workbench::file_templates::Declared::of_plugin(
                        &entry.plugin.id,
                        &declared.templates,
                        &declared.variables,
                    )
                })
                .collect()
        }));
        plugins.update_in_background();
        let areas: zen_modules::SharedAreas = Arc::new(Mutex::new(zen_modules::Areas::load(
            product.name,
            product.areas.iter().map(|&id| id.to_owned()).collect(),
            modules,
            Some(Arc::clone(&plugins) as Arc<dyn zen_modules::AreaPlugins>),
            data.as_ref().map(|data| data.join("areas.json")),
        )));
        if let Err(error) = zen_modules::register_areas(&mut registry, &areas) {
            #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
            {
                eprintln!("cannot register the area commands: {error}");
            }
        }
        let launcher = zen_workbench::launcher::LauncherTarget::this_process(product.id);
        if let Err(error) = zen_workbench::launcher::register(&mut registry, launcher) {
            #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
            {
                eprintln!("cannot register the command-line launcher commands: {error}");
            }
        }
        let product = Arc::new(product);
        let recent = recent_file.as_deref().map(RecentProjects::load).unwrap_or_default();
        let to_open = if open.is_empty() && !headless { recent.to_reopen() } else { open };
        let settings_file = recent_file.as_ref().and_then(|file| Some(file.parent()?.join("settings.json")));
        let help =
            super::productivity::Help::new(recent_file.as_ref().and_then(|file| Some(file.parent()?.to_owned())));
        super::keymap_page::set_product(product.name);
        let (settings, settings_damage) = settings_file
            .as_deref()
            .map(|file| IdeSettings::load_checked(file, product.mcp_port))
            .unwrap_or((IdeSettings { mcp_port: product.mcp_port, ..IdeSettings::default() }, None));
        super::customization::set_current(&settings.customization);
        let automation_port = settings.mcp_enabled.then_some(settings.mcp_port);
        apply_interface_language(&settings);
        zen_workbench::network::apply(&settings.proxy);
        if let Some(data) = recent_file.as_deref().and_then(Path::parent) {
            zen_workbench::network::certificates::open_store(data);
        }
        zen_workbench::network::certificates::apply(settings.accept_untrusted_certificates);
        let language_servers = language_servers(&product, &plugins, recent_file.as_deref(), toolchain.as_ref());
        plugins.set_language_servers(language_servers.clone());
        if let Some(data) = recent_file.as_ref().and_then(|file| file.parent()).filter(|_| !headless) {
            zen_workbench::maintenance::apply_scheduled_invalidation(data);
        }
        let local_history = recent_file
            .as_ref()
            .and_then(|file| file.parent())
            .map(|data| {
                zen_workbench::local_history::LocalHistory::open(data.join("local-history"), Default::default())
            })
            .unwrap_or_default();
        let templates = zen_workbench::templates::LiveTemplates::load(
            recent_file.as_ref().and_then(|file| Some(file.parent()?.join("templates.json"))),
        );
        for builtin in &product.templates {
            templates.declare_language(builtin.clone());
        }
        plugins.set_live_templates(templates.clone());
        let source_plugins = Arc::clone(&plugins);
        let highlighters = product.highlighters.clone();
        templates.set_highlighters(Arc::new(move |name: &str| source_plugins.highlighter(&highlighters, name)));
        let file_types = zen_workbench::file_types::FileTypeStore::load(
            data.as_ref().map(|data| data.join(zen_workbench::file_types::FILE_NAME)),
        );
        let mut welcome_session = Session::new();
        welcome_session.set_state(templates.clone());
        welcome_session.set_state(file_templates.clone());
        welcome_session.set_state(file_types.clone());
        welcome_session.set_state(new_projects.clone());
        settings.apply_to_session(welcome_session.settings_mut());
        #[cfg(test)]
        let mut caffeine = zen_workbench::caffeine::Caffeine::with(
            Box::new(zen_workbench::caffeine::keeper::FakeKeeper::default()),
            Arc::new(zen_workbench::caffeine::power::FakePower::new("Now drawing from 'AC Power'\n")),
        );
        #[cfg(not(test))]
        let mut caffeine = zen_workbench::caffeine::Caffeine::default();
        caffeine.set_who(product.name);
        caffeine.configure(settings.caffeine.clone(), std::time::Instant::now());
        if settings.caffeine.start_on_launch {
            let _ = caffeine.start(&zen_workbench::caffeine::StartParams::default(), std::time::Instant::now());
        }
        welcome_session.set_state(caffeine.clone());
        zen_workbench::internal_errors::InternalErrors::global()
            .set_product(format!("{} {}", product.name, product.version));
        let (themes, pending_notice) = theme_store(recent_file.as_deref(), &settings);
        if let Err(error) = themes::register(&mut registry, &themes) {
            #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
            {
                eprintln!("cannot register the theme commands: {error}");
            }
        }
        let (keymaps, keymap_notice) = keymap_store(recent_file.as_deref(), settings.keymap.as_deref());
        let macros = zen_workbench::macros::Macros::load(data.as_ref().map(|data| data.join("macros.json")));
        macros.set_keymaps(Arc::clone(&keymaps));
        let external_tools = zen_workbench::external_tools::ExternalTools::load(
            data.as_ref().map(|data| data.join(zen_workbench::external_tools::FILE_NAME)),
        );
        external_tools.connect_keymaps(Arc::clone(&keymaps));
        super::external_tools::load(external_tools.clone());
        let file_watchers = zen_workbench::file_watchers::GlobalWatchers::load(
            data.as_ref().map(|data| data.join(zen_workbench::file_watchers::FILE_NAME)),
        );
        if let Err(error) = keymaps::register(&mut registry, &keymaps) {
            #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
            {
                eprintln!("cannot register the keymap commands: {error}");
            }
        }
        crate::keymap::load(Arc::clone(&keymaps));
        let keymap_applied = keymaps::lock(&keymaps).current().to_owned();
        let settings_stuck = settings_damage.as_ref().is_some_and(|damage| damage.kept.is_none());
        let pending_notice = [
            settings_damage.map(|damage| damage.notice()),
            pending_notice,
            keymap_notice,
            templates.take_notice(),
            macros.take_notice(),
            external_tools.take_notice(),
            file_watchers.take_notice(),
        ]
        .into_iter()
        .flatten()
        .reduce(|first, second| format!("{first}\n{second}"));
        let _ = crate::fonts::register(&mut registry);
        let _ = super::customization::register(&mut registry);
        let managed: zen_workbench::plugin_manager::SharedPlugins = Arc::clone(&plugins) as _;
        if let Err(error) = zen_workbench::plugin_manager::register(&mut registry, &managed) {
            #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
            {
                eprintln!("cannot register the plug-in commands: {error}");
            }
        }
        let settings_target = zen_workbench::ide_settings::SettingsTarget {
            data: settings_data(data.as_deref()),
            product: product.name.to_owned(),
            version: product.version.to_owned(),
            keymaps: Some(Arc::clone(&keymaps)),
            themes: Some(Arc::clone(&themes)),
            plugins: Some(Arc::clone(&managed)),
        };
        if let Err(error) = zen_workbench::ide_settings::register(&mut registry, settings_target) {
            #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
            {
                eprintln!("cannot register the IDE settings commands: {error}");
            }
        }
        let (theme, theme_applied) = {
            let store = themes::lock(&themes);
            (store.resolve_current().theme, (store.generation(), store.current().to_owned()))
        };
        let layouts_file = data.as_deref().map(|data| data.join("window-layouts.json"));
        let search_history = data
            .as_deref()
            .map(|data| zen_workbench::search_everywhere::SearchHistory::load(&data.join("search-everywhere.json")))
            .unwrap_or_default();
        let find_history = data
            .as_deref()
            .map(|data| zen_workbench::find_history::FindHistory::load(&data.join("find-history.json")))
            .unwrap_or_default();
        let mcp_stats = Arc::new(zen_automation::stats::Stats::default());
        let counted = Arc::clone(&mcp_stats);
        let report: zen_workbench::mcp_stats::Report =
            Arc::new(move |reset| serde_json::to_value(counted.report(reset)).unwrap_or_default());
        if let Err(error) = registry.register(zen_workbench::mcp_stats::McpSessionStats(report)) {
            #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
            {
                eprintln!("cannot register mcp.session.stats: {error}");
            }
        }
        let shared = Arc::new(Shared {
            product: Arc::clone(&product),
            registry: Arc::new(registry),
            search_history,
            find_history,
            plugins: Arc::clone(&plugins),
            new_projects,
            state: Mutex::new(State {
                windows: Vec::new(),
                active_tab: None,
                recent,
                recent_file,
                trust_question: false,
                requests: Vec::new(),
                headless,
            }),
            automation_url: Mutex::new(None),
            settings: Mutex::new(settings),
            layouts: Mutex::new(layouts_file.as_deref().map(SavedLayouts::load).unwrap_or_default()),
            layouts_file,
            settings_file,
            help,
            update_offer: Mutex::new(None),
            toolchain,
            window_states,
            language_servers,
            local_history,
            templates,
            file_templates,
            macros,
            external_tools,
            file_watchers,
            clipboard_history: zen_workbench::clipboard_history::ClipboardHistory::default(),
            file_types,
            caffeine,
            theme: Mutex::new(theme),
            themes,
            keymaps,
            exiting: std::sync::atomic::AtomicBool::new(false),
            handed_over: Mutex::new(Vec::new()),
            system_quit: if cfg!(test) {
                Box::leak(Box::new(zen_macos::QuitGate::new()))
            } else {
                zen_macos::quit_gate()
            },
            autosave: Mutex::new(super::autosave::AutosaveClock::default()),
            consoles: Mutex::new(super::autosave::AutosaveClock::default()),
            leaving: Mutex::new(super::autosave::Leaving::default()),
        });
        let automation_file =
            shared.state().recent_file.as_ref().and_then(|file| Some(file.parent()?.join("automation.json")));
        let update_watch = UpdateWatch::for_this_process().filter(|_| !headless);
        let updater = update_watch.as_ref().and_then(|_| Updater::new(product.id));
        let weak = Arc::downgrade(&shared);
        crate::capture::before_quit(move || {
            if let Some(shared) = weak.upgrade() {
                shared.stop_work();
            }
        });
        let workspace_file =
            shared.state().recent_file.as_ref().and_then(|file| Some(file.parent()?.join("workspace.json")));
        let workspace = workspace_file.as_deref().map(Workspace::load).unwrap_or_default();
        let mut welcome = WelcomeView::new(product);
        welcome.set_palette(shared.palette());
        let mut app = Self {
            welcome,
            theme_applied,
            settings_stuck,
            update_watch,
            internal_errors_watched: false,
            keymap_applied,
            file_types_seen: zen_vfs::file_types::generation(),
            pending_notice,
            welcome_session,
            trust_questions: std::collections::VecDeque::new(),
            trust_answered: std::collections::BTreeSet::new(),
            asks_trust: !headless && cfg!(not(test)),
            #[cfg(not(test))]
            trust_env: zen_workbench::build_tools::BuildEnv::global(),
            #[cfg(test)]
            trust_env: super::trusting_everything(),
            areas,
            areas_dialog: None,
            areas_checked: false,
            updater,
            workspace,
            workspace_file,
            workspace_saved: 0.0,
            splash: super::splash::Splash::default(),
            automation: None,
            mcp_stats,
            kept_from_agents: Arc::default(),
            no_restore: !restore,
            automation_port,
            automation_for_run: None,
            automation_file,
            shared,
            root_mode: None,
            root_title: String::new(),
            paths: Vec::new(),
            tool_viewports: Vec::new(),
            ui_zoom: None,
            welcome_exit: false,
            welcome_window: false,
            asking_where: None,
            kept_notes: None,
            start_help_done: false,
            exit_at: None,
            system_quit_hooked: false,
            runs_stopped: false,
            headless,
            finished: false,
            ending: false,
            agent_seen: None,
            leases_counted: None,
            caffeine_woken: false,
            caffeine_was_on: false,
            waiting_runs: Vec::new(),
            waiting_lookups: Vec::new(),
            launchers_wanted: false,
            launchers: None,
            waiting_starts: Vec::new(),
        };
        for path in to_open {
            app.open(None, &path);
        }
        if let Some(active) = app.workspace.active_tab.clone() {
            let mut state = app.shared.state();
            if state.windows.iter().any(|slot| slot.tabbed && slot.path == active) {
                state.active_tab = Some(active);
            }
        }
        app.splash = if app.shared.state().windows.is_empty() {
            super::splash::Splash::finished()
        } else {
            let window = &app.workspace.window;
            super::splash::Splash::starting(super::splash::Restore {
                maximized: window.maximized,
                fullscreen: window.fullscreen,
            })
        };
        app
    }

    pub fn enable_automation(&mut self, port: u16) {
        self.automation_port = Some(port);
        self.automation_for_run = Some(port);
    }

    pub fn initial_viewport(&self, builder: ViewportBuilder) -> ViewportBuilder {
        let builder = builder.with_inner_size(self.initial_size());
        if self.shared.state().windows.is_empty() {
            return builder;
        }
        let window = &self.workspace.window;
        let mut builder = builder;
        if window.fullscreen
            && let Some(screen) = window.screen
        {
            return builder.with_inner_size(screen).with_position([0.0, 0.0]);
        }
        if let Some([width, height]) = window.size {
            builder = builder.with_inner_size([width.max(640.0), height.max(480.0)]);
        }
        if let Some(position) = window.position {
            builder = builder.with_position(position);
        }
        builder
    }

    fn remember_workspace(&mut self, ctx: &egui::Context, now: bool) {
        let time = ctx.input(|input| input.time);
        if !now && time - self.workspace_saved < 1.0 {
            return;
        }
        self.workspace_saved = time;
        let Some(file) = self.workspace_file.clone() else { return };
        let mut workspace = self.workspace.clone();
        let (windows, active_tab) = {
            let state = self.shared.state();
            (state.windows.clone(), state.active_tab.clone())
        };
        let mut by_mode = false;
        for slot in &windows {
            if let Ok(mut window) = slot.window.try_lock() {
                let window = &mut *window;
                by_mode |= slot.tabbed && window.view.appearance.full_screen_by_mode();
                let left = window.session.state_mut::<zen_workbench::workspaces::Workspaces>().take_left();
                for (project, saved) in left {
                    workspace.projects.entry(project).or_default().workspaces = saved;
                }
                let project = window.session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
                let project = project.unwrap_or_else(|| slot.path.clone());
                let layout = workspace::capture(&window.view, &mut window.session, &project);
                workspace.projects.insert(project, layout);
                workspace::capture_repository(&window.session, &mut workspace.repositories);
            }
        }
        workspace.active_tab = active_tab.or(workspace.active_tab);
        if self.root_mode == Some(RootMode::Project) && !self.splash.is_active() && !by_mode {
            let info = ctx.input(|input| input.viewport().clone());
            let fullscreen = info.fullscreen.unwrap_or(false);
            let maximized = info.maximized.unwrap_or(false);
            workspace.window.fullscreen = fullscreen;
            workspace.window.maximized = maximized;
            if fullscreen && let Some(size) = info.monitor_size {
                workspace.window.screen = Some([size.x, size.y]);
            }
            if !fullscreen && !maximized {
                if let Some(rect) = info.outer_rect {
                    workspace.window.position = Some([rect.min.x, rect.min.y]);
                }
                if let Some(rect) = info.inner_rect {
                    workspace.window.size = Some([rect.width(), rect.height()]);
                }
            }
        }
        if workspace != self.workspace {
            workspace.save(&file);
            self.workspace = workspace;
        }
    }

    fn notice(&mut self, notice: String) {
        self.pending_notice = Some(match self.pending_notice.take() {
            Some(earlier) => format!("{earlier}\n{notice}"),
            None => notice,
        });
    }

    fn save_settings(&mut self, settings: &IdeSettings) {
        let Some(file) = self.shared.settings_file.clone() else { return };
        let file_name = file.display().to_string();
        if self.settings_stuck {
            self.notice(t!("ide-settings-not-saved-damaged", file = file_name));
        } else if let Err(error) = settings.save(&file) {
            self.notice(t!("ide-settings-not-saved", file = file_name, detail = error.to_string()));
        }
    }

    fn help_on_start(&mut self) {
        if self.start_help_done {
            return;
        }
        let Some(slot) = self.shared.state().windows.first().cloned() else { return };
        self.start_help_done = true;
        let (wanted, tips) = self
            .shared
            .settings
            .lock()
            .map_or((false, false), |settings| (settings.whats_new_on_update, settings.show_tips_on_startup));
        let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let ProjectWindow { session, view } = &mut *window;
        if tips {
            view.open_tip(&serde_json::json!({}));
        }
        let Some(folder) = &self.shared.help.folder else { return };
        let show = super::whats_new::after_start(folder, self.shared.product.version, wanted);
        if show == super::whats_new::AfterStart::Show {
            let _ = view.open_whats_new(&self.shared.registry, session);
        }
    }

    fn follow_customization_commands(&self) {
        let windows = self.shared.state().windows.clone();
        for slot in &windows {
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(changed) = window.session.state_mut::<super::customization::Changed>().0.take() {
                let mut settings = self.shared.overview().settings;
                settings.customization = changed;
                self.shared.state().requests.push(Request::Settings(Box::new(settings)));
            }
        }
    }

    fn updates(&mut self, ctx: &egui::Context) {
        let Some(updater) = &mut self.updater else { return };
        updater.tick(ctx.input(|input| input.time));
        let notes = updater.notes().filter(|notes| !notes.is_empty() && self.kept_notes.as_ref() != Some(notes));
        if let (Some(notes), Some(folder)) = (notes, &self.shared.help.folder) {
            super::whats_new::keep_notes(folder, &notes);
            self.kept_notes = Some(notes);
        }
        let offer = updater.offer();
        if offer.as_ref().is_some_and(|offer| offer.progress.is_some()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(400));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(60));
        }
        if let Ok(mut slot) = self.shared.update_offer.lock() {
            *slot = offer;
        }
    }

    fn apply_settings(&mut self, ctx: &egui::Context, mut settings: IdeSettings) {
        if let Ok(current) = self.shared.settings.lock() {
            settings.theme.clone_from(&current.theme);
            settings.theme_follow_system = current.theme_follow_system;
            settings.theme_dark.clone_from(&current.theme_dark);
            settings.theme_light.clone_from(&current.theme_light);
            settings.keymap.clone_from(&current.keymap);
        }
        self.save_settings(&settings);
        super::customization::set_current(&settings.customization);
        let wanted = self.automation_for_run.or(settings.mcp_enabled.then_some(settings.mcp_port));
        let running_port = self.automation.as_ref().map(|(server, _)| server.port());
        let moved = self.automation_for_run.is_none() && running_port.is_some_and(|port| Some(port) != wanted);
        if wanted.is_none() || moved {
            self.stop_automation();
        }
        if let Some(port) = wanted
            && self.automation.is_none()
        {
            self.automation_port = Some(port);
            self.start_automation(ctx);
        }
        apply_interface_language(&settings);
        zen_workbench::network::apply(&settings.proxy);
        zen_workbench::network::certificates::apply(settings.accept_untrusted_certificates);
        let windows = self.shared.state().windows.clone();
        for slot in &windows {
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            settings.apply_to_session(window.session.settings_mut());
            zen_workbench::tabs::follow_policy(&mut window.session);
            let previews = window.session.state_mut::<zen_workbench::preview::Previews>();
            let kind = zen_workbench::preview::PreviewKind::Markdown;
            if previews.mode(kind) != settings.markdown.preview_layout {
                previews.set_mode(kind, settings.markdown.preview_layout);
            }
        }
        settings.apply_to_session(self.welcome_session.settings_mut());
        self.shared.clipboard_history.keep(settings.clipboard_limit);
        self.shared.caffeine.configure(settings.caffeine.clone(), std::time::Instant::now());
        ctx.request_repaint();
        if let Ok(mut slot) = self.shared.settings.lock() {
            *slot = settings;
        }
    }

    fn stop_automation(&mut self) {
        self.automation = None;
        if let Ok(mut slot) = self.shared.automation_url.lock() {
            *slot = None;
        }
        if let Some(file) = self.automation_file.as_ref().filter(|_| !self.headless) {
            let _ = std::fs::remove_file(file);
        }
    }

    fn listen_for_launchers(&mut self, ctx: &egui::Context) {
        static PROMPTS: std::sync::Once = std::sync::Once::new();
        PROMPTS.call_once(|| {
            let wake = ctx.clone();
            zen_net::trust::enable_prompts(move || wake.request_repaint());
        });
        if !std::mem::take(&mut self.launchers_wanted) {
            return;
        }
        let data = self.shared.state().recent_file.as_deref().and_then(Path::parent).map(Path::to_owned);
        let Some(data) = data else { return };
        let shared = Arc::downgrade(&self.shared);
        let wake = ctx.clone();
        let listener = zen_workbench::handoff::Listener::start_waiting(&data, move |paths, wait| {
            let Some(shared) = shared.upgrade() else { return false };
            if shared.exiting.load(std::sync::atomic::Ordering::SeqCst) {
                return false;
            }
            shared.handed_over.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((paths, wait));
            wake.request_repaint();
            true
        });
        self.launchers = listener.ok();
    }

    fn start_automation(&mut self, ctx: &egui::Context) {
        let Some(port) = self.automation_port.take() else { return };
        let product = &self.shared.product;
        let info = zen_automation::ServerInfo {
            name: product.name.to_owned(),
            version: product.version.to_owned(),
            instructions: super::automation::instructions(product.name, &self.shared.registry),
        };
        let wake = ctx.clone();
        let mut tools = super::automation::tools();
        tools.extend(super::automation::command_tools(&self.shared.registry));
        let started =
            zen_automation::Server::start_counting(port, info, tools, Arc::clone(&self.mcp_stats), move || {
                wake.request_repaint_of(ViewportId::ROOT);
            });
        match started {
            Ok((server, calls)) => {
                let url = server.url();
                if let Some(file) = self.automation_file.as_ref().filter(|_| !self.headless) {
                    let json = serde_json::json!({ "url": url, "pid": std::process::id() });
                    let _ = zen_vfs::write_atomic(file, json.to_string().as_bytes());
                }
                if let Ok(mut slot) = self.shared.automation_url.lock() {
                    *slot = Some(url);
                }
                self.automation = Some((server, calls));
            }
            Err(error) => {
                #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
                {
                    eprintln!("cannot start the automation server on port {port}: {error}");
                }
            }
        }
    }

    fn answer_calls(&mut self, ctx: &egui::Context) {
        let calls: Vec<_> = match &self.automation {
            Some((_, calls)) => calls.try_iter().collect(),
            None => return,
        };
        if !calls.is_empty() {
            self.agent_seen = Some(std::time::Instant::now());
        }
        let kept = self.kept_from_agents.lock().map(|mut kept| std::mem::take(&mut *kept)).unwrap_or_default();
        for (project, files) in kept {
            self.kept_from_agent(project.as_deref(), &files);
        }
        for call in calls {
            let (tool, arguments) = match super::automation::as_dispatch(&call.tool, &call.arguments) {
                Some(dispatch) => ("dispatch_command".to_owned(), dispatch),
                None => (call.tool.clone(), call.arguments.clone()),
            };
            self.route(ctx, &tool, &arguments, Box::new(move |result| call.reply(result)));
        }
    }

    fn start_waiting_run(&mut self, ctx: &egui::Context, arguments: &serde_json::Value, reply: super::agent::Reply) {
        let mut arguments = arguments.clone();
        let wait = arguments["params"]
            .as_object_mut()
            .and_then(|params| params.remove("waitMs"))
            .and_then(|wait| wait.as_u64())
            .unwrap_or(0);
        let project = super::automation::project_argument(&arguments);
        let started = self.tool(ctx, "dispatch_command", &arguments);
        let window = self.slot_for(project.as_deref()).map(|slot| slot.window);
        match (started, window) {
            (Ok(started), Ok(window)) => {
                let Some(run_id) = started["runId"].as_u64() else { return reply(Ok(started)) };
                let wait = std::time::Duration::from_millis(wait).min(zen_workbench::run::MAX_RUN_WAIT);
                let until = std::time::Instant::now() + wait;
                self.waiting_runs.push(WaitingRun { window, run_id, until, reply });
                self.answer_waiting_runs(ctx);
            }
            (Err(error), _) | (Ok(_), Err(error)) => reply(Err(error)),
        }
    }

    fn answer_waiting_runs(&mut self, ctx: &egui::Context) {
        let now = std::time::Instant::now();
        let mut soonest: Option<std::time::Duration> = None;
        for waiting in std::mem::take(&mut self.waiting_runs) {
            let summary = {
                let mut window = waiting.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                zen_workbench::run::wait_for_run(&mut window.session, waiting.run_id, std::time::Duration::ZERO)
            };
            match summary {
                Ok(summary) if !summary.done && now < waiting.until => {
                    let left = waiting.until - now;
                    soonest = Some(soonest.map_or(left, |soonest| soonest.min(left)));
                    self.waiting_runs.push(waiting);
                }
                Ok(summary) => (waiting.reply)(Ok(serde_json::json!({ "runId": waiting.run_id, "summary": summary }))),
                Err(error) => (waiting.reply)(Err(error.to_string())),
            }
        }
        if let Some(left) = soonest {
            ctx.request_repaint_after(left.min(std::time::Duration::from_millis(500)));
        }
    }

    fn start_waiting_lookup(
        &mut self,
        ctx: &egui::Context,
        arguments: &serde_json::Value,
        asked: std::time::Duration,
        reply: super::agent::Reply,
    ) {
        let mut arguments = arguments.clone();
        arguments["params"]["waitMs"] = serde_json::json!(0);
        let wait = zen_workbench::language::SearchWait::explicit(asked);
        self.waiting_lookups.push(WaitingLookup { arguments, wait, reply });
        self.answer_waiting_lookups(ctx);
    }

    fn answer_waiting_lookups(&mut self, ctx: &egui::Context) {
        if self.waiting_lookups.is_empty() {
            return;
        }
        let language = self.mcp_language();
        for mut waiting in std::mem::take(&mut self.waiting_lookups) {
            let answer = zen_i18n::with_language(&language, || self.tool(ctx, "dispatch_command", &waiting.arguments));
            match answer {
                Ok(answer) if super::automation::waits_on(&answer, &mut waiting.wait) => {
                    self.waiting_lookups.push(waiting);
                }
                answer => (waiting.reply)(answer),
            }
        }
        if !self.waiting_lookups.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn mcp_language(&self) -> String {
        self.shared
            .settings
            .lock()
            .map_or_else(|_| zen_i18n::SOURCE.to_owned(), |settings| settings.mcp_language.clone())
    }

    fn route(&mut self, ctx: &egui::Context, tool: &str, arguments: &serde_json::Value, reply: super::agent::Reply) {
        let language = self.mcp_language();
        let redactor = self.redactor();
        let project = super::automation::project_argument(arguments);
        let named = redactor.named_in(tool, arguments).or_else(|| {
            let requires_file = arguments
                .get("command")
                .and_then(serde_json::Value::as_str)
                .and_then(|command| self.shared.registry.info(command))
                .is_some_and(|info| super::redact::requires_file(&info.params_schema));
            super::redact::acts_on_active_document(tool, arguments, requires_file)
                .then(|| self.active_document(project.as_deref()))
                .flatten()
                .filter(|path| redactor.hides(path))
        });
        if let Some(file) = named {
            let message = zen_i18n::with_language(&language, || zen_i18n::t!("mcp-redacted", file = file.as_str()));
            self.kept_from_agent(project.as_deref(), std::slice::from_ref(&file));
            let error = serde_json::json!({ "error": super::redact::REDACTED_CODE, "file": file, "message": message });
            reply(Err(error.to_string()));
            return;
        }
        let patterns =
            self.shared.settings.lock().map(|settings| settings.mcp_redact_files.clone()).unwrap_or_default();
        for slot in self.shared.state().windows.clone() {
            if let Ok(mut window) = slot.window.lock() {
                window.session.state_mut::<zen_workbench::hidden_files::HiddenFromAgents>().set(&patterns);
            }
        }
        let kept = Arc::clone(&self.kept_from_agents);
        let reply: super::agent::Reply = Box::new(move |result| {
            reply(result.map(|mut answer| {
                let hidden = redactor.redact(&mut answer);
                if !hidden.is_empty()
                    && let Ok(mut kept) = kept.lock()
                {
                    kept.push((project, hidden));
                }
                answer
            }));
        });
        zen_i18n::with_language(&language, || self.route_call(ctx, tool, arguments, reply));
    }

    fn active_document(&self, project: Option<&Path>) -> Option<String> {
        let slot = self.slot_for(project).ok()?;
        let window = slot.window.lock().ok()?;
        let document = window.session.state::<Workbench>()?.active_document()?;
        Some(document.path().to_string_lossy().into_owned())
    }

    fn redactor(&self) -> super::redact::Redactor {
        let patterns =
            self.shared.settings.lock().map(|settings| settings.mcp_redact_files.clone()).unwrap_or_default();
        let roots = self.shared.state().windows.iter().map(|slot| slot.path.clone()).collect();
        let closed: Vec<PathBuf> = self
            .shared
            .state()
            .windows
            .iter()
            .filter_map(|slot| {
                let window = slot.window.lock().ok()?;
                let provider = window.session.state::<zen_workbench::hidden_files::AgentHiddenPaths>()?.0.clone()?;
                Some(provider(&window.session))
            })
            .flatten()
            .collect();
        let redactor = super::redact::Redactor::new(&patterns, roots).with_closed(closed);
        match self.shared.state().recent_file.as_ref().and_then(|file| file.parent()).map(Path::to_owned) {
            Some(data) => redactor.with_data_folder(data),
            None => redactor,
        }
    }

    fn kept_from_agent(&self, project: Option<&Path>, files: &[String]) {
        let Ok(slot) = self.slot_for(project) else { return };
        let Ok(mut window) = slot.window.lock() else { return };
        for file in files {
            let body = zen_i18n::t!("mcp-redacted-notice", file = file.as_str());
            let notice = zen_workbench::notifications::NewNotification::message(
                zen_workbench::notifications::Group::General,
                zen_workbench::notifications::Kind::Warning,
                body,
            );
            zen_workbench::notifications::post(&mut window.session, notice);
        }
    }

    fn route_call(
        &mut self,
        ctx: &egui::Context,
        tool: &str,
        arguments: &serde_json::Value,
        reply: super::agent::Reply,
    ) {
        if super::automation::waits_for_run(tool, arguments) {
            self.start_waiting_run(ctx, arguments, reply);
            return;
        }
        if let Some(asked) = super::automation::loading_wait(tool, arguments) {
            self.start_waiting_lookup(ctx, arguments, asked, reply);
            return;
        }
        if self.headless {
            reply(self.headless_tool(ctx, tool, arguments));
            return;
        }
        match self.window_request(tool, arguments) {
            Some(Ok((slot, request))) => {
                if slot.tabbed {
                    self.shared.state().active_tab = Some(slot.path.clone());
                }
                let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let ProjectWindow { session, view } = &mut *window;
                if let Some(refusal) = super::run_launch::refusal_while_person_decides(session) {
                    reply(Err(refusal));
                    return;
                }
                match request {
                    WindowCall::Action { action, command, params } => {
                        let params = super::automation::spelled_params(&slot.path, params);
                        let result =
                            view.run_for_agent(ctx, &self.shared.registry, session, (&action, &command, params));
                        reply(result);
                    }
                    WindowCall::Keys(keys) => view.queue_keys(keys, reply),
                }
                drop(window);
                ctx.request_repaint_of(slot.drawn_in());
                ctx.request_repaint();
            }
            Some(Err(error)) => reply(Err(error)),
            None => reply(self.tool(ctx, tool, arguments)),
        }
    }

    fn headless_tool(
        &mut self,
        ctx: &egui::Context,
        tool: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let window_only = |tool: &str| Err(zen_i18n::t!("ide-headless-window-only", tool = tool));
        match self.window_request(tool, arguments) {
            Some(Ok((_, WindowCall::Action { command, .. }))) if command == "view.ide.exit" => {
                self.save_projects(ctx, None)?;
                let (exiting, running) = self.exit(ctx, true);
                Ok(serde_json::json!({ "exiting": exiting, "asking": false, "running": running }))
            }
            Some(Ok((_, WindowCall::Action { command, .. })))
                if matches!(command.as_str(), "view.ideSettings.restoreDefaults" | "view.ideSettings.import") =>
            {
                window_only(tool)
            }
            Some(Ok((slot, WindowCall::Action { action, command, params }))) => {
                let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let ProjectWindow { session, view } = &mut *window;
                let params = super::automation::spelled_params(&slot.path, params);
                let result = view.run_for_agent(ctx, &self.shared.registry, session, (&action, &command, params));
                drop(window);
                ctx.request_repaint();
                result
            }
            Some(Ok((_, WindowCall::Keys(_)))) => window_only(tool),
            Some(Err(error)) => Err(error),
            None => self.tool(ctx, tool, arguments),
        }
    }

    fn window_request(&self, tool: &str, arguments: &serde_json::Value) -> Option<Result<(Slot, WindowCall), String>> {
        if self.welcome_call(tool, arguments).is_some() {
            return None;
        }
        let text = |name: &str| arguments.get(name).and_then(serde_json::Value::as_str);
        let params = arguments.get("params").filter(|params| !params.is_null()).cloned();
        let request = match tool {
            "run_action" => {
                let Some(action) = text("action") else { return Some(Err("`action` is required".to_owned())) };
                self.action_request(action, params)
            }
            "dispatch_command" => {
                let command = text("command")?;
                let action = command == zen_workbench::notifications::RUN_ACTION;
                if self.shared.registry.info(command).is_some() && !action {
                    return None;
                }
                let bound = crate::keymap::current().bindings().into_iter().any(|binding| binding.action == command);
                if !bound && !command.starts_with("view.") && !action {
                    return None;
                }
                self.action_request(command, params)
            }
            "press_keys" if self.shared.state().trust_question => {
                Err("The person answers the Trust and Open Project dialog; agents cannot press keys in it".to_owned())
            }
            "press_keys" => {
                let keys: Vec<String> = match arguments.get("keys") {
                    None | Some(serde_json::Value::Null) => Vec::new(),
                    Some(keys) => match serde_json::from_value(keys.clone()) {
                        Ok(keys) => keys,
                        Err(error) => return Some(Err(format!("`keys`: {error}"))),
                    },
                };
                let mut keystrokes = Vec::new();
                for key in &keys {
                    match zen_core::keymap::Keystroke::parse(key) {
                        Some(stroke) => keystrokes.push(stroke),
                        None => return Some(Err(format!("`{key}` is not a keystroke, such as ⎋, ⏎ or ⇧⌘A"))),
                    }
                }
                let text = text("text").map(str::to_owned);
                if keystrokes.is_empty() && text.as_deref().is_none_or(str::is_empty) {
                    return Some(Err("give `keys`, `text` or both".to_owned()));
                }
                Ok(WindowCall::Keys(super::agent::KeyPress { keystrokes, text }))
            }
            _ => return None,
        };
        let project = super::automation::project_argument(arguments);
        Some(request.and_then(|request| Ok((self.slot_for(project.as_deref())?, request))))
    }

    fn welcome_call<'a>(&self, tool: &str, arguments: &'a serde_json::Value) -> Option<&'a str> {
        let key = match tool {
            "run_action" => "action",
            "dispatch_command" => "command",
            _ => return None,
        };
        let id = arguments.get(key).and_then(serde_json::Value::as_str)?;
        let new_project = id.starts_with("project.new.")
            || id.starts_with("view.newProject.")
            || id.starts_with("project.fromVcs.")
            || id.starts_with("view.project.fromVcs")
            || id == "project.fromSources.detect"
            || id == "view.project.fromSources"
            || id.starts_with("view.welcome.")
            || id == "view.guide.open"
            || id == "view.settings.open"
            || id == "view.ideSettings.import"
            || id.starts_with("settings.ide.")
            || id.starts_with("ui.theme.")
            || id.starts_with("ui.keymap.")
            || id.starts_with("plugins.");
        (new_project && self.shared.state().windows.is_empty()).then_some(id)
    }

    fn welcome_tool(
        &mut self,
        ctx: &egui::Context,
        id: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let params = arguments.get("params").filter(|params| !params.is_null());
        let params = params.cloned().unwrap_or_else(|| serde_json::json!({}));
        match id {
            "view.newProject.open" | "view.newProject.empty" => {
                let generator = params.get("generator").and_then(serde_json::Value::as_str).map(str::to_owned);
                let empty = zen_workbench::new_project::EMPTY.to_owned();
                self.welcome.want_new_project(if id.ends_with("empty") { Some(empty) } else { generator }, true);
                Ok(serde_json::json!({ "dialog": "newProject" }))
            }
            "view.newProject.close" => {
                self.welcome.new_project = None;
                Ok(serde_json::json!({}))
            }
            "view.project.fromVcs" => {
                self.welcome.want_clone(true);
                Ok(serde_json::json!({ "dialog": "clone" }))
            }
            "view.project.fromVcs.close" => {
                self.welcome.clone = None;
                Ok(serde_json::json!({}))
            }
            "view.project.fromSources" => {
                let path = params.get("path").and_then(serde_json::Value::as_str);
                let path = path.ok_or_else(|| "call project.fromSources.detect with the path".to_owned())?;
                let registry = &self.shared.registry;
                let found = zen_core::macros::as_agent(&mut self.welcome_session, |session| {
                    registry.dispatch(session, "project.fromSources.detect", serde_json::json!({ "path": path }))
                })
                .map_err(|error| super::automation::dispatch_error(&error))?;
                if let Some(root) = found["root"].as_str() {
                    self.open_without_asking(ctx, Path::new(root));
                }
                Ok(found)
            }
            "view.welcome.open" => {
                let section = WelcomeSection::from_params(&params)?;
                self.bring_welcome(ctx, section);
                Ok(serde_json::json!({ "section": self.welcome.section.name() }))
            }
            "view.guide.open" => {
                let url = super::welcome::guide_url(&params)?;
                ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
                Ok(serde_json::json!({ "url": url }))
            }
            "view.settings.open" => {
                self.welcome.want_settings(params.get("page").and_then(serde_json::Value::as_str));
                Ok(serde_json::json!({}))
            }
            "view.ideSettings.import" => {
                self.welcome.want_import(params);
                Ok(serde_json::json!({ "dialog": "importSettings" }))
            }
            _ => {
                let registry = &self.shared.registry;
                zen_core::macros::as_agent(&mut self.welcome_session, |session| registry.dispatch(session, id, params))
                    .map_err(|error| super::automation::dispatch_error(&error))
            }
        }
    }

    fn action_request(&self, action: &str, params: Option<serde_json::Value>) -> Result<WindowCall, String> {
        let request = |command: &str, params: serde_json::Value| WindowCall::Action {
            action: action.to_owned(),
            command: command.to_owned(),
            params,
        };
        if let Some(binding) = crate::keymap::current().bindings().into_iter().find(|binding| binding.action == action)
        {
            return Ok(request(&binding.command, merged(binding.params, params)));
        }
        if let Some(step) = super::search_everywhere::command_step(action) {
            return Ok(request(&step.command, params.unwrap_or(step.params)));
        }
        if let Some(id) = action.strip_prefix("workspace.host.toggle/") {
            return Ok(request("workspace.host.toggle", params.unwrap_or_else(|| serde_json::json!({ "id": id }))));
        }
        if let Some(info) = self.shared.registry.info(action) {
            return Ok(match (params, super::search_everywhere::command_binding(info)) {
                (Some(params), _) => request(action, params),
                (None, Some(binding)) => request(&binding.command, binding.params),
                (None, None) => request(action, serde_json::json!({})),
            });
        }
        if self.is_window_action(action) {
            return Ok(request(action, params.unwrap_or_else(|| serde_json::json!({}))));
        }
        Err(format!("no action or command `{action}`; find_action and get_keymap list the actions"))
    }

    fn is_window_action(&self, action: &str) -> bool {
        self.shared.registry.commands().any(|info| {
            super::search_everywhere::command_ui(info.id.as_str()).is_some_and(|(window, _)| window == action)
        })
    }

    fn window_for(&self, project: Option<&Path>) -> Result<Arc<Mutex<ProjectWindow>>, String> {
        Ok(self.slot_for(project)?.window)
    }

    fn slot_for(&self, project: Option<&Path>) -> Result<Slot, String> {
        let windows = self.shared.state().windows.clone();
        match project {
            Some(project) => windows
                .into_iter()
                .find(|slot| slot.path == project)
                .ok_or_else(|| format!("no window has the project {}", project.display())),
            None if windows.len() == 1 => Ok(windows[0].clone()),
            None if windows.is_empty() => Err("no project is open; call open_project first".to_owned()),
            None => Err("several projects are open; name one with `project`".to_owned()),
        }
    }

    fn tool(
        &mut self,
        ctx: &egui::Context,
        tool: &str,
        arguments: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use super::automation::{describe, path_argument, spelled_params};
        let project = super::automation::project_argument(arguments);
        if let Some(id) = self.welcome_call(tool, arguments).map(str::to_owned) {
            ctx.request_repaint();
            return self.welcome_tool(ctx, &id, arguments);
        }
        match tool {
            "list_windows" => {
                let windows = self.shared.state().windows.clone();
                let list: Vec<_> = windows
                    .iter()
                    .filter_map(|slot| {
                        slot.window.lock().ok().map(|window| describe(&slot.path, window.session.state::<Workbench>()))
                    })
                    .collect();
                Ok(serde_json::json!({ "windows": list }))
            }
            "list_commands" => super::automation::list_commands(&self.shared.registry, arguments),
            "describe_command" => super::automation::describe_command(&self.shared.registry, arguments),
            "dispatch_command" => {
                let command =
                    arguments.get("command").and_then(serde_json::Value::as_str).ok_or("`command` is required")?;
                let params = arguments.get("params").cloned().unwrap_or_else(|| serde_json::json!({}));
                let slot = self.slot_for(project.as_deref())?;
                let params = spelled_params(&slot.path, params);
                let viewport = slot.drawn_in();
                let mut window = slot.window.lock().map_err(|_| "the window is unavailable")?;
                let registry = &self.shared.registry;
                if let Some(reason) = zen_workbench::workspaces::refusal(&window.session, command, &params) {
                    return Err(reason);
                }
                let result = zen_core::macros::as_agent(&mut window.session, |session| {
                    registry.dispatch(session, command, params)
                })
                .map_err(|error| super::automation::dispatch_error(&error));
                ctx.request_repaint_of(viewport);
                ctx.request_repaint();
                result
            }
            "open_project" => {
                let path = path_argument(arguments, "path").ok_or("`path` is required")?;
                if !path.exists() {
                    return Err(format!("{} does not exist", path.display()));
                }
                self.open_without_asking(ctx, &path);
                Ok(serde_json::json!({ "open": self.shared.overview().open }))
            }
            "focus_project" => {
                let path = project.ok_or("`project` is required")?;
                self.focus(ctx, &path);
                Ok(serde_json::json!({}))
            }
            "close_project" => {
                let path = project.ok_or("`project` is required")?;
                self.window_for(Some(&path))?;
                self.save_projects(ctx, Some(&path))?;
                self.handle(ctx, Request::Close(path));
                ctx.request_repaint();
                Ok(serde_json::json!({ "open": self.shared.overview().open }))
            }
            "exit_ide" => {
                let confirmed = arguments.get("confirmed").and_then(serde_json::Value::as_bool).unwrap_or(false);
                self.save_projects(ctx, None)?;
                let (exiting, running) = self.exit(ctx, confirmed);
                Ok(serde_json::json!({ "exiting": exiting, "asking": !exiting, "running": running }))
            }
            "list_plugins" => Ok(self.shared.plugins.list_plugins()),
            "install_plugin" => {
                let id = arguments.get("id").and_then(serde_json::Value::as_str).ok_or("`id` is required")?;
                self.shared.plugins.install_plugin_by_id(id)
            }
            "set_plugin_enabled" => {
                let id = arguments.get("id").and_then(serde_json::Value::as_str).ok_or("`id` is required")?;
                let enabled =
                    arguments.get("enabled").and_then(serde_json::Value::as_bool).ok_or("`enabled` is required")?;
                if enabled {
                    self.shared.plugins.agent_may_enable(id)?;
                }
                self.shared.plugins.set_enabled(id, enabled)?;
                Ok(serde_json::json!({ "id": id, "enabled": enabled }))
            }
            "uninstall_plugin" => {
                let id = arguments.get("id").and_then(serde_json::Value::as_str).ok_or("`id` is required")?;
                self.shared.plugins.uninstall(id)?;
                Ok(serde_json::json!({ "uninstalled": id }))
            }
            "list_themes" => serde_json::to_value(themes::list(&themes::lock(&self.shared.themes)))
                .map_err(|error| error.to_string()),
            "select_theme" => {
                let id = arguments.get("id").and_then(serde_json::Value::as_str).ok_or("`id` is required")?;
                themes::select(&mut themes::lock(&self.shared.themes), id).map_err(|error| error.message)?;
                self.follow_themes(ctx);
                Ok(serde_json::json!({}))
            }
            "follow_system_theme" => {
                let params: themes::FollowSystemParams =
                    serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
                themes::follow_system(&mut themes::lock(&self.shared.themes), &params)
                    .map_err(|error| error.message)?;
                self.follow_themes(ctx);
                Ok(serde_json::json!({}))
            }
            "import_theme" => {
                let params: themes::ImportParams =
                    serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
                let imported =
                    themes::import(&mut themes::lock(&self.shared.themes), &params).map_err(|error| error.message)?;
                self.follow_themes(ctx);
                serde_json::to_value(imported).map_err(|error| error.to_string())
            }
            "list_keymaps" => serde_json::to_value(keymaps::list(&keymaps::lock(&self.shared.keymaps)))
                .map_err(|error| error.to_string()),
            "select_keymap" => {
                let id = arguments.get("id").and_then(serde_json::Value::as_str).ok_or("`id` is required")?;
                keymaps::select(&mut keymaps::lock(&self.shared.keymaps), id).map_err(|error| error.message)?;
                self.follow_keymaps();
                ctx.request_repaint();
                Ok(serde_json::json!({}))
            }
            "import_keymap" => {
                let params: keymaps::ImportParams =
                    serde_json::from_value(arguments.clone()).map_err(|error| error.to_string())?;
                let imported = keymaps::import(&mut keymaps::lock(&self.shared.keymaps), &params)
                    .map_err(|error| error.message)?;
                self.follow_keymaps();
                ctx.request_repaint();
                serde_json::to_value(imported).map_err(|error| error.to_string())
            }
            "get_update_status" => {
                let offer = self.shared.overview().update;
                Ok(serde_json::json!({
                    "current": self.shared.product.version,
                    "available": offer.as_ref().map(|offer| offer.version.clone()),
                    "progress": offer.as_ref().and_then(|offer| offer.progress),
                    "error": offer.as_ref().and_then(|offer| offer.error.clone()),
                    "notes": offer.map(|offer| offer.notes.into_iter().map(|note| serde_json::json!({ "version": note.version, "notes": note.notes })).collect::<Vec<_>>()),
                }))
            }
            "update_ide" => {
                let updater = self
                    .updater
                    .as_mut()
                    .ok_or("this IDE is not installed through Zen Toolbox, so it cannot update itself")?;
                updater.update()?;
                Ok(serde_json::json!({ "updating": true }))
            }
            "get_settings" => serde_json::to_value(self.shared.overview().settings).map_err(|error| error.to_string()),
            "set_settings" => {
                let current = self.shared.overview().settings;
                let mut settings = super::automation::merge_settings(&current, arguments)?;
                settings.customization.note_agent_items(&current.customization);
                self.shared.state().requests.push(Request::Settings(Box::new(settings.clone())));
                ctx.request_repaint();
                serde_json::to_value(settings).map_err(|error| error.to_string())
            }
            "find_action" => {
                let query = arguments.get("query").and_then(serde_json::Value::as_str).unwrap_or_default();
                let limit = arguments.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(20) as usize;
                let window = self.window_for(project.as_deref())?;
                let mut window = window.lock().map_err(|_| "the window is unavailable")?;
                let found =
                    super::search_everywhere::find_actions(query, &self.shared.registry, &mut window.session, limit);
                let actions: Vec<_> = found
                    .into_iter()
                    .map(|action| {
                        let opens = (action.command != action.binding.command).then_some(&action.binding.command);
                        serde_json::json!({
                            "action": action.binding.action,
                            "name": action.name,
                            "group": action.group,
                            "shortcut": action.shortcut,
                            "command": action.command,
                            "params": if opens.is_some() { serde_json::Value::Null } else { action.binding.params },
                            "opens": opens,
                            "available": action.available,
                            "ranges": action.ranges.iter().map(|range| [range.start, range.end]).collect::<Vec<_>>(),
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "actions": actions }))
            }
            "get_keymap" => {
                let (id, keymap, abbreviations) = {
                    let store = keymaps::lock(&self.shared.keymaps);
                    (store.current().to_owned(), store.active().clone(), store.abbreviations().clone())
                };
                let actions: Vec<_> = keymap
                    .bindings()
                    .into_iter()
                    .map(|binding| {
                        serde_json::json!({
                            "action": binding.action,
                            "name": super::keymap_page::action_title(&binding.action),
                            "command": binding.command,
                            "params": binding.params,
                            "context": binding.context,
                            "keystrokes": binding.keystrokes,
                            "gesture": zen_core::keymap::gesture(&binding.action),
                            "changed": keymap.is_changed(&binding.action),
                            "abbreviations": abbreviations.get(&binding.action).cloned().unwrap_or_default(),
                            "synonyms": super::search_everywhere::synonyms(&binding.action),
                        })
                    })
                    .collect();
                let conflicts: Vec<_> = keymap
                    .conflicts()
                    .into_iter()
                    .map(|(keystroke, actions)| serde_json::json!({ "keystroke": keystroke, "actions": actions }))
                    .collect();
                Ok(serde_json::json!({
                    "keymap": id,
                    "actions": actions,
                    "conflicts": conflicts,
                }))
            }
            "set_keymap" => {
                let action =
                    arguments.get("action").and_then(serde_json::Value::as_str).ok_or("`action` is required")?;
                let mut store = keymaps::lock(&self.shared.keymaps);
                let command = self.shared.registry.info(action).and_then(super::search_everywhere::command_binding);
                let step = super::search_everywhere::command_step(action);
                if !store.active().has_default(action) && command.is_none() && step.is_none() {
                    return Err(format!("no action `{action}`; see get_keymap and find_action"));
                }
                let keystrokes = match arguments.get("keystrokes") {
                    None | Some(serde_json::Value::Null) => None,
                    Some(value) => Some(
                        serde_json::from_value::<Vec<zen_core::keymap::Keystroke>>(value.clone())
                            .map_err(|error| error.to_string())?,
                    ),
                };
                let mut changes = store.active().user_changes().clone();
                match keystrokes {
                    Some(keys) => {
                        changes.insert(action.to_owned(), keys);
                    }
                    None => {
                        changes.remove(action);
                    }
                }
                store.set_changes(changes).map_err(|error| keymaps::message(&error).message)?;
                ctx.request_repaint();
                Ok(serde_json::json!({
                    "keymap": store.current(),
                    "action": action,
                    "keystrokes": store.active().keystrokes(action),
                }))
            }
            "get_window_state" if self.shared.state().windows.is_empty() => Ok(self.welcome.state()),
            "get_window_state" => {
                let slot = self.slot_for(project.as_deref())?;
                let mut window = slot.window.lock().map_err(|_| "the window is unavailable")?;
                let mut state = window.view.window_state(&window.session);
                state["headless"] = serde_json::json!(self.headless);
                state["session"] = session_overview(&self.shared.registry, &mut window.session, &slot.path);
                Ok(state)
            }
            "list_recent_projects" => Ok(serde_json::json!({ "projects": self.shared.overview().recent })),
            "get_document_text" => {
                let mut path = path_argument(arguments, "path").ok_or("`path` is required")?;
                if let Ok(slot) = self.slot_for(project.as_deref()) {
                    path = if path.is_relative() {
                        slot.path.join(&path)
                    } else {
                        zen_vfs::spelled_from(&slot.path, &path)
                    };
                }
                if zen_image::opens_as_picture(&path) {
                    return Err("the file is an image: open it and use the editor.image.info command".to_owned());
                }
                if let Ok(window) = self.window_for(project.as_deref()) {
                    let window = window.lock().map_err(|_| "the window is unavailable")?;
                    let open = window
                        .session
                        .state::<Workbench>()
                        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path));
                    if let Some(document) = open {
                        return super::automation::document_text(document.text(), document.is_dirty(), arguments);
                    }
                }
                let text = zen_vfs::read_text(&path).map_err(|error| error.to_string())?;
                super::automation::document_text(&text, false, arguments)
            }
            #[cfg(not(target_arch = "wasm32"))]
            "terminal_open" | "terminal_write" | "terminal_read" => {
                let slot = self.slot_for(project.as_deref())?;
                let viewport = slot.drawn_in();
                let mut window = slot.window.lock().map_err(|_| "the window is unavailable")?;
                let session = &mut window.session;
                let registry = &self.shared.registry;
                let dispatch = |session: &mut Session, id: &str, params: serde_json::Value| {
                    registry.dispatch(session, id, params).map_err(|error| error.to_string())
                };
                let result = if tool == "terminal_open" {
                    dispatch(session, "terminal.session.new", serde_json::json!({}))
                } else {
                    zen_workbench::terminal::restore_pending(session).map_err(|error| error.to_string())?;
                    let tab = arguments.get("tab").and_then(serde_json::Value::as_u64).map(|tab| tab as usize);
                    let terminals = session.state::<zen_workbench::terminal::Terminals>();
                    let id = match tab {
                        Some(index) => {
                            terminals.and_then(|terminals| terminals.tabs().get(index)).map(|tab| tab.focused())
                        }
                        None => terminals.and_then(zen_workbench::terminal::Terminals::focused),
                    };
                    let index = tab.unwrap_or_default();
                    let id = id.ok_or_else(|| format!("there is no terminal tab {index}; call terminal_open"))?;
                    if tool == "terminal_write" {
                        let text =
                            arguments.get("text").and_then(serde_json::Value::as_str).ok_or("`text` is required")?;
                        dispatch(session, "terminal.session.send", serde_json::json!({ "id": id, "text": text }))
                    } else {
                        dispatch(session, "terminal.session.read", serde_json::json!({ "id": id }))
                    }
                };
                ctx.request_repaint_of(viewport);
                ctx.request_repaint();
                result
            }
            "get_tree_selection" | "select_in_tree" => {
                let slot = self.slot_for(project.as_deref())?;
                let viewport = slot.drawn_in();
                let mut window = slot.window.lock().map_err(|_| "the window is unavailable")?;
                let root = slot.path.clone();
                let view = &mut window.view;
                if tool == "select_in_tree" {
                    let paths: Vec<PathBuf> = arguments
                        .get("paths")
                        .and_then(serde_json::Value::as_array)
                        .ok_or("`paths` is required")?
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(|path| zen_vfs::spelled_from(&root, Path::new(path)))
                        .collect();
                    view.select_paths(&root, paths)?;
                    ctx.request_repaint_of(viewport);
                    ctx.request_repaint();
                }
                Ok(serde_json::json!({ "paths": view.selection.items(&view.visible_rows) }))
            }
            other => Err(format!("unknown tool `{other}`")),
        }
    }

    pub fn initial_size(&self) -> [f32; 2] {
        if self.shared.state().windows.is_empty() { WELCOME_SIZE } else { PROJECT_SIZE }
    }

    pub fn set_theme(&mut self, ctx: &egui::Context, theme: ResolvedTheme) {
        let palette = IdePalette::from_theme(&theme);
        self.welcome.set_palette(palette);
        let windows = self.shared.state().windows.clone();
        for slot in &windows {
            slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner).view.set_theme(&theme);
        }
        *self.shared.theme.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = theme;
        palette.apply(ctx);
        ctx.request_repaint();
    }

    fn follow_themes(&mut self, ctx: &egui::Context) {
        if let Some(system) = ctx.system_theme() {
            themes::lock(&self.shared.themes).set_system_dark(system == egui::Theme::Dark);
        }
        let (theme, applied, saved) = {
            let store = themes::lock(&self.shared.themes);
            let applied = (store.generation(), store.current().to_owned());
            if applied == self.theme_applied {
                return;
            }
            (store.resolve_current().theme, applied, SavedTheme::of(&store))
        };
        self.set_theme(ctx, theme);
        let settings = self.shared.settings.lock().ok().and_then(|mut settings| {
            let changed = SavedTheme::from_settings(&settings) != saved;
            saved.write(&mut settings);
            changed.then(|| settings.clone())
        });
        if let Some(settings) = settings {
            self.save_settings(&settings);
        }
        self.theme_applied = applied;
    }

    fn follow_keymaps(&mut self) {
        let current = keymaps::lock(&self.shared.keymaps).current().to_owned();
        if current == self.keymap_applied {
            return;
        }
        let chosen = (current != zen_core::keymap::MACOS_KEYMAP).then(|| current.clone());
        let settings = self.shared.settings.lock().ok().map(|mut settings| {
            settings.keymap = chosen;
            settings.clone()
        });
        if let Some(settings) = settings {
            self.save_settings(&settings);
        }
        self.keymap_applied = current;
    }

    fn follow_file_types(&mut self, ctx: &egui::Context) {
        let generation = zen_vfs::file_types::generation();
        if generation == self.file_types_seen {
            return;
        }
        self.file_types_seen = generation;
        let windows = self.shared.state().windows.clone();
        for slot in &windows {
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            zen_workbench::file_types::follow(&mut window.session);
        }
        ctx.request_repaint();
    }

    pub fn palette(&self) -> IdePalette {
        self.shared.palette()
    }

    fn follow_project_tab_moves(&mut self, ctx: &egui::Context) {
        let windows = self.shared.state().windows.clone();
        let mut moves = Vec::new();
        for slot in &windows {
            let Ok(mut window) = slot.window.try_lock() else { continue };
            let asked =
                window.session.state::<zen_workbench::tabs::ProjectTabMove>().is_some_and(|moving| moving.asked());
            if asked && let Some(to) = window.session.state_mut::<zen_workbench::tabs::ProjectTabMove>().take() {
                moves.push((slot.path.clone(), to));
            }
        }
        for (path, to) in moves {
            self.handle(ctx, Request::MoveTab(path, to));
        }
    }

    pub fn logic(&mut self, ctx: &egui::Context) {
        self.shared.plugins.attach(ctx);
        if let Some(watch) = self.update_watch.take() {
            watch.spawn(Arc::clone(&self.shared), ctx.clone());
        }
        self.watch_internal_errors(ctx);
        self.start_automation(ctx);
        self.listen_for_launchers(ctx);
        self.answer_calls(ctx);
        self.answer_waiting_runs(ctx);
        self.answer_waiting_lookups(ctx);
        self.follow_themes(ctx);
        self.follow_keymaps();
        self.follow_file_types(ctx);
        self.follow_customization_commands();
        self.follow_project_tab_moves(ctx);
        if !self.headless {
            self.updates(ctx);
            self.help_on_start();
            self.follow_areas(ctx);
            #[cfg(target_os = "macos")]
            if super::main_menu::take_quit() {
                self.shared.state().requests.push(Request::Exit { confirmed: false });
            }
            self.follow_system_quit(ctx);
        }
        let handed: Vec<HandedOver> =
            std::mem::take(&mut *self.shared.handed_over.lock().unwrap_or_else(std::sync::PoisonError::into_inner));
        let requests = std::mem::take(&mut self.shared.state().requests);
        for request in requests {
            self.handle(ctx, request);
        }
        for (paths, wait) in handed {
            self.handed_over(ctx, &paths);
            if let Some(held) = wait {
                self.hold_start(&paths, held);
            }
        }
        self.follow_waiting_starts(ctx);
        self.follow_exit(ctx);
        self.follow_caffeine(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.follow_disk(ctx);
        self.follow_autosave(ctx);
        self.follow_consoles(ctx);
        self.follow_recovery(ctx);
        self.step_undrawn(ctx);
        if !self.headless {
            self.follow_ui_zoom(ctx);
        }
    }

    fn watch_internal_errors(&mut self, ctx: &egui::Context) {
        if !std::mem::replace(&mut self.internal_errors_watched, true) {
            let ctx = ctx.clone();
            zen_workbench::internal_errors::InternalErrors::global().set_waker(Arc::new(move || ctx.request_repaint()));
        }
    }

    fn step_undrawn(&mut self, ctx: &egui::Context) {
        let now = if self.headless { exit_clock() } else { ctx.input(|input| input.time) };
        let windows = self.shared.state().windows.clone();
        let overview = self.headless.then(|| self.shared.overview());
        for slot in windows {
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let ProjectWindow { session, view } = &mut *window;
            if view.take_drawn() {
                continue;
            }
            if let Some(overview) = &overview {
                view.overview = overview.clone();
            }
            let focused =
                self.headless || ctx.input_for(slot.drawn_in(), |input| input.viewport().focused.unwrap_or(false));
            view.step(ctx, &self.shared.registry, session, now, focused);
        }
    }

    fn follow_caffeine(&mut self, ctx: &egui::Context) {
        let caffeine = self.shared.caffeine.clone();
        if !std::mem::replace(&mut self.caffeine_woken, true) {
            let wake = ctx.clone();
            caffeine.set_waker(Arc::new(move || wake.request_repaint_of(ViewportId::ROOT)));
        }
        let on = caffeine.is_on();
        let windows = self.shared.state().windows.clone();
        if std::mem::replace(&mut self.caffeine_was_on, on) != on {
            for slot in &windows {
                ctx.request_repaint_of(slot.drawn_in());
            }
        }
        if !on {
            return;
        }
        let now = std::time::Instant::now();
        let mut busy = Vec::new();
        for slot in &windows {
            let window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            busy.extend(zen_workbench::caffeine::session_activity(&window.session));
        }
        let recent = self
            .agent_seen
            .is_some_and(|seen| now.saturating_duration_since(seen) < zen_workbench::caffeine::AGENT_IDLE);
        if recent || self.bridge_leases(now) > 0 {
            busy.push(zen_workbench::caffeine::Activity::Agent);
        }
        caffeine.observe(&busy, now);
        if let Some(wait) = caffeine.next_check(std::time::Instant::now()) {
            ctx.request_repaint_after(wait);
        }
    }

    fn bridge_leases(&mut self, now: std::time::Instant) -> usize {
        if let Some((at, count)) = self.leases_counted
            && now.saturating_duration_since(at) < zen_workbench::caffeine::TICK
        {
            return count;
        }
        let data = self.shared.state().recent_file.as_deref().and_then(Path::parent).map(Path::to_owned);
        let count = data.map_or(0, |data| zen_automation::instances::clients(&data, std::process::id()));
        self.leases_counted = Some((now, count));
        count
    }

    pub(super) fn finished(&self) -> bool {
        self.finished
    }

    pub(super) fn automation_url(&self) -> Option<String> {
        self.shared.automation_url.lock().ok().and_then(|url| url.clone())
    }

    pub(super) fn open_projects(&self) -> Vec<PathBuf> {
        self.shared.overview().open
    }

    pub(super) fn language_server_report(&self) -> Vec<String> {
        let language = self
            .shared
            .settings
            .lock()
            .map_or_else(|_| zen_i18n::SOURCE.to_owned(), |settings| settings.mcp_language.clone());
        zen_i18n::with_language(&language, || self.server_report_lines())
    }

    fn server_report_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for root in self.open_projects() {
            let project = root.display().to_string();
            if !zen_workbench::trust::is_trusted(&self.trust_env, &root) {
                lines.push(zen_i18n::t!("ide-headless-safe-mode", project = project.as_str()));
                continue;
            }
            let needed = self.shared.language_servers.needed_by(&root);
            let found: Vec<String> = needed
                .iter()
                .filter_map(|server| {
                    server.program.as_ref().map(|program| format!("{} ({})", server.id, program.display()))
                })
                .collect();
            lines.push(if found.is_empty() {
                zen_i18n::t!("ide-headless-servers-none-found", project = project.as_str())
            } else {
                zen_i18n::t!("ide-headless-servers-found", project = project.as_str(), servers = found.join(", "))
            });
            for server in needed {
                let line = match &server.install {
                    None => continue,
                    Some(Ok(command)) => zen_i18n::t!(
                        "ide-headless-server-missing",
                        project = project.as_str(),
                        server = server.id.as_str(),
                        command = command.as_str()
                    ),
                    Some(Err(hint)) => zen_i18n::t!(
                        "ide-headless-server-missing-manual",
                        project = project.as_str(),
                        server = server.id.as_str(),
                        hint = hint.as_str()
                    ),
                };
                lines.push(line);
            }
        }
        lines
    }

    #[cfg(test)]
    pub(super) fn with_session<T>(&self, f: impl FnOnce(&mut Session) -> T) -> Option<T> {
        let slot = self.slot_for(None).ok()?;
        let mut window = slot.window.lock().ok()?;
        Some(f(&mut window.session))
    }

    pub(super) fn request_exit(&mut self) {
        self.ending = true;
        self.shared.state().requests.push(Request::Exit { confirmed: true });
    }

    fn follow_areas(&mut self, ctx: &egui::Context) {
        let mut areas = zen_modules::Areas::lock(&self.areas);
        let first = !std::mem::replace(&mut self.areas_checked, true) && areas.should_offer();
        if (areas.take_show() || first) && self.areas_dialog.is_none() {
            self.areas_dialog = Some(super::areas_dialog::AreasDialog::new(areas.list(), areas.generation()));
            ctx.request_repaint_of(ViewportId::ROOT);
        }
        if self.areas_dialog.as_ref().is_some_and(|dialog| dialog.generation != areas.generation()) {
            self.areas_dialog = None;
        }
        areas.set_open(self.areas_dialog.is_some());
    }

    fn areas_dialog(&mut self, ui: &mut Ui) {
        let palette = self.shared.palette();
        let Some(dialog) = &mut self.areas_dialog else { return };
        let Some(outcome) = dialog.show(ui, &palette) else { return };
        let (command, params, restart) = match outcome {
            super::areas_dialog::Outcome::Choose(areas) => {
                ("plugins.areas.choose", serde_json::json!({ "areas": areas }), !dialog.restarts().is_empty())
            }
            super::areas_dialog::Outcome::Skip => ("plugins.areas.skip", serde_json::json!({}), false),
        };
        match self.shared.registry.dispatch(&mut self.welcome_session, command, params) {
            Ok(_) => {
                self.areas_dialog = None;
                zen_modules::Areas::lock(&self.areas).set_open(false);
                if restart {
                    self.shared.state().requests.push(Request::Restart);
                    ui.ctx().request_repaint();
                }
            }
            Err(zen_core::DispatchError::Failed { error, .. }) => dialog.error = Some(error.message),
            Err(other) => dialog.error = Some(other.to_string()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn follow_disk(&mut self, ctx: &egui::Context) {
        let windows = self.shared.state().windows.clone();
        for slot in windows {
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let ProjectWindow { session, view } = &mut *window;
            if view.follow_disk(ctx, &self.shared.registry, session) {
                ctx.request_repaint_of(slot.drawn_in());
            }
        }
    }

    fn follow_autosave(&mut self, ctx: &egui::Context) {
        let (on, seconds) = self
            .shared
            .settings
            .lock()
            .map(|settings| (settings.autosave_on_idle, settings.autosave_idle_seconds))
            .unwrap_or_default();
        if !on || self.shared.is_exiting() {
            return;
        }
        let modified = self.modified_fingerprint();
        let idle = self.shared.autosave.lock().unwrap_or_else(std::sync::PoisonError::into_inner).idle(
            exit_clock(),
            f64::from(seconds),
            modified,
        );
        match idle {
            super::autosave::Idle::Due => self.autosave(ctx),
            super::autosave::Idle::In(left) => {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.05)));
            }
            super::autosave::Idle::Waiting => {}
        }
    }

    fn follow_consoles(&mut self, ctx: &egui::Context) {
        if self.shared.is_exiting() {
            return;
        }
        let modified = self.consoles_fingerprint();
        let idle = self.shared.consoles.lock().unwrap_or_else(std::sync::PoisonError::into_inner).idle(
            exit_clock(),
            CONSOLE_IDLE_SECONDS,
            modified,
        );
        match idle {
            super::autosave::Idle::Due => {
                let windows = self.shared.state().windows.clone();
                for slot in windows {
                    let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                    let (saved, _) = zen_workbench::workspaces::save_consoles(&mut window.session);
                    if !saved.is_empty() {
                        ctx.request_repaint_of(slot.drawn_in());
                    }
                }
            }
            super::autosave::Idle::In(left) => {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(left.max(0.05)));
            }
            super::autosave::Idle::Waiting => {}
        }
    }

    fn follow_recovery(&mut self, ctx: &egui::Context) {
        if self.shared.is_exiting() {
            return;
        }
        let windows = self.shared.state().windows.clone();
        for slot in windows {
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if zen_workbench::database::recovery::capture(&mut window.session) {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
    }

    fn consoles_fingerprint(&self) -> Option<u64> {
        use std::hash::{Hash, Hasher};
        let windows = self.shared.state().windows.clone();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut any = false;
        for slot in windows {
            let window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            for console in zen_workbench::workspaces::dirty_console_versions(&window.session) {
                any = true;
                console.hash(&mut hasher);
            }
        }
        any.then(|| hasher.finish())
    }

    fn modified_fingerprint(&self) -> Option<u64> {
        use std::hash::{Hash, Hasher};
        let windows = self.shared.state().windows.clone();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut any = false;
        for slot in windows {
            let window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(workbench) = window.session.state::<Workbench>() else { continue };
            for document in workbench.documents().iter().filter(|document| document.is_dirty()) {
                any = true;
                document.path().hash(&mut hasher);
                document.version().hash(&mut hasher);
            }
        }
        any.then(|| hasher.finish())
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        let windows = self.shared.state().windows.clone();
        for slot in windows {
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let ProjectWindow { session, view } = &mut *window;
            match self.shared.registry.dispatch(session, "file.document.autosave", serde_json::json!({})) {
                Ok(done) if done == serde_json::json!({ "saved": [], "skipped": [], "failed": [] }) => continue,
                Ok(_) => {}
                Err(error) => view.report_at(ctx, error),
            }
            ctx.request_repaint_of(slot.drawn_in());
        }
        let modified = self.modified_fingerprint();
        self.shared.autosave.lock().unwrap_or_else(std::sync::PoisonError::into_inner).saved(modified);
    }

    fn follow_ui_zoom(&mut self, ctx: &egui::Context) {
        let (zoom, ide_zoom) =
            self.shared.settings.lock().map_or((1.0, 1.0), |settings| (settings.ui_zoom(), settings.ide_zoom_factor()));
        if self.ui_zoom == Some((zoom, ide_zoom)) {
            return;
        }
        if self.ui_zoom.is_none() {
            ctx.options_mut(|options| options.zoom_with_keyboard = false);
        }
        ctx.set_zoom_factor(zoom);
        super::ide_zoom::remember(ctx, zoom, ide_zoom);
        self.ui_zoom = Some((zoom, ide_zoom));
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        #[cfg(not(target_arch = "wasm32"))]
        crate::capture::tick(&ctx);
        self.remember_workspace(&ctx, false);
        self.shared.observe_input(&ctx);

        let windows = self.shared.state().windows.clone();
        let mut tool_viewports = Vec::new();
        #[cfg(target_os = "macos")]
        if windows.is_empty() && ctx.input(|input| input.viewport().focused.unwrap_or(false)) {
            let available = |action: &str| {
                matches!(
                    action,
                    "view.newProject.open"
                        | "view.newProject.empty"
                        | "view.project.fromVcs"
                        | "view.project.fromSources"
                        | "view.welcome.open"
                        | "view.ideSettings.import"
                ) || action.starts_with(super::main_menu::RECENT)
            };
            let recent = super::main_menu::recent_items(&self.shared.overview().recent);
            let name = self.shared.product.name;
            let lists = super::main_menu::Lists { layouts: &[], new_items: &[], recent: &recent, tool_windows: &[] };
            super::main_menu::update(&ctx, name, &available, &|_| None, &|_| false, lists);
            for action in super::main_menu::take_chosen(ctx.viewport_id()) {
                match (action.strip_prefix(super::main_menu::RECENT), action.as_str()) {
                    (Some(path), _) => self.shared.state().requests.push(Request::Open(PathBuf::from(path))),
                    (None, "view.project.fromVcs") => self.welcome.want_clone(false),
                    (None, "view.project.fromSources") => self.welcome.want_sources(),
                    (None, "view.welcome.open") => self.bring_welcome(&ctx, None),
                    (None, "view.ideSettings.import") => self.welcome.want_import(serde_json::json!({})),
                    (None, "view.newProject.empty") => {
                        self.welcome.want_new_project(Some(zen_workbench::new_project::EMPTY.to_owned()), false);
                    }
                    (None, _) => self.welcome.want_new_project(None, false),
                }
            }
        }
        self.splash.show(&ctx, &self.shared.product, !windows.is_empty());
        let overview = self.shared.overview();
        let active = overview
            .active_tab
            .as_ref()
            .and_then(|path| windows.iter().find(|slot| slot.tabbed && &slot.path == path))
            .cloned();
        match active {
            None => {
                self.set_root(&ctx, RootMode::Welcome, super::welcome::title(&self.shared.product));
                if let Some(notice) = self.pending_notice.take() {
                    self.welcome.post(ui, notice);
                }
                self.welcome_window = false;
                let requests = self.welcome_pass(ui, &overview);
                for request in requests {
                    self.handle(&ctx, request);
                }
                self.welcome_exit_dialog(ui);
            }
            Some(slot) => {
                let title = window_title(&slot.path, self.shared.product.name);
                self.set_root(&ctx, RootMode::Project, title);
                let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let ProjectWindow { session, view, .. } = &mut *window;
                view.overview = overview.clone();
                if let Some(notice) = self.pending_notice.take() {
                    view.warn(notice);
                }
                view.show(ui, &self.shared.registry, session);
                let mut requests = std::mem::take(&mut view.requests);
                if ctx.input(|input| input.viewport().close_requested()) && !self.shared.is_exiting() {
                    ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                    requests.push(Request::Close(slot.path.clone()));
                }
                let detached = view.detached_tools();
                let editors = view.detached_editors(session);
                let raised = view.window_to_raise(session);
                drop(window);
                self.shared.submit(&ctx, requests);
                tool_viewports.extend(self.detached_windows(&ctx, &slot, detached, &windows));
                tool_viewports.extend(self.editor_windows(&ctx, &slot, editors, raised));
                self.welcome_viewport(&ctx, &overview);
            }
        }

        self.areas_dialog(ui);
        self.where_to_open_dialog(ui);
        self.trust_dialog(ui);

        for slot in windows.iter().filter(|slot| !slot.tabbed) {
            let builder = ViewportBuilder::default()
                .with_title(window_title(&slot.path, self.shared.product.name))
                .with_app_id(format!("space.ifzenelse.{}", self.shared.product.id))
                .with_inner_size(PROJECT_SIZE)
                .with_min_inner_size([720.0, 480.0])
                .with_fullsize_content_view(true)
                .with_titlebar_shown(false)
                .with_title_shown(false);
            let shared = Arc::clone(&self.shared);
            let window = Arc::clone(&slot.window);
            let path = slot.path.clone();
            ctx.show_viewport_deferred(slot.viewport, builder, move |ui, _class| {
                shared.palette().apply(ui.ctx());
                shared.observe_input(ui.ctx());
                let overview = shared.overview();
                let mut window = window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let ProjectWindow { session, view, .. } = &mut *window;
                view.overview = overview;
                view.show(ui, &shared.registry, session);
                let mut requests = std::mem::take(&mut view.requests);
                if ui.ctx().input(|input| input.viewport().close_requested()) && !shared.is_exiting() {
                    requests.push(Request::Close(path.clone()));
                }
                drop(window);
                shared.submit(ui.ctx(), requests);
            });
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let ProjectWindow { session, view } = &mut *window;
            let detached = view.detached_tools();
            let editors = view.detached_editors(session);
            let raised = view.window_to_raise(session);
            drop(window);
            tool_viewports.extend(self.detached_windows(&ctx, slot, detached, &windows));
            tool_viewports.extend(self.editor_windows(&ctx, slot, editors, raised));
        }
        self.paths = windows.iter().map(|slot| slot.path.clone()).collect();
        self.tool_viewports = tool_viewports;
        self.follow_leaving(&ctx, &windows);
    }

    fn ide_focused(&self, ctx: &egui::Context, windows: &[Slot]) -> bool {
        let focused =
            |viewport: ViewportId| ctx.input_for(viewport, |input| input.viewport().focused.unwrap_or(input.focused));
        focused(ViewportId::ROOT)
            || windows.iter().any(|other| !other.tabbed && focused(other.viewport))
            || self.tool_viewports.iter().any(|viewport| focused(*viewport))
    }

    fn follow_leaving(&mut self, ctx: &egui::Context, windows: &[Slot]) {
        let focused = self.ide_focused(ctx, windows);
        let now = ctx.input(|input| input.time);
        let leaving = self.shared.leaving.lock().unwrap_or_else(std::sync::PoisonError::into_inner).poll(now, focused);
        match leaving {
            super::autosave::Idle::Due => self.shared.submit(ctx, vec![Request::Autosave]),
            super::autosave::Idle::In(left) => {
                ctx.request_repaint_after_for(std::time::Duration::from_secs_f64(left), ViewportId::ROOT);
            }
            super::autosave::Idle::Waiting => {}
        }
    }

    fn welcome_pass(&mut self, ui: &mut Ui, overview: &Overview) -> Vec<Request> {
        let registry = &self.shared.registry;
        let session = &mut self.welcome_session;
        let agent = self.welcome.driven_by_agent();
        let mut dispatch = |id: &str, params| {
            if agent {
                zen_core::macros::as_agent(session, |session| theme_dispatch(registry, session, id, params))
            } else {
                theme_dispatch(registry, session, id, params)
            }
        };
        self.welcome.show(ui, overview, &mut dispatch)
    }

    fn bring_welcome(&mut self, ctx: &egui::Context, section: Option<WelcomeSection>) {
        if let Some(section) = section {
            self.welcome.section = section;
        }
        let behind_project = self.shared.state().active_tab.is_some();
        self.welcome_window = behind_project;
        ctx.send_viewport_cmd_to(
            if behind_project { welcome_viewport() } else { ViewportId::ROOT },
            ViewportCommand::Focus,
        );
        ctx.request_repaint();
    }

    fn welcome_viewport(&mut self, ctx: &egui::Context, overview: &Overview) {
        if !self.welcome_window {
            return;
        }
        let builder = ViewportBuilder::default()
            .with_title(super::welcome::title(&self.shared.product))
            .with_app_id(format!("space.ifzenelse.{}", self.shared.product.id))
            .with_inner_size(WELCOME_SIZE)
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false);
        let (requests, closed) = ctx.show_viewport_immediate(welcome_viewport(), builder, |ui, _class| {
            self.shared.palette().apply(ui.ctx());
            let requests = self.welcome_pass(ui, overview);
            (requests, ui.ctx().input(|input| input.viewport().close_requested()))
        });
        let opened = requests.iter().any(|request| matches!(request, Request::Open(_) | Request::OpenWindow(_)));
        if closed || opened {
            self.welcome_window = false;
        }
        for request in requests {
            self.handle(ctx, request);
        }
    }

    fn detached_windows(
        &self,
        ctx: &egui::Context,
        slot: &Slot,
        detached: Vec<Detached>,
        windows: &[Slot],
    ) -> Vec<ViewportId> {
        if detached.is_empty() {
            return Vec::new();
        }
        let ide_focused = self.ide_focused(ctx, windows);
        let main = slot.drawn_in();
        let mut viewports = Vec::new();
        for tool in detached {
            let viewport = tool_viewport(&slot.path, tool.id, tool.mode);
            let builder = detached_builder(&tool, self.shared.product.id, ide_focused);
            let shared = Arc::clone(&self.shared);
            let window = Arc::clone(&slot.window);
            let (id, size) = (tool.id, tool.size);
            ctx.show_viewport_deferred(viewport, builder, move |ui, class| {
                shared.palette().apply(ui.ctx());
                let mut window = window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let ProjectWindow { session, view } = &mut *window;
                let changed =
                    view.detached_tool_window(ui, id, (ViewClass::from(class), size), &shared.registry, session);
                let requests = std::mem::take(&mut view.requests);
                drop(window);
                shared.submit(ui.ctx(), requests);
                if changed {
                    ui.ctx().request_repaint_of(main);
                }
            });
            viewports.push(viewport);
        }
        viewports
    }

    fn editor_windows(
        &self,
        ctx: &egui::Context,
        slot: &Slot,
        editors: Vec<super::editor_windows::DetachedEditor>,
        raised: Option<Option<zen_workbench::groups::GroupId>>,
    ) -> Vec<ViewportId> {
        let main = slot.drawn_in();
        if let Some(raised) = raised {
            let viewport = raised.map_or(main, |group| editor_viewport(&slot.path, group));
            ctx.send_viewport_cmd_to(viewport, ViewportCommand::Focus);
        }
        let mut viewports = Vec::new();
        for editor in editors {
            let viewport = editor_viewport(&slot.path, editor.group);
            let builder = super::editor_windows::editor_window_builder(&editor, self.shared.product.id);
            let shared = Arc::clone(&self.shared);
            let window = Arc::clone(&slot.window);
            let group = editor.group;
            ctx.show_viewport_deferred(viewport, builder, move |ui, class| {
                shared.palette().apply(ui.ctx());
                shared.observe_input(ui.ctx());
                let size = egui::vec2(800.0, 600.0);
                let mut window = window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let ProjectWindow { session, view } = &mut *window;
                let changed =
                    view.detached_editor_window(ui, group, (ViewClass::from(class), size), &shared.registry, session);
                let requests = std::mem::take(&mut view.requests);
                drop(window);
                shared.submit(ui.ctx(), requests);
                if changed {
                    ui.ctx().request_repaint_of(main);
                }
            });
            viewports.push(viewport);
        }
        viewports
    }

    fn set_root(&mut self, ctx: &egui::Context, mode: RootMode, title: String) {
        if self.root_mode != Some(mode) {
            if self.root_mode.is_some() {
                let [width, height] = if mode == RootMode::Welcome { WELCOME_SIZE } else { PROJECT_SIZE };
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(width, height)));
            }
            self.root_mode = Some(mode);
        }
        if self.root_title != title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.root_title = title;
        }
    }

    fn handle(&mut self, ctx: &egui::Context, request: Request) {
        match request {
            Request::Open(path) if self.asks_where_to_open(&path) => self.asking_where = Some(path),
            Request::Open(path) => self.open(Some(ctx), &path),
            Request::OpenWithoutAsking(path) => self.open_without_asking(ctx, &path),
            Request::OpenWindow(path) => self.open_in(Some(ctx), &path, true),
            Request::AskTrust(path) => {
                self.ask(super::trust_dialog::Question::of_open_project(path));
                ctx.request_repaint();
            }
            Request::Focus(path) => self.focus(ctx, &path),
            Request::MoveTab(path, to) => {
                move_tabbed(&mut self.shared.state().windows, &path, to);
                ctx.request_repaint();
            }
            Request::Close(path) => {
                if self.save_projects(ctx, Some(&path)).is_err() {
                    return;
                }
                let mut state = self.shared.state();
                let tabs: Vec<PathBuf> =
                    state.windows.iter().filter(|slot| slot.tabbed).map(|slot| slot.path.clone()).collect();
                if state.active_tab.as_ref() == Some(&path) {
                    let index = tabs.iter().position(|tab| tab == &path).unwrap_or(0);
                    state.active_tab =
                        tabs.get(index + 1).or_else(|| index.checked_sub(1).and_then(|i| tabs.get(i))).cloned();
                }
                state.windows.retain(|slot| slot.path != path);
                state.recent.closed(&path);
                save(&state);
            }
            Request::RemoveRecent(path) => {
                let mut state = self.shared.state();
                state.recent.remove(&path);
                save(&state);
            }
            Request::Settings(settings) => self.apply_settings(ctx, *settings),
            Request::Update => {
                if let Some(updater) = &mut self.updater {
                    let _ = updater.update();
                }
            }
            Request::DismissUpdate(version) => {
                if let Some(updater) = &mut self.updater {
                    updater.dismiss(&version);
                }
            }
            Request::Welcome(section) => self.bring_welcome(ctx, section),
            Request::Layouts(saved) => {
                if let Some(file) = &self.shared.layouts_file
                    && let Err(error) = saved.save(file)
                {
                    #[allow(clippy::print_stderr, reason = "a file the IDE cannot write is reported on the terminal")]
                    {
                        eprintln!("cannot save the layouts to {}: {error}", file.display());
                    }
                }
                if let Ok(mut layouts) = self.shared.layouts.lock() {
                    *layouts = *saved;
                }
            }
            Request::Restart if self.headless => {}
            Request::Restart => {
                self.restart_saving(ctx, None);
            }
            Request::UpdateInstalled(version) => {
                if self.save_projects(ctx, None).is_err() {
                    self.notice(t!("ide-update-restart-unsaved", version = version));
                } else if let Err(error) = self.restart_now(ctx) {
                    self.notice(t!("ide-update-restart-failed", version = version, detail = error.to_string()));
                }
            }
            Request::RestartAsking(ask, invalidate) => self.restart_asking(ctx, ask, invalidate),
            Request::Exit { confirmed } => {
                self.exit(ctx, confirmed);
            }
            Request::Autosave => {
                let on = self.shared.settings.lock().is_ok_and(|settings| settings.autosave_on_frame_deactivation);
                if on && !self.shared.is_exiting() {
                    self.autosave(ctx);
                }
            }
        }
    }

    fn restart_asking(
        &mut self,
        ctx: &egui::Context,
        ask: RestartAsk,
        invalidate: Option<zen_workbench::maintenance::InvalidateParams>,
    ) {
        if self.headless {
            return;
        }
        let running = self.running_work();
        let wanted = match ask {
            RestartAsk::Person | RestartAsk::Always => true,
            RestartAsk::IfWorkRuns => !running.is_empty(),
            RestartAsk::Never => false,
        };
        let person = ask == RestartAsk::Person;
        let confirm = person || self.shared.settings.lock().map_or(true, |settings| settings.confirm_exit);
        match self.front_window(ctx).filter(|_| wanted && confirm) {
            Some(slot) => {
                if slot.tabbed {
                    self.shared.state().active_tab = Some(slot.path.clone());
                }
                let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                window.view.ask_restart(running, invalidate);
                drop(window);
                ctx.send_viewport_cmd_to(slot.drawn_in(), ViewportCommand::Focus);
                ctx.request_repaint_of(slot.drawn_in());
            }
            None if person => {}
            None => self.restart_saving(ctx, invalidate),
        }
    }

    fn restart_saving(
        &mut self,
        ctx: &egui::Context,
        invalidate: Option<zen_workbench::maintenance::InvalidateParams>,
    ) {
        if self.save_projects(ctx, None).is_err() {
            return;
        }
        if let Some(params) = invalidate
            && self.schedule_invalidation(ctx, params).is_err()
        {
            return;
        }
        if let Err(error) = self.restart_now(ctx) {
            self.notice(t!("ide-restart-failed", detail = error.to_string()));
        }
    }

    fn restart_now(&mut self, ctx: &egui::Context) -> std::io::Result<()> {
        self.remember_workspace(ctx, true);
        let bundle = UpdateWatch::for_this_process().map(|watch| watch.bundle);
        restart(&self.shared, bundle.as_deref())
    }

    fn schedule_invalidation(
        &mut self,
        ctx: &egui::Context,
        params: zen_workbench::maintenance::InvalidateParams,
    ) -> Result<(), ()> {
        let Some(slot) = self.front_window(ctx) else { return Err(()) };
        let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let ProjectWindow { session, view } = &mut *window;
        let params = serde_json::to_value(params).map_err(|_| ())?;
        self.shared.registry.dispatch(session, "ide.caches.invalidate", params).map(|_| ()).map_err(|error| {
            view.report_at(ctx, error);
            ctx.request_repaint_of(slot.drawn_in());
        })
    }

    fn save_projects(&mut self, ctx: &egui::Context, project: Option<&Path>) -> Result<(), String> {
        let windows = self.shared.state().windows.clone();
        let mut failed = Vec::new();
        for slot in windows.iter().filter(|slot| project.is_none_or(|project| slot.path == project)) {
            let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let ProjectWindow { session, view } = &mut *window;
            if let Err(error) = self.shared.registry.dispatch(session, "file.document.saveAll", serde_json::json!({})) {
                failed.push(error.to_string());
                view.report_at(ctx, error);
                ctx.request_repaint_of(slot.drawn_in());
            }
        }
        if failed.is_empty() { Ok(()) } else { Err(failed.join("\n")) }
    }

    fn running_work(&self) -> Vec<zen_workbench::exit::RunningWork> {
        let windows = self.shared.state().windows.clone();
        windows
            .iter()
            .flat_map(|slot| {
                let window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                zen_workbench::exit::running_work(&window.session)
            })
            .collect()
    }

    fn exit(&mut self, ctx: &egui::Context, confirmed: bool) -> (bool, Vec<zen_workbench::exit::RunningWork>) {
        let running = self.running_work();
        let ask =
            !confirmed && !self.headless && self.shared.settings.lock().map_or(true, |settings| settings.confirm_exit);
        if !ask {
            if self.save_projects(ctx, None).is_err() && !self.ending {
                return (false, running);
            }
            self.shared.exiting.store(true, std::sync::atomic::Ordering::SeqCst);
            self.shared.system_quit.allow();
            self.exit_at = Some(exit_clock() + EXIT_DELAY);
            ctx.request_repaint();
            return (true, running);
        }
        match self.front_window(ctx) {
            Some(slot) => {
                if slot.tabbed {
                    self.shared.state().active_tab = Some(slot.path.clone());
                }
                let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                window.view.ask_exit(running.clone());
                drop(window);
                ctx.send_viewport_cmd_to(slot.drawn_in(), ViewportCommand::Focus);
                ctx.request_repaint_of(slot.drawn_in());
            }
            None => self.welcome_exit = true,
        }
        ctx.request_repaint();
        (false, running)
    }

    fn follow_system_quit(&mut self, ctx: &egui::Context) {
        if !self.system_quit_hooked && std::ptr::eq(self.shared.system_quit, zen_macos::quit_gate()) {
            let wake = ctx.clone();
            self.system_quit_hooked = zen_macos::intercept_system_quit(move || wake.request_repaint());
        }
        if self.shared.system_quit.take_request() {
            self.shared.state().requests.push(Request::Exit { confirmed: false });
        }
    }

    fn front_window(&self, ctx: &egui::Context) -> Option<Slot> {
        let windows = self.shared.state().windows.clone();
        let active = self.shared.overview().active_tab;
        let focused = |slot: &&Slot| {
            let shown = !slot.tabbed || active.as_ref() == Some(&slot.path);
            shown && ctx.input_for(slot.drawn_in(), |input| input.viewport().focused.unwrap_or(false))
        };
        windows
            .iter()
            .find(focused)
            .or_else(|| windows.iter().find(|slot| slot.tabbed && active.as_ref() == Some(&slot.path)))
            .or_else(|| windows.first())
            .cloned()
    }

    fn follow_exit(&mut self, ctx: &egui::Context) {
        let Some(at) = self.exit_at else { return };
        let now = exit_clock();
        if now < at {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(at - now));
            return;
        }
        if !self.runs_stopped {
            self.runs_stopped = true;
            self.shared.stop_work();
            for slot in self.shared.state().windows.clone() {
                let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                zen_workbench::database::recovery::finish(&mut window.session);
            }
        }
        if self.headless {
            self.finished = true;
            return;
        }
        self.remember_workspace(ctx, true);
        ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Close);
        ctx.request_repaint();
    }

    fn welcome_exit_dialog(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let exit = crate::keymap::current().keystrokes("view.ide.exit");
        let pressed = ctx.input(|input| {
            input.events.iter().any(|event| match event {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    exit.contains(&crate::keymap::keystroke(*key, *modifiers))
                }
                _ => false,
            })
        });
        if pressed && !self.welcome_exit {
            self.exit(&ctx, false);
        }
        if !self.welcome_exit {
            return;
        }
        let palette = self.shared.palette();
        let exit = super::exit_dialog::Ending::Exit;
        match super::exit_dialog::show(ui, &palette, exit, self.shared.product.name, &[]) {
            Some(super::exit_dialog::Choice::Exit) => {
                self.welcome_exit = false;
                self.exit(&ctx, true);
            }
            Some(super::exit_dialog::Choice::Cancel) => self.welcome_exit = false,
            None => {}
        }
    }

    fn handed_over(&mut self, ctx: &egui::Context, paths: &[PathBuf]) {
        for path in paths {
            self.open(Some(ctx), path);
        }
        if self.headless {
            return;
        }
        let shown = paths.last().and_then(|path| {
            let path = file_at_line(path).map_or_else(|| path.clone(), |(file, _, _)| file);
            let path = std::fs::canonicalize(&path).ok()?;
            self.project_of(&path)
        });
        let viewport = match shown {
            Some(project) => {
                self.focus(ctx, &project);
                self.shared.state().windows.iter().find(|slot| slot.path == project).map(Slot::drawn_in)
            }
            None => self.front_window(ctx).map(|slot| slot.drawn_in()),
        };
        let viewport = viewport.unwrap_or(ViewportId::ROOT);
        ctx.send_viewport_cmd_to(viewport, ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd_to(viewport, ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn hold_start(&mut self, paths: &[PathBuf], held: zen_workbench::handoff::Waiting) {
        let files: Vec<PathBuf> = paths
            .iter()
            .map(|path| file_at_line(path).map_or_else(|| path.clone(), |(file, ..)| file))
            .filter(|path| path.is_file())
            .filter_map(|path| std::fs::canonicalize(path).ok())
            .collect();
        if !files.is_empty() {
            self.waiting_starts.push(WaitingStart { files, seen: Vec::new(), _held: held });
        }
    }

    fn open_documents(&self) -> Vec<PathBuf> {
        let windows = self.shared.state().windows.clone();
        let mut open = Vec::new();
        for slot in &windows {
            let window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(workbench) = window.session.state::<Workbench>() {
                open.extend(workbench.documents().iter().map(|document| document.path().to_owned()));
            }
        }
        open
    }

    fn follow_waiting_starts(&mut self, ctx: &egui::Context) {
        if self.waiting_starts.is_empty() {
            return;
        }
        ctx.request_repaint_after(WAITING_STARTS_CHECK);
        let open = self.open_documents();
        let asking = self.shared.overview().trust_question;
        self.waiting_starts.retain_mut(|start| {
            for file in &start.files {
                if open.contains(file) && !start.seen.contains(file) {
                    start.seen.push(file.clone());
                }
            }
            start.files.iter().any(|file| open.contains(file) || (asking && !start.seen.contains(file)))
        });
    }

    fn project_of(&self, path: &Path) -> Option<PathBuf> {
        let state = self.shared.state();
        state
            .windows
            .iter()
            .filter(|slot| path.starts_with(&slot.path))
            .max_by_key(|slot| slot.path.components().count())
            .map(|slot| slot.path.clone())
    }

    fn focus(&self, ctx: &egui::Context, path: &Path) {
        let mut state = self.shared.state();
        let Some(slot) = state.windows.iter().find(|slot| slot.path == path).cloned() else { return };
        if slot.tabbed {
            state.active_tab = Some(slot.path.clone());
        }
        drop(state);
        ctx.send_viewport_cmd_to(slot.drawn_in(), ViewportCommand::Focus);
        ctx.request_repaint();
    }

    fn open(&mut self, ctx: Option<&egui::Context>, path: &Path) {
        self.open_in(ctx, path, false);
    }

    fn open_without_asking(&mut self, ctx: &egui::Context, path: &Path) {
        let asks = std::mem::replace(&mut self.asks_trust, false);
        self.open(Some(ctx), path);
        self.asks_trust = asks;
    }

    fn asks_where_to_open(&self, path: &Path) -> bool {
        let asks = self
            .shared
            .settings
            .lock()
            .is_ok_and(|settings| settings.project_opening() == super::settings::OpenProjectIn::Ask);
        let state = self.shared.state();
        asks && !self.headless
            && path.is_dir()
            && state.windows.iter().any(|slot| slot.tabbed)
            && !state.windows.iter().any(|slot| slot.path == path)
    }

    fn where_to_open_dialog(&mut self, ui: &mut Ui) {
        let Some(path) = self.asking_where.clone() else { return };
        let palette = self.shared.palette();
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let Some(answer) = super::new_project_dialog::where_to_open(ui, &palette, &name) else { return };
        self.asking_where = None;
        if let Some(own_window) = answer {
            self.open_in(Some(ui.ctx()), &path, own_window);
        }
    }

    fn folder_to_ask_about(&self, path: &Path) -> Option<PathBuf> {
        if !self.asks_trust {
            return None;
        }
        let path = file_at_line(path).map_or_else(|| path.to_owned(), |(file, ..)| file);
        let path = std::fs::canonicalize(&path).ok()?;
        let folder = if path.is_file() {
            self.project_of(&path).map_or_else(|| project_for_file(&path), |_| None)?
        } else {
            path
        };
        let asked = folder.is_dir()
            && !self.shared.overview().open.contains(&folder)
            && !self.trust_answered.contains(&folder)
            && !zen_workbench::trust::is_trusted(&self.trust_env, &folder);
        asked.then_some(folder)
    }

    fn ask(&mut self, question: super::trust_dialog::Question) {
        if !self.trust_questions.iter().any(|asked| asked.folder == question.folder) {
            self.trust_questions.push_back(question);
        }
        self.shared.state().trust_question = true;
    }

    fn trust_session(&self) -> Session {
        let mut session = Session::new();
        session.set_state(self.trust_env.clone());
        session.set_state(self.shared.language_servers.clone());
        session
    }

    fn answer_trust(&self, question: &super::trust_dialog::Question) -> Result<(), String> {
        let mut session = self.trust_session();
        let registry = &self.shared.registry;
        let dispatched = match question.parent().filter(|_| question.all_in_parent) {
            Some(parent) => {
                let mut locations = zen_workbench::trust::locations(&self.trust_env);
                locations.push(parent.to_owned());
                registry.dispatch(
                    &mut session,
                    "project.trustedLocations.set",
                    serde_json::json!({ "locations": locations }),
                )
            }
            None => {
                registry.dispatch(&mut session, "project.trust.grant", serde_json::json!({ "path": question.folder }))
            }
        };
        dispatched.map(|_| ()).map_err(|error| match error {
            zen_core::DispatchError::Failed { error, .. } => error.message,
            other => other.to_string(),
        })
    }

    fn settle_trust_questions(&mut self, ctx: &egui::Context) {
        let env = self.trust_env.clone();
        while let Some(question) =
            self.trust_questions.front().filter(|asked| zen_workbench::trust::is_trusted(&env, &asked.folder)).cloned()
        {
            self.trust_questions.pop_front();
            if !question.already_open {
                self.open_in(Some(ctx), &question.then_open, question.own_window);
            }
        }
        self.shared.state().trust_question = !self.trust_questions.is_empty();
    }

    fn trust_dialog(&mut self, ui: &mut Ui) {
        use super::trust_dialog::Answer;
        self.settle_trust_questions(ui.ctx());
        let palette = self.shared.palette();
        let Some(question) = self.trust_questions.front_mut() else { return };
        let Some(answer) = super::trust_dialog::show(ui, &palette, question) else { return };
        let question = question.clone();
        match answer {
            Answer::Trust => match self.answer_trust(&question) {
                Ok(()) => self.settle_trust_questions(ui.ctx()),
                Err(error) => {
                    if let Some(front) = self.trust_questions.front_mut() {
                        front.error = Some(error);
                    }
                }
            },
            Answer::Preview => {
                self.trust_questions.pop_front();
                self.trust_answered.insert(question.folder.clone());
                self.open_in(Some(ui.ctx()), &question.then_open, question.own_window);
            }
            Answer::Leave => {
                self.trust_questions.pop_front();
            }
        }
        self.shared.state().trust_question = !self.trust_questions.is_empty();
        ui.ctx().request_repaint();
    }

    fn open_in(&mut self, ctx: Option<&egui::Context>, path: &Path, own_window: bool) {
        if let Some(folder) = self.folder_to_ask_about(path) {
            self.ask(super::trust_dialog::Question::opening(folder, path.to_owned(), own_window));
            if let Some(ctx) = ctx {
                ctx.send_viewport_cmd_to(ViewportId::ROOT, ViewportCommand::Focus);
                ctx.request_repaint();
            }
            return;
        }
        if let Some((file, line, column)) = file_at_line(path) {
            self.open_in(ctx, &file, own_window);
            let file = std::fs::canonicalize(&file).unwrap_or(file);
            let project = self.project_of(&file);
            let windows = self.shared.state().windows.clone();
            for slot in windows.iter().filter(|slot| Some(&slot.path) == project.as_ref()) {
                let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                let params = serde_json::json!({ "line": line, "column": column });
                let _ = self.shared.registry.dispatch(&mut window.session, "editor.caret.goTo", params);
            }
            return;
        }
        let opened_as = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        if path.is_file() {
            let Some(folder) = path.parent() else { return };
            let project = match self.project_of(&path) {
                Some(project) => {
                    if let Some(ctx) = ctx {
                        self.focus(ctx, &project);
                    }
                    project
                }
                None => {
                    let home = project_for_file(&path).unwrap_or_else(|| folder.to_owned());
                    let spelled = project_for_file(&opened_as);
                    self.open_in(ctx, spelled.as_deref().unwrap_or(&home), own_window);
                    home
                }
            };
            let windows = self.shared.state().windows.clone();
            for slot in windows.iter().filter(|slot| slot.path == project) {
                let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                {
                    let params = serde_json::json!({ "path": path });
                    let _ = self.shared.registry.dispatch(&mut window.session, "file.document.open", params);
                }
            }
            return;
        }
        if self.shared.overview().open.contains(&path) {
            if let Some(ctx) = ctx {
                self.focus(ctx, &path);
            }
            return;
        }
        let mut session = Session::new();
        if self.trust_answered.remove(&path) {
            session.set_state(zen_workbench::trust::Answered);
        }
        let settings = self.shared.settings.lock().map(|settings| settings.clone()).unwrap_or_default();
        settings.apply_to_session(session.settings_mut());
        for state in &self.shared.window_states {
            state(&mut session);
        }
        zen_workbench::vcs::workspace::install(&mut session);
        if let Some(toolchain) = &self.shared.toolchain {
            session.set_state(zen_modules::WindowToolchain(Arc::clone(toolchain)));
        }
        session.set_state(self.shared.language_servers.clone());
        session.set_state(self.shared.local_history.clone());
        session.set_state(zen_workbench::internal_errors::InternalErrors::global().clone());
        session.set_state(self.shared.templates.clone());
        session.set_state(self.shared.file_templates.clone());
        session.set_state(self.shared.new_projects.clone());
        session.set_state(self.shared.file_types.clone());
        session.set_state(self.shared.macros.clone());
        session.set_state(self.shared.external_tools.clone());
        session.set_state(self.shared.file_watchers.clone());
        let data = self.shared.state().recent_file.as_ref().and_then(|file| Some(file.parent()?.to_owned()));
        if let Some(data) = data {
            session.state_mut::<zen_workbench::http::HttpClient>().set_data_dir(data);
        }
        session.set_state(self.shared.clipboard_history.clone());
        session.set_state(self.shared.caffeine.clone());
        session.set_state(self.shared.search_history.clone());
        session.set_state(self.shared.find_history.clone());
        session
            .state_mut::<zen_modules::search::SearchContributors>()
            .add(Arc::new(super::search_everywhere::ActionsContributor::new(Arc::clone(&self.shared.registry))));
        session
            .state_mut::<zen_modules::search::SearchContributors>()
            .add(Arc::new(super::search_everywhere::CommandsContributor::new(Arc::clone(&self.shared.registry))));
        if super::system_clipboard::ArboardClipboard::wanted() && !self.headless {
            zen_workbench::file_clipboard::install(&mut session, Arc::new(super::system_clipboard::ArboardClipboard));
        }
        session.state_mut::<zen_workbench::project_model::ProjectModels>().set_providers(
            self.shared.product.project_models.clone(),
            toolchain_model_env(self.shared.toolchain.as_ref()),
        );
        session.state_mut::<zen_workbench::language::structure::Outliners>().set(self.shared.product.outliners.clone());
        let theme = Arc::downgrade(&self.shared);
        session.state_mut::<zen_workbench::database::diagrams::Diagrams>().set_renderer(Arc::new(
            super::database_view::ThemeRenderer(Arc::new(move || {
                theme.upgrade().map_or_else(IdePalette::zen_dark, |shared| shared.palette())
            })),
        ));
        let data = self.shared.state().recent_file.as_deref().and_then(Path::parent).map(Path::to_owned);
        let caches = data.as_ref().map(|data| data.join("caches"));
        if let Some(data) = &data {
            let product = &self.shared.product;
            session.state_mut::<zen_workbench::diagnostics::Diagnostics>().set_product(
                data.clone(),
                product.name,
                product.version,
            );
            session.state_mut::<zen_workbench::maintenance::Maintenance>().set_data_dir(data.clone());
            session.state_mut::<zen_workbench::vcs::history::GitHistory>().set_folder(data.join("vcs-history"));
            session.state_mut::<zen_workbench::vcs::changelists::LocalChanges>().set_folder(data.clone());
            session.state_mut::<zen_workbench::vcs::avatars::Avatars>().set_folder(data.join("caches").join("avatars"));
            session.state_mut::<zen_workbench::search_scopes::SearchScopes>().set_scratches(data.join("scratches"));
            session
                .state_mut::<zen_workbench::spelling::Spelling>()
                .set_application_dictionary(data.join("dictionaries").join("application.dic"));
            session
                .state_mut::<zen_workbench::inspection_profiles::InspectionProfiles>()
                .set_ide_folder(data.join("inspectionProfiles"));
            session.state_mut::<zen_workbench::scratch::Scratches>().set_data_dir(data.clone());
            session.state_mut::<zen_workbench::workspaces::Workspaces>().set_data_dir(data.clone());
            session
                .state_mut::<zen_workbench::terminal::Terminals>()
                .set_host(zen_terminal::Host::installed(data.join("terminal")));
            session.state_mut::<zen_workbench::test_history::TestHistory>().set_data_dir(data.join("test-history"));
            session
                .state_mut::<zen_workbench::database::Database>()
                .set_application_name(format!("Zen {}", self.shared.product.name));
            session.state_mut::<zen_workbench::plugin_projects::PluginProjects>().set_data_dir(data.clone());
            let previews = session.state_mut::<zen_workbench::preview::Previews>();
            previews.set_data_dir(data.clone());
            let layout = previews.mode(zen_workbench::preview::PreviewKind::Markdown);
            if let Ok(mut settings) = self.shared.settings.lock() {
                settings.markdown.preview_layout = layout;
            }
            session.state_mut::<zen_workbench::markdown::RemoteImages>().set_data_dir(data.clone());
            zen_workbench::jdk::set_data_dir(data.clone());
        }
        let plugins = Arc::clone(&self.shared.plugins);
        session
            .state_mut::<zen_workbench::scratch::Scratches>()
            .set_languages(Arc::new(move || scratch_languages(&plugins)));
        let plugins = Arc::clone(&self.shared.plugins);
        session
            .state_mut::<zen_workbench::terminal::Terminals>()
            .set_plugin_agents(Arc::new(move || plugins.terminal_agents()));
        let plugins = Arc::clone(&self.shared.plugins);
        session
            .state_mut::<zen_run::test_support::TestSupports>()
            .set_plugins(Arc::new(move || plugins.test_supports()));
        let plugins = Arc::clone(&self.shared.plugins);
        session.state_mut::<zen_workbench::project_index::Indexes>().set_plugins(Arc::new(move || plugins.features()));
        let (shared, own) = (Arc::downgrade(&self.shared), path.clone());
        session
            .state_mut::<zen_workbench::scratch::Scratches>()
            .set_open_elsewhere(Arc::new(move |file| open_elsewhere(&shared, &own, file)));
        let dependencies = session.state_mut::<zen_workbench::dependencies::Dependencies>().clone();
        dependencies.set_transport(Arc::new(zen_workbench::dependencies::sources::Http));
        let mut inspectors = self.shared.product.inspectors.clone();
        inspectors.push(dependencies.inspector());
        session.state_mut::<zen_workbench::inspections::Inspections>().set_inspectors(inspectors, caches.as_deref());
        session.state_mut::<zen_workbench::spelling::Spelling>().activate();
        session.set_state(self.trust_env.clone());
        zen_workbench::before_launch::register_type(&mut session, zen_workbench::artifacts::task_type());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let plugins = Arc::clone(&self.shared.plugins);
            session
                .state_mut::<zen_workbench::database::Database>()
                .set_more_drivers(Arc::new(move || plugins.database_drivers()));
        }
        let plugins = Arc::clone(&self.shared.plugins);
        session.state_mut::<zen_workbench::plugins::PluginFeatures>().set_source(Arc::new(move || plugins.features()));
        let plugins = Arc::clone(&self.shared.plugins);
        session
            .state_mut::<zen_workbench::plugin_projects::PluginProjects>()
            .set_source(Arc::new(move || plugins.project_support()));
        zen_workbench::dependencies::service(&mut session);
        let (plugins, product) = (Arc::clone(&self.shared.plugins), Arc::clone(&self.shared.product));
        session
            .state_mut::<zen_workbench::brackets::Highlighters>()
            .set_source(Arc::new(move |name| plugins.highlighter(&product.highlighters, name)));
        let (plugins, product) = (Arc::clone(&self.shared.plugins), Arc::clone(&self.shared.product));
        session
            .state_mut::<zen_workbench::brackets::Highlighters>()
            .set_injectors(Arc::new(move |name| injectors_for(&plugins, &product.highlighters, name)));
        session
            .state_mut::<zen_workbench::project_settings::ProjectSettingsState>()
            .set_providers(settings_providers());
        session.state_mut::<zen_workbench::run::Runs>().set_providers(
            self.shared.product.run_providers.clone(),
            toolchain_run_env(self.shared.toolchain.as_ref()),
        );
        session
            .state_mut::<zen_workbench::run::Runs>()
            .set_debugger(toolchain_debugger(self.shared.toolchain.as_ref()));
        zen_workbench::before_launch::register_type(&mut session, zen_workbench::file_watchers::task_type());
        let opened =
            self.shared.registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": path }));
        if opened.is_ok() {
            let _ = self.shared.registry.dispatch(&mut session, "project.model.refresh", serde_json::json!({}));
        }
        let mut view = IdeView::new(Arc::clone(&self.shared.product), Arc::clone(&self.shared.plugins));
        view.help = self.shared.help.clone();
        view.set_theme(&self.shared.theme());
        view.expanded.insert(path.clone());
        view.opened_as = (opened_as != path).then_some(opened_as);
        if opened.is_ok()
            && !self.no_restore
            && let Some(layout) = self.workspace.projects.get(&path).cloned()
        {
            workspace::restore(&mut view, &self.shared.registry, &mut session, &layout);
        }
        if opened.is_ok() {
            workspace::restore_repository(&self.shared.registry, &mut session, &self.workspace.repositories);
            let workspaces = session.state_mut::<zen_workbench::workspaces::Workspaces>();
            for (project, layout) in self.workspace.projects.iter().filter(|(project, _)| **project != path) {
                workspaces.remember_project(project.clone(), layout.workspaces.clone());
            }
        }
        let mut state = self.shared.state();
        if opened.is_err() {
            state.recent.remove(&path);
            save(&state);
            return;
        }
        let viewport = ViewportId::from_hash_of(("project", &path));
        let in_own_window = self
            .shared
            .settings
            .lock()
            .is_ok_and(|settings| settings.project_opening() == super::settings::OpenProjectIn::NewWindow);
        let own_window = own_window || (in_own_window && state.windows.iter().any(|slot| slot.tabbed));
        let window = Arc::new(Mutex::new(ProjectWindow { session, view }));
        state.windows.push(Slot { path: path.clone(), viewport, tabbed: !own_window, window });
        if !own_window {
            state.active_tab = Some(path.clone());
        }
        state.recent.opened(&path, zen_workbench::recent::now());
        save(&state);
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
    }
}

fn theme_dispatch(
    registry: &Registry,
    session: &mut Session,
    id: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    if !id.starts_with("ui.theme.")
        && !id.starts_with("project.new.")
        && !id.starts_with("project.fromVcs.")
        && id != "project.fromSources.detect"
        && !id.starts_with("ui.keymap.")
        && !id.starts_with("plugins.")
        && !id.starts_with("settings.ide.")
        && !id.starts_with("editor.templates.")
        && !id.starts_with("file.templates.")
    {
        return Err(zen_core::DispatchError::UnknownCommand { id: id.to_owned() }.to_string());
    }
    registry.dispatch(session, id, params).map_err(|error| match error {
        zen_core::DispatchError::Failed { error, .. } => error.to_string(),
        other => other.to_string(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SavedTheme {
    chosen: Option<String>,
    follow_system: bool,
    dark: Option<String>,
    light: Option<String>,
}

impl SavedTheme {
    fn of(store: &ThemeStore) -> SavedTheme {
        let appearance = store.appearance();
        SavedTheme {
            chosen: (store.chosen() != ZEN_DARK_ID).then(|| store.chosen().to_owned()),
            follow_system: appearance.follow_system,
            dark: (appearance.dark != ZEN_DARK_ID).then_some(appearance.dark),
            light: (appearance.light != zen_theme::ZEN_LIGHT_ID).then_some(appearance.light),
        }
    }

    fn from_settings(settings: &IdeSettings) -> SavedTheme {
        SavedTheme {
            chosen: settings.theme.clone(),
            follow_system: settings.theme_follow_system,
            dark: settings.theme_dark.clone(),
            light: settings.theme_light.clone(),
        }
    }

    fn write(&self, settings: &mut IdeSettings) {
        settings.theme.clone_from(&self.chosen);
        settings.theme_follow_system = self.follow_system;
        settings.theme_dark.clone_from(&self.dark);
        settings.theme_light.clone_from(&self.light);
    }
}

fn theme_store(recent_file: Option<&Path>, settings: &IdeSettings) -> (SharedThemes, Option<String>) {
    let dir = recent_file.and_then(Path::parent).map(|data| data.join("themes"));
    let mut importers: Vec<Box<dyn ThemeImporter>> = zen_jb_import::importers();
    importers.push(Box::new(VsCodeImporter::new()));
    let mut store = ThemeStore::new(dir, Box::new(themes::DiskFiles), importers);
    let missing = |id: &str| zen_i18n::t!("workbench-theme-missing", id = id);
    let mut notices: Vec<String> = Vec::new();
    if let Some(id) = settings.theme.as_deref()
        && store.select(id).is_err()
    {
        notices.push(missing(id));
    }
    if settings.theme_follow_system {
        let usable = |id: &Option<String>| id.as_deref().filter(|id| store.theme(id).is_some()).map(str::to_owned);
        let (dark, light) = (usable(&settings.theme_dark), usable(&settings.theme_light));
        for id in [&settings.theme_dark, &settings.theme_light].into_iter().flatten() {
            if store.theme(id).is_none() {
                notices.push(missing(id));
            }
        }
        let _ = store.set_follow_system(true, dark.as_deref(), light.as_deref());
    }
    let notice = (!notices.is_empty()).then(|| notices.join("\n"));
    (Arc::new(Mutex::new(store)), notice)
}

fn language_servers(
    product: &IdeProduct,
    plugins: &Plugins,
    recent_file: Option<&Path>,
    toolchain: Option<&Arc<dyn zen_modules::LanguageToolchain>>,
) -> zen_workbench::language::servers::LanguageServers {
    let file = recent_file.and_then(|file| Some(file.parent()?.join("language-servers.json")));
    let tools: Arc<dyn zen_workbench::language::servers::ServerTools> = match toolchain {
        Some(toolchain) => Arc::clone(toolchain) as Arc<dyn zen_workbench::language::servers::ServerTools>,
        None => Arc::new(zen_workbench::language::servers::SystemTools),
    };
    let servers = zen_workbench::language::servers::LanguageServers::load(file, tools);
    servers.declare(product.language_servers.iter().cloned());
    servers.declare(
        plugins.entries().into_iter().filter(|entry| entry.enabled).flat_map(|entry| entry.plugin.language_servers),
    );
    for entry in plugins.entries().into_iter().filter(|entry| entry.enabled && !entry.plugin.server_plugins.is_empty())
    {
        servers.contribute(&entry.plugin.id, entry.plugin.server_plugins);
    }
    servers
}

fn keymap_store(recent_file: Option<&Path>, saved: Option<&str>) -> (SharedKeymaps, Option<String>) {
    let data = recent_file.and_then(Path::parent).map(Path::to_owned);
    let mut store = KeymapStore::new(data, zen_jb_import::keymap_importers());
    let notice = saved.and_then(|id| store.select(id).err().map(|_| zen_i18n::t!("workbench-keymap-missing", id = id)));
    (Arc::new(Mutex::new(store)), notice)
}

fn settings_providers() -> Vec<Arc<dyn zen_project::settings::SettingsProvider>> {
    vec![Arc::new(zen_jb_import::IdeaProject::new())]
}

fn injectors_for(
    plugins: &Plugins,
    builtin: &[Arc<dyn zen_syntax::Highlighter>],
    name: &str,
) -> Vec<Arc<dyn zen_syntax::inject::Injector>> {
    if !zen_db::inject::hosts(name) {
        return Vec::new();
    }
    let highlighter = plugins.highlighter(builtin, "fragment.sql");
    vec![Arc::new(zen_db::inject::SqlStrings::new(highlighter))]
}

fn scratch_languages(plugins: &Plugins) -> Vec<zen_workbench::scratch::ScratchLanguage> {
    plugins
        .entries()
        .into_iter()
        .filter(|entry| entry.enabled)
        .filter_map(|entry| {
            let extension = entry.plugin.extensions.first()?.clone();
            Some(zen_workbench::scratch::ScratchLanguage { id: entry.plugin.id, name: entry.plugin.name, extension })
        })
        .collect()
}

fn open_elsewhere(shared: &std::sync::Weak<Shared>, own: &Path, file: &Path) -> bool {
    let Some(shared) = shared.upgrade() else { return false };
    let Ok(state) = shared.state.try_lock() else { return true };
    let windows: Vec<Slot> = state.windows.iter().filter(|slot| slot.path != own).cloned().collect();
    drop(state);
    windows.iter().any(|slot| match slot.window.try_lock() {
        Ok(window) => window
            .session
            .state::<Workbench>()
            .is_some_and(|workbench| workbench.documents().iter().any(|document| document.path() == file)),
        Err(_) => true,
    })
}

pub(super) fn toolchain_model_env(
    toolchain: Option<&Arc<dyn zen_modules::LanguageToolchain>>,
) -> zen_workbench::model::ModelEnv {
    toolchain.map(|toolchain| toolchain.model_env()).unwrap_or_default()
}

pub(super) fn toolchain_run_env(toolchain: Option<&Arc<dyn zen_modules::LanguageToolchain>>) -> zen_run::RunEnv {
    toolchain.map(|toolchain| toolchain.run_env()).unwrap_or_default()
}

pub(super) fn toolchain_debugger(toolchain: Option<&Arc<dyn zen_modules::LanguageToolchain>>) -> PathBuf {
    toolchain.and_then(|toolchain| toolchain.debugger()).unwrap_or_default()
}

fn build_tool_settings_pages() -> [zen_modules::SettingsPage; 8] {
    use zen_workbench::maven::settings as maven;
    let page = |id, title, search, form, parent| zen_modules::SettingsPage {
        id,
        title,
        search,
        form,
        commands: &["maven.settings.", "maven.importing.", "maven.runner.", "maven.runningTests."],
        parent,
    };
    [
        page(maven::BUILD_TOOLS_PAGE, "build-tools-page", &["build-tools-page"], "buildTools.settings.form", "build"),
        page(
            maven::PAGE,
            "maven-settings-page",
            &[
                "maven-settings-offline",
                "maven-settings-update-snapshots",
                "maven-settings-output-level",
                "maven-settings-checksum",
                "maven-settings-fail-policy",
                "maven-settings-threads",
                "maven-settings-home",
                "maven-settings-user-settings",
                "maven-settings-local-repository",
            ],
            "maven.settings.form",
            maven::BUILD_TOOLS_PAGE,
        ),
        page(
            maven::IMPORTING_PAGE,
            "maven-settings-importing-page",
            &[
                "maven-settings-generated",
                "maven-settings-download",
                "maven-settings-download-sources",
                "maven-settings-download-docs",
                "maven-settings-importer-vm",
                "maven-settings-importer-jdk",
            ],
            "maven.importing.form",
            maven::PAGE,
        ),
        page(
            maven::RUNNER_PAGE,
            "maven-settings-runner-page",
            &[
                "maven-settings-delegate",
                "maven-settings-runner-vm",
                "maven-settings-runner-jre",
                "maven-settings-runner-env",
                "maven-settings-runner-properties",
                "maven-settings-skip-tests",
            ],
            "maven.runner.form",
            maven::PAGE,
        ),
        page(
            maven::TESTS_PAGE,
            "maven-settings-tests-page",
            &["maven-settings-tests-pass"],
            "maven.runningTests.form",
            maven::PAGE,
        ),
        zen_modules::SettingsPage {
            id: zen_workbench::gradle::settings::PAGE,
            title: "gradle-settings-page",
            search: &[
                "gradle-settings-user-home",
                "gradle-settings-offline",
                "gradle-settings-build-and-run",
                "gradle-settings-run-tests",
                "gradle-settings-distribution",
                "gradle-settings-jvm",
                "gradle-settings-vm-options",
                "gradle-settings-download-sources",
            ],
            form: "gradle.settings.form",
            commands: &["gradle.settings.", "gradle.distribution."],
            parent: maven::BUILD_TOOLS_PAGE,
        },
        zen_modules::SettingsPage {
            id: zen_workbench::java::processing::COMPILER_PAGE,
            title: "processing-compiler-page",
            search: &["processing-compiler-page"],
            form: "compiler.settings.form",
            commands: &["compiler.settings."],
            parent: "build",
        },
        zen_modules::SettingsPage {
            id: zen_workbench::java::processing::PAGE,
            title: "processing-page",
            search: &[
                "processing-page",
                "processing-enabled",
                "processing-source",
                "processing-custom-path",
                "processing-generated",
                "processing-generated-main",
                "processing-options",
                "processing-lombok",
            ],
            form: "annotationProcessing.settings.form",
            commands: &["annotationProcessing.settings."],
            parent: zen_workbench::java::processing::COMPILER_PAGE,
        },
    ]
}

fn start_modules(
    product: &mut IdeProduct,
    registry: &mut Registry,
    data: Option<&Path>,
    file_templates: &zen_workbench::file_templates::FileTemplates,
    new_projects: &zen_workbench::new_project::NewProjects,
) -> (Option<Arc<dyn zen_modules::LanguageToolchain>>, Vec<zen_modules::WindowState>, zen_modules::SharedModules) {
    zen_i18n::register(zen_modules::LOCALES);
    let modules =
        zen_modules::Modules::load(product.name, product.modules.clone(), data.map(|data| data.join("modules.json")));
    modules.register_locales();
    let mut host = zen_modules::ModuleHost::new(registry, data);
    for module in modules.active() {
        if let Err(error) = host.run(module) {
            #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
            {
                eprintln!("cannot register the {} module: {error}", module.id);
            }
        }
    }
    let given = host.finish();
    product.highlighters.extend(given.highlighters);
    product.outliners.extend(given.outliners);
    product.project_models.extend(given.project_models);
    product.run_providers.extend(given.run_providers);
    product.run_providers.push(Arc::new(zen_workbench::npm::NpmRun::default()));
    product.run_providers.push(Arc::new(zen_workbench::plugin_runs::PluginRun::default()));
    product.project_models.push(Arc::new(zen_workbench::npm::model::NpmModel));
    product.run_providers.push(Arc::new(zen_workbench::nodejs::NodeRun::default()));
    let build = zen_workbench::build_tools::BuildEnv::global();
    build.set_data_dir(data.map(Path::to_owned));
    product.run_providers.push(Arc::new(zen_workbench::maven::MavenRun::new(build.clone())));
    product.run_providers.push(Arc::new(zen_workbench::gradle::GradleRun::new(build.clone())));
    product.run_providers.push(Arc::new(zen_workbench::ant::AntRun::new(build.clone())));
    zen_workbench::jvm_libraries::set_cache(&build);
    product.project_models.push(Arc::new(zen_workbench::jvm_libraries::JvmLibraries::new(build.clone())));
    product.run_providers.push(Arc::new(zen_workbench::java::JavaTestRun::new(build)));
    product.run_providers.push(Arc::new(zen_workbench::spring_boot::run::SpringBootRun::default()));
    product.run_providers.push(Arc::new(zen_workbench::java_run::JavaRun::default()));
    product.inspectors.extend(given.inspectors);
    product.inspectors.push(Arc::new(zen_workbench::spring_boot::support::BootSupport::new(Arc::new(
        zen_workbench::spring_boot::support::Http,
    ))));
    product.language_servers.extend(given.language_servers);
    product.templates.extend(given.templates.into_iter().map(|set| zen_workbench::templates::Builtin {
        language: set.language.to_owned(),
        templates: zen_workbench::templates::Template::parse_list(set.templates).unwrap_or_default(),
        add_import: set.add_import,
        surrounders: zen_workbench::templates::Surrounder::parse_list(set.surrounders).unwrap_or_default(),
        postfix: zen_workbench::templates::PostfixTemplate::parse_list(set.postfix).unwrap_or_default(),
        doc_comments: zen_workbench::templates::DocComment::parse_list(set.doc_comments).unwrap_or_default(),
        statements: serde_json::from_str(set.statements).unwrap_or_default(),
    }));
    for set in &given.file_templates {
        file_templates.declare(zen_workbench::file_templates::Declared::of_module(set));
    }
    for generator in given.project_generators {
        new_projects.declare(generator);
    }
    product.settings_pages.extend(given.settings_pages);
    product.settings_pages.push(zen_modules::SettingsPage {
        id: zen_workbench::code_quality::PAGE,
        title: "code-quality-page",
        search: &["code-quality-page", "code-quality-on-reformat", "code-quality-on-save"],
        form: "codeQuality.settings.form",
        commands: &["codeQuality."],
        parent: "",
    });
    product.settings_pages.push(zen_modules::SettingsPage {
        id: zen_workbench::jdk::PAGE,
        title: "jdk-page",
        search: &["jdk-page", "jdk-project-sdk", "jdk-language-level", "jdk-add-from-disk", "jdk-found"],
        form: zen_workbench::jdk::FORM,
        commands: &["toolchain.jdk."],
        parent: "",
    });
    product.settings_pages.extend(build_tool_settings_pages());
    product.code_styles.extend(given.code_styles);
    product.folding_options.extend(given.folding_options);
    zen_vfs::file_types::declare(given.file_types.iter().map(|declared| zen_vfs::file_types::FileType {
        id: declared.id.to_owned(),
        name: declared.name.to_owned(),
        localized: true,
        patterns: declared.patterns.iter().map(|pattern| (*pattern).to_owned()).collect(),
        hashbangs: declared.hashbangs.iter().map(|hashbang| (*hashbang).to_owned()).collect(),
        contents: Vec::new(),
    }));
    for folder in &given.excluded_folders {
        zen_vfs::excluded_folders::declare(zen_vfs::excluded_folders::ExcludedFolder::new(folder.name, folder.beside));
    }
    product.debugger = product.debugger.or(given.debugger);
    for page in &product.settings_pages {
        super::search_everywhere::declare_module_page(page);
    }
    for (prefix, group) in given.notification_groups {
        zen_workbench::notifications::Group::declare(prefix, group);
    }
    let mut window_states = given.window_states;
    let code_styles =
        zen_workbench::code_style_options::CodeStyles::new(given.code_style_declarations, given.arrangers);
    window_states.push(Arc::new(move |session: &mut Session| session.set_state(code_styles.clone())));
    let folding = zen_modules::folding::FoldingGroups(product.folding_options.clone());
    window_states.push(Arc::new(move |session: &mut Session| session.set_state(folding.clone())));
    #[cfg(not(target_arch = "wasm32"))]
    {
        let application = format!("Zen {}", product.name);
        window_states.push(Arc::new(move |session: &mut Session| {
            session.state_mut::<zen_workbench::database::Database>().set_application_name(application.clone());
        }));
    }
    let watcher_templates = given.file_watcher_templates;
    window_states.push(Arc::new(move |session: &mut Session| {
        let declared = session.state_mut::<zen_workbench::file_watchers::Templates>();
        for set in &watcher_templates {
            declared.declare(set.origin, set.templates);
        }
    }));
    let generators = given.code_generators;
    window_states.push(Arc::new(move |session: &mut Session| {
        let declared = session.state_mut::<zen_modules::generate::CodeGenerators>();
        for generator in &generators {
            declared.add(Arc::clone(generator));
        }
    }));
    let assists = given.language_assists;
    window_states.push(Arc::new(move |session: &mut Session| {
        let declared = session.state_mut::<zen_modules::assist::LanguageAssists>();
        for assist in &assists {
            declared.add(Arc::clone(assist));
        }
    }));
    let file_references = given.file_references;
    window_states.push(Arc::new(move |session: &mut Session| {
        let declared = session.state_mut::<zen_modules::file_references::FileReferenceSet>();
        for references in &file_references {
            declared.add(Arc::clone(references));
        }
    }));
    for contributor in given.search_contributors {
        window_states.push(Arc::new(move |session: &mut Session| {
            session.state_mut::<zen_modules::search::SearchContributors>().add(Arc::clone(&contributor));
        }));
    }
    let shared: zen_modules::SharedModules = Arc::new(Mutex::new(modules));
    if let Err(error) = zen_modules::register(registry, &shared) {
        #[allow(clippy::print_stderr, reason = "start-up problems are reported on the terminal")]
        {
            eprintln!("cannot register the built-in plug-in commands: {error}");
        }
    }
    (given.toolchain, window_states, shared)
}

fn mark_areas(product: &mut IdeProduct) {
    #[cfg(debug_assertions)]
    if let Ok(ids) = std::env::var("ZEN_DEV_AREAS") {
        product.areas = ids
            .split(',')
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| &*Box::leak(id.to_owned().into_boxed_str()))
            .collect();
    }
    let areas = product.areas.clone();
    for module in &mut product.modules {
        if areas.contains(&module.descriptor.id) {
            module.on_by_default = false;
        }
    }
    if let Some(config) = &mut product.plugins {
        config.optional.extend(areas.iter().map(|&id| id.to_owned()));
    }
}

fn save(state: &State) {
    if state.headless {
        return;
    }
    if let Some(file) = &state.recent_file {
        let _ = state.recent.save(file);
    }
}

fn project_for_file(file: &Path) -> Option<PathBuf> {
    let git = file.ancestors().find(|folder| folder.file_name().is_some_and(|name| name == ".git"));
    git.and_then(Path::parent).or_else(|| file.parent()).map(Path::to_owned)
}

pub(super) fn file_at_line(path: &Path) -> Option<(PathBuf, usize, Option<usize>)> {
    if path.exists() {
        return None;
    }
    let text = path.to_str()?;
    let (rest, last) = text.rsplit_once(':')?;
    let last: usize = last.parse().ok().filter(|number| *number > 0)?;
    let with_column = rest
        .rsplit_once(':')
        .and_then(|(file, line)| Some((file, line.parse::<usize>().ok().filter(|line| *line > 0)?)))
        .filter(|(file, _)| Path::new(file).is_file());
    let (file, line, column) = match with_column {
        Some((file, line)) => (file, line, Some(last)),
        None => (rest, last, None),
    };
    Path::new(file).is_file().then(|| (PathBuf::from(file), line, column))
}

#[cfg(test)]
mod location_tests {
    use super::*;

    #[test]
    fn a_file_and_line_from_the_command_line() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "package main\n").unwrap();
        let at = |text: String| file_at_line(Path::new(&text));
        let name = file.display();
        assert_eq!(at(format!("{name}:12")), Some((file.clone(), 12, None)));
        assert_eq!(at(format!("{name}:12:5")), Some((file.clone(), 12, Some(5))));
        assert_eq!(at(name.to_string()), None, "a plain file opens as it is");
        assert_eq!(at(format!("{name}:x")), None);
        assert_eq!(at(format!("{name}:0")), None);
        assert_eq!(at(format!("{}/missing.go:3", dir.path().display())), None);
        let odd = dir.path().join("a:1");
        std::fs::write(&odd, "").unwrap();
        assert_eq!(at(odd.display().to_string()), None, "a file whose name has a colon is that file");
    }
}

pub(super) fn window_title(path: &Path, product: &str) -> String {
    format!("{} – {product}", super::project_name(path))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn run_mcp_bridge(app_id: &str) -> Result<(), String> {
    let data = zen_workbench::recent::default_file(app_id)
        .and_then(|file| Some(file.parent()?.to_owned()))
        .ok_or("the application data folder is unknown")?;
    #[cfg_attr(not(target_os = "macos"), allow(unused_variables, reason = "macOS starts the app by its id"))]
    let app_id = app_id.to_owned();
    zen_automation::bridge::run(&data.join("automation.json"), move || {
        #[cfg(target_os = "macos")]
        {
            let status = std::process::Command::new("open")
                .args(["-g", "-b", &app_id, "--env", "ZEN_AUTOMATION=1"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            if status.is_ok_and(|status| status.success()) {
                return Ok(());
            }
        }
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        std::process::Command::new(exe)
            .env("ZEN_AUTOMATION", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("cannot start the IDE: {error}"))
    })
}

fn apply_interface_language(settings: &IdeSettings) {
    zen_i18n::set_language(interface_language(settings));
}

fn interface_language(settings: &IdeSettings) -> Option<&str> {
    settings.language.as_deref()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod accessibility_tests;
#[cfg(test)]
mod manage_settings_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod file_template_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod caret_popup_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod macro_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod hierarchy_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod autosave_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod scratch_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod bundle_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod preview_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod trust_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod workspace_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod git_host_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(super) mod markdown_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod quick_switch_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod view_mode_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod maven_window_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod transaction_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod safety_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod new_project_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod parameters_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod several_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    mod usages_tests;

    use std::io::{Read, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use serde_json::{Value, json};

    use super::*;

    fn call(url: &str, id: u64, tool: &str, arguments: Value) -> Value {
        let address = url.trim_start_matches("http://").trim_end_matches("/mcp");
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": "tools/call", "params": { "name": tool, "arguments": arguments } })
            .to_string();
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        write!(
            stream,
            "POST /mcp HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (_, json) = response.split_once("\r\n\r\n").unwrap();
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_new_theme_reaches_every_window() {
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: Vec::new(),
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            folding_options: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            inspectors: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
        };
        let mut app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project]);
        assert_eq!(app.palette(), IdePalette::zen_dark());

        let mut theme = zen_theme::Theme::new("mine", "Mine", true);
        theme.ui.insert(zen_theme::UiKey::Accent, zen_theme::Rgba::rgb(0xFF, 0, 0));
        theme.ui.insert(zen_theme::UiKey::PopupBackground, zen_theme::Rgba::rgb(0x10, 0x20, 0x30));
        theme.terminal.insert(zen_theme::TerminalKey::Background, zen_theme::Rgba::rgb(0, 0, 0xFF));
        let resolved = theme.resolve(|_| None).theme;
        let ctx = egui::Context::default();
        app.set_theme(&ctx, resolved.clone());

        let expected = IdePalette::from_theme(&resolved);
        assert_eq!(expected.accent, egui::Color32::RED);
        assert_eq!(app.palette(), expected);
        assert_eq!(app.welcome.palette, expected);
        assert_eq!(IdePalette::of(&ctx), expected);
        let windows = app.shared.state().windows.clone();
        assert_eq!(windows.len(), 1);
        let window = windows[0].window.lock().unwrap();
        assert_eq!(window.view.palette, expected);
        #[cfg(not(target_arch = "wasm32"))]
        assert_eq!(window.view.terminal_palette.background, egui::Color32::BLUE);
        drop(window);

        let later = data.path().join("later");
        std::fs::create_dir(&later).unwrap();
        app.open(None, &later);
        let windows = app.shared.state().windows.clone();
        assert_eq!(windows[1].window.lock().unwrap().view.palette, expected);
    }

    pub(super) fn test_product() -> IdeProduct {
        IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: Vec::new(),
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            folding_options: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            inspectors: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_waiting_start_ends_when_the_tab_of_its_file_closes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let data = root.join("data");
        std::fs::create_dir_all(root.join("repo/.git")).unwrap();
        std::fs::write(root.join("repo/.git/COMMIT_EDITMSG"), "subject\n").unwrap();
        let mut app = themed_app(&data, Vec::new());
        let ctx = egui::Context::default();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));

        let (ended, returned) = mpsc::channel();
        let (client, message) = (data.clone(), root.join("repo/.git/COMMIT_EDITMSG"));
        std::thread::spawn(move || ended.send(zen_workbench::handoff::send_and_wait(&client, &[message])));
        let started = Instant::now();
        while app.open_documents().is_empty() {
            assert!(started.elapsed() < Duration::from_secs(60), "{:?}", app.open_projects());
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.open_projects(), [root.join("repo")], "a file in .git opens its repository, not .git");
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert!(returned.try_recv().is_err(), "the start waits while the tab is open");

        let close = json!({ "command": "file.document.close", "project": root.join("repo"), "params": {} });
        app.tool(&ctx, "dispatch_command", &close).unwrap();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert_eq!(returned.recv_timeout(Duration::from_secs(20)), Ok(true));
    }

    #[test]
    fn a_waiting_start_of_a_folder_is_not_held() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let data = root.join("data");
        std::fs::create_dir_all(root.join("folder")).unwrap();
        let mut app = themed_app(&data, Vec::new());
        let ctx = egui::Context::default();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        let (ended, returned) = mpsc::channel();
        let (client, folder) = (data.clone(), root.join("folder"));
        std::thread::spawn(move || ended.send(zen_workbench::handoff::send_and_wait(&client, &[folder])));
        let started = Instant::now();
        loop {
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
            if let Ok(accepted) = returned.try_recv() {
                assert!(accepted);
                break;
            }
            assert!(started.elapsed() < Duration::from_secs(60));
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_file_outside_git_opens_its_folder() {
        assert_eq!(project_for_file(Path::new("/w/a/b.txt")), Some(PathBuf::from("/w/a")));
        assert_eq!(project_for_file(Path::new("/w/.git/rebase-merge/todo")), Some(PathBuf::from("/w")));
    }

    #[test]
    fn a_project_tab_moves_along_the_row() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let data = root.join("data");
        for name in ["one", "two", "three"] {
            std::fs::create_dir_all(root.join(name)).unwrap();
        }
        let mut app = themed_app(&data, ["one", "two", "three"].map(|name| root.join(name)).to_vec());
        let ctx = egui::Context::default();
        let started = Instant::now();
        while app.shared.overview().tabs.len() < 3 {
            assert!(started.elapsed() < Duration::from_secs(60), "{:?}", app.shared.overview().tabs);
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
            std::thread::sleep(Duration::from_millis(20));
        }
        let names = |app: &IdeApp| -> Vec<String> {
            app.shared
                .overview()
                .tabs
                .iter()
                .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        let before = names(&app);
        let first = root.join(&before[0]);
        let call = json!({ "command": "project.tab.move", "project": first, "params": { "to": 2 } });
        app.tool(&ctx, "dispatch_command", &call).unwrap();
        let started = Instant::now();
        while names(&app)[2] != before[0] {
            assert!(started.elapsed() < Duration::from_secs(60), "{:?}", names(&app));
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(names(&app), [before[1].clone(), before[2].clone(), before[0].clone()]);
        app.handle(&ctx, Request::MoveTab(first.clone(), 0));
        assert_eq!(names(&app), before, "the drag's request does the same");
    }

    #[test]
    fn what_a_second_start_hands_over_opens_in_the_running_ide() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let data = root.join("data");
        std::fs::create_dir_all(root.join("project/pkg")).unwrap();
        std::fs::create_dir_all(root.join("second")).unwrap();
        std::fs::write(root.join("project/pkg/a.go"), "package pkg\n\nfunc A() {\n\treturn\n}\n").unwrap();
        let mut app = themed_app(&data, vec![root.join("project")]);
        let ctx = egui::Context::default();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        let socket = zen_workbench::handoff::socket_path(&data);
        assert_eq!(app.launcher_socket(), Some(socket.as_path()));

        let file = PathBuf::from(format!("{}:4:2", root.join("project/pkg/a.go").display()));
        assert!(zen_workbench::handoff::send(&data, &[file, root.join("second")]));
        let started = Instant::now();
        while app.open_projects().len() < 2 {
            assert!(started.elapsed() < Duration::from_secs(60), "{:?}", app.open_projects());
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(app.open_projects(), vec![root.join("project"), root.join("second")], "no project for pkg");
        let listed = app.tool(&ctx, "list_windows", &json!({})).unwrap();
        assert_eq!(listed["windows"][0]["active"], json!(root.join("project/pkg/a.go")), "{listed}");
        let selection = json!({ "command": "editor.selection.get", "project": root.join("project"), "params": {} });
        let carets = app.tool(&ctx, "dispatch_command", &selection).unwrap();
        assert_eq!((carets[0]["line"].clone(), carets[0]["column"].clone()), (json!(3), json!(1)), "{carets}");
        assert_eq!(app.shared.overview().active_tab, Some(root.join("second")), "the last one handed over shows");

        assert!(zen_workbench::handoff::send(&data, &[root.join("project/pkg/a.go")]));
        let started = Instant::now();
        while app.shared.overview().active_tab != Some(root.join("project")) {
            assert!(started.elapsed() < Duration::from_secs(60), "the file's project comes to the front");
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
            std::thread::sleep(Duration::from_millis(20));
        }

        let mut second = themed_app(&data, Vec::new());
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| second.logic(ctx));
        assert_eq!(second.launcher_socket(), None, "one IDE per data folder takes the second starts");
        drop(second);
        app.shared.exiting.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(!zen_workbench::handoff::send(&data, &[root.join("second")]), "an exiting IDE takes nothing");
        drop(app);
        assert!(!socket.exists(), "the socket goes with the IDE");
    }

    pub(super) fn themed_app(data: &Path, projects: Vec<PathBuf>) -> IdeApp {
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        IdeApp::new(test_product(), registry, Some(data.join("recent.json")), projects)
    }

    fn write_dusk(data: &Path) -> ResolvedTheme {
        let themes = data.join("themes");
        std::fs::create_dir_all(&themes).unwrap();
        let text = r##"{"formatVersion": 1, "id": "dusk", "name": "Dusk", "dark": true, "ui": {"accent": "#FF0000"}}"##;
        std::fs::write(themes.join("dusk.json"), text).unwrap();
        zen_theme::parse(text).unwrap().resolve(|_| None).theme
    }

    fn saved_theme(data: &Path) -> Value {
        let text = std::fs::read_to_string(data.join("settings.json")).unwrap();
        serde_json::from_str::<Value>(&text).unwrap()["theme"].clone()
    }

    #[test]
    fn settings_without_a_theme_use_zen_dark() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), r#"{"mcpEnabled": false, "ligatures": true}"#).unwrap();
        let settings = IdeSettings::load(&data.path().join("settings.json"), 1);
        assert_eq!(settings.theme, None);
        assert!(settings.ligatures);
        let app = themed_app(data.path(), Vec::new());
        assert_eq!(app.palette(), IdePalette::zen_dark());
        assert_eq!(app.pending_notice, None);
        assert_eq!(themes::lock(&app.shared.themes).current(), ZEN_DARK_ID);
    }

    #[test]
    fn the_saved_theme_applies_at_start() {
        let data = tempfile::tempdir().unwrap();
        let dusk = write_dusk(data.path());
        std::fs::write(data.path().join("settings.json"), r#"{"theme": "dusk"}"#).unwrap();
        let app = themed_app(data.path(), Vec::new());
        assert_eq!(app.palette(), IdePalette::from_theme(&dusk));
        assert_eq!(app.welcome.palette, IdePalette::from_theme(&dusk));
        assert_eq!(app.pending_notice, None);
    }

    #[test]
    fn the_build_tool_and_compiler_pages_are_under_build_and_speak_both_languages() {
        zen_i18n::register(zen_workbench::LOCALES);
        let pages = build_tool_settings_pages();
        let tree: Vec<(&str, u8)> = super::super::settings::page::sections(&pages, &[])
            .iter()
            .map(|section| (section.key, section.level))
            .collect();
        let build = tree.iter().position(|(key, _)| *key == "settings-build").unwrap();
        assert_eq!(
            tree[build + 1..build + 11],
            [
                ("settings-debugger", 1),
                ("settings-debugger-data-views", 2),
                ("build-tools-page", 1),
                ("maven-settings-page", 2),
                ("maven-settings-importing-page", 3),
                ("maven-settings-runner-page", 3),
                ("maven-settings-tests-page", 3),
                ("gradle-settings-page", 2),
                ("processing-compiler-page", 1),
                ("processing-page", 2),
            ]
        );
        for language in ["en", "ru"] {
            zen_i18n::set_language(Some(language));
            for key in pages.iter().flat_map(|page| std::iter::once(page.title).chain(page.search.iter().copied())) {
                assert_ne!(zen_i18n::t!(key), key, "{key} in {language}");
            }
        }
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn a_saved_theme_that_is_gone_falls_back_to_zen_dark_with_a_notice() {
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), r#"{"theme": "gone"}"#).unwrap();
        let app = themed_app(data.path(), Vec::new());
        assert_eq!(app.palette(), IdePalette::zen_dark());
        assert!(
            app.pending_notice.as_deref().is_some_and(|notice| notice.contains("gone")),
            "{:?}",
            app.pending_notice
        );
        assert_eq!(app.shared.overview().settings.theme.as_deref(), Some("gone"), "the file is left as it was");
    }

    #[test]
    fn a_damaged_settings_file_is_kept_aside_with_a_notice_and_saved_over_only_after_that() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), "{ broken").unwrap();
        let app = themed_app(data.path(), Vec::new());
        let notice = app.pending_notice.as_deref().unwrap_or_default();
        assert!(notice.contains("settings.json.corrupt"), "{notice}");
        assert_eq!(std::fs::read_to_string(data.path().join("settings.json.corrupt")).unwrap(), "{ broken");
    }

    #[test]
    fn a_settings_file_that_stays_damaged_is_left_alone_and_the_person_is_told() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let file = data.path().join("settings.json");
        std::fs::write(&file, "{ broken").unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        std::fs::write(&file, "{ broken").unwrap();
        app.settings_stuck = true;
        app.pending_notice = None;
        app.save_settings(&IdeSettings::default());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "{ broken");
        assert!(app.pending_notice.is_some());
    }

    #[test]
    fn a_settings_save_that_fails_tells_the_person() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        std::fs::create_dir_all(data.path().join("settings.json").join("in the way")).unwrap();
        app.save_settings(&IdeSettings::default());
        assert!(app.pending_notice.as_deref().unwrap_or_default().contains("settings.json"));
    }

    #[test]
    fn an_internal_error_shows_in_the_status_bar_until_it_is_cleared() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let errors = zen_workbench::internal_errors::InternalErrors::default();
        app.slot_for(None).unwrap().window.lock().unwrap().session.set_state(errors.clone());
        let state = |ctx: &egui::Context, app: &mut IdeApp| ask(ctx, app, "get_window_state", json!({})).unwrap();
        assert_eq!(state(&ctx, &mut app)["internalErrors"], 0, "nothing to show");

        errors.report("boom (thread worker, a.rs:1:1)".to_owned(), "0: worker".to_owned());
        errors.report("again".to_owned(), "0: worker".to_owned());
        frame(&ctx, &mut app);
        assert_eq!(state(&ctx, &mut app)["internalErrors"], 2);
        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.ideErrors.open" })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["internalErrors"]), "{opened}");
        let described = json!({ "command": "ide.errors.describe", "params": { "id": 1, "description": "typing" } });
        assert_eq!(ask(&ctx, &mut app, "dispatch_command", described).unwrap(), true);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "ide.errors.clear" })).unwrap();
        frame(&ctx, &mut app);
        let state = state(&ctx, &mut app);
        assert_eq!(state["internalErrors"], 0);
        assert_eq!(state["popups"], json!([]), "the dialog closes with the last error");
    }

    #[test]
    fn a_damaged_templates_file_is_kept_aside_with_a_notice() {
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("templates.json"), "[").unwrap();
        let app = themed_app(data.path(), Vec::new());
        let notice = app.pending_notice.as_deref().unwrap_or_default();
        assert!(notice.contains("templates.json.corrupt"), "{notice}");
        assert!(data.path().join("templates.json.corrupt").exists());
    }

    #[test]
    fn selecting_a_theme_reaches_every_window_and_is_saved() {
        let data = tempfile::tempdir().unwrap();
        let dusk = write_dusk(data.path());
        let (first, second) = (data.path().join("first"), data.path().join("second"));
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let mut app = themed_app(data.path(), vec![first, second]);
        let windows = app.shared.state().windows.clone();
        assert_eq!(windows.len(), 2);

        let ctx = egui::Context::default();
        {
            let mut window = windows[0].window.lock().unwrap();
            let result = app.shared.registry.dispatch(&mut window.session, "ui.theme.select", json!({ "id": "dusk" }));
            assert_eq!(result.unwrap(), json!({}));
        }
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        let expected = IdePalette::from_theme(&dusk);
        assert_eq!(app.palette(), expected);
        assert_eq!(app.welcome.palette, expected);
        assert_eq!(IdePalette::of(&ctx), expected);
        for slot in &windows {
            assert_eq!(slot.window.lock().unwrap().view.palette, expected);
        }
        assert_eq!(saved_theme(data.path()), json!("dusk"));
        assert_eq!(app.shared.overview().settings.theme.as_deref(), Some("dusk"));

        let stale = IdeSettings { ligatures: true, theme: Some(ZEN_DARK_ID.to_owned()), ..IdeSettings::default() };
        app.handle(&ctx, Request::Settings(Box::new(stale)));
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert_eq!(saved_theme(data.path()), json!("dusk"), "a settings draft with the old theme keeps the chosen one");
        assert_eq!(app.shared.overview().settings.theme.as_deref(), Some("dusk"));
        assert!(app.shared.overview().settings.ligatures);
        assert_eq!(app.palette(), expected);

        {
            let mut window = windows[1].window.lock().unwrap();
            let result =
                app.shared.registry.dispatch(&mut window.session, "ui.theme.select", json!({ "id": ZEN_DARK_ID }));
            assert!(result.is_ok());
        }
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert_eq!(app.palette(), IdePalette::zen_dark());
        assert_eq!(saved_theme(data.path()), Value::Null);
    }

    #[test]
    fn the_welcome_window_and_the_projects_share_the_live_templates() {
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let declared = zen_workbench::templates::Template::parse_list(
            r#"[{ "language": "go", "abbreviation": "main", "text": "func main() {\n\t$END$\n}" }]"#,
        )
        .unwrap();
        let product = IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: Vec::new(),
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            folding_options: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: vec![zen_workbench::templates::Builtin {
                language: "go".to_owned(),
                templates: declared,
                add_import: None,
                surrounders: Vec::new(),
                ..Default::default()
            }],
            inspectors: Vec::new(),
        };
        let mut app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project]);
        let registry = &app.shared.registry;
        let template = json!({ "language": "go", "abbreviation": "main", "text": "func main() {}", "enabled": false });
        theme_dispatch(registry, &mut app.welcome_session, "editor.templates.set", json!({ "template": template }))
            .unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(data.path().join("templates.json")).unwrap()).unwrap();
        assert_eq!(saved["templates"][0]["enabled"], false);
        let windows = app.shared.state().windows.clone();
        let mut window = windows[0].window.lock().unwrap();
        let listed = registry.dispatch(&mut window.session, "editor.templates.list", json!({})).unwrap();
        assert_eq!((listed[0]["modified"].as_bool(), listed[0]["builtin"].as_bool()), (Some(true), Some(true)));
    }

    #[test]
    fn the_welcome_window_chooses_themes_through_the_shared_store() {
        let data = tempfile::tempdir().unwrap();
        let dusk = write_dusk(data.path());
        let mut app = themed_app(data.path(), Vec::new());
        let ctx = egui::Context::default();
        let registry = &app.shared.registry;
        let session = &mut app.welcome_session;
        let listed = theme_dispatch(registry, session, "ui.theme.list", json!({})).unwrap();
        assert_eq!(listed["current"], ZEN_DARK_ID);
        assert_eq!(theme_dispatch(registry, session, "ui.theme.select", json!({ "id": "dusk" })).unwrap(), json!({}));
        assert!(theme_dispatch(registry, session, "ui.theme.select", json!({ "id": "nope" })).is_err());
        assert!(theme_dispatch(registry, session, "file.project.open", json!({ "path": "/" })).is_err());
        assert_eq!(theme_dispatch(registry, session, "ui.keymap.list", json!({})).unwrap()["current"], "macos");
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert_eq!(app.welcome.palette, IdePalette::from_theme(&dusk));
        assert_eq!(saved_theme(data.path()), json!("dusk"));
    }

    #[test]
    fn an_agent_customizes_a_popup_through_commands_and_the_file_follows() {
        let group = crate::ide::customization::SCRATCH_POPUP;
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        let command = |app: &mut IdeApp, command: &str, params: Value| {
            let arguments = json!({ "command": command, "params": params });
            let answer = app.tool(&ctx, "dispatch_command", &arguments);
            frame(&ctx, app);
            frame(&ctx, app);
            answer
        };
        let listed = command(&mut app, "ui.customization.list", json!({ "group": group })).unwrap();
        let first = listed["groups"][0]["items"][0]["id"].clone();
        assert_eq!(first, "slot:settings-menus-item-run");
        assert_eq!(listed["groups"][0]["customized"], false);
        let removed = command(&mut app, "ui.customization.remove", json!({ "group": group, "index": 0 })).unwrap();
        assert_eq!(removed["missing"][0]["id"], first);
        let file = data.path().join(crate::ide::customization::FILE);
        let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_ne!(saved["groups"][group][0], first);
        let shown = app.shared.overview().settings.customization.items(group);
        assert_ne!(shown[0], first.as_str().unwrap(), "the windows follow without a restart");
        command(&mut app, "ui.customization.restore", json!({ "group": group })).unwrap();
        assert!(!file.exists(), "nothing left to keep");
        let refused = command(&mut app, "ui.customization.add", json!({ "group": "nothing", "item": "-" }));
        assert!(refused.unwrap_err().contains("nothing"));
        let refused = command(&mut app, "ui.customization.add", json!({ "group": group, "item": "gone.action" }));
        assert!(refused.unwrap_err().contains("gone.action"));
        let refused = command(&mut app, "ui.customization.remove", json!({ "group": group, "index": 99 }));
        assert!(refused.is_err());
        let refused = command(&mut app, "ui.customization.setIcon", json!({ "action": "a.b", "icon": "nope" }));
        assert!(refused.is_err());
    }

    #[test]
    fn caffeine_follows_agents_the_settings_and_ends_with_the_ide() {
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        std::fs::write(data.path().join("settings.json"), r#"{"caffeine": {"startOnLaunch": true, "what": ["disk"]}}"#)
            .unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        let status = |app: &mut IdeApp| {
            app.tool(&ctx, "dispatch_command", &json!({ "command": "caffeine.status.get" })).unwrap()
        };
        let first = status(&mut app);
        assert_eq!((first["on"].clone(), first["awake"].clone()), (json!(true), json!(true)), "started with the IDE");
        assert_eq!(first["what"], json!(["disk"]));
        let start = json!({ "command": "caffeine.awake.start", "params": { "while": ["agent"] } });
        let started = app.tool(&ctx, "dispatch_command", &start).unwrap();
        assert_eq!(started["paused"], "waiting", "no agent has called yet");
        app.agent_seen = Some(Instant::now());
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        let now = status(&mut app);
        assert_eq!(now["awake"], true);
        assert!(now["busy"].as_array().unwrap().contains(&json!("agent")), "{now}");
        app.agent_seen = Instant::now().checked_sub(Duration::from_secs(180));
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert_eq!(status(&mut app)["paused"], "waiting", "an agent silent for minutes is gone");
        app.tool(&ctx, "set_settings", &json!({ "caffeine": { "what": ["idle", "display"] } })).unwrap();
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        assert_eq!(status(&mut app)["what"], json!(["idle", "display"]), "the settings apply at once");
        assert_eq!(saved_settings(data.path())["caffeine"]["what"], json!(["idle", "display"]));
        assert!(app.tool(&ctx, "set_settings", &json!({ "caffeine": { "lowBatteryLevel": 2 } })).is_err());
        app.shared.stop_work();
        assert_eq!(status(&mut app)["on"], false, "exiting turns Caffeine off");
    }

    #[test]
    fn caffeines_timer_wakes_the_ide_and_every_window_is_drawn_again() {
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        let (repaint, repaints) = mpsc::channel();
        ctx.set_request_repaint_callback(move |info| {
            let _ = repaint.send((info.viewport_id, info.delay));
        });
        let soon = Instant::now() - Duration::from_secs(57);
        let timed = zen_workbench::caffeine::StartParams { minutes: Some(1), ..Default::default() };
        app.shared.caffeine.start(&timed, soon).unwrap();
        for _ in 0..20 {
            while repaints.try_recv().is_ok() {}
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
            if !repaints.try_iter().any(|(_, delay)| delay.is_zero()) {
                break;
            }
        }
        assert!(app.shared.caffeine.is_on());
        let windows: Vec<ViewportId> = app.shared.state().windows.iter().map(Slot::drawn_in).collect();
        assert!(!windows.is_empty());
        let waited = Instant::now();
        while waited.elapsed() < Duration::from_secs(60) && app.shared.caffeine.is_on() {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!app.shared.caffeine.is_on(), "the timer turned Caffeine off without a pass");
        assert!(ctx.has_requested_repaint_for(&ViewportId::ROOT), "and woke the IDE");
        while repaints.try_recv().is_ok() {}
        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        let drawn: Vec<ViewportId> = repaints.try_iter().map(|(viewport, _)| viewport).collect();
        assert!(windows.iter().all(|window| drawn.contains(window)), "{drawn:?} draws {windows:?}");
    }

    #[test]
    fn a_window_that_draws_nothing_reloads_files_changed_on_disk() {
        let data = tempfile::tempdir().unwrap();
        let project = data.path().canonicalize().unwrap().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let file = project.join("main.go");
        std::fs::write(&file, "package main\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        let open = json!({ "command": "file.document.open", "params": { "path": file } });
        app.tool(&ctx, "dispatch_command", &open).unwrap();
        let text =
            |app: &mut IdeApp| app.tool(&ctx, "get_document_text", &json!({ "path": file })).unwrap()["text"].clone();

        let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        std::thread::sleep(Duration::from_millis(300));
        std::fs::write(&file, "package main\n\nfunc main() {}\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while text(&mut app) != "package main\n\nfunc main() {}\n" && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        }
        assert_eq!(text(&mut app), "package main\n\nfunc main() {}\n", "reloaded without a drawn frame");
    }

    #[cfg(unix)]
    #[test]
    fn a_project_opened_through_a_link_is_its_real_folder() {
        let data = tempfile::tempdir().unwrap();
        let real = data.path().canonicalize().unwrap().join("real");
        std::fs::create_dir_all(real.join("pkg")).unwrap();
        std::fs::write(real.join("pkg/a.go"), "package pkg\n").unwrap();
        let link = data.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut app = themed_app(data.path(), vec![link.clone()]);
        let ctx = egui::Context::default();

        app.open(None, &real);
        app.open(None, &link);
        let windows = app.shared.state().windows.clone();
        let paths: Vec<&Path> = windows.iter().map(|slot| slot.path.as_path()).collect();
        assert_eq!(paths, [real.as_path()]);
        assert_eq!(windows[0].window.lock().unwrap().view.opened_as.as_deref(), Some(link.as_path()));

        let file = link.join("pkg/a.go");
        let params = json!({ "command": "file.document.open", "params": { "path": file }, "project": link });
        app.tool(&ctx, "dispatch_command", &params).unwrap();
        let listed = app.tool(&ctx, "list_windows", &json!({})).unwrap();
        assert_eq!(listed["windows"][0]["documents"][0]["path"], json!(real.join("pkg/a.go")));
        let selected = app.tool(&ctx, "select_in_tree", &json!({ "paths": [file] })).unwrap();
        assert_eq!(selected["paths"], json!([real.join("pkg/a.go")]), "{selected}");
        let text = app.tool(&ctx, "get_document_text", &json!({ "path": file })).unwrap();
        assert_eq!(text["text"], "package pkg\n");
    }

    #[test]
    fn agents_list_select_and_import_themes() {
        let data = tempfile::tempdir().unwrap();
        let dusk = write_dusk(data.path());
        let mut app = themed_app(data.path(), Vec::new());
        let ctx = egui::Context::default();

        let listed = app.tool(&ctx, "list_themes", &json!({})).unwrap();
        assert_eq!(listed["current"], ZEN_DARK_ID);
        assert_eq!(listed["themes"][1]["id"], zen_theme::ZEN_LIGHT_ID);
        assert_eq!(listed["themes"][2]["id"], "dusk");
        assert_eq!(listed["themes"][2]["source"], "user");

        assert!(app.tool(&ctx, "select_theme", &json!({ "id": "nope" })).is_err());
        assert_eq!(app.tool(&ctx, "select_theme", &json!({ "id": "dusk" })).unwrap(), json!({}));
        assert_eq!(app.palette(), IdePalette::from_theme(&dusk));
        assert_eq!(saved_theme(data.path()), json!("dusk"));

        let scheme = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../jb-import/tests/fixtures/every-key.icls");
        let imported = app.tool(&ctx, "import_theme", &json!({ "path": scheme, "select": true })).unwrap();
        assert_eq!(imported["id"], "fixture-night");
        assert!(imported["mapped"].as_u64().is_some_and(|mapped| mapped > 0), "{imported}");
        assert!(data.path().join("themes/fixture-night.json").is_file());
        assert_eq!(saved_theme(data.path()), json!("fixture-night"));
        let again = app.tool(&ctx, "import_theme", &json!({ "path": scheme })).unwrap();
        assert_eq!(again["id"], "fixture-night-2");
        assert_eq!(app.tool(&ctx, "list_themes", &json!({})).unwrap()["current"], "fixture-night");
        let tools: Vec<String> = super::super::automation::tools().into_iter().map(|tool| tool.name).collect();
        for name in ["list_themes", "select_theme", "import_theme", "follow_system_theme"] {
            assert!(tools.iter().any(|tool| tool == name), "{name}");
        }
    }

    fn saved_settings(data: &Path) -> Value {
        let text = std::fs::read_to_string(data.join("settings.json")).unwrap();
        serde_json::from_str::<Value>(&text).unwrap()
    }

    #[test]
    fn the_theme_follows_the_system_appearance_without_a_restart() {
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let dusk = write_dusk(data.path());
        let mut app = themed_app(data.path(), Vec::new());
        let ctx = egui::Context::default();
        let system = |theme: egui::Theme, app: &mut IdeApp| {
            ctx.begin_pass(egui::RawInput { system_theme: Some(theme), ..egui::RawInput::default() });
            ctx.end_pass().drop_without_applying_deltas();
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        };
        system(egui::Theme::Light, &mut app);
        assert_eq!(app.palette(), IdePalette::zen_dark(), "not following yet: the system does not matter");

        let params = json!({ "enabled": true, "dark": "dusk" });
        assert_eq!(app.tool(&ctx, "follow_system_theme", &params).unwrap(), json!({}));
        system(egui::Theme::Light, &mut app);
        let light = IdePalette::from_theme(&ResolvedTheme::zen_light());
        assert_eq!(app.palette(), light);
        assert!(!IdePalette::of(&ctx).dark);
        assert_eq!(ctx.theme(), egui::Theme::Light, "egui's own widgets are light too");
        let saved = saved_settings(data.path());
        assert_eq!((saved["themeFollowSystem"].clone(), saved["themeDark"].clone()), (json!(true), json!("dusk")));
        assert_eq!(saved["themeLight"], Value::Null, "Zen Light is not written");

        system(egui::Theme::Dark, &mut app);
        assert_eq!(app.palette(), IdePalette::from_theme(&dusk), "the system turned dark");
        assert_eq!(app.tool(&ctx, "list_themes", &json!({})).unwrap()["appearance"]["systemDark"], true);

        drop(app);
        let mut again = themed_app(data.path(), Vec::new());
        system(egui::Theme::Light, &mut again);
        assert_eq!(again.palette(), light);
        assert!(app_appearance(&mut again, &ctx)["followSystem"].as_bool().unwrap_or_default());

        assert_eq!(again.tool(&ctx, "follow_system_theme", &json!({ "enabled": false })).unwrap(), json!({}));
        system(egui::Theme::Light, &mut again);
        assert_eq!(again.palette(), IdePalette::zen_dark(), "back to the theme chosen without following");
        assert_eq!(saved_settings(data.path())["themeFollowSystem"], false);
    }

    fn app_appearance(app: &mut IdeApp, ctx: &egui::Context) -> Value {
        app.tool(ctx, "list_themes", &json!({})).unwrap()["appearance"].clone()
    }

    fn saved_keymap(data: &Path) -> Value {
        let text = std::fs::read_to_string(data.join("settings.json")).unwrap_or_default();
        serde_json::from_str::<Value>(&text).unwrap_or_default()["keymap"].clone()
    }

    #[test]
    fn agents_list_select_and_import_keymaps() {
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        crate::keymap::load(Arc::new(Mutex::new(KeymapStore::new(None, Vec::new()))));
        let ctx = egui::Context::default();

        let listed = app.tool(&ctx, "list_keymaps", &json!({})).unwrap();
        assert_eq!(listed["current"], "macos");
        assert_eq!(listed["keymaps"][0]["source"], "builtin");

        let file =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../jb-import/tests/fixtures/keymaps/synthetic-macos.xml");
        let imported = app.tool(&ctx, "import_keymap", &json!({ "path": file })).unwrap();
        assert_eq!(imported["id"], "fixture-keys");
        assert_eq!(imported["mapped"], 7);
        assert_eq!(imported["unmapped"].as_array().map(Vec::len), Some(3));
        assert!(data.path().join("keymaps/fixture-keys.json").is_file());
        assert_eq!(app.tool(&ctx, "get_keymap", &json!({})).unwrap()["keymap"], "macos", "not selected yet");

        assert!(app.tool(&ctx, "select_keymap", &json!({ "id": "nope" })).is_err());
        assert_eq!(app.tool(&ctx, "select_keymap", &json!({ "id": "fixture-keys" })).unwrap(), json!({}));
        assert_eq!(saved_keymap(data.path()), json!("fixture-keys"));
        let keymap = app.tool(&ctx, "get_keymap", &json!({})).unwrap();
        assert_eq!(keymap["keymap"], "fixture-keys");
        let duplicate = keymap["actions"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|action| action["action"] == "editor.line.duplicate")
            .cloned()
            .unwrap_or_default();
        assert_eq!(duplicate["keystrokes"], json!(["⇧⌘D"]));
        assert_eq!(duplicate["changed"], json!(true));

        let set =
            app.tool(&ctx, "set_keymap", &json!({ "action": "editor.line.delete", "keystrokes": ["⌥⌘K"] })).unwrap();
        assert_eq!(set, json!({ "keymap": "fixture-keys", "action": "editor.line.delete", "keystrokes": ["⌥⌘K"] }));
        let named: Value =
            serde_json::from_str(&std::fs::read_to_string(data.path().join("keymaps/fixture-keys.json")).unwrap())
                .unwrap();
        assert_eq!(named["shortcuts"]["editor.line.delete"], json!(["⌥⌘K"]), "edits go to the named keymap");
        assert!(!data.path().join("keymap.json").exists(), "keymap.json is the built-in keymap's");

        assert_eq!(app.tool(&ctx, "select_keymap", &json!({ "id": "macos" })).unwrap(), json!({}));
        assert_eq!(saved_keymap(data.path()), Value::Null);
        app.tool(&ctx, "set_keymap", &json!({ "action": "editor.line.delete", "keystrokes": [] })).unwrap();
        let builtin: Value =
            serde_json::from_str(&std::fs::read_to_string(data.path().join("keymap.json")).unwrap()).unwrap();
        assert_eq!(builtin, json!({ "editor.line.delete": [] }));

        let again = app.tool(&ctx, "import_keymap", &json!({ "path": file, "select": true })).unwrap();
        assert_eq!(again["id"], "fixture-keys-2");
        assert_eq!(saved_keymap(data.path()), json!("fixture-keys-2"));
        let tools: Vec<String> = super::super::automation::tools().into_iter().map(|tool| tool.name).collect();
        for name in ["list_keymaps", "select_keymap", "import_keymap", "get_keymap", "set_keymap"] {
            assert!(tools.iter().any(|tool| tool == name), "{name}");
        }
    }

    #[test]
    fn a_saved_keymap_that_is_gone_falls_back_to_macos_with_a_notice() {
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), r#"{"keymap": "gone"}"#).unwrap();
        let app = themed_app(data.path(), Vec::new());
        crate::keymap::load(Arc::new(Mutex::new(KeymapStore::new(None, Vec::new()))));
        assert_eq!(keymaps::lock(&app.shared.keymaps).current(), "macos");
        assert!(
            app.pending_notice.as_deref().is_some_and(|notice| notice.contains("gone")),
            "{:?}",
            app.pending_notice
        );
    }

    #[test]
    fn answers_agents_without_drawing_a_frame() {
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let file = project.join("main.go");
        std::fs::write(&file, "package main\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: Vec::new(),
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            inspectors: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            folding_options: Vec::new(),
            debugger: None,
        };
        let mut app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        app.enable_automation(0);

        let ctx = egui::Context::default();
        let (repaint, repaints) = mpsc::channel();
        ctx.set_request_repaint_callback(move |_| {
            let _ = repaint.send(());
        });
        let logic = |app: &mut IdeApp| {
            let _ = ctx.run_logic(&egui::RawInput::default(), |ctx| app.logic(ctx));
        };
        logic(&mut app);
        let url = app.shared.automation_url.lock().unwrap().clone().expect("the server runs");

        let file_path = file.clone();
        let agent = std::thread::spawn(move || {
            let started = Instant::now();
            let windows = call(&url, 1, "list_windows", json!({}));
            let listed = started.elapsed();
            let started = Instant::now();
            let opened = call(
                &url,
                2,
                "dispatch_command",
                json!({ "command": "file.document.open", "params": { "path": file_path } }),
            );
            let dispatched = started.elapsed();
            let after = call(&url, 3, "list_windows", json!({}));
            let started = Instant::now();
            let action = call(&url, 4, "dispatch_command", json!({ "command": "view.settings.open" }));
            let acted = started.elapsed();
            (windows, listed, opened, dispatched, after, (action, acted))
        });
        while !agent.is_finished() {
            if repaints.recv_timeout(Duration::from_millis(50)).is_ok() {
                logic(&mut app);
            }
        }
        let (windows, listed, opened, dispatched, after, (action, acted)) = agent.join().unwrap();

        assert_eq!(ctx.cumulative_pass_nr(), 0, "no egui pass ran");
        if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
            assert!(listed < Duration::from_secs(1), "list_windows took {listed:?}");
            assert!(dispatched < Duration::from_secs(1), "dispatch_command took {dispatched:?}");
        }
        assert_eq!(windows["result"]["isError"], false, "{windows}");
        assert_eq!(opened["result"]["isError"], false, "{opened}");
        let documents = after["result"]["content"][0]["text"].as_str().unwrap().to_owned();
        assert!(documents.contains("main.go"), "{after}");
        if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
            assert!(acted < Duration::from_secs(1), "a window action took {acted:?}");
        }
        let action: Value = serde_json::from_str(action["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(action["window"]["popups"], json!(["settings:mcp"]), "{action}");
    }

    pub(super) fn frame(ctx: &egui::Context, app: &mut IdeApp) {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
    }

    pub(super) fn ask(ctx: &egui::Context, app: &mut IdeApp, tool: &str, arguments: Value) -> Result<Value, String> {
        let (sender, answer) = mpsc::channel();
        app.route(
            ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for _ in 0..5 {
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            frame(ctx, app);
        }
        answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments} after 5 frames"))
    }

    pub(super) fn window_project(data: &Path) -> (PathBuf, PathBuf) {
        let project = data.join("project");
        std::fs::create_dir(&project).unwrap();
        let file = project.join("main.go");
        std::fs::write(&file, "package main\n\nfunc main() {\n\tprintln(1)\n}\n").unwrap();
        (project, file)
    }

    fn test_git(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn the_git_workspace_commit_menu_opens_its_dialogs_and_copies_through_the_window() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First", "-m", "The body."]);
        let head = test_git(&project, &["rev-parse", "HEAD"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        frame(&ctx, &mut app);
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        assert_eq!(state(&mut app)["editor"], "gitWorkspace");

        run(&mut app, "vcs.workspace.menu", json!({ "revision": "HEAD" })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["commitMenu"]));
        for (action, dialog) in [
            ("createBranch", "newBranch"),
            ("createTag", "createTag"),
            ("checkout", "checkoutCommit"),
            ("resetHard", "resetHard"),
        ] {
            let done =
                run(&mut app, "vcs.workspace.commitAction", json!({ "revision": "HEAD", "action": action })).unwrap();
            assert_eq!(done["dialog"], dialog);
            frame(&ctx, &mut app);
            assert_eq!(state(&mut app)["popups"], json!([dialog]), "{action}");
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            frame(&ctx, &mut app);
            assert_eq!(state(&mut app)["popups"], json!([]), "{action} closes with Escape");
        }
        assert_eq!(test_git(&project, &["rev-parse", "HEAD"]), head, "nothing ran");

        let (sender, answer) = mpsc::channel();
        app.route(
            &ctx,
            "dispatch_command",
            &json!({ "command": "vcs.workspace.commitAction", "params": { "revision": "HEAD", "action": "copyMessage" } }),
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        let mut copied = Vec::new();
        for _ in 0..5 {
            let output = ctx.run_ui(egui::RawInput::default(), |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            copied.extend(output.platform_output.commands.into_iter().filter_map(|command| match command {
                egui::OutputCommand::CopyText(text) => Some(text),
                _ => None,
            }));
        }
        assert_eq!(answer.try_recv().unwrap().unwrap()["text"], "First\n\nThe body.");
        assert_eq!(copied, ["First\n\nThe body."]);
    }

    #[test]
    fn the_status_bar_indent_switches_and_the_code_style_page_turns_detection_off() {
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let go = project.join("spaces.go");
        std::fs::write(&go, "package main\n\nfunc main() {\n    a()\n    if b {\n        c()\n    }\n    d()\n}\n")
            .unwrap();
        let json_file = project.join("tabbed.json");
        std::fs::write(&json_file, "{\n\t\"a\": {\n\t\t\"b\": 1\n\t},\n\t\"c\": 2\n}\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        app.splash = super::super::splash::Splash::finished();
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0);
        type Screen = crate::offscreen::Offscreen;
        let step = |screen: &mut Screen, app: &mut IdeApp, events: Vec<egui::Event>| {
            screen.step(events, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        };
        let draw = |screen: &mut Screen, app: &mut IdeApp| {
            screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            })
        };
        let call = |screen: &mut Screen, app: &mut IdeApp, tool: &str, arguments: Value| -> Value {
            let (sender, answer) = mpsc::channel();
            let ctx = screen.context().clone();
            app.route(
                &ctx,
                tool,
                &arguments,
                Box::new(move |result| {
                    let _ = sender.send(result);
                }),
            );
            for _ in 0..5 {
                if let Ok(result) = answer.try_recv() {
                    return result.unwrap();
                }
                step(screen, app, Vec::new());
            }
            answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments}")).unwrap()
        };
        let run = |screen: &mut Screen, app: &mut IdeApp, command: &str, params: Value| {
            call(screen, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let click = |screen: &mut Screen, app: &mut IdeApp, name: &str| {
            let tree = screen.accessibility_tree(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            let node = tree
                .nodes
                .iter()
                .map(|(_, node)| node)
                .find(|node| crate::accessibility::accessible_name(node, &tree.nodes).as_deref() == Some(name))
                .unwrap_or_else(|| panic!("nothing named {name:?} is drawn"));
            let bounds = node.bounds().expect("the node has bounds");
            let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                step(screen, app, events);
            }
        };
        let snapshot = |image: &crate::offscreen::Frame, name: &str| {
            if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
                crate::offscreen::write_png(&Path::new(&dir).join(format!("indent-{name}.png")), image).unwrap();
            }
        };
        let popups =
            |screen: &mut Screen, app: &mut IdeApp| call(screen, app, "get_window_state", json!({}))["popups"].clone();
        let s = &mut screen;
        draw(s, &mut app);

        run(s, &mut app, "file.document.open", json!({ "path": go }));
        let indent = run(s, &mut app, "editor.indent.get", json!({}));
        assert_eq!(
            (indent["source"].clone(), indent["detected"].clone()),
            (json!("detected"), json!({ "kind": "spaces", "width": 4 }))
        );
        snapshot(&draw(s, &mut app), "status-detected");
        click(s, &mut app, "4 spaces");
        snapshot(&draw(s, &mut app), "menu-detected");
        assert_eq!(popups(s, &mut app), json!(["indentMenu"]), "a click on the status bar's indent opens its menu");
        click(s, &mut app, "Tab (configured)");
        let indent = run(s, &mut app, "editor.indent.get", json!({}));
        assert_eq!((indent["source"].clone(), indent["usesConfigured"].clone()), (json!("configured"), json!(true)));
        snapshot(&draw(s, &mut app), "status-configured");
        click(s, &mut app, "Tab");
        snapshot(&draw(s, &mut app), "menu-configured");
        click(s, &mut app, "4 spaces (detected in the file)");
        assert_eq!(run(s, &mut app, "editor.indent.get", json!({}))["source"], "detected");

        run(s, &mut app, "file.document.open", json!({ "path": json_file }));
        assert_eq!(run(s, &mut app, "editor.indent.get", json!({}))["source"], "detected");
        call(s, &mut app, "run_action", json!({ "action": "view.indent.menu" }));
        for _ in 0..5 {
            draw(s, &mut app);
        }
        click(s, &mut app, "Configure Indents…");
        for _ in 0..10 {
            if popups(s, &mut app) != json!([]) {
                break;
            }
            step(s, &mut app, Vec::new());
        }
        assert_eq!(popups(s, &mut app), json!(["settings:codeStyleOther"]), "JSON has no page of its own");
        snapshot(&draw(s, &mut app), "configure-indents");
        call(s, &mut app, "press_keys", json!({ "keys": ["⎋"] }));

        call(s, &mut app, "run_action", json!({ "action": "view.settings.open", "params": { "page": "codeStyle" } }));
        snapshot(&draw(s, &mut app), "code-style-detect-on");
        click(s, &mut app, "Follow the indentation a file already has");
        snapshot(&draw(s, &mut app), "code-style-detect-off");
        click(s, &mut app, "OK");
        assert_eq!(popups(s, &mut app), json!([]));
        let indent = run(s, &mut app, "editor.indent.get", json!({}));
        assert_eq!((indent["source"].clone(), indent["detectionEnabled"].clone()), (json!("configured"), json!(false)));
        assert_eq!(
            run(s, &mut app, "editor.indent.get", json!({ "path": go }))["source"],
            "configured",
            "every open file"
        );
        assert!(!app.shared.overview().settings.detect_indents, "the setting is saved");
        snapshot(&draw(s, &mut app), "status-detection-off");
    }

    #[test]
    fn a_narrow_git_workspace_keeps_the_details_panel_off_the_left_panel() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        test_git(&project, &["branch", "feature/narrow-window-layout"]);
        test_git(&project, &["tag", "v0.1.0"]);
        std::fs::write(&file, "package main\n\nfunc main() {}\n").unwrap();
        test_git(&project, &["commit", "-q", "-am", "Keep the details panel off the left panel"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1000.0, 560.0), 2.0);
        let ctx = screen.context().clone();
        screen.step(Vec::new(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        run(&mut app, "vcs.workspace.view", json!({ "detailsWidth": 1600 })).unwrap();
        run(&mut app, "vcs.workspace.select", json!({ "revision": "HEAD" })).unwrap();
        run(&mut app, "vcs.workspace.details", json!({ "open": true })).unwrap();
        for section in ["local", "tags"] {
            run(&mut app, "vcs.workspace.section", json!({ "section": section, "open": true })).unwrap();
        }
        let mut draw = |app: &mut IdeApp| {
            screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            })
        };
        draw(&mut app);
        let panels = |app: &mut IdeApp| {
            let windows = app.shared.state().windows.clone();
            let window = windows[0].window.lock().unwrap();
            window.view.git_workspace.panels
        };
        let (left, details) = panels(&mut app).expect("the workspace is drawn");
        let details = details.expect("the details panel is open");
        assert!(details.width() >= 1.0, "{details:?}");
        assert!(details.left() > left.right(), "{details:?} covers the left panel {left:?}");
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
        run(&mut app, "vcs.workspaceColumns.set", json!({ "widths": { "refs": 74 } })).unwrap();
        for language in ["en", "ru"] {
            zen_i18n::set_language(Some(language));
            for (open, name) in [(true, "details"), (false, "columns")] {
                run(&mut app, "vcs.workspace.details", json!({ "open": open })).unwrap();
                let image = draw(&mut app);
                let path = Path::new(&dir).join(format!("git-workspace-narrow-{name}-{language}.png"));
                crate::offscreen::write_png(&path, &image).unwrap();
            }
        }
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn a_smaller_clipboard_limit_applies_at_once() {
        let data = tempfile::tempdir().unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        let ctx = egui::Context::default();
        zoom_pass(&ctx, &mut app);
        for text in ["a", "b", "c", "d"] {
            app.shared.clipboard_history.record(text, 20);
        }
        app.tool(&ctx, "set_settings", &json!({ "clipboardLimit": 2 })).unwrap();
        zoom_pass(&ctx, &mut app);
        assert_eq!(app.shared.clipboard_history.len(), 2, "no command has to run first");
        assert_eq!(app.shared.clipboard_history.get(0).as_deref(), Some("d"));
    }

    #[test]
    fn typing_in_the_projects_popup_filters_in_either_layout_and_enter_opens() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let billing = data.path().join("billing-service");
        let skills = data.path().join("fast-path-search");
        for folder in [&billing, &skills] {
            std::fs::create_dir(folder).unwrap();
        }
        std::fs::create_dir_all(billing.join(".git")).unwrap();
        std::fs::write(billing.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let recent = json!({ "projects": [
            { "path": skills, "opened": 2, "open": false },
            { "path": billing, "opened": 1, "open": false },
        ] });
        std::fs::write(data.path().join("recent.json"), recent.to_string()).unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let popup = |app: &mut IdeApp, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": "view.projects.popup", "params": params })).unwrap()
        };
        let names = |answer: &Value| -> Vec<String> {
            answer["projects"].as_array().unwrap().iter().map(|p| p["name"].as_str().unwrap().to_owned()).collect()
        };

        let opened = popup(&mut app, json!({}));
        assert_eq!(opened["window"]["popups"], json!(["projects"]), "{opened}");
        assert_eq!(names(&opened["result"]), ["project", "fast-path-search", "billing-service"], "{opened}");
        assert_eq!(opened["result"]["projects"][2]["branch"], "main", "the branch under the path");

        let typed = ask(&ctx, &mut app, "press_keys", json!({ "text": "ишд" })).unwrap();
        assert_eq!(typed["window"]["popups"], json!(["projects"]), "{typed}");
        let shown = popup(&mut app, json!({ "open": true }));
        assert_eq!(shown["result"]["filter"], "ишд", "{shown}");
        assert_eq!(names(&shown["result"]), ["billing-service"], "{shown}");
        assert_eq!(shown["result"]["projects"][0]["selected"], true, "{shown}");
        let none = popup(&mut app, json!({ "filter": "zzz" }));
        assert_eq!(names(&none["result"]), Vec::<String>::new(), "{none}");
        popup(&mut app, json!({ "filter": "fps" }));

        let entered = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        assert_eq!(entered["window"]["popups"], json!([]), "{entered}");
        frame(&ctx, &mut app);
        let windows = app.tool(&ctx, "list_windows", &json!({})).unwrap();
        let paths: Vec<&str> =
            windows["windows"].as_array().unwrap().iter().filter_map(|w| w["project"].as_str()).collect();
        assert!(paths.iter().any(|path| path.ends_with("fast-path-search")), "{windows}");
    }

    #[test]
    fn bookmarks_by_their_keys_and_dialogs_survive_a_restart() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let file = file.canonicalize().unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        let keys = |app: &mut IdeApp, keys: Value| ask(&ctx, app, "press_keys", json!({ "keys": keys })).unwrap();
        let listed = |app: &mut IdeApp| run(app, "navigate.bookmark.list", json!({}))["bookmarks"].clone();
        let line = |app: &mut IdeApp| run(app, "editor.selection.get", json!({}))[0]["line"].clone();
        run(&mut app, "file.document.open", json!({ "path": file }));

        keys(&mut app, json!(["F3"]));
        assert_eq!(listed(&mut app), json!([{ "path": file, "line": 1 }]), "F3 toggles at the caret");
        run(&mut app, "editor.caret.goTo", json!({ "line": 3, "column": 1 }));
        keys(&mut app, json!(["⌃⇧1"]));
        assert_eq!(listed(&mut app)[1], json!({ "path": file, "line": 3, "mnemonic": "1" }));
        run(&mut app, "editor.caret.goTo", json!({ "line": 1, "column": 1 }));
        keys(&mut app, json!(["⌃1"]));
        assert_eq!(line(&mut app), 2, "⌃1 goes to bookmark 1");
        let back = run(&mut app, "navigate.history.list", json!({}))["back"].clone();
        assert_eq!(back.as_array().unwrap().last().unwrap()["line"], 1, "the jump is in Back");

        let opened =
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.bookmark.mnemonic", "params": { "line": 5 } }))
                .unwrap();
        assert_eq!(opened["window"]["popups"], json!(["bookmarkMnemonic"]), "{opened}");
        ask(&ctx, &mut app, "press_keys", json!({ "text": "k" })).unwrap();
        let typed = ask(&ctx, &mut app, "press_keys", json!({ "text": "done" })).unwrap();
        assert_eq!(typed["window"]["popups"], json!(["bookmarkMnemonic"]), "{typed}");
        let set = keys(&mut app, json!(["⏎"]));
        assert_eq!(set["window"]["popups"], json!([]), "{set}");
        assert_eq!(listed(&mut app)[2], json!({ "path": file, "line": 5, "mnemonic": "K", "description": "done" }));

        let asked =
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.bookmark.mnemonic", "params": { "line": 1 } }))
                .unwrap();
        assert_eq!(asked["window"]["popups"], json!(["bookmarkMnemonic"]), "{asked}");
        ask(&ctx, &mut app, "press_keys", json!({ "text": "1" })).unwrap();
        keys(&mut app, json!(["⏎"]));
        let question = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(question["popups"], json!(["bookmarkReplace"]), "a taken mnemonic asks first: {question}");
        keys(&mut app, json!(["⎋"]));
        keys(&mut app, json!(["⎋"]));

        let go = ask(&ctx, &mut app, "run_action", json!({ "action": "view.bookmark.goToMnemonic" })).unwrap();
        assert_eq!(go["window"]["popups"], json!(["goToMnemonic"]), "{go}");
        let went = ask(&ctx, &mut app, "press_keys", json!({ "text": "k" })).unwrap();
        assert_eq!(went["window"]["popups"], json!([]), "{went}");
        assert_eq!(line(&mut app), 4);

        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(saved.contains("\"mnemonic\": \"K\""), "{saved}");
        assert!(
            !project.join(".idea").exists() && std::fs::read_dir(&project).unwrap().count() == 1,
            "nothing in the project"
        );
        drop(app);
        let mut app = themed_app(data.path(), vec![project]);
        frame(&ctx, &mut app);
        assert_eq!(listed(&mut app).as_array().map(Vec::len), Some(3), "the bookmarks survive a restart");
    }

    #[test]
    fn the_bookmarks_tool_window_and_the_line_bookmarks_popup_end_to_end() {
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let file = file.canonicalize().unwrap();
        let shop = file.with_file_name("shop.go");
        let body: String = (1..=12).map(|n| format!("// step {n}\n")).collect();
        std::fs::write(&shop, format!("package main\n\n{body}")).unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let screen = std::cell::RefCell::new(crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0));
        let step = |app: &mut IdeApp, events: Vec<egui::Event>| {
            screen.borrow_mut().step(events, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        };
        let ask = |app: &mut IdeApp, tool: &str, arguments: Value| -> Result<Value, String> {
            let (sender, answer) = mpsc::channel();
            let ctx = screen.borrow().context().clone();
            app.route(
                &ctx,
                tool,
                &arguments,
                Box::new(move |result| {
                    let _ = sender.send(result);
                }),
            );
            for _ in 0..5 {
                if let Ok(result) = answer.try_recv() {
                    return result;
                }
                step(app, Vec::new());
            }
            answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments}"))
        };
        step(&mut app, Vec::new());
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        let action = |app: &mut IdeApp, action: &str| ask(app, "run_action", json!({ "action": action })).unwrap();
        let keys = |app: &mut IdeApp, keys: Value| ask(app, "press_keys", json!({ "keys": keys })).unwrap();
        let caret = |app: &mut IdeApp| -> String {
            let state = ask(app, "get_window_state", json!({})).unwrap();
            let line = run(app, "editor.selection.get", json!({}))[0]["line"].as_u64().unwrap();
            let editor = state["editor"].as_str().unwrap_or_default().rsplit('/').next().unwrap().to_owned();
            format!("{editor}:{line}")
        };
        let shown = |answer: &Value| {
            answer["window"]["toolWindows"]["windows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|window| window["id"] == "bookmarks")
                .map(|window| (window["side"].clone(), window["visible"].clone()))
                .unwrap()
        };
        let groups = |app: &mut IdeApp| {
            let listed = run(app, "navigate.bookmark.list", json!({}));
            listed["groups"]
                .as_array()
                .unwrap()
                .iter()
                .map(|group| {
                    let lines: Vec<String> = group["bookmarks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|b| format!("{}:{}", b["path"].as_str().unwrap().rsplit('/').next().unwrap(), b["line"]))
                        .collect();
                    let tag = if group["isDefault"] == true { " (Default)" } else { "" };
                    format!("{}{tag} [{}]", group["name"].as_str().unwrap(), lines.join(" "))
                })
                .collect::<Vec<_>>()
        };

        run(&mut app, "file.document.open", json!({ "path": file }));
        let opened = keys(&mut app, json!(["⌘2"]));
        assert_eq!(shown(&opened), (json!("leftBottom"), json!(true)), "⌘2 shows it on the left stripe: {opened}");
        run(&mut app, "navigate.bookmark.toggle", json!({ "path": file, "line": 3 }));
        run(
            &mut app,
            "navigate.bookmark.setMnemonic",
            json!({ "path": file, "line": 4, "mnemonic": "k", "description": "print" }),
        );
        run(&mut app, "navigate.bookmarkList.create", json!({ "name": "Review" }));
        run(&mut app, "navigate.bookmark.toggle", json!({ "path": shop, "line": 5 }));
        run(&mut app, "navigate.bookmark.toggle", json!({ "path": shop, "line": 10 }));
        run(&mut app, "navigate.bookmark.moveToList", json!({ "path": shop, "line": 5, "list": "Review" }));
        run(
            &mut app,
            "navigate.bookmark.moveToList",
            json!({ "path": shop, "line": 10, "list": "Review", "index": 0 }),
        );
        run(&mut app, "debug.breakpoint.toggle", json!({ "path": file, "line": 3 }));
        assert_eq!(groups(&mut app), ["project (Default) [main.go:3 main.go:4]", "Review [shop.go:10 shop.go:5]"]);
        run(&mut app, "navigate.bookmark.moveDown", json!({ "path": shop, "line": 10 }));
        run(&mut app, "navigate.bookmarkList.setDefault", json!({ "name": "Review" }));
        assert_eq!(
            groups(&mut app),
            ["Review (Default) [shop.go:5 shop.go:10]", "project [main.go:3 main.go:4]"],
            "Mark List as Default moves it to the top"
        );
        let draw = |app: &mut IdeApp| {
            screen.borrow_mut().render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            })
        };
        draw(&mut app);
        draw(&mut app);
        let tree = screen.borrow_mut().accessibility_tree(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        for name in [
            "Review",
            "project",
            "print",
            "shop.go:5",
            "shop.go:10",
            "main.go:3",
            &zen_i18n::t!("ide-bookmarks-breakpoints"),
        ] {
            assert!(
                crate::accessibility::testing::find(&tree, egui::accesskit::Role::TreeItem, name).is_some(),
                "{name} is a row of the tree"
            );
        }

        let first = action(&mut app, "view.bookmarks.next");
        assert_eq!(first["result"]["line"], 5, "{first}");
        assert_eq!(caret(&mut app), "shop.go:4", "the first bookmark of the tree, in the default list");
        action(&mut app, "view.bookmarks.next");
        action(&mut app, "view.bookmarks.next");
        assert_eq!(caret(&mut app), "main.go:2");
        action(&mut app, "view.bookmarks.previous");
        action(&mut app, "view.bookmarks.previous");
        action(&mut app, "view.bookmarks.previous");
        assert_eq!(caret(&mut app), "main.go:3", "round to the breakpoint, the last row");
        let back = run(&mut app, "navigate.history.list", json!({}))["back"].clone();
        assert!(!back.as_array().unwrap().is_empty(), "the jumps are in Back");

        let press =
            |app: &mut IdeApp, role: egui::accesskit::Role, name: &str, button: egui::PointerButton, clicks: usize| {
                let tree = screen.borrow_mut().accessibility_tree(|ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
                let b = tree
                    .nodes
                    .iter()
                    .filter(|(_, node)| node.role() == role && node.label() == Some(name))
                    .filter_map(|(_, node)| node.bounds())
                    .max_by(|a, b| a.y0.total_cmp(&b.y0))
                    .unwrap_or_else(|| panic!("no {role:?} {name}"));
                let at = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
                let event =
                    |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
                let mut events = vec![vec![egui::Event::PointerMoved(at)]];
                for _ in 0..clicks {
                    events.extend([vec![event(true)], vec![event(false)]]);
                }
                events.push(Vec::new());
                for events in events {
                    step(app, events);
                }
            };
        let click = |app: &mut IdeApp, name: &str, clicks: usize| {
            press(app, egui::accesskit::Role::TreeItem, name, egui::PointerButton::Primary, clicks);
        };
        click(&mut app, "print", 2);
        assert_eq!(caret(&mut app), "main.go:3", "a double-click opens the bookmark");
        click(&mut app, "shop.go:5", 1);
        keys(&mut app, json!(["⏎"]));
        assert_eq!(caret(&mut app), "shop.go:4", "Enter opens the selected one");
        click(&mut app, "shop.go:5", 1);
        keys(&mut app, json!(["⌃⌥↓"]));
        assert_eq!(caret(&mut app), "shop.go:9", "⌃⌥↓ goes to the next one in the window");
        keys(&mut app, json!(["⌃⌥↑"]));
        assert_eq!(caret(&mut app), "shop.go:4", "⌃⌥↑ to the previous one");
        keys(&mut app, json!(["⌥⌘↓"]));
        assert_eq!(caret(&mut app), "shop.go:9", "⌥⌘↓, Next Occurrence, does it too");
        keys(&mut app, json!(["⌥⌘↑"]));
        assert_eq!(caret(&mut app), "shop.go:4");
        click(&mut app, "shop.go:10", 1);
        keys(&mut app, json!(["⌦"]));
        assert_eq!(groups(&mut app)[0], "Review (Default) [shop.go:5]", "Delete removes the selected one");
        run(&mut app, "navigate.bookmark.toggle", json!({ "path": shop, "line": 10 }));

        let popup = action(&mut app, "view.bookmark.show");
        assert_eq!(popup["window"]["popups"], json!(["lineBookmarks"]), "{popup}");
        assert_eq!(
            popup["result"]["rows"],
            json!(["[Review]", "shop.go:5", "shop.go:10", "[project]", "main.go:3", "K main.go:4 print"]),
            "line bookmarks only, under their lists"
        );
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                let image = draw(&mut app);
                crate::offscreen::write_png(&Path::new(&dir).join(format!("bookmarks-popup-{language}.png")), &image)
                    .unwrap();
            }
            zen_i18n::set_language(Some("en"));
        }
        let jumped = ask(&mut app, "press_keys", json!({ "text": "k" })).unwrap();
        assert_eq!(jumped["window"]["popups"], json!([]), "a typed mnemonic jumps at once: {jumped}");
        assert_eq!(caret(&mut app), "main.go:3");
        let shortcut = keys(&mut app, json!(["⌘F3"]));
        assert_eq!(shortcut["window"]["popups"], json!(["lineBookmarks"]), "⌘F3 opens it: {shortcut}");
        keys(&mut app, json!(["↓", "⏎"]));
        assert_eq!(caret(&mut app), "shop.go:9", "Enter opens the selected one");
        ask(&mut app, "set_settings", json!({ "bookmarksPopupLineOnly": false })).unwrap();
        step(&mut app, Vec::new());
        step(&mut app, Vec::new());
        let all = action(&mut app, "view.bookmark.show");
        assert_eq!(all["result"]["rows"].as_array().unwrap().last(), Some(&json!("main.go:4")), "{all}");
        let closed = keys(&mut app, json!(["⎋"]));
        assert_eq!(closed["window"]["popups"], json!([]), "⎋ closes it: {closed}");

        run(&mut app, "navigate.bookmark.options", json!({ "groupByFile": true }));
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                for (group, name) in [(true, "by-file"), (false, "flat")] {
                    run(&mut app, "navigate.bookmark.options", json!({ "groupByFile": group }));
                    draw(&mut app);
                    let image = draw(&mut app);
                    let path = Path::new(&dir).join(format!("bookmarks-tool-window-{name}-{language}.png"));
                    crate::offscreen::write_png(&path, &image).unwrap();
                }
            }
            zen_i18n::set_language(Some("en"));
        }
        run(&mut app, "navigate.bookmark.options", json!({ "groupByFile": false }));
        let (tree_item, button, check) =
            (egui::accesskit::Role::TreeItem, egui::accesskit::Role::Button, egui::accesskit::Role::CheckBox);
        let (primary, secondary) = (egui::PointerButton::Primary, egui::PointerButton::Secondary);
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                press(&mut app, tree_item, "project", secondary, 1);
                let image = draw(&mut app);
                let path = Path::new(&dir).join(format!("bookmarks-list-menu-{language}.png"));
                crate::offscreen::write_png(&path, &image).unwrap();
                keys(&mut app, json!(["⎋"]));
                press(&mut app, tree_item, "print", secondary, 1);
                let image = draw(&mut app);
                let path = Path::new(&dir).join(format!("bookmarks-bookmark-menu-{language}.png"));
                crate::offscreen::write_png(&path, &image).unwrap();
                keys(&mut app, json!(["⎋"]));
                press(&mut app, button, &zen_i18n::t!("ide-structure-options"), primary, 1);
                let image = draw(&mut app);
                let path = Path::new(&dir).join(format!("bookmarks-options-menu-{language}.png"));
                crate::offscreen::write_png(&path, &image).unwrap();
                keys(&mut app, json!(["⎋"]));
            }
            zen_i18n::set_language(Some("en"));
        }
        press(&mut app, tree_item, "project", secondary, 1);
        press(&mut app, button, &zen_i18n::t!("command-navigate-bookmarkList-setDefault"), primary, 1);
        assert_eq!(
            groups(&mut app),
            ["project (Default) [main.go:3 main.go:4]", "Review [shop.go:5 shop.go:10]"],
            "the list's context menu marks it as the default"
        );
        press(&mut app, tree_item, "shop.go:10", secondary, 1);
        press(&mut app, button, &zen_i18n::t!("command-navigate-bookmark-moveUp"), primary, 1);
        assert_eq!(groups(&mut app)[1], "Review [shop.go:10 shop.go:5]", "Move Up from the bookmark's context menu");
        press(&mut app, button, &zen_i18n::t!("ide-structure-options"), primary, 1);
        press(&mut app, check, &zen_i18n::t!("ide-bookmarks-group-by-file"), primary, 1);
        assert_eq!(
            run(&mut app, "navigate.bookmark.list", json!({}))["groupByFile"],
            true,
            "⋮ → Group Line Bookmarks by File"
        );
        press(&mut app, button, &zen_i18n::t!("ide-structure-options"), primary, 1);
        press(&mut app, check, &zen_i18n::t!("ide-bookmarks-ask-mnemonic"), primary, 1);
        assert_eq!(
            run(&mut app, "navigate.bookmark.list", json!({}))["replaceWithoutAsking"],
            true,
            "⋮ → Ask Before Rewriting Mnemonic off"
        );
        keys(&mut app, json!(["⎋"]));
        run(&mut app, "navigate.bookmarkList.rename", json!({ "name": "project", "newName": "Later" }));
        let ctx = screen.borrow().context().clone();
        app.remember_workspace(&ctx, true);
        drop(app);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        step(&mut app, Vec::new());
        assert_eq!(
            groups(&mut app),
            ["Later (Default) [main.go:3 main.go:4]", "Review [shop.go:10 shop.go:5]"],
            "the lists, their order and the default survive a restart"
        );
        assert_eq!(run(&mut app, "navigate.bookmark.list", json!({}))["groupByFile"], true);
        let state = ask(&mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(
            shown(&json!({ "window": state })),
            (json!("leftBottom"), json!(true)),
            "and the window is shown again"
        );
        let hidden = keys(&mut app, json!(["⌘2"]));
        assert_eq!(shown(&hidden).1, json!(false), "⌘2 again hides it");
        run(&mut app, "navigate.bookmarkList.delete", json!({ "name": "Review" }));
        assert_eq!(groups(&mut app), ["Later (Default) [main.go:3 main.go:4]"], "a deleted list takes its bookmarks");
        assert_eq!(run(&mut app, "navigate.bookmark.list", json!({}))["replaceWithoutAsking"], true);
        assert!(
            run(&mut app, "navigate.bookmark.list", json!({ "path": shop }))["bookmarks"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn file_and_folder_bookmarks_from_the_project_tree_and_the_tabs_end_to_end() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        let main = project.join(file.file_name().unwrap());
        let shop = project.join("shop.go");
        std::fs::write(&shop, "package main\n\nvar total = 0\n").unwrap();
        let pkg = project.join("pkg");
        std::fs::create_dir(&pkg).unwrap();
        std::fs::write(pkg.join("util.go"), "package pkg\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let screen = std::cell::RefCell::new(crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), 2.0));
        let step = |app: &mut IdeApp, events: Vec<egui::Event>| {
            screen.borrow_mut().step(events, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        };
        let ask = |app: &mut IdeApp, tool: &str, arguments: Value| -> Result<Value, String> {
            let (sender, answer) = mpsc::channel();
            let ctx = screen.borrow().context().clone();
            app.route(
                &ctx,
                tool,
                &arguments,
                Box::new(move |result| {
                    let _ = sender.send(result);
                }),
            );
            for _ in 0..5 {
                if let Ok(result) = answer.try_recv() {
                    return result;
                }
                step(app, Vec::new());
            }
            answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments}"))
        };
        step(&mut app, Vec::new());
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        let keys = |app: &mut IdeApp, keys: Value| ask(app, "press_keys", json!({ "keys": keys })).unwrap();
        let text = |app: &mut IdeApp, text: &str| ask(app, "press_keys", json!({ "text": text })).unwrap();
        let select = |app: &mut IdeApp, paths: Value| {
            ask(app, "run_action", json!({ "action": "view.project.select", "params": { "paths": paths } })).unwrap()
        };
        let marks = |app: &mut IdeApp| -> Vec<String> {
            let listed = run(app, "navigate.bookmark.list", json!({}));
            listed["bookmarks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| {
                    let name = b["path"].as_str().unwrap().rsplit('/').next().unwrap().to_owned();
                    let place = match b["line"].as_u64() {
                        Some(line) => format!("{name}:{line}"),
                        None if b["folder"] == true => format!("{name}/"),
                        None => name,
                    };
                    match b["mnemonic"].as_str() {
                        Some(mnemonic) => format!("{place} {mnemonic}"),
                        None => place,
                    }
                })
                .collect()
        };
        let groups = |app: &mut IdeApp| -> Vec<String> {
            let listed = run(app, "navigate.bookmark.list", json!({}));
            listed["groups"]
                .as_array()
                .unwrap()
                .iter()
                .map(|group| {
                    let names: Vec<&str> = group["bookmarks"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|b| b["path"].as_str().unwrap().rsplit('/').next().unwrap())
                        .collect();
                    format!("{} [{}]", group["name"].as_str().unwrap(), names.join(" "))
                })
                .collect()
        };
        let draw = |app: &mut IdeApp| {
            screen.borrow_mut().render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            })
        };
        let tree = |app: &mut IdeApp| {
            screen.borrow_mut().accessibility_tree(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            })
        };
        let press = |app: &mut IdeApp,
                     role: egui::accesskit::Role,
                     name: &str,
                     button: egui::PointerButton,
                     clicks: usize,
                     rightmost: bool| {
            let found = (0..4).find_map(|_| {
                let tree = tree(app);
                tree.nodes
                    .iter()
                    .filter(|(_, node)| {
                        node.role() == role && node.label().is_some_and(|label| label.starts_with(name))
                    })
                    .filter_map(|(_, node)| Some((node.bounds()?, node.label().map_or(0, |label| label.len()))))
                    .filter(|(bounds, _)| !rightmost || bounds.x0 > 40.0)
                    .max_by(|(a, a_len), (b, b_len)| {
                        let (a_at, b_at) = if rightmost { (a.x0, b.x0) } else { (a.y0, b.y0) };
                        a_at.total_cmp(&b_at).then(b_len.cmp(a_len))
                    })
                    .map(|(bounds, _)| bounds)
            });
            let b = found.unwrap_or_else(|| panic!("no {role:?} {name}"));
            let at = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
            let event =
                |pressed| egui::Event::PointerButton { pos: at, button, pressed, modifiers: egui::Modifiers::NONE };
            let mut events = vec![vec![egui::Event::PointerMoved(at)]];
            for _ in 0..clicks {
                events.extend([vec![event(true)], vec![event(false)]]);
            }
            events.push(Vec::new());
            for events in events {
                step(app, events);
            }
        };
        let (button, tree_item, tab) =
            (egui::accesskit::Role::Button, egui::accesskit::Role::TreeItem, egui::accesskit::Role::Tab);
        let (primary, secondary) = (egui::PointerButton::Primary, egui::PointerButton::Secondary);
        let menu = |app: &mut IdeApp, label: &str| {
            press(app, button, &zen_i18n::t!(label), primary, 1, true);
        };

        while app.splash.is_active() {
            step(&mut app, Vec::new());
        }
        run(&mut app, "file.document.open", json!({ "path": main }));
        run(&mut app, "file.document.open", json!({ "path": shop }));
        select(&mut app, json!([main]));
        keys(&mut app, json!(["F3"]));
        assert_eq!(marks(&mut app), ["main.go"], "F3 in the Project tool window bookmarks the selected file");
        select(&mut app, json!([shop, pkg]));
        keys(&mut app, json!(["F3"]));
        assert_eq!(marks(&mut app), ["main.go", "pkg/", "shop.go"], "and several at once");
        keys(&mut app, json!(["F3"]));
        assert_eq!(marks(&mut app), ["main.go"], "F3 again removes them");
        keys(&mut app, json!(["F3"]));
        select(&mut app, json!([pkg]));
        let chooser = keys(&mut app, json!(["⌥F3"]));
        assert_eq!(chooser["window"]["popups"], json!(["bookmarkMnemonic"]), "⌥F3 asks for a mnemonic: {chooser}");
        text(&mut app, "p");
        keys(&mut app, json!(["⏎"]));
        assert_eq!(marks(&mut app), ["main.go", "pkg/ P", "shop.go"]);

        let open_menu = |app: &mut IdeApp, path: &Path| {
            ask(app, "run_action", json!({ "action": "view.project.contextMenu", "params": { "path": path } }))
                .unwrap();
            menu(app, "ide-tool-bookmarks");
        };
        open_menu(&mut app, &shop);
        menu(&mut app, "ide-bookmarks-delete");
        assert_eq!(marks(&mut app), ["main.go", "pkg/ P"], "Bookmarks › Delete");
        open_menu(&mut app, &shop);
        menu(&mut app, "keymap-navigate-bookmark-toggle");
        assert_eq!(marks(&mut app), ["main.go", "pkg/ P", "shop.go"], "Bookmarks › Toggle Bookmark");
        select(&mut app, json!([main, shop]));
        open_menu(&mut app, &main);
        menu(&mut app, "keymap-navigate-bookmark-toggle");
        assert_eq!(marks(&mut app), ["pkg/ P"], "on two rows, both bookmarked: both go");
        open_menu(&mut app, &main);
        menu(&mut app, "keymap-navigate-bookmark-toggle");
        assert_eq!(marks(&mut app), ["main.go", "pkg/ P", "shop.go"]);
        select(&mut app, json!([main, shop, pkg]));
        open_menu(&mut app, &shop);
        menu(&mut app, "ide-bookmarks-delete");
        assert_eq!(marks(&mut app), Vec::<String>::new(), "Delete on three rows");
        select(&mut app, json!([main, shop, pkg]));
        keys(&mut app, json!(["F3"]));
        select(&mut app, json!([pkg]));
        keys(&mut app, json!(["⌥F3"]));
        text(&mut app, "p");
        keys(&mut app, json!(["⏎"]));
        open_menu(&mut app, &main);
        menu(&mut app, "ide-bookmark-toggle-mnemonic");
        let state = ask(&mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!(["bookmarkMnemonic"]), "Bookmarks › Toggle Bookmark Mnemonic…: {state}");
        text(&mut app, "m");
        keys(&mut app, json!(["⏎"]));
        assert_eq!(marks(&mut app), ["main.go M", "pkg/ P", "shop.go"]);
        assert!(marks(&mut app).iter().all(|mark| !mark.contains(':')), "no line bookmark was set");

        let described = |app: &mut IdeApp, name: &str| -> Vec<String> {
            let tree = tree(app);
            tree.nodes
                .iter()
                .filter(|(_, node)| node.role() == tree_item && node.label() == Some(name))
                .filter_map(|(_, node)| node.description().map(str::to_owned))
                .collect()
        };
        assert_eq!(described(&mut app, "main.go"), [zen_i18n::t!("ide-bookmark-hint-mnemonic", mnemonic = "M")]);
        assert_eq!(described(&mut app, "shop.go"), [zen_i18n::t!("ide-bookmark-hint")], "the tree marks them");
        assert_eq!(described(&mut app, "util.go"), Vec::<String>::new());
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            ask(&mut app, "run_action", json!({ "action": "view.project.select", "params": { "paths": [pkg] } }))
                .unwrap();
            run(&mut app, "file.document.open", json!({ "path": pkg.join("util.go") }));
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                draw(&mut app);
                let image = draw(&mut app);
                let path = Path::new(&dir).join(format!("project-tree-bookmarks-{language}.png"));
                crate::offscreen::write_png(&path, &image).unwrap();
                select(&mut app, json!([main, shop]));
                open_menu(&mut app, &shop);
                draw(&mut app);
                let image = draw(&mut app);
                let path = Path::new(&dir).join(format!("project-bookmarks-menu-{language}.png"));
                crate::offscreen::write_png(&path, &image).unwrap();
                keys(&mut app, json!(["⎋"]));
                keys(&mut app, json!(["⎋"]));
                press(&mut app, tab, "shop.go", secondary, 1, true);
                menu(&mut app, "ide-tool-bookmarks");
                draw(&mut app);
                let image = draw(&mut app);
                let path = Path::new(&dir).join(format!("tab-bookmarks-menu-{language}.png"));
                crate::offscreen::write_png(&path, &image).unwrap();
                keys(&mut app, json!(["⎋"]));
                keys(&mut app, json!(["⎋"]));
            }
            zen_i18n::set_language(Some("en"));
            run(&mut app, "file.document.close", json!({ "path": pkg.join("util.go") }));
        }

        press(&mut app, tab, "shop.go", secondary, 1, true);
        menu(&mut app, "ide-tool-bookmarks");
        menu(&mut app, "ide-bookmarks-delete");
        assert_eq!(marks(&mut app), ["main.go M", "pkg/ P"], "the tab's Bookmarks › Delete");
        press(&mut app, tab, "shop.go", secondary, 1, true);
        menu(&mut app, "ide-tool-bookmarks");
        menu(&mut app, "keymap-navigate-bookmark-toggle");
        assert_eq!(marks(&mut app), ["main.go M", "pkg/ P", "shop.go"], "the tab's Bookmarks › Toggle Bookmark");
        press(&mut app, tab, "shop.go", secondary, 1, true);
        menu(&mut app, "ide-tool-bookmarks");
        menu(&mut app, "ide-bookmark-toggle-mnemonic");
        text(&mut app, "s");
        keys(&mut app, json!(["⏎"]));
        assert_eq!(marks(&mut app), ["main.go M", "pkg/ P", "shop.go S"], "the tab's Toggle Bookmark Mnemonic…");
        let project_list = project.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(groups(&mut app), [format!("{project_list} [main.go pkg shop.go]")], "all in the default list");

        press(&mut app, tab, "main.go", secondary, 1, true);
        menu(&mut app, "ide-tool-bookmarks");
        menu(&mut app, "keymap-view-bookmark-openTabs");
        let state = ask(&mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!(["bookmarkOpenTabs"]), "Bookmark Open Tabs… asks for a name: {state}");
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                draw(&mut app);
                let image = draw(&mut app);
                let path = Path::new(&dir).join(format!("bookmark-open-tabs-{language}.png"));
                crate::offscreen::write_png(&path, &image).unwrap();
            }
            zen_i18n::set_language(Some("en"));
        }
        text(&mut app, "Review");
        keys(&mut app, json!(["⏎"]));
        assert_eq!(
            groups(&mut app),
            [format!("{project_list} [pkg]"), "Review [main.go shop.go]".to_owned()],
            "a new list with every open tab"
        );
        let agent = ask(&mut app, "run_action", json!({ "action": "view.bookmark.openTabs" })).unwrap();
        assert_eq!(agent["window"]["popups"], json!(["bookmarkOpenTabs"]), "{agent}");
        keys(&mut app, json!(["⎋"]));
        let tabs = run(&mut app, "navigate.bookmarkList.fromOpenTabs", json!({ "name": "Tabs" }));
        assert_eq!(tabs["bookmarks"].as_array().map(Vec::len), Some(2), "{tabs}");
        for path in [&main, &shop] {
            run(&mut app, "navigate.bookmark.moveToList", json!({ "path": path, "whole": true, "list": "Review" }));
        }
        assert_eq!(run(&mut app, "navigate.bookmarkList.delete", json!({ "name": "Tabs" })), json!(0));
        run(&mut app, "navigate.bookmark.toggle", json!({ "path": main, "line": 4 }));
        run(&mut app, "navigate.bookmark.moveToList", json!({ "path": main, "line": 4, "list": "Review" }));

        let shown = keys(&mut app, json!(["⌘2"]));
        assert!(shown["window"].to_string().contains("bookmarks"), "{shown}");
        draw(&mut app);
        for name in ["Review", "pkg", "main.go", "shop.go", "main.go:4"] {
            let tree = tree(&mut app);
            assert!(
                crate::accessibility::testing::find(&tree, tree_item, name).is_some(),
                "{name} is a row of the Bookmarks tool window"
            );
        }
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                for (group, name) in [(false, "flat"), (true, "by-file")] {
                    run(&mut app, "navigate.bookmark.options", json!({ "groupByFile": group }));
                    draw(&mut app);
                    let image = draw(&mut app);
                    let path = Path::new(&dir).join(format!("bookmarks-tool-window-files-{name}-{language}.png"));
                    crate::offscreen::write_png(&path, &image).unwrap();
                }
            }
            zen_i18n::set_language(Some("en"));
            run(&mut app, "navigate.bookmark.options", json!({ "groupByFile": false }));
        }
        let apart = |app: &mut IdeApp| {
            for _ in 0..30 {
                step(app, Vec::new());
            }
        };
        run(&mut app, "file.document.open", json!({ "path": main }));
        apart(&mut app);
        press(&mut app, tree_item, "pkg", primary, 2, false);
        let selection = ask(&mut app, "get_tree_selection", json!({})).unwrap();
        assert_eq!(selection["paths"], json!([pkg]), "a folder's bookmark selects it in the Project tool window");
        let state = ask(&mut app, "get_window_state", json!({})).unwrap();
        assert!(state["editor"].as_str().unwrap().ends_with("main.go"), "{state}");
        apart(&mut app);
        press(&mut app, tree_item, "shop.go", primary, 2, false);
        let state = ask(&mut app, "get_window_state", json!({})).unwrap();
        assert!(state["editor"].as_str().unwrap().ends_with("shop.go"), "a file's bookmark opens it: {state}");
        run(&mut app, "file.document.open", json!({ "path": main }));
        press(&mut app, tree_item, "shop.go", primary, 1, false);
        keys(&mut app, json!(["⏎"]));
        let state = ask(&mut app, "get_window_state", json!({})).unwrap();
        assert!(state["editor"].as_str().unwrap().ends_with("shop.go"), "and Enter does: {state}");
        run(&mut app, "file.document.open", json!({ "path": main }));
        run(&mut app, "navigate.bookmark.goTo", json!({ "mnemonic": "S" }));
        let state = ask(&mut app, "get_window_state", json!({})).unwrap();
        assert!(state["editor"].as_str().unwrap().ends_with("shop.go"), "Go to Bookmark S opens the file: {state}");
        let refused = ask(
            &mut app,
            "dispatch_command",
            json!({ "command": "navigate.bookmark.goTo", "params": { "mnemonic": "P" } }),
        );
        assert!(refused.is_err(), "an agent learns the folder's bookmark is a folder: {refused:?}");

        let ctx = screen.borrow().context().clone();
        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(saved.contains("\"files\"") && saved.contains("\"index\""), "{saved}");
        drop(app);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        step(&mut app, Vec::new());
        assert_eq!(marks(&mut app), ["main.go M", "main.go:4", "pkg/ P", "shop.go S"], "they survive a restart");
        assert_eq!(
            groups(&mut app),
            [format!("{project_list} [pkg]"), "Review [main.go shop.go main.go]".to_owned()],
            "in their lists and order"
        );

        while app.splash.is_active() {
            step(&mut app, Vec::new());
        }
        run(&mut app, "file.document.open", json!({ "path": main }));
        run(&mut app, "file.document.open", json!({ "path": shop }));
        let popups = |app: &mut IdeApp| ask(app, "get_window_state", json!({})).unwrap()["popups"].clone();
        for (item, popup) in [
            ("keymap-view-localHistory-show", "localHistory"),
            ("keymap-view-localHistory-putLabel", "localHistoryLabel"),
        ] {
            press(&mut app, tab, "main.go", secondary, 1, true);
            menu(&mut app, "menu-file-local-history");
            menu(&mut app, item);
            assert_eq!(popups(&mut app), json!([popup]), "the tab menu's Local History › {item}");
            keys(&mut app, json!(["⎋"]));
            assert_eq!(popups(&mut app), json!([]));
        }
        let splits = |app: &mut IdeApp| !ask(app, "get_window_state", json!({})).unwrap()["splits"].is_null();
        press(&mut app, tab, "shop.go", secondary, 1, true);
        menu(&mut app, "keymap-editor-split-right");
        assert!(splits(&mut app), "the tab menu's Split Right");
        press(&mut app, tab, "shop.go", secondary, 1, true);
        menu(&mut app, "keymap-editor-split-unsplitAll");
        assert!(!splits(&mut app), "and Unsplit All");
        press(&mut app, tab, "shop.go", secondary, 1, true);
        menu(&mut app, "keymap-file-document-close");
        let state = ask(&mut app, "get_window_state", json!({})).unwrap();
        assert!(!state.to_string().contains("shop.go\""), "the tab menu's Close Tab: {state}");

        let created = |app: &mut IdeApp, command: &str, name: &str| {
            PathBuf::from(run(app, command, json!({ "directory": project, "name": name })).as_str().unwrap())
        };
        let notes = created(&mut app, "file.fs.newFile", "notes.go");
        let docs = created(&mut app, "file.fs.newDirectory", "docs");
        let dep = created(&mut app, "file.fs.newDirectory", "node_modules/dep");
        let built = created(&mut app, "file.fs.newFile", "target/debug/out.txt");
        select(&mut app, json!([notes, docs, dep, built]));
        keys(&mut app, json!(["F3"]));
        assert_eq!(
            marks(&mut app),
            ["docs/", "main.go M", "main.go:4", "dep/", "notes.go", "pkg/ P", "shop.go S", "out.txt"],
            "folders and files inside excluded folders are bookmarked too"
        );
        let hint = zen_i18n::t!("ide-bookmark-hint");
        for name in ["notes.go", "docs", "dep", "out.txt"] {
            assert!(described(&mut app, name).contains(&hint), "{name} is marked in the tree");
        }
        let renamed = |app: &mut IdeApp, path: &Path, name: &str| {
            PathBuf::from(run(app, "file.fs.rename", json!({ "path": path, "name": name })).as_str().unwrap())
        };
        let memo = renamed(&mut app, &notes, "memo.go");
        let guide = renamed(&mut app, &docs, "guide");
        assert_eq!(
            marks(&mut app),
            ["guide/", "main.go M", "main.go:4", "memo.go", "dep/", "pkg/ P", "shop.go S", "out.txt"],
            "a file or folder renamed in the IDE keeps its bookmark"
        );
        assert!(described(&mut app, "memo.go").contains(&hint));
        assert!(described(&mut app, "guide").contains(&hint));
        select(&mut app, json!([memo, guide]));
        ask(&mut app, "run_action", json!({ "action": "view.project.contextMenu", "params": { "path": memo } }))
            .unwrap();
        menu(&mut app, "ide-tool-bookmarks");
        menu(&mut app, "ide-bookmark-toggle-mnemonic");
        assert_eq!(popups(&mut app), json!(["bookmarkMnemonic"]), "the menu's chooser with two rows selected");
        text(&mut app, "9");
        keys(&mut app, json!(["⏎"]));
        select(&mut app, json!([memo, guide]));
        keys(&mut app, json!(["⌥F3"]));
        text(&mut app, "8");
        keys(&mut app, json!(["⏎"]));
        assert_eq!(
            marks(&mut app),
            ["guide/ 8", "main.go M", "main.go:4", "memo.go 9", "dep/", "pkg/ P", "shop.go S", "out.txt"],
            "the menu gives the right-clicked row a mnemonic, ⌥F3 the last one selected"
        );
        select(&mut app, json!([main]));
        keys(&mut app, json!(["⌃8"]));
        let selection = ask(&mut app, "get_tree_selection", json!({})).unwrap();
        assert_eq!(selection["paths"], json!([guide]), "⌃8 selects the bookmarked folder in the tree");
        std::fs::remove_file(&memo).unwrap();
        std::fs::remove_dir(&guide).unwrap();
        std::fs::remove_dir_all(project.join("target")).unwrap();
        assert_eq!(
            marks(&mut app),
            ["main.go M", "main.go:4", "memo.go 9", "dep/", "pkg/ P", "shop.go S", "out.txt"],
            "a folder deleted by another program loses its bookmark, an open file keeps it while its tab is open"
        );
        let windows = ask(&mut app, "list_windows", json!({})).unwrap();
        let open: Vec<&str> = windows["windows"][0]["documents"]
            .as_array()
            .unwrap()
            .iter()
            .map(|document| document["path"].as_str().unwrap())
            .collect();
        assert_eq!(open, [&main, &memo, &built].map(|path| path.to_str().unwrap()));
        run(&mut app, "file.document.close", json!({ "index": 2 }));
        run(&mut app, "file.document.close", json!({ "index": 1 }));
        assert_eq!(
            marks(&mut app),
            ["main.go M", "main.go:4", "dep/", "pkg/ P", "shop.go S"],
            "and loses it with the tab"
        );
        assert!(run(&mut app, "navigate.bookmark.list", json!({}))["groups"].to_string().contains("dep"));
    }

    #[test]
    fn agents_drive_recent_files_recent_locations_and_file_structure() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let other = project.join("util.go");
        std::fs::write(&other, "package main\n\nvar answer = 42\n").unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": file }))).unwrap();
        ask(&ctx, &mut app, "dispatch_command", command("editor.edit.insert", json!({ "text": "// x\n" }))).unwrap();
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": other }))).unwrap();
        let popups = |answer: &Value| answer["window"]["popups"].clone();

        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.recentFiles.open" })).unwrap();
        assert_eq!(popups(&opened), json!(["recentFiles"]), "{opened}");
        assert_eq!(opened["result"].as_array().map(Vec::len), Some(2), "{opened}");
        let edited = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘E"] })).unwrap();
        assert_eq!(popups(&edited), json!(["recentFiles"]), "{edited}");
        let files = ask(
            &ctx,
            &mut app,
            "run_action",
            json!({ "action": "view.recentFiles.open", "params": { "editedOnly": true } }),
        )
        .unwrap();
        assert_eq!(files["result"][0]["relative"], "main.go", "{files}");
        assert_eq!(files["result"].as_array().map(Vec::len), Some(1), "{files}");

        let locations = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⌘E"] })).unwrap();
        assert_eq!(popups(&locations), json!(["recentLocations"]), "{locations}");
        let listed = ask(&ctx, &mut app, "run_action", json!({ "action": "view.recentLocations.open" })).unwrap();
        assert!(listed["result"].as_array().unwrap().iter().all(|place| place["edited"] == true), "{listed}");
        assert_eq!(listed["result"][0]["relative"], "main.go", "{listed}");
        let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(popups(&closed), json!([]), "{closed}");

        let structure = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘F12"] })).unwrap();
        assert_eq!(popups(&structure), json!(["fileStructure"]), "{structure}");
        let again = ask(&ctx, &mut app, "run_action", json!({ "action": "view.fileStructure.open" })).unwrap();
        assert!(again["result"]["path"].as_str().is_some_and(|path| path.ends_with("project/util.go")), "{again}");
        assert_eq!(popups(&again), json!(["fileStructure"]), "⌘F12 again keeps it open: {again}");
        let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(popups(&closed), json!([]), "{closed}");
    }

    #[test]
    fn the_switcher_goes_back_to_the_previous_file_and_shows_tool_windows() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let other = project.join("util.go");
        std::fs::write(&other, "package main\n\nvar answer = 42\n").unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": file }))).unwrap();
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": other }))).unwrap();
        let editor = |answer: &Value| answer["window"]["editor"].as_str().unwrap_or_default().to_owned();

        let tapped = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌃⇥"] })).unwrap();
        assert_eq!(tapped["window"]["popups"], json!([]), "a quick ⌃⇥ shows nothing: {tapped}");
        assert!(editor(&tapped).ends_with("main.go"), "{tapped}");

        let held = |events: Vec<egui::Event>, ctrl: bool| {
            let modifiers = if ctrl { egui::Modifiers::CTRL } else { egui::Modifiers::NONE };
            let mut all = vec![egui::Event::ModifiersChanged(modifiers)];
            all.extend(events);
            egui::RawInput { events: all, focused: true, ..Default::default() }
        };
        let tab = |shift: bool| egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers { ctrl: true, shift, ..Default::default() },
        };
        let run = |app: &mut IdeApp, input: egui::RawInput| {
            let mut output = ctx.run_ui(input, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
        };
        run(&mut app, held(vec![tab(false)], true));
        run(&mut app, held(vec![tab(false)], true));
        run(&mut app, held(vec![tab(true)], true));
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!(["switcher"]), "open while ⌃ is held: {state}");
        run(&mut app, held(Vec::new(), false));
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!([]), "{state}");
        assert!(state["editor"].as_str().unwrap().ends_with("util.go"), "releasing ⌃ opens the selection: {state}");

        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": file }))).unwrap();
        ask(&ctx, &mut app, "dispatch_command", command("editor.edit.insert", json!({ "text": "// unsaved\n" })))
            .unwrap();
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": other }))).unwrap();
        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.switcher.open" })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["switcher"]), "{opened}");
        assert_eq!(opened["result"].as_array().map(Vec::len), Some(2), "{opened}");
        let removed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌦"] })).unwrap();
        assert_eq!(removed["window"]["popups"], json!(["switcher"]), "{removed}");
        let saved = std::fs::read_to_string(&file).unwrap();
        assert!(saved.starts_with("// unsaved\npackage main"), "the unsaved edit is saved, not lost: {saved}");
        let files = ask(&ctx, &mut app, "dispatch_command", command("navigate.recent.files", json!({}))).unwrap();
        let listed: Vec<&str> = files.as_array().unwrap().iter().filter_map(|file| file["relative"].as_str()).collect();
        assert_eq!(listed, ["util.go"], "⌦ deletes main.go from the list: {files}");
        let split = ask(&ctx, &mut app, "dispatch_command", command("editor.split.list", json!({}))).unwrap();
        assert!(!split.to_string().contains("main.go"), "and closes its tab: {split}");

        for key in ["←", "↓"] {
            ask(&ctx, &mut app, "press_keys", json!({ "keys": [key] })).unwrap();
        }
        let chosen = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        assert_eq!(chosen["window"]["popups"], json!([]), "{chosen}");
        assert_eq!(chosen["window"]["left"], "project", "the second tool window, Project: {chosen}");

        let recent = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘E"] })).unwrap();
        assert_eq!(recent["window"]["popups"], json!(["recentFiles"]), "{recent}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["←"] })).unwrap();
        let tool = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        assert_eq!(tool["window"]["left"], "commit", "Recent Files has the same tool windows: {tool}");

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘E"] })).unwrap();
        let keys = json!({ "keys": ["←", "↓", "↓", "↓", "↓", "⏎"] });
        let run = ask(&ctx, &mut app, "press_keys", keys).unwrap();
        assert_eq!(run["window"]["bottom"], "run", "keys pressed in one frame count one by one: {run}");
    }

    #[test]
    fn the_switcher_follows_control_as_winit_reports_it() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let other = project.join("util.go");
        std::fs::write(&other, "package main\n").unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": file }))).unwrap();
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": other }))).unwrap();
        let control = |held: bool| {
            egui::Event::ModifiersChanged(if held { egui::Modifiers::CTRL } else { egui::Modifiers::NONE })
        };
        let tab = |pressed: bool, shift: bool| egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: Some(egui::Key::Tab),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers { ctrl: true, shift, ..Default::default() },
        };
        let shows = |shapes: &[egui::epaint::ClippedShape], wanted: &str| {
            fn has(shape: &egui::Shape, wanted: &str) -> bool {
                match shape {
                    egui::Shape::Text(text) => text.galley.text() == wanted,
                    egui::Shape::Vec(shapes) => shapes.iter().any(|shape| has(shape, wanted)),
                    _ => false,
                }
            }
            shapes.iter().any(|clipped| has(&clipped.shape, wanted))
        };
        let run = |app: &mut IdeApp, time: f64, events: Vec<egui::Event>| {
            let input = egui::RawInput { events, time: Some(time), focused: true, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
            shows(&output.shapes, "Switcher")
        };
        let state = |app: &mut IdeApp, time: f64| {
            run(app, time, Vec::new());
            let open = app.slot_for(None).unwrap().window.lock().unwrap().view.recent_files.is_open();
            (open, active_name(app))
        };

        assert!(!run(&mut app, 10.0, vec![control(true), tab(true, false), tab(false, false), control(false)]));
        assert_eq!(state(&mut app, 10.02), (false, "main.go".to_owned()), "a tap within one frame switches");

        assert!(!run(&mut app, 20.0, vec![control(true), tab(true, false)]));
        assert!(!run(&mut app, 20.08, vec![tab(false, false)]), "not shown before 150 ms");
        assert!(!run(&mut app, 20.12, vec![control(false)]));
        assert_eq!(state(&mut app, 20.14), (false, "util.go".to_owned()), "a tap over frames switches back");

        assert!(!run(&mut app, 30.0, vec![control(true), tab(true, false)]));
        run(&mut app, 30.2, vec![tab(false, false)]);
        assert!(run(&mut app, 30.22, Vec::new()), "shown while ⌃ is held");
        assert!(run(&mut app, 30.3, vec![tab(true, false), tab(false, false)]), "⇥ moves, still shown");
        assert!(run(&mut app, 30.4, vec![tab(true, true), tab(false, true)]), "⇧⇥ moves back");
        assert!(run(&mut app, 30.5, Vec::new()), "the key-up of ⇥ does not close it");
        assert!(!run(&mut app, 30.6, vec![control(false)]), "releasing ⌃ closes it");
        assert_eq!(state(&mut app, 30.62), (false, "main.go".to_owned()), "and opens the selection");

        assert!(!run(&mut app, 40.0, vec![control(true), tab(true, true)]));
        run(&mut app, 40.2, vec![tab(false, true)]);
        assert!(run(&mut app, 40.22, Vec::new()), "⌃⇧⇥ opens it too");
        assert!(!run(&mut app, 40.3, vec![egui::Event::WindowFocused(false)]), "losing the focus releases ⌃");
        assert!(!state(&mut app, 40.32).0);
    }

    #[test]
    fn select_in_shows_the_file_in_the_tree_the_structure_and_the_commit_window() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        let nested = project.join("pkg/deep/util.go");
        std::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        std::fs::write(&nested, "package deep\n\nfunc Answer() int { return 42 }\n").unwrap();
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": nested }))).unwrap();
        let action = |app: &mut IdeApp, params: Value| ask(&ctx, app, "run_action", params);
        let view = |app: &IdeApp, read: &dyn Fn(&IdeView) -> Value| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            read(&window.view)
        };

        let opened = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥F1"] })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["selectIn"]), "{opened}");
        let listed = action(&mut app, json!({ "action": "view.selectIn.open" })).unwrap();
        assert_eq!(listed["result"]["targets"], json!(["projectView", "structure", "finder", "terminal"]), "{listed}");
        let picked = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["1"] })).unwrap();
        assert_eq!(picked["window"]["popups"], json!([]), "{picked}");
        assert_eq!(picked["window"]["left"], "project", "{picked}");
        let lead = view(&app, &|view| json!(view.selection.lead()));
        assert_eq!(lead, json!(nested), "the file is selected");
        let unfolded = view(&app, &|view| json!(view.expanded.contains(&project.join("pkg/deep"))));
        assert_eq!(unfolded, json!(true), "its folders are unfolded");

        let structure =
            action(&mut app, json!({ "action": "view.selectIn.target", "params": { "target": "structure" } })).unwrap();
        assert_eq!(structure["window"]["left"], "structure", "{structure}");
        let finder = action(
            &mut app,
            json!({ "action": "view.selectIn.target", "params": { "target": "finder", "dryRun": true } }),
        )
        .unwrap();
        assert_eq!(finder["result"]["target"], "finder", "{finder}");
        let refused = action(&mut app, json!({ "action": "view.selectIn.target", "params": { "target": "commit" } }));
        assert!(refused.is_err_and(|message| message.contains("no changes")), "an unchanged file");

        std::fs::write(&nested, "package deep\n\nfunc Answer() int { return 43 }\n").unwrap();
        let mut targets = Value::Null;
        for _ in 0..200 {
            frame(&ctx, &mut app);
            targets = action(&mut app, json!({ "action": "view.selectIn.open" })).unwrap()["result"]["targets"].clone();
            if targets.as_array().is_some_and(|targets| targets.contains(&json!("commit"))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(targets, json!(["projectView", "structure", "commit", "finder", "terminal"]));
        let commit = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["3"] })).unwrap();
        assert_eq!(commit["window"]["popups"], json!([]), "{commit}");
        let selected = view(&app, &|view| json!(view.commit_window.selected()));
        assert_eq!(selected, json!(nested), "the change is selected");
    }

    #[test]
    fn jump_to_source_opens_the_change_from_the_commit_window_and_the_diff() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        let changed = project.join("notes.md");
        let other = project.join("other.md");
        let gone = project.join("gone.md");
        std::fs::write(&changed, "one\ntwo\nthree\nfour\nfive\n").unwrap();
        std::fs::write(&other, "other\n").unwrap();
        std::fs::write(&gone, "gone\n").unwrap();
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        std::fs::write(&changed, "one\nTWO\nnew\nthree\nfour\nfive\n").unwrap();
        std::fs::remove_file(&gone).unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        let open = |app: &mut IdeApp, path: &PathBuf| {
            ask(&ctx, app, "dispatch_command", command("file.document.open", json!({ "path": path }))).unwrap();
        };
        let action = |app: &mut IdeApp, params: Value| ask(&ctx, app, "run_action", params);
        let keys = |app: &mut IdeApp, keys: &[&str]| {
            ask(&ctx, app, "press_keys", json!({ "keys": keys })).unwrap();
            frame(&ctx, app);
        };
        let caret = |app: &IdeApp| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            let document = window.session.state::<Workbench>().unwrap().active_document().unwrap();
            let snapshot = document.snapshot();
            let line = snapshot.offset_to_point(document.selections().primary().head).unwrap().line;
            (document.path().to_owned(), line)
        };
        let with_view = |app: &IdeApp, change: &dyn Fn(&mut IdeView)| {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            change(&mut window.view);
        };

        open(&mut app, &changed);
        let mut selected = Err(String::new());
        for _ in 0..200 {
            frame(&ctx, &mut app);
            let params = json!({ "target": "commit" });
            selected = action(&mut app, json!({ "action": "view.selectIn.target", "params": params }));
            if selected.is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        selected.unwrap();
        frame(&ctx, &mut app);
        for key in ["F4", "⌘↓"] {
            open(&mut app, &other);
            frame(&ctx, &mut app);
            with_view(&app, &|view| view.commit_window.focus_tree = true);
            frame(&ctx, &mut app);
            keys(&mut app, &[key]);
            assert_eq!(caret(&app).0, changed, "{key} in the change list opens the selected file");
        }
        let jumped =
            action(&mut app, json!({ "action": "view.vcs.jumpToSource", "params": { "paths": [other] } })).unwrap();
        assert_eq!(jumped["result"]["opened"][0]["path"], json!(other), "{jumped}");
        with_view(&app, &|view| {
            view.commit_window.select(&gone);
        });
        let refused = action(&mut app, json!({ "action": "view.vcs.jumpToSource" }));
        assert!(refused.is_err_and(|message| message.contains("Jump to Source")), "a deleted file opens nothing");
        assert_eq!(caret(&app).0, other);

        open(&mut app, &changed);
        action(&mut app, json!({ "action": "view.vcs.showDiff" })).unwrap();
        frame(&ctx, &mut app);
        let editor = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["editor"].clone();
        assert_eq!(editor, "diff");
        with_view(&app, &|view| view.commit_window.focus_tree = false);
        frame(&ctx, &mut app);
        keys(&mut app, &["F4"]);
        assert_eq!(caret(&app), (changed.clone(), 1), "F4 in the diff goes to its first difference");

        for unified in [false, true] {
            action(&mut app, json!({ "action": "view.vcs.showDiff" })).unwrap();
            frame(&ctx, &mut app);
            action(&mut app, json!({ "action": "view.diff.viewer", "params": { "unified": unified } })).unwrap();
            let params = json!({ "side": "before", "line": 2 });
            action(&mut app, json!({ "action": "view.vcs.jumpToSource", "params": params })).unwrap();
            assert_eq!(caret(&app), (changed.clone(), 3), "HEAD's third line is the file's fourth, unified {unified}");
        }
    }

    #[test]
    fn select_in_finds_a_library_file_under_external_libraries() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        let root = data.path().canonicalize().unwrap().join("cache/example.com/lib@v1.0.0");
        let file = root.join("inner/lib.go");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "package inner\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let library = zen_workbench::model::Library {
            kind: zen_workbench::model::LibraryKind::Dependency,
            name: "example.com/lib".to_owned(),
            version: Some("v1.0.0".to_owned()),
            root: root.clone(),
            exists: true,
            group: None,
            label: None,
        };
        {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            window.view.libraries = Some(super::super::libraries_tree::Libraries::build(0, &[library], &[], &project));
        }
        let action = |app: &mut IdeApp, params: Value| ask(&ctx, app, "run_action", params).unwrap();
        let listed = action(&mut app, json!({ "action": "view.selectIn.open", "params": { "path": file } }));
        assert_eq!(listed["result"]["targets"][0], "projectView", "{listed}");
        let shown = action(
            &mut app,
            json!({ "action": "view.selectIn.target", "params": { "target": "projectView", "path": file } }),
        );
        assert_eq!(shown["window"]["left"], "project", "{shown}");
        assert_eq!(shown["window"]["popups"], json!([]), "going to a place closes the popup");
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        assert_eq!(window.view.selection.lead(), Some(&file));
        let groups = super::super::libraries_tree::group_path(&project, "libraries");
        assert!(window.view.expanded.contains(&groups), "External Libraries unfolds");
        assert!(window.view.expanded.contains(&root.join("inner")), "and the library's folders");
    }

    #[test]
    fn select_in_from_the_project_tree_shows_its_selected_file() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, main) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        let main = project.join(main.file_name().unwrap());
        let other = project.join("other.go");
        std::fs::write(&other, "package main\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        ask(&ctx, &mut app, "dispatch_command", command("file.document.open", json!({ "path": other }))).unwrap();
        {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            window.view.select_paths(&project, vec![main.clone()]).unwrap();
            window.view.tree_focused = true;
        }
        let opened = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥F1"] })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["selectIn"]), "{opened}");
        let picked = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["2"] })).unwrap();
        assert_eq!(picked["window"]["left"], "structure", "{picked}");
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        let active =
            window.session.state::<Workbench>().and_then(Workbench::active_document).map(|d| d.path().to_owned());
        assert_eq!(active, Some(main), "the tree's file, not the editor's, is shown in Structure");
    }

    #[test]
    fn copies_go_into_paste_from_history_and_it_pastes_them() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let open = json!({ "command": "file.document.open", "params": { "path": file } });
        ask(&ctx, &mut app, "dispatch_command", open).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let draw = |app: &mut IdeApp, events: Vec<egui::Event>| -> Vec<String> {
            let input = egui::RawInput { events, focused: true, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
            output
                .platform_output
                .commands
                .into_iter()
                .filter_map(|command| match command {
                    egui::OutputCommand::CopyText(text) => Some(text),
                    _ => None,
                })
                .collect()
        };
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        let select = command("editor.selection.set", json!({ "selections": [{ "anchor": 0, "head": 7 }] }));
        ask(&ctx, &mut app, "dispatch_command", select).unwrap();
        let copied = draw(&mut app, vec![egui::Event::Copy]);
        assert_eq!(copied, ["package"], "Copy goes through the window's output, never the system clipboard");
        let add = command("editor.clipboardHistory.add", json!({ "text": "second\nline" }));
        assert_eq!(ask(&ctx, &mut app, "dispatch_command", add).unwrap(), json!(true));
        let list = ask(&ctx, &mut app, "dispatch_command", command("editor.clipboardHistory.list", json!({}))).unwrap();
        assert_eq!(list[0], json!({ "index": 0, "preview": "second", "lines": 2, "chars": 11 }), "{list}");
        assert_eq!(list[1]["preview"], "package", "{list}");

        let caret = command("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
        ask(&ctx, &mut app, "dispatch_command", caret).unwrap();
        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.pasteFromHistory.open" })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["pasteFromHistory"]), "{opened}");
        assert_eq!(opened["result"][0]["preview"], "second", "the action answers with the previews: {opened}");
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌦"] })).unwrap();
        let list = ask(&ctx, &mut app, "dispatch_command", command("editor.clipboardHistory.list", json!({}))).unwrap();
        assert_eq!(list.as_array().map(Vec::len), Some(1), "Delete removed the newest: {list}");
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        let copied = draw(&mut app, vec![enter]);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!([]), "{state}");
        let slot = app.slot_for(None).unwrap();
        let text = slot
            .window
            .lock()
            .unwrap()
            .session
            .state::<Workbench>()
            .unwrap()
            .active_document()
            .unwrap()
            .text()
            .to_owned();
        assert!(text.starts_with("packagepackage main"), "{text}");
        assert_eq!(copied, ["package"], "the pasted entry becomes the clipboard's");
        let shortcut = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⌘V"] })).unwrap();
        assert_eq!(shortcut["window"]["popups"], json!(["pasteFromHistory"]), "{shortcut}");
        let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(closed["window"]["popups"], json!([]), "{closed}");
    }

    #[test]
    fn always_select_opened_file_is_kept_per_project_and_hides_select_opened_file() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        std::fs::create_dir(project.join("pkg")).unwrap();
        let nested = project.join("pkg").join("util.go");
        std::fs::write(&nested, "package pkg\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let action = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        let shown = |app: &IdeApp| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            window.view.header_action_shown("locate")
        };
        let selection = |app: &mut IdeApp| ask(&ctx, app, "get_tree_selection", json!({})).unwrap()["paths"].clone();

        let refused = action(&mut app, "view.project.selectOpened", json!({})).unwrap_err();
        assert!(refused.contains("file open in the editor"), "{refused}");
        let open = json!({ "command": "file.document.open", "params": { "path": nested } });
        ask(&ctx, &mut app, "dispatch_command", open).unwrap();
        frame(&ctx, &mut app);
        let selected = action(&mut app, "view.project.selectOpened", json!({})).unwrap();
        assert_eq!(selected["result"]["selected"], json!(nested), "{selected}");
        frame(&ctx, &mut app);
        assert_eq!(selection(&mut app), json!([nested]));
        assert!(shown(&app), "Select Opened File is in the header while Always Select Opened File is off");

        let toggled = action(&mut app, "view.project.alwaysSelectOpened", json!({})).unwrap();
        assert_eq!(toggled["result"]["alwaysSelectOpened"], true, "{toggled}");
        assert_eq!(toggled["window"]["projectView"]["alwaysSelectOpened"], true, "{toggled}");
        assert!(!shown(&app), "Select Opened File hides while Always Select Opened File is on");
        let main = project.join("main.go");
        let open = json!({ "command": "file.document.open", "params": { "path": main } });
        ask(&ctx, &mut app, "dispatch_command", open).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(selection(&mut app), json!([main]), "the tree follows the editor's tab");

        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(saved.contains("\"alwaysSelectOpened\": true"), "{saved}");
        drop(app);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["projectView"]["alwaysSelectOpened"], true, "restored with the project: {state}");
        assert!(!shown(&app));

        let off = action(&mut app, "view.project.alwaysSelectOpened", json!({ "on": false })).unwrap();
        assert_eq!(off["result"]["alwaysSelectOpened"], false, "{off}");
        assert!(shown(&app));
        let wrong = action(&mut app, "view.project.alwaysSelectOpened", json!({ "on": "yes" })).unwrap_err();
        assert!(wrong.contains("`on`"), "{wrong}");
        app.remember_workspace(&ctx, true);
        drop(app);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["projectView"]["alwaysSelectOpened"], false, "{state}");
    }

    #[test]
    fn the_project_tree_copies_cuts_and_pastes_files_without_the_system_clipboard() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        let file = project.join(file.file_name().unwrap());
        std::fs::create_dir(project.join("pkg")).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut app =
            IdeApp::new(test_product(), registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let clipboard = |app: &IdeApp| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            let state = window.session.state::<zen_workbench::file_clipboard::FileClipboard>();
            state.map(|state| state.copied().0.to_vec()).unwrap_or_default()
        };
        let select = |app: &mut IdeApp, paths: Value| {
            ask(&ctx, app, "run_action", json!({ "action": "view.project.select", "params": { "paths": paths } }))
                .unwrap()
        };
        let keys = |app: &mut IdeApp, key: &str| ask(&ctx, app, "press_keys", json!({ "keys": [key] })).unwrap();

        select(&mut app, json!([file]));
        keys(&mut app, "⌘C");
        assert_eq!(clipboard(&app), vec![file.clone()]);
        keys(&mut app, "⌘V");
        frame(&ctx, &mut app);
        assert!(project.join("main copy.go").is_file(), "pasted as main copy.go");
        assert!(file.is_file());

        select(&mut app, json!([project.join("main copy.go")]));
        keys(&mut app, "⌘X");
        select(&mut app, json!([project.join("pkg")]));
        keys(&mut app, "⌘V");
        frame(&ctx, &mut app);
        assert!(project.join("pkg/main copy.go").is_file(), "moved into pkg");
        assert!(!project.join("main copy.go").exists());

        let menu = ask(
            &ctx,
            &mut app,
            "run_action",
            json!({ "action": "view.project.contextMenu", "params": { "path": file } }),
        )
        .unwrap();
        frame(&ctx, &mut app);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert!(state.to_string().contains("projectMenu"), "{menu} {state}");
        let rename =
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.project.rename", "params": { "path": file } }))
                .unwrap();
        assert!(rename["window"]["popups"].as_array().unwrap().contains(&json!("newEntry")), "{rename}");
    }

    #[test]
    fn a_folder_marked_excluded_leaves_every_search_and_the_tree_keeps_excluded_files_per_project() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("shop");
        for (name, text) in [
            ("go.mod", "module shop\n\ngo 1.22\n\nrequire example.com/m v1.0.0\n"),
            ("main.go", "package main // needle\n"),
            ("gen/needle_gen.go", "package gen // needle\n"),
            ("vendor/modules.txt", "# example.com/m v1.0.0\n## explicit\nexample.com/m\n"),
            ("vendor/example.com/m/needle_vendor.go", "package m // needle\n"),
        ] {
            let path = project.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let project = project.canonicalize().unwrap();
        let gen_folder = project.join("gen");
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        let action = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params })).unwrap()
        };
        let searched = |app: &mut IdeApp| -> (Vec<String>, Vec<String>, Vec<String>) {
            let status = |app: &mut IdeApp| run(app, "search.index.status", json!({}));
            let mut waited = 0;
            while !(status(app)["namesReady"] == true && status(app)["ready"] == true) && waited < 500 {
                std::thread::sleep(Duration::from_millis(10));
                waited += 1;
            }
            let strings = |value: &Value, key: &str| -> Vec<String> {
                let mut found: Vec<String> =
                    value.as_array().unwrap().iter().map(|item| item[key].as_str().unwrap().to_owned()).collect();
                found.sort();
                found
            };
            let files = run(app, "navigate.file.goTo", json!({ "query": "needle" }));
            let everywhere =
                run(app, "search.everywhere.query", json!({ "tab": "files", "query": "needle", "waitMs": 2000 }));
            let items: Vec<Value> = everywhere["groups"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|group| group["items"].as_array().unwrap().clone())
                .collect();
            let text = run(app, "search.text.find", json!({ "query": { "text": "needle" } }));
            (strings(&files["files"], "relative"), strings(&json!(items), "name"), strings(&text["matches"], "path"))
        };
        let rows = |app: &IdeApp| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            window.view.visible_rows.clone()
        };

        let (files, everywhere, text) = searched(&mut app);
        assert_eq!(files, ["gen/needle_gen.go", "vendor/example.com/m/needle_vendor.go"], "vendor is searched");
        assert_eq!(everywhere, ["needle_gen.go", "needle_vendor.go"]);
        assert_eq!(text, ["gen/needle_gen.go", "main.go", "vendor/example.com/m/needle_vendor.go"]);
        assert!(!project.join(".idea").exists());

        let marked = run(&mut app, "project.directory.markExcluded", json!({ "path": gen_folder }));
        assert_eq!(marked["changed"], true, "{marked}");
        assert_eq!(marked["excluded"], json!([gen_folder]));
        assert!(project.join(".idea/modules.xml").is_file() && project.join(".idea/shop.iml").is_file());
        let reread = zen_project::settings::SettingsProvider::load(
            &zen_jb_import::IdeaProject::with_user_home(None),
            &project,
            &zen_workbench::project_settings::DiskFiles::new(&project),
        )
        .unwrap();
        assert_eq!(reread.excluded, std::slice::from_ref(&gen_folder), "the .idea reader sees the same exclusion");
        let (files, everywhere, text) = searched(&mut app);
        assert_eq!(files, ["vendor/example.com/m/needle_vendor.go"]);
        assert_eq!(everywhere, ["needle_vendor.go"]);
        assert_eq!(text, ["main.go", "vendor/example.com/m/needle_vendor.go"]);

        action(&mut app, "view.project.contextMenu", json!({ "path": gen_folder }));
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            let menu = window.view.tree_menu.clone().unwrap();
            assert_eq!(menu.folder, super::super::FolderMark::Excluded, "the menu offers Cancel Exclusion");
        }
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        assert!(rows(&app).contains(&gen_folder), "Excluded Files is on by default");
        assert_eq!(
            action(&mut app, "view.project.excludedFiles", json!({ "show": false }))["result"]["shown"],
            json!(false)
        );
        frame(&ctx, &mut app);
        assert!(!rows(&app).contains(&gen_folder), "{:?}", rows(&app));
        assert!(rows(&app).contains(&project.join("vendor")));
        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(saved.contains("\"hideExcludedFiles\": true"), "{saved}");
        drop(app);

        let mut app = themed_app(data.path(), vec![project.clone()]);
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert!(!rows(&app).contains(&gen_folder), "the choice is kept for the project");
        let cancelled = run(&mut app, "project.directory.cancelExclusion", json!({ "path": "gen" }));
        assert_eq!(cancelled["excluded"], json!([]), "{cancelled}");
        frame(&ctx, &mut app);
        assert!(rows(&app).contains(&gen_folder), "an included folder shows again");
        let (files, everywhere, text) = searched(&mut app);
        assert_eq!(files, ["gen/needle_gen.go", "vendor/example.com/m/needle_vendor.go"]);
        assert_eq!(everywhere, ["needle_gen.go", "needle_vendor.go"]);
        assert_eq!(text, ["gen/needle_gen.go", "main.go", "vendor/example.com/m/needle_vendor.go"]);
        assert_eq!(action(&mut app, "view.project.excludedFiles", json!({}))["result"]["shown"], json!(true));
        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(!saved.contains("hideExcludedFiles"), "{saved}");
    }

    #[test]
    fn the_git_workspace_branch_menu_opens_its_dialogs_and_renames_from_the_keyboard() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        test_git(&project, &["branch", "feature"]);
        test_git(&project, &["tag", "v1"]);
        let nowhere = data.path().join("no-remote");
        test_git(&project, &["remote", "add", "origin", nowhere.to_str().unwrap()]);
        test_git(&project, &["update-ref", "refs/remotes/origin/feature", "HEAD"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        frame(&ctx, &mut app);

        run(&mut app, "vcs.workspace.refMenu", json!({ "name": "feature" })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["refMenu"]));
        for (name, kind, action, dialog) in [
            ("feature", "local", "rename", "renameBranch"),
            ("feature", "local", "setUpstream", "setUpstream"),
            ("origin/feature", "remote", "delete", "deleteRemoteBranch"),
            ("v1", "tag", "checkout", "checkoutCommit"),
        ] {
            let params = json!({ "name": name, "kind": kind, "action": action });
            let done = run(&mut app, "vcs.workspace.refAction", params).unwrap();
            assert_eq!(done["dialog"], dialog);
            frame(&ctx, &mut app);
            assert_eq!(state(&mut app)["popups"], json!([dialog]), "{action}");
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            frame(&ctx, &mut app);
            assert_eq!(state(&mut app)["popups"], json!([]), "{action} closes with Escape");
        }
        assert!(test_git(&project, &["branch", "--list", "feature"]).contains("feature"), "nothing ran");

        run(&mut app, "vcs.workspace.refAction", json!({ "name": "feature", "action": "rename" })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "text": "topic" })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert_eq!(test_git(&project, &["branch", "--list", "topic", "feature"]).trim(), "topic");
    }

    #[test]
    fn a_language_servers_links_go_once_used_and_a_failed_install_is_told() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let dispatch = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let slot = app.slot_for(None).unwrap();
        let notice = |command: &str, id: &str| {
            use zen_workbench::notifications::{Action, Group, Kind, NewNotification, post};
            let mut window = slot.window.lock().unwrap();
            let link = Action::new("link", "Link".to_owned(), command, json!({ "id": id })).once();
            post(
                &mut window.session,
                NewNotification::message(Group::LanguageServers, Kind::Warning, "Missing").action(link),
            )
        };

        let id = notice("lsp.server.install", "no-such-server");
        let failed = dispatch(&mut app, "ui.notifications.action", json!({ "id": id, "action": "link" }));
        assert!(failed.is_err(), "{failed:?}");
        let list = dispatch(&mut app, "ui.notifications.list", json!({})).unwrap();
        let used = list["notifications"].as_array().unwrap().iter().find(|item| item["id"] == id).unwrap();
        assert_eq!(used["actions"], json!([]), "{used}");
        let installing = zen_i18n::t!("ide-lsp-installing", name = "no-such-server");
        assert!(
            list["notifications"].as_array().unwrap().iter().all(|item| item["body"] != installing.as_str()),
            "{list}"
        );
    }

    #[test]
    fn agents_read_act_on_and_clear_the_notifications() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let dispatch = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let make = json!({ "id": "make", "label": "Make", "command": "file.fs.newDirectory",
                           "params": { "directory": project, "name": "made" } });
        let lure = json!({ "type": "warning", "title": "Folder missing", "body": "Make it?", "actions": [make] });
        assert!(dispatch(&mut app, "ui.notifications.post", lure).is_err(), "agents post no actions");
        let slot = app.slot_for(None).unwrap();
        let id = {
            use zen_workbench::notifications::{Action, Group, Kind, NewNotification, post};
            let mut window = slot.window.lock().unwrap();
            let root = window.session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
            let make = Action::new(
                "make",
                "Make".to_owned(),
                "file.fs.newDirectory",
                json!({ "directory": root, "name": "made" }),
            );
            let new = NewNotification::message(Group::Git, Kind::Warning, "Make it?").titled("Folder missing");
            post(&mut window.session, new.action(make))
        };

        let missing = data.path().join("nowhere.go");
        let failed =
            ask(&ctx, &mut app, "run_action", json!({ "action": "file.document.open", "params": { "path": missing } }));
        assert!(failed.is_err());
        let list = dispatch(&mut app, "ui.notifications.list", json!({})).unwrap();
        assert_eq!(list["unread"], 2, "{list}");
        let newest = &list["notifications"][0];
        assert_eq!((&newest["type"], &newest["group"]), (&json!("error"), &json!("errors")), "{newest}");
        assert_eq!(list["notifications"][1]["actions"][0]["id"], "make");

        let ran = dispatch(&mut app, "ui.notifications.action", json!({ "id": id, "action": "make" })).unwrap();
        assert_eq!(ran["result"]["command"], "file.fs.newDirectory", "{ran}");
        assert!(project.join("made").is_dir(), "{ran}");
        let unread = dispatch(&mut app, "ui.notifications.list", json!({ "unreadOnly": true })).unwrap();
        assert_eq!(unread["unread"], 1, "acting on it read it: {unread}");
        let wrong = dispatch(&mut app, "ui.notifications.action", json!({ "id": id, "action": "nothing" }));
        assert!(wrong.is_err());
        let told = dispatch(&mut app, "ui.notifications.list", json!({ "unreadOnly": true })).unwrap();
        assert_eq!(told["unread"], 2, "the window tells of the failure too: {told}");

        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.toolWindow.notifications" })).unwrap();
        assert_eq!(opened["window"]["right"], "notifications", "{opened}");
        frame(&ctx, &mut app);
        let seen = dispatch(&mut app, "ui.notifications.list", json!({})).unwrap();
        assert_eq!(seen["unread"], 0, "{seen}");
        let cleared = dispatch(&mut app, "ui.notifications.clear", json!({})).unwrap();
        assert_eq!(cleared["count"], 3);
        let closed = ask(&ctx, &mut app, "run_action", json!({ "action": "view.toolWindow.notifications" })).unwrap();
        assert_eq!(closed["window"]["right"], Value::Null);
    }

    #[test]
    fn what_a_git_view_posts_is_git_but_other_messages_and_errors_keep_their_group() {
        use zen_workbench::notifications::{Group, Kind, Notifications};
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let slot = app.slot_for(None).unwrap();
        let mut window = slot.window.lock().unwrap();
        let ProjectWindow { session, view, .. } = &mut *window;
        let failed = |id: &str| zen_core::DispatchError::Failed {
            id: zen_core::CommandId::new(id).unwrap(),
            error: zen_core::CommandError::new("broken"),
        };
        view.as_git(|view| {
            view.post_at(&ctx, "pushed".to_owned());
            view.with_group(Group::of_command("view.file.open"), |view| view.post_at(&ctx, "copied".to_owned()));
            view.report_at(&ctx, failed("file.document.open"));
            view.report_at(&ctx, failed("vcs.remote.push"));
        });
        view.post_at(&ctx, "plain".to_owned());
        view.notifications.flush(session);
        let store = session.state::<Notifications>().unwrap();
        let logged: Vec<(Group, Kind, &str)> =
            store.newest_first().map(|item| (item.group, item.kind, item.body.as_str())).collect();
        assert_eq!(
            logged[..5],
            [
                (Group::General, Kind::Info, "plain"),
                (Group::Git, Kind::Error, "broken"),
                (Group::Errors, Kind::Error, "broken"),
                (Group::General, Kind::Info, "copied"),
                (Group::Git, Kind::Info, "pushed"),
            ],
            "{logged:?}"
        );
    }

    #[test]
    fn a_theme_that_is_gone_is_a_warning_in_the_notifications() {
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), r#"{"theme": "gone"}"#).unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let list = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "ui.notifications.list" })).unwrap();
        let found = list["notifications"].as_array().unwrap().iter().find(|item| item["type"] == "warning").cloned();
        let found = found.unwrap_or_else(|| panic!("{list}"));
        assert!(found["body"].as_str().is_some_and(|body| body.contains("gone")), "{found}");
        assert_eq!(found["group"], "general");
    }

    #[test]
    fn the_git_workspace_stash_menu_renames_and_drops_after_asking() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        std::fs::write(&file, "package main\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        run(&mut app, "vcs.stash.create", json!({ "message": "Halfway" })).unwrap();
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        frame(&ctx, &mut app);

        run(&mut app, "vcs.workspace.stashMenu", json!({ "index": 0 })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["stashMenu"]));
        let done = run(&mut app, "vcs.workspace.stashAction", json!({ "action": "drop" })).unwrap();
        assert_eq!(done["dialog"], "dropStash");
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["dropStash"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert_eq!(test_git(&project, &["stash", "list"]).lines().count(), 1, "nothing dropped");

        run(&mut app, "vcs.workspace.stashAction", json!({ "index": 0, "action": "rename" })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["renameStash"]));
        ask(&ctx, &mut app, "press_keys", json!({ "text": "Later" })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert_eq!(test_git(&project, &["stash", "list", "--format=%gs"]).trim(), "On main: Later");

        run(&mut app, "vcs.workspace.stashAction", json!({ "action": "drop" })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert_eq!(test_git(&project, &["stash", "list"]).trim(), "", "dropped");
    }

    #[test]
    fn changelists_and_the_shelf_are_reached_from_their_dialogs() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        std::fs::write(&file, "package main\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let action = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        run(&mut app, "vcs.status.refresh", json!({ "wait": true })).unwrap();

        action(&mut app, "view.vcs.newChangelist", json!({ "paths": [file] })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["newChangelist"]));
        ask(&ctx, &mut app, "press_keys", json!({ "text": "Feature" })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        let lists = run(&mut app, "vcs.changelist.list", json!({})).unwrap();
        assert_eq!(lists["lists"][1]["name"], "Feature");
        assert_eq!(lists["lists"][1]["files"][0]["relative"], "main.go");

        action(&mut app, "view.vcs.shelve", json!({ "paths": [file] })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["shelve"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert!(std::fs::read_to_string(&file).unwrap().contains("println"), "rolled back");
        let shelf = run(&mut app, "vcs.shelf.list", json!({})).unwrap();
        assert_eq!(shelf["shelves"][0]["name"], "Feature");

        action(&mut app, "view.vcs.unshelve", json!({})).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["unshelve"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        run(&mut app, "vcs.shelf.unshelve", json!({ "id": 1 })).unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package main\n");
        run(&mut app, "vcs.shelf.delete", json!({ "id": 1 })).unwrap();
        assert!(action(&mut app, "view.vcs.unshelve", json!({})).is_err());
        action(&mut app, "view.vcs.showShelf", json!({})).unwrap();
        frame(&ctx, &mut app);
    }

    fn patch_window(window: &mut ProjectWindow) -> (&mut Session, &mut IdeView) {
        (&mut window.session, &mut window.view)
    }

    #[test]
    fn patches_are_made_and_applied_through_their_dialogs() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        let original = std::fs::read_to_string(&file).unwrap();
        std::fs::write(&file, "package main\n\nfunc main() {\n\tprintln(2)\n}\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let action = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        let window = |app: &IdeApp| app.shared.state().windows[0].window.clone();
        run(&mut app, "vcs.status.refresh", json!({ "wait": true })).unwrap();

        action(&mut app, "view.vcs.createPatch", json!({ "paths": [file] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["createPatch"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        let written = project.join("Changes.patch");
        let text = std::fs::read_to_string(&written).unwrap();
        assert!(text.contains("+\tprintln(2)"), "{text}");
        test_git(&project, &["apply", "--check", "-R", written.to_str().unwrap()]);
        std::fs::remove_file(&written).unwrap();

        let copied = action(&mut app, "view.vcs.copyPatch", json!({ "paths": [file] })).unwrap();
        assert_eq!(copied["result"]["clipboard"], true, "{copied}");
        let on_clipboard = {
            let slot = window(&app);
            let mut window = slot.lock().unwrap();
            let (session, _) = patch_window(&mut window);
            zen_workbench::file_clipboard::clipboard_text(session)
        };
        assert!(on_clipboard.contains("+\tprintln(2)"), "{on_clipboard}");

        std::fs::write(&file, &original).unwrap();
        run(&mut app, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        action(&mut app, "view.vcs.applyPatchFromClipboard", json!({})).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["applyPatch"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert!(std::fs::read_to_string(&file).unwrap().contains("println(2)"), "applied");

        let dropped = data.path().join("dropped.patch");
        std::fs::write(&dropped, &on_clipboard).unwrap();
        #[derive(Debug)]
        struct Dropped(PathBuf);
        impl egui::DroppedFile for Dropped {
            fn path(&self) -> &Path {
                &self.0
            }
            fn bytes(&self) -> Result<Vec<u8>, String> {
                std::fs::read(&self.0).map_err(|error| error.to_string())
            }
        }
        let input = egui::RawInput { dropped_files: vec![Arc::new(Dropped(dropped.clone()))], ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["applyPatch"]));
        assert!(!project.join("dropped.patch").exists(), "not pasted into the project");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert!(action(&mut app, "view.vcs.applyPatch", json!({})).is_err(), "agents give the file");
        action(&mut app, "view.vcs.applyPatch", json!({ "path": dropped })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["applyPatch"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);

        test_git(&project, &["commit", "-q", "-am", "Second"]);
        let opened =
            run(&mut app, "vcs.workspace.commitAction", json!({ "revision": "HEAD", "action": "createPatch" }))
                .unwrap();
        assert_eq!(opened["dialog"], "createPatch");
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["createPatch"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        let commit_patch = std::fs::read_to_string(project.join("Second.patch")).unwrap();
        assert!(commit_patch.contains("Subject: [PATCH] Second\n"), "{commit_patch}");

        std::fs::write(&file, "package main\n").unwrap();
        run(&mut app, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        run(&mut app, "vcs.shelf.create", json!({ "paths": [file], "name": "Shelved" })).unwrap();
        action(&mut app, "view.vcs.createPatch", json!({ "shelf": 1 })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["createPatch"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);

        let mail = "From 1111111111111111111111111111111111111111 Mon Sep 17 00:00:00 2001\nFrom: Grace <grace@example.com>\nSubject: [PATCH] Shout\n\nWhy.\n---\ndiff --git a/main.go b/main.go\n--- a/main.go\n+++ b/main.go\n@@ -1,3 +1,3 @@\n package main\n \n-func main() {\n+func main() { // shout\n";
        run(&mut app, "vcs.patch.apply", json!({ "text": mail })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let slot = window(&app);
        let window = slot.lock().unwrap();
        assert_eq!(window.view.commit_window.message, "Shout\n\nWhy.");
        assert_eq!(window.view.commit_window.author.as_deref(), Some("Grace <grace@example.com>"));
    }

    #[test]
    fn the_patch_dialogs_are_drawn_in_both_languages() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        std::fs::write(
            project.join("util.go"),
            "package main\n\nfunc one() int { return 1 }\n\nfunc two() int { return 2 }\n",
        )
        .unwrap();
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        std::fs::write(&file, "package main\n\nfunc main() {\n\tprintln(1, 2)\n}\n").unwrap();
        std::fs::write(
            project.join("util.go"),
            "package main\n\nfunc one() int { return 10 }\n\nfunc two() int { return 2 }\n",
        )
        .unwrap();
        let patch = data.path().join("change.patch");
        std::fs::write(
            &patch,
            "From: Grace <grace@example.com>\nSubject: [PATCH] Count up\n\n---\ndiff --git a/main.go b/main.go\nold mode 100644\nnew mode 100755\n--- a/main.go\n+++ b/main.go\n@@ -1,5 +1,5 @@\n package main\n \n func main() {\n-\tprintln(1)\n+\tprintln(3)\n }\ndiff --git a/util.go b/util.go\n--- a/util.go\n+++ b/util.go\n@@ -3 +3 @@\n-func one() int { return 1 }\n+func one() int { return 100 }\n",
        )
        .unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1200.0, 860.0), 1.5);
        let ctx = screen.context().clone();
        screen.step(Vec::new(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let action = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        run(&mut app, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let read = run(&mut app, "vcs.patch.read", json!({ "path": patch })).unwrap();
        assert_eq!(read["files"][0]["state"], "conflict", "{read}");
        assert_eq!(read["files"][1]["state"], "conflict", "{read}");
        assert_eq!(read["files"][0]["executable"], true, "{read}");
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for &language in languages {
            if snapshots.is_some() {
                zen_i18n::set_language(Some(language));
            }
            for (name, open, params) in [
                ("create", "view.vcs.createPatch", json!({ "paths": [file, project.join("util.go")] })),
                ("apply", "view.vcs.applyPatch", json!({ "path": patch })),
            ] {
                action(&mut app, open, params).unwrap();
                frame(&ctx, &mut app);
                frame(&ctx, &mut app);
                let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
                assert_eq!(state["popups"], json!([format!("{name}Patch")]), "{state}");
                if let Some(dir) = &snapshots {
                    let image = screen.render(|ui| {
                        app.logic(ui.ctx());
                        app.ui(ui);
                    });
                    crate::offscreen::write_png(&dir.join(format!("patch-{name}-{language}.png")), &image).unwrap();
                }
                ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
                frame(&ctx, &mut app);
                assert_eq!(ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"], json!([]));
            }
        }
        if snapshots.is_some() {
            zen_i18n::set_language(Some("en"));
        }
    }

    #[test]
    fn the_git_workspace_drop_menu_merges_into_another_branch_after_asking() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        test_git(&project, &["branch", "feature"]);
        std::fs::write(project.join("b.txt"), "b\n").unwrap();
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "Second"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        frame(&ctx, &mut app);

        let drop = json!({ "source": { "name": "main" }, "target": { "name": "feature" } });
        let menu = run(&mut app, "vcs.workspace.dropMenu", drop.clone()).unwrap();
        let actions: Vec<&Value> = menu["dropMenu"]["items"].as_array().unwrap().iter().map(|i| &i["action"]).collect();
        assert_eq!(actions, ["fastForward", "merge", "rebase", "interactiveRebase"]);
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["dropMenu"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));

        let mut params = drop.clone();
        params["action"] = json!("merge");
        let done = run(&mut app, "vcs.workspace.dropAction", params.clone()).unwrap();
        assert_eq!(done["dialog"], "checkoutAndMerge");
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["checkoutAndMerge"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert_eq!(test_git(&project, &["branch", "--show-current"]).trim(), "main", "nothing ran");
        run(&mut app, "vcs.workspace.dropAction", params).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert_eq!(test_git(&project, &["branch", "--show-current"]).trim(), "feature");
        assert_eq!(test_git(&project, &["rev-parse", "feature"]), test_git(&project, &["rev-parse", "main"]));
    }

    #[test]
    fn the_interactive_rebase_editor_follows_its_keys_and_closes_into_the_git_tab() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        for name in ["b", "c"] {
            std::fs::write(project.join(format!("{name}.txt")), name).unwrap();
            test_git(&project, &["add", "-A"]);
            test_git(&project, &["commit", "-q", "-m", name]);
        }
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        frame(&ctx, &mut app);
        run(&mut app, "vcs.rebase.plan", json!({ "from": "HEAD~2" })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["editor"], "rebaseEditor");
        for keys in [["S"], ["↓"], ["⌥↑"]] {
            ask(&ctx, &mut app, "press_keys", json!({ "keys": keys })).unwrap();
            frame(&ctx, &mut app);
        }
        let plan = run(&mut app, "vcs.rebase.state", json!({})).unwrap()["plan"].clone();
        let steps: Vec<(String, String)> = plan["steps"]
            .as_array()
            .unwrap()
            .iter()
            .map(|step| (step["subject"].as_str().unwrap().to_owned(), step["action"].as_str().unwrap().to_owned()))
            .collect();
        assert_eq!(steps, [("c".to_owned(), "squash".to_owned()), ("b".to_owned(), "pick".to_owned())]);
        let head = test_git(&project, &["rev-parse", "HEAD"]);
        run(&mut app, "vcs.rebase.cancel", json!({})).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["editor"], "gitWorkspace");
        assert_eq!(test_git(&project, &["rev-parse", "HEAD"]), head);
    }

    #[test]
    fn conflicts_open_from_the_dialog_in_the_conflict_editor_which_closes_into_the_git_tab() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        std::fs::write(project.join("a.txt"), "one\ntwo\n").unwrap();
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        test_git(&project, &["switch", "-q", "-c", "feature"]);
        std::fs::write(project.join("a.txt"), "one\nfeature\n").unwrap();
        test_git(&project, &["commit", "-q", "-am", "Feature"]);
        test_git(&project, &["switch", "-q", "main"]);
        std::fs::write(project.join("a.txt"), "one\nmain\n").unwrap();
        test_git(&project, &["commit", "-q", "-am", "Main"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        frame(&ctx, &mut app);
        assert!(run(&mut app, "vcs.branch.merge", json!({ "name": "feature" })).is_err());
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["gitConflicts"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let window = state(&mut app);
        assert_eq!((window["popups"].clone(), window["editor"].clone()), (json!([]), json!("conflictEditor")));
        run(&mut app, "vcs.conflict.close", json!({})).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["editor"], "gitWorkspace");
        assert!(std::fs::read_to_string(project.join("a.txt")).unwrap().contains("<<<<<<<"));
        run(&mut app, "vcs.conflict.open", json!({})).unwrap();
        run(&mut app, "vcs.conflict.resolve", json!({ "conflicts": true, "take": "both", "markResolved": true }))
            .unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["editor"], "gitWorkspace", "marked resolved: the editor closes");
        run(&mut app, "vcs.operation.continue", json!({})).unwrap();
        assert_eq!(std::fs::read_to_string(project.join("a.txt")).unwrap(), "one\nmain\nfeature\n");
        assert_eq!(test_git(&project, &["log", "-1", "--format=%s"]).trim(), "Merge branch 'feature'");
    }

    #[test]
    fn the_git_workspace_searches_with_command_f_and_remembers_hide_and_solo() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        test_git(&project, &["switch", "-q", "-c", "topic"]);
        test_git(&project, &["commit", "-q", "--allow-empty", "-m", "Fix the parser"]);
        test_git(&project, &["switch", "-q", "main"]);
        test_git(&project, &["commit", "-q", "--allow-empty", "-m", "Fix the lexer"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let search = |app: &mut IdeApp| run(app, "vcs.workspace.state", json!({})).unwrap()["search"].clone();
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        run(&mut app, "vcs.graph.page", json!({ "wait": true })).unwrap();
        frame(&ctx, &mut app);

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘F"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(search(&mut app)["open"], json!(true));
        ask(&ctx, &mut app, "press_keys", json!({ "text": "fix" })).unwrap();
        frame(&ctx, &mut app);
        let found = search(&mut app);
        assert_eq!((found["text"].clone(), found["count"].clone()), (json!("fix"), json!(2)), "{found}");
        let first = found["current"].as_u64().unwrap();
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["↓"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(search(&mut app)["current"], json!((first + 1) % 2));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(search(&mut app)["current"], json!(first));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["↑"] })).unwrap();
        frame(&ctx, &mut app);
        let current = search(&mut app)["current"].as_u64().unwrap();
        assert_eq!(current, (first + 1) % 2);
        let selected = run(&mut app, "vcs.workspace.state", json!({})).unwrap()["selected"].clone();
        let found = run(&mut app, "vcs.graph.search", json!({})).unwrap();
        assert_eq!(found["matches"][current as usize]["id"], selected, "the current match is selected");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(search(&mut app)["open"], json!(false));

        run(&mut app, "vcs.graph.solo", json!({ "name": "topic" })).unwrap();
        run(&mut app, "vcs.graph.hide", json!({ "name": "main" })).unwrap();
        frame(&ctx, &mut app);
        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(saved.contains("graphFocus"), "{saved}");
        drop(app);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        let focus = run(&mut app, "vcs.graph.focus", json!({})).unwrap();
        assert_eq!(focus["solo"], json!([{ "name": "topic", "kind": "local" }]), "{focus}");
        assert_eq!(focus["hidden"], json!([{ "name": "main", "kind": "local" }]));
        let page = run(&mut app, "vcs.graph.page", json!({ "wait": true })).unwrap();
        let subjects: Vec<&str> =
            page["commits"].as_array().unwrap().iter().map(|row| row["subject"].as_str().unwrap()).collect();
        assert_eq!(subjects, ["Fix the parser", "First"]);
        run(&mut app, "vcs.graph.hide", json!({ "showAll": true })).unwrap();
        run(&mut app, "vcs.graph.solo", json!({ "clear": true })).unwrap();
        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(!saved.contains("graphFocus"), "{saved}");
    }

    #[test]
    fn the_git_workspace_keeps_its_columns_and_pull_per_repository_and_has_its_keys() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        run(&mut app, "vcs.graph.page", json!({ "wait": true })).unwrap();
        frame(&ctx, &mut app);
        let order = json!(["graph", "refs", "message", "sha", "author", "date"]);
        let params = json!({ "order": order, "hide": ["author"], "widths": { "sha": 90 }, "highlightOnHover": false });
        run(&mut app, "vcs.workspaceColumns.set", params).unwrap();
        run(&mut app, "vcs.workspace.pullDefault", json!({ "mode": "fetchAll" })).unwrap();
        frame(&ctx, &mut app);
        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(saved.contains("\"columns\"") && saved.contains("\"pull\": \"fetchAll\""), "{saved}");
        drop(app);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        let columns = run(&mut app, "vcs.workspaceColumns.get", json!({})).unwrap();
        assert_eq!((columns["order"].clone(), columns["hidden"].clone()), (order, json!(["author"])), "{columns}");
        assert_eq!(
            (columns["widths"]["sha"].clone(), columns["highlightOnHover"].clone()),
            (json!(90.0), json!(false))
        );
        assert_eq!(run(&mut app, "vcs.workspace.pullDefault", json!({})).unwrap()["mode"], "fetchAll");
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘T"] })).unwrap();
        frame(&ctx, &mut app);
        let operation = run(&mut app, "vcs.operation.status", json!({})).unwrap();
        assert_eq!(operation["kind"], "fetch", "{operation}");
        std::fs::write(project.join("main.go"), "package main // changed\n").unwrap();
        run(&mut app, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⇧⌘S"] })).unwrap();
        frame(&ctx, &mut app);
        let stashes = run(&mut app, "vcs.stash.list", json!({})).unwrap();
        assert_eq!(stashes["stashes"].as_array().map(Vec::len), Some(1), "{stashes}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⇧⌘P"] })).unwrap();
        frame(&ctx, &mut app);
        let stashes = run(&mut app, "vcs.stash.list", json!({})).unwrap();
        assert_eq!(stashes["stashes"].as_array().map(Vec::len), Some(0), "{stashes}");
        run(&mut app, "vcs.workspaceColumns.set", json!({ "reset": true })).unwrap();
        run(&mut app, "vcs.workspace.pullDefault", json!({ "mode": "fastForward" })).unwrap();
        app.remember_workspace(&ctx, true);
        let saved = std::fs::read_to_string(data.path().join("workspace.json")).unwrap();
        assert!(!saved.contains("\"repositories\""), "{saved}");
    }

    #[test]
    fn directories_compare_filter_synchronise_and_open_their_files() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::with_language("en", compare_directories_in_english);
    }

    fn compare_directories_in_english() {
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        let (left, right) = (project.join("left"), project.join("right"));
        let put = |root: &Path, relative: &str, text: &str| {
            let path = root.join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        put(&left, "a.txt", "same\n");
        put(&right, "a.txt", "same\n");
        put(&left, "b.txt", "left b\n");
        put(&right, "b.txt", "right side b\n");
        put(&left, "only-left.txt", "mine\n");
        put(&left, "notes_test.go", "package notes\n");
        put(&right, "only-right.md", "# theirs\n");
        put(&left, "sub/x.go", "package sub\n");
        put(&right, "sub/x.go", "package sub\n");
        put(&left, ".DS_Store", "junk");
        put(&right, ".git/HEAD", "ref: refs/heads/main\n");
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let action = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let keys = |app: &mut IdeApp, keys: Value| ask(&ctx, app, "press_keys", json!({ "keys": keys })).unwrap();
        let window = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        let settled = |app: &mut IdeApp| {
            for _ in 0..500 {
                frame(&ctx, app);
                let state = window(app)["folderDiff"].clone();
                if state["comparing"] == false {
                    return state;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the comparison did not end");
        };
        let relatives = |state: &Value| -> Vec<String> {
            state["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry["relative"].as_str().unwrap().to_owned())
                .collect()
        };

        action(&mut app, "view.project.select", json!({ "paths": [left, right] })).unwrap();
        keys(&mut app, json!(["⌘D"]));
        frame(&ctx, &mut app);
        assert_eq!(window(&mut app)["editor"], "folderDiff", "⌘D with two folders compares them");
        let state = settled(&mut app);
        assert_eq!(state["title"], "left vs right");
        assert_eq!(state["counts"], json!({ "onlyLeft": 2, "onlyRight": 1, "different": 1, "equal": 2 }), "{state}");
        assert_eq!(state["shown"], json!(["onlyLeft", "onlyRight", "different"]), "equal files are hidden at first");
        assert_eq!(relatives(&state), ["b.txt", "notes_test.go", "only-left.txt", "only-right.md"]);
        assert_eq!(state["entries"][0]["action"], "toRight");
        assert_eq!(state["entries"][3]["action"], "toLeft");

        let all = action(
            &mut app,
            "view.folderDiff.filter",
            json!({ "states": ["onlyLeft", "onlyRight", "different", "equal"] }),
        )
        .unwrap();
        assert_eq!(all["result"]["rows"], 6, "{all}");
        let filtered = action(&mut app, "view.folderDiff.filter", json!({ "text": "*.txt & !only*" })).unwrap();
        assert_eq!(relatives(&filtered["result"]), ["a.txt", "b.txt"]);
        let either = action(&mut app, "view.folderDiff.filter", json!({ "text": "*.md | sub/" })).unwrap();
        assert_eq!(relatives(&either["result"]), ["only-right.md", "sub/x.go"]);
        let different =
            action(&mut app, "view.folderDiff.filter", json!({ "text": "", "states": ["different"] })).unwrap();
        assert_eq!(relatives(&different["result"]), ["b.txt"]);
        action(
            &mut app,
            "view.folderDiff.filter",
            json!({ "states": ["onlyLeft", "onlyRight", "different", "equal"] }),
        )
        .unwrap();

        let refused = action(&mut app, "view.folderDiff.setAction", json!({ "path": "a.txt", "action": "toRight" }));
        assert!(refused.is_err(), "an equal file has nothing to do");
        let delete =
            action(&mut app, "view.folderDiff.setAction", json!({ "path": "notes_test.go", "action": "delete" }))
                .unwrap();
        let notes =
            delete["result"]["entries"].as_array().unwrap().iter().find(|entry| entry["relative"] == "notes_test.go");
        assert_eq!(notes.unwrap()["action"], "delete");

        action(&mut app, "view.folderDiff.select", json!({ "paths": ["only-left.txt"] })).unwrap();
        let copied = action(&mut app, "view.folderDiff.synchronize", json!({})).unwrap();
        assert_eq!(
            copied["result"]["synchronized"]["applied"],
            json!([{ "relative": "only-left.txt", "action": "toRight" }])
        );
        assert_eq!(std::fs::read_to_string(right.join("only-left.txt")).unwrap(), "mine\n");
        assert_eq!(copied["result"]["counts"]["equal"], 3, "the list is compared again: {}", copied["result"]);

        let asked = action(&mut app, "view.folderDiff.synchronize", json!({ "all": true })).unwrap_err();
        assert!(
            asked.contains("confirm: true")
                && asked.contains("toRight b.txt")
                && asked.contains("delete notes_test.go"),
            "{asked}"
        );
        assert_eq!(
            std::fs::read_to_string(right.join("b.txt")).unwrap(),
            "right side b\n",
            "nothing done before confirming"
        );
        assert!(left.join("notes_test.go").exists());
        let synced = action(&mut app, "view.folderDiff.synchronize", json!({ "all": true, "confirm": true })).unwrap();
        assert_eq!(synced["result"]["synchronized"]["applied"].as_array().unwrap().len(), 3, "{synced}");
        assert_eq!(std::fs::read_to_string(right.join("b.txt")).unwrap(), "left b\n");
        assert!(!left.join("notes_test.go").exists());
        assert_eq!(std::fs::read_to_string(left.join("only-right.md")).unwrap(), "# theirs\n");
        assert!(!right.join(".DS_Store").exists() && right.join(".git/HEAD").exists(), "ignored names are left alone");
        assert_eq!(synced["result"]["counts"], json!({ "onlyLeft": 0, "onlyRight": 0, "different": 0, "equal": 5 }));

        put(&right, "sub/x.go", "package sub // changed\n");
        let refreshed = action(&mut app, "view.folderDiff.refresh", json!({})).unwrap();
        assert_eq!(refreshed["result"]["counts"]["different"], 1, "a change on disk shows after Refresh");

        action(&mut app, "view.folderDiff.select", json!({ "paths": ["sub/x.go"] })).unwrap();
        frame(&ctx, &mut app);
        keys(&mut app, json!(["Enter"]));
        frame(&ctx, &mut app);
        let state = window(&mut app);
        assert_eq!(state["editor"], "diff", "Enter opens the file diff: {state}");
        assert_eq!(
            state["diff"]["titles"],
            json!([left.join("sub/x.go").display().to_string(), right.join("sub/x.go").display().to_string()])
        );
        let equal = action(&mut app, "view.folderDiff.open", json!({ "path": "only-right.md" })).unwrap();
        assert_eq!(equal["result"]["hunks"], json!([]), "a file copied over is equal now");
        assert!(action(&mut app, "view.folderDiff.open", json!({ "path": "../escape" })).is_err());

        put(&left, "fresh.txt", "new\n");
        let back = action(&mut app, "view.project.compareWith", json!({ "paths": [left, right] })).unwrap();
        assert_eq!(back["result"]["counts"]["onlyLeft"], 1, "{back}");
        let one = action(&mut app, "view.folderDiff.open", json!({ "path": "fresh.txt" })).unwrap();
        assert_eq!(one["result"]["before"], "new\n");
        assert_eq!(one["result"]["after"], "");
        frame(&ctx, &mut app);
        assert_eq!(window(&mut app)["editor"], "diff");

        let listed =
            run(&mut app, "file.compare.folders", json!({ "left": "left", "right": "right", "states": ["onlyLeft"] }))
                .unwrap();
        assert_eq!(listed["entries"][0]["relative"], "fresh.txt", "paths relative to the project: {listed}");
        let found = ask(&ctx, &mut app, "find_action", json!({ "query": "directories", "limit": 50 })).unwrap();
        let names: Vec<&str> =
            found["actions"].as_array().unwrap().iter().filter_map(|entry| entry["action"].as_str()).collect();
        assert!(
            names.contains(&"view.folderDiff.synchronize") && names.contains(&"view.folderDiff.compare"),
            "{names:?}"
        );
        assert!(!names.contains(&"view.folderDiff.setAction"), "{names:?}");

        crate::ide::compare::ASKED_OTHER.with(|asked| asked.take());
        action(&mut app, "view.project.select", json!({ "paths": [left] })).unwrap();
        keys(&mut app, json!(["⌘D"]));
        frame(&ctx, &mut app);
        assert_eq!(
            crate::ide::compare::ASKED_OTHER.with(|asked| asked.take()),
            Some(left.clone()),
            "⌘D with one folder asks for the other"
        );
        let outside = action(&mut app, "view.folderDiff.compare", json!({ "left": left, "right": left.join("a.txt") }));
        assert!(outside.is_err_and(|error| error.contains("not a folder")));
    }

    #[test]
    fn files_compare_from_the_tree_with_the_clipboard_and_in_a_blank_diff() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::with_language("en", compare_files_in_english);
    }

    fn compare_files_in_english() {
        let data = tempfile::tempdir().unwrap();
        let (project, main) = window_project(data.path());
        let project = project.canonicalize().unwrap();
        let main = project.join(main.file_name().unwrap());
        let [a, b, c] = ["a.txt", "b.txt", "c.txt"].map(|name| project.join(name));
        std::fs::write(&a, "one\ntwo\nthree\n").unwrap();
        std::fs::write(&b, "one\n2\nthree\n").unwrap();
        std::fs::write(&c, "one\n2\nthree\nfour\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let action = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let keys = |app: &mut IdeApp, keys: Value| ask(&ctx, app, "press_keys", json!({ "keys": keys })).unwrap();
        let diff = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap()["diff"].clone();
        let full = |app: &mut IdeApp| {
            action(app, "view.diff.whitespace", json!({ "mode": "none" })).unwrap()["result"].clone()
        };
        let editor = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap()["editor"].clone();

        action(&mut app, "view.project.select", json!({ "paths": [a, b] })).unwrap();
        keys(&mut app, json!(["⌘D"]));
        frame(&ctx, &mut app);
        assert_eq!(editor(&mut app), "diff", "⌘D with two files compares them at once");
        let state = diff(&mut app);
        assert_eq!(state["sides"], 2, "{state}");
        assert_eq!(state["titles"], json!([a.display().to_string(), b.display().to_string()]));
        assert_eq!(state["differences"], 1);

        action(&mut app, "view.project.select", json!({ "paths": [a, b, c] })).unwrap();
        keys(&mut app, json!(["⌘D"]));
        frame(&ctx, &mut app);
        let state = full(&mut app);
        assert_eq!(state["sides"], 3, "⌘D with three files opens the three-side viewer: {state}");
        assert_eq!(
            (state["leftHunks"].as_array().unwrap().len(), state["rightHunks"].as_array().unwrap().len()),
            (1, 1)
        );
        assert_eq!(state["differences"], 2);
        keys(&mut app, json!(["F7"]));
        assert_eq!(diff(&mut app)["current"], 1, "F7 goes to the next difference");
        keys(&mut app, json!(["⇧F7"]));
        assert_eq!(diff(&mut app)["current"], 0, "⇧F7 to the previous one");
        assert_eq!(action(&mut app, "view.diff.swapSides", json!({})).unwrap()["result"]["paths"], json!([c, b, a]));
        assert!(
            action(&mut app, "view.diff.viewer", json!({ "unified": true })).is_err(),
            "three files are side by side"
        );

        crate::ide::compare::ASKED_OTHER.with(|asked| asked.take());
        action(&mut app, "view.project.select", json!({ "paths": [a] })).unwrap();
        keys(&mut app, json!(["⌘D"]));
        frame(&ctx, &mut app);
        assert_eq!(
            crate::ide::compare::ASKED_OTHER.with(|asked| asked.take()),
            Some(a.clone()),
            "⌘D with one file asks for the other"
        );
        let refused = action(&mut app, "view.project.compareWith", json!({ "path": a })).unwrap_err();
        assert!(refused.contains("system dialog"), "{refused}");
        let both = action(&mut app, "view.project.compareWith", json!({ "paths": [b, c] })).unwrap();
        assert_eq!(both["result"]["hunks"].as_array().map(Vec::len), Some(1), "{both}");

        run(&mut app, "file.document.open", json!({ "path": main })).unwrap();
        frame(&ctx, &mut app);
        let empty = action(&mut app, "view.compare.clipboard", json!({ "path": main }));
        assert!(empty.is_err_and(|error| error.contains("clipboard")), "nothing on the clipboard yet");
        {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            let clipboard = Arc::new(zen_workbench::file_clipboard::MemoryClipboard::default());
            zen_workbench::file_clipboard::SystemClipboard::set_text(
                clipboard.as_ref(),
                "package main\n\nfunc main() {\n    println(2)\n}\n",
            );
            zen_workbench::file_clipboard::install(&mut window.session, clipboard);
        }
        action(&mut app, "view.compare.clipboard", json!({ "path": main })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(editor(&mut app), "diff");
        let state = diff(&mut app);
        assert_eq!(state["titles"], json!(["Clipboard", main.display().to_string()]), "{state}");
        assert_eq!(state["editable"], json!([false, true]), "the file's side is editable");
        assert_eq!(state["differences"], 1);
        assert_eq!(
            action(&mut app, "view.diff.whitespace", json!({ "mode": "ignore" })).unwrap()["result"]["differences"],
            1
        );
        let typed = action(
            &mut app,
            "view.diff.setText",
            json!({ "side": "right", "text": "package main\n\nfunc main() {\n\tprintln(2)\n}\n" }),
        )
        .unwrap();
        assert_eq!(typed["result"]["differences"], 0, "only whitespace differs now: {typed}");
        assert_eq!(
            action(&mut app, "view.diff.whitespace", json!({ "mode": "none" })).unwrap()["result"]["differences"],
            1
        );
        keys(&mut app, json!(["⌘S"]));
        frame(&ctx, &mut app);
        assert_eq!(
            std::fs::read_to_string(&main).unwrap(),
            "package main\n\nfunc main() {\n\tprintln(2)\n}\n",
            "the file's side typed into and saved with ⌘S"
        );
        let unified = action(&mut app, "view.diff.viewer", json!({ "unified": true })).unwrap();
        assert_eq!(unified["result"]["unified"], true);
        frame(&ctx, &mut app);
        keys(&mut app, json!(["F7"]));
        assert_eq!(diff(&mut app)["unified"], true);
        action(&mut app, "view.diff.viewer", json!({ "unified": false })).unwrap();

        run(&mut app, "file.document.open", json!({ "path": main })).unwrap();
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": 14, "head": 27 }] })).unwrap();
        let selected =
            action(&mut app, "view.compare.clipboard", json!({ "path": main, "text": "func main() {" })).unwrap();
        assert_eq!(selected["result"]["after"], "func main() {", "{selected}");
        assert_eq!(selected["result"]["afterRange"], json!([14, 27]));
        frame(&ctx, &mut app);
        action(&mut app, "view.diff.setText", json!({ "side": "right", "text": "func start() {" })).unwrap();
        assert_eq!(diff(&mut app)["range"], json!([14, 28]), "the selection's side grows with its text");
        keys(&mut app, json!(["⌘S"]));
        frame(&ctx, &mut app);
        assert_eq!(std::fs::read_to_string(&main).unwrap(), "package main\n\nfunc start() {\n\tprintln(2)\n}\n");
        let replace = json!({ "path": main, "oldText": "package main", "newText": "package main // x" });
        run(&mut app, "editor.text.replace", replace).unwrap();
        frame(&ctx, &mut app);
        let followed = full(&mut app);
        assert_eq!(followed["range"], json!([19, 33]), "an edit before the selection moves its side: {followed}");
        assert_eq!(followed["texts"][1], "func start() {");
        run(&mut app, "file.document.save", json!({ "path": main })).unwrap();
        std::fs::write(&main, "package main // x\n\nfunc begin() {\n\tprintln(2)\n}\n").unwrap();
        run(&mut app, "file.document.syncWithDisk", json!({})).unwrap();
        frame(&ctx, &mut app);
        let reloaded = full(&mut app);
        assert_eq!(reloaded["texts"][1], "func begin() {", "the side follows the file read again: {reloaded}");
        assert_eq!(reloaded["range"], json!([19, 33]));
        assert_eq!(reloaded["differences"], 1);
        action(&mut app, "view.diff.setText", json!({ "side": "right", "text": "func main() {" })).unwrap();
        run(&mut app, "file.document.save", json!({ "path": main })).unwrap();
        assert_eq!(std::fs::read_to_string(&main).unwrap(), "package main // x\n\nfunc main() {\n\tprintln(2)\n}\n");
        let found = ask(&ctx, &mut app, "find_action", json!({ "query": "diff", "limit": 200 })).unwrap();
        let listed: Vec<&str> =
            found["actions"].as_array().unwrap().iter().filter_map(|entry| entry["action"].as_str()).collect();
        assert!(listed.contains(&"view.diff.next") && listed.contains(&"view.diff.blank"), "{listed:?}");
        assert!(!listed.contains(&"view.diff.setText"), "a pane's text is given by agents only: {listed:?}");

        let blank = action(&mut app, "view.diff.blank", json!({})).unwrap();
        assert_eq!(blank["result"]["title"], "Blank Diff", "{blank}");
        assert_eq!(blank["result"]["editable"], json!([true, true]));
        assert_eq!(blank["result"]["texts"], json!(["", ""]));
        frame(&ctx, &mut app);
        assert_eq!(editor(&mut app), "diff");
        action(&mut app, "view.diff.setText", json!({ "side": "left", "text": "a\n  b\n" })).unwrap();
        let dropped = action(&mut app, "view.diff.setText", json!({ "side": "right", "path": b })).unwrap();
        assert_eq!(dropped["result"]["titles"][1], b.display().to_string());
        assert_eq!(dropped["result"]["differences"], 1);
        let swapped = action(&mut app, "view.diff.swapSides", json!({})).unwrap();
        assert_eq!(swapped["result"]["texts"], json!(["one\n2\nthree\n", "a\n  b\n"]));
        assert_eq!(swapped["result"]["swapped"], true);
        action(&mut app, "view.diff.setText", json!({ "side": "left", "text": "a\nb\n" })).unwrap();
        assert_eq!(
            action(&mut app, "view.diff.whitespace", json!({ "mode": "trim" })).unwrap()["result"]["differences"],
            0
        );
        assert_eq!(
            action(&mut app, "view.diff.whitespace", json!({ "mode": "none" })).unwrap()["result"]["differences"],
            1
        );
        frame(&ctx, &mut app);

        #[derive(Debug)]
        struct Dropped(PathBuf);
        impl egui::DroppedFile for Dropped {
            fn path(&self) -> &Path {
                &self.0
            }

            fn bytes(&self) -> Result<Vec<u8>, String> {
                std::fs::read(&self.0).map_err(|error| error.to_string())
            }
        }
        let outside = data.path().join("outside.txt");
        std::fs::write(&outside, "dropped\n").unwrap();
        let drop = |app: &mut IdeApp| {
            let handle: egui::DroppedFileHandle = Arc::new(Dropped(outside.clone()));
            let input = egui::RawInput { dropped_files: vec![handle], ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
        };
        drop(&mut app);
        frame(&ctx, &mut app);
        let state = diff(&mut app);
        assert_eq!(
            state["titles"][0],
            outside.display().to_string(),
            "the typed diff in front takes the drop: {state}"
        );
        assert!(!project.join("outside.txt").exists(), "the tree does not copy what the diff took");
        run(&mut app, "file.document.open", json!({ "path": main })).unwrap();
        frame(&ctx, &mut app);
        drop(&mut app);
        for _ in 0..5 {
            frame(&ctx, &mut app);
        }
        assert!(project.join("outside.txt").exists(), "with a file in front the tree takes drops again");
        let replace = json!({ "path": main, "oldText": "package main // x", "newText": "package zen" });
        run(&mut app, "editor.text.replace", replace).unwrap();
        frame(&ctx, &mut app);
        keys(&mut app, json!(["⌘Z"]));
        frame(&ctx, &mut app);
        let text = ask(&ctx, &mut app, "get_document_text", json!({ "path": main })).unwrap();
        assert!(
            text["text"].as_str().unwrap().starts_with("package main // x"),
            "⌘Z in the editor still undoes: {text}"
        );
    }

    #[test]
    fn the_git_workspace_asks_before_discarding_and_its_diff_follows_staging() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        std::fs::write(&file, "package main\n\nfunc a() {}\n\nfunc b() {}\n").unwrap();
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        std::fs::write(&file, "package main\n\nfunc A() {}\n\nfunc b() {}\n\nfunc c() {}\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        run(&mut app, "vcs.workspace.select", json!({ "wip": true })).unwrap();
        frame(&ctx, &mut app);

        let shown = run(&mut app, "vcs.workspace.openDiff", json!({ "path": file, "part": "unstaged" })).unwrap();
        assert_eq!(shown["hunks"].as_array().map(Vec::len), Some(2));
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["editor"], "diff");
        run(&mut app, "vcs.index.stage", json!({ "paths": [file], "hunks": [1] })).unwrap();
        run(&mut app, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let diff = |app: &mut IdeApp| {
            let windows = app.shared.state().windows.clone();
            let window = windows[0].window.lock().unwrap();
            window.view.diff.as_ref().map(|diff| diff.view.hunks.len())
        };
        assert_eq!(diff(&mut app), Some(1), "the staged hunk left the unstaged diff");

        let asked = run(&mut app, "vcs.workspace.discard", json!({ "paths": [file], "hunks": [0] })).unwrap();
        assert_eq!(asked["dialog"], "discardChanges");
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["discardChanges"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert!(std::fs::read_to_string(&file).unwrap().contains("func A()"), "nothing discarded");
        run(&mut app, "vcs.workspace.discard", json!({ "paths": [file], "hunks": [0] })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!([]));
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "package main\n\nfunc a() {}\n\nfunc b() {}\n\nfunc c() {}\n"
        );
        assert_eq!(diff(&mut app), Some(0), "the diff tab shows nothing left to stage");

        run(&mut app, "vcs.commit.create", json!({ "message": "Add c", "staged": true })).unwrap();
        assert_eq!(test_git(&project, &["status", "--porcelain"]).trim(), "");
    }

    #[test]
    fn the_git_workspace_compares_picked_points_and_blames_a_file_from_its_menu() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        let first = test_git(&project, &["rev-parse", "HEAD"]).trim().to_owned();
        std::fs::write(&file, "package main\n\nfunc main() {}\n\nfunc b() {}\n").unwrap();
        test_git(&project, &["commit", "-q", "-am", "Add b"]);
        let second = test_git(&project, &["rev-parse", "HEAD"]).trim().to_owned();
        std::fs::write(project.join("notes.txt"), "notes\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        let compared = |app: &mut IdeApp| {
            let windows = app.shared.state().windows.clone();
            let window = windows[0].window.lock().unwrap();
            window.view.git_workspace.compared_files()
        };
        run(&mut app, "vcs.workspace.open", json!({})).unwrap();
        run(&mut app, "vcs.workspace.select", json!({ "revision": first })).unwrap();

        run(&mut app, "vcs.workspace.select", json!({ "wip": true, "toggle": true })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let files = compared(&mut app).expect("the panel shows the comparison");
        assert_eq!(files, ["main.go modified", "notes.txt added"]);
        run(&mut app, "vcs.workspace.select", json!({ "revision": second })).unwrap();
        run(&mut app, "vcs.workspace.select", json!({ "revision": first, "toggle": true })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(compared(&mut app).unwrap(), ["main.go modified"]);

        run(&mut app, "vcs.workspace.fileMenu", json!({ "path": file, "id": second })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(state(&mut app)["popups"], json!(["fileMenu"]));
        run(&mut app, "vcs.workspace.fileAction", json!({ "path": file, "action": "blame" })).unwrap();
        frame(&ctx, &mut app);
        let shown = state(&mut app);
        assert!(shown["editor"].as_str().is_some_and(|path| path.ends_with("main.go")), "{shown}");
        assert_eq!(run(&mut app, "vcs.blame.get", json!({ "wait": true })).unwrap()["shown"], json!(true));
        {
            let windows = app.shared.state().windows.clone();
            windows[0].window.lock().unwrap().view.editor_view.click_annotation(first.clone());
        }
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let workspace = run(&mut app, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(workspace["selected"], json!(first));
        assert_eq!(state(&mut app)["editor"], "gitWorkspace", "the workspace comes to the front");

        run(&mut app, "vcs.workspace.fileAction", json!({ "path": file, "action": "history" })).unwrap();
        frame(&ctx, &mut app);
        let windows = app.shared.state().windows.clone();
        let window = windows[0].window.lock().unwrap();
        let history = window.view.git_workspace.history_subjects();
        assert_eq!(history, Some(vec!["Add b".to_owned(), "First".to_owned()]));
    }

    #[test]
    fn exiting_asks_first_and_names_the_work_still_running() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let fetch = {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            window.session.state_mut::<zen_workbench::tasks::Tasks>().start("Fetching origin", "vcs", false)
        };
        let popups = |ctx: &egui::Context, app: &mut IdeApp| {
            frame(ctx, app);
            ask(ctx, app, "get_window_state", json!({})).unwrap()["popups"].clone()
        };

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘Q"] })).unwrap();
        assert_eq!(popups(&ctx, &mut app), json!(["confirmExit"]));
        assert!(!app.shared.is_exiting());
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(popups(&ctx, &mut app), json!([]));
        assert!(!app.shared.is_exiting(), "Cancel keeps the IDE open");

        let answer = ask(&ctx, &mut app, "exit_ide", json!({})).unwrap();
        assert_eq!(answer["exiting"], false, "{answer}");
        assert_eq!(answer["running"], json!([{ "kind": "git", "title": "Fetching origin" }]), "{answer}");
        assert_eq!(popups(&ctx, &mut app), json!(["confirmExit"]));
        {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            let running = window.view.exit_prompt.clone().unwrap();
            let line = super::super::exit_dialog::work_line(&running[0]);
            assert_eq!(line, zen_i18n::t!("ide-exit-work-git", title = "Fetching origin"));
        }

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert!(app.shared.is_exiting());
        assert!(app.exit_at.is_some());
        assert_eq!(app.shared.overview().open.len(), 1);
        fetch.finish(Ok(String::new()));
    }

    #[cfg(unix)]
    #[test]
    fn closing_a_project_or_exiting_saves_unsaved_changes_and_stays_when_it_cannot() {
        use std::os::unix::fs::PermissionsExt;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let before = std::fs::read_to_string(&file).unwrap();
        let ctx = egui::Context::default();
        let edit = |ctx: &egui::Context, app: &mut IdeApp, text: &str| {
            let open = json!({ "command": "file.document.open", "params": { "path": file } });
            ask(ctx, app, "dispatch_command", open).unwrap();
            let insert = json!({ "command": "editor.edit.insert", "params": { "text": text } });
            ask(ctx, app, "dispatch_command", insert).unwrap();
        };
        let folder = |mode| std::fs::set_permissions(&project, std::fs::Permissions::from_mode(mode)).unwrap();

        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        edit(&ctx, &mut app, "// closed\n");
        ask(&ctx, &mut app, "close_project", json!({ "project": project })).unwrap();
        assert!(app.shared.overview().open.is_empty(), "the project closed");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), format!("// closed\n{before}"), "and its edit is on disk");
        drop(app);

        let saved = std::fs::read_to_string(&file).unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        edit(&ctx, &mut app, "// stays\n");
        folder(0o555);
        let closed = ask(&ctx, &mut app, "close_project", json!({ "project": project }));
        let exited = ask(&ctx, &mut app, "exit_ide", json!({ "confirmed": true }));
        let open = app.shared.overview().open;
        app.handle(&ctx, Request::Close(open[0].clone()));
        app.handle(&ctx, Request::Exit { confirmed: true });
        folder(0o755);
        assert!(closed.as_ref().is_err_and(|error| error.contains("main.go")), "{closed:?}");
        assert!(exited.as_ref().is_err_and(|error| error.contains("main.go")), "{exited:?}");
        assert_eq!(app.shared.overview().open, open, "the window stays");
        assert!(!app.shared.is_exiting(), "and the IDE too");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), saved);
        {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            let document = window.session.state::<Workbench>().unwrap().active_document().unwrap();
            assert!(document.is_dirty() && document.text().starts_with("// stays\n"), "with its changes");
        }
        frame(&ctx, &mut app);
        let list = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "ui.notifications.list" })).unwrap();
        let errors: Vec<&Value> =
            list["notifications"].as_array().unwrap().iter().filter(|item| item["type"] == "error").collect();
        assert!(errors.iter().any(|item| item["body"].as_str().is_some_and(|body| body.contains("main.go"))), "{list}");
    }

    #[test]
    fn agents_and_the_setting_exit_without_asking() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let ctx = egui::Context::default();

        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        let answer = ask(&ctx, &mut app, "exit_ide", json!({ "confirmed": true })).unwrap();
        assert_eq!(answer, json!({ "exiting": true, "asking": false, "running": [] }));
        frame(&ctx, &mut app);
        assert!(app.shared.is_exiting());
        assert_eq!(ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"], json!([]));
        drop(app);

        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        let ran =
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.ide.exit", "params": { "confirmed": true } }))
                .unwrap();
        assert_eq!(ran["action"], "view.ide.exit", "{ran}");
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert!(app.shared.is_exiting());
        drop(app);

        let mut app = themed_app(data.path(), vec![project]);
        frame(&ctx, &mut app);
        assert_eq!(app.tool(&ctx, "get_settings", &json!({})).unwrap()["confirmExit"], true);
        app.tool(&ctx, "set_settings", &json!({ "confirmExit": false })).unwrap();
        frame(&ctx, &mut app);
        assert!(!app.shared.overview().settings.confirm_exit);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘Q"] })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert!(app.shared.is_exiting());
    }

    #[test]
    fn a_system_quit_asks_as_the_ides_own_exit_does() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let ctx = egui::Context::default();
        let popups = |ctx: &egui::Context, app: &mut IdeApp| {
            frame(ctx, app);
            ask(ctx, app, "get_window_state", json!({})).unwrap()["popups"].clone()
        };

        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        assert_eq!(app.shared.system_quit.system_asks(), zen_macos::QuitAnswer::Cancel);
        assert_eq!(popups(&ctx, &mut app), json!(["confirmExit"]));
        assert!(!app.shared.is_exiting());

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(popups(&ctx, &mut app), json!([]));
        assert!(!app.shared.is_exiting());
        assert_eq!(app.shared.system_quit.system_asks(), zen_macos::QuitAnswer::Cancel);
        assert_eq!(popups(&ctx, &mut app), json!(["confirmExit"]));

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        assert!(app.shared.is_exiting());
        assert_eq!(app.shared.system_quit.system_asks(), zen_macos::QuitAnswer::Terminate);
        drop(app);

        let mut app = themed_app(data.path(), vec![project.clone()]);
        frame(&ctx, &mut app);
        app.tool(&ctx, "set_settings", &json!({ "confirmExit": false })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(app.shared.system_quit.system_asks(), zen_macos::QuitAnswer::Cancel);
        frame(&ctx, &mut app);
        assert!(app.shared.is_exiting());
        assert_eq!(ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"], json!([]));
        drop(app);

        let mut app = themed_app(data.path(), vec![project]);
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "exit_ide", json!({ "confirmed": true })).unwrap();
        frame(&ctx, &mut app);
        assert!(app.shared.is_exiting());
        assert_eq!(app.shared.system_quit.system_asks(), zen_macos::QuitAnswer::Terminate);
        assert!(!app.shared.system_quit.take_request());
    }

    #[cfg(unix)]
    #[test]
    fn exiting_stops_the_runs_before_the_ide_ends() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let pid_file = data.path().join("pid");
        let script = format!("echo $$ > '{}'; sleep 60", pid_file.display());
        {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            let runs = window.session.state_mut::<zen_workbench::run::Runs>();
            runs.set_providers(vec![Arc::new(zen_run::fake::ShellProvider)], zen_run::RunEnv::default());
            let context = zen_run::RunContext { root: project, ..zen_run::RunContext::default() };
            runs.start(zen_run::fake::ShellProvider::configuration("server", &script), &context).unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        let pid = loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file).ok().filter(|pid| pid.ends_with('\n')) {
                break pid.trim().to_owned();
            }
            assert!(Instant::now() < deadline, "the program did not start");
            std::thread::sleep(Duration::from_millis(20));
        };
        let alive = |pid: &str| {
            std::process::Command::new("kill")
                .args(["-0", pid])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        assert!(alive(&pid));

        let answer = ask(&ctx, &mut app, "exit_ide", json!({ "confirmed": true })).unwrap();
        assert_eq!(answer["running"], json!([{ "kind": "run", "title": "server" }]), "{answer}");
        app.exit_at = Some(f64::MIN);
        frame(&ctx, &mut app);
        let deadline = Instant::now() + Duration::from_secs(30);
        while alive(&pid) {
            assert!(Instant::now() < deadline, "the run outlives the exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        assert!(zen_workbench::exit::running_runs(&window.session).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn a_start_that_waits_is_answered_when_the_run_ends_while_the_ide_goes_on() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            let runs = window.session.state_mut::<zen_workbench::run::Runs>();
            runs.set_providers(vec![Arc::new(zen_run::fake::ShellProvider)], zen_run::RunEnv::default());
        }
        let gate = data.path().join("gate");
        let script = format!("while [ ! -f '{}' ]; do sleep 0.05; done; echo done", gate.display());
        let configuration = serde_json::to_value(zen_run::fake::ShellProvider::configuration("slow", &script)).unwrap();
        let params = json!({ "configuration": configuration, "waitMs": 60_000 });
        let (sender, answer) = mpsc::channel();
        app.route(
            &ctx,
            "dispatch_command",
            &json!({ "command": "run.process.start", "params": params }),
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        assert!(answer.try_recv().is_err(), "the call does not hold the UI thread: no answer while the run runs");
        let windows = ask(&ctx, &mut app, "list_windows", json!({})).unwrap();
        assert_eq!(windows["windows"].as_array().unwrap().len(), 1, "other calls are answered meanwhile");
        std::fs::write(&gate, "").unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        let started = loop {
            if let Ok(result) = answer.try_recv() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline, "the waiting call was never answered");
            frame(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(started["summary"]["state"], "finished", "{started}");
        assert_eq!(
            (started["summary"]["done"].as_bool(), started["summary"]["exitCode"].as_i64()),
            (Some(true), Some(0))
        );
        let run_id = started["runId"].clone();

        let configuration =
            serde_json::to_value(zen_run::fake::ShellProvider::configuration("long", "sleep 30")).unwrap();
        let params = json!({ "configuration": configuration, "waitMs": 100 });
        let (sender, answer) = mpsc::channel();
        app.route(
            &ctx,
            "dispatch_command",
            &json!({ "command": "run.process.start", "params": params }),
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        let started = loop {
            if let Ok(result) = answer.try_recv() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline, "the time ran out without an answer");
            frame(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_ne!(started["runId"], run_id);
        assert_eq!(
            (started["summary"]["state"].as_str(), started["summary"]["done"].as_bool()),
            (Some("running"), Some(false))
        );
        ask(
            &ctx,
            &mut app,
            "dispatch_command",
            json!({ "command": "run.process.stop", "params": { "runId": started["runId"] } }),
        )
        .unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn exiting_stops_a_git_operation_that_hangs() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let server = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let remote = format!("http://127.0.0.1:{}/never.git", server.local_addr().unwrap().port());
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        test_git(&project, &["remote", "add", "origin", &remote]);
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "vcs.remote.fetch", "params": {} })).unwrap();

        let listings = tempfile::tempdir().unwrap();
        let listing = listings.path().join("listing.txt");
        let printed = |program: &str, args: &[&str]| -> String {
            let file = std::fs::File::create(&listing).unwrap();
            std::process::Command::new(program).args(args).stdout(file).status().unwrap();
            std::fs::read_to_string(&listing).unwrap()
        };
        let processes = || -> Vec<String> {
            let found = printed("pgrep", &["-f", &remote]);
            let helpers: Vec<String> = found.split_whitespace().map(str::to_owned).collect();
            let mut all = helpers.clone();
            for helper in &helpers {
                let parent = printed("ps", &["-o", "ppid=", "-p", helper]).trim().to_owned();
                if !parent.is_empty() && parent != std::process::id().to_string() && !all.contains(&parent) {
                    all.push(parent);
                }
            }
            all
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        let started = loop {
            let started = processes();
            if !started.is_empty() {
                break started;
            }
            assert!(Instant::now() < deadline, "the fetch did not reach the remote");
            std::thread::sleep(Duration::from_millis(50));
        };
        let answer = ask(&ctx, &mut app, "exit_ide", json!({ "confirmed": true })).unwrap();
        assert_eq!(answer["running"][0]["kind"], "git", "{answer}");

        app.exit_at = Some(f64::MIN);
        frame(&ctx, &mut app);
        let alive = |pid: &str| {
            std::process::Command::new("kill")
                .args(["-0", pid])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        while started.iter().any(|pid| alive(pid)) {
            assert!(Instant::now() < deadline, "git outlives the exit: {started:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(server);
    }

    #[test]
    fn the_welcome_window_asks_before_exiting_too() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        let ctx = egui::Context::default();
        let answer = app.tool(&ctx, "exit_ide", &json!({})).unwrap();
        assert_eq!(answer["asking"], true, "{answer}");
        assert!(app.welcome_exit);
        assert!(!app.shared.is_exiting());
    }

    #[test]
    fn agents_open_and_close_tool_windows_and_dialogs() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();

        let opened = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "view.toolWindow.problems" })).unwrap();
        assert_eq!(opened["action"], "view.toolWindow.problems");
        assert_eq!(opened["window"]["bottom"], "problems", "{opened}");
        assert_eq!(opened["changed"], true);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["bottom"], "problems");
        assert!(state["editor"].as_str().is_some_and(|path| path.ends_with("main.go")), "{state}");
        let closed = ask(&ctx, &mut app, "run_action", json!({ "action": "view.toolWindow.problems" })).unwrap();
        assert_eq!(closed["window"]["bottom"], Value::Null, "the same action again hides it");

        let settings = ask(&ctx, &mut app, "run_action", json!({ "action": "view.settings.open" })).unwrap();
        assert_eq!(settings["window"]["popups"], json!(["settings:mcp"]), "{settings}");
        frame(&ctx, &mut app);
        let escaped = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(escaped["window"]["popups"], json!([]), "{escaped}");
        let structure = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘;"] })).unwrap();
        assert_eq!(structure["window"]["popups"], json!(["settings:projectStructure"]), "{structure}");
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();

        let keys = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⌘A"] })).unwrap();
        assert_eq!(keys["window"]["popups"], json!(["searchEverywhere:actions"]), "{keys}");
        ask(&ctx, &mut app, "press_keys", json!({ "text": "problems" })).unwrap();
        let picked = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        assert_eq!(picked["window"]["bottom"], "problems", "Enter ran the first match: {picked}");

        let typed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⌘A"], "text": "git tool" })).unwrap();
        assert_eq!(typed["window"]["popups"], json!(["searchEverywhere:actions"]), "{typed}");
        let picked = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        assert_eq!(picked["window"]["bottom"], "git", "Enter ran the match of the text: {picked}");

        let classes = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘O"] })).unwrap();
        assert_eq!(classes["window"]["popups"], json!(["searchEverywhere:classes"]), "{classes}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        let symbols = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⌘O"] })).unwrap();
        assert_eq!(symbols["window"]["popups"], json!(["searchEverywhere:symbols"]), "{symbols}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();

        let refused = ask(&ctx, &mut app, "run_action", json!({ "action": "view.project.open" })).unwrap_err();
        assert!(refused.contains("open_project"), "{refused}");
        let unknown = ask(&ctx, &mut app, "run_action", json!({ "action": "view.nothing" })).unwrap_err();
        assert!(unknown.contains("no action"), "{unknown}");
        assert!(ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘"] })).unwrap_err().contains("not a keystroke"));
    }

    #[test]
    fn agents_show_the_todo_tool_window_and_walk_its_items() {
        struct Comments;

        impl zen_syntax::Highlighter for Comments {
            fn handles(&self, file_name: &str) -> bool {
                file_name.ends_with(".go")
            }

            fn highlight(&self, text: &str) -> Vec<zen_syntax::Span> {
                let mut spans = Vec::new();
                let mut offset = 0;
                for line in text.split_inclusive('\n') {
                    if let Some(at) = line.find("//") {
                        let range = offset + at..offset + line.trim_end().len();
                        spans.push(zen_syntax::Span { range, kind: zen_syntax::HighlightKind::Comment });
                    }
                    offset += line.len();
                }
                spans
            }
        }

        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        std::fs::write(&file, "package main\n\n// TODO: first\nfunc main() {} // FIXME second\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct { highlighters: vec![Arc::new(Comments)], ..test_product() };
        let mut app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);

        let shown = ask(&ctx, &mut app, "run_action", json!({ "action": "view.toolWindow.todo" })).unwrap();
        assert_eq!(shown["window"]["bottom"], "todo", "{shown}");
        let mut state = Value::Null;
        for _ in 0..400 {
            frame(&ctx, &mut app);
            state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
            if state["todo"]["items"] == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let todo = &state["todo"];
        assert_eq!(
            (&todo["items"], &todo["files"], &todo["tab"]),
            (&json!(2), &json!(1), &json!("project")),
            "{state}"
        );
        assert_eq!(todo["tabs"], json!(["project", "currentFile", "scopeBased"]), "no Git, no Current Changelist");

        let next = ask(&ctx, &mut app, "run_action", json!({ "action": "view.occurrence.next" })).unwrap();
        assert_eq!(next["window"]["todo"]["selected"]["line"], 3, "the item after the first: {next}");
        assert!(next["window"]["editor"].as_str().is_some_and(|path| path.ends_with("main.go")), "{next}");
        let previous = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⌘↑"] })).unwrap();
        assert_eq!(previous["window"]["todo"]["selected"]["line"], 2, "{previous}");

        let preview = ask(&ctx, &mut app, "run_action", json!({ "action": "view.todo.previewSource.toggle" })).unwrap();
        assert_eq!(preview["result"], json!({ "previewSource": true }));
        let single =
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.todo.navigateWithSingleClick.toggle" })).unwrap();
        assert_eq!(single["window"]["todo"]["navigateWithSingleClick"], true);
        let unknown = json!({ "action": "view.todo.filter", "params": { "name": "gone" } });
        assert!(ask(&ctx, &mut app, "run_action", unknown).unwrap_err().contains("gone"));
        let collapsed = ask(&ctx, &mut app, "run_action", json!({ "action": "view.todo.collapseAll" })).unwrap();
        assert_eq!(collapsed["window"]["todo"]["collapsed"].as_array().map(Vec::len), Some(1));
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.todo.expandAll" })).unwrap();
        let tab = json!({ "action": "view.todo.tab", "params": { "tab": "scopeBased", "scope": "named:Nope" } });
        let scoped = ask(&ctx, &mut app, "run_action", tab).unwrap();
        assert_eq!(
            (&scoped["window"]["todo"]["tab"], &scoped["window"]["todo"]["scope"]),
            (&json!("scopeBased"), &json!("named:Nope"))
        );
        let changelist = json!({ "action": "view.todo.tab", "params": { "tab": "currentChangelist" } });
        assert!(ask(&ctx, &mut app, "run_action", changelist).unwrap_err().contains("Git"));
        let items = ask(
            &ctx,
            &mut app,
            "dispatch_command",
            json!({ "command": "todo.items.list", "params": { "tab": "currentFile", "path": file } }),
        )
        .unwrap();
        assert_eq!(items["total"], 2, "{items}");
    }

    #[test]
    fn agents_move_tool_windows_and_restore_the_default_layout() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let params = json!({ "id": "problems", "side": "rightBottom" });
        let moved =
            ask(&ctx, &mut app, "dispatch_command", json!({ "command": "view.toolWindow.move", "params": params }))
                .unwrap();
        assert_eq!(moved["result"], json!({ "id": "problems", "side": "rightBottom" }), "{moved}");
        let shown = ask(&ctx, &mut app, "run_action", json!({ "action": "view.toolWindow.problems" })).unwrap();
        assert_eq!(shown["window"]["right"], "problems", "{shown}");
        assert_eq!(shown["window"]["bottom"], Value::Null);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["toolWindows"]["active"], "problems", "{state}");
        let restored = ask(&ctx, &mut app, "run_action", json!({ "action": "view.layout.restoreDefault" })).unwrap();
        assert_eq!(restored["window"]["right"], Value::Null, "{restored}");
        assert_eq!(restored["window"]["left"], "project");
        let refused = ask(&ctx, &mut app, "run_action", json!({ "action": "view.toolWindow.move" })).unwrap_err();
        assert!(refused.contains("side"), "{refused}");
    }

    #[test]
    fn power_save_mode_and_the_status_bar_widgets_are_switched_by_actions_and_kept_across_restarts() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        let slot = app.slot_for(None).unwrap();
        let session_on = || {
            let window = slot.window.lock().unwrap();
            zen_workbench::power_save::is_on(&window.session)
        };
        assert_eq!(ask(&ctx, &mut app, "get_settings", json!({})).unwrap()["powerSaveMode"], false, "off by default");
        let on = ask(&ctx, &mut app, "run_action", json!({ "action": "view.powerSaveMode.toggle" })).unwrap();
        assert_eq!(on["result"]["powerSaveMode"], true, "{on}");
        let kept = ask(
            &ctx,
            &mut app,
            "run_action",
            json!({ "action": "view.powerSaveMode.toggle", "params": { "on": true } }),
        )
        .unwrap();
        assert_eq!(kept["result"]["powerSaveMode"], true, "asking for the state it is in keeps it: {kept}");
        let widgets = ask(
            &ctx,
            &mut app,
            "run_action",
            json!({ "action": "view.statusBarWidgets.open", "params": { "widget": "memoryIndicator", "shown": true } }),
        )
        .unwrap();
        assert_eq!(widgets["result"]["widgets"]["memoryIndicator"], true, "{widgets}");
        let menu_item =
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.statusBarWidgets.open/lineSeparator" })).unwrap();
        assert_eq!(menu_item["result"]["widgets"]["lineSeparator"], false, "the menu's item toggles it: {menu_item}");
        let refused = ask(
            &ctx,
            &mut app,
            "run_action",
            json!({ "action": "view.statusBarWidgets.open", "params": { "widget": "nothing" } }),
        )
        .unwrap_err();
        assert!(refused.contains("nothing"), "{refused}");
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert!(session_on(), "the session pauses its background work");
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["statusBar"]["powerSaveMode"], true, "{state}");
        assert_eq!(state["statusBar"]["widgets"]["memoryIndicator"], true, "{state}");
        let status =
            ask(&ctx, &mut app, "dispatch_command", json!({ "command": "powerSave.status.get", "params": {} }))
                .unwrap();
        assert_eq!(status["on"], true, "{status}");
        assert_eq!(state["statusBar"]["widgets"]["powerSaveMode"], false, "off by default: {state}");
        let listed =
            ask(&ctx, &mut app, "dispatch_command", json!({ "command": "ui.notifications.list", "params": {} }))
                .unwrap();
        let notices: Vec<&Value> = listed["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|notice| notice["group"] == "powerSave")
            .collect();
        assert_eq!(notices.len(), 1, "one notice for turning it on: {listed}");
        assert_eq!(notices[0]["title"], "Power saving is on");
        let hidden = ask(
            &ctx,
            &mut app,
            "dispatch_command",
            json!({ "command": "ui.notifications.action", "params": { "id": notices[0]["id"], "action": "hide" } }),
        )
        .unwrap();
        assert_eq!(hidden["result"]["result"]["balloonGroupsOff"], json!(["powerSave"]), "{hidden}");
        let saved = std::fs::read_to_string(app.shared.settings_file.as_ref().unwrap()).unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!(
            (saved["powerSaveMode"].clone(), saved["statusBarWidgets"]["memoryIndicator"].clone()),
            (json!(true), json!(true))
        );
        drop(app);

        let mut app = themed_app(data.path(), vec![project]);
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        let settings = ask(&ctx, &mut app, "get_settings", json!({})).unwrap();
        assert_eq!(settings["powerSaveMode"], true, "kept across a restart");
        assert_eq!(settings["statusBarWidgets"]["memoryIndicator"], true);
        let off = ask(&ctx, &mut app, "run_action", json!({ "action": "view.powerSaveMode.toggle" })).unwrap();
        assert_eq!(off["result"]["powerSaveMode"], false, "{off}");
    }

    #[test]
    fn show_code_authors_switches_the_setting_from_the_action_its_shortcut_and_find_action() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        let slot = app.slot_for(None).unwrap();
        let authors =
            |app: &mut IdeApp| ask(&ctx, app, "get_settings", json!({})).unwrap()["codeVision"]["authors"].clone();
        let session_authors = || {
            let window = slot.window.lock().unwrap();
            zen_workbench::language::code_vision::Switches::from_settings(window.session.settings()).authors
        };
        assert_eq!(authors(&mut app), json!(false), "off by default");
        assert!(!session_authors());

        let shown = ask(&ctx, &mut app, "run_action", json!({ "action": "view.codeVision.authors.toggle" })).unwrap();
        assert_eq!(shown["result"]["codeVision"]["authors"], true, "{shown}");
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert_eq!(authors(&mut app), json!(true));
        assert!(session_authors(), "the session's Code Vision lists the authors");
        let saved = std::fs::read_to_string(app.shared.settings_file.as_ref().unwrap()).unwrap();
        assert!(serde_json::from_str::<Value>(&saved).unwrap()["codeVision"]["authors"] == true, "{saved}");

        let text = || {
            let window = slot.window.lock().unwrap();
            let workbench = window.session.state::<Workbench>().unwrap();
            workbench.active_document().unwrap().snapshot().to_string()
        };
        let before = text();
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌃⌥A"] })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert_eq!(authors(&mut app), json!(false), "⌃⌥A hides them again");
        assert_eq!(text(), before, "the shortcut types nothing");

        app.tool(&ctx, "set_settings", &json!({ "codeVision": { "enabled": false, "authors": false } })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.codeVision.authors.toggle" })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        let settings = ask(&ctx, &mut app, "get_settings", json!({})).unwrap();
        assert_eq!(
            settings["codeVision"],
            json!({ "enabled": true, "usages": true, "authors": true, "authorsDefaultV2": true }),
            "Code Vision on with them"
        );

        let found = ask(&ctx, &mut app, "find_action", json!({ "query": "show code authors", "limit": 5 })).unwrap();
        let entry = found["actions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["action"] == "view.codeVision.authors.toggle");
        let entry = entry.unwrap_or_else(|| panic!("{found}"));
        assert_eq!(entry["shortcut"], "⌃⌥A", "{entry}");
    }

    #[test]
    fn the_navigation_bar_browses_the_project_from_the_keyboard_and_over_mcp() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        std::fs::create_dir(project.join("sub")).unwrap();
        std::fs::write(project.join("sub").join("util.go"), "package sub\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        let names = |bar: &Value, key: &str| -> Vec<String> {
            bar[key].as_array().unwrap().iter().map(|item| item["name"].as_str().unwrap().to_owned()).collect()
        };
        let jumped = ask(&ctx, &mut app, "run_action", json!({ "action": "view.navigationBar.jump" })).unwrap();
        let bar = &jumped["result"];
        assert_eq!(names(bar, "segments"), ["project", "main.go"], "{jumped}");
        assert_eq!((bar["active"].clone(), bar["selected"].clone()), (json!(true), json!(1)), "the last segment");
        assert_eq!((bar["placement"].clone(), bar["floating"].clone()), (json!("statusBar"), json!(false)));
        assert_eq!(jumped["window"]["popups"], json!(["navigationBar"]), "{jumped}");

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["←", "↓"] })).unwrap();
        let state =
            |app: &mut IdeApp| ask(&ctx, app, "run_action", json!({ "action": "view.navigationBar.jump" })).unwrap();
        let opened = state(&mut app);
        assert_eq!(opened["result"]["popup"]["segment"], 0, "{opened}");
        assert_eq!(names(&opened["result"]["popup"], "items"), ["sub", "main.go"], "folders first");
        assert_eq!(opened["result"]["popup"]["selected"], 1, "the next segment is selected in the popup");
        assert_eq!(opened["window"]["popups"], json!(["navigationBar", "navigationBarPopup"]));

        ask(&ctx, &mut app, "press_keys", json!({ "text": "su" })).unwrap();
        let typed = state(&mut app);
        assert_eq!(typed["result"]["popup"]["filter"], "su", "{typed}");
        assert_eq!(names(&typed["result"]["popup"], "items"), ["sub"], "speed search keeps what matches");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        let drilled = state(&mut app);
        assert_eq!(names(&drilled["result"], "segments"), ["project", "sub"], "a folder is browsed into: {drilled}");
        assert_eq!(drilled["result"]["popup"]["segment"], 1);
        assert_eq!(names(&drilled["result"]["popup"], "items"), ["util.go"]);
        let slot = app.slot_for(None).unwrap();
        let text = || {
            let window = slot.window.lock().unwrap();
            window.session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned()
        };
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert!(text().ends_with("sub/util.go"), "a file opens on Enter: {:?}", text());
        let window = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(window["popups"], json!([]), "and the bar gives the keyboard back: {window}");

        let shortcut = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘↑"] })).unwrap();
        assert_eq!(shortcut["window"]["popups"], json!(["navigationBar"]), "⌘↑ jumps to the bar: {shortcut}");
        let escaped = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(escaped["window"]["popups"], json!([]), "Escape returns to the editor");

        let open = |app: &mut IdeApp, segment: usize| {
            ask(
                &ctx,
                app,
                "run_action",
                json!({ "action": "view.navigationBar.jump", "params": { "segment": segment, "open": true } }),
            )
        };
        let refused = open(&mut app, 2).unwrap_err();
        assert!(refused.contains("has no children"), "a file without members has no popup: {refused}");
        let filtered = ask(
            &ctx,
            &mut app,
            "run_action",
            json!({ "action": "view.navigationBar.jump", "params": { "segment": 0, "open": true, "filter": "main" } }),
        )
        .unwrap();
        assert_eq!(names(&filtered["result"]["popup"], "items"), ["main.go"], "{filtered}");
        let closed = ask(
            &ctx,
            &mut app,
            "run_action",
            json!({ "action": "view.navigationBar.jump", "params": { "close": true } }),
        )
        .unwrap();
        assert_eq!(closed["result"]["active"], false);
        assert_eq!(closed["window"]["popups"], json!([]));
    }

    #[test]
    fn the_navigation_bar_placement_and_members_are_saved_settings() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        let settings = |app: &mut IdeApp| ask(&ctx, app, "get_settings", json!({})).unwrap();
        let initial = settings(&mut app);
        assert_eq!(
            (initial["navigationBar"].clone(), initial["navigationBarMembers"].clone()),
            (json!("statusBar"), json!(true))
        );

        let top = ask(&ctx, &mut app, "run_action", json!({ "action": "view.navigationBar.top" })).unwrap();
        assert_eq!(top["result"]["navigationBar"]["placement"], "top", "{top}");
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert_eq!(settings(&mut app)["navigationBar"], "top");
        let saved = std::fs::read_to_string(app.shared.settings_file.as_ref().unwrap()).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&saved).unwrap()["navigationBar"], "top", "{saved}");
        let drawn = app.slot_for(None).unwrap().window.lock().unwrap().view.navigation_bar.drawn_rects();
        assert_eq!(drawn.len(), 2, "the bar under the toolbar has the project and the file");
        assert!(
            drawn.iter().all(|rect| rect.top() < 2.0 * crate::ide::metrics::TOOLBAR),
            "under the toolbar: {drawn:?}"
        );

        ask(&ctx, &mut app, "run_action", json!({ "action": "view.navigationBar.hide" })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert!(
            app.slot_for(None).unwrap().window.lock().unwrap().view.navigation_bar.drawn_rects().is_empty(),
            "Don't Show"
        );
        let floating = ask(&ctx, &mut app, "run_action", json!({ "action": "view.navigationBar.jump" })).unwrap();
        assert_eq!(floating["result"]["floating"], true, "shown over the editor while hidden: {floating}");
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert_eq!(app.slot_for(None).unwrap().window.lock().unwrap().view.navigation_bar.drawn_rects().len(), 2);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();

        let members =
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.navigationBar.members.toggle" })).unwrap();
        assert_eq!(members["result"]["navigationBar"]["showMembers"], false);
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.navigationBar.statusBar" })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        let after = settings(&mut app);
        assert_eq!(
            (after["navigationBar"].clone(), after["navigationBarMembers"].clone()),
            (json!("statusBar"), json!(false))
        );
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        let shown = window.session.settings().get::<bool>(zen_workbench::language::navigation_bar::SHOW_MEMBERS);
        assert_eq!(shown, Some(false), "commands see Show Members");
        assert!(window.view.navigation_bar_checked("view.navigationBar.statusBar"));
        assert!(!window.view.navigation_bar_checked("view.navigationBar.members.toggle"));
        drop(window);
        let found =
            ask(&ctx, &mut app, "find_action", json!({ "query": "jump to navigation bar", "limit": 5 })).unwrap();
        let entry =
            found["actions"].as_array().unwrap().iter().find(|entry| entry["action"] == "view.navigationBar.jump");
        assert_eq!(entry.unwrap_or_else(|| panic!("{found}"))["shortcut"], "⌘↑");
    }

    struct ShopOutliner;

    impl zen_syntax::Outliner for ShopOutliner {
        fn handles(&self, file_name: &str) -> bool {
            file_name == "cart.go"
        }

        fn outline(&self, text: &str) -> Vec<zen_syntax::OutlineItem> {
            let at = |needle: &str| text.find(needle).unwrap();
            let item = |name: &str, kind, start: &str, children| zen_syntax::OutlineItem {
                name: name.to_owned(),
                detail: None,
                kind,
                start: at(start),
                end: text.len() - 1,
                selection_start: at(name),
                children,
            };
            vec![item(
                "Cart",
                zen_syntax::SymbolKind::Struct,
                "type Cart",
                vec![
                    item("Add", zen_syntax::SymbolKind::Method, "func (c *Cart) Add", Vec::new()),
                    item("Total", zen_syntax::SymbolKind::Method, "func (c *Cart) Total", Vec::new()),
                ],
            )]
        }
    }

    #[test]
    fn the_navigation_bar_draws_in_every_placement_and_keeps_a_long_paths_end_in_view() {
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS");
        let (languages, scale): (&[&str], f32) =
            if snapshots.is_some() { (&["en", "ru"], 2.0) } else { (&["en"], 1.0) };
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("shop");
        let cart = project.join("internal").join("cart").join("cart.go");
        std::fs::create_dir_all(cart.parent().unwrap()).unwrap();
        std::fs::create_dir_all(project.join("empty")).unwrap();
        std::fs::write(project.join("go.mod"), "module shop\n").unwrap();
        std::fs::write(project.join("main.go"), "package main\n").unwrap();
        let source = "package cart\n\ntype Cart struct {\n\titems []string\n}\n\nfunc (c *Cart) Add(item string) {\n\tc.items = append(c.items, item)\n}\n\nfunc (c *Cart) Total() int {\n\treturn len(c.items)\n}\n";
        std::fs::write(&cart, source).unwrap();
        let deep = data
            .path()
            .join("toolchains/go/1.27.1/libexec/src/vendor/golang.org/x/text/unicode/norm/internal/tables/print.go");
        std::fs::create_dir_all(deep.parent().unwrap()).unwrap();
        std::fs::write(&deep, "package tables\n").unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), scale);
        let ctx = screen.context().clone();
        screen.step(Vec::new(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            let outliners = window.session.state_mut::<zen_workbench::language::structure::Outliners>();
            outliners.set(vec![Arc::new(ShopOutliner)]);
        }
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        let action = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params })).unwrap()
        };
        run(&mut app, "file.document.open", json!({ "path": cart }));
        let caret = source.find("append").unwrap();
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": caret, "head": caret }] }));
        let mut draw = |app: &mut IdeApp| {
            for _ in 0..2 {
                screen.render(|ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
            }
            screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            })
        };
        let drawn =
            |app: &mut IdeApp| app.slot_for(None).unwrap().window.lock().unwrap().view.navigation_bar.drawn_rects();
        for language in languages {
            if *language != "en" {
                ask(&ctx, &mut app, "set_settings", json!({ "language": language })).unwrap();
                for _ in 0..2 {
                    frame(&ctx, &mut app);
                }
            }
            let mut shots: Vec<(String, crate::offscreen::Frame)> = Vec::new();
            action(&mut app, "view.navigationBar.statusBar", json!({}));
            shots.push(("status-bar".to_owned(), draw(&mut app)));
            let bar = action(&mut app, "view.navigationBar.jump", json!({ "segment": 0, "open": true }));
            assert_eq!(bar["result"]["popup"]["items"][0]["name"], "empty", "{bar}");
            shots.push(("status-bar-folder-popup".to_owned(), draw(&mut app)));
            action(&mut app, "view.navigationBar.jump", json!({ "filter": "mn" }));
            shots.push(("status-bar-speed-search".to_owned(), draw(&mut app)));
            action(&mut app, "view.navigationBar.jump", json!({ "segment": 4, "open": true }));
            shots.push(("status-bar-members-popup".to_owned(), draw(&mut app)));
            action(&mut app, "view.navigationBar.jump", json!({ "close": true }));
            action(&mut app, "view.navigationBar.top", json!({}));
            shots.push(("top".to_owned(), draw(&mut app)));
            action(&mut app, "view.navigationBar.jump", json!({ "segment": 3, "open": true }));
            shots.push(("top-file-popup".to_owned(), draw(&mut app)));
            action(&mut app, "view.navigationBar.jump", json!({ "close": true }));
            action(&mut app, "view.navigationBar.hide", json!({}));
            action(&mut app, "view.navigationBar.jump", json!({}));
            shots.push(("floating".to_owned(), draw(&mut app)));
            action(&mut app, "view.navigationBar.jump", json!({ "segment": 0, "open": true, "filter": "empty" }));
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
            shots.push(("floating-empty-folder".to_owned(), draw(&mut app)));
            action(&mut app, "view.navigationBar.jump", json!({ "close": true }));
            action(&mut app, "view.navigationBar.statusBar", json!({}));
            run(&mut app, "file.document.open", json!({ "path": deep }));
            let image = draw(&mut app);
            let rects = drawn(&mut app);
            let last = *rects.last().unwrap();
            assert!(rects[0] == egui::Rect::NOTHING, "the leading segments give way: {rects:?}");
            assert!(last.is_positive() && last.right() <= 1100.0, "the file stays in view: {rects:?}");
            shots.push(("status-bar-long-path".to_owned(), image));
            run(&mut app, "file.document.open", json!({ "path": cart }));
            if let Some(dir) = &snapshots {
                for (name, image) in shots {
                    let path = Path::new(dir).join(format!("navigation-bar-{name}-{language}.png"));
                    crate::offscreen::write_png(&path, &image).unwrap();
                }
            }
        }
    }

    #[test]
    fn appearance_modes_hide_the_window_and_bring_it_back() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.toolWindow.problems" })).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let slot = app.slot_for(None).unwrap();
        let run = |app: &mut IdeApp, action: &str| ask(&ctx, app, "run_action", json!({ "action": action })).unwrap();
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        let editor = |read: &dyn Fn(&IdeView) -> Value| read(&slot.window.lock().unwrap().view);
        let before = state(&mut app);
        assert_eq!((before["left"].clone(), before["bottom"].clone()), (json!("project"), json!("problems")));
        let zoom = ctx.zoom_factor();
        assert_eq!((before["appearance"].clone(), before["fullScreen"].clone()), (Value::Null, json!(false)));

        let entered = run(&mut app, "view.presentationMode.toggle");
        let window = &entered["window"];
        assert_eq!(window["appearance"], "presentation", "{entered}");
        assert_eq!(window["fullScreen"], true, "Presentation Mode goes full screen");
        assert_eq!((window["left"].clone(), window["bottom"].clone()), (Value::Null, Value::Null), "{window}");
        frame(&ctx, &mut app);
        assert_eq!(editor(&|view| json!(view.editor_view.font.size)), json!(24.0), "the font of Presentation Mode");
        assert_eq!(ctx.zoom_factor(), zoom, "the rest of the interface keeps its size");
        assert_eq!(
            editor(&|view| json!(view.overview.settings.editor_font.size)),
            json!(13.0),
            "the editor's own font size does not change"
        );
        let layout = {
            let mut window = slot.window.lock().unwrap();
            let window = &mut *window;
            workspace::capture(&window.view, &mut window.session, &project)
        };
        assert!(layout.project_panel, "the saved layout keeps the tool windows the mode hid");
        let title = |action: &str| editor(&|view| json!(view.appearance_title(action)));
        assert_eq!(title("view.presentationMode.toggle"), json!(zen_i18n::t!("menu-view-exit-presentationMode")));
        assert_eq!(title("view.zenMode.toggle"), json!(zen_i18n::t!("menu-view-enter-zenMode")));
        assert_eq!(title("view.fullScreen.toggle"), json!(zen_i18n::t!("menu-view-exit-fullScreen")));

        let settings = run(&mut app, "view.settings.open");
        assert_eq!(settings["window"]["popups"].as_array().map(Vec::len), Some(1), "{settings}");
        frame(&ctx, &mut app);
        let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(closed["window"]["popups"], json!([]), "{closed}");
        assert_eq!(closed["window"]["appearance"], "presentation", "Escape closed the dialog only");
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        run(&mut app, "editor.caret.cloneBelow");
        frame(&ctx, &mut app);
        let carets = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(carets["window"]["appearance"], "presentation", "Escape removed the other carets only: {carets}");
        let left = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        let window = &left["window"];
        assert_eq!(window["appearance"], Value::Null, "Escape left the mode: {left}");
        assert_eq!((window["left"].clone(), window["bottom"].clone()), (json!("project"), json!("problems")));
        assert_eq!(window["fullScreen"], false, "full screen ends with the mode that began it");
        frame(&ctx, &mut app);
        assert_eq!(editor(&|view| json!(view.editor_view.font.size)), json!(13.0));

        let free = run(&mut app, "view.distractionFreeMode.toggle");
        assert_eq!(free["window"]["appearance"], "distractionFree", "{free}");
        assert_eq!(free["window"]["fullScreen"], false, "Distraction Free Mode stays in the window");
        assert_eq!(free["window"]["left"], Value::Null);
        frame(&ctx, &mut app);
        assert_eq!(editor(&|view| json!(view.editor_view.hide_line_numbers)), json!(true));
        let zen = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "view.zenMode.toggle" })).unwrap();
        assert_eq!(zen["window"]["appearance"], "zen", "Zen Mode replaces Distraction Free Mode: {zen}");
        assert_eq!(zen["window"]["fullScreen"], true);
        let back = run(&mut app, "view.zenMode.toggle");
        assert_eq!(back["window"]["appearance"], Value::Null, "{back}");
        assert_eq!(back["window"]["fullScreen"], false);
        assert_eq!(back["window"]["bottom"], "problems", "the tool windows are back as they were");
        frame(&ctx, &mut app);
        assert_eq!(editor(&|view| json!(view.editor_view.hide_line_numbers)), json!(false));

        let full = run(&mut app, "view.fullScreen.toggle");
        assert_eq!(
            (full["window"]["fullScreen"].clone(), full["window"]["appearance"].clone()),
            (json!(true), Value::Null)
        );
        let presentation = run(&mut app, "view.presentationMode.toggle");
        assert_eq!(presentation["window"]["fullScreen"], true);
        let left = run(&mut app, "view.presentationMode.toggle");
        assert_eq!(left["window"]["fullScreen"], true, "a window full screen before the mode stays so");
        let windowed = run(&mut app, "view.fullScreen.toggle");
        assert_eq!(windowed["window"]["fullScreen"], false);

        app.tool(&ctx, "set_settings", &json!({ "presentationFontSize": 30 })).unwrap();
        run(&mut app, "view.presentationMode.toggle");
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert_eq!(editor(&|view| json!(view.editor_view.font.size)), json!(30.0), "the size from Settings");
    }

    fn frame_full_screen(ctx: &egui::Context, app: &mut IdeApp, full_screen: bool) {
        let time = ctx.input(|input| input.time).max(10.0) + 0.1;
        let mut input = egui::RawInput { time: Some(time), ..egui::RawInput::default() };
        input
            .viewports
            .insert(ViewportId::ROOT, egui::ViewportInfo { fullscreen: Some(full_screen), ..Default::default() });
        let mut output = ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn a_mode_on_when_the_workspace_is_saved_leaves_the_normal_layout_for_the_next_start() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        for _ in 0..2 {
            frame_full_screen(&ctx, &mut app, false);
        }
        assert!(!app.splash.is_active());
        app.remember_workspace(&ctx, true);
        assert!(!app.workspace.window.fullscreen);
        assert_eq!(ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["left"], "project");

        let entered = ask(&ctx, &mut app, "run_action", json!({ "action": "view.presentationMode.toggle" })).unwrap();
        assert_eq!(entered["window"]["left"], Value::Null, "{entered}");
        for _ in 0..3 {
            frame_full_screen(&ctx, &mut app, true);
        }
        app.remember_workspace(&ctx, true);
        assert!(!app.workspace.window.fullscreen, "the full screen of the mode is not the window's own");
        let layout = app.workspace.projects.values().next().unwrap();
        assert!(layout.project_panel, "the tool windows the mode hid are saved shown");
        drop(app);

        let mut app = themed_app(data.path(), vec![project.clone()]);
        assert!(!app.workspace.window.fullscreen, "the next start opens a window, not full screen");
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(
            (state["appearance"].clone(), state["fullScreen"].clone(), state["left"].clone()),
            (Value::Null, json!(false), json!("project")),
            "{state}"
        );
    }

    #[test]
    fn full_screen_turned_off_by_the_system_during_a_mode_is_not_turned_on_again() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame_full_screen(&ctx, &mut app, false);
        let run = |app: &mut IdeApp, action: &str| ask(&ctx, app, "run_action", json!({ "action": action })).unwrap();
        assert_eq!(run(&mut app, "view.presentationMode.toggle")["window"]["fullScreen"], true);
        frame_full_screen(&ctx, &mut app, true);
        let state = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap();
        assert_eq!(state(&mut app)["fullScreen"], true, "the window followed the mode");
        frame_full_screen(&ctx, &mut app, false);
        frame_full_screen(&ctx, &mut app, true);
        assert_eq!(state(&mut app)["fullScreen"], true, "the system's Enter Full Screen, chosen by the user");
        let slot = app.slot_for(None).unwrap();
        assert!(!slot.window.lock().unwrap().view.appearance.full_screen_by_mode(), "full screen is the user's now");
        let left = run(&mut app, "view.presentationMode.toggle");
        assert_eq!(left["window"]["fullScreen"], true, "leaving the mode keeps the user's full screen: {left}");
    }

    #[test]
    fn search_in_selection_from_the_editor_gives_the_search_field_the_keyboard() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let open = json!({ "command": "file.document.open", "params": { "path": file } });
        ask(&ctx, &mut app, "dispatch_command", open).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let selection = json!({ "selections": [{
            "anchor": { "line": 2, "column": 0 },
            "head": { "line": 4, "column": 1 },
        }] });
        let select = json!({ "command": "editor.selection.set", "params": selection });
        ask(&ctx, &mut app, "dispatch_command", select).unwrap();
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌃⌥E"] })).unwrap();
        ask(&ctx, &mut app, "press_keys", json!({ "text": "println" })).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let info = json!({ "command": "editor.find.info", "params": {} });
        let info = ask(&ctx, &mut app, "dispatch_command", info).unwrap();
        assert_eq!(info["state"]["query"]["text"], "println", "{info}");
        assert_eq!((info["inSelection"].as_bool(), info["count"].as_u64()), (Some(true), Some(1)));
        let text = std::fs::read_to_string(&file).unwrap();
        let shown = json!({ "path": file });
        let shown = ask(&ctx, &mut app, "get_document_text", shown).unwrap();
        assert_eq!(shown["text"], text, "the typing went to the search field, not the file");
    }

    #[test]
    fn escape_in_a_mode_closes_the_find_bar_and_the_branches_popup_first() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        let run = |app: &mut IdeApp, action: &str| ask(&ctx, app, "run_action", json!({ "action": action })).unwrap();
        let escape = |app: &mut IdeApp| ask(&ctx, app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        run(&mut app, "view.distractionFreeMode.toggle");
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }

        let branches = run(&mut app, "view.vcs.branches");
        assert_eq!(branches["window"]["popups"], json!(["branches"]), "{branches}");
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let slot = app.slot_for(None).unwrap();
        assert!(slot.window.lock().unwrap().view.vcs.popup.open, "the popup shows with the toolbar hidden");
        let closed = escape(&mut app);
        assert_eq!(closed["window"]["popups"], json!([]), "{closed}");
        assert_eq!(closed["window"]["appearance"], "distractionFree", "Escape closed the popup only");

        run(&mut app, "view.find.open");
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let find = escape(&mut app);
        assert_eq!(find["window"]["appearance"], "distractionFree", "Escape closed the search bar only: {find}");
        let open = |app: &IdeApp| {
            app.slot_for(None).unwrap().window.lock().unwrap().session.state::<Workbench>().unwrap().find().open
        };
        assert!(!open(&app));
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let left = escape(&mut app);
        assert_eq!(left["window"]["appearance"], Value::Null, "{left}");
    }

    #[test]
    fn closing_a_dialog_gives_the_keyboard_back_to_the_editor() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let slot = app.slot_for(None).unwrap();
        let editor_has_keyboard = || slot.window.lock().unwrap().view.editor_view.has_focus_in(&ctx);
        let selected = || {
            let window = slot.window.lock().unwrap();
            let workbench = window.session.state::<Workbench>().unwrap();
            workbench.active_document().unwrap().selections().primary().range()
        };
        assert!(editor_has_keyboard(), "an opened file takes the keyboard");

        for (action, keys) in [
            ("view.settings.open", json!(["⎋"])),
            ("view.gotoLine.open", json!(["⎋"])),
            ("view.searchEverywhere.open", json!(["⎋"])),
        ] {
            let opened = ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
            assert_ne!(opened["window"]["popups"], json!([]), "{action}: {opened}");
            frame(&ctx, &mut app);
            let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": keys })).unwrap();
            assert_eq!(closed["window"]["popups"], json!([]), "{action}: {closed}");
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            assert!(editor_has_keyboard(), "{action}: closing it gives the keyboard back to the editor");
        }

        assert!(selected().is_empty());
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌃G"] })).unwrap();
        assert_eq!(selected(), 0..7, "⌃G reached the editor");
    }

    const OTHER: &str = "package main\n\ntype OtherStore struct{}\n\nfunc otherWork() {}\n";

    fn enter_app(data: &Path) -> (IdeApp, egui::Context, PathBuf, zen_lsp::fake::FakeServer) {
        let (project, file) = window_project(data);
        let other = project.join("other.go");
        std::fs::write(&other, OTHER).unwrap();
        let mut app = themed_app(data, vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let server = zen_lsp::fake::FakeServer::new();
        server.set_capabilities(json!({
            "workspaceSymbolProvider": true,
            "referencesProvider": true,
            "renameProvider": { "prepareProvider": true }
        }));
        let uri = zen_lsp::position::file_uri(&other);
        let symbol = |name: &str, kind: u64, line: u32, character: u32| {
            json!({ "name": name, "kind": kind, "containerName": "main",
                "location": { "uri": uri, "range": {
                    "start": { "line": line, "character": character },
                    "end": { "line": line, "character": character + 5 } } } })
        };
        let answer = json!([symbol("OtherStore", 23, 2, 5), symbol("otherWork", 12, 4, 5)]);
        server.on_with("workspace/symbol", move |_| answer.clone());
        let main = json!({ "start": { "line": 2, "character": 5 }, "end": { "line": 2, "character": 9 } });
        server.on("textDocument/prepareRename", json!({ "range": main, "placeholder": "main" }));
        let launched = server.clone();
        app.slot_for(None)
            .unwrap()
            .window
            .lock()
            .unwrap()
            .session
            .state_mut::<zen_workbench::language::Languages>()
            .set_specs(vec![zen_workbench::language::ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch: Arc::new(move |_| Ok(launched.launcher())),
                initialization_options: json!({}),
            }]);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        (app, ctx, project, server)
    }

    fn settle(ctx: &egui::Context, app: &mut IdeApp, command: &str, params: Value) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let found = ask(ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap();
            if found["pending"] != true && found["indexing"] != true && found["done"] != false {
                break;
            }
            assert!(Instant::now() < deadline, "{command} still looking: {found}");
            std::thread::sleep(Duration::from_millis(10));
        }
        frame(ctx, app);
    }

    fn untouched(app: &IdeApp, what: &str) {
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        for document in window.session.state::<Workbench>().unwrap().documents() {
            let disk = std::fs::read_to_string(document.path()).unwrap();
            assert_eq!(document.snapshot().to_string(), disk, "{what}: {} changed", document.path().display());
            assert!(!document.is_dirty(), "{what}: {} is marked modified", document.path().display());
        }
    }

    fn active_text(app: &IdeApp) -> String {
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        window.session.state::<Workbench>().and_then(Workbench::active_document).unwrap().text().to_owned()
    }

    fn active_name(app: &IdeApp) -> String {
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        let workbench = window.session.state::<Workbench>().unwrap();
        let path = workbench.active_document().map(|document| document.path().to_owned());
        path.and_then(|path| path.file_name().map(|name| name.to_string_lossy().into_owned())).unwrap_or_default()
    }

    fn show_file(ctx: &egui::Context, app: &mut IdeApp, project: &Path, name: &str) {
        let params = json!({ "command": "file.document.open", "params": { "path": project.join(name) } });
        ask(ctx, app, "dispatch_command", params).unwrap();
        for _ in 0..3 {
            frame(ctx, app);
        }
    }

    fn press_return(ctx: &egui::Context, app: &mut IdeApp, keyboard: bool, what: &str) -> Value {
        if keyboard {
            for pressed in [true, false] {
                let key = egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: Some(egui::Key::Enter),
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                };
                let mut output = ctx.run_ui(egui::RawInput { events: vec![key], ..Default::default() }, |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
                output.textures_delta.clear();
            }
        } else {
            ask(ctx, app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        }
        for _ in 0..3 {
            frame(ctx, app);
        }
        untouched(app, &format!("{what}, keyboard {keyboard}"));
        ask(ctx, app, "get_window_state", json!({})).unwrap()["popups"].clone()
    }

    #[test]
    fn enter_in_search_everywhere_opens_the_item_without_typing_into_the_file() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, _server) = enter_app(data.path());
        for keyboard in [true, false] {
            for (keys, query, wait, opened) in [
                ("⇧⌘O", "other", "navigate.file.goTo", "other.go"),
                ("⌘O", "OtherStore", "navigate.class.goTo", "other.go"),
                ("⌥⌘O", "otherWork", "navigate.symbol.goTo", "other.go"),
                ("⇧⇧", "other.go", "navigate.file.goTo", "other.go"),
                ("⇧⌘A", "problems", "", "main.go"),
            ] {
                show_file(&ctx, &mut app, &project, "main.go");
                let shown = if keys == "⇧⇧" {
                    ask(&ctx, &mut app, "run_action", json!({ "action": "view.searchEverywhere.open" })).unwrap()
                } else {
                    ask(&ctx, &mut app, "press_keys", json!({ "keys": [keys] })).unwrap()
                };
                assert_ne!(shown["window"]["popups"], json!([]), "{keys}: {shown}");
                frame(&ctx, &mut app);
                ask(&ctx, &mut app, "press_keys", json!({ "text": query })).unwrap();
                if !wait.is_empty() {
                    settle(&ctx, &mut app, wait, json!({ "query": query, "limit": 10 }));
                }
                let mut popups = press_return(&ctx, &mut app, keyboard, keys);
                let deadline = Instant::now() + Duration::from_secs(60);
                while popups != json!([]) && keys == "⇧⇧" && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(20));
                    frame(&ctx, &mut app);
                    popups = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
                }
                untouched(&app, &format!("{keys}, keyboard {keyboard}"));
                assert_eq!(popups, json!([]), "{keyboard}");
                assert_eq!(active_name(&app), opened, "{keys} {query}, keyboard {keyboard}");
            }
        }
    }

    #[test]
    fn search_everywhere_marks_matches_opens_places_and_folders_and_lists_more_in_a_window() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let _english = snapshots.is_some().then_some(English);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let main: String = (1..=60).map(|line| format!("\tprintln({line}) // line {line}\n")).collect();
        std::fs::write(&file, format!("package main\n\nfunc main() {{\n{main}}}\n")).unwrap();
        std::fs::write(project.join("other.go"), OTHER).unwrap();
        std::fs::create_dir_all(project.join("internal/cart")).unwrap();
        std::fs::write(project.join("internal/cart/cart.go"), "package cart\n").unwrap();
        for index in 0..10 {
            std::fs::write(project.join(format!("handler{index}.go")), "package main\n").unwrap();
        }
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1200.0, 760.0), 2.0);
        let ctx = screen.context().clone();
        screen.step(Vec::new(), |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        let server = zen_lsp::fake::FakeServer::new();
        server.set_capabilities(json!({ "workspaceSymbolProvider": true }));
        let uri = zen_lsp::position::file_uri(&project.join("other.go"));
        let symbol = |name: &str, kind: u64, line: u32| {
            json!({ "name": name, "kind": kind, "containerName": "main",
                "location": { "uri": uri, "range": {
                    "start": { "line": line, "character": 5 }, "end": { "line": line, "character": 10 } } } })
        };
        let answer = json!([symbol("OtherStore", 23, 2), symbol("otherWork", 12, 4)]);
        server.on_with("workspace/symbol", move |_| answer.clone());
        let launched = server.clone();
        app.slot_for(None)
            .unwrap()
            .window
            .lock()
            .unwrap()
            .session
            .state_mut::<zen_workbench::language::Languages>()
            .set_specs(vec![zen_workbench::language::ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch: Arc::new(move |_| Ok(launched.launcher())),
                initialization_options: json!({}),
            }]);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        settle(&ctx, &mut app, "navigate.file.goTo", json!({ "query": "handler", "limit": 20 }));
        settle(&ctx, &mut app, "navigate.class.goTo", json!({ "query": "OtherStore", "limit": 10 }));

        let step = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, events: Vec<egui::Event>| {
            screen.step(events, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        };
        type Rows = [(String, bool)];
        let rows = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp| -> Vec<(String, bool)> {
            let tree = screen.accessibility_tree(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            let mut found: Vec<(f64, String, bool)> = tree
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == egui::accesskit::Role::ListBoxOption)
                .map(|(_, node)| {
                    let name = crate::accessibility::accessible_name(node, &tree.nodes).unwrap_or_default();
                    (node.bounds().map_or(0.0, |bounds| bounds.y0), name, node.is_selected() == Some(true))
                })
                .collect();
            found.sort_by(|a, b| a.0.total_cmp(&b.0));
            found.into_iter().map(|(_, name, selected)| (name, selected)).collect()
        };
        let wait = |screen: &mut crate::offscreen::Offscreen,
                    app: &mut IdeApp,
                    what: &str,
                    done: &dyn Fn(&Rows) -> bool|
         -> Vec<(String, bool)> {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                let found = rows(screen, app);
                if done(&found) {
                    return found;
                }
                assert!(Instant::now() < deadline, "{what}: {found:?}");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let selected = |found: &[(String, bool)]| found.iter().find(|(_, on)| *on).map(|(name, _)| name.clone());
        let keys = |key: egui::Key, modifiers: egui::Modifiers| -> Vec<egui::Event> {
            [true, false]
                .into_iter()
                .map(|pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers })
                .collect()
        };
        let shot = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str, language: &str| {
            let Some(dir) = &snapshots else { return };
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            crate::offscreen::write_png(&dir.join(format!("se-{name}-{language}.png")), &image).unwrap();
        };
        let open = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, keys: &str, text: &str| {
            for _ in 0..3 {
                if ask(&ctx, app, "get_window_state", json!({})).unwrap()["popups"] == json!([]) {
                    break;
                }
                ask(&ctx, app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            }
            let shown = if keys.is_empty() {
                ask(&ctx, app, "run_action", json!({ "action": "view.searchEverywhere.open" })).unwrap()
            } else {
                ask(&ctx, app, "press_keys", json!({ "keys": [keys] })).unwrap()
            };
            assert_ne!(shown["window"]["popups"], json!([]), "{keys}: {shown}");
            for _ in 0..3 {
                screen.step(Vec::new(), |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
            }
            if !text.is_empty() {
                ask(&ctx, app, "press_keys", json!({ "text": text })).unwrap();
            }
        };

        for language in languages {
            zen_i18n::set_language(Some(language));
            let more = zen_i18n::t!("ide-se-more");
            open(&mut screen, &mut app, "", "handler");
            let first =
                wait(&mut screen, &mut app, "more… on All", &|found| found.iter().any(|(name, _)| *name == more));
            let files: Vec<&String> =
                first.iter().map(|(name, _)| name).filter(|name| name.starts_with("handler")).collect();
            assert_eq!(files.len(), zen_workbench::search_everywhere::ALL_LIMIT, "{first:?}");
            assert_eq!(first.last().map(|(name, _)| name), Some(&more), "more… ends the group: {first:?}");
            shot(&mut screen, &mut app, "all-more", language);
            step(&mut screen, &mut app, keys(egui::Key::ArrowDown, egui::Modifiers::COMMAND));
            assert_eq!(selected(&rows(&mut screen, &mut app)), Some(more.clone()), "⌘↓ goes to the end");
            shot(&mut screen, &mut app, "cmd-down", language);
            step(&mut screen, &mut app, keys(egui::Key::Enter, egui::Modifiers::NONE));
            let all = wait(&mut screen, &mut app, "the rest of the group", &|found| {
                found.iter().filter(|(name, _)| name.starts_with("handler")).count() == 10
            });
            assert!(!all.iter().any(|(name, _)| *name == more), "{all:?}");
            let at = selected(&all).unwrap();
            assert!(at.starts_with("handler") && !files.iter().any(|name| **name == at), "the first loaded: {all:?}");
            assert_eq!(
                ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"],
                json!(["searchEverywhere:all"]),
                "the popup stays open"
            );
            shot(&mut screen, &mut app, "more-expanded", language);
            step(&mut screen, &mut app, keys(egui::Key::ArrowUp, egui::Modifiers::COMMAND));
            let top = rows(&mut screen, &mut app);
            assert_eq!(selected(&top), top.first().map(|(name, _)| name.clone()), "⌘↑ goes to the start");
            shot(&mut screen, &mut app, "cmd-up", language);

            for (tab, keys_of, query, expected) in [
                ("all", "", "other", "OtherStore"),
                ("classes", "⌘O", "OS", "OtherStore"),
                ("files", "⇧⌘O", "oth", "other.go"),
                ("symbols", "⌥⌘O", "oW", "otherWork"),
                ("actions", "⇧⌘A", "dup line", ""),
                ("actions-ru", "⇧⌘A", "дубл", ""),
                ("text", "", "line 42", "line 42"),
            ] {
                if tab == "actions-ru" && *language != "ru" {
                    continue;
                }
                if tab == "text" {
                    open(&mut screen, &mut app, "⇧⌘O", "");
                    for _ in 0..3 {
                        step(&mut screen, &mut app, keys(egui::Key::Tab, egui::Modifiers::NONE));
                    }
                    ask(&ctx, &mut app, "press_keys", json!({ "text": query })).unwrap();
                } else {
                    open(&mut screen, &mut app, keys_of, query);
                }
                let expected = if expected.is_empty() {
                    zen_i18n::t!("keymap-editor-line-duplicate")
                } else {
                    expected.to_owned()
                };
                wait(&mut screen, &mut app, tab, &|found| found.iter().any(|(name, _)| name.contains(&expected)));
                shot(&mut screen, &mut app, &format!("tab-{tab}"), language);
            }

            open(&mut screen, &mut app, "⇧⌘O", "intern");
            wait(&mut screen, &mut app, "the folder", &|found| found.iter().any(|(name, _)| name == "internal"));
            shot(&mut screen, &mut app, "files-folder", language);
            step(&mut screen, &mut app, keys(egui::Key::Enter, egui::Modifiers::NONE));
            for _ in 0..3 {
                step(&mut screen, &mut app, Vec::new());
            }
            let tree = ask(&ctx, &mut app, "get_tree_selection", json!({})).unwrap();
            let paths: Vec<String> =
                tree["paths"].as_array().unwrap().iter().map(|path| path.as_str().unwrap().to_owned()).collect();
            assert!(paths.iter().any(|path| path.ends_with("internal")), "{tree}");
            shot(&mut screen, &mut app, "folder-in-tree", language);

            for (query, line, column) in [("main.go:42:7", 41, 6), ("main.go:42", 41, 0)] {
                ask(
                    &ctx,
                    &mut app,
                    "dispatch_command",
                    json!({ "command": "file.document.open", "params": { "path": project.join("other.go") } }),
                )
                .unwrap();
                open(&mut screen, &mut app, "⇧⌘O", query);
                wait(&mut screen, &mut app, query, &|found| found.iter().any(|(name, _)| name == "main.go"));
                shot(&mut screen, &mut app, &format!("line-{}", query.replace(':', "-")), language);
                step(&mut screen, &mut app, keys(egui::Key::Enter, egui::Modifiers::NONE));
                for _ in 0..3 {
                    step(&mut screen, &mut app, Vec::new());
                }
                assert_eq!(active_name(&app), "main.go", "{query}");
                let at = {
                    let slot = app.slot_for(None).unwrap();
                    let window = slot.window.lock().unwrap();
                    let workbench = window.session.state::<Workbench>().unwrap();
                    let document = workbench.active_document().unwrap();
                    document.snapshot().offset_to_point(document.selections().primary().head).unwrap()
                };
                assert_eq!((at.line, at.column), (line, column), "{query}");
                shot(&mut screen, &mut app, &format!("opened-{}", query.replace(':', "-")), language);
            }
        }
    }

    fn type_into(ctx: &egui::Context, app: &mut IdeApp, keyboard: bool, text: &str, popup: &str) {
        if keyboard {
            let typed = egui::Event::Text(text.to_owned());
            let mut output = ctx.run_ui(egui::RawInput { events: vec![typed], ..Default::default() }, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
        } else {
            ask(ctx, app, "press_keys", json!({ "text": text })).unwrap();
        }
        for _ in 0..3 {
            frame(ctx, app);
        }
        let what = format!("{popup}, keyboard {keyboard}");
        untouched(app, &what);
        let popups = ask(ctx, app, "get_window_state", json!({})).unwrap()["popups"].clone();
        assert_eq!(popups, json!([popup]), "{what}: typing keeps the popup open");
    }

    #[test]
    fn control_twice_opens_run_anything_unless_double_modifier_shortcuts_are_off() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, _project, _server) = enter_app(data.path());
        let ctrl = egui::Modifiers { ctrl: true, ..egui::Modifiers::NONE };
        let mut time = 100.0;
        let mut step = |app: &mut IdeApp, pressed: bool| {
            time += 0.05;
            let modifiers = if pressed { ctrl } else { egui::Modifiers::NONE };
            let key = egui::Key::ControlLeft;
            let event = egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers };
            let events = vec![egui::Event::ModifiersChanged(modifiers), event];
            let input = egui::RawInput { time: Some(time), events, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
        };
        let twice = |app: &mut IdeApp, step: &mut dyn FnMut(&mut IdeApp, bool)| {
            for pressed in [true, false, true, false] {
                step(app, pressed);
            }
            step(app, false);
        };
        twice(&mut app, &mut step);
        let shown = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
        assert_eq!(shown, json!(["runAnything"]));
        escape_all(&ctx, &mut app);
        let mut settings = app.shared.settings.lock().unwrap().clone();
        settings.disable_double_modifiers = true;
        app.apply_settings(&ctx, settings);
        twice(&mut app, &mut step);
        let shown = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
        assert_eq!(shown, json!([]), "Advanced Settings turned the double modifiers off");
    }

    #[test]
    fn typing_in_the_popups_that_filter_never_types_into_the_file() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, server) = enter_app(data.path());
        let main = zen_lsp::position::file_uri(&project.join("main.go").canonicalize().unwrap());
        let place = |line: u32| {
            json!({ "uri": main, "range": {
                "start": { "line": line, "character": 5 }, "end": { "line": line, "character": 9 } } })
        };
        server.on("textDocument/references", json!([place(2), place(3)]));
        let main_at = "package main\n\nfunc ".len();
        let popups: [(&str, Value, &str); 14] = [
            ("press_keys", json!({ "keys": ["⌘E"] }), "recentFiles"),
            ("press_keys", json!({ "keys": ["⇧⌘E"] }), "recentLocations"),
            ("press_keys", json!({ "keys": ["⌘F12"] }), "fileStructure"),
            ("run_action", json!({ "action": "view.searchEverywhere.open" }), "searchEverywhere:all"),
            ("press_keys", json!({ "keys": ["⌘O"] }), "searchEverywhere:classes"),
            ("press_keys", json!({ "keys": ["⇧⌘O"] }), "searchEverywhere:files"),
            ("press_keys", json!({ "keys": ["⌥⌘O"] }), "searchEverywhere:symbols"),
            ("press_keys", json!({ "keys": ["⇧⌘A"] }), "searchEverywhere:actions"),
            ("press_keys", json!({ "keys": ["⇧⌘V"] }), "pasteFromHistory"),
            ("run_action", json!({ "action": "view.projects.popup" }), "projects"),
            ("run_action", json!({ "action": "view.runAnything.open" }), "runAnything"),
            ("press_keys", json!({ "keys": ["⌃⌥R"] }), "chooseRunConfiguration"),
            ("press_keys", json!({ "keys": ["⌥⌘T"] }), "surround"),
            (
                "dispatch_command",
                json!({ "command": "navigate.usages.show",
                    "params": { "path": project.join("main.go"), "offset": main_at } }),
                "showUsages",
            ),
        ];
        for keyboard in [true, false] {
            for (tool, params, popup) in &popups {
                show_file(&ctx, &mut app, &project, "other.go");
                show_file(&ctx, &mut app, &project, "main.go");
                ask(&ctx, &mut app, tool, params.clone()).unwrap();
                let deadline = Instant::now() + Duration::from_secs(60);
                loop {
                    let shown = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
                    if shown == json!([popup]) {
                        break;
                    }
                    assert!(Instant::now() < deadline, "{popup} did not open: {shown}");
                    std::thread::sleep(Duration::from_millis(10));
                    frame(&ctx, &mut app);
                }
                frame(&ctx, &mut app);
                type_into(&ctx, &mut app, keyboard, "mgo", popup);
                type_into(&ctx, &mut app, keyboard, "x", popup);
                for keys in ["⌫", "⌦", "⌥⌫", "⌘⌫", "⌥⌦"] {
                    ask(&ctx, &mut app, "press_keys", json!({ "keys": [keys] })).unwrap();
                    frame(&ctx, &mut app);
                    untouched(&app, &format!("{popup} after {keys}, keyboard {keyboard}"));
                }
                for _ in 0..3 {
                    let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
                    frame(&ctx, &mut app);
                    if closed["window"]["popups"] == json!([]) {
                        break;
                    }
                }
                untouched(&app, &format!("{popup} closed, keyboard {keyboard}"));
            }
        }
    }

    fn typed(app: &IdeApp, popup: &str) -> String {
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        let view = &window.view;
        match popup.split(':').next().unwrap() {
            "findInFiles" | "replaceInFiles" => view.find_in_files.typed(),
            "searchEverywhere" => view.search_everywhere.typed(),
            "recentFiles" => view.recent_files.typed(),
            "recentLocations" => view.recent_locations.typed(),
            "fileStructure" => view.file_structure.typed(),
            "pasteFromHistory" => view.paste_history.typed(),
            "runAnything" | "chooseRunConfiguration" => view.run_anything.typed(),
            "projects" => view.projects.filter(),
            other => panic!("no field known for {other}"),
        }
        .to_owned()
    }

    fn escape_all(ctx: &egui::Context, app: &mut IdeApp) {
        for _ in 0..3 {
            let closed = ask(ctx, app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            frame(ctx, app);
            if closed["window"]["popups"] == json!([]) {
                return;
            }
        }
        panic!("the popups did not close");
    }

    fn type_now(ctx: &egui::Context, app: &mut IdeApp, keyboard: bool, text: &str) {
        if keyboard {
            let typed = egui::Event::Text(text.to_owned());
            let mut output = ctx.run_ui(egui::RawInput { events: vec![typed], ..Default::default() }, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
        } else {
            ask(ctx, app, "press_keys", json!({ "text": text })).unwrap();
        }
        for _ in 0..3 {
            frame(ctx, app);
        }
    }

    #[test]
    fn typing_at_once_after_search_everywhere_closes_goes_into_the_popup_opened_next() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, _server) = enter_app(data.path());
        let popups = [
            ("view.findInFiles.open", "findInFiles"),
            ("view.replaceInFiles.open", "replaceInFiles"),
            ("view.searchEverywhere.open", "searchEverywhere:all"),
            ("view.gotoClass.open", "searchEverywhere:classes"),
            ("view.gotoFile.open", "searchEverywhere:files"),
            ("view.gotoSymbol.open", "searchEverywhere:symbols"),
            ("view.findAction.open", "searchEverywhere:actions"),
            ("view.recentFiles.open", "recentFiles"),
            ("view.recentLocations.open", "recentLocations"),
            ("view.fileStructure.open", "fileStructure"),
            ("view.pasteFromHistory.open", "pasteFromHistory"),
            ("view.projects.popup", "projects"),
            ("view.runAnything.open", "runAnything"),
        ];
        for (keyboard, gap) in [(false, 0), (true, 0), (false, 1), (true, 1)] {
            for (action, popup) in popups {
                show_file(&ctx, &mut app, &project, "main.go");
                let what = format!("{popup}, keyboard {keyboard}, {gap} frames after ⎋");
                ask(&ctx, &mut app, "run_action", json!({ "action": "view.searchEverywhere.open" })).unwrap();
                for _ in 0..3 {
                    frame(&ctx, &mut app);
                }
                let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
                assert_eq!(closed["window"]["popups"], json!([]), "{what}: ⎋ closes Search Everywhere");
                for _ in 0..gap {
                    frame(&ctx, &mut app);
                }
                ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
                type_now(&ctx, &mut app, keyboard, "q");
                untouched(&app, &what);
                let shown = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
                assert_eq!(shown, json!([popup]), "{what}");
                assert_eq!(typed(&app, popup), "q", "{what}: the letter goes into the popup");
                escape_all(&ctx, &mut app);
                untouched(&app, &format!("{what}, closed"));
            }
        }
    }

    #[test]
    fn find_in_files_opens_with_the_last_query_selected_so_typing_replaces_it() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, _server) = enter_app(data.path());
        for keyboard in [false, true] {
            for action in ["view.findInFiles.open", "view.replaceInFiles.open"] {
                show_file(&ctx, &mut app, &project, "main.go");
                let what = format!("{action}, keyboard {keyboard}");
                ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
                frame(&ctx, &mut app);
                type_now(&ctx, &mut app, keyboard, "println");
                let popup = if action == "view.findInFiles.open" { "findInFiles" } else { "replaceInFiles" };
                assert_eq!(typed(&app, popup), "println", "{what}");
                escape_all(&ctx, &mut app);
                ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
                frame(&ctx, &mut app);
                assert_eq!(typed(&app, popup), "println", "{what}: the last query comes back");
                type_now(&ctx, &mut app, keyboard, "main");
                assert_eq!(typed(&app, popup), "main", "{what}: typing replaces the last query");
                escape_all(&ctx, &mut app);
                untouched(&app, &what);
            }
        }
    }

    fn frame_with(ctx: &egui::Context, app: &mut IdeApp, events: Vec<egui::Event>) {
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        output.textures_delta.clear();
    }

    fn press(key: egui::Key, modifiers: egui::Modifiers) -> Vec<egui::Event> {
        [true, false]
            .map(|pressed| egui::Event::Key { key, physical_key: Some(key), pressed, repeat: false, modifiers })
            .to_vec()
    }

    fn text(text: &str) -> egui::Event {
        egui::Event::Text(text.to_owned())
    }

    fn editor_has_keyboard(ctx: &egui::Context, app: &IdeApp) -> bool {
        app.slot_for(None).unwrap().window.lock().unwrap().view.editor_view.has_focus_in(ctx)
    }

    fn frames_to_answer(ctx: &egui::Context, app: &mut IdeApp, tool: &str, arguments: Value) -> usize {
        let (sender, answer) = mpsc::channel();
        app.route(
            ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for frames in 0..10 {
            if let Ok(result) = answer.try_recv() {
                result.unwrap();
                return frames;
            }
            frame(ctx, app);
        }
        panic!("no answer to {tool} {arguments} after 10 frames");
    }

    #[test]
    fn keys_typed_in_the_frame_a_popup_opens_reach_it_in_their_order() {
        use egui::{ImeEvent, Key, Modifiers};
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, _server) = enter_app(data.path());
        let none = Modifiers::NONE;
        let cases: [(&str, Vec<egui::Event>, &str); 6] = [
            ("letters and ⌫", [vec![text("ab")], press(Key::Backspace, none), vec![text("cd")]].concat(), "acd"),
            ("letters and ←", [vec![text("q")], press(Key::ArrowLeft, none), vec![text("x")]].concat(), "xq"),
            ("Russian", vec![text("й"), text("цу")], "йцу"),
            (
                "an input method",
                vec![
                    egui::Event::Ime(ImeEvent::Preedit { text: "ni".to_owned(), active_range_chars: Some(0..2) }),
                    egui::Event::Ime(ImeEvent::Commit("你".to_owned())),
                ],
                "你",
            ),
            ("paste", vec![egui::Event::Paste("pasted".to_owned())], "pasted"),
            ("⌥ letter", [press(Key::F, Modifiers::ALT), vec![text("ƒ")]].concat(), "ƒ"),
        ];
        let popups = [
            ("view.findInFiles.open", "findInFiles"),
            ("view.searchEverywhere.open", "searchEverywhere:all"),
            ("view.gotoFile.open", "searchEverywhere:files"),
        ];
        for (action, popup) in popups {
            for (case, events, expected) in &cases {
                show_file(&ctx, &mut app, &project, "main.go");
                let what = format!("{popup}, {case}");
                ask(&ctx, &mut app, "run_action", json!({ "action": "view.searchEverywhere.open" })).unwrap();
                frame(&ctx, &mut app);
                ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
                ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
                frame_with(&ctx, &mut app, events.clone());
                for _ in 0..3 {
                    frame(&ctx, &mut app);
                }
                untouched(&app, &what);
                let shown = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
                assert_eq!(shown, json!([popup]), "{what}");
                assert_eq!(typed(&app, popup), *expected, "{what}");
                escape_all(&ctx, &mut app);
                untouched(&app, &format!("{what}, closed"));
            }
        }

        for (action, events, still_open) in [
            ("view.searchEverywhere.open", [vec![text("q")], press(Key::Escape, none)].concat(), None),
            ("view.findInFiles.open", [vec![text("q")], press(Key::Escape, none)].concat(), None),
            ("view.findInFiles.open", [vec![text("zzz")], press(Key::Enter, none)].concat(), Some("findInFiles")),
            ("view.recentFiles.open", [vec![text("oth")], press(Key::Enter, none)].concat(), None),
        ] {
            show_file(&ctx, &mut app, &project, "main.go");
            let what = format!("{action}, {events:?}");
            ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
            frame_with(&ctx, &mut app, events);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            untouched(&app, &what);
            let shown = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
            match still_open {
                Some(popup) => {
                    assert_eq!(shown, json!([popup]), "{what}: Enter with nothing found keeps it open");
                    assert_eq!(typed(&app, popup), "zzz", "{what}: the letters came before Enter");
                    escape_all(&ctx, &mut app);
                }
                None => assert_eq!(shown, json!([]), "{what}: the key after the letters closed the popup"),
            }
            assert!(editor_has_keyboard(&ctx, &app), "{what}: the editor has the keyboard again");
        }
    }

    #[test]
    fn keys_typed_while_a_dialog_opens_reach_it_in_their_order() {
        use egui::{Key, Modifiers};
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, _server) = enter_app(data.path());
        let field = |app: &IdeApp, action: &str| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            match action {
                "view.settings.open" => window.view.settings.search_text().to_owned(),
                _ => window.view.goto_line.as_ref().map(|(text, _)| text.clone()).unwrap_or_default(),
            }
        };
        for opening in 0..3 {
            show_file(&ctx, &mut app, &project, "main.go");
            let what = format!("Settings, typing from frame {opening}");
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.settings.open" })).unwrap();
            for _ in 0..opening {
                frame(&ctx, &mut app);
            }
            frame_with(&ctx, &mut app, vec![text("12")]);
            frame_with(&ctx, &mut app, vec![text("3")]);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            untouched(&app, &what);
            assert!(!editor_has_keyboard(&ctx, &app), "{what}: the file under the dialog has no keyboard");
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘F"] })).unwrap();
            frame_with(&ctx, &mut app, vec![text("12")]);
            frame_with(&ctx, &mut app, [vec![text("3")], press(Key::Backspace, Modifiers::NONE)].concat());
            frame_with(&ctx, &mut app, vec![text("4")]);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            assert_eq!(field(&app, "view.settings.open"), "124", "{what}: ⌘F gives the keyboard to the search field");
            escape_all(&ctx, &mut app);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            assert!(editor_has_keyboard(&ctx, &app), "{what}: the editor has the keyboard again");
        }
        for (action, opening) in [("view.gotoLine.open", 0), ("view.gotoLine.open", 1)] {
            show_file(&ctx, &mut app, &project, "main.go");
            let what = format!("{action}, typing from frame {opening}");
            ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
            for _ in 0..opening {
                frame(&ctx, &mut app);
            }
            frame_with(&ctx, &mut app, vec![text("12")]);
            frame_with(&ctx, &mut app, [vec![text("3")], press(Key::Backspace, Modifiers::NONE)].concat());
            frame_with(&ctx, &mut app, vec![text("4")]);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            untouched(&app, &what);
            assert_eq!(field(&app, action), "1:1124", "{what}: after the caret's place the field opens with");
            escape_all(&ctx, &mut app);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            assert!(editor_has_keyboard(&ctx, &app), "{what}: the editor has the keyboard again");
        }

        let fetch = {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            window.session.state_mut::<zen_workbench::tasks::Tasks>().start("Fetching origin", "vcs", false)
        };
        for events in [vec![text("q")], press(Key::Escape, Modifiers::NONE)] {
            show_file(&ctx, &mut app, &project, "main.go");
            let what = format!("confirmExit, {events:?}");
            frame_with(&ctx, &mut app, press(Key::Q, Modifiers::COMMAND));
            frame_with(&ctx, &mut app, events.clone());
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            untouched(&app, &what);
            assert!(!app.shared.is_exiting(), "{what}");
            let shown = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
            let open = if events.len() == 1 { json!(["confirmExit"]) } else { json!([]) };
            assert_eq!(shown, open, "{what}");
            if shown != json!([]) {
                escape_all(&ctx, &mut app);
            }
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            assert!(editor_has_keyboard(&ctx, &app), "{what}: the editor has the keyboard again");
        }
        drop(fetch);
    }

    #[test]
    fn press_keys_waits_one_frame_at_most_for_a_popup_that_opens() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, _server) = enter_app(data.path());
        show_file(&ctx, &mut app, &project, "main.go");
        let text = json!({ "text": "zz" });
        for (action, popup) in [
            ("view.recentFiles.open", "recentFiles"),
            ("view.findInFiles.open", "findInFiles"),
            ("view.settings.open", "settings:appearance"),
        ] {
            ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
            let opening = frames_to_answer(&ctx, &mut app, "press_keys", text.clone());
            assert!(opening <= 2, "{popup}: the answer to text typed as it opens took {opening} frames");
            let open = frames_to_answer(&ctx, &mut app, "press_keys", text.clone());
            assert_eq!(open, 1, "{popup}: once it is open, the answer comes after one frame");
            let shown = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"].clone();
            assert_eq!(shown[0].as_str().unwrap().split(':').next(), popup.split(':').next(), "{shown}");
            escape_all(&ctx, &mut app);
            untouched(&app, popup);
        }
        let keys = frames_to_answer(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘E"] }));
        assert!(keys <= 2, "keys that open a popup are answered after {keys} frames");
        escape_all(&ctx, &mut app);
        let plain = frames_to_answer(&ctx, &mut app, "press_keys", json!({ "keys": ["→"] }));
        assert_eq!(plain, 1, "without a popup opening, the answer comes after one frame");
    }

    #[cfg(unix)]
    #[test]
    fn typing_as_a_popup_opens_over_the_terminal_goes_into_the_popup() {
        use zen_workbench::terminal::Terminals;
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, _project, _server) = enter_app(data.path());
        with_terminals(&app, |window| window.session.state_mut::<Terminals>().set_launcher(test_shell(data.path())));
        let id = ask(&ctx, &mut app, "terminal_open", json!({})).unwrap()["id"].as_u64().unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        wait_screen(&ctx, &mut app, id, "$");
        for (action, popup) in
            [("view.searchEverywhere.open", "searchEverywhere:all"), ("view.findInFiles.open", "findInFiles")]
        {
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.searchEverywhere.open" })).unwrap();
            frame(&ctx, &mut app);
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap();
            frame_with(&ctx, &mut app, vec![text("q")]);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            assert_eq!(typed(&app, popup), "q", "{popup}");
            escape_all(&ctx, &mut app);
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
        }
        ask(&ctx, &mut app, "press_keys", json!({ "text": "echo back-$((40+2))\n" })).unwrap();
        frame(&ctx, &mut app);
        wait_screen(&ctx, &mut app, id, "back-42");
        untouched(&app, "the terminal");
    }

    #[test]
    fn surround_with_shows_its_items_and_the_preview_and_a_digit_applies_one() {
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let text = "package main\n\nfunc main() {\n\tx := 1\n\tprintln(x)\n}\n";
        std::fs::write(&file, text).unwrap();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let surrounders = r#"[
            { "language": "go", "id": "if", "title": "if", "text": "if $CONDITION$ {\n\t$SELECTION$\n}" },
            { "language": "go", "id": "ifElse", "title": "if / else", "text": "if $CONDITION$ {\n\t$SELECTION$\n} else {\n\t$END$\n}" },
            { "language": "go", "id": "for", "title": "for", "text": "for $CONDITION$ {\n\t$SELECTION$\n}" },
            { "language": "go", "id": "func", "title": "func() { … }()", "text": "func() {\n\t$SELECTION$\n}()" }
        ]"#;
        app.shared.templates.declare_language(zen_workbench::templates::Builtin {
            language: "go".to_owned(),
            templates: Vec::new(),
            add_import: None,
            surrounders: zen_workbench::templates::Surrounder::parse_list(surrounders).unwrap(),
            ..Default::default()
        });
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS");
        let scale = if snapshots.is_some() { 2.0 } else { 0.5 };
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1000.0, 560.0), scale);
        let ctx = screen.context().clone();
        let draw = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp| {
            screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            })
        };
        draw(&mut screen, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        run(&mut app, "file.document.open", json!({ "path": file }));
        let wrap = json!({ "language": "go", "abbreviation": "defer", "description": "Defer the code",
            "text": "defer func() {\n\t$SELECTION$\n}()$END$" });
        run(&mut app, "editor.templates.set", json!({ "template": wrap }));
        let caret = text.find("println").unwrap();
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for &language in languages {
            zen_i18n::set_language(Some(language));
            for (keys, popup, name) in [("⌥⌘T", "surround", "surround"), ("⌥⌘J", "surroundWithTemplate", "templates")]
            {
                run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": caret, "head": caret }] }));
                draw(&mut screen, &mut app);
                let shown = ask(&ctx, &mut app, "press_keys", json!({ "keys": [keys] })).unwrap();
                assert_eq!(shown["window"]["popups"], json!([popup]), "{keys}: {shown}");
                let down = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["↓"] })).unwrap();
                assert_eq!(down["window"]["popups"], json!([popup]), "{keys} ↓");
                let image = draw(&mut screen, &mut app);
                if let Some(dir) = &snapshots {
                    crate::offscreen::write_png(&Path::new(dir).join(format!("{name}-{language}.png")), &image)
                        .unwrap();
                }
                ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
                draw(&mut screen, &mut app);
            }
        }
        zen_i18n::set_language(Some("en"));
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": caret, "head": caret }] }));
        draw(&mut screen, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⌘T"] })).unwrap();
        draw(&mut screen, &mut app);
        let applied = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["2"] })).unwrap();
        draw(&mut screen, &mut app);
        assert_eq!(applied["window"]["popups"], json!([]), "{applied}");
        let changed = app.tool(&ctx, "get_document_text", &json!({ "path": file })).unwrap();
        assert_eq!(
            changed["text"],
            "package main\n\nfunc main() {\n\tx := 1\n\tif  {\n\t\tprintln(x)\n\t} else {\n\t\t\n\t}\n}\n",
            "the digit picked if / else and typed nothing"
        );
        run(&mut app, "editor.edit.undo", json!({}));
        let undone = app.tool(&ctx, "get_document_text", &json!({ "path": file })).unwrap();
        assert_eq!(undone["text"], text, "one undo");
    }

    #[test]
    fn enter_in_the_other_popups_does_not_type_into_the_file() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, _server) = enter_app(data.path());
        for keyboard in [true, false] {
            show_file(&ctx, &mut app, &project, "other.go");
            show_file(&ctx, &mut app, &project, "main.go");
            let recent = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘E"] })).unwrap();
            assert_eq!(recent["window"]["popups"], json!(["recentFiles"]), "{recent}");
            frame(&ctx, &mut app);
            assert_eq!(press_return(&ctx, &mut app, keyboard, "Recent Files"), json!([]), "{keyboard}");
            assert_eq!(active_name(&app), "other.go", "⌘E ⏎ goes back to the previous file");

            show_file(&ctx, &mut app, &project, "main.go");
            let goto = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘L"] })).unwrap();
            assert_eq!(goto["window"]["popups"], json!(["gotoLine"]), "{goto}");
            frame(&ctx, &mut app);
            assert_eq!(press_return(&ctx, &mut app, keyboard, "Go to Line"), json!([]), "{keyboard}");

            let query = if keyboard { "otherWork" } else { "" };
            let find = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⌘F"], "text": query })).unwrap();
            assert_eq!(find["window"]["popups"], json!(["findInFiles"]), "{find}");
            for _ in 0..30 {
                frame(&ctx, &mut app);
                std::thread::sleep(Duration::from_millis(10));
            }
            settle(&ctx, &mut app, "search.text.results", json!({}));
            assert_eq!(press_return(&ctx, &mut app, keyboard, "Find in Files"), json!([]), "{keyboard}");
            assert_eq!(active_name(&app), "other.go", "Enter opened the match");
        }
    }

    #[test]
    fn find_in_files_pins_and_opens_its_results_in_the_find_window_over_mcp() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, _project, _server) = enter_app(data.path());
        let run = |ctx: &egui::Context, app: &mut IdeApp, action: &str, params: Value| {
            ask(ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        assert!(run(&ctx, &mut app, "view.findInFiles.openInFindWindow", json!({})).is_err(), "nothing to open");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⌘F"], "text": "package" })).unwrap();
        for _ in 0..30 {
            frame(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(10));
        }
        settle(&ctx, &mut app, "search.text.results", json!({}));
        let pinned = run(&ctx, &mut app, "view.findInFiles.pin", json!({})).unwrap();
        assert_eq!(pinned["result"]["pinned"], true, "{pinned}");
        let found = run(&ctx, &mut app, "view.findInFiles.openInFindWindow", json!({ "newTab": true })).unwrap();
        assert_eq!(found["result"]["title"], "Occurrences of 'package'", "{found}");
        let tree = json!({ "command": "search.results.tree", "params": { "waitMs": 5000 } });
        let tree = ask(&ctx, &mut app, "dispatch_command", tree).unwrap();
        assert_eq!((&tree["total"], &tree["done"]), (&json!(2), &json!(true)), "{tree}");
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!(["findInFiles"]), "pinned, it stays open: {state}");
        assert_eq!(state["bottom"], "find", "{state}");
        app.tool(&ctx, "set_settings", &json!({ "findInFilesTooMany": 1 })).unwrap();
        frame(&ctx, &mut app);
        let found = run(&ctx, &mut app, "view.findInFiles.openInFindWindow", json!({})).unwrap();
        assert!(found["result"]["tab"].as_u64().is_some(), "{found}");
        let mut state = json!(null);
        for _ in 0..200 {
            frame(&ctx, &mut app);
            state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
            if state["popups"] == json!(["findInFiles", "tooManyResults"]) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state["popups"], json!(["findInFiles", "tooManyResults"]), "{state}");
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        let tree = json!({ "command": "search.results.tree", "params": { "waitMs": 5000 } });
        let tree = ask(&ctx, &mut app, "dispatch_command", tree).unwrap();
        assert_eq!((&tree["total"], &tree["done"]), (&json!(2), &json!(true)), "Continue lists every match: {tree}");
        let list = json!({ "command": "search.results.list", "params": {} });
        let list = ask(&ctx, &mut app, "dispatch_command", list).unwrap();
        assert_eq!(
            list["tabs"].as_array().unwrap().len(),
            1,
            "the results took the place of the tab not pinned: {list}"
        );
        let unpinned = run(&ctx, &mut app, "view.findInFiles.pin", json!({ "on": false })).unwrap();
        assert_eq!(unpinned["result"]["pinned"], false);
    }

    fn screen_step(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, events: Vec<egui::Event>) {
        screen.step(events, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
    }

    fn node_at(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, label: &str) -> Option<egui::Rect> {
        let tree = screen.accessibility_tree(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        tree.nodes.iter().find(|(_, node)| node.label().is_some_and(|text| text == label)).and_then(|(_, node)| {
            let b = node.bounds()?;
            Some(egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32)))
        })
    }

    fn node_when(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, label: &str) -> Option<egui::Rect> {
        let until = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(rect) = node_at(screen, app, label) {
                return Some(rect);
            }
            if Instant::now() > until {
                return None;
            }
            screen_step(screen, app, Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn screen_click(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        screen_step(screen, app, vec![egui::Event::PointerMoved(at)]);
        screen_step(screen, app, vec![button(true)]);
        screen_step(screen, app, vec![button(false)]);
        screen_step(screen, app, Vec::new());
    }

    fn screen_wait(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, wait: Duration) {
        let until = Instant::now() + wait;
        while Instant::now() < until {
            screen_step(screen, app, Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn screen_until(
        screen: &mut crate::offscreen::Offscreen,
        app: &mut IdeApp,
        timeout: Duration,
        done: &dyn Fn() -> bool,
    ) {
        let until = Instant::now() + timeout;
        while !done() {
            assert!(Instant::now() < until, "the condition never held in {timeout:?}");
            screen_step(screen, app, Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn screen_ask(
        screen: &mut crate::offscreen::Offscreen,
        app: &mut IdeApp,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        let (sender, answer) = mpsc::channel();
        let ctx = screen.context().clone();
        app.route(
            &ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for _ in 0..5 {
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            screen_step(screen, app, Vec::new());
        }
        answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments} after 5 frames"))
    }

    fn screen_results(
        screen: &mut crate::offscreen::Offscreen,
        app: &mut IdeApp,
        wanted: &dyn Fn(&Value) -> bool,
    ) -> Value {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let params = json!({ "command": "search.text.results", "params": {} });
            let found = screen_ask(screen, app, "dispatch_command", params).unwrap();
            if found["done"] == true && wanted(&found) {
                return found;
            }
            assert!(Instant::now() < deadline, "the search never gave what was wanted: {found}");
            screen_step(screen, app, Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn screen_settle(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, command: &str) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let params = json!({ "command": command, "params": {} });
            let found = screen_ask(screen, app, "dispatch_command", params).unwrap();
            if found["pending"] != true && found["indexing"] != true && found["done"] != false {
                break;
            }
            assert!(Instant::now() < deadline, "{command} still looking: {found}");
            std::thread::sleep(Duration::from_millis(10));
        }
        screen_step(screen, app, Vec::new());
    }

    struct GoLiterals;

    impl zen_syntax::Highlighter for GoLiterals {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<zen_syntax::Span> {
            let mut spans = Vec::new();
            let mut offset = 0;
            for line in text.split_inclusive('\n') {
                let comment = line.find("//").unwrap_or(line.len());
                let quotes: Vec<usize> = line[..comment].match_indices('"').map(|(at, _)| at).collect();
                for pair in quotes.chunks(2).filter(|pair| pair.len() == 2) {
                    let range = offset + pair[0]..offset + pair[1] + 1;
                    spans.push(zen_syntax::Span { range, kind: zen_syntax::HighlightKind::String });
                }
                if comment < line.len() {
                    let range = offset + comment..offset + line.trim_end().len();
                    spans.push(zen_syntax::Span { range, kind: zen_syntax::HighlightKind::Comment });
                }
                offset += line.len();
            }
            spans
        }
    }

    #[test]
    fn the_find_window_lists_groups_excludes_and_replaces_occurrences_in_a_running_ide() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        for (name, text) in [
            ("kucoin/client.go", "package kucoin\n\n// Version of the client.\nvar Version = \"Version 1\"\n"),
            ("internal/bootstrap/run.go", "package bootstrap\n\nfunc Run() string {\n\treturn Version\n}\n"),
            ("internal/bootstrap/run_test.go", "package bootstrap\n\nvar _ = Version\n"),
            ("sol.go", "package main\r\n\r\nfunc sol() { println(Version) }\r\n"),
            ("notes.txt", "Version notes\n"),
        ] {
            let path = project.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut product = test_product();
        product.highlighters = vec![Arc::new(GoLiterals)];
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let scale = if snapshots.is_some() { 2.0 } else { 1.0 };
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1280.0, 1000.0), scale);
        let ctx = screen.context().clone();
        for _ in 0..130 {
            screen_step(&mut screen, &mut app, Vec::new());
        }
        let snapshot = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str| {
            let Some(dir) = &snapshots else { return };
            for language in ["en", "ru"] {
                zen_i18n::set_language(Some(language));
                for _ in 0..2 {
                    screen_step(screen, app, Vec::new());
                }
                let image = screen.render(|ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
                crate::offscreen::write_png(&dir.join(format!("find-window-{name}-{language}.png")), &image).unwrap();
            }
            zen_i18n::set_language(Some("en"));
            screen_step(screen, app, Vec::new());
        };
        let command = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, command: &str, params: Value| {
            screen_ask(screen, app, "dispatch_command", json!({ "command": command, "params": params }))
                .unwrap_or_else(|error| panic!("{command}: {error}"))
        };
        let state = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp| {
            screen_ask(screen, app, "get_window_state", json!({})).unwrap()
        };
        let keys = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, keys: &[&str]| {
            screen_ask(screen, app, "press_keys", json!({ "keys": keys })).unwrap();
            screen_step(screen, app, Vec::new());
        };
        let wait_until = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, what: &dyn Fn(&Value) -> bool| {
            for _ in 0..300 {
                screen_step(screen, app, Vec::new());
                let now = screen_ask(screen, app, "get_window_state", json!({})).unwrap();
                if what(&now) {
                    return now;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the window never got there: {}", screen_ask(screen, app, "get_window_state", json!({})).unwrap());
        };
        screen_settle(&mut screen, &mut app, "search.index.status");

        app.tool(&ctx, "set_settings", &json!({ "findInFilesTooMany": 2 })).unwrap();
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⇧⌘F"], "text": "Version" })).unwrap();
        screen_wait(&mut screen, &mut app, Duration::from_millis(300));
        screen_settle(&mut screen, &mut app, "search.text.results");
        keys(&mut screen, &mut app, &["⌘⏎"]);
        wait_until(&mut screen, &mut app, &|now| now["popups"] == json!(["tooManyResults"]));
        let tasks = command(&mut screen, &mut app, "tasks.background.list", json!({}));
        let task = tasks["tasks"].as_array().unwrap().iter().rev().find(|task| task["source"] == "search").unwrap();
        assert_eq!(
            (&task["title"], &task["state"], &task["cancellable"]),
            (&json!("Searching for 'Version'"), &json!("running"), &json!(true))
        );
        assert!(task["detail"].as_str().unwrap().starts_with("in "), "{task}");
        snapshot(&mut screen, &mut app, "too-many");
        keys(&mut screen, &mut app, &["⏎"]);
        let tree = command(&mut screen, &mut app, "search.results.tree", json!({ "waitMs": 5000 }));
        assert_eq!(
            (&tree["done"], &tree["total"], &tree["title"]),
            (&json!(true), &json!(7), &json!("Occurrences of 'Version'")),
            "{tree}"
        );
        let now = wait_until(&mut screen, &mut app, &|now| now["popups"] == json!([]) && now["bottom"] == "find");
        assert_eq!(now["bottom"], "find");
        let top: Vec<&str> =
            tree["nodes"].as_array().unwrap().iter().map(|node| node["label"].as_str().unwrap()).collect();
        assert_eq!(top, ["Targets", "Found occurrences in Project"]);
        let types: Vec<&str> = tree["nodes"][1]["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["label"].as_str().unwrap())
            .collect();
        assert_eq!(types, ["Unclassified", "Usage in comments", "Usage in string constants"], "{tree}");
        snapshot(&mut screen, &mut app, "occurrences");

        let group = node_when(&mut screen, &mut app, "Group By").expect("Group By");
        screen_click(&mut screen, &mut app, group.center());
        snapshot(&mut screen, &mut app, "group-by");
        keys(&mut screen, &mut app, &["⎋"]);
        assert!(!egui::Popup::is_any_open(&ctx), "⎋ closes the Group By menu");
        screen_wait(&mut screen, &mut app, Duration::from_millis(50));
        let row = node_when(&mut screen, &mut app, "run_test.go").expect("the row of run_test.go");
        screen_click(&mut screen, &mut app, row.center());
        let selected = command(&mut screen, &mut app, "search.results.tree", json!({ "depth": 0 }))["selected"].clone();
        assert!(selected.as_str().unwrap_or_default().ends_with("file:internal/bootstrap/run_test.go"), "{selected}");
        keys(&mut screen, &mut app, &["⌃P"]);
        let options = command(&mut screen, &mut app, "search.results.list", json!({}))["options"].clone();
        assert_eq!(
            (&options["directory"], &options["directoryStructure"]),
            (&json!(true), &json!(false)),
            "⌃P: {options}"
        );
        keys(&mut screen, &mut app, &["⌃D"]);
        let options = command(&mut screen, &mut app, "search.results.list", json!({}))["options"].clone();
        assert_eq!(
            (&options["directory"], &options["directoryStructure"]),
            (&json!(false), &json!(true)),
            "⌃D: {options}"
        );

        screen_step(&mut screen, &mut app, Vec::new());
        screen_wait(&mut screen, &mut app, Duration::from_millis(50));
        let row = node_when(&mut screen, &mut app, "run_test.go").expect("the row of run_test.go");
        screen_click(&mut screen, &mut app, row.center());
        keys(&mut screen, &mut app, &["⌫"]);
        let excluded = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp| {
            let tree = command(screen, app, "search.results.tree", json!({}));
            let text = tree.to_string();
            text.matches("\"excluded\":true").count()
        };
        assert_eq!(excluded(&mut screen, &mut app), 2, "the file and its line");
        snapshot(&mut screen, &mut app, "excluded");
        keys(&mut screen, &mut app, &["⌫"]);
        assert_eq!(excluded(&mut screen, &mut app), 0, "⌫ on an excluded node includes it");

        app.tool(&ctx, "set_settings", &json!({ "findInFilesTooMany": 1000 })).unwrap();
        screen_step(&mut screen, &mut app, Vec::new());
        keys(&mut screen, &mut app, &["⌘R"]);
        let tree = command(&mut screen, &mut app, "search.results.tree", json!({ "waitMs": 5000 }));
        assert_eq!(tree["total"], 7, "Rerun: {tree}");
        assert_eq!(state(&mut screen, &mut app)["popups"], json!([]), "⌘R in the tree is Rerun, not Replace");
        screen_wait(&mut screen, &mut app, Duration::from_millis(50));
        let row = node_when(&mut screen, &mut app, "run_test.go").expect("the row of run_test.go");
        screen_click(&mut screen, &mut app, row.center());
        keys(&mut screen, &mut app, &["⇧⌘F"]);
        assert_eq!(state(&mut screen, &mut app)["popups"], json!(["findInFiles"]), "Settings reopens the popup");
        let results = screen_results(&mut screen, &mut app, &|found| {
            found["matches"].as_array().is_some_and(|matches| !matches.is_empty())
        });
        assert!(!results["matches"].as_array().unwrap().is_empty(), "with the tab's query: {results}");
        keys(&mut screen, &mut app, &["⎋"]);

        command(&mut screen, &mut app, "file.document.open", json!({ "path": project.join("sol.go") }));
        let opened = command(
            &mut screen,
            &mut app,
            "search.text.occurrences",
            json!({ "query": { "text": "Version", "matchCase": true }, "mask": "*.go", "replacement": "Release", "newTab": true, "waitMs": 5000 }),
        );
        assert_eq!(opened["replacement"], "Release");
        wait_until(&mut screen, &mut app, &|now| now["bottom"] == "find");
        screen_wait(&mut screen, &mut app, Duration::from_millis(100));
        let list = command(&mut screen, &mut app, "search.results.list", json!({}));
        assert_eq!(list["tabs"].as_array().unwrap().len(), 2, "Open results in new tab: {list}");
        screen_wait(&mut screen, &mut app, Duration::from_millis(50));
        let found = node_when(&mut screen, &mut app, "Found occurrences in Project").expect("the Found node");
        screen_click(&mut screen, &mut app, found.center());
        let selected = command(&mut screen, &mut app, "search.results.tree", json!({ "depth": 0 }));
        assert_eq!(selected["selected"], "found", "a click selects the node");
        keys(&mut screen, &mut app, &["⌃T"]);
        keys(&mut screen, &mut app, &["⌃M"]);
        let options = command(&mut screen, &mut app, "search.results.list", json!({}))["options"].clone();
        assert_eq!((&options["usageType"], &options["module"]), (&json!(false), &json!(false)), "⌃T, ⌃M: {options}");
        screen_wait(&mut screen, &mut app, Duration::from_millis(50));
        let row = node_when(&mut screen, &mut app, "run_test.go").expect("the row of run_test.go");
        screen_click(&mut screen, &mut app, row.center());
        keys(&mut screen, &mut app, &["⌫"]);
        screen_wait(&mut screen, &mut app, Duration::from_millis(50));
        let row = node_when(&mut screen, &mut app, "run.go").expect("the row of run.go");
        screen_click(&mut screen, &mut app, row.center());
        snapshot(&mut screen, &mut app, "replace");
        let selected = node_when(&mut screen, &mut app, "Replace Selected").expect("Replace Selected");
        screen_click(&mut screen, &mut app, selected.center());
        let run_go = project.join("internal/bootstrap/run.go");
        assert_eq!(
            std::fs::read_to_string(&run_go).unwrap(),
            "package bootstrap\n\nfunc Run() string {\n\treturn Release\n}\n"
        );
        let versions = command(&mut screen, &mut app, "localHistory.versions.list", json!({ "path": run_go }));
        assert!(!versions["versions"].as_array().unwrap().is_empty(), "{versions}");
        let all = node_when(&mut screen, &mut app, "Do Replace All").expect("Do Replace All");
        screen_click(&mut screen, &mut app, all.center());
        assert_eq!(
            std::fs::read_to_string(project.join("kucoin/client.go")).unwrap(),
            "package kucoin\n\n// Release of the client.\nvar Release = \"Release 1\"\n"
        );
        assert_eq!(
            std::fs::read_to_string(project.join("internal/bootstrap/run_test.go")).unwrap(),
            "package bootstrap\n\nvar _ = Version\n",
            "excluded"
        );
        let text = active_text(&app);
        assert_eq!(
            text, "package main\r\n\r\nfunc sol() { println(Release) }\r\n",
            "the open file in its editor, CRLF kept"
        );
        command(&mut screen, &mut app, "editor.edit.undo", json!({}));
        let text = active_text(&app);
        assert_eq!(text, "package main\r\n\r\nfunc sol() { println(Version) }\r\n", "one undo step");
        snapshot(&mut screen, &mut app, "replaced");

        app.tool(&ctx, "set_settings", &json!({ "findInFilesTooMany": 1 })).unwrap();
        screen_step(&mut screen, &mut app, Vec::new());
        let pending = command(
            &mut screen,
            &mut app,
            "search.text.occurrences",
            json!({ "query": { "text": "package" }, "waitMs": 5000 }),
        );
        assert_eq!(pending["more"], true, "{pending}");
        wait_until(&mut screen, &mut app, &|now| now["popups"] == json!(["tooManyResults"]));
        snapshot(&mut screen, &mut app, "searching");
        command(&mut screen, &mut app, "tasks.background.cancel", json!({}));
        let tree = command(&mut screen, &mut app, "search.results.tree", json!({ "waitMs": 5000 }));
        assert_eq!((&tree["done"], &tree["more"]), (&json!(true), &json!(false)), "{tree}");
        wait_until(&mut screen, &mut app, &|now| now["popups"] == json!([]));
    }

    #[test]
    fn the_find_window_keeps_frames_short_with_ten_thousand_usages_and_its_keys_in_its_tree() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let text = "hit hit\n".repeat(25);
        for index in 0..200 {
            let path = project.join(format!("d{}/f{index}.txt", index % 10));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &text).unwrap();
        }
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1280.0, 900.0), 1.0);
        let command = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, command: &str, params: Value| {
            screen_ask(screen, app, "dispatch_command", json!({ "command": command, "params": params }))
                .unwrap_or_else(|error| panic!("{command}: {error}"))
        };
        let state = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp| {
            screen_ask(screen, app, "get_window_state", json!({})).unwrap()
        };
        screen_step(&mut screen, &mut app, Vec::new());
        screen_settle(&mut screen, &mut app, "search.index.status");
        let timed = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp| {
            let start = Instant::now();
            screen_step(screen, app, Vec::new());
            start.elapsed()
        };
        let opened = command(
            &mut screen,
            &mut app,
            "search.text.occurrences",
            json!({ "query": { "text": "hit" }, "limit": 5000, "replacement": "miss" }),
        );
        let tab = opened["tab"].clone();
        let mut while_searching = Vec::new();
        while state(&mut screen, &mut app)["popups"] != json!(["tooManyResults"]) {
            while_searching.push(timed(&mut screen, &mut app));
            assert!(while_searching.len() < 1000, "Too Many Results never asked");
        }
        for _ in 0..10 {
            while_searching.push(timed(&mut screen, &mut app));
        }
        command(&mut screen, &mut app, "search.results.tooMany", json!({ "continue": true }));
        loop {
            while_searching.push(timed(&mut screen, &mut app));
            let list = command(&mut screen, &mut app, "search.results.list", json!({}));
            if list["tabs"][0]["done"] == true {
                break;
            }
        }
        for _ in 0..5 {
            screen_step(&mut screen, &mut app, Vec::new());
        }
        let tree = command(&mut screen, &mut app, "search.results.tree", json!({ "depth": 0 }));
        assert_eq!(tree["total"], 10_000, "{tree}");
        let shown: Vec<Duration> = (0..20).map(|_| timed(&mut screen, &mut app)).collect();
        let worst = |frames: &[Duration]| frames.iter().max().copied().unwrap_or_default();
        let median = |frames: &[Duration]| {
            let mut sorted = frames.to_vec();
            sorted.sort();
            sorted.get(sorted.len() / 2).copied().unwrap_or_default()
        };
        if std::env::var_os("ZEN_TIMING_CHECKS").is_some() {
            assert!(median(&shown) < Duration::from_millis(250), "with 10 000 usages: {shown:?}");
            assert!(worst(&while_searching) < Duration::from_secs(1), "while searching: {while_searching:?}");
        }

        let row = node_when(&mut screen, &mut app, "f0.txt").expect("the row of f0.txt");
        screen_click(&mut screen, &mut app, row.center());
        let searches = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp| {
            let tasks = command(screen, app, "tasks.background.list", json!({}));
            tasks["tasks"].as_array().unwrap().iter().filter(|task| task["source"] == "search").count()
        };
        let before = searches(&mut screen, &mut app);
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⇧⌘R"] })).unwrap();
        screen_step(&mut screen, &mut app, Vec::new());
        assert_eq!(state(&mut screen, &mut app)["popups"], json!(["replaceInFiles"]), "⇧⌘R is still Replace in Files");
        assert_eq!(searches(&mut screen, &mut app), before, "⇧⌘R in the tree is not Rerun ⌘R");
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        screen_step(&mut screen, &mut app, Vec::new());

        command(&mut screen, &mut app, "file.document.open", json!({ "path": project.join("main.go") }));
        screen_step(&mut screen, &mut app, Vec::new());
        let options = command(&mut screen, &mut app, "search.results.list", json!({}))["options"].clone();
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⌃D", "⌃T", "⌫"] })).unwrap();
        screen_step(&mut screen, &mut app, Vec::new());
        let after = command(&mut screen, &mut app, "search.results.list", json!({}))["options"].clone();
        assert_eq!(after, options, "the tree's ⌃D and ⌃T do nothing when the editor has the keyboard");
        let tree = command(&mut screen, &mut app, "search.results.tree", json!({ "tab": tab }));
        assert!(!tree.to_string().contains("\"excluded\":true"), "⌫ in the editor excludes nothing");
        command(&mut screen, &mut app, "search.results.close", json!({ "tab": tab }));
        let tasks = command(&mut screen, &mut app, "tasks.background.list", json!({}));
        assert!(
            tasks["tasks"]
                .as_array()
                .unwrap()
                .iter()
                .all(|task| task["source"] != "search" || task["state"] != "running"),
            "{tasks}"
        );
    }

    #[test]
    fn find_in_files_in_a_running_ide_masks_pins_previews_and_edits() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        for (name, text) in [
            ("kucoin/client.go", "package kucoin\n\n// Version of the client.\nvar Version = \"1\" // Version\n"),
            ("binance/client.go", "package binance\n\nvar Version = \"2\"\n"),
            ("sol.go", "package main\n\nfunc sol() { println(Version) }\n"),
            ("sol_test.go", "package main\n\nvar _ = Version\n"),
            ("notes.txt", "Version notes\n"),
        ] {
            let path = project.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let scale = if snapshots.is_some() { 2.0 } else { 1.0 };
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1280.0, 800.0), scale);
        let ctx = screen.context().clone();
        screen_step(&mut screen, &mut app, Vec::new());
        let snapshot = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str| {
            let Some(dir) = &snapshots else { return };
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            crate::offscreen::write_png(&dir.join(format!("fif-app-{name}.png")), &image).unwrap();
        };
        let command = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, command: &str, params: Value| {
            screen_ask(screen, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        let state = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp| {
            screen_ask(screen, app, "get_window_state", json!({})).unwrap()
        };
        screen_settle(&mut screen, &mut app, "search.index.status");

        let typed = screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⇧⌘F"] })).unwrap();
        assert_eq!(typed["window"]["popups"], json!(["findInFiles"]), "{typed}");
        snapshot(&mut screen, &mut app, "empty");
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "text": "Version" })).unwrap();
        screen_wait(&mut screen, &mut app, Duration::from_millis(300));
        let found =
            screen_results(&mut screen, &mut app, &|found| found["matches"].as_array().map(Vec::len) == Some(7));
        assert_eq!(found["matches"].as_array().unwrap().len(), 7, "{found}");

        let mask = node_at(&mut screen, &mut app, &zen_i18n::t!("ide-fif-mask")).expect("the File mask field");
        screen_click(&mut screen, &mut app, mask.left_center() - egui::vec2(70.0, 0.0));
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "text": "*.go" })).unwrap();
        screen_wait(&mut screen, &mut app, Duration::from_millis(300));
        let masked =
            screen_results(&mut screen, &mut app, &|found| found["matches"].as_array().map(Vec::len) == Some(6));
        let paths: Vec<&str> =
            masked["matches"].as_array().unwrap().iter().filter_map(|m| m["path"].as_str()).collect();
        assert_eq!(paths.len(), 6, "{masked}");
        assert!(paths.iter().all(|path| path.ends_with(".go")), "{paths:?}");
        snapshot(&mut screen, &mut app, "results-en");
        if snapshots.is_some() {
            zen_i18n::set_language(Some("ru"));
            snapshot(&mut screen, &mut app, "results-ru");
            zen_i18n::set_language(Some("en"));
        }

        let pinned =
            screen_ask(&mut screen, &mut app, "run_action", json!({ "action": "view.findInFiles.pin", "params": {} }));
        assert_eq!(pinned.unwrap()["result"]["pinned"], true);
        screen_click(&mut screen, &mut app, egui::pos2(640.0, 760.0));
        assert_eq!(state(&mut screen, &mut app)["popups"], json!(["findInFiles"]), "pinned, a click outside keeps it");
        snapshot(&mut screen, &mut app, "pinned-outside");

        let started = Instant::now();
        let text_box = loop {
            let tree = screen.accessibility_tree(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            let found = tree
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::MultilineTextInput
                        && !node.label().is_some_and(|label| label.starts_with("Editor"))
                })
                .and_then(|(_, node)| node.bounds())
                .map(|b| egui::pos2(b.x0 as f32 + 60.0, b.y0 as f32 + 10.0));
            if let Some(found) = found {
                break found;
            }
            assert!(started.elapsed() < Duration::from_secs(60), "the preview editor");
            std::thread::sleep(Duration::from_millis(20));
        };
        screen_click(&mut screen, &mut app, text_box);
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⌘↑"], "text": "// edited\n" })).unwrap();
        let first = project.join("binance/client.go");
        screen_until(&mut screen, &mut app, Duration::from_secs(10), &|| {
            std::fs::read_to_string(&first).is_ok_and(|text| text.starts_with("// edited\n"))
        });
        assert_eq!(
            std::fs::read_to_string(&first).unwrap(),
            "// edited\npackage binance\n\nvar Version = \"2\"\n",
            "the closed file is written when typing pauses"
        );
        let versions = command(&mut screen, &mut app, "localHistory.versions.list", json!({ "path": first }));
        assert!(!versions["versions"].as_array().unwrap().is_empty(), "{versions}");
        snapshot(&mut screen, &mut app, "preview-edited");

        app.tool(&ctx, "set_settings", &json!({ "findInFilesTooMany": 2 })).unwrap();
        screen_step(&mut screen, &mut app, Vec::new());
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⌘⏎"] })).unwrap();
        for _ in 0..200 {
            screen_step(&mut screen, &mut app, Vec::new());
            if state(&mut screen, &mut app)["popups"] == json!(["findInFiles", "tooManyResults"]) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(state(&mut screen, &mut app)["popups"], json!(["findInFiles", "tooManyResults"]));
        snapshot(&mut screen, &mut app, "too-many");
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        screen_step(&mut screen, &mut app, Vec::new());

        let unpinned = screen_ask(
            &mut screen,
            &mut app,
            "run_action",
            json!({ "action": "view.findInFiles.pin", "params": { "on": false } }),
        );
        assert_eq!(unpinned.unwrap()["result"]["pinned"], false);
        let query = node_at(&mut screen, &mut app, &zen_i18n::t!("ide-find-hint")).expect("the query field");
        screen_click(&mut screen, &mut app, query.center());
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⌘A"], "text": "func sol" })).unwrap();
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        for _ in 0..50 {
            screen_step(&mut screen, &mut app, Vec::new());
            if active_name(&app) == "sol.go" {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(active_name(&app), "sol.go", "Enter in the pause opens the new query's match");
        assert_eq!(state(&mut screen, &mut app)["popups"], json!([]));

        let println =
            json!({ "selections": [{ "anchor": { "line": 2, "column": 13 }, "head": { "line": 2, "column": 20 } }] });
        command(&mut screen, &mut app, "editor.selection.set", println);
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⇧⌘F"] })).unwrap();
        screen_wait(&mut screen, &mut app, Duration::from_millis(300));
        let found =
            screen_results(&mut screen, &mut app, &|found| found["matches"].as_array().map(Vec::len) == Some(2));
        let lines: Vec<&str> = found["matches"].as_array().unwrap().iter().filter_map(|m| m["text"].as_str()).collect();
        assert_eq!(lines, ["\tprintln(1)", "func sol() { println(Version) }"], "the selection is the query");
        let previewed = |app: &IdeApp| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            window.view.find_in_files.previewed().map(str::to_owned)
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        while !previewed(&app).is_some_and(|path| path.ends_with("main.go")) {
            assert!(Instant::now() < deadline, "the preview shows main.go first: {:?}", previewed(&app));
            screen_step(&mut screen, &mut app, Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["↓"] })).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !previewed(&app).is_some_and(|path| path.ends_with("sol.go")) {
            assert!(Instant::now() < deadline, "the preview shows sol.go: {:?}", previewed(&app));
            screen_step(&mut screen, &mut app, Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
        snapshot(&mut screen, &mut app, "selection-query");

        let tree = screen.accessibility_tree(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        let text_box = tree
            .nodes
            .iter()
            .find(|(_, node)| {
                node.role() == egui::accesskit::Role::MultilineTextInput
                    && !node.label().is_some_and(|label| label.starts_with("Editor"))
            })
            .and_then(|(_, node)| node.bounds())
            .map(|b| egui::pos2(b.x0 as f32 + 60.0, b.y0 as f32 + 10.0))
            .expect("the preview editor");
        screen_click(&mut screen, &mut app, text_box);
        for _ in 0..3 {
            screen_step(&mut screen, &mut app, Vec::new());
        }
        screen_ask(&mut screen, &mut app, "press_keys", json!({ "keys": ["⌘↑"], "text": "// one\n" })).unwrap();
        let sol = project.join("sol.go");
        let text = |app: &IdeApp| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            let workbench = window.session.state::<Workbench>().unwrap();
            let document = workbench.documents().iter().find(|document| document.path().ends_with("sol.go"));
            document.map(|document| (document.snapshot().to_string(), document.is_dirty())).unwrap_or_default()
        };
        let deadline = Instant::now() + Duration::from_secs(60);
        while !text(&app).0.starts_with("// one") {
            assert!(Instant::now() < deadline, "the preview's edit reached the editor: {:?}", text(&app));
            screen_step(&mut screen, &mut app, Vec::new());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(text(&app), ("// one\npackage main\n\nfunc sol() { println(Version) }\n".to_owned(), true));
        assert_eq!(std::fs::read_to_string(&sol).unwrap(), "package main\n\nfunc sol() { println(Version) }\n");
        command(&mut screen, &mut app, "editor.edit.undo", json!({}));
        assert_eq!(text(&app).0, "package main\n\nfunc sol() { println(Version) }\n", "one undo step");
        let history = command(&mut screen, &mut app, "search.findHistory.list", json!({}));
        assert_eq!((&history["queries"][0], &history["masks"][0]), (&json!("func sol"), &json!("*.go")), "{history}");
    }

    fn vcs_settled(ctx: &egui::Context, app: &mut IdeApp) -> Value {
        let since = Instant::now();
        loop {
            frame(ctx, app);
            let status =
                ask(ctx, app, "dispatch_command", json!({ "command": "vcs.operation.status", "params": {} })).unwrap();
            let status = status.get("result").cloned().unwrap_or(status);
            if status["running"] != true {
                return status;
            }
            assert!(since.elapsed() < Duration::from_secs(60), "the Git operation never ended: {status}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn enter_in_the_git_popups_does_not_type_into_the_file() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        test_git(&project, &["init", "-q", "-b", "main"]);
        test_git(&project, &["add", "-A"]);
        test_git(&project, &["commit", "-q", "-m", "First"]);
        test_git(&project, &["branch", "feature"]);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        for _ in 0..5 {
            frame(&ctx, &mut app);
        }
        for keyboard in [true, false] {
            let update = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘T"] })).unwrap();
            assert_eq!(update["window"]["popups"], json!(["updateProject"]), "{update}");
            frame(&ctx, &mut app);
            assert_eq!(press_return(&ctx, &mut app, keyboard, "Update Project"), json!([]), "{keyboard}");
            vcs_settled(&ctx, &mut app);

            let branches = ask(&ctx, &mut app, "run_action", json!({ "action": "view.vcs.branches" })).unwrap();
            assert_eq!(branches["window"]["popups"], json!(["branches"]), "{branches}");
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            ask(&ctx, &mut app, "press_keys", json!({ "text": "feature" })).unwrap();
            frame(&ctx, &mut app);
            let menu = press_return(&ctx, &mut app, keyboard, "Branches");
            assert_eq!(menu, json!(["branches"]), "Enter opens the branch's actions");
            assert_eq!(press_return(&ctx, &mut app, keyboard, "Checkout"), json!([]), "{keyboard}");
            let checkout = vcs_settled(&ctx, &mut app);
            assert_eq!(
                (checkout["kind"].as_str(), checkout["succeeded"].as_bool()),
                (Some("checkout"), Some(true)),
                "{checkout}"
            );
            assert_eq!(test_git(&project, &["branch", "--show-current"]).trim(), "feature", "checked out");
            test_git(&project, &["checkout", "-q", "main"]);
            ask(
                &ctx,
                &mut app,
                "dispatch_command",
                json!({ "command": "vcs.status.refresh", "params": { "wait": true } }),
            )
            .unwrap();
        }
    }

    #[test]
    fn enter_in_the_editor_dialogs_does_not_type_into_the_file() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (mut app, ctx, project, _server) = enter_app(data.path());
        let caret = |app: &IdeApp| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            let workbench = window.session.state::<Workbench>().unwrap();
            workbench.active_document().unwrap().selections().primary().range()
        };
        for keyboard in [true, false] {
            show_file(&ctx, &mut app, &project, "main.go");
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.find.open" })).unwrap();
            frame(&ctx, &mut app);
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘A"], "text": "main" })).unwrap();
            frame(&ctx, &mut app);
            let first = caret(&app);
            press_return(&ctx, &mut app, keyboard, "Find");
            assert_ne!(caret(&app), first, "Enter goes to the next match, keyboard {keyboard}");
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⏎"] })).unwrap();
            frame(&ctx, &mut app);
            untouched(&app, "Find, ⇧⏎");
            assert_eq!(caret(&app), first, "⇧⏎ goes back to the previous match");
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            frame(&ctx, &mut app);

            show_file(&ctx, &mut app, &project, "main.go");
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.rename.open" })).unwrap();
            let deadline = Instant::now() + Duration::from_secs(60);
            while app.slot_for(None).unwrap().window.lock().unwrap().view.editor_view.has_focus_in(&ctx) {
                assert!(Instant::now() < deadline, "Rename did not take the keyboard");
                frame(&ctx, &mut app);
                std::thread::sleep(Duration::from_millis(10));
            }
            for _ in 0..3 {
                frame(&ctx, &mut app);
            }
            assert_eq!(press_return(&ctx, &mut app, keyboard, "Rename"), json!([]), "{keyboard}");

            let name = if keyboard { "third.go" } else { "fourth.go" };
            let from = if keyboard { "other.go" } else { "third.go" };
            let params = json!({ "action": "view.project.rename", "params": { "path": project.join(from) } });
            ask(&ctx, &mut app, "run_action", params).unwrap();
            frame(&ctx, &mut app);
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘A"], "text": name })).unwrap();
            frame(&ctx, &mut app);
            assert_eq!(press_return(&ctx, &mut app, keyboard, "Rename File"), json!([]), "{keyboard}");
            assert!(project.join(name).is_file(), "renamed to {name}");

            ask(&ctx, &mut app, "run_action", json!({ "action": "view.localHistory.putLabel" })).unwrap();
            frame(&ctx, &mut app);
            ask(&ctx, &mut app, "press_keys", json!({ "text": format!("label {keyboard}") })).unwrap();
            frame(&ctx, &mut app);
            assert_eq!(press_return(&ctx, &mut app, keyboard, "Put Label"), json!([]), "{keyboard}");
        }
    }

    #[test]
    fn safe_delete_of_a_file_nothing_uses_moves_it_to_the_trash_without_a_dialog() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let notes = project.join("notes.txt");
        std::fs::write(&notes, "notes\n").unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let params = json!({ "action": "view.safeDelete.open", "params": { "paths": [notes] } });
        ask(&ctx, &mut app, "run_action", params).unwrap();
        for _ in 0..5 {
            frame(&ctx, &mut app);
        }
        assert!(!notes.exists(), "deleted");
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!([]), "{state}");
        let unknown = json!({ "action": "view.refactoring.open", "params": { "provider": "nope" } });
        assert!(ask(&ctx, &mut app, "run_action", unknown).unwrap_err().contains("refactor.dialog.list"));
        std::fs::write(&notes, "notes\n").unwrap();
        let moving = json!({ "action": "view.project.move", "params": { "path": notes } });
        let refused = ask(&ctx, &mut app, "run_action", moving).unwrap_err();
        assert!(refused.contains("no refactoring") && refused.contains("move"), "{refused}");
        let changing =
            json!({ "action": "view.changeSignature.open", "params": { "context": { "path": notes, "offset": 0 } } });
        let refused = ask(&ctx, &mut app, "run_action", changing).unwrap_err();
        assert!(refused.contains("changeSignature"), "{refused}");
        let pulling =
            json!({ "action": "view.pullMembersUp.open", "params": { "context": { "path": notes, "offset": 0 } } });
        let refused = ask(&ctx, &mut app, "run_action", pulling).unwrap_err();
        assert!(refused.contains("pullMembersUp"), "{refused}");
    }

    #[test]
    fn text_actions_are_refused_without_a_text_file_and_a_file_comes_back_over_a_diff() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let open = json!({ "command": "file.document.open", "params": { "path": file } });
        ask(&ctx, &mut app, "dispatch_command", open.clone()).unwrap();
        let show_diff = |app: &mut IdeApp| {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            let ProjectWindow { session, view } = &mut *window;
            let diff = zen_workbench::vcs::commit::DiffView {
                path: file.clone(),
                relative: "main.go".to_owned(),
                before: "package main\n".to_owned(),
                after: "package main\n\nfunc main() {}\n".to_owned(),
                ..Default::default()
            };
            view.show_diff(session, diff);
        };

        ask(&ctx, &mut app, "run_action", json!({ "action": "view.gotoLine.open" })).unwrap();
        show_diff(&mut app);
        frame(&ctx, &mut app);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!((&state["editor"], &state["popups"]), (&json!("diff"), &json!([])), "{state}");

        for action in ["view.gotoLine.open", "view.rename.open", "view.safeDelete.open"] {
            let refused = ask(&ctx, &mut app, "run_action", json!({ "action": action })).unwrap_err();
            assert!(refused.contains("text file") && refused.contains("a diff"), "{action}: {refused}");
        }
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!((&state["editor"], &state["popups"]), (&json!("diff"), &json!([])), "{state}");

        let opened = ask(&ctx, &mut app, "dispatch_command", open.clone()).unwrap();
        assert!(opened.is_null() || opened.is_object(), "{opened}");
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert!(state["editor"].as_str().is_some_and(|path| path.ends_with("main.go")), "{state}");
        frame(&ctx, &mut app);
        let goto = ask(&ctx, &mut app, "run_action", json!({ "action": "view.gotoLine.open" })).unwrap();
        assert_eq!(goto["window"]["popups"], json!(["gotoLine"]), "{goto}");
        frame(&ctx, &mut app);
        let escaped = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(escaped["window"]["popups"], json!([]), "{escaped}");

        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.close", "params": {} })).unwrap();
        show_diff(&mut app);
        let slot = app.slot_for(None).unwrap();
        slot.window.lock().unwrap().view.diff = None;
        let refused = ask(&ctx, &mut app, "run_action", json!({ "action": "view.gotoLine.open" })).unwrap_err();
        assert!(refused.contains("no file"), "{refused}");
    }

    struct FieldsComment;

    impl zen_modules::generate::CodeGenerator for FieldsComment {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn offers(&self, _: &zen_modules::generate::GenerateContext<'_>) -> Vec<zen_modules::generate::Offer> {
            vec![zen_modules::generate::Offer {
                id: "fields".to_owned(),
                title: "Fields".to_owned(),
                chooser: true,
                action: None,
            }]
        }

        fn chooser(
            &self,
            _: &zen_modules::generate::GenerateContext<'_>,
            _: &str,
        ) -> Option<zen_modules::generate::Chooser> {
            let member = |id: &str| zen_modules::generate::Member {
                id: id.to_owned(),
                name: id.to_owned(),
                selected: id == "a",
                ..Default::default()
            };
            Some(zen_modules::generate::Chooser {
                title: "Choose".to_owned(),
                members: vec![member("a"), member("b")],
                options: Vec::new(),
                allow_empty: false,
                search: false,
            })
        }

        fn generate(
            &self,
            context: &zen_modules::generate::GenerateContext<'_>,
            _: &str,
            chosen: &zen_modules::generate::Chosen,
        ) -> Result<zen_modules::generate::Insertion, String> {
            let at = context.text.chars().count();
            Ok(zen_modules::generate::Insertion {
                at,
                end: None,
                text: format!("// {}\n", chosen.members.join(", ")),
                also: Vec::new(),
            })
        }
    }

    #[test]
    fn generate_opens_in_the_editor_and_writes_what_the_member_chooser_picks() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let source = "package main\n";
        std::fs::write(&file, source).unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        {
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            window.session.state_mut::<zen_modules::generate::CodeGenerators>().add(Arc::new(FieldsComment));
        }
        let dispatch = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        dispatch(&mut app, "file.document.open", json!({ "path": file }));
        frame(&ctx, &mut app);
        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.generate.open" })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["generate"]), "{opened}");
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!(["memberChooser"]), "{state}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["↓", "Space"] })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        frame(&ctx, &mut app);
        let text = |app: &mut IdeApp| {
            ask(&ctx, app, "get_document_text", json!({ "path": file })).unwrap()["text"].as_str().unwrap().to_owned()
        };
        assert_eq!(text(&mut app), "package main\n// a, b\n");
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!([]), "{state}");
        dispatch(&mut app, "editor.edit.undo", json!({}));
        assert_eq!(text(&mut app), source, "one undo step");
    }

    #[test]
    fn unwrap_remove_lists_in_a_popup_or_applies_its_only_entry() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let source = "package main\n\nfunc main() {\n\tif ok {\n\t\tfor {\n\t\t\trun()\n\t\t}\n\t}\n}\n";
        std::fs::write(&file, source).unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let dispatch = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        dispatch(&mut app, "file.document.open", json!({ "path": file }));
        let caret = source[..source.find("run").unwrap()].chars().count();
        dispatch(&mut app, "editor.selection.set", json!({ "selections": [{ "head": caret }] }));
        frame(&ctx, &mut app);
        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "view.unwrap.open" })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["unwrap"]), "{opened}");
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["↓", "⏎"] })).unwrap();
        let text = |app: &mut IdeApp| {
            ask(&ctx, app, "get_document_text", json!({ "path": file })).unwrap()["text"].as_str().unwrap().to_owned()
        };
        assert_eq!(text(&mut app), "package main\n\nfunc main() {\n\tfor {\n\t\trun()\n\t}\n}\n");
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["popups"], json!([]), "{state}");
        let applied = ask(&ctx, &mut app, "run_action", json!({ "action": "view.unwrap.open" })).unwrap();
        assert_eq!(applied["window"]["popups"], json!([]), "the only entry runs at once: {applied}");
        assert_eq!(text(&mut app), "package main\n\nfunc main() {\n\trun()\n}\n");
        dispatch(&mut app, "editor.edit.undo", json!({}));
        assert_eq!(text(&mut app), "package main\n\nfunc main() {\n\tfor {\n\t\trun()\n\t}\n}\n", "one undo step");
    }

    #[test]
    fn a_nuxt_project_gets_the_plugins_configurations_and_the_prepare_offer() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("shop");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("nuxt.config.ts"), "export default defineNuxtConfig({})\n").unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/nuxt/plugin.json");
        let mut manifest: Value = serde_json::from_slice(&std::fs::read(source).unwrap()).unwrap();
        manifest["id"] = json!("nuxt");
        manifest["version"] = json!("0.1.0");
        let bundle = data.path().join("bundle");
        std::fs::create_dir_all(bundle.join("nuxt")).unwrap();
        std::fs::write(bundle.join("nuxt/plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(bundle.join("nuxt/plugin.wasm"), crate::plugins::fake::EMPTY_MODULE).unwrap();
        let mut product = test_product();
        product.plugins = Some(crate::plugins::PluginConfig {
            folder: data.path().join("plugins"),
            catalog_url: None,
            public_key: String::new(),
            bundled: Some(bundle),
            optional: Vec::new(),
        });
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let dispatch = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        let configurations = dispatch(&mut app, "run.configuration.list", json!({}));
        let names: Vec<&str> =
            configurations["configurations"].as_array().unwrap().iter().filter_map(|c| c["name"].as_str()).collect();
        assert_eq!(names, ["nuxi dev", "nuxi build", "nuxi generate", "nuxi preview"], "{configurations}");
        assert!(
            configurations["kinds"].as_array().unwrap().iter().any(|kind| kind["id"] == "npm"),
            "every IDE runs npm: {configurations}"
        );
        let list = dispatch(&mut app, "ui.notifications.list", json!({}));
        let offer = list["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .find(|notification| notification["actions"][0]["id"] == "setup")
            .cloned()
            .unwrap_or_else(|| panic!("no prepare offer: {list}"));
        assert!(offer["body"].as_str().unwrap().contains("nuxi prepare"), "{offer}");
        assert!(
            list["notifications"].as_array().unwrap().iter().any(|n| n["body"].as_str().unwrap().contains("nuxi dev")),
            "the added configurations are told: {list}"
        );
        let count = list["notifications"].as_array().unwrap().len();
        frame(&ctx, &mut app);
        assert_eq!(
            dispatch(&mut app, "ui.notifications.list", json!({}))["notifications"].as_array().unwrap().len(),
            count
        );
        let listed = dispatch(&mut app, "plugins.project.list", json!({}));
        assert_eq!(listed["plugins"][0]["setup"]["needed"], true, "{listed}");
    }

    #[test]
    fn agents_search_the_settings_for_actions_and_plugins() {
        let _types = super::super::settings::file_types::testing::exclusive();
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let bundle = data.path().join("bundle");
        assert!(crate::plugins::fake::repository_plugins(&bundle) > 10);
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let product = IdeProduct {
            plugins: Some(crate::plugins::PluginConfig {
                folder: data.path().join("plugins"),
                catalog_url: None,
                public_key: String::new(),
                bundled: Some(bundle),
                optional: Vec::new(),
            }),
            ..test_product()
        };
        let mut app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project]);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 720.0), 1.0);
        let ctx = screen.context().clone();
        frame(&ctx, &mut app);
        let search = |app: &mut IdeApp, query: &str| {
            let params = json!({ "query": query });
            ask(&ctx, app, "dispatch_command", json!({ "command": "view.settings.open", "params": params })).unwrap()
        };
        let mut snapshot = |app: &mut IdeApp, name: &str| {
            let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
            for _ in 0..3 {
                screen.step(Vec::new(), |ui| {
                    app.logic(ui.ctx());
                    app.ui(ui);
                });
            }
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            crate::offscreen::write_png(&Path::new(&dir).join(format!("{name}.png")), &image).unwrap();
        };
        let languages: &[&str] = if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { &["en", "ru"] } else { &["en"] };
        for &language in languages {
            zen_i18n::set_language(Some(language));
            let action = zen_i18n::t!("keymap-editor-line-duplicate");
            let found = search(&mut app, &action);
            assert_eq!(found["window"]["popups"], json!(["settings:keymap"]), "{language}: {found}");
            let answer = &found["result"]["search"];
            assert_eq!((&answer["page"], &answer["pages"]), (&json!("keymap"), &json!(["keymap"])), "{answer}");
            assert_eq!(answer["keymapSearch"], json!(action));
            assert_eq!(answer["plugins"]["search"], "", "{answer}");
            snapshot(&mut app, &format!("settings-search-e2e-keymap-{language}"));

            let found = search(&mut app, "Duplicate Line");
            assert_eq!(found["result"]["search"]["page"], "keymap", "{language}: the English name: {found}");

            let found = search(&mut app, "MySQL");
            let answer = &found["result"]["search"];
            assert_eq!(found["window"]["popups"], json!(["settings:plugins"]), "{language}: {found}");
            assert_eq!(answer["pages"], json!(["plugins"]), "{answer}");
            assert_eq!(answer["plugins"], json!({ "tab": "installed", "search": "MySQL" }), "{answer}");
            assert_eq!(answer["keymapSearch"], "", "{answer}");
            snapshot(&mut app, &format!("settings-search-e2e-plugins-{language}"));

            let label = zen_i18n::t!("settings-mcp-port");
            let found = search(&mut app, &label);
            let answer = &found["result"]["search"];
            assert_eq!((&answer["page"], &answer["pages"]), (&json!("mcp"), &json!(["mcp"])), "{language}: {answer}");
            assert_eq!((&answer["keymapSearch"], &answer["plugins"]["search"]), (&json!(""), &json!("")), "{answer}");
            snapshot(&mut app, &format!("settings-search-e2e-neither-{language}"));
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        }
        zen_i18n::set_language(Some("en"));

        ask(&ctx, &mut app, "set_settings", json!({ "mcpLanguage": "ru" })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(app.mcp_language(), "ru");
        let found = search(&mut app, &zen_i18n::t!("settings-mcp-port"));
        assert_eq!(found["result"]["search"]["pages"], json!(["mcp"]), "in the interface's language: {found}");
        let refused = ask(&ctx, &mut app, "set_settings", json!({ "mcpLanguage": "de" })).unwrap_err();
        assert!(refused.chars().any(|c| matches!(c, 'А'..='я')), "the other answers stay in mcpLanguage: {refused}");
    }

    #[test]
    fn the_plugin_commands_are_reachable_over_mcp() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut app = IdeApp::new(test_product(), registry, Some(data.path().join("recent.json")), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let dispatch = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };
        let listed = dispatch(&mut app, "plugins.catalog.list", json!({ "query": "markdown" })).unwrap();
        assert_eq!(listed["catalog"], "unavailable", "{listed}");
        assert_eq!(listed["plugins"], json!([]));
        assert_eq!(dispatch(&mut app, "plugins.installed.list", json!({})).unwrap()["installing"], json!([]));
        assert!(dispatch(&mut app, "plugins.plugin.install", json!({ "id": "markdown" })).is_err());
        assert!(dispatch(&mut app, "plugins.updates.all", json!({})).is_err());
        assert!(dispatch(&mut app, "plugins.plugin.disable", json!({ "id": "markdown" })).is_err());
    }

    #[test]
    fn agents_get_english_on_a_russian_ui_unless_mcp_language_says_otherwise() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut app = IdeApp::new(test_product(), registry, Some(data.path().join("recent.json")), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let cyrillic = |text: &str| text.chars().any(|c| ('\u{400}'..='\u{4ff}').contains(&c));
        let replace =
            json!({ "command": "editor.text.replace", "params": { "path": file, "oldText": "p", "newText": "q" } });
        zen_i18n::with_language("ru", || {
            assert!(cyrillic(&zen_i18n::t!("settings-mcp-language")), "the UI keeps its language");
            assert_eq!(app.shared.overview().settings.mcp_language, "en", "English by default");

            let error = ask(&ctx, &mut app, "dispatch_command", replace.clone()).unwrap_err();
            let error: Value = serde_json::from_str(&error).unwrap();
            assert_eq!((&error["error"], &error["count"]), (&json!("not_unique"), &json!(2)), "{error}");
            let message = error["message"].as_str().unwrap();
            assert!(message.starts_with("The text to replace occurs 2 times"), "{message}");
            let unknown = ask(&ctx, &mut app, "describe_command", json!({ "id": "no.such.command" })).unwrap_err();
            assert!(unknown.starts_with("There is no command no.such.command"), "{unknown}");
            for tool in super::super::automation::tools() {
                assert!(!cyrillic(&tool.description), "{}: {}", tool.name, tool.description);
            }
            let instructions = super::super::automation::instructions("Test", &app.shared.registry);
            assert!(!cyrillic(&instructions), "{instructions}");

            let refused = ask(&ctx, &mut app, "set_settings", json!({ "mcpLanguage": "de" })).unwrap_err();
            assert_eq!(refused, "“mcpLanguage” must be one of: en, ru.");
            ask(&ctx, &mut app, "set_settings", json!({ "mcpLanguage": "ru" })).unwrap();
            frame(&ctx, &mut app);
            let error = ask(&ctx, &mut app, "dispatch_command", replace.clone()).unwrap_err();
            let error: Value = serde_json::from_str(&error).unwrap();
            assert_eq!((&error["error"], &error["count"]), (&json!("not_unique"), &json!(2)), "{error}");
            assert!(cyrillic(error["message"].as_str().unwrap()), "{error}");
        });
        assert!(!cyrillic(&zen_i18n::with_language("en", || zen_i18n::t!("settings-mcp-language"))));
    }

    #[test]
    fn find_action_finds_every_registered_command_and_runs_it() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut app = IdeApp::new(test_product(), registry, Some(data.path().join("recent.json")), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();

        let actions: Vec<String> = crate::keymap::current().bindings().into_iter().map(|b| b.action).collect();
        let keymap_names = include_str!("../../locales/en/ui.ftl");
        let names = [include_str!("../../locales/en/commands.ftl"), include_str!("../../locales/ru/commands.ftl")];
        let mut listed = 0;
        let mut missing: Vec<String> = Vec::new();
        for info in app.shared.registry.commands() {
            let id = info.id.as_str();
            let dashed = id.replace('.', "-");
            if actions.iter().any(|action| action == id) {
                assert!(keymap_names.contains(&format!("\nkeymap-{dashed} = ")), "{id}");
                continue;
            }
            for names in names {
                if !names.contains(&format!("\ncommand-{dashed} = ")) {
                    missing.push(id.to_owned());
                }
            }
            listed += usize::from(super::super::search_everywhere::command_binding(info).is_some());
        }
        missing.dedup();
        assert!(missing.is_empty(), "commands without a name in Find Action (en and ru): {missing:?}");
        assert!(listed > 80, "{listed} commands listed");
        let run = app.shared.registry.info("http.request.run").unwrap();
        assert!(
            super::super::search_everywhere::command_binding(run).is_none(),
            "Find Action never sends a request by itself"
        );

        for (query, id) in [
            ("select theme", "ui.theme.select"),
            ("import theme", "ui.theme.import"),
            ("reload project model", "project.model.refresh"),
            ("language servers", "language.servers.status"),
            ("command-plugins-modules-list", "plugins.modules.list"),
        ] {
            let query =
                if query.starts_with("command-") { zen_i18n::t!(query).to_lowercase() } else { query.to_owned() };
            let found = ask(&ctx, &mut app, "find_action", json!({ "query": query, "limit": 8 })).unwrap();
            let entry = found["actions"].as_array().unwrap().iter().find(|entry| entry["action"] == id).cloned();
            let entry = entry.unwrap_or_else(|| panic!("{query}: {found}"));
            assert_eq!(entry["command"], id);
        }
        for (query, action, step) in [
            ("zoom in", "editor.image.zoom/in", "in"),
            ("zoom out", "editor.image.zoom/out", "out"),
            ("actual size", "editor.image.zoom/actual", "actual"),
            ("fit zoom", "editor.image.zoom/fit", "fit"),
        ] {
            let found = ask(&ctx, &mut app, "find_action", json!({ "query": query, "limit": 8 })).unwrap();
            let entry = found["actions"].as_array().unwrap().iter().find(|entry| entry["action"] == action).cloned();
            let entry = entry.unwrap_or_else(|| panic!("{query}: {found}"));
            assert_eq!(
                (&entry["command"], &entry["params"]),
                (&json!("editor.image.zoom"), &json!({ "action": step }))
            );
            assert!(!found.to_string().contains("\"action\":\"editor.image.zoom\""), "{found}");
        }
        let found = ask(&ctx, &mut app, "find_action", json!({ "query": "select theme", "limit": 3 })).unwrap();
        let entry = found["actions"].as_array().unwrap().iter().find(|e| e["action"] == "ui.theme.select").unwrap();
        assert_eq!(entry["opens"], "view.settings.open", "{entry}");
        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "ui.theme.select" })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["settings:appearance"]), "{opened}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        for id in ["language.servers.status", "lsp.servers.list"] {
            let found =
                ask(&ctx, &mut app, "find_action", json!({ "query": "language servers", "limit": 20 })).unwrap();
            let entry = found["actions"].as_array().unwrap().iter().find(|e| e["action"] == id).cloned();
            assert_eq!(entry.unwrap_or_else(|| panic!("{id}: {found}"))["opens"], "view.settings.open");
            let opened = ask(&ctx, &mut app, "run_action", json!({ "action": id })).unwrap();
            assert_eq!(opened["window"]["popups"], json!(["settings:languageServers"]), "{opened}");
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            let answer = ask(&ctx, &mut app, "dispatch_command", json!({ "command": id })).unwrap();
            assert!(answer.get("window").is_none() && !answer.is_null(), "{id}: {answer}");
        }
        let ran = ask(&ctx, &mut app, "run_action", json!({ "action": "editor.fold.list" })).unwrap();
        assert!(ran["result"].is_array(), "{ran}");
        let typed = ask(&ctx, &mut app, "find_action", json!({ "query": "type text", "limit": 50 })).unwrap();
        assert!(!typed.to_string().contains("editor.edit.type"), "{typed}");
        let commit = ask(&ctx, &mut app, "find_action", json!({ "query": "commit", "limit": 50 })).unwrap();
        assert!(commit.to_string().contains("view.vcs.commit") && !commit.to_string().contains("vcs.commit.create"));
    }

    #[test]
    fn search_everywhere_commands_calculator_shortcuts_and_abbreviations_work_in_a_window() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });

        let slash = command("search.everywhere.query", json!({ "query": "/" }));
        let answer = ask(&ctx, &mut app, "dispatch_command", slash).unwrap();
        assert_eq!(answer["groups"][0]["contributor"], "commands", "{answer}");
        assert_eq!(answer["groups"][0]["items"].as_array().map(Vec::len), Some(10), "{answer}");
        assert_eq!(answer["groups"][0]["items"][4]["target"], json!({ "kind": "settings", "page": "plugins" }));
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.searchEverywhere.open" })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "text": "/" })).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let opened = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["↓", "⏎"] })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["settings:appearance"]), "{opened}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();

        let sum = command("search.everywhere.query", json!({ "query": "2 + 2 * 2" }));
        let answer = ask(&ctx, &mut app, "dispatch_command", sum).unwrap();
        assert_eq!(answer["groups"][0]["items"][0]["target"], json!({ "kind": "value", "value": "6" }), "{answer}");
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.searchEverywhere.open" })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "text": "2 + 2 * 2" })).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let entered = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        assert_eq!(entered["window"]["popups"], json!([]), "{entered}");
        let list = ask(&ctx, &mut app, "dispatch_command", command("editor.clipboardHistory.list", json!({}))).unwrap();
        assert_eq!(list[0]["preview"], "6", "Enter copied the value: {list}");

        let add = command("ui.keymap.addAbbreviation", json!({ "action": "view.gotoLine.open", "abbreviation": "gl" }));
        ask(&ctx, &mut app, "dispatch_command", add).unwrap();
        let found = ask(&ctx, &mut app, "find_action", json!({ "query": "gl", "limit": 3 })).unwrap();
        assert_eq!(found["actions"][0]["action"], "view.gotoLine.open", "{found}");
        let found = ask(&ctx, &mut app, "find_action", json!({ "query": "preferences", "limit": 3 })).unwrap();
        assert_eq!(found["actions"][0]["action"], "view.settings.open", "{found}");
        let keymap = ask(&ctx, &mut app, "get_keymap", json!({})).unwrap();
        let goto = keymap["actions"].as_array().unwrap().iter().find(|a| a["action"] == "view.gotoLine.open").unwrap();
        assert_eq!(goto["abbreviations"], json!(["gl"]), "{goto}");

        ask(&ctx, &mut app, "run_action", json!({ "action": "view.findAction.open" })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "text": "Clear Search Everywhere History" })).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let recording = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⏎"] })).unwrap();
        assert_eq!(
            recording["window"]["popups"],
            json!(["searchEverywhere:actions", "keyboardShortcut"]),
            "{recording}"
        );
        let pressed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘1"] })).unwrap();
        assert_eq!(pressed["window"]["left"], "project", "the keys go to the dialog, not to the window: {pressed}");
        let cancelled = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(cancelled["window"]["popups"], json!(["searchEverywhere:actions"]), "{cancelled}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();

        let set = json!({ "action": "search.history.clear", "keystrokes": ["⌃⌥⇧H"] });
        let answer = ask(&ctx, &mut app, "set_keymap", set).unwrap();
        assert_eq!(answer["keystrokes"], json!(["⌃⌥⇧H"]), "{answer}");
        assert!(ask(&ctx, &mut app, "set_keymap", json!({ "action": "no.such.thing", "keystrokes": [] })).is_err());
        let remember = command("search.everywhere.query", json!({ "query": "x" }));
        ask(&ctx, &mut app, "dispatch_command", remember).unwrap();
        let mut ran = false;
        let mut shown = Value::Null;
        for _ in 0..5 {
            app.shared.search_history.remember_query("before");
            crate::keymap::load(Arc::clone(&app.shared.keymaps));
            ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌃⌥⇧H"] })).unwrap();
            let query = json!({ "query": "clear search everywhere history" });
            shown = ask(&ctx, &mut app, "find_action", query).unwrap()["actions"][0]["shortcut"].clone();
            if app.shared.search_history.data().queries.is_empty() && shown == "⌃⌥⇧H" {
                ran = true;
                break;
            }
        }
        assert!(ran, "the new shortcut runs the command and shows in Find Action: {shown}");
    }

    #[test]
    fn go_to_text_opens_the_text_tab_and_enter_opens_the_match_in_a_window() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _file) = window_project(data.path());
        std::fs::write(project.join("notes.md"), "Println in the notes\n").unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let find = command("search.text.find", json!({ "query": { "text": "println" } }));
            let found = ask(&ctx, &mut app, "dispatch_command", find).unwrap();
            if found["matches"].as_array().is_some_and(|matches| matches.len() == 2) {
                break;
            }
            assert!(Instant::now() < deadline, "the text index timed out: {found}");
            frame(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(20));
        }

        let all = command("search.everywhere.query", json!({ "query": "println(1)", "waitMs": 5000 }));
        let answer = ask(&ctx, &mut app, "dispatch_command", all).unwrap();
        let groups = answer["groups"].as_array().unwrap();
        assert_eq!(groups.last().map(|group| group["contributor"].clone()), Some(json!("text")), "{answer}");
        assert_eq!(groups.last().unwrap()["items"][0]["hint"], "main.go:4", "{answer}");

        let options = command("search.everywhere.setOptions", json!({ "textMatchCase": true, "textMask": "*.go" }));
        ask(&ctx, &mut app, "dispatch_command", options).unwrap();
        let opened = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⇧⌘E"] })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["searchEverywhere:text"]), "{opened}");
        ask(&ctx, &mut app, "press_keys", json!({ "text": "println" })).unwrap();
        let entered = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        let mut state = entered["window"].clone();
        while state["popups"] != json!([]) {
            assert!(Instant::now() < deadline, "Enter opened nothing: {state}");
            frame(&ctx, &mut app);
            std::thread::sleep(Duration::from_millis(20));
            state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        }
        assert!(state["editor"].as_str().is_some_and(|path| path.ends_with("project/main.go")), "{state}");
        let caret = ask(&ctx, &mut app, "dispatch_command", command("editor.selection.get", json!({}))).unwrap();
        assert_eq!(caret[0]["head"], 29, "the caret at the match: {caret}");
        let history = ask(&ctx, &mut app, "dispatch_command", command("search.findHistory.list", json!({}))).unwrap();
        assert_eq!(history["masks"], json!(["*.go"]), "{history}");
        assert!(data.path().join("find-history.json").exists(), "the history is kept for Find in Files");

        let run = ask(&ctx, &mut app, "run_action", json!({ "action": "view.gotoText.open" })).unwrap();
        assert_eq!(run["window"]["popups"], json!(["searchEverywhere:text"]), "{run}");
        let again = ask(&ctx, &mut app, "run_action", json!({ "action": "view.gotoText.open" })).unwrap();
        assert_eq!(again["window"]["popups"], json!([]), "the shortcut again closes it: {again}");
    }

    #[test]
    fn option_enter_shows_context_actions_in_the_editor_whenever_search_everywhere_is_closed() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let server = zen_lsp::fake::FakeServer::new();
        server.set_capabilities(json!({ "codeActionProvider": true }));
        server.on("textDocument/codeAction", json!([{ "title": "Add a comment", "kind": "refactor" }]));
        let launched = server.clone();
        app.slot_for(None)
            .unwrap()
            .window
            .lock()
            .unwrap()
            .session
            .state_mut::<zen_workbench::language::Languages>()
            .set_specs(vec![zen_workbench::language::ServerSpec {
                language_id: "go".to_owned(),
                extensions: vec!["go".to_owned()],
                launch: Arc::new(move |_| Ok(launched.launcher())),
                initialization_options: json!({}),
            }]);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        let context_actions = |app: &mut IdeApp| {
            app.slot_for(None).unwrap().window.lock().unwrap().view.editor_view.focus();
            for _ in 0..3 {
                frame(&ctx, app);
            }
            ask(&ctx, app, "press_keys", json!({ "keys": ["⌥⏎"] })).unwrap();
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                frame(&ctx, app);
                let popups = ask(&ctx, app, "get_window_state", json!({})).unwrap()["popups"].clone();
                if popups == json!(["intentions"]) {
                    break;
                }
                assert!(Instant::now() < deadline, "⌥⏎ in the editor shows no context actions: {popups}");
                std::thread::sleep(Duration::from_millis(20));
            }
            ask(&ctx, app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
            frame(&ctx, app);
        };
        context_actions(&mut app);

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⇧⌘O"] })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "text": "main.go" })).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let on_file = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⏎"] })).unwrap();
        assert_eq!(on_file["window"]["popups"], json!(["searchEverywhere:files"]), "{on_file}");
        let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(closed["window"]["popups"], json!([]), "{closed}");
        context_actions(&mut app);
    }

    #[test]
    fn every_keymap_action_runs_through_the_automation_layer() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let open = json!({ "command": "file.document.open", "params": { "path": file } });
        let bindings = keymaps::lock(&app.shared.keymaps).active().bindings();
        assert!(bindings.len() > 90, "{} actions", bindings.len());
        for binding in bindings {
            if binding.command.starts_with("db.") && app.shared.registry.info(&binding.command).is_none() {
                continue;
            }
            if matches!(binding.action.as_str(), "view.project.close" | "view.project.closeAll") {
                continue;
            }
            ask(&ctx, &mut app, "dispatch_command", open.clone()).unwrap();
            for (tool, arguments) in [
                ("run_action", json!({ "action": binding.action })),
                ("dispatch_command", json!({ "command": binding.action })),
            ] {
                match ask(&ctx, &mut app, tool, arguments) {
                    Ok(answer) if tool == "dispatch_command" && app.shared.registry.info(&binding.action).is_some() => {
                        assert!(answer.get("window").is_none(), "{}: {answer}", binding.action);
                    }
                    Ok(answer) => {
                        assert!(answer["window"].is_object(), "{}: {answer}", binding.action);
                        assert_eq!(answer["action"], binding.action.as_str());
                    }
                    Err(error) => assert!(
                        !error.contains("unknown command") && !error.contains("no action"),
                        "{} through {tool}: {error}",
                        binding.action
                    ),
                }
                if let Err(error) = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })) {
                    assert_eq!(error, t!("ide-agent-person-answers"), "{}", binding.action);
                    for slot in app.shared.state().windows.clone() {
                        let mut window = slot.window.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                        window.view.restart_prompt = None;
                        window.view.invalidate_dialog = None;
                        window.view.manage_settings = Default::default();
                    }
                }
            }
        }
    }

    fn zoom_pass(ctx: &egui::Context, app: &mut IdeApp) {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.logic(ui.ctx()));
        output.textures_delta.clear();
    }

    fn welcome_frame(app: &mut IdeApp, ui: &mut Ui) {
        app.logic(ui.ctx());
        let overview = app.shared.overview();
        let requests = app.welcome.show(ui, &overview, &mut |_, _| Ok(Value::Null));
        for request in requests {
            app.handle(ui.ctx(), request);
        }
    }

    struct English;

    impl Drop for English {
        fn drop(&mut self) {
            zen_i18n::set_language(Some("en"));
        }
    }

    fn click_node(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, role: egui::accesskit::Role, name: &str) {
        let tree = screen.accessibility_tree(|ui| {
            welcome_frame(app, ui);
        });
        let node = crate::accessibility::testing::find(&tree, role, name)
            .unwrap_or_else(|| panic!("no {role:?} named {name:?} on the welcome screen"));
        let bounds = node.bounds().expect("the node has bounds");
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            screen.step(events, |ui| {
                welcome_frame(app, ui);
            });
        }
    }

    #[test]
    fn the_help_items_are_commands_and_found_by_find_action() {
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let run = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        let popups = |app: &mut IdeApp| ask(&ctx, app, "get_window_state", json!({})).unwrap()["popups"].clone();

        let outside = data.path().join("keys.html");
        let refused = run(&mut app, "view.keyboardShortcuts.export", json!({ "path": outside }));
        assert!(refused.is_err() && !outside.exists(), "an agent writes inside the project");
        let html = project.join("keys.html");
        let done = run(&mut app, "view.keyboardShortcuts.export", json!({ "path": html })).unwrap();
        assert!(done["result"]["path"].as_str().unwrap().ends_with("keys.html"), "{done}");
        assert!(std::fs::read_to_string(&html).unwrap().contains("<table>"));
        let again = run(&mut app, "view.keyboardShortcuts.export", json!({ "path": html }));
        assert!(again.is_err(), "an agent overwrites nothing");
        let pdf = project.join("keys.pdf");
        run(&mut app, "view.keyboardShortcuts.export", json!({ "path": pdf })).unwrap();
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF"));
        if let Ok(dir) = std::env::var("ZEN_UI_SNAPSHOTS") {
            std::fs::copy(&pdf, Path::new(&dir).join("keyboard-shortcuts.pdf")).unwrap();
        }
        let refused = run(&mut app, "view.keyboardShortcuts.export", json!({}));
        assert!(refused.unwrap_err().contains("path"), "agents never get the save dialog");
        let opened = run(&mut app, "view.keyboardShortcuts.open", json!({})).unwrap();
        assert!(opened["result"]["path"].as_str().unwrap().ends_with("keyboard-shortcuts.pdf"));

        let refused = run(&mut app, "view.whatsNew.open", json!({}));
        assert!(refused.is_err(), "no notes are kept yet");

        let tip = run(&mut app, "view.tipOfTheDay.open", json!({ "index": 1 })).unwrap();
        assert_eq!(tip["result"]["action"], "view.findAction.open");
        frame(&ctx, &mut app);
        assert_eq!(popups(&mut app), json!(["tipOfTheDay"]));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert_eq!(popups(&mut app), json!([]));

        let list = run(&mut app, "view.productivity.open", json!({})).unwrap();
        assert_eq!(list["result"]["used"], json!([]), "what an agent runs is not counted");
        assert!(list["result"]["features"].as_u64().unwrap() > 100);
        frame(&ctx, &mut app);
        assert_eq!(popups(&mut app), json!(["myProductivity"]));

        for action in [
            "view.whatsNew.open",
            "view.keyboardShortcuts.open",
            "view.keyboardShortcuts.export",
            "view.tipOfTheDay.open",
            "view.productivity.open",
        ] {
            assert!(crate::keymap::shortcut(action).is_none(), "{action} has no default shortcut");
            let bindings = crate::keymap::current().bindings();
            assert!(bindings.iter().any(|binding| binding.action == action), "Find Action lists the keymap: {action}");
        }
        assert!(super::super::keymap_page::action_title("view.whatsNew.open").starts_with("What's New in "));
    }

    #[test]
    fn whats_new_opens_in_the_first_window_after_an_update_and_only_once() {
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let folder = data.path().to_owned();
        let version = test_product().version.to_owned();
        let note = zen_toolbox::ReleaseNote {
            version: version.clone(),
            date: "2026-11-05".to_owned(),
            notes: "- New".to_owned(),
            installed: true,
        };
        super::super::whats_new::keep_notes(&folder, &[note]);
        std::fs::write(folder.join("whats-new-shown"), "0.0.0-alpha").unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let open = |app: &IdeApp| -> usize {
            let windows = app.shared.state().windows.clone();
            let window = windows[0].window.lock().unwrap();
            let documents = window.session.state::<Workbench>().unwrap().documents();
            documents.iter().filter(|document| document.path().to_string_lossy().contains("release-notes")).count()
        };
        assert_eq!(open(&app), 1);
        assert_eq!(std::fs::read_to_string(folder.join("whats-new-shown")).unwrap(), version);
        app.start_help_done = false;
        frame(&ctx, &mut app);
        assert_eq!(open(&app), 1, "only once");
    }

    #[test]
    fn help_welcome_opens_a_window_of_its_own_over_a_project_and_goes_away_with_a_project() {
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        assert!(!app.welcome_window);
        let answer = ask(
            &ctx,
            &mut app,
            "run_action",
            json!({ "action": "view.welcome.open", "params": { "section": "learn" } }),
        );
        assert_eq!(answer.unwrap()["result"]["section"], "learn");
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        assert!(app.welcome_window, "the project keeps the main window");
        assert_eq!(app.welcome.section, WelcomeSection::Learn);
        let refused =
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.welcome.open", "params": { "section": "ssh" } }));
        assert!(refused.unwrap_err().contains("customize"));
        let open = app.shared.state().windows[0].path.clone();
        app.handle(&ctx, Request::Close(open));
        frame(&ctx, &mut app);
        assert!(!app.welcome_window, "without a project in the main window the welcome window is there");
    }

    #[test]
    fn with_no_project_agents_reach_the_welcome_window() {
        let data = tempfile::tempdir().unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        let state = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert_eq!(state["welcome"]["section"], "projects", "{state}");

        let run = |app: &mut IdeApp, action: &str, params: Value| {
            ask(&ctx, app, "run_action", json!({ "action": action, "params": params }))
        };
        assert_eq!(
            run(&mut app, "view.welcome.open", json!({ "section": "customize" })).unwrap()["section"],
            "customize"
        );
        frame(&ctx, &mut app);
        assert!(!app.welcome_window, "the main window shows it");
        assert_eq!(ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["welcome"]["section"], "customize");

        run(&mut app, "view.ideSettings.import", json!({})).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"], json!(["importSettings"]));
        app.welcome.import = None;

        run(&mut app, "view.settings.open", json!({ "page": "keymap" })).unwrap();
        frame(&ctx, &mut app);
        assert_eq!(ask(&ctx, &mut app, "get_window_state", json!({})).unwrap()["popups"], json!(["settings:keymap"]));

        let url = run(&mut app, "view.guide.open", json!({ "chapter": "keyboard-shortcuts" })).unwrap();
        assert_eq!(url["url"], "https://ide.ifzenelse.space/docs/keyboard-shortcuts.html");

        let themes = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "ui.theme.list" })).unwrap();
        assert!(themes["themes"].is_array(), "{themes}");
    }

    #[test]
    fn the_welcome_screens_language_switch_changes_the_language_and_saves_it() {
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(WELCOME_SIZE[0], WELCOME_SIZE[1]), 1.0);
        screen.render(|ui| {
            welcome_frame(&mut app, ui);
        });
        let switch = zen_i18n::t!("ide-welcome-language");
        click_node(&mut screen, &mut app, egui::accesskit::Role::ComboBox, &switch);
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
            let image = screen.render(|ui| welcome_frame(&mut app, ui));
            crate::offscreen::write_png(&Path::new(&dir).join("welcome-language-menu.png"), &image).unwrap();
        }
        click_node(&mut screen, &mut app, egui::accesskit::Role::MenuItemRadio, "Русский");

        let settings = app.shared.overview().settings;
        assert_eq!(settings.language.as_deref(), Some("ru"), "it is the Language setting");
        assert_eq!(zen_i18n::chosen_language().as_deref(), Some("ru"), "the interface is in Russian");
        assert_ne!(zen_i18n::t!("ide-welcome-language"), switch, "and speaks it");
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(data.path().join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["language"], "ru", "and it is saved: {saved}");

        zen_i18n::set_language(None);
        let _restarted = themed_app(data.path(), Vec::new());
        assert_eq!(zen_i18n::chosen_language().as_deref(), Some("ru"), "remembered across a restart");
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn a_fresh_ide_speaks_english_whatever_the_system_language() {
        let data = tempfile::tempdir().unwrap();
        zen_i18n::set_language(Some("ru"));
        let app = themed_app(data.path(), Vec::new());
        assert_eq!(app.shared.overview().settings.language, None);
        assert_eq!(zen_i18n::chosen_language(), None);
        assert_eq!(zen_i18n::interface_language(), "en", "English even on a Mac set to Russian");
        let system = IdeSettings { language: Some(zen_i18n::SYSTEM.to_owned()), ..IdeSettings::default() };
        apply_interface_language(&system);
        assert_eq!(zen_i18n::interface_language(), zen_i18n::system_language(), "System follows the system");
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn the_welcome_screens_header_never_overlaps_in_any_language_or_width() {
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS");
        let (languages, scale): (&[&str], f32) =
            if snapshots.is_some() { (&["en", "ru"], 2.0) } else { (&["en"], 1.0) };
        for theme in ["zen-dark", "zen-light"] {
            let data = tempfile::tempdir().unwrap();
            std::fs::write(data.path().join("settings.json"), format!(r#"{{"theme": "{theme}"}}"#)).unwrap();
            let mut app = themed_app(data.path(), Vec::new());
            for language in languages {
                zen_i18n::with_language(language, || {
                    for (width, name) in [(WELCOME_SIZE[0], "wide"), (640.0, "narrow")] {
                        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(width, 520.0), scale);
                        let image = screen.render(|ui| {
                            welcome_frame(&mut app, ui);
                        });
                        let tree = screen.accessibility_tree(|ui| {
                            welcome_frame(&mut app, ui);
                        });
                        let rect = |role, name: &str| {
                            let node = crate::accessibility::testing::find(&tree, role, name)
                                .unwrap_or_else(|| panic!("{name} in {language}, {width}"));
                            let b = node.bounds().unwrap();
                            egui::Rect::from_min_max(
                                egui::pos2(b.x0 as f32, b.y0 as f32),
                                egui::pos2(b.x1 as f32, b.y1 as f32),
                            )
                        };
                        let mut controls = vec![
                            rect(egui::accesskit::Role::ComboBox, &zen_i18n::t!("ide-welcome-language")),
                            rect(egui::accesskit::Role::TextInput, &zen_i18n::t!("ide-welcome-search")),
                        ];
                        for label in ["ide-welcome-new", "ide-welcome-open", "ide-welcome-clone"] {
                            controls.push(rect(egui::accesskit::Role::Button, &zen_i18n::t!(label)));
                        }
                        for (index, one) in controls.iter().enumerate() {
                            assert!(one.right() <= width - 19.0, "{one:?} leaves the window ({language}, {width})");
                            for other in &controls[index + 1..] {
                                assert!(
                                    !one.intersects(*other),
                                    "{one:?} overlaps {other:?} ({language}, {theme}, {width})"
                                );
                            }
                        }
                        if let Some(dir) = &snapshots {
                            let path = Path::new(dir).join(format!("welcome-{language}-{theme}-{name}.png"));
                            crate::offscreen::write_png(&path, &image).unwrap();
                        }
                    }
                });
            }
        }
    }

    #[test]
    fn the_welcome_screen_shows_the_products_kozo_above_the_gear() {
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS");
        for (id, name, theme) in [("gogarden", "GoGarden", "zen-dark"), ("test", "Test", "zen-light")] {
            let data = tempfile::tempdir().unwrap();
            std::fs::write(data.path().join("settings.json"), format!(r#"{{"theme": "{theme}"}}"#)).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let product = IdeProduct { id, name, ..test_product() };
            let mut app = IdeApp::new(product, registry, Some(data.path().join("recent.json")), Vec::new());
            let mut screen = crate::offscreen::Offscreen::new(egui::vec2(WELCOME_SIZE[0], WELCOME_SIZE[1]), 2.0);
            let image = screen.render(|ui| welcome_frame(&mut app, ui));
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&Path::new(dir).join(format!("welcome-kozo-{id}.png")), &image).unwrap();
            }
            let (left, top) = (113.0 - 40.0, WELCOME_SIZE[1] - 24.0 - 13.0 - 8.0 - 80.0);
            let at = |x: f32, y: f32| {
                let (x, y) = (((left + x * 2.0 + 1.0) * 2.0) as usize, ((top + y * 2.0 + 1.0) * 2.0) as usize);
                image.pixels[y * image.size[0] + x]
            };
            let variant = zen_mascot::Variant::of(id);
            let [r, g, b] = zen_mascot::Ink::Robe.rgb(variant).unwrap();
            assert_eq!(at(14.0, 24.0), egui::Color32::from_rgb(r, g, b), "{id}: the robe");
            let [r, g, b] = zen_mascot::Ink::Skin.rgb(variant).unwrap();
            assert_eq!(at(20.0, 14.0), egui::Color32::from_rgb(r, g, b), "{id}: his face");
        }
    }

    #[test]
    fn the_ui_font_size_zooms_the_ui_at_start_and_when_it_changes() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), r#"{"uiFontSize": 26}"#).unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        let ctx = egui::Context::default();
        zoom_pass(&ctx, &mut app);
        zoom_pass(&ctx, &mut app);
        assert_eq!(ctx.zoom_factor(), 2.0, "the saved size applies at start");
        assert!(!ctx.options(|options| options.zoom_with_keyboard), "the setting is the only zoom");

        let applied = app.tool(&ctx, "set_settings", &json!({ "uiFontSize": 19.5 })).unwrap();
        assert_eq!(applied["uiFontSize"], 19.5);
        zoom_pass(&ctx, &mut app);
        zoom_pass(&ctx, &mut app);
        assert_eq!(ctx.zoom_factor(), 1.5);

        app.handle(
            &ctx,
            Request::Settings(Box::new(IdeSettings { ui_font_size: None, ..app.shared.overview().settings })),
        );
        zoom_pass(&ctx, &mut app);
        zoom_pass(&ctx, &mut app);
        assert_eq!(ctx.zoom_factor(), 1.0, "the Settings dialog's OK zooms back");
    }

    #[test]
    fn zoom_ide_scales_the_ui_and_the_editor_on_top_of_the_ui_font_size() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        std::fs::write(data.path().join("settings.json"), r#"{"uiFontSize": 26}"#).unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        let slot = app.slot_for(None).unwrap();
        let run = |app: &mut IdeApp, action: &str| ask(&ctx, app, "run_action", json!({ "action": action })).unwrap();
        let line_pixels = |ctx: &egui::Context| {
            let window = slot.window.lock().unwrap();
            super::super::metrics::EditorMetrics::new(ctx, &window.view.overview.settings.editor_font, 4).line_height
                * ctx.zoom_factor()
        };
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert_eq!(ctx.zoom_factor(), 2.0);
        assert_eq!(line_pixels(&ctx), 22.0, "the UI font size leaves the editor alone");

        let zoomed = run(&mut app, "view.ideZoom.in");
        assert_eq!(zoomed["result"]["ideZoom"], 110, "{zoomed}");
        assert_eq!(zoomed["window"]["ideZoom"], 110, "{zoomed}");
        assert_eq!(run(&mut app, "view.ideZoom.in")["result"]["ideZoom"], 125);
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert_eq!(ctx.zoom_factor(), 2.5, "the zoom is on top of the UI font size");
        assert_eq!(line_pixels(&ctx), 27.5, "the editor scales with the zoom");
        let saved = std::fs::read_to_string(app.shared.settings_file.as_ref().unwrap()).unwrap();
        let saved: Value = serde_json::from_str(&saved).unwrap();
        assert_eq!((saved["ideZoom"].clone(), saved["uiFontSize"].clone()), (json!(125), json!(26.0)));

        assert_eq!(run(&mut app, "view.ideZoom.out")["result"]["ideZoom"], 110);
        assert_eq!(run(&mut app, "view.ideZoom.reset")["result"]["ideZoom"], 100);
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert_eq!(ctx.zoom_factor(), 2.0, "Reset leaves the UI font size");

        let opened = run(&mut app, "view.ideZoom.open");
        assert_eq!(opened["window"]["popups"], json!(["zoomIde"]), "{opened}");
        assert_eq!(
            slot.window.lock().unwrap().view.appearance_title("view.ideZoom.open"),
            Some(zen_i18n::t!("menu-view-ideZoom", percent = 100))
        );
        frame(&ctx, &mut app);
        let closed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        assert_eq!(closed["window"]["popups"], json!([]), "{closed}");

        app.tool(&ctx, "set_settings", &json!({ "ideZoom": 150 })).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert_eq!(ctx.zoom_factor(), 3.0);
        assert_eq!(app.tool(&ctx, "get_settings", &json!({})).unwrap()["ideZoom"], 150);
    }

    fn whole_window(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp) -> crate::offscreen::Frame {
        screen.render(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
    }

    pub(super) fn ask_on(
        screen: &mut crate::offscreen::Offscreen,
        app: &mut IdeApp,
        tool: &str,
        arguments: Value,
    ) -> Value {
        let (sender, answer) = mpsc::channel();
        let ctx = screen.context().clone();
        app.route(
            &ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for _ in 0..5 {
            if let Ok(result) = answer.try_recv() {
                return result.unwrap_or_else(|error| panic!("{tool} {arguments}: {error}"));
            }
            screen.step(Vec::new(), |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        }
        panic!("no answer to {tool} {arguments} after 5 frames")
    }

    fn node_rect(
        screen: &mut crate::offscreen::Offscreen,
        app: &mut IdeApp,
        role: egui::accesskit::Role,
        name: &str,
    ) -> Option<egui::Rect> {
        let tree = screen.accessibility_tree(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        let bounds = crate::accessibility::testing::find(&tree, role, name)?.bounds()?;
        Some(egui::Rect::from_min_max(
            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
        ))
    }

    fn click_window(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, at: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            screen.step(events, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
        }
    }

    #[test]
    fn zoom_ide_draws_the_whole_window_at_every_zoom() {
        let _english = English;
        zen_i18n::register(crate::toolbox::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let native = if snapshots.is_some() { 2.0 } else { 1.0 };
        let window = egui::vec2(1440.0, 860.0);
        let percents: &[u16] = if snapshots.is_some() { &[70, 100, 110, 125, 150, 200] } else { &[125, 200] };
        for &percent in percents {
            let data = tempfile::tempdir().unwrap();
            std::fs::write(data.path().join("settings.json"), format!(r#"{{"ideZoom": {percent}}}"#)).unwrap();
            let (project, file) = window_project(data.path());
            let mut app = themed_app(data.path(), vec![project]);
            let zoom = f32::from(percent) / 100.0;
            let mut screen = crate::offscreen::Offscreen::new(window / zoom, native);
            let ctx = screen.context().clone();
            let write = |name: &str, image: &crate::offscreen::Frame| {
                if let Some(dir) = &snapshots {
                    crate::offscreen::write_png(&dir.join(format!("zoom-ide-{name}-{percent}.png")), image).unwrap();
                }
            };
            let _ = whole_window(&mut screen, &mut app);
            let open = json!({ "command": "file.document.open", "params": { "path": file } });
            ask_on(&mut screen, &mut app, "dispatch_command", open);
            let image = whole_window(&mut screen, &mut app);
            assert_eq!(ctx.zoom_factor(), zoom, "the saved zoom applies at start");
            assert_eq!(ctx.content_rect().width() * zoom, window.x, "the window keeps its size");
            let slot = app.slot_for(None).unwrap();
            let line = {
                let view = &slot.window.lock().unwrap().view;
                super::super::metrics::EditorMetrics::new(&ctx, &view.overview.settings.editor_font, 4).line_height
            };
            assert!((line * ctx.zoom_factor() - 22.0 * zoom).abs() < 0.01, "the editor scales: {line} at {percent}");
            write("window", &image);
            if percent == 125 {
                click_window(&mut screen, &mut app, egui::pos2(483.0, 160.0));
                let selection = json!({ "command": "editor.selection.get" });
                let selection = ask_on(&mut screen, &mut app, "dispatch_command", selection);
                assert_eq!((&selection[0]["line"], &selection[0]["column"]), (&json!(3), &json!(1)), "{selection}");
            }
            let title = slot.window.lock().unwrap().view.appearance_title("view.ideZoom.open");
            assert_eq!(title, Some(format!("Zoom IDE (Current: {percent}%)…")));

            ask_on(&mut screen, &mut app, "run_action", json!({ "action": "view.ideZoom.open" }));
            let image = whole_window(&mut screen, &mut app);
            write("dialog", &image);
            let screen_rect = ctx.content_rect();
            let plus = node_rect(&mut screen, &mut app, egui::accesskit::Role::Button, "+");
            let plus = plus.expect("the dialog's + button");
            assert!(screen_rect.contains_rect(plus), "{plus:?} on {screen_rect:?}");
            if percent < 200 {
                click_window(&mut screen, &mut app, plus.center());
                let state = ask_on(&mut screen, &mut app, "get_window_state", json!({}));
                assert!(state["ideZoom"].as_u64() > Some(u64::from(percent)), "+ at {percent}: {state}");
                assert_eq!(state["popups"], json!(["zoomIde"]), "the dialog stays open");
                let reset = json!({ "action": "view.ideZoom.open" });
                ask_on(&mut screen, &mut app, "set_settings", json!({ "ideZoom": percent }));
                ask_on(&mut screen, &mut app, "run_action", reset);
                let _ = whole_window(&mut screen, &mut app);
            }
            ask_on(&mut screen, &mut app, "press_keys", json!({ "keys": ["⎋"] }));

            ask_on(&mut screen, &mut app, "run_action", json!({ "action": "ui.theme.select" }));
            let image = whole_window(&mut screen, &mut app);
            write("settings", &image);
            let ok =
                node_rect(&mut screen, &mut app, egui::accesskit::Role::DefaultButton, &zen_i18n::t!("settings-ok"));
            let ok = ok.expect("the Settings dialog's OK");
            assert!(screen_rect.contains_rect(ok), "OK {ok:?} leaves the window {screen_rect:?} at {percent}");
            ask_on(&mut screen, &mut app, "press_keys", json!({ "keys": ["⎋"] }));

            ask_on(&mut screen, &mut app, "press_keys", json!({ "keys": ["⇧⌘A"], "text": "zoom" }));
            let image = whole_window(&mut screen, &mut app);
            write("find-action", &image);
            ask_on(&mut screen, &mut app, "press_keys", json!({ "keys": ["⎋"] }));
        }
    }

    #[test]
    fn set_settings_merges_deeply_and_get_settings_returns_every_field() {
        let data = tempfile::tempdir().unwrap();
        std::fs::write(
            data.path().join("settings.json"),
            r#"{"ligatures": true, "codeStyle": {"json": {"useTabs": false, "tabSize": 2, "indent": 2}}}"#,
        )
        .unwrap();
        let mut app = themed_app(data.path(), Vec::new());
        let ctx = egui::Context::default();
        let all = app.tool(&ctx, "get_settings", &json!({})).unwrap();
        let properties = super::super::automation::settings_schema()["properties"].clone();
        let mut fields: Vec<&String> = all.as_object().unwrap().keys().collect();
        let mut listed: Vec<&String> = properties.as_object().unwrap().keys().collect();
        fields.sort();
        listed.sort();
        assert_eq!(fields, listed, "{all}");

        app.tool(
            &ctx,
            "set_settings",
            &json!({ "codeStyle": { "go": { "tabSize": 8 } }, "editorFont": { "size": 15 } }),
        )
        .unwrap();
        zoom_pass(&ctx, &mut app);
        let all = app.tool(&ctx, "get_settings", &json!({})).unwrap();
        assert_eq!(all["codeStyle"]["go"], json!({ "useTabs": true, "tabSize": 8, "indent": 4 }));
        assert_eq!(all["codeStyle"]["json"], json!({ "useTabs": false, "tabSize": 2, "indent": 2 }));
        assert_eq!(all["editorFont"], json!({ "family": null, "size": 15.0, "lineSpacing": 1.2_f32 }));
        assert_eq!(all["ligatures"], true);
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(data.path().join("settings.json")).unwrap()).unwrap();
        assert_eq!(saved["codeStyle"], all["codeStyle"], "the merge is saved");

        let refused = app.tool(&ctx, "set_settings", &json!({ "ligatures": false, "editorFont": { "bold": true } }));
        assert!(refused.is_err());
        zoom_pass(&ctx, &mut app);
        assert!(app.shared.overview().settings.ligatures, "a refused call changes nothing");
    }

    #[test]
    fn zen_automation_runs_the_server_for_one_run_and_never_reaches_the_settings_file() {
        let data = tempfile::tempdir().unwrap();
        let dusk = write_dusk(data.path());
        let file = data.path().join("settings.json");
        std::fs::write(&file, r#"{"mcpEnabled": false, "mcpPort": 7000, "ligatures": true}"#).unwrap();
        let project = data.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        app.enable_automation(0);
        let ctx = egui::Context::default();
        zoom_pass(&ctx, &mut app);
        assert!(app.shared.overview().automation.is_some(), "the server runs for this run");

        let windows = app.shared.state().windows.clone();
        {
            let mut window = windows[0].window.lock().unwrap();
            let result = app.shared.registry.dispatch(&mut window.session, "ui.theme.select", json!({ "id": "dusk" }));
            assert!(result.is_ok());
        }
        zoom_pass(&ctx, &mut app);
        assert_eq!(app.palette(), IdePalette::from_theme(&dusk));
        app.tool(&ctx, "set_settings", &json!({ "ligatures": false })).unwrap();
        zoom_pass(&ctx, &mut app);
        app.handle(&ctx, Request::Settings(Box::new(app.shared.overview().settings)));
        zoom_pass(&ctx, &mut app);

        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(saved["theme"], "dusk", "the settings were saved");
        assert_eq!(saved["ligatures"], false);
        assert_eq!(saved["mcpEnabled"], false, "the variable never turns MCP on for the next runs: {saved}");
        assert_eq!(saved["mcpPort"], 7000, "{saved}");
        assert!(app.shared.overview().automation.is_some(), "saving settings keeps this run's server");
        assert!(!app.shared.overview().settings.mcp_enabled, "the settings show the user's own choice");
    }

    mod demo {
        use super::*;
        use zen_core::{Command, CommandError, NoParams};

        pub(super) struct Highlight;

        impl zen_syntax::Highlighter for Highlight {
            fn handles(&self, file_name: &str) -> bool {
                file_name.ends_with(".demo")
            }

            fn highlight(&self, _text: &str) -> Vec<zen_syntax::Span> {
                Vec::new()
            }
        }

        pub(super) struct Choose;

        impl Command for Choose {
            type Params = zen_modules::SwitchParams;
            type Output = ();
            const ID: &'static str = "demo.sdk.choose";
            const TITLE: &'static str = "Choose Demo SDK";
            const DESCRIPTION: &'static str = "Chooses the demo SDK.";

            fn run(&self, _: &mut Session, _: zen_modules::SwitchParams) -> Result<(), CommandError> {
                Ok(())
            }
        }

        pub(super) struct Page;

        impl Command for Page {
            type Params = NoParams;
            type Output = zen_modules::form::Form;
            const ID: &'static str = "demo.settings.page";
            const TITLE: &'static str = "Demo Settings";
            const DESCRIPTION: &'static str = "The demo page.";

            fn run(&self, _: &mut Session, _: NoParams) -> Result<zen_modules::form::Form, CommandError> {
                Ok(zen_modules::form::Form::default())
            }
        }

        fn register(host: &mut zen_modules::ModuleHost<'_>) -> Result<(), zen_core::RegistryError> {
            host.commands().register(Choose)?;
            host.commands().register(Page)?;
            host.highlighter(Arc::new(Highlight));
            host.settings_page(zen_modules::SettingsPage {
                id: "demo",
                title: "settings-languages",
                search: &[],
                form: "demo.settings.page",
                commands: &["demo.sdk."],
                parent: "",
            });
            Ok(())
        }

        pub(super) static MODULE: zen_modules::ModuleDescriptor = zen_modules::ModuleDescriptor {
            id: "demo",
            name: "settings-languages",
            description: "settings-languages",
            locales: &[],
            register,
        };
    }

    #[test]
    fn a_built_in_module_switched_on_is_there_after_a_restart_and_off_again_is_gone() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let start = || {
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let product =
                IdeProduct { modules: vec![zen_modules::BuiltinModule::off(&demo::MODULE)], ..test_product() };
            IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project.clone()])
        };
        let ctx = egui::Context::default();
        let dispatch = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };

        let mut app = start();
        frame(&ctx, &mut app);
        assert!(app.shared.registry.info("demo.sdk.choose").is_none(), "off by default: not registered");
        let pages = || app.shared.product.settings_pages.iter().map(|page| page.id).collect::<Vec<_>>();
        assert_eq!(
            pages(),
            [
                zen_workbench::code_quality::PAGE,
                zen_workbench::jdk::PAGE,
                zen_workbench::maven::settings::BUILD_TOOLS_PAGE,
                zen_workbench::maven::settings::PAGE,
                zen_workbench::maven::settings::IMPORTING_PAGE,
                zen_workbench::maven::settings::RUNNER_PAGE,
                zen_workbench::maven::settings::TESTS_PAGE,
                zen_workbench::gradle::settings::PAGE,
                zen_workbench::java::processing::COMPILER_PAGE,
                zen_workbench::java::processing::PAGE,
            ],
            "the platform's pages only"
        );
        assert!(app.shared.product.highlighters.is_empty());
        let enabled = dispatch(&mut app, "plugins.module.enable", json!({ "id": "demo" })).unwrap();
        assert_eq!((enabled["enabled"].clone(), enabled["restartRequired"].clone()), (json!(true), json!(true)));
        assert!(app.shared.registry.info("demo.sdk.choose").is_none(), "only a restart applies it");
        drop(app);

        let mut app = start();
        frame(&ctx, &mut app);
        assert!(app.shared.registry.info("demo.sdk.choose").is_some());
        assert!(app.shared.product.highlighters.iter().any(|highlighter| highlighter.handles("a.demo")));
        let listed = dispatch(&mut app, "plugins.modules.list", json!({})).unwrap();
        assert_eq!(listed["modules"][0]["active"], true, "{listed}");
        let opened = ask(&ctx, &mut app, "run_action", json!({ "action": "demo.sdk.choose" })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["settings:demo"]), "{opened}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        dispatch(&mut app, "plugins.module.disable", json!({ "id": "demo" })).unwrap();
        drop(app);

        let mut app = start();
        frame(&ctx, &mut app);
        assert!(app.shared.registry.info("demo.sdk.choose").is_none(), "switched off: gone after the restart");
    }

    #[test]
    fn a_product_with_areas_offers_them_on_its_first_start() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let bundle = data.path().join("bundle").join("markdown");
        std::fs::create_dir_all(&bundle).unwrap();
        let manifest = json!({ "id": "markdown", "name": "Markdown", "description": "Markdown files.",
            "version": "0.1.0", "extensions": ["md"] });
        std::fs::write(bundle.join("plugin.json"), manifest.to_string()).unwrap();
        std::fs::write(bundle.join("plugin.wasm"), crate::plugins::fake::EMPTY_MODULE).unwrap();
        let start = || {
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let product = IdeProduct {
                name: "TestTotami",
                modules: vec![zen_modules::BuiltinModule::on(&demo::MODULE)],
                areas: vec!["demo", "markdown", "not-in-this-bundle"],
                plugins: Some(crate::plugins::PluginConfig {
                    folder: data.path().join("plugins"),
                    catalog_url: None,
                    public_key: String::new(),
                    bundled: Some(data.path().join("bundle")),
                    optional: Vec::new(),
                }),
                ..test_product()
            };
            IdeApp::new(product, registry, Some(data.path().join("recent.json")), vec![project.clone()])
        };
        let ctx = egui::Context::default();
        let dispatch = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params }))
        };

        let mut app = start();
        frame(&ctx, &mut app);
        assert!(app.areas_dialog.is_some(), "the first start offers the areas");
        assert!(app.shared.registry.info("demo.sdk.choose").is_none(), "an area is off until chosen");
        let listed = dispatch(&mut app, "plugins.areas.list", json!({})).unwrap();
        assert_eq!((&listed["offered"], &listed["dialogOpen"]), (&json!(false), &json!(true)), "{listed}");
        let areas: Vec<(&str, &str, bool)> = listed["areas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|area| (area["id"].as_str().unwrap(), area["kind"].as_str().unwrap(), area["enabled"] == true))
            .collect();
        assert_eq!(areas, [("demo", "module", false), ("markdown", "plugin", false)]);
        assert!(app.shared.plugins.highlighters().is_empty(), "the bundled area is not loaded");

        let chosen = dispatch(&mut app, "plugins.areas.choose", json!({ "areas": ["demo", "markdown"] })).unwrap();
        assert_eq!(chosen["restartRequired"], json!([zen_i18n::t!("settings-languages")]), "{chosen}");
        frame(&ctx, &mut app);
        assert!(app.areas_dialog.is_none(), "a choice made by an agent closes the dialog");
        assert_eq!(app.shared.plugins.highlighters().len(), 1, "a bundled plug-in is on at once");
        drop(app);

        let mut app = start();
        frame(&ctx, &mut app);
        assert!(app.areas_dialog.is_none(), "the choice is asked once");
        assert!(app.shared.registry.info("demo.sdk.choose").is_some(), "the module is on after the restart");
        dispatch(&mut app, "plugins.areas.show", json!({})).unwrap();
        frame(&ctx, &mut app);
        assert!(app.areas_dialog.is_some(), "the choice shows again on request");
        dispatch(&mut app, "plugins.areas.skip", json!({})).unwrap();
        frame(&ctx, &mut app);
        assert!(app.areas_dialog.is_none());
    }

    #[test]
    fn without_a_toolchain_the_windows_get_the_defaults() {
        assert_eq!(toolchain_model_env(None), zen_workbench::model::ModelEnv::default());
        assert_eq!(toolchain_run_env(None), zen_run::RunEnv::default());
        assert_eq!(toolchain_debugger(None), PathBuf::new());
    }

    fn assert_clean(what: &str, text: &str) {
        assert!(!text.is_empty(), "{what} has no description");
        assert_eq!(text, text.trim(), "{what}: whitespace around the description");
        for bad in ["\\", "\n", "\r", "\t", "  "] {
            assert!(!text.contains(bad), "{what}: {bad:?} in the description {text:?}");
        }
    }

    fn assert_clean_schema(what: &str, schema: &Value) {
        match schema {
            Value::Object(fields) => {
                for (key, value) in fields {
                    match value {
                        Value::String(text) if key == "description" => assert_clean(what, text),
                        _ => assert_clean_schema(&format!("{what}.{key}"), value),
                    }
                }
            }
            Value::Array(items) => items.iter().for_each(|item| assert_clean_schema(what, item)),
            _ => {}
        }
    }

    #[test]
    fn every_command_and_tool_has_a_clean_description() {
        let data = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        zen_toolbox::register(&mut registry).unwrap();
        let app = IdeApp::new(test_product(), registry, Some(data.path().join("recent.json")), Vec::new());
        let mut count = 0;
        for info in app.shared.registry.commands() {
            assert_clean(info.id.as_str(), &info.description);
            count += 1;
        }
        assert!(count > 100, "{count} commands");
        for tool in super::super::automation::tools() {
            assert_clean(&tool.name, &tool.description);
            assert_clean_schema(&tool.name, &tool.input_schema);
        }
    }

    #[test]
    fn every_command_parameter_has_a_clean_description() {
        let data = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        zen_toolbox::register(&mut registry).unwrap();
        let app = IdeApp::new(test_product(), registry, Some(data.path().join("recent.json")), Vec::new());
        let mut described = 0;
        for info in app.shared.registry.commands() {
            assert_clean_schema(&format!("{} params", info.id), &info.params_schema);
            described += info.params_schema.to_string().matches("\"description\":").count();
        }
        assert!(described > 500, "{described} parameter descriptions");
    }

    #[cfg(unix)]
    pub(super) fn test_shell(home: &Path) -> zen_workbench::terminal::Launcher {
        let home = format!("HOME={}", home.display());
        Arc::new(move |cwd: &Path| {
            zen_terminal::Options::command(
                "/usr/bin/env",
                &["-i", &home, "PATH=/usr/bin:/bin", "PS1=$ ", "/bin/sh"],
                cwd,
            )
        })
    }

    #[cfg(unix)]
    pub(super) fn with_terminals<T>(app: &IdeApp, run: impl FnOnce(&mut ProjectWindow) -> T) -> T {
        let windows = app.shared.state().windows.clone();
        let mut window = windows[0].window.lock().unwrap();
        run(&mut window)
    }

    #[cfg(unix)]
    fn screen_of(ctx: &egui::Context, app: &mut IdeApp, id: u64) -> String {
        let read =
            ask(ctx, app, "dispatch_command", json!({ "command": "terminal.session.read", "params": { "id": id } }));
        read.unwrap()["text"].as_str().unwrap().to_owned()
    }

    #[cfg(unix)]
    fn wait_screen(ctx: &egui::Context, app: &mut IdeApp, id: u64, needle: &str) {
        let start = Instant::now();
        while !screen_of(ctx, app, id).contains(needle) {
            assert!(
                start.elapsed() < Duration::from_secs(60),
                "no {needle:?} in session {id}: {}",
                screen_of(ctx, app, id)
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_terminal_link_opens_only_on_command_click_and_asks_first() {
        use zen_workbench::terminal::Terminals;
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        app.splash = super::super::splash::Splash::finished();
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        with_terminals(&app, |window| window.session.state_mut::<Terminals>().set_launcher(test_shell(data.path())));
        let id = ask(&ctx, &mut app, "terminal_open", json!({})).unwrap()["id"].as_u64().unwrap();
        ask(
            &ctx,
            &mut app,
            "terminal_write",
            json!({ "text": "printf 'go https://example.com/x\\nmain.go:4:2\\n'\n" }),
        )
        .unwrap();
        wait_screen(&ctx, &mut app, id, "main.go:4:2\n");
        let list = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "terminal.link.list" })).unwrap();
        let links = list["links"].as_array().unwrap().clone();
        let place = |text: &str| {
            let link = links.iter().rev().find(|link| link["text"] == text).unwrap_or_else(|| panic!("{list}"));
            (link["row"].as_u64().unwrap() as usize - 1, link["startColumn"].as_u64().unwrap() as usize)
        };
        let ((url_row, url_col), (file_row, file_col)) = (place("https://example.com/x"), place("main.go:4:2"));
        frame(&ctx, &mut app);
        let center = |row: usize, col: usize| {
            with_terminals(&app, |window| window.view.terminal_panes[&id].cell_center(row, col).unwrap())
        };
        let (url_at, file_at) = (center(url_row, url_col + 2), center(file_row, file_col + 2));
        let click = |at: egui::Pos2, modifiers: egui::Modifiers| {
            [true, false]
                .map(|pressed| egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers,
                })
                .to_vec()
        };
        let hovered =
            |app: &IdeApp| with_terminals(app, |window| window.view.terminal_panes[&id].hovered_link().is_some());
        let dialog =
            |app: &IdeApp| with_terminals(app, |window| window.view.terminal_link.as_ref().map(|d| d.click.kind));

        let none = egui::Modifiers::NONE;
        frame_with(&ctx, &mut app, vec![egui::Event::ModifiersChanged(none), egui::Event::PointerMoved(url_at)]);
        frame_with(&ctx, &mut app, vec![egui::Event::PointerMoved(url_at)]);
        assert!(!hovered(&app));
        frame_with(&ctx, &mut app, click(url_at, none));
        frame(&ctx, &mut app);
        assert_eq!(dialog(&app), None, "a plain click never opens a link");

        let command = egui::Modifiers::COMMAND;
        frame_with(&ctx, &mut app, vec![egui::Event::ModifiersChanged(command), egui::Event::PointerMoved(url_at)]);
        assert!(hovered(&app));
        frame_with(&ctx, &mut app, click(url_at, command));
        frame(&ctx, &mut app);
        assert_eq!(dialog(&app), Some(zen_terminal::links::LinkKind::Url));
        frame_with(&ctx, &mut app, vec![egui::Event::ModifiersChanged(none)]);
        assert!(!hovered(&app), "releasing ⌘ takes the underline away");
        frame_with(&ctx, &mut app, press(egui::Key::Escape, none));
        frame(&ctx, &mut app);
        assert_eq!(dialog(&app), None);

        frame_with(&ctx, &mut app, vec![egui::Event::ModifiersChanged(command), egui::Event::PointerMoved(file_at)]);
        frame_with(&ctx, &mut app, click(file_at, command));
        frame(&ctx, &mut app);
        assert_eq!(dialog(&app), Some(zen_terminal::links::LinkKind::File));
        with_terminals(&app, |window| window.view.terminal_link.as_mut().unwrap().remember = true);
        frame_with(&ctx, &mut app, press(egui::Key::Enter, none));
        frame(&ctx, &mut app);
        assert_eq!(dialog(&app), None);
        let active = with_terminals(&app, |window| {
            let document = window.session.state::<Workbench>().unwrap().active_document().unwrap();
            let point = document.snapshot().offset_to_point(document.selections().primary().head).unwrap();
            (document.path().canonicalize().unwrap(), point.line, point.column)
        });
        assert_eq!(active, (file.canonicalize().unwrap(), 3, 1), "main.go at line 4, column 2");
        let settings = ask(&ctx, &mut app, "get_settings", json!({})).unwrap();
        assert_eq!(settings["terminalLinks"], json!({ "url": null, "file": "openInEditor" }));
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.close", "params": {} })).ok();
        frame_with(&ctx, &mut app, vec![egui::Event::ModifiersChanged(command), egui::Event::PointerMoved(file_at)]);
        frame_with(&ctx, &mut app, vec![egui::Event::ModifiersChanged(command), egui::Event::PointerMoved(file_at)]);
        frame_with(&ctx, &mut app, click(file_at, command));
        frame(&ctx, &mut app);
        assert_eq!(dialog(&app), None, "the remembered choice does not ask");
        let answer = ask(&ctx, &mut app, "set_settings", json!({ "terminalLinks": null })).unwrap();
        assert_eq!(answer["terminalLinks"], json!({ "url": null, "file": null }), "{answer}");
        frame(&ctx, &mut app);
        let settings = ask(&ctx, &mut app, "get_settings", json!({})).unwrap();
        assert_eq!(settings["terminalLinks"], json!({ "url": null, "file": null }), "asking again");
    }

    #[cfg(unix)]
    #[test]
    fn terminal_tabs_split_panes_and_the_keyboard() {
        use zen_workbench::terminal::Terminals;
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        with_terminals(&app, |window| window.session.state_mut::<Terminals>().set_launcher(test_shell(data.path())));

        let opened = ask(&ctx, &mut app, "terminal_open", json!({})).unwrap();
        let first = opened["id"].as_u64().unwrap();
        let split = json!({ "command": "terminal.session.split", "params": { "direction": "right" } });
        let second = ask(&ctx, &mut app, "dispatch_command", split).unwrap()["id"].as_u64().unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let (drawn, shown) =
            with_terminals(&app, |window| (window.view.terminal_drawn.clone(), window.view.tool_visible("terminal")));
        assert!(shown, "a new session shows the tool window");
        assert_eq!(drawn.iter().map(|(id, ..)| *id).collect::<Vec<_>>(), [first, second], "both panes are painted");
        let (left, right) = (drawn[0].2, drawn[1].2);
        assert!(left.right() <= right.left() && left.width() > 50.0 && right.width() > 50.0, "{left:?} {right:?}");
        assert_eq!(left.top(), right.top(), "side by side");
        assert!(ctx.memory(|memory| memory.has_focus(drawn[1].1)), "the new pane has the keyboard");

        ask(&ctx, &mut app, "press_keys", json!({ "text": "echo typed-$((40+2))\n" })).unwrap();
        frame(&ctx, &mut app);
        wait_screen(&ctx, &mut app, second, "typed-42");
        assert!(!screen_of(&ctx, &mut app, first).contains("typed"), "the other pane got nothing");

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⇥"] })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "text": "echo left\n" })).unwrap();
        frame(&ctx, &mut app);
        wait_screen(&ctx, &mut app, first, "left");
        assert!(!screen_of(&ctx, &mut app, second).contains("left"), "⌥⇥ moved the keyboard to the other pane");

        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘T"] })).unwrap();
        frame(&ctx, &mut app);
        let list = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "terminal.session.list" })).unwrap();
        assert_eq!(list["tabs"].as_array().unwrap().len(), 2, "⌘T in the terminal opens a tab: {list}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌃D"] })).unwrap();
        let start = Instant::now();
        loop {
            frame(&ctx, &mut app);
            let list = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "terminal.session.list" })).unwrap();
            if list["tabs"].as_array().unwrap().len() == 1 {
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(60), "⌃D reaches the shell, which ends: {list}");
            std::thread::sleep(Duration::from_millis(10));
        }
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘T"] })).unwrap();
        frame(&ctx, &mut app);
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘W"] })).unwrap();
        frame(&ctx, &mut app);
        let list = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "terminal.session.list" })).unwrap();
        assert_eq!(list["tabs"].as_array().unwrap().len(), 1, "⌘W in the terminal closes its session: {list}");
        ask(
            &ctx,
            &mut app,
            "dispatch_command",
            json!({ "command": "terminal.tab.rename", "params": { "name": "dev" } }),
        )
        .unwrap();

        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "view.toolWindow.terminal" })).unwrap();
        frame(&ctx, &mut app);
        assert!(!with_terminals(&app, |window| window.view.tool_visible("terminal")));
        ask(&ctx, &mut app, "terminal_write", json!({ "text": "echo hidden\n" })).unwrap();
        wait_screen(&ctx, &mut app, first, "hidden");
        assert!(
            with_terminals(&app, |window| window.view.terminal_drawn.is_empty()),
            "nothing is painted while hidden"
        );

        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "view.toolWindow.terminal" })).unwrap();
        frame(&ctx, &mut app);
        app.remember_workspace(&ctx, true);
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(data.path().join("workspace.json")).unwrap()).unwrap();
        let layout = saved["projects"].as_object().and_then(|projects| projects.values().next()).unwrap();
        assert_eq!(layout["terminal"][0]["name"], "dev", "{layout}");
        assert_eq!(layout["terminal"][0]["layout"]["direction"], "right");
        drop(app);

        let mut app = themed_app(data.path(), vec![project.clone()]);
        with_terminals(&app, |window| {
            let terminals = window.session.state_mut::<Terminals>();
            assert!(terminals.tabs().is_empty() && terminals.has_pending(), "no process starts with the window");
            terminals.set_launcher(test_shell(data.path()));
        });
        frame(&ctx, &mut app);
        frame(&ctx, &mut app);
        let list = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "terminal.session.list" })).unwrap();
        assert_eq!(list["tabs"][0]["name"], "dev", "{list}");
        assert_eq!(list["tabs"][0]["layout"]["direction"], "right");
        assert_eq!(list["tabs"][0]["sessions"][0]["cwd"], json!(project.canonicalize().unwrap()));
        assert_eq!(with_terminals(&app, |window| window.view.terminal_drawn.len()), 2);
    }

    #[cfg(unix)]
    #[test]
    fn terminal_tabs_of_a_hidden_terminal_are_there_for_agents_and_saved_at_exit() {
        use zen_workbench::terminal::Terminals;
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        frame(&ctx, &mut app);
        with_terminals(&app, |window| window.session.state_mut::<Terminals>().set_launcher(test_shell(data.path())));
        ask(&ctx, &mut app, "terminal_open", json!({})).unwrap();
        let rename = json!({ "command": "terminal.tab.rename", "params": { "name": "Work" } });
        ask(&ctx, &mut app, "dispatch_command", rename).unwrap();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert!(with_terminals(&app, |window| window.view.tool_visible("terminal")));
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "view.toolWindow.terminal" })).unwrap();
        frame(&ctx, &mut app);
        assert!(!with_terminals(&app, |window| window.view.tool_visible("terminal")));
        app.remember_workspace(&ctx, true);
        drop(app);

        let mut app = themed_app(data.path(), vec![project.clone()]);
        with_terminals(&app, |window| window.session.state_mut::<Terminals>().set_launcher(test_shell(data.path())));
        frame(&ctx, &mut app);
        let list = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "terminal.session.list" })).unwrap();
        assert_eq!(list["tabs"][0]["name"], "Work", "the remembered tab is listed: {list}");
        assert!(!with_terminals(&app, |window| window.view.tool_visible("terminal")), "and the Terminal stays hidden");
        ask(&ctx, &mut app, "terminal_write", json!({ "text": "echo still-$((6*7))\n" })).unwrap();
        let id = list["tabs"][0]["focused"].as_u64().unwrap();
        wait_screen(&ctx, &mut app, id, "still-42");

        app.remember_workspace(&ctx, true);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "terminal.session.new" })).unwrap();
        let answer = ask(&ctx, &mut app, "exit_ide", json!({ "confirmed": true })).unwrap();
        assert_eq!(answer["exiting"], true);
        app.exit_at = Some(f64::MIN);
        frame(&ctx, &mut app);
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(data.path().join("workspace.json")).unwrap()).unwrap();
        let layout = saved["projects"].as_object().and_then(|projects| projects.values().next()).unwrap();
        assert_eq!(layout["terminal"].as_array().map(Vec::len), Some(2), "the tab just opened is saved: {layout}");
    }

    fn lock_frame(
        screen: &mut crate::offscreen::Offscreen,
        app: &mut IdeApp,
        events: Vec<egui::Event>,
        paint: bool,
    ) -> Option<crate::offscreen::Frame> {
        let mut draw = |ui: &mut Ui| {
            app.logic(ui.ctx());
            app.shared.palette().apply(ui.ctx());
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            let ProjectWindow { session, view, .. } = &mut *window;
            view.overview = app.shared.overview();
            view.show(ui, &app.shared.registry, session);
            let requests = std::mem::take(&mut view.requests);
            drop(window);
            app.shared.submit(ui.ctx(), requests);
        };
        if paint {
            Some(screen.frame(events, &mut draw))
        } else {
            screen.step(events, &mut draw);
            None
        }
    }

    fn lock_centre(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str) -> egui::Pos2 {
        let tree = screen.accessibility_tree(|ui| {
            app.logic(ui.ctx());
            let slot = app.slot_for(None).unwrap();
            let mut window = slot.window.lock().unwrap();
            let ProjectWindow { session, view, .. } = &mut *window;
            view.show(ui, &app.shared.registry, session);
            view.requests.clear();
        });
        let node = crate::accessibility::testing::find(&tree, egui::accesskit::Role::Button, name)
            .unwrap_or_else(|| panic!("no lock named {name:?} in the status bar"));
        let b = node.bounds().expect("the lock has bounds");
        egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)
    }

    fn lock_ask(
        screen: &mut crate::offscreen::Offscreen,
        app: &mut IdeApp,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        let (sender, answer) = mpsc::channel();
        let ctx = screen.context().clone();
        app.route(
            &ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for _ in 0..5 {
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            lock_frame(screen, app, Vec::new(), false);
        }
        answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments} after 5 frames"))
    }

    fn window_session<T>(app: &IdeApp, read: impl FnOnce(&Session) -> T) -> T {
        let slot = app.slot_for(None).unwrap();
        let window = slot.window.lock().unwrap();
        read(&window.session)
    }

    #[test]
    fn the_status_bar_lock_toggles_the_write_permission_and_typing_offers_make_writable() {
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::register(crate::toolbox::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let _english = snapshots.is_some().then_some(English);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        let scale = 2.0;
        for language in languages {
            let data = tempfile::tempdir().unwrap();
            let (project, file) = window_project(data.path());
            let before = std::fs::read_to_string(&file).unwrap();
            let mut app = themed_app(data.path(), vec![project.clone()]);
            let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 560.0), scale);
            lock_frame(&mut screen, &mut app, Vec::new(), false);
            if snapshots.is_some() {
                zen_i18n::set_language(Some(language));
            }
            let open_file = json!({ "command": "file.document.open", "params": { "path": file } });
            lock_ask(&mut screen, &mut app, "dispatch_command", open_file).unwrap();
            for _ in 0..3 {
                lock_frame(&mut screen, &mut app, Vec::new(), false);
            }
            let snap = |name: &str, image: &crate::offscreen::Frame| {
                if let Some(dir) = &snapshots {
                    let path = dir.join(format!("read-only-{name}-{language}.png"));
                    crate::offscreen::write_png(&path, image).unwrap();
                }
            };
            let click = |at: egui::Pos2, pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };

            let open = lock_centre(&mut screen, &mut app, &zen_i18n::t!("ide-status-writable"));
            lock_frame(&mut screen, &mut app, vec![egui::Event::PointerMoved(open)], false);
            for _ in 0..40 {
                lock_frame(&mut screen, &mut app, Vec::new(), false);
            }
            snap("open-hover", &lock_frame(&mut screen, &mut app, Vec::new(), true).unwrap());
            lock_frame(&mut screen, &mut app, vec![click(open, true)], false);
            lock_frame(&mut screen, &mut app, vec![click(open, false)], false);
            assert!(!zen_vfs::is_writable(&file), "a click on the open lock makes the file read-only on disk");

            let closed = lock_centre(&mut screen, &mut app, &zen_i18n::t!("ide-status-read-only"));
            lock_frame(&mut screen, &mut app, vec![egui::Event::PointerMoved(closed + egui::vec2(0.0, 1.0))], false);
            for _ in 0..40 {
                lock_frame(&mut screen, &mut app, Vec::new(), false);
            }
            snap("closed-hover", &lock_frame(&mut screen, &mut app, Vec::new(), true).unwrap());
            let editor = egui::pos2(600.0, 200.0);
            lock_frame(&mut screen, &mut app, vec![egui::Event::PointerMoved(editor)], false);
            lock_frame(&mut screen, &mut app, vec![click(editor, true)], false);
            lock_frame(&mut screen, &mut app, vec![click(editor, false)], false);

            lock_ask(&mut screen, &mut app, "press_keys", json!({ "text": "x" })).unwrap();
            lock_frame(&mut screen, &mut app, vec![egui::Event::Text("y".to_owned())], false);
            for _ in 0..3 {
                lock_frame(&mut screen, &mut app, Vec::new(), false);
            }
            untouched(&app, "typing into a read-only file");
            assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
            let listed =
                lock_ask(&mut screen, &mut app, "dispatch_command", json!({ "command": "ui.notifications.list" }))
                    .unwrap();
            let offers: Vec<&Value> = listed["notifications"]
                .as_array()
                .or_else(|| listed.as_array())
                .map(|items| items.iter().filter(|item| item["actions"][0]["id"] == "makeWritable").collect())
                .unwrap_or_default();
            assert_eq!(offers.len(), 1, "one offer for both keystrokes: {listed}");
            assert_eq!(offers[0]["type"], "warning");
            let id = offers[0]["id"].clone();
            snap("make-writable", &lock_frame(&mut screen, &mut app, Vec::new(), true).unwrap());

            let params = json!({ "id": id, "action": "makeWritable" });
            lock_ask(
                &mut screen,
                &mut app,
                "dispatch_command",
                json!({ "command": "ui.notifications.action", "params": params }),
            )
            .unwrap();
            for _ in 0..3 {
                lock_frame(&mut screen, &mut app, Vec::new(), false);
            }
            assert!(zen_vfs::is_writable(&file), "Make Writable makes the file writable");
            lock_ask(&mut screen, &mut app, "press_keys", json!({ "text": "z" })).unwrap();
            let text = window_session(&app, |session| {
                session.state::<Workbench>().unwrap().active_document().unwrap().snapshot().to_string()
            });
            assert_eq!(text, format!("{before}z"), "a writable file is edited again");
        }
    }

    #[test]
    fn font_size_in_all_editors_from_the_actions_and_their_shortcuts_is_saved() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        let size = |app: &mut IdeApp| ask(&ctx, app, "get_settings", json!({})).unwrap()["editorFont"]["size"].clone();
        let bigger = ask(&ctx, &mut app, "run_action", json!({ "action": "view.editorFontSize.increase" })).unwrap();
        assert_eq!(bigger["result"], json!({ "size": 14.0, "default": 13.0 }), "{bigger}");
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert_eq!(size(&mut app), json!(14.0));
        let saved = std::fs::read_to_string(app.shared.settings_file.as_ref().unwrap()).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&saved).unwrap()["editorFont"]["size"], 14.0, "{saved}");
        let file_size = |app: &mut IdeApp| {
            ask(&ctx, app, "dispatch_command", json!({ "command": "editor.fontSize.get" })).unwrap()["size"].clone()
        };
        assert_eq!(file_size(&mut app), json!(14.0), "the editor shows the setting");
        let slot = app.slot_for(None).unwrap();
        let shown = || slot.window.lock().unwrap().view.editor_view.font.size;
        assert_eq!(shown(), 14.0);
        let now = ctx.input(|input| input.time);
        assert!(slot.window.lock().unwrap().view.font_zoom.indicator_until() > now, "the zoom indicator shows");

        for keys in [json!(["⌃⇧."]), json!(["⌃⇧."]), json!(["⌃⇧,"])] {
            ask(&ctx, &mut app, "press_keys", json!({ "keys": keys })).unwrap();
            for _ in 0..2 {
                frame(&ctx, &mut app);
            }
        }
        assert_eq!(size(&mut app), json!(15.0), "⌃⇧. twice and ⌃⇧, once");
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.editorFontSize.reset" })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert_eq!(size(&mut app), json!(13.0));
        assert_eq!(shown(), 13.0);
    }

    #[test]
    fn font_size_of_the_active_editor_and_the_command_wheel() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
        let clock = std::cell::Cell::new(0.0);
        let step = |app: &mut IdeApp, events: Vec<egui::Event>| {
            clock.set(clock.get() + 1.0);
            let input =
                egui::RawInput { screen_rect: Some(screen), events, time: Some(clock.get()), ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            output.textures_delta.clear();
        };
        for _ in 0..10 {
            step(&mut app, Vec::new());
        }
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        let bigger = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "editor.fontSize.increase" })).unwrap();
        assert_eq!((bigger["size"].clone(), bigger["changed"].clone()), (json!(14.0), json!(true)), "{bigger}");
        step(&mut app, Vec::new());
        let slot = app.slot_for(None).unwrap();
        assert_eq!(slot.window.lock().unwrap().view.editor_view.font.size, 14.0, "the editor draws the file's size");
        let settings = ask(&ctx, &mut app, "get_settings", json!({})).unwrap();
        assert_eq!(settings["editorFont"]["size"], 13.0, "the setting stays");
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "editor.fontSize.reset" })).unwrap();

        let file_size = |app: &mut IdeApp| {
            ask(&ctx, app, "dispatch_command", json!({ "command": "editor.fontSize.get" })).unwrap()["size"].clone()
        };
        let at = egui::pos2(800.0, 400.0);
        let wheel = |lines: f32| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: egui::vec2(0.0, lines),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                    modifiers: egui::Modifiers::COMMAND,
                },
            ]
        };
        step(&mut app, wheel(1.0));
        step(&mut app, Vec::new());
        assert_eq!(file_size(&mut app), json!(13.0), "off by default");

        app.tool(&ctx, "set_settings", &json!({ "wheelFontSize": true })).unwrap();
        step(&mut app, Vec::new());
        for _ in 0..2 {
            step(&mut app, wheel(1.0));
        }
        step(&mut app, Vec::new());
        assert_eq!(file_size(&mut app), json!(15.0), "two notches up in the active editor");
        assert_eq!(ask(&ctx, &mut app, "get_settings", json!({})).unwrap()["editorFont"]["size"], 13.0);
        step(&mut app, wheel(-1.0));
        assert_eq!(file_size(&mut app), json!(14.0));

        app.tool(&ctx, "set_settings", &json!({ "wheelFontSizeIn": "allEditors" })).unwrap();
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "editor.fontSize.reset" })).unwrap();
        step(&mut app, Vec::new());
        step(&mut app, wheel(1.0));
        for _ in 0..2 {
            step(&mut app, Vec::new());
        }
        assert_eq!(ask(&ctx, &mut app, "get_settings", json!({})).unwrap()["editorFont"]["size"], 14.0);
        assert_eq!(file_size(&mut app), json!(14.0));

        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "editor.fontSize.increase" })).unwrap();
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.close" })).unwrap();
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        assert_eq!(file_size(&mut app), json!(14.0), "the file's own size went with its closing");
    }

    fn wheel_notches(at: egui::Pos2, lines: f32, modifiers: egui::Modifiers) -> Vec<Vec<egui::Event>> {
        vec![
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                delta: egui::vec2(0.0, lines),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Wheel,
                modifiers,
            }],
        ]
    }

    fn trackpad_swipe(at: egui::Pos2, moves: &[f32], modifiers: egui::Modifiers) -> Vec<Vec<egui::Event>> {
        let event = |phase, y: f32| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, y),
            phase,
            source: egui::MouseWheelSource::Trackpad,
            modifiers,
        };
        let mut frames = vec![vec![egui::Event::PointerMoved(at)], vec![event(egui::TouchPhase::Start, 0.0)]];
        frames.extend(moves.iter().map(|y| vec![event(egui::TouchPhase::Move, *y)]));
        frames.push(vec![event(egui::TouchPhase::End, 0.0)]);
        frames
    }

    fn long_go_file(project: &Path) -> PathBuf {
        let file = project.join("long.go");
        let body: String = (0..400).map(|line| format!("var value{line} = {line}\n")).collect();
        std::fs::write(&file, format!("package main\n\n{body}")).unwrap();
        file
    }

    fn app_frame(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, events: Vec<egui::Event>) {
        screen.step(events, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
    }

    fn settle_frames(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp) {
        for _ in 0..90 {
            app_frame(screen, app, Vec::new());
        }
    }

    fn click_link(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str) -> bool {
        let tree = screen.accessibility_tree(|ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        });
        let Some(node) = crate::accessibility::testing::find(&tree, egui::accesskit::Role::Link, name) else {
            return false;
        };
        let bounds = node.bounds().expect("the link has bounds");
        let at = egui::pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            app_frame(screen, app, events);
        }
        true
    }

    fn few_frames(screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp) {
        for _ in 0..6 {
            app_frame(screen, app, Vec::new());
        }
    }

    fn ask_drawn(
        screen: &mut crate::offscreen::Offscreen,
        app: &mut IdeApp,
        tool: &str,
        arguments: Value,
    ) -> Result<Value, String> {
        let (sender, answer) = mpsc::channel();
        let ctx = screen.context().clone();
        app.route(
            &ctx,
            tool,
            &arguments,
            Box::new(move |result| {
                let _ = sender.send(result);
            }),
        );
        for _ in 0..5 {
            if let Ok(result) = answer.try_recv() {
                return result;
            }
            app_frame(screen, app, Vec::new());
        }
        answer.try_recv().unwrap_or_else(|_| panic!("no answer to {tool} {arguments} after 5 frames"))
    }

    fn command() -> egui::Modifiers {
        egui::Modifiers { mac_cmd: cfg!(target_os = "macos"), ..egui::Modifiers::COMMAND }
    }

    #[test]
    fn the_command_wheel_changes_the_font_size_without_scrolling_and_the_indicator_resets_it() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, _) = window_project(data.path());
        let file = long_go_file(&project);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 700.0), 1.0);
        let ctx = screen.context().clone();
        settle_frames(&mut screen, &mut app);
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        settle_frames(&mut screen, &mut app);
        let slot = app.slot_for(None).unwrap();
        let top = || slot.window.lock().unwrap().view.editor_view.shown_top();
        let file_size = |app: &mut IdeApp| {
            ask(&ctx, app, "dispatch_command", json!({ "command": "editor.fontSize.get" })).unwrap()["size"].clone()
        };
        let editor_font =
            |app: &mut IdeApp| ask(&ctx, app, "get_settings", json!({})).unwrap()["editorFont"]["size"].clone();
        let swipe = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, moves: &[f32], modifiers| {
            for events in trackpad_swipe(egui::pos2(800.0, 350.0), moves, modifiers) {
                app_frame(screen, app, events);
            }
            settle_frames(screen, app);
        };
        let notches = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, lines: f32, modifiers| {
            for events in wheel_notches(egui::pos2(800.0, 350.0), lines, modifiers) {
                app_frame(screen, app, events);
            }
            settle_frames(screen, app);
        };
        let offset = |(line, height): (usize, f32)| line as f32 * height;

        notches(&mut screen, &mut app, -10.0, egui::Modifiers::NONE);
        let (scrolled, height) = top();
        assert!(scrolled >= 3, "the plain wheel scrolls: line {scrolled}");
        swipe(&mut screen, &mut app, &[-20.0; 10], egui::Modifiers::NONE);
        assert!(top().0 > scrolled, "so does the trackpad: line {:?}", top());
        let (scrolled, height) = (top().0, height);

        notches(&mut screen, &mut app, -3.0, command());
        swipe(&mut screen, &mut app, &[-20.0; 10], command());
        assert_eq!(top(), (scrolled, height), "with the option off ⌘ and the wheel neither scroll nor zoom");
        assert_eq!(file_size(&mut app), json!(13.0));

        app.tool(&ctx, "set_settings", &json!({ "wheelFontSize": true })).unwrap();
        settle_frames(&mut screen, &mut app);
        let before = offset(top());
        notches(&mut screen, &mut app, 5.0, command());
        assert_eq!(file_size(&mut app), json!(14.0), "a notch is a point, however many lines it turns");
        let (_, bigger) = top();
        assert!(bigger > height, "the lines grow with the font: {height} → {bigger}");
        assert!((offset(top()) - before).abs() <= bigger, "⌘ and the wheel do not scroll too: {before} → {:?}", top());

        let before = offset(top());
        swipe(&mut screen, &mut app, &[8.0; 10], command());
        assert_eq!(file_size(&mut app), json!(16.0), "80 points of trackpad travel in small moves are two points");
        assert!((offset(top()) - before).abs() <= top().1, "the trackpad does not scroll either: {:?}", top());
        swipe(&mut screen, &mut app, &[30.0, 15.0, -5.0], command());
        assert_eq!(file_size(&mut app), json!(17.0), "40 points of travel in all are one point");
        swipe(&mut screen, &mut app, &[-10.0, -10.0], command());
        swipe(&mut screen, &mut app, &[-10.0, -10.0], command());
        assert_eq!(file_size(&mut app), json!(16.0), "the travel left over adds up across swipes");
        notches(&mut screen, &mut app, -1.0, command());
        assert_eq!(file_size(&mut app), json!(15.0));
        assert_eq!(editor_font(&mut app), json!(13.0), "Active editor leaves the setting alone");

        notches(&mut screen, &mut app, 1.0, command());
        assert!(click_link(&mut screen, &mut app, "Reset"), "the zoom indicator offers Reset");
        settle_frames(&mut screen, &mut app);
        let state = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "editor.fontSize.get" })).unwrap();
        assert_eq!((state["size"].clone(), state["changed"].clone()), (json!(13.0), json!(false)), "{state}");
        assert_eq!(top().1, height);

        app.tool(&ctx, "set_settings", &json!({ "wheelFontSizeIn": "allEditors" })).unwrap();
        settle_frames(&mut screen, &mut app);
        notches(&mut screen, &mut app, 1.0, command());
        notches(&mut screen, &mut app, 1.0, command());
        assert_eq!(editor_font(&mut app), json!(15.0));
        notches(&mut screen, &mut app, 1.0, command());
        assert!(click_link(&mut screen, &mut app, "Reset"));
        settle_frames(&mut screen, &mut app);
        assert_eq!(editor_font(&mut app), json!(13.0), "Reset of all editors brings back 13 points");

        app.tool(&ctx, "set_settings", &json!({ "zoomIndicator": false })).unwrap();
        settle_frames(&mut screen, &mut app);
        notches(&mut screen, &mut app, 1.0, command());
        assert_eq!(editor_font(&mut app), json!(14.0));
        assert!(!click_link(&mut screen, &mut app, "Reset"), "Show the zoom level when it changes off: no indicator");

        let before = top().0;
        swipe(&mut screen, &mut app, &[-20.0; 10], egui::Modifiers::NONE);
        assert!(top().0 > before, "the plain wheel still scrolls: {before} → {}", top().0);
        assert_eq!(editor_font(&mut app), json!(14.0));
    }

    #[test]
    fn font_size_changes_in_presentation_mode_start_from_its_size() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let ctx = egui::Context::default();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "file.document.open", "params": { "path": file } }))
            .unwrap();
        ask(&ctx, &mut app, "run_action", json!({ "action": "view.presentationMode.toggle" })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        let slot = app.slot_for(None).unwrap();
        let shown = || slot.window.lock().unwrap().view.editor_view.font.size;
        assert_eq!(shown(), 24.0);
        let bigger = ask(&ctx, &mut app, "dispatch_command", json!({ "command": "editor.fontSize.increase" })).unwrap();
        assert_eq!((bigger["size"].clone(), bigger["default"].clone()), (json!(25.0), json!(24.0)), "{bigger}");
        frame(&ctx, &mut app);
        assert_eq!(shown(), 25.0, "one point above Presentation Mode's size, not the editor font's");
        ask(&ctx, &mut app, "dispatch_command", json!({ "command": "editor.fontSize.reset" })).unwrap();

        let all = ask(&ctx, &mut app, "run_action", json!({ "action": "view.editorFontSize.increase" })).unwrap();
        assert_eq!(all["result"], json!({ "size": 25.0, "default": 24.0 }), "{all}");
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        let settings = ask(&ctx, &mut app, "get_settings", json!({})).unwrap();
        assert_eq!(
            (settings["presentationFontSize"].clone(), settings["editorFont"]["size"].clone()),
            (json!(25.0), json!(13.0))
        );
        assert_eq!(shown(), 25.0, "every editor in Presentation Mode grows");

        ask(&ctx, &mut app, "run_action", json!({ "action": "view.presentationMode.toggle" })).unwrap();
        for _ in 0..2 {
            frame(&ctx, &mut app);
        }
        assert_eq!(shown(), 13.0, "the editor font is left as it was");
    }

    #[test]
    fn the_editor_and_its_popups_scale_with_the_font_size() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let pixels = if snapshots.is_some() { 2.0 } else { 1.0 };
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1100.0, 640.0), pixels);
        let ctx = screen.context().clone();
        settle_frames(&mut screen, &mut app);
        let run = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, command: &str, params: Value| {
            ask_drawn(screen, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        run(&mut screen, &mut app, "file.document.open", json!({ "path": file }));
        let slot = app.slot_for(None).unwrap();
        slot.window.lock().unwrap().session.set_state({
            let templates = zen_workbench::templates::LiveTemplates::default();
            templates.declare(["for", "fori", "forr", "func", "fmt"].into_iter().map(|abbreviation| {
                zen_workbench::templates::Template {
                    language: "go".to_owned(),
                    abbreviation: abbreviation.to_owned(),
                    description: format!("The {abbreviation} template"),
                    text: abbreviation.to_owned(),
                    variables: Vec::new(),
                    imports: Vec::new(),
                    contexts: Vec::new(),
                    enabled: true,
                }
            }));
            templates
        });
        let draw = |screen: &mut crate::offscreen::Offscreen, app: &mut IdeApp, name: &str| {
            let image = screen.frame(Vec::new(), |ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("font-size-{name}.png")), &image).unwrap();
            }
        };
        let mut heights = Vec::new();
        for size in [10.0, 13.0, 24.0] {
            app.tool(&ctx, "set_settings", &json!({ "editorFont": { "size": size } })).unwrap();
            few_frames(&mut screen, &mut app);
            run(&mut screen, &mut app, "editor.selection.set", json!({ "selections": [{ "head": 41 }] }));
            few_frames(&mut screen, &mut app);
            ask_drawn(&mut screen, &mut app, "press_keys", json!({ "text": "f" })).unwrap();
            for _ in 0..5 {
                app_frame(&mut screen, &mut app, Vec::new());
            }
            let window = slot.window.lock().unwrap();
            assert!(window.view.editor_view.completing(), "the completion popup is open at {size} pt");
            heights.push(window.view.editor_view.shown_top().1);
            drop(window);
            draw(&mut screen, &mut app, &format!("editor-{size}pt"));
            ask_drawn(&mut screen, &mut app, "press_keys", json!({ "keys": ["⎋", "⌫"] })).unwrap();
            few_frames(&mut screen, &mut app);
        }
        assert!(heights[0] < heights[1] && heights[1] < heights[2], "the line height follows the size: {heights:?}");

        app.tool(&ctx, "set_settings", &json!({ "editorFont": { "size": 13 } })).unwrap();
        few_frames(&mut screen, &mut app);
        run(&mut screen, &mut app, "editor.fontSize.increase", json!({}));
        run(&mut screen, &mut app, "editor.fontSize.increase", json!({}));
        for _ in 0..3 {
            app_frame(&mut screen, &mut app, Vec::new());
        }
        draw(&mut screen, &mut app, "indicator-active-editor");
        run(&mut screen, &mut app, "editor.fontSize.reset", json!({}));
        ask_drawn(&mut screen, &mut app, "run_action", json!({ "action": "view.editorFontSize.increase" })).unwrap();
        for _ in 0..3 {
            app_frame(&mut screen, &mut app, Vec::new());
        }
        draw(&mut screen, &mut app, "indicator-all-editors");
        ask_drawn(&mut screen, &mut app, "run_action", json!({ "action": "view.editorFontSize.reset" })).unwrap();
        few_frames(&mut screen, &mut app);

        ask_drawn(&mut screen, &mut app, "run_action", json!({ "action": "view.findAction.open" })).unwrap();
        few_frames(&mut screen, &mut app);
        app_frame(&mut screen, &mut app, vec![egui::Event::Text("font size".to_owned())]);
        few_frames(&mut screen, &mut app);
        draw(&mut screen, &mut app, "find-action");
        ask_drawn(&mut screen, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
        few_frames(&mut screen, &mut app);

        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for language in languages {
            if snapshots.is_some() {
                zen_i18n::set_language(Some(language));
            }
            for page in ["editor", "advanced"] {
                ask_drawn(
                    &mut screen,
                    &mut app,
                    "run_action",
                    json!({ "action": "view.settings.open", "params": { "page": page } }),
                )
                .unwrap();
                few_frames(&mut screen, &mut app);
                draw(&mut screen, &mut app, &format!("settings-{page}-{language}"));
                ask_drawn(&mut screen, &mut app, "press_keys", json!({ "keys": ["⎋"] })).unwrap();
                few_frames(&mut screen, &mut app);
            }
        }
        if snapshots.is_some() {
            zen_i18n::set_language(Some("en"));
        }
    }

    #[test]
    fn the_command_wheel_works_in_either_editor_of_a_split() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let data = tempfile::tempdir().unwrap();
        let (project, file) = window_project(data.path());
        let other = long_go_file(&project);
        let mut app = themed_app(data.path(), vec![project.clone()]);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(1200.0, 700.0), 1.0);
        let ctx = screen.context().clone();
        app.tool(&ctx, "set_settings", &json!({ "wheelFontSize": true })).unwrap();
        settle_frames(&mut screen, &mut app);
        let run = |app: &mut IdeApp, command: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", json!({ "command": command, "params": params })).unwrap()
        };
        run(&mut app, "file.document.open", json!({ "path": file }));
        run(&mut app, "editor.split.right", json!({}));
        run(&mut app, "file.document.open", json!({ "path": other }));
        settle_frames(&mut screen, &mut app);
        let own_size = |app: &IdeApp, path: &Path| {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            let workbench = window.session.state::<Workbench>().unwrap();
            workbench
                .documents()
                .iter()
                .find(|document| document.path().file_name() == path.file_name())
                .unwrap()
                .font_size()
        };
        for (at, path) in [(egui::pos2(500.0, 350.0), &file), (egui::pos2(1050.0, 350.0), &other)] {
            for events in trackpad_swipe(at, &[10.0; 8], command()) {
                app_frame(&mut screen, &mut app, events);
            }
            settle_frames(&mut screen, &mut app);
            settle_frames(&mut screen, &mut app);
            assert_eq!(own_size(&app, path), Some(15.0), "80 points of trackpad travel over {path:?}");
        }
        assert_eq!(own_size(&app, &file), Some(15.0), "the other editor keeps its size");
    }

    #[test]
    fn fold_selection_custom_regions_and_custom_folding_from_keys_to_the_next_start() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let file = project.join("main.go");
        std::fs::write(&file, FOLDING_GO).unwrap();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(900.0, 460.0), 2.0);
        let ctx = screen.context().clone();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        app.splash = super::super::splash::Splash::finished();
        let mut snapshot = |app: &mut IdeApp, name: &str| {
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("folding-{name}.png")), &image).unwrap();
            }
        };
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        let run = |app: &mut IdeApp, id: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", command(id, params)).unwrap_or_else(|error| panic!("{id}: {error}"))
        };
        run(&mut app, "file.document.open", json!({ "path": file }));
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let regions = run(&mut app, "editor.fold.regions", json!({}));
        let names: Vec<&str> = regions.as_array().unwrap().iter().map(|r| r["description"].as_str().unwrap()).collect();
        assert_eq!(names, ["Helpers", "Entry point"], "{regions}");
        let helpers = regions[0]["start"].clone();
        run(&mut app, "editor.fold.toggle", json!({ "start": helpers }));
        let listed = run(&mut app, "editor.fold.list", json!({}));
        let folded = listed.as_array().unwrap().iter().find(|fold| fold["start"] == helpers).unwrap().clone();
        assert_eq!((folded["kind"].clone(), folded["placeholder"].clone()), (json!("region"), json!("Helpers")));

        let args = FOLDING_GO.find("greet(\"world\")").unwrap();
        let end = FOLDING_GO.find("))\n}").unwrap() + 1;
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": args, "head": end }] }));
        let pressed = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘."] })).unwrap();
        assert_eq!(pressed["window"]["popups"], json!([]), "{pressed}");
        let manual = |app: &mut IdeApp| -> Vec<(u64, u64, bool)> {
            let listed = run(app, "editor.fold.list", json!({}));
            listed
                .as_array()
                .unwrap()
                .iter()
                .filter(|fold| fold["kind"] == "manual")
                .map(|fold| (fold["start"].as_u64().unwrap(), fold["end"].as_u64().unwrap(), fold["folded"] == true))
                .collect()
        };
        assert_eq!(manual(&mut app), [(args as u64, end as u64, true)], "⌘. folds the selection");
        snapshot(&mut app, "selection-and-region-folded");

        let opened = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌥⌘."] })).unwrap();
        assert_eq!(opened["window"]["popups"], json!(["customFolding"]), "{opened}");
        snapshot(&mut app, "custom-folding-popup");
        let shown = run(&mut app, "view.customFolding.open", json!({}));
        let current = run(&mut app, "editor.fold.regions", json!({}));
        assert_eq!(shown["result"], current, "the action answers with the regions it lists");
        assert_eq!(current[0]["folded"], true, "{current}");
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["↓"] })).unwrap();
        let chosen = ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⏎"] })).unwrap();
        assert_eq!(chosen["window"]["popups"], json!([]), "{chosen}");
        let caret = |app: &mut IdeApp| -> usize {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            window.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head
        };
        assert_eq!(caret(&mut app), FOLDING_GO.find("//<editor-fold").unwrap(), "at the region chosen");

        run(&mut app, "editor.selection.set", json!({ "selections": [{ "head": args - 3 }] }));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘."] })).unwrap();
        assert_eq!(manual(&mut app), [], "⌘. on the region's line removes it");
        let body = FOLDING_GO.find("\tfmt.Println").unwrap();
        let next = FOLDING_GO.find("}\n//</editor-fold>").unwrap();
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": args, "head": end }] }));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘."] })).unwrap();
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": body, "head": next }] }));
        ask(&ctx, &mut app, "press_keys", json!({ "keys": ["⌘."] })).unwrap();
        let before = manual(&mut app);
        assert_eq!(
            before,
            [(body as u64, (next - 1) as u64, true), (args as u64, end as u64, true)],
            "the line break stays out"
        );
        let lines = |app: &mut IdeApp| -> Vec<(u64, u64, bool)> {
            let listed = run(app, "editor.fold.list", json!({}));
            listed
                .as_array()
                .unwrap()
                .iter()
                .map(|fold| {
                    (fold["startLine"].as_u64().unwrap(), fold["endLine"].as_u64().unwrap(), fold["folded"] == true)
                })
                .collect()
        };
        let wanted = lines(&mut app);
        snapshot(&mut app, "before-restart");
        run(&mut app, "file.document.save", json!({}));
        app.remember_workspace(&ctx, true);
        drop(app);

        let mut app = themed_app(data.path(), vec![project]);
        app.splash = super::super::splash::Splash::finished();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        let reopened = ask(&ctx, &mut app, "get_window_state", json!({})).unwrap();
        assert!(reopened["editor"].as_str().is_some_and(|editor| editor.ends_with("main.go")), "{reopened}");
        assert_eq!(manual(&mut app), before, "the manual regions come back at the next start, folded as left");
        assert_eq!(lines(&mut app), wanted, "and the custom region with them");
        snapshot(&mut app, "after-restart");
        if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() {
            zen_i18n::set_language(Some("ru"));
            ask(&ctx, &mut app, "run_action", json!({ "action": "view.customFolding.open" })).unwrap();
            snapshot(&mut app, "custom-folding-popup-ru");
            zen_i18n::set_language(Some("en"));
        }
    }

    #[test]
    fn regions_in_strings_folds_sharing_a_line_edits_at_the_edges_and_a_file_changed_while_closed() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let data = tempfile::tempdir().unwrap();
        let project = data.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let file = project.join("main.go");
        std::fs::write(&file, REVIEW_GO).unwrap();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let mut screen = crate::offscreen::Offscreen::new(egui::vec2(900.0, 560.0), 2.0);
        let ctx = screen.context().clone();
        let mut app = themed_app(data.path(), vec![project.clone()]);
        app.splash = super::super::splash::Splash::finished();
        let mut snapshot = |app: &mut IdeApp, name: &str| {
            let image = screen.render(|ui| {
                app.logic(ui.ctx());
                app.ui(ui);
            });
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("review-{name}.png")), &image).unwrap();
            }
        };
        let command = |command: &str, params: Value| json!({ "command": command, "params": params });
        let run = |app: &mut IdeApp, id: &str, params: Value| {
            ask(&ctx, app, "dispatch_command", command(id, params)).unwrap_or_else(|error| panic!("{id}: {error}"))
        };
        let keys = |app: &mut IdeApp, keys: Value| ask(&ctx, app, "press_keys", json!({ "keys": keys })).unwrap();
        let text = |app: &mut IdeApp| -> String {
            let slot = app.slot_for(None).unwrap();
            let window = slot.window.lock().unwrap();
            window.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
        };
        let manual = |app: &mut IdeApp| -> Vec<(String, bool)> {
            let now = text(app);
            let listed = run(app, "editor.fold.list", json!({}));
            listed
                .as_array()
                .unwrap()
                .iter()
                .filter(|fold| fold["kind"] == "manual")
                .map(|fold| {
                    let (start, end) =
                        (fold["start"].as_u64().unwrap() as usize, fold["end"].as_u64().unwrap() as usize);
                    (now.chars().skip(start).take(end - start).collect(), fold["folded"] == true)
                })
                .collect()
        };
        run(&mut app, "file.document.open", json!({ "path": file }));
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }

        let regions = run(&mut app, "editor.fold.regions", json!({}));
        let names: Vec<&str> = regions.as_array().unwrap().iter().map(|r| r["description"].as_str().unwrap()).collect();
        assert_eq!(names, ["Helpers"], "the region comments in a string and in a block comment are not regions");
        run(
            &mut app,
            "editor.selection.set",
            json!({ "selections": [{ "head": REVIEW_GO.find("func main").unwrap() }] }),
        );
        let opened = keys(&mut app, json!(["⌥⌘."]));
        assert_eq!(opened["window"]["popups"], json!(["customFolding"]), "{opened}");
        snapshot(&mut app, "custom-folding-real-regions-only");
        keys(&mut app, json!(["⎋"]));

        let name = REVIEW_GO.find("main()").unwrap();
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": name, "head": name + 6 }] }));
        keys(&mut app, json!(["⌘."]));
        let block = REVIEW_GO.find("main() {").unwrap() + "main() {".len();
        run(&mut app, "editor.fold.toggle", json!({ "start": block }));
        let args = REVIEW_GO.find("greet(\"world\")").unwrap();
        let args_end = REVIEW_GO.find("))\n}").unwrap() + 1;
        assert_eq!(manual(&mut app), [("main()".to_owned(), true)]);
        snapshot(&mut app, "manual-and-block-fold-on-one-line");

        run(&mut app, "editor.fold.expandAll", json!({}));
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "anchor": args, "head": args_end }] }));
        keys(&mut app, json!(["⌘."]));
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "head": args_end }] }));
        ask(&ctx, &mut app, "press_keys", json!({ "text": "Z" })).unwrap();
        run(&mut app, "editor.selection.set", json!({ "selections": [{ "head": args }] }));
        ask(&ctx, &mut app, "press_keys", json!({ "text": "A" })).unwrap();
        let held = "greet(\"world\"), greet(\"zen\")".to_owned();
        assert_eq!(
            manual(&mut app),
            [("main()".to_owned(), false), (held.clone(), true)],
            "typed at both edges: outside"
        );
        assert!(text(&mut app).contains("Println(A"), "{}", text(&mut app));
        snapshot(&mut app, "typed-at-both-edges");
        keys(&mut app, json!(["⌘Z", "⌘Z"]));
        assert_eq!(manual(&mut app), [("main()".to_owned(), false), (held.clone(), true)], "undone around it");
        assert_eq!(text(&mut app), REVIEW_GO);
        run(&mut app, "file.document.save", json!({}));
        app.remember_workspace(&ctx, true);
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(data.path().join("workspace.json")).unwrap()).unwrap();
        assert!(saved.to_string().contains("textHash"), "{saved}");
        drop(app);

        std::fs::write(&file, REVIEW_GO.replace("greet(\"zen\")", "greet(\"you\")")).unwrap();
        let mut app = themed_app(data.path(), vec![project]);
        app.splash = super::super::splash::Splash::finished();
        for _ in 0..3 {
            frame(&ctx, &mut app);
        }
        assert_eq!(
            manual(&mut app),
            [("main()".to_owned(), false)],
            "at the next start, the manual region whose text changed on disk is dropped, the other comes back"
        );
        snapshot(&mut app, "after-restart-text-changed-on-disk");
    }

    const FOLDING_GO: &str = "package main\n\nimport \"fmt\"\n\n// region Helpers\n// greet says hello\nfunc greet(name string) string {\n\treturn fmt.Sprintf(\"hello, %s\", name)\n}\n// endregion\n\n//<editor-fold desc=\"Entry point\">\nfunc main() {\n\tfmt.Println(greet(\"world\"), greet(\"zen\"))\n}\n//</editor-fold>\n";

    const REVIEW_GO: &str = "package main\n\nimport \"fmt\"\n\n// region Helpers\nfunc greet(name string) string {\n\treturn fmt.Sprintf(\"hello, %s\", name)\n}\n// endregion\n\nconst usage = \"// region Not a region\"\n\n/*\n// region Commented out\n// endregion\n*/\n\nfunc main() {\n\tfmt.Println(greet(\"world\"), greet(\"zen\"))\n}\n";
}

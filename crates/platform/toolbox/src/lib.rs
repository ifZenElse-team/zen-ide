pub mod auto_update;
pub mod launchers;
pub mod login;
pub mod markdown;
pub mod notifications;
pub mod projects;
pub mod settings;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_update::Manifest;
use zen_update::install::{Installation, LaunchCommand};

pub use projects::{ListProjects, ListProjectsParams, OpenProject, OpenProjectParams, Project};

pub use settings::{
    Channel, CommandLine, Defaults, RunSettings, SettingsPatch, Theme, ToolboxSettings, UpdateCheck, UpdateMode,
};

pub const LOCALES: &[(&str, &str)] =
    &[("en", include_str!("../locales/en/toolbox.ftl")), ("ru", include_str!("../locales/ru/toolbox.ftl"))];

pub const TOOLBOX_ID: &str = "toolbox";

pub const WHATS_NEW_SETTING: &str = "toolbox.whatsNewShown";

/// A product Zen Toolbox knows about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    /// The stable product id: the product name in lowercase (docs/naming.md), e.g. `rustronin`.
    pub id: String,
    /// The product name, e.g. `RustRonin`.
    pub name: String,
    /// What the tool is for, e.g. `Rust`.
    pub subject: String,
    /// The bundle identifier, `space.ifzenelse.<id>` (ADR-0017).
    pub bundle_id: String,
    /// The shell launcher's name: the product id, as the IDE's executable is named (ADR-0085), e.g. `gogarden`.
    pub launcher: String,
    /// Whether the tool is installed, can be installed, or is not released yet.
    pub state: ToolState,
    /// The download size in bytes of the release that Install or Update gets, when known: from the
    /// manifest, or for a manifest without sizes from the server (a cached `HEAD` request).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_size: Option<u64>,
}

pub fn megabytes(bytes: u64) -> u64 {
    (bytes.saturating_add(500_000) / 1_000_000).max(1)
}

/// Where a tool stands on this machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ToolState {
    /// Installed at `version`.
    #[serde(rename_all = "camelCase")]
    Installed {
        /// The installed version.
        version: String,
        /// A newer version on the selected channel.
        update: Option<String>,
        /// Whether the tool is running now.
        running: bool,
    },
    /// Released and installable.
    Available {
        /// The latest version on the selected channel.
        version: String,
    },
    /// Announced but not released yet.
    ComingSoon,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub manifest_url: String,
    pub public_key: String,
    pub channel: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub source: Option<Source>,
    pub tools_dir: Option<PathBuf>,
    pub applications: Option<PathBuf>,
    pub self_version: String,
    pub self_executable: Option<PathBuf>,
    pub other_tools_dirs: Vec<PathBuf>,
    pub updates_check: UpdateCheck,
    pub updates_self: UpdateMode,
    pub command_line: CommandLine,
    pub defaults: Defaults,
    pub shell_scripts: ShellScripts,
    pub data_dir: Option<PathBuf>,
    pub login_item: Option<login::LoginItem>,
    pub launch_at_login: bool,
}

impl Config {
    pub fn apply(&mut self, run: RunSettings) {
        if let Some(source) = &mut self.source {
            run.channel.as_str().clone_into(&mut source.channel);
        }
        self.tools_dir = run.tools_dir;
        self.other_tools_dirs = run.other_tools_dirs;
        self.updates_check = run.updates_check;
        self.updates_self = run.updates_self;
        self.shell_scripts = run.shell_scripts;
        self.launch_at_login = run.launch_at_login;
    }

    fn roots(&self) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = Vec::new();
        for root in self.tools_dir.iter().chain(&self.other_tools_dirs) {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }
        roots
    }

    fn installed_root(&self, id: &str) -> Option<PathBuf> {
        self.roots().into_iter().find(|root| Installation::new(root).current(id).is_some())
    }

    fn installations(&self) -> Vec<Installation> {
        self.roots().into_iter().map(|root| self.installation_in(root)).collect()
    }

    fn installation_in(&self, root: PathBuf) -> Installation {
        match &self.applications {
            Some(applications) => Installation::new(root).with_applications(applications),
            None => Installation::new(root),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellScriptsView {
    pub enabled: bool,
    pub location: Option<PathBuf>,
    pub status: Option<zen_update::scripts::PathStatus>,
    pub launchers: Vec<PathBuf>,
    pub checking: bool,
    pub demo: bool,
}

fn ours(dir: &std::path::Path) -> Vec<PathBuf> {
    let shell = launchers::Shell::current();
    catalog()
        .into_iter()
        .map(|tool| tool.launcher)
        .chain([launchers::ZEN_CLI.to_owned()])
        .map(|name| dir.join(shell.file_name(&name)))
        .filter(|path| zen_update::scripts::owner(path) == zen_update::scripts::Owner::Ours)
        .collect()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShellScripts {
    pub enabled: bool,
    pub location: Option<PathBuf>,
}

pub type RunningPrograms = Arc<dyn Fn() -> Option<Vec<PathBuf>> + Send + Sync>;

pub type Notifier = Arc<dyn Fn(&notifications::Notice) + Send + Sync>;

pub type Launcher = Arc<dyn Fn(&LaunchCommand) -> std::io::Result<()> + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub done: u64,
    pub total: u64,
    pub unpacking: bool,
}

impl Job {
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0).then(|| (self.done as f32 / self.total as f32).clamp(0.0, 1.0))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SelfUpdate {
    #[default]
    UpToDate,
    Available(String),
    Downloading(String, Job),
    ReadyToRestart(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ServerStatus {
    #[default]
    NotConfigured,
    Checking,
    NotChecked,
    Online {
        releases: usize,
    },
    Unreachable(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub tools: Vec<Tool>,
    pub jobs: BTreeMap<String, Job>,
    pub self_update: SelfUpdate,
    pub self_update_size: Option<u64>,
    pub refreshing: bool,
    pub demo: bool,
    pub server: ServerStatus,
    pub launch_at_login: Option<bool>,
}

impl Snapshot {
    pub fn updates(&self) -> usize {
        self.tools.iter().filter(|tool| matches!(tool.state, ToolState::Installed { update: Some(_), .. })).count()
    }
}

/// The notes of one release, as "What's new" shows them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNote {
    /// The version, e.g. `0.2.0`.
    pub version: String,
    /// The release date, `YYYY-MM-DD`.
    pub date: String,
    /// The notes, in Markdown ([`markdown::parse`]); may be empty.
    pub notes: String,
    /// Whether this is the version installed (for Zen Toolbox: running) on this machine.
    pub installed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartNotes {
    Nothing,
    Remember,
    Show { newer_than: Option<String> },
}

pub fn notes_on_start(last_shown: Option<&str>, current: &str, ran_before: bool) -> StartNotes {
    match last_shown {
        Some(last) if last == current => StartNotes::Nothing,
        Some(last) if newer(current, last) => StartNotes::Show { newer_than: Some(last.to_owned()) },
        Some(_) => StartNotes::Remember,
        None if ran_before => StartNotes::Show { newer_than: None },
        None => StartNotes::Remember,
    }
}

#[derive(Default)]
struct Shared {
    config: Config,
    demo: bool,
    manifest: Option<Manifest>,
    tools: Vec<Tool>,
    jobs: BTreeMap<String, Job>,
    self_update: SelfUpdate,
    refreshing: bool,
    restart_requested: bool,
    messages: Vec<String>,
    server: ServerStatus,
    last_check: Option<std::time::Instant>,
    launcher: Option<Launcher>,
    notifier: Option<Notifier>,
    automatic_updates: std::collections::BTreeSet<String>,
    automatic_turn: Option<std::time::Instant>,
    running_programs: Option<RunningPrograms>,
    self_update_size: Option<u64>,
    sizes: BTreeMap<String, u64>,
    sizes_asked: std::collections::BTreeSet<String>,
    path_status: Option<(zen_update::scripts::PathStatus, Vec<PathBuf>)>,
    path_checks: (u64, u64),
}

#[derive(Clone, Default)]
pub struct Tools {
    shared: Arc<Mutex<Shared>>,
}

impl Tools {
    pub fn new(config: Config) -> Self {
        let server = initial_status(&config);
        for root in config.roots() {
            let installation = Installation::new(root);
            for tool in catalog() {
                installation.prune(&tool.id);
            }
        }
        let tools = Self { shared: Arc::new(Mutex::new(Shared { config, server, ..Shared::default() })) };
        {
            let mut shared = tools.lock();
            recompute(&mut shared);
            refresh_shell_scripts(&mut shared);
            sync_login_item(&mut shared);
        }
        tools
    }

    pub fn demo() -> Self {
        let config = Config { self_version: "0.0.9".to_owned(), ..Config::default() };
        let shared = Shared { config, tools: demo(), demo: true, manifest: Some(demo_manifest()), ..Shared::default() };
        let tools = Self { shared: Arc::new(Mutex::new(shared)) };
        {
            let mut shared = tools.lock();
            shared.self_update = SelfUpdate::Available("0.1.0".to_owned());
            shared.self_update_size = Some(14_200_000);
        }
        tools
    }

    pub fn with_launcher(self, launcher: Launcher) -> Self {
        self.lock().launcher = Some(launcher);
        self
    }

    pub fn with_notifier(self, notifier: Notifier) -> Self {
        self.lock().notifier = Some(notifier);
        self
    }

    pub fn with_running_programs(self, list: RunningPrograms) -> Self {
        self.lock().running_programs = Some(list);
        self
    }

    pub(crate) fn take_automatic_turn(&self) -> bool {
        let mut shared = self.lock();
        let due = shared.last_check.is_some() && shared.last_check != shared.automatic_turn;
        shared.automatic_turn = shared.last_check;
        due
    }

    pub(crate) fn running_programs(&self) -> Option<Vec<PathBuf>> {
        let list = self.lock().running_programs.clone()?;
        list()
    }

    pub(crate) fn is_running(&self, id: &str, programs: &[PathBuf]) -> bool {
        let installations = self.lock().config.installations();
        auto_update::runs(auto_update::tool_executable(&installations, id), programs)
    }

    pub(crate) fn notify(&self, notice: &notifications::Notice) {
        let notifier = self.lock().notifier.clone();
        if let Some(notifier) = notifier {
            notifier(notice);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_update_for_test(&self, version: &str) {
        self.lock().self_update = SelfUpdate::Available(version.to_owned());
    }

    fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn snapshot(&self) -> Snapshot {
        let shared = self.lock();
        Snapshot {
            tools: shared.tools.clone(),
            jobs: shared.jobs.clone(),
            self_update: shared.self_update.clone(),
            self_update_size: shared.self_update_size,
            refreshing: shared.refreshing,
            demo: shared.demo,
            server: shared.server.clone(),
            launch_at_login: shared.config.login_item.as_ref().map(|_| shared.config.launch_at_login),
        }
    }

    pub fn login_status(&self) -> login::LoginStatus {
        let shared = self.lock();
        let item = shared.config.login_item.as_ref();
        login::LoginStatus {
            enabled: shared.config.launch_at_login,
            supported: item.is_some(),
            registered: item.is_some_and(login::LoginItem::is_registered),
            file: item.map(login::LoginItem::file),
        }
    }

    pub fn updates(&self) -> usize {
        self.snapshot().updates()
    }

    pub fn busy(&self) -> bool {
        let shared = self.lock();
        shared.refreshing || !shared.jobs.is_empty() || matches!(shared.self_update, SelfUpdate::Downloading(..))
    }

    pub fn take_messages(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().messages)
    }

    pub fn since_last_check(&self) -> Option<std::time::Duration> {
        self.lock().last_check.map(|instant| instant.elapsed())
    }

    pub fn take_restart_request(&self) -> bool {
        std::mem::take(&mut self.lock().restart_requested)
    }

    fn post(&self, message: impl Into<String>) {
        self.lock().messages.push(message.into());
    }

    pub fn reconfigure(&self, settings: &zen_core::Settings) {
        let mut shared = self.lock();
        if shared.demo {
            return;
        }
        let run = settings::for_this_run(settings, &shared.config.defaults, &shared.config.command_line);
        let before = shared.config.shell_scripts.clone();
        shared.config.apply(run);
        match shared.server {
            ServerStatus::Checking | ServerStatus::NotChecked if shared.manifest.is_none() && !shared.refreshing => {
                shared.server = initial_status(&shared.config);
            }
            _ => {}
        }
        recompute(&mut shared);
        follow_shell_scripts(&mut shared, &before);
        sync_login_item(&mut shared);
    }

    pub fn check_due(&self, stale_after: std::time::Duration) -> bool {
        let shared = self.lock();
        settings::check_due(
            shared.config.updates_check,
            shared.last_check.map(|instant| instant.elapsed()),
            stale_after,
        )
    }

    pub fn defaults(&self) -> Defaults {
        self.lock().config.defaults.clone()
    }

    pub fn set_shell_scripts(&self, shell_scripts: ShellScripts) {
        let mut shared = self.lock();
        if shared.demo {
            return;
        }
        let before = std::mem::replace(&mut shared.config.shell_scripts, shell_scripts);
        follow_shell_scripts(&mut shared, &before);
    }

    pub fn shell_scripts_location(&self, requested: Option<PathBuf>) -> Option<PathBuf> {
        shell_scripts_location(&self.lock().config.shell_scripts, requested)
    }

    pub fn shell_scripts(&self) -> ShellScriptsView {
        let shared = self.lock();
        let location = shell_scripts_location(&shared.config.shell_scripts, None);
        let checked = shared.path_status.clone().filter(|(status, _)| Some(&status.location) == location.as_ref());
        let (status, launchers) = checked.map_or((None, Vec::new()), |(status, launchers)| (Some(status), launchers));
        ShellScriptsView {
            enabled: shared.config.shell_scripts.enabled,
            location,
            status,
            launchers,
            checking: shared.path_checks.0 != shared.path_checks.1,
            demo: shared.demo,
        }
    }

    pub fn check_path(&self) {
        let (location, home, check) = {
            let mut shared = self.lock();
            if shared.demo {
                return;
            }
            let Some(location) = shell_scripts_location(&shared.config.shell_scripts, None) else { return };
            shared.path_checks.0 += 1;
            (location, shared.config.defaults.home.clone(), shared.path_checks.0)
        };
        let tools = self.clone();
        background(move || {
            let status = zen_update::scripts::path_status(&location, home.as_deref());
            let launchers = ours(&location);
            let mut shared = tools.lock();
            if check == shared.path_checks.0 {
                shared.path_status = Some((status, launchers));
                shared.path_checks.1 = check;
            }
        });
    }

    pub fn self_version(&self) -> String {
        self.lock().config.self_version.clone()
    }

    pub fn release_notes(&self, id: &str, newer_than: Option<&str>, up_to: Option<&str>) -> Option<Vec<ReleaseNote>> {
        let shared = self.lock();
        let manifest = shared.manifest.as_ref()?;
        let installed = if id == TOOLBOX_ID {
            Some(shared.config.self_version.clone())
        } else {
            shared.tools.iter().find(|tool| tool.id == id).and_then(|tool| match &tool.state {
                ToolState::Installed { version, .. } => Some(version.clone()),
                _ => None,
            })
        };
        let channel = shared.config.source.as_ref().map_or("stable", |source| source.channel.as_str());
        let platform = zen_update::current_platform();
        let Some(product) = manifest.product(id) else { return Some(Vec::new()) };
        let mut releases: Vec<(semver::Version, &zen_update::Release)> = product
            .releases
            .iter()
            .filter(|release| release.channel == channel)
            .filter(|release| release.artifacts.iter().any(|artifact| artifact.platform == platform))
            .filter(|release| newer_than.is_none_or(|older| newer(&release.version, older)))
            .filter(|release| up_to.is_none_or(|limit| !newer(&release.version, limit)))
            .filter_map(|release| Some((semver::Version::parse(&release.version).ok()?, release)))
            .collect();
        releases.sort_by(|a, b| b.0.cmp(&a.0));
        Some(
            releases
                .into_iter()
                .map(|(_, release)| ReleaseNote {
                    version: release.version.clone(),
                    date: release.date.clone(),
                    notes: release.notes.clone(),
                    installed: installed.as_deref() == Some(release.version.as_str()),
                })
                .collect(),
        )
    }

    pub fn versions(&self, id: &str) -> Result<Vec<ToolVersion>, CommandError> {
        let shared = self.lock();
        let manifest = shared.manifest.as_ref().ok_or_else(|| CommandError::new(t!("toolbox-error-no-manifest")))?;
        let channel = shared.config.source.as_ref().map_or("stable", |source| source.channel.as_str());
        let installed = shared.config.installed_root(id).and_then(|root| Installation::new(root).current(id));
        let source = shared.config.source.as_ref();
        Ok(manifest
            .versions(id, channel, &zen_update::current_platform())
            .into_iter()
            .map(|(release, artifact)| ToolVersion {
                version: release.version.clone(),
                date: release.date.clone(),
                download_size: artifact_size(&shared.sizes, source, artifact),
                installed: installed.as_deref() == Some(release.version.as_str()),
            })
            .collect())
    }

    pub fn own_notes(&self, newer_than: Option<&str>) -> Option<Vec<ReleaseNote>> {
        let current = self.self_version();
        let notes = self.release_notes(TOOLBOX_ID, newer_than, Some(&current))?;
        Some(match newer_than {
            Some(_) => notes,
            None => notes.into_iter().filter(|note| note.version == current).collect(),
        })
    }
}

fn initial_status(config: &Config) -> ServerStatus {
    match (&config.source, config.updates_check) {
        (None, _) => ServerStatus::NotConfigured,
        (Some(_), UpdateCheck::Automatic) => ServerStatus::Checking,
        (Some(_), UpdateCheck::Manual) => ServerStatus::NotChecked,
    }
}

fn recompute(shared: &mut Shared) {
    if shared.demo {
        return;
    }
    let platform = zen_update::current_platform();
    let channel = shared.config.source.as_ref().map_or_else(|| "stable".to_owned(), |source| source.channel.clone());
    let latest = |id: &str| {
        shared
            .manifest
            .as_ref()
            .and_then(|manifest| manifest.latest(id, &channel, &platform))
            .map(|(release, _)| release.version.clone())
    };
    let size = |id: &str| {
        let (_, artifact) = shared.manifest.as_ref()?.latest(id, &channel, &platform)?;
        artifact_size(&shared.sizes, shared.config.source.as_ref(), artifact)
    };
    let mut tools = catalog();
    for tool in &mut tools {
        let installed =
            shared.config.installed_root(&tool.id).and_then(|root| Installation::new(root).current(&tool.id));
        let available = latest(&tool.id);
        tool.download_size = size(&tool.id);
        tool.state = match installed {
            Some(version) => ToolState::Installed {
                update: available.filter(|latest| newer(latest, &version)),
                version,
                running: false,
            },
            None => match available {
                Some(version) => ToolState::Available { version },
                None => ToolState::ComingSoon,
            },
        };
    }
    for tool in &mut tools {
        if !matches!(tool.state, ToolState::Available { .. } | ToolState::Installed { update: Some(_), .. }) {
            tool.download_size = None;
        }
    }
    let self_update_size = size(TOOLBOX_ID);
    tools.sort_by_key(|tool| !matches!(tool.state, ToolState::Installed { .. }));
    shared.tools = tools;
    shared.self_update_size = self_update_size;

    if shared.manifest.is_some() {
        let releases = catalog().iter().filter(|tool| latest(&tool.id).is_some()).count();
        shared.server = ServerStatus::Online { releases };
    }

    if !matches!(shared.self_update, SelfUpdate::Downloading(..) | SelfUpdate::ReadyToRestart(_)) {
        let installed = shared.config.self_executable.is_some();
        shared.self_update = match latest(TOOLBOX_ID) {
            Some(version) if installed && newer(&version, &shared.config.self_version) => {
                SelfUpdate::Available(version)
            }
            _ => SelfUpdate::UpToDate,
        };
    }
}

fn artifact_size(
    sizes: &BTreeMap<String, u64>,
    source: Option<&Source>,
    artifact: &zen_update::Artifact,
) -> Option<u64> {
    if artifact.size > 0 {
        return Some(artifact.size);
    }
    let url = artifact_url(source?, artifact);
    sizes.get(&url).copied()
}

#[cfg(not(target_arch = "wasm32"))]
fn artifact_url(source: &Source, artifact: &zen_update::Artifact) -> String {
    zen_update::fetch::resolve(&source.manifest_url, &artifact.url)
}

#[cfg(target_arch = "wasm32")]
fn artifact_url(_: &Source, artifact: &zen_update::Artifact) -> String {
    artifact.url.clone()
}

fn unsized_artifacts(shared: &mut Shared) -> Vec<String> {
    let (Some(manifest), Some(source)) = (&shared.manifest, &shared.config.source) else { return Vec::new() };
    let platform = zen_update::current_platform();
    let wanted: Vec<String> = shared
        .tools
        .iter()
        .filter(|tool| tool.download_size.is_none())
        .filter(|tool| matches!(tool.state, ToolState::Available { .. } | ToolState::Installed { update: Some(_), .. }))
        .map(|tool| tool.id.as_str())
        .chain(matches!(shared.self_update, SelfUpdate::Available(_)).then_some(TOOLBOX_ID))
        .filter_map(|id| manifest.latest(id, &source.channel, &platform))
        .filter(|(_, artifact)| artifact.size == 0)
        .map(|(_, artifact)| artifact_url(source, artifact))
        .filter(|url| !shared.sizes.contains_key(url) && !shared.sizes_asked.contains(url))
        .collect();
    shared.sizes_asked.extend(wanted.iter().cloned());
    wanted
}

fn ask_sizes(tools: &Tools) {
    let urls = unsized_artifacts(&mut tools.lock());
    if urls.is_empty() {
        return;
    }
    let tools = tools.clone();
    background(move || {
        for url in urls {
            if let Some(size) = content_length(&url) {
                let mut shared = tools.lock();
                shared.sizes.insert(url, size);
                recompute(&mut shared);
            }
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn content_length(url: &str) -> Option<u64> {
    zen_update::fetch::content_length(url, std::time::Duration::from_secs(15)).ok().flatten()
}

#[cfg(target_arch = "wasm32")]
fn content_length(_: &str) -> Option<u64> {
    None
}

fn sync_login_item(shared: &mut Shared) {
    if shared.demo {
        return;
    }
    let Some(item) = &shared.config.login_item else { return };
    if let Err(error) = item.set(shared.config.launch_at_login) {
        shared.messages.push(t!("toolbox-login-failed", error = error.as_str()));
    }
}

fn shell_scripts_location(shell_scripts: &ShellScripts, requested: Option<PathBuf>) -> Option<PathBuf> {
    requested
        .or_else(|| shell_scripts.location.clone())
        .or_else(launchers::default_location)
        .map(|path| launchers::expand_home(&path))
}

fn followed_location(shell_scripts: &ShellScripts) -> Option<PathBuf> {
    let location = shell_scripts.location.as_deref().filter(|_| shell_scripts.enabled)?;
    Some(launchers::expand_home(location)).filter(|path| path.is_absolute())
}

fn refresh_shell_scripts(shared: &mut Shared) {
    if shared.demo {
        return;
    }
    let Some(location) = followed_location(&shared.config.shell_scripts) else { return };
    let installations = shared.config.installations();
    let home = shared.config.defaults.home.clone();
    if let Err(error) = generate_with_completions(&installations, &location, home.as_deref()) {
        shared.messages.push(t!("toolbox-shell-scripts-failed", error = error.as_str()));
    }
}

fn generate_with_completions(
    installations: &[Installation],
    location: &std::path::Path,
    home: Option<&std::path::Path>,
) -> Result<launchers::Report, String> {
    let shell = launchers::Shell::current();
    let mut report = launchers::generate(&catalog(), installations, location, shell)?;
    if let Some(home) = home {
        launchers::follow_with_completions(&mut report, home, shell)?;
    }
    Ok(report)
}

fn remove_with_completions(
    location: &std::path::Path,
    home: Option<&std::path::Path>,
) -> Result<launchers::Report, String> {
    let mut report = launchers::remove(&catalog(), location, launchers::Shell::current())?;
    if let Some(home) = home {
        launchers::remove_completions(&mut report, home)?;
    }
    Ok(report)
}

fn follow_shell_scripts(shared: &mut Shared, before: &ShellScripts) {
    let (old, new) = (followed_location(before), followed_location(&shared.config.shell_scripts));
    if old == new {
        return;
    }
    let home = shared.config.defaults.home.clone();
    if let Some(old) = old
        && let Err(error) = remove_with_completions(&old, home.as_deref())
    {
        shared.messages.push(t!("toolbox-shell-scripts-failed", error = error.as_str()));
    }
    refresh_shell_scripts(shared);
}

fn newer(a: &str, b: &str) -> bool {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(a), Ok(b)) => a > b,
        (Ok(_), Err(_)) => true,
        _ => false,
    }
}

pub fn catalog() -> Vec<Tool> {
    const PRODUCTS: &[(&str, &str)] = &[
        ("RustRonin", "Rust"),
        ("GoGarden", "Go"),
        ("PyPetal", "Python"),
        ("JvmJizo", "for Java and the JVM"),
        ("WebWave", "The web: TypeScript, JavaScript, HTML and CSS"),
        ("CppCrane", "C and C++"),
        ("SharpShuriken", "C# and .NET"),
        ("PhpPagoda", "PHP"),
        ("RubyRyokan", "Ruby"),
        ("SqlScroll", "SQL and databases"),
        ("DevopsDaruma", "DevOps: containers, infrastructure as code, pipelines"),
        ("TestTotami", "QA: end-to-end and UI tests, API and load testing, test management"),
    ];
    PRODUCTS
        .iter()
        .map(|&(name, subject)| {
            let id = name.to_lowercase();
            Tool {
                bundle_id: format!("space.ifzenelse.{id}"),
                launcher: id.clone(),
                id,
                name: name.to_owned(),
                subject: subject.to_owned(),
                state: ToolState::ComingSoon,
                download_size: None,
            }
        })
        .collect()
}

pub fn demo() -> Vec<Tool> {
    let mut tools = catalog();
    let mut set = |id: &str, state: ToolState, size: Option<u64>| {
        if let Some(tool) = tools.iter_mut().find(|tool| tool.id == id) {
            tool.state = state;
            tool.download_size = size;
        }
    };
    let installed = |version: &str, update: Option<&str>, running: bool| ToolState::Installed {
        version: version.to_owned(),
        update: update.map(str::to_owned),
        running,
    };
    set("rustronin", installed("0.1.0", Some("0.2.0"), true), Some(48_300_000));
    set("gogarden", installed("0.1.0", Some("0.1.1"), false), Some(41_200_000));
    set("pypetal", installed("0.1.1", None, false), None);
    set("webwave", ToolState::Available { version: "0.1.0".to_owned() }, Some(37_600_000));
    tools
}

fn demo_manifest() -> Manifest {
    const RELEASES: &[(&str, &str, &str, &str, &str)] = &[
        (
            "rustronin",
            "RustRonin",
            "0.1.0",
            "2026-10-02",
            "### Added\n\n- The first window: project tree, tabs and an editor with line numbers.\n- Rust highlighting.",
        ),
        (
            "rustronin",
            "RustRonin",
            "0.2.0",
            "2026-10-16",
            "### Added\n\n- **Find in Files** (`⇧⌘F`) with a preview of each match.\n- The Structure tool window.\n\n\
             ### Fixed\n\n- The caret no longer jumps after *Undo* in a long line.",
        ),
        ("gogarden", "GoGarden", "0.1.0", "2026-10-10", "The first release: Go highlighting and the IDE window."),
        (
            "gogarden",
            "GoGarden",
            "0.1.1",
            "2026-10-16",
            "- Constants are highlighted wherever they are used.\n- A file opened from the command line opens with its folder.",
        ),
        ("pypetal", "PyPetal", "0.1.1", "2026-10-12", ""),
        ("webwave", "WebWave", "0.1.0", "2026-10-15", "The first release of the web IDE."),
        ("toolbox", "Zen Toolbox", "0.0.9", "2026-10-12", "- Installs, updates and launches the IDEs."),
        (
            "toolbox",
            "Zen Toolbox",
            "0.1.0",
            "2026-10-16",
            "### Added\n\n- **What's new**: release notes for every tool, and for Zen Toolbox after it updates itself.",
        ),
    ];
    let mut manifest = Manifest { schema: zen_update::SCHEMA, products: Vec::new() };
    for &(id, name, version, date, notes) in RELEASES {
        let artifact = zen_update::Artifact {
            platform: zen_update::current_platform(),
            url: format!("artifacts/{id}/{version}/demo.tar.gz"),
            sha256: String::new(),
            size: 0,
        };
        let release = zen_update::Release {
            version: version.to_owned(),
            channel: "stable".to_owned(),
            date: date.to_owned(),
            notes: notes.to_owned(),
            artifacts: vec![artifact],
        };
        manifest.add(id, name, release);
    }
    manifest
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTools)?;
    registry.register(RefreshReleases)?;
    registry.register(InstallTool)?;
    registry.register(UpdateTool)?;
    registry.register(UpdateAllTools)?;
    registry.register(ToolVersions)?;
    registry.register(InstallToolVersion)?;
    registry.register(UninstallTool)?;
    registry.register(LaunchTool)?;
    registry.register(UpdateSelf)?;
    registry.register(RestartSelf)?;
    registry.register(ReleaseNotes)?;
    registry.register(GetSettings)?;
    registry.register(SetSettings)?;
    registry.register(GenerateShellScripts)?;
    registry.register(RemoveShellScripts)?;
    registry.register(ShellScriptsStatus)?;
    registry.register(AddShellScriptsToPath)?;
    registry.register(AddShellCompletions)?;
    registry.register(ListProjects)?;
    registry.register(OpenProject)?;
    registry.register(LoginStatusCommand)?;
    registry.register(CheckNotifications)?;
    registry.register(auto_update::SetAutoUpdate)?;
    registry.register(auto_update::AutoUpdateStatus)?;
    registry.register(ListLicences)?;
    Ok(())
}

/// Parameters naming one tool.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ToolParams {
    /// The tool's id, e.g. `rustronin`.
    pub id: String,
}

fn tools(session: &Session) -> Result<Tools, CommandError> {
    session.state::<Tools>().cloned().ok_or_else(|| CommandError::new(t!("toolbox-error-not-initialised")))
}

fn not_in_demo(tools: &Tools) -> Result<(), CommandError> {
    if tools.lock().demo { Err(CommandError::new(t!("toolbox-error-demo"))) } else { Ok(()) }
}

fn installation(tools: &Tools, id: &str) -> Result<Installation, CommandError> {
    let config = tools.lock().config.clone();
    let root = config
        .installed_root(id)
        .or_else(|| config.tools_dir.clone())
        .ok_or_else(|| CommandError::new(t!("toolbox-error-no-tools-dir")))?;
    Ok(config.installation_in(root))
}

fn tool(tools: &Tools, id: &str) -> Result<Tool, CommandError> {
    tools
        .lock()
        .tools
        .iter()
        .find(|tool| tool.id == id)
        .cloned()
        .ok_or_else(|| CommandError::new(t!("toolbox-error-unknown-tool", id = id)))
}

pub struct ListTools;

impl Command for ListTools {
    type Params = NoParams;
    type Output = Vec<Tool>;
    const ID: &'static str = "toolbox.tools.list";
    const TITLE: &'static str = "List Tools";
    const DESCRIPTION: &'static str = "Lists every tool Zen Toolbox knows, with its installation state.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<Tool>, CommandError> {
        Ok(tools(session)?.snapshot().tools)
    }
}

pub struct RefreshReleases;

impl Command for RefreshReleases {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "toolbox.releases.refresh";
    const TITLE: &'static str = "Check for Updates";
    const DESCRIPTION: &'static str = "Fetches the release manifest and updates which versions are available.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Tools>().is_some_and(|tools| {
            let shared = tools.lock();
            shared.config.source.is_some() && !shared.refreshing
        })
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        let tools = tools(session)?;
        let source =
            tools.lock().config.source.clone().ok_or_else(|| CommandError::new(t!("toolbox-error-no-server")))?;
        tools.lock().refreshing = true;
        background(move || {
            let result = fetch_manifest(&source);
            let update_self = {
                let mut shared = tools.lock();
                shared.refreshing = false;
                shared.last_check = Some(std::time::Instant::now());
                match result {
                    Ok(manifest) => {
                        shared.manifest = Some(manifest);
                        recompute(&mut shared);
                    }
                    Err(error) => shared.server = ServerStatus::Unreachable(error),
                }
                shared.config.updates_self == UpdateMode::Automatic
                    && matches!(shared.self_update, SelfUpdate::Available(_))
            };
            ask_sizes(&tools);
            if update_self {
                let _ = start_self_update(&tools);
            }
        });
        Ok(())
    }
}

pub struct CheckNotifications;

impl Command for CheckNotifications {
    type Params = NoParams;
    type Output = Vec<notifications::Notice>;
    const ID: &'static str = "toolbox.notifications.check";
    const TITLE: &'static str = "Check Notifications";
    const DESCRIPTION: &'static str = "Shows a system notification for every new version not announced yet, once per \
        version, unless toolbox.notifications.enabled is off, and returns what it showed. It reads the releases \
        Toolbox knows; toolbox.releases.refresh fetches them.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<notifications::Notice>, CommandError> {
        Ok(notifications::follow(session))
    }
}

pub struct LoginStatusCommand;

impl Command for LoginStatusCommand {
    type Params = NoParams;
    type Output = login::LoginStatus;
    const ID: &'static str = "toolbox.login.status";
    const TITLE: &'static str = "Launch at Login Status";
    const DESCRIPTION: &'static str = "Reports whether Zen Toolbox starts in the tray at login: the \
        toolbox.launchAtLogin setting, whether this build manages a login item, whether the item is in place, \
        and its file. Change it with toolbox.settings.set {\"launchAtLogin\": false}.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<login::LoginStatus, CommandError> {
        Ok(tools(session)?.login_status())
    }
}

pub struct InstallTool;

impl Command for InstallTool {
    type Params = ToolParams;
    type Output = ();
    const ID: &'static str = "toolbox.tool.install";
    const TITLE: &'static str = "Install";
    const DESCRIPTION: &'static str = "Downloads and installs the newest version of a tool on its channel.";

    fn run(&self, session: &mut Session, params: ToolParams) -> Result<(), CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let tool = tool(&tools, &params.id)?;
        if tool.state == ToolState::ComingSoon {
            return Err(CommandError::new(t!("toolbox-error-not-released", tool = tool.name.as_str())));
        }
        start_install(&tools, &tool, None)
    }
}

/// One release of a tool that can be installed here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolVersion {
    /// The version, e.g. `0.2.0`.
    pub version: String,
    /// The release date, `YYYY-MM-DD`.
    pub date: String,
    /// The download size in bytes, when the manifest gives it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_size: Option<u64>,
    /// Whether this version is the one installed.
    pub installed: bool,
}

pub struct ToolVersions;

impl Command for ToolVersions {
    type Params = ToolParams;
    type Output = Vec<ToolVersion>;
    const ID: &'static str = "toolbox.tool.versions";
    const TITLE: &'static str = "Available Versions";
    const DESCRIPTION: &'static str = "Lists the versions of a tool in the signed release manifest for this platform \
        on the selected channel, newest first, marking the one installed.";

    fn run(&self, session: &mut Session, params: ToolParams) -> Result<Vec<ToolVersion>, CommandError> {
        let tools = tools(session)?;
        let tool = tool(&tools, &params.id)?;
        tools.versions(&tool.id)
    }
}

/// Parameters of `toolbox.tool.installVersion`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InstallVersionParams {
    /// The tool's id, e.g. `rustronin`.
    pub id: String,
    /// The version from `toolbox.tool.versions`, e.g. `0.2.0`.
    pub version: String,
}

pub struct InstallToolVersion;

impl Command for InstallToolVersion {
    type Params = InstallVersionParams;
    type Output = ();
    const ID: &'static str = "toolbox.tool.installVersion";
    const TITLE: &'static str = "Install Version";
    const DESCRIPTION: &'static str = "Downloads and installs a chosen version of a tool, older or newer than the \
        installed one, in its place: this is how a tool rolls back. Settings and projects stay. The version must be \
        one `toolbox.tool.versions` lists.";

    fn run(&self, session: &mut Session, params: InstallVersionParams) -> Result<(), CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::new(t!("toolbox-error-person-chooses")));
        }
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let tool = tool(&tools, &params.id)?;
        if matches!(&tool.state, ToolState::Installed { version, .. } if *version == params.version) {
            let (tool, version) = (tool.name.as_str(), params.version.as_str());
            return Err(CommandError::new(t!("toolbox-error-version-installed", tool = tool, version = version)));
        }
        start_install(&tools, &tool, Some(&params.version))?;
        auto_update::hold_after_going_back(session, &tool, &params.version);
        Ok(())
    }
}

pub struct UpdateTool;

impl Command for UpdateTool {
    type Params = ToolParams;
    type Output = ();
    const ID: &'static str = "toolbox.tool.update";
    const TITLE: &'static str = "Update";
    const DESCRIPTION: &'static str = "Updates an installed tool to the newest version on its channel.";

    fn run(&self, session: &mut Session, params: ToolParams) -> Result<(), CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let tool = tool(&tools, &params.id)?;
        if !matches!(tool.state, ToolState::Installed { update: Some(_), .. }) {
            return Err(CommandError::new(t!("toolbox-error-up-to-date", tool = tool.name.as_str())));
        }
        start_install(&tools, &tool, None)
    }
}

pub struct UpdateAllTools;

impl Command for UpdateAllTools {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "toolbox.tools.updateAll";
    const TITLE: &'static str = "Update All";
    const DESCRIPTION: &'static str = "Updates every installed tool that has an update available.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Tools>().is_some_and(|tools| tools.updates() > 0)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let outdated: Vec<Tool> = tools
            .snapshot()
            .tools
            .into_iter()
            .filter(|tool| matches!(tool.state, ToolState::Installed { update: Some(_), .. }))
            .collect();
        for tool in &outdated {
            start_install(&tools, tool, None)?;
        }
        Ok(())
    }
}

pub struct UninstallTool;

impl Command for UninstallTool {
    type Params = ToolParams;
    type Output = ();
    const ID: &'static str = "toolbox.tool.uninstall";
    const TITLE: &'static str = "Uninstall";
    const DESCRIPTION: &'static str = "Removes a tool and all its installed versions.";

    fn run(&self, session: &mut Session, params: ToolParams) -> Result<(), CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let tool = tool(&tools, &params.id)?;
        installation(&tools, &tool.id)?.uninstall(&tool.id).map_err(|error| CommandError::new(error.to_string()))?;
        {
            let mut shared = tools.lock();
            recompute(&mut shared);
            refresh_shell_scripts(&mut shared);
        }
        tools.post(t!("toolbox-uninstalled", tool = tool.name.as_str()));
        Ok(())
    }
}

/// Parameters of `toolbox.tool.launch`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
pub struct LaunchParams {
    /// The tool's id, e.g. `gogarden`.
    pub id: String,
    /// A project folder for the tool to open; without it the tool starts (or comes to the front) as it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

pub struct LaunchTool;

impl Command for LaunchTool {
    type Params = LaunchParams;
    type Output = ();
    const ID: &'static str = "toolbox.tool.launch";
    const TITLE: &'static str = "Launch";
    const DESCRIPTION: &'static str = "Starts an installed tool. With `path`, the tool opens that folder as a project.";

    fn run(&self, session: &mut Session, params: LaunchParams) -> Result<(), CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        launch(&tools, &params.id, params.path.as_deref())
    }
}

fn launch(tools: &Tools, id: &str, path: Option<&std::path::Path>) -> Result<(), CommandError> {
    let tool = tool(tools, id)?;
    let args = match path {
        Some(path) => vec![project_folder(path)?.into_os_string()],
        None => Vec::new(),
    };
    let line = installation(tools, &tool.id)?
        .launch_with(&tool.id, &args)
        .ok_or_else(|| CommandError::new(t!("toolbox-error-not-installed", tool = tool.name.as_str())))?;
    let launcher = tools.lock().launcher.clone();
    let started = match launcher {
        Some(launcher) => launcher(&line),
        None => line.to_command().spawn().map(drop),
    };
    started.map_err(|error| {
        CommandError::new(t!("toolbox-error-cannot-start", tool = tool.name.as_str(), error = error.to_string()))
    })
}

fn project_folder(path: &std::path::Path) -> Result<PathBuf, CommandError> {
    let shown = path.display().to_string();
    if !path.exists() {
        return Err(CommandError::new(t!("toolbox-error-no-folder", path = shown.as_str())));
    }
    if !path.is_dir() {
        return Err(CommandError::new(t!("toolbox-error-not-a-folder", path = shown.as_str())));
    }
    std::fs::canonicalize(path).map_err(|_| CommandError::new(t!("toolbox-error-no-folder", path = shown.as_str())))
}

pub struct UpdateSelf;

impl Command for UpdateSelf {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "toolbox.self.update";
    const TITLE: &'static str = "Update Zen Toolbox";
    const DESCRIPTION: &'static str =
        "Downloads the newer Zen Toolbox and replaces this one; it takes effect after a restart.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Tools>().is_some_and(|tools| matches!(tools.lock().self_update, SelfUpdate::Available(_)))
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        start_self_update(&tools(session)?)
    }
}

fn start_self_update(tools: &Tools) -> Result<(), CommandError> {
    not_in_demo(tools)?;
    let (source, manifest, executable, version) = {
        let shared = tools.lock();
        let SelfUpdate::Available(version) = shared.self_update.clone() else {
            return Err(CommandError::new(t!("toolbox-error-self-up-to-date")));
        };
        (
            shared.config.source.clone().ok_or_else(|| CommandError::new(t!("toolbox-error-no-server")))?,
            shared.manifest.clone().ok_or_else(|| CommandError::new(t!("toolbox-error-no-manifest")))?,
            shared
                .config
                .self_executable
                .clone()
                .ok_or_else(|| CommandError::new(t!("toolbox-error-no-executable")))?,
            version,
        )
    };
    let (_, artifact) = manifest
        .latest(TOOLBOX_ID, &source.channel, &zen_update::current_platform())
        .ok_or_else(|| CommandError::new(t!("toolbox-error-no-build", tool = "Zen Toolbox")))?;
    let artifact = artifact.clone();
    tools.lock().self_update =
        SelfUpdate::Downloading(version.clone(), Job { done: 0, total: artifact.size, unpacking: false });
    let worker = tools.clone();
    background(move || {
        let result = self_update(&worker, &source, &artifact, &executable, &version);
        let mut shared = worker.lock();
        match result {
            Ok(()) => {
                shared.self_update = SelfUpdate::ReadyToRestart(version.clone());
                shared.messages.push(t!("toolbox-self-installed", version = version.as_str()));
            }
            Err(error) => {
                shared.self_update = SelfUpdate::Available(version);
                shared.messages.push(t!("toolbox-self-failed", error = error.as_str()));
            }
        }
    });
    Ok(())
}

pub struct RestartSelf;

impl Command for RestartSelf {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "toolbox.self.restart";
    const TITLE: &'static str = "Restart Zen Toolbox";
    const DESCRIPTION: &'static str = "Restarts Zen Toolbox so that an installed update takes effect.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Tools>().is_some_and(|tools| matches!(tools.lock().self_update, SelfUpdate::ReadyToRestart(_)))
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        tools(session)?.lock().restart_requested = true;
        Ok(())
    }
}

/// Parameters of `toolbox.tool.releaseNotes`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseNotesParams {
    /// The tool's id, e.g. `rustronin`, or `toolbox` for Zen Toolbox itself.
    pub id: String,
    /// Only versions newer than this one, e.g. the installed version, to see what an update brings.
    #[serde(default)]
    pub newer_than: Option<String>,
}

/// The open-source licences shipped with Zen Toolbox.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Licences {
    /// The folder that holds them, absent in a development build.
    pub folder: Option<String>,
    /// Their files inside the folder, the third-party notices first.
    pub files: Vec<String>,
}

pub fn licences() -> Licences {
    let folder = zen_update::licences::bundled();
    Licences {
        files: folder.as_deref().map(zen_update::licences::files).unwrap_or_default(),
        folder: folder.map(|folder| folder.display().to_string()),
    }
}

pub struct ListLicences;

impl Command for ListLicences {
    type Params = NoParams;
    type Output = Licences;
    const ID: &'static str = "toolbox.licences.list";
    const TITLE: &'static str = "Open-source Licences";
    const DESCRIPTION: &'static str = "Lists the licence files shipped with Zen Toolbox (its licences, the third-party \
         notices of every library and the font licences) and the folder that holds them; the folder is absent in a \
         development build. Settings shows them under Open-source Licences.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<Licences, CommandError> {
        Ok(licences())
    }
}

pub struct ReleaseNotes;

impl Command for ReleaseNotes {
    type Params = ReleaseNotesParams;
    type Output = Vec<ReleaseNote>;
    const ID: &'static str = "toolbox.tool.releaseNotes";
    const TITLE: &'static str = "What's New";
    const DESCRIPTION: &'static str =
        "Lists the release notes of a tool (or of Zen Toolbox), newest first, optionally only after a version.";

    fn run(&self, session: &mut Session, params: ReleaseNotesParams) -> Result<Vec<ReleaseNote>, CommandError> {
        let tools = tools(session)?;
        if params.id != TOOLBOX_ID {
            tool(&tools, &params.id)?;
        }
        tools
            .release_notes(&params.id, params.newer_than.as_deref(), None)
            .ok_or_else(|| CommandError::new(t!("toolbox-error-no-manifest")))
    }
}

fn defaults(session: &Session) -> Defaults {
    session.state::<Tools>().map_or_else(Defaults::for_this_user, Tools::defaults)
}

pub struct GetSettings;

impl Command for GetSettings {
    type Params = NoParams;
    type Output = ToolboxSettings;
    const ID: &'static str = "toolbox.settings.get";
    const TITLE: &'static str = "Settings";
    const DESCRIPTION: &'static str = "Returns every Zen Toolbox setting (tools location, channel, updates, shell scripts, appearance), with defaults filled in.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ToolboxSettings, CommandError> {
        Ok(ToolboxSettings::read(session.settings(), &defaults(session)))
    }
}

pub struct SetSettings;

impl Command for SetSettings {
    type Params = SettingsPatch;
    type Output = ToolboxSettings;
    const ID: &'static str = "toolbox.settings.set";
    const TITLE: &'static str = "Change Settings";
    const DESCRIPTION: &'static str = "Validates and saves the given Zen Toolbox settings, leaving the others unchanged, \
         applies them (the channel at once, the tools location to new installations) and returns every setting.";

    fn run(&self, session: &mut Session, params: SettingsPatch) -> Result<ToolboxSettings, CommandError> {
        if params.updates_tools.as_deref() == Some("automatic") && zen_core::macros::by_agent(session) {
            return Err(CommandError::new(t!("toolbox-error-person-chooses")));
        }
        let defaults = defaults(session);
        params.apply(session.settings_mut(), &defaults)?;
        if let Some(language) = &params.language {
            zen_i18n::set_language(Some(language.as_str()));
        }
        if let Some(tools) = session.state::<Tools>() {
            tools.reconfigure(session.settings());
        }
        Ok(ToolboxSettings::read(session.settings(), &defaults))
    }
}

/// Parameters of `toolbox.shellScripts.generate` and `toolbox.shellScripts.remove`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShellScriptsParams {
    /// The directory of the launchers, absolute or starting with `~`; `toolbox.shellScripts.location` when omitted.
    #[serde(default)]
    pub location: Option<PathBuf>,
}

fn shell_scripts_dir(tools: &Tools, params: ShellScriptsParams) -> Result<PathBuf, CommandError> {
    let dir = tools
        .shell_scripts_location(params.location)
        .ok_or_else(|| CommandError::new(t!("toolbox-error-no-shell-scripts-location")))?;
    if dir.is_absolute() {
        Ok(dir)
    } else {
        Err(CommandError::new(t!("toolbox-error-launcher-relative", path = dir.display().to_string())))
    }
}

pub struct GenerateShellScripts;

impl Command for GenerateShellScripts {
    type Params = ShellScriptsParams;
    type Output = launchers::Report;
    const ID: &'static str = "toolbox.shellScripts.generate";
    const TITLE: &'static str = "Generate Shell Scripts";
    const DESCRIPTION: &'static str = "Writes a shell launcher named by the product id (gogarden, rustronin, ...) for \
        every installed tool, and `zen` for the zen-cli the IDEs ship, into a directory, with zsh, bash and fish \
        completions for `zen` in their usual folders, and deletes those of tools no longer installed; files Zen \
        Toolbox did not write are skipped. Reports the files and whether the directory is on PATH.";

    fn run(&self, session: &mut Session, params: ShellScriptsParams) -> Result<launchers::Report, CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let dir = shell_scripts_dir(&tools, params)?;
        let installations = tools.lock().config.installations();
        if installations.is_empty() {
            return Err(CommandError::new(t!("toolbox-error-no-tools-dir")));
        }
        generate_with_completions(&installations, &dir, tools.defaults().home.as_deref()).map_err(CommandError::new)
    }
}

pub struct RemoveShellScripts;

impl Command for RemoveShellScripts {
    type Params = ShellScriptsParams;
    type Output = launchers::Report;
    const ID: &'static str = "toolbox.shellScripts.remove";
    const TITLE: &'static str = "Remove Shell Scripts";
    const DESCRIPTION: &'static str = "Deletes the shell launchers Zen Toolbox wrote into a directory and its \
        completion files; files it did not write are skipped. Reports the files and whether the directory is on PATH.";

    fn run(&self, session: &mut Session, params: ShellScriptsParams) -> Result<launchers::Report, CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let dir = shell_scripts_dir(&tools, params)?;
        remove_with_completions(&dir, tools.defaults().home.as_deref()).map_err(CommandError::new)
    }
}

/// What `toolbox.shellScripts.status` says.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShellScriptsReport {
    /// Whether Zen Toolbox keeps the launchers current (`toolbox.shellScripts.enabled`).
    pub enabled: bool,
    /// The folder of the launchers.
    pub location: PathBuf,
    /// The launchers Zen wrote that are in the folder now.
    pub launchers: Vec<PathBuf>,
    /// Whether the folder is on the `PATH` of a new terminal window (the user's login shell is asked).
    pub on_path: bool,
    /// Whether a shell profile has the line already (then only terminal windows opened before lack it).
    pub in_profile: bool,
    /// The exact line `toolbox.shellScripts.addToPath` adds.
    pub line: String,
    /// The shell profiles it adds the line to.
    pub profiles: Vec<PathBuf>,
    /// Whether the shell profiles load the completions of `zen` (`toolbox.shellScripts.addCompletions`).
    pub completions: bool,
}

pub struct ShellScriptsStatus;

impl Command for ShellScriptsStatus {
    type Params = ShellScriptsParams;
    type Output = ShellScriptsReport;
    const ID: &'static str = "toolbox.shellScripts.status";
    const TITLE: &'static str = "Shell Scripts Status";
    const DESCRIPTION: &'static str = "Lists the shell launchers Zen wrote in the shell scripts folder and says \
        whether the folder is on the PATH of a new terminal window (asking the user's login shell), with the exact \
        line toolbox.shellScripts.addToPath would add and the shell profiles it would go to.";

    fn run(&self, session: &mut Session, params: ShellScriptsParams) -> Result<ShellScriptsReport, CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let dir = shell_scripts_dir(&tools, params)?;
        let launchers = ours(&dir);
        let status = zen_update::scripts::path_status(&dir, tools.defaults().home.as_deref());
        let enabled = {
            let mut shared = tools.lock();
            shared.path_status = Some((status.clone(), launchers.clone()));
            shared.config.shell_scripts.enabled
        };
        Ok(ShellScriptsReport {
            enabled,
            location: dir,
            launchers,
            on_path: status.on_path,
            in_profile: status.in_profile,
            line: status.line,
            profiles: status.profiles,
            completions: status.completions,
        })
    }
}

/// What `toolbox.shellScripts.addToPath` did.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddedToPath {
    /// The line added.
    pub line: String,
    /// The shell profiles changed; empty when every one had the line already.
    pub changed: Vec<PathBuf>,
}

pub struct AddShellScriptsToPath;

impl Command for AddShellScriptsToPath {
    type Params = ShellScriptsParams;
    type Output = AddedToPath;
    const ID: &'static str = "toolbox.shellScripts.addToPath";
    const TITLE: &'static str = "Add to PATH";
    const DESCRIPTION: &'static str = "Puts the shell scripts folder on PATH for new terminal windows: appends one \
        commented line, export PATH=\"<folder>:$PATH\", to ~/.zprofile (created if missing) and to ~/.bash_profile \
        if it exists, unless it is there already. It edits the user's shell files: run it only when the user asked \
        for it.";

    fn run(&self, session: &mut Session, params: ShellScriptsParams) -> Result<AddedToPath, CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let dir = shell_scripts_dir(&tools, params)?;
        let home = tools.defaults().home.ok_or_else(|| CommandError::new(t!("toolbox-error-no-home")))?;
        let changed = zen_update::scripts::add_to_profiles(&dir, &home)
            .map_err(|error| CommandError::new(t!("toolbox-error-profile", error = error.to_string())))?;
        if let Some((status, _)) = tools.lock().path_status.as_mut().filter(|(status, _)| status.location == dir) {
            status.in_profile = true;
        }
        tools.check_path();
        Ok(AddedToPath { line: zen_update::scripts::path_line(&dir, Some(&home)), changed })
    }
}

/// What `toolbox.shellScripts.addCompletions` did.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AddedCompletions {
    /// The shell profiles changed; empty when every one had its line already.
    pub changed: Vec<PathBuf>,
}

pub struct AddShellCompletions;

impl Command for AddShellCompletions {
    type Params = NoParams;
    type Output = AddedCompletions;
    const ID: &'static str = "toolbox.shellScripts.addCompletions";
    const TITLE: &'static str = "Turn On Shell Completions";
    const DESCRIPTION: &'static str = "Adds the line that loads the completions of `zen` to ~/.zprofile (and to \
        ~/.bash_profile if it exists), unless it is there. It edits the user's shell files, so run it only when they \
        asked to. Fish needs no line.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<AddedCompletions, CommandError> {
        let tools = tools(session)?;
        not_in_demo(&tools)?;
        let home = tools.defaults().home.ok_or_else(|| CommandError::new(t!("toolbox-error-no-home")))?;
        let changed = zen_update::scripts::add_completions_to_profiles(&home)
            .map_err(|error| CommandError::new(t!("toolbox-error-profile", error = error.to_string())))?;
        Ok(AddedCompletions { changed })
    }
}

fn background(work: impl FnOnce() + Send + 'static) {
    std::thread::spawn(work);
}

#[cfg(not(target_arch = "wasm32"))]
fn fetch_manifest(source: &Source) -> Result<Manifest, String> {
    zen_update::fetch::manifest(&source.manifest_url, &source.public_key).map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
fn fetch_manifest(_: &Source) -> Result<Manifest, String> {
    Err("downloads are not available in the browser".to_owned())
}

fn start_install(tools: &Tools, tool: &Tool, chosen: Option<&str>) -> Result<(), CommandError> {
    let (source, manifest) = {
        let shared = tools.lock();
        if shared.jobs.contains_key(&tool.id) {
            return Err(CommandError::new(t!("toolbox-error-busy", tool = tool.name.as_str())));
        }
        (
            shared.config.source.clone().ok_or_else(|| CommandError::new(t!("toolbox-error-no-server")))?,
            shared.manifest.clone().ok_or_else(|| CommandError::new(t!("toolbox-error-no-manifest")))?,
        )
    };
    let installation = installation(tools, &tool.id)?;
    let platform = zen_update::current_platform();
    let found = match chosen {
        Some(version) => manifest
            .versions(&tool.id, &source.channel, &platform)
            .into_iter()
            .find(|(release, _)| release.version == version),
        None => manifest.latest(&tool.id, &source.channel, &platform),
    };
    let (release, artifact) = found.ok_or_else(|| {
        CommandError::new(match chosen {
            Some(version) => t!("toolbox-error-no-version", tool = tool.name.as_str(), version = version),
            None => t!("toolbox-error-no-build", tool = tool.name.as_str()),
        })
    })?;
    let (version, artifact) = (release.version.clone(), artifact.clone());
    let told = quiet_update_of(tools, &tool.id);
    tools.lock().jobs.insert(tool.id.clone(), Job { done: 0, total: artifact.size, unpacking: false });

    let (worker, tool) = (tools.clone(), tool.clone());
    background(move || {
        let result = download(&worker, &tool.id, &source, &artifact)
            .and_then(|bytes| installation.install(&tool.id, &version, &bytes).map_err(|error| error.to_string()));
        let mut shared = worker.lock();
        shared.jobs.remove(&tool.id);
        let notice = match result {
            Ok(()) => {
                shared.messages.push(t!("toolbox-installed", tool = tool.name.as_str(), version = version.as_str()));
                told.then(|| auto_update::updated(&tool.id, &tool.name, &version))
            }
            Err(error) => {
                shared.messages.push(t!("toolbox-install-failed", tool = tool.name.as_str(), error = error.as_str()));
                told.then(|| auto_update::failed(&tool.id, &tool.name, &version, &error))
            }
        };
        recompute(&mut shared);
        refresh_shell_scripts(&mut shared);
        drop(shared);
        if let Some(notice) = notice {
            worker.notify(&notice);
        }
    });
    Ok(())
}

fn quiet_update_of(tools: &Tools, id: &str) -> bool {
    tools.lock().automatic_updates.remove(id)
}

#[cfg(not(target_arch = "wasm32"))]
fn download(tools: &Tools, id: &str, source: &Source, artifact: &zen_update::Artifact) -> Result<Vec<u8>, String> {
    let bytes = zen_update::fetch::artifact(&source.manifest_url, artifact, |done, total| {
        if let Some(job) = tools.lock().jobs.get_mut(id) {
            job.done = done;
            if total > 0 {
                job.total = total;
            }
        }
    })
    .map_err(|error| error.to_string())?;
    if let Some(job) = tools.lock().jobs.get_mut(id) {
        job.unpacking = true;
    }
    Ok(bytes)
}

#[cfg(target_arch = "wasm32")]
fn download(_: &Tools, _: &str, _: &Source, _: &zen_update::Artifact) -> Result<Vec<u8>, String> {
    Err("downloads are not available in the browser".to_owned())
}

#[cfg(not(target_arch = "wasm32"))]
fn self_update(
    tools: &Tools,
    source: &Source,
    artifact: &zen_update::Artifact,
    executable: &std::path::Path,
    version: &str,
) -> Result<(), String> {
    let bytes = zen_update::fetch::artifact(&source.manifest_url, artifact, |done, total| {
        if let SelfUpdate::Downloading(_, job) = &mut tools.lock().self_update {
            job.done = done;
            if total > 0 {
                job.total = total;
            }
        }
    })
    .map_err(|error| error.to_string())?;

    let staging = std::env::temp_dir().join(format!("zen-toolbox-update-{version}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    zen_update::install::unpack_tar_gz(&bytes, &staging).map_err(|error| error.to_string())?;
    let result = match (zen_update::install::bundle_of(executable), zen_update::install::bundle_in_release(&staging)) {
        (Some(current), Some(new)) => {
            zen_update::install::replace_bundle(&current, &new).map(drop).map_err(|error| error.to_string())
        }
        _ => {
            let binary = staging.join("bin").join(zen_update::install::executable_name("zen-toolbox"));
            let new_executable = std::fs::read(&binary).map_err(|_| "the archive has no bin/zen-toolbox".to_owned())?;
            zen_update::install::replace_executable(executable, &new_executable)
                .map(drop)
                .map_err(|error| error.to_string())
        }
    };
    let _ = std::fs::remove_dir_all(&staging);
    result
}

#[cfg(target_arch = "wasm32")]
fn self_update(
    _tools: &Tools,
    _source: &Source,
    _artifact: &zen_update::Artifact,
    _executable: &std::path::Path,
    _version: &str,
) -> Result<(), String> {
    Err("downloads are not available in the browser".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_lists_every_product_with_its_bundle_id() {
        let catalog = catalog();
        assert_eq!(catalog.len(), 12);
        assert_eq!(catalog[0].id, "rustronin");
        assert_eq!(catalog[10].bundle_id, "space.ifzenelse.devopsdaruma");
        assert_eq!(catalog[11].bundle_id, "space.ifzenelse.testtotami");
        assert_eq!(catalog[11].launcher, "testtotami");
        assert_eq!(catalog[1].launcher, "gogarden");
        assert_eq!(catalog[0].bundle_id, "space.ifzenelse.rustronin");
        assert!(catalog.iter().all(|tool| tool.state == ToolState::ComingSoon));
    }

    #[test]
    fn demo_commands_do_not_pretend() {
        zen_i18n::register(LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Tools::demo());

        let list = registry.dispatch(&mut session, "toolbox.tools.list", serde_json::Value::Null).unwrap();
        assert_eq!(list[0]["state"]["kind"], "installed");
        assert!(registry.is_available(&session, "toolbox.tools.updateAll"));
        let error = registry.run::<UpdateTool>(&mut session, ToolParams { id: "rustronin".into() }).unwrap_err();
        assert!(error.to_string().contains("demo data"));
    }

    #[test]
    fn states_come_from_the_tools_directory_and_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path());
        installation.install("rustronin", "0.1.0", &archive("rustronin")).unwrap();

        let tools = Tools::new(Config {
            tools_dir: Some(dir.path().to_owned()),
            self_version: "0.0.0".into(),
            self_executable: Some("/nonexistent/zen-toolbox".into()),
            ..Config::default()
        });
        let mut manifest = Manifest { schema: zen_update::SCHEMA, products: Vec::new() };
        let platform = zen_update::current_platform();
        for (id, version) in [("rustronin", "0.2.0"), ("gogarden", "0.1.0"), (TOOLBOX_ID, "0.0.1")] {
            manifest.add(
                id,
                id,
                zen_update::Release {
                    version: version.into(),
                    channel: "stable".into(),
                    date: "2026-10-16".into(),
                    notes: String::new(),
                    artifacts: vec![zen_update::Artifact {
                        platform: platform.clone(),
                        url: "artifacts/x".into(),
                        sha256: String::new(),
                        size: 0,
                    }],
                },
            );
        }
        {
            let mut shared = tools.lock();
            shared.manifest = Some(manifest);
            recompute(&mut shared);
        }
        let snapshot = tools.snapshot();
        let state = |id: &str| snapshot.tools.iter().find(|tool| tool.id == id).unwrap().state.clone();
        assert_eq!(
            state("rustronin"),
            ToolState::Installed { version: "0.1.0".into(), update: Some("0.2.0".into()), running: false }
        );
        assert_eq!(state("gogarden"), ToolState::Available { version: "0.1.0".into() });
        assert_eq!(state("sqlscroll"), ToolState::ComingSoon);
        assert_eq!(snapshot.tools[0].id, "rustronin", "installed tools come first");
        assert_eq!(snapshot.self_update, SelfUpdate::Available("0.0.1".into()));
        assert_eq!(snapshot.server, ServerStatus::Online { releases: 2 });
    }

    fn served_releases(versions: &[&str]) -> (String, Manifest) {
        use std::io::{BufRead, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let bytes = archive("gogarden");
        let mut manifest = Manifest { schema: zen_update::SCHEMA, products: Vec::new() };
        for version in versions {
            manifest.add(
                "gogarden",
                "GoGarden",
                zen_update::Release {
                    version: (*version).into(),
                    channel: "stable".into(),
                    date: "2026-10-17".into(),
                    notes: String::new(),
                    artifacts: vec![zen_update::Artifact {
                        platform: zen_update::current_platform(),
                        url: format!("artifacts/gogarden/{version}.tar.gz"),
                        sha256: zen_update::sha256_hex(&bytes),
                        size: bytes.len() as u64,
                    }],
                },
            );
        }
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = std::io::BufReader::new(stream);
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                    line.clear();
                }
                let mut stream = reader.into_inner();
                let length = bytes.len();
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(&bytes);
            }
        });
        (format!("http://{address}/releases/manifest.json"), manifest)
    }

    #[test]
    fn an_older_version_replaces_the_installed_one_and_keeps_settings_and_projects() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let (url, manifest) = served_releases(&["0.2.0", "0.1.0"]);
        let (registry, mut session, tools) = settings_session(dir.path(), CommandLine::default());
        let root = tools.lock().config.tools_dir.clone().unwrap();
        let settings = dir.path().join("data/space.ifzenelse.gogarden/settings.json");
        std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
        std::fs::write(&settings, "{}").unwrap();
        let project = dir.path().join("work/app/main.go");
        std::fs::create_dir_all(project.parent().unwrap()).unwrap();
        std::fs::write(&project, "package main\n").unwrap();
        tools.lock().config.installation_in(root.clone()).install("gogarden", "0.2.0", &archive("gogarden")).unwrap();
        {
            let mut shared = tools.lock();
            shared.config.source =
                Some(Source { manifest_url: url, public_key: String::new(), channel: "stable".into() });
            shared.manifest = Some(manifest);
            recompute(&mut shared);
        }

        let listed = registry.dispatch(&mut session, "toolbox.tool.versions", json!({ "id": "gogarden" })).unwrap();
        let versions: Vec<ToolVersion> = serde_json::from_value(listed).unwrap();
        assert_eq!(
            versions.iter().map(|version| (version.version.as_str(), version.installed)).collect::<Vec<_>>(),
            [("0.2.0", true), ("0.1.0", false)]
        );
        assert!(versions[0].download_size.is_some());

        let installed = json!({ "id": "gogarden", "version": "0.2.0" });
        assert!(registry.dispatch(&mut session, "toolbox.tool.installVersion", installed).is_err());
        let unknown = json!({ "id": "gogarden", "version": "9.9.9" });
        assert!(registry.dispatch(&mut session, "toolbox.tool.installVersion", unknown).is_err());

        let older = json!({ "id": "gogarden", "version": "0.1.0" });
        registry.dispatch(&mut session, "toolbox.tool.installVersion", older).unwrap();
        let started = std::time::Instant::now();
        while tools.busy() {
            assert!(started.elapsed() < std::time::Duration::from_secs(30), "{:?}", tools.take_messages());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(
            Installation::new(&root).current("gogarden").as_deref(),
            Some("0.1.0"),
            "{:?}",
            tools.take_messages()
        );
        assert!(!root.join("gogarden/0.2.0").exists(), "one version on disk, as ever");
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{}");
        assert_eq!(std::fs::read_to_string(&project).unwrap(), "package main\n");
        assert_eq!(
            gogarden(&tools),
            ToolState::Installed { version: "0.1.0".into(), update: Some("0.2.0".into()), running: false },
            "the newest version is offered again"
        );
    }

    #[test]
    fn versions_wait_for_the_manifest_and_follow_the_channel() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, tools) = settings_session(dir.path(), CommandLine::default());
        let ask = json!({ "id": "gogarden" });
        tools.lock().manifest = None;
        assert!(registry.dispatch(&mut session, "toolbox.tool.versions", ask.clone()).is_err());
        tools.lock().manifest = Some(manifest(&[("gogarden", "0.1.0", "stable"), ("gogarden", "0.2.0", "nightly")]));
        let listed = registry.dispatch(&mut session, "toolbox.tool.versions", ask).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1, "{listed}");
        assert_eq!(listed[0]["version"], "0.1.0");
    }

    struct Automatic {
        registry: Registry,
        session: Session,
        tools: Tools,
        root: PathBuf,
        told: Arc<Mutex<Vec<notifications::Notice>>>,
        running: Arc<Mutex<Vec<PathBuf>>>,
        _dir: tempfile::TempDir,
    }

    fn automatic(installed: &str, versions: &[&str]) -> Automatic {
        let dir = tempfile::tempdir().unwrap();
        let (url, manifest) = served_releases(versions);
        let (registry, session, tools) = settings_session(dir.path(), CommandLine::default());
        let told = Arc::new(Mutex::new(Vec::new()));
        let running = Arc::new(Mutex::new(Vec::new()));
        let (sent, programs) = (told.clone(), running.clone());
        let tools = tools
            .with_notifier(Arc::new(move |notice: &notifications::Notice| sent.lock().unwrap().push(notice.clone())))
            .with_running_programs(Arc::new(move || Some(programs.lock().unwrap().clone())));
        let root = tools.lock().config.tools_dir.clone().unwrap();
        tools.lock().config.installation_in(root.clone()).install("gogarden", installed, &archive("gogarden")).unwrap();
        {
            let mut shared = tools.lock();
            shared.config.source =
                Some(Source { manifest_url: url, public_key: String::new(), channel: "stable".into() });
            shared.manifest = Some(manifest);
            recompute(&mut shared);
        }
        Automatic { registry, session, tools, root, told, running, _dir: dir }
    }

    impl Automatic {
        fn after_a_check(&mut self) {
            self.tools.lock().last_check = Some(std::time::Instant::now());
            let (sender, done) = std::sync::mpsc::channel();
            auto_update::follow_then(&mut self.session, move || {
                let _ = sender.send(());
            });
            done.recv_timeout(std::time::Duration::from_secs(30)).unwrap();
            let started = std::time::Instant::now();
            while self.tools.busy() {
                assert!(started.elapsed() < std::time::Duration::from_secs(30));
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }

        fn version(&self) -> Option<String> {
            Installation::new(&self.root).current("gogarden")
        }

        fn turn_on(&mut self) {
            let on = serde_json::json!({ "id": "gogarden", "enabled": true });
            self.registry.dispatch(&mut self.session, "toolbox.tool.setAutoUpdate", on).unwrap();
        }

        fn executable(&self) -> PathBuf {
            let installations = self.tools.lock().config.installations();
            auto_update::tool_executable(&installations, "gogarden").unwrap()
        }
    }

    #[test]
    fn nothing_updates_by_itself_until_it_is_turned_on_for_that_tool() {
        let mut case = automatic("0.1.0", &["0.2.0", "0.1.0"]);
        case.after_a_check();
        assert_eq!(case.version().as_deref(), Some("0.1.0"));
        assert!(case.told.lock().unwrap().is_empty());
    }

    #[test]
    fn a_tool_turned_on_updates_after_a_check_and_says_so() {
        zen_i18n::register(LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut case = automatic("0.1.0", &["0.2.0", "0.1.0"]);
        case.turn_on();
        case.after_a_check();
        assert_eq!(case.version().as_deref(), Some("0.2.0"));
        let started = std::time::Instant::now();
        while case.told.lock().unwrap().is_empty() {
            assert!(started.elapsed() < std::time::Duration::from_secs(30));
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let told = case.told.lock().unwrap();
        assert_eq!(told.len(), 1, "{told:?}");
        assert_eq!(told[0].title, "GoGarden is updated to 0.2.0");
    }

    #[test]
    fn a_running_tool_is_left_alone_until_a_check_finds_it_stopped() {
        let mut case = automatic("0.1.0", &["0.2.0", "0.1.0"]);
        case.turn_on();
        case.running.lock().unwrap().push(case.executable());
        case.after_a_check();
        assert_eq!(case.version().as_deref(), Some("0.1.0"), "never while the IDE runs");
        assert!(case.told.lock().unwrap().is_empty());
        let status = serde_json::json!({ "id": "gogarden" });
        let state = case.registry.dispatch(&mut case.session, "toolbox.tool.autoUpdateStatus", status).unwrap();
        assert_eq!(state["running"], true);

        case.running.lock().unwrap().clear();
        case.after_a_check();
        assert_eq!(case.version().as_deref(), Some("0.2.0"));
    }

    #[test]
    fn without_a_process_list_nothing_updates() {
        let mut case = automatic("0.1.0", &["0.2.0", "0.1.0"]);
        case.tools.lock().running_programs = Some(Arc::new(|| None));
        case.turn_on();
        case.after_a_check();
        assert_eq!(case.version().as_deref(), Some("0.1.0"));
    }

    #[test]
    fn an_agent_chooses_no_version_and_turns_on_no_automatic_update() {
        let mut case = automatic("0.2.0", &["0.2.0", "0.1.0"]);
        let (registry, session) = (&case.registry, &mut case.session);
        let back = serde_json::json!({ "id": "gogarden", "version": "0.1.0" });
        let on = serde_json::json!({ "id": "gogarden", "enabled": true });
        let everything = serde_json::json!({ "updatesTools": "automatic" });
        zen_core::macros::as_agent(session, |session| {
            assert!(registry.dispatch(session, "toolbox.tool.installVersion", back.clone()).is_err());
            assert!(registry.dispatch(session, "toolbox.tool.setAutoUpdate", on.clone()).is_err());
            assert!(registry.dispatch(session, "toolbox.settings.set", everything.clone()).is_err());
        });
        assert!(!case.tools.busy());
        assert_eq!(case.version().as_deref(), Some("0.2.0"));
        assert!(!auto_update::is_on(&case.session, "gogarden"));

        let off = serde_json::json!({ "id": "gogarden", "enabled": false });
        let (registry, session) = (&case.registry, &mut case.session);
        zen_core::macros::as_agent(session, |session| {
            registry.dispatch(session, "toolbox.tool.setAutoUpdate", off.clone()).unwrap();
            registry
                .dispatch(session, "toolbox.settings.set", serde_json::json!({ "updatesTools": "notify" }))
                .unwrap();
        });
        case.turn_on();
        assert!(auto_update::is_on(&case.session, "gogarden"));
    }

    #[test]
    fn going_back_holds_automatic_updates_until_a_newer_release() {
        let mut case = automatic("0.2.0", &["0.2.0", "0.1.0"]);
        case.turn_on();
        let back = serde_json::json!({ "id": "gogarden", "version": "0.1.0" });
        case.registry.dispatch(&mut case.session, "toolbox.tool.installVersion", back).unwrap();
        while case.tools.busy() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        case.after_a_check();
        assert_eq!(case.version().as_deref(), Some("0.1.0"), "the version left is not taken back");
        assert!(case.told.lock().unwrap().is_empty(), "a person's own install is not announced as automatic");

        let (url, manifest) = served_releases(&["0.3.0", "0.2.0", "0.1.0"]);
        {
            let mut shared = case.tools.lock();
            shared.config.source.as_mut().unwrap().manifest_url = url;
            shared.manifest = Some(manifest);
            recompute(&mut shared);
        }
        case.after_a_check();
        assert_eq!(case.version().as_deref(), Some("0.3.0"));
    }

    #[test]
    fn a_short_process_name_counts_as_the_program() {
        let path = PathBuf::from("/Applications/GoGarden.app/Contents/MacOS/gogarden");
        assert!(auto_update::runs(Some(path.clone()), std::slice::from_ref(&path)));
        assert!(auto_update::runs(Some(path.clone()), &[PathBuf::from("gogarden")]));
        assert!(!auto_update::runs(Some(path.clone()), &[PathBuf::from("/opt/other/gogarden")]));
        assert!(!auto_update::runs(None, &[path]));
    }

    #[test]
    fn a_restart_request_is_taken_once() {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        let tools = Tools::new(Config::default());
        tools.lock().self_update = SelfUpdate::ReadyToRestart("0.0.1".into());
        session.set_state(tools.clone());
        registry.dispatch(&mut session, "toolbox.self.restart", serde_json::Value::Null).unwrap();
        assert!(tools.take_restart_request());
        assert!(!tools.take_restart_request(), "a second frame must not restart again");
    }

    #[test]
    fn release_notes_are_newest_first_and_mark_the_installed_version() {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Tools::demo());

        let all = registry
            .run::<ReleaseNotes>(&mut session, ReleaseNotesParams { id: "rustronin".into(), newer_than: None })
            .unwrap();
        let versions: Vec<(&str, bool)> = all.iter().map(|note| (note.version.as_str(), note.installed)).collect();
        assert_eq!(versions, [("0.2.0", false), ("0.1.0", true)]);
        assert_eq!(all[0].date, "2026-10-16");
        assert!(all[0].notes.contains("Find in Files"));

        let update = registry
            .run::<ReleaseNotes>(
                &mut session,
                ReleaseNotesParams { id: "rustronin".into(), newer_than: Some("0.1.0".into()) },
            )
            .unwrap();
        assert_eq!(update.len(), 1);
        assert_eq!(update[0].version, "0.2.0");

        let none = registry
            .run::<ReleaseNotes>(&mut session, ReleaseNotesParams { id: "sqlscroll".into(), newer_than: None })
            .unwrap();
        assert!(none.is_empty(), "a tool without releases has no notes");
        assert!(
            registry
                .run::<ReleaseNotes>(&mut session, ReleaseNotesParams { id: "nope".into(), newer_than: None })
                .is_err()
        );

        let toolbox = registry
            .run::<ReleaseNotes>(&mut session, ReleaseNotesParams { id: TOOLBOX_ID.into(), newer_than: None })
            .unwrap();
        assert_eq!(toolbox.iter().filter(|note| note.installed).count(), 1, "the running Toolbox is marked");
    }

    #[test]
    fn release_notes_wait_for_the_manifest() {
        let tools = Tools::new(Config::default());
        assert_eq!(tools.release_notes("rustronin", None, None), None);
        assert_eq!(tools.own_notes(None), None);
    }

    #[test]
    fn own_notes_after_an_update() {
        let tools = Tools::demo();
        assert_eq!(
            tools.own_notes(None).unwrap().iter().map(|note| note.version.as_str()).collect::<Vec<_>>(),
            ["0.0.9"]
        );
        tools.lock().config.self_version = "0.1.0".into();
        let since = tools.own_notes(Some("0.0.8")).unwrap();
        assert_eq!(since.iter().map(|note| note.version.as_str()).collect::<Vec<_>>(), ["0.1.0", "0.0.9"]);
        assert!(tools.own_notes(Some("0.1.0")).unwrap().is_empty());
    }

    #[test]
    fn toolbox_shows_its_own_notes_once_per_update() {
        assert_eq!(notes_on_start(None, "0.1.0", false), StartNotes::Remember, "a first installation");
        assert_eq!(notes_on_start(None, "0.1.0", true), StartNotes::Show { newer_than: None });
        assert_eq!(notes_on_start(Some("0.1.0"), "0.1.0", true), StartNotes::Nothing);
        assert_eq!(notes_on_start(Some("0.0.9"), "0.1.0", true), StartNotes::Show { newer_than: Some("0.0.9".into()) });
        assert_eq!(notes_on_start(Some("0.2.0"), "0.1.0", true), StartNotes::Remember, "a downgrade");
    }

    #[test]
    fn shell_scripts_commands_write_and_remove_launchers() {
        zen_i18n::register(LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let tools_dir = dir.path().join("tools");
        Installation::new(&tools_dir).install("rustronin", "0.1.0", &archive("rustronin")).unwrap();
        let tools = Tools::new(Config { tools_dir: Some(tools_dir), ..Config::default() });
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(tools.clone());
        let bin = dir.path().join("bin");
        let ronin = bin.join(launchers::Shell::current().file_name("rustronin"));
        let params = serde_json::json!({ "location": bin });

        let report = registry.dispatch(&mut session, "toolbox.shellScripts.generate", params.clone()).unwrap();
        assert_eq!(report["written"], serde_json::json!([ronin]));
        assert_eq!(report["onPath"], false);
        assert!(ronin.is_file());

        tools.set_shell_scripts(ShellScripts { enabled: true, location: Some(bin.clone()) });
        registry.run::<UninstallTool>(&mut session, ToolParams { id: "rustronin".into() }).unwrap();
        assert!(!ronin.exists(), "uninstalling regenerates the launchers");

        let report = registry.dispatch(&mut session, "toolbox.shellScripts.remove", serde_json::json!({})).unwrap();
        assert_eq!(report["location"], serde_json::json!(bin), "the configured location is the default");
        assert_eq!(report["removed"], serde_json::json!([]));

        let mut demo = Session::new();
        demo.set_state(Tools::demo());
        assert!(registry.dispatch(&mut demo, "toolbox.shellScripts.generate", params).is_err());
    }

    #[test]
    fn shell_scripts_go_to_the_requested_then_the_configured_location() {
        let configured = ShellScripts { enabled: false, location: Some("/opt/bin".into()) };
        assert_eq!(shell_scripts_location(&configured, Some("/usr/local/bin".into())), Some("/usr/local/bin".into()));
        assert_eq!(shell_scripts_location(&configured, None), Some("/opt/bin".into()));
        assert_eq!(shell_scripts_location(&ShellScripts::default(), None), launchers::default_location());
    }

    #[test]
    fn version_comparison() {
        assert!(newer("0.10.0", "0.9.0"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(newer("0.0.1", "dev"));
        assert!(!newer("garbage", "0.1.0"));
    }

    fn manifest(releases: &[(&str, &str, &str)]) -> Manifest {
        let mut manifest = Manifest { schema: zen_update::SCHEMA, products: Vec::new() };
        for &(id, version, channel) in releases {
            manifest.add(
                id,
                id,
                zen_update::Release {
                    version: version.into(),
                    channel: channel.into(),
                    date: "2026-10-17".into(),
                    notes: String::new(),
                    artifacts: vec![zen_update::Artifact {
                        platform: zen_update::current_platform(),
                        url: "artifacts/x".into(),
                        sha256: String::new(),
                        size: 0,
                    }],
                },
            );
        }
        manifest
    }

    fn settings_session(dir: &std::path::Path, command_line: CommandLine) -> (Registry, Session, Tools) {
        session_with(dir, zen_core::Settings::default(), command_line)
    }

    fn session_with(
        dir: &std::path::Path,
        settings: zen_core::Settings,
        command_line: CommandLine,
    ) -> (Registry, Session, Tools) {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let defaults = Defaults {
            tools_location: Some(dir.join("tools")),
            shell_scripts_location: Some(dir.join("bin")),
            home: Some(dir.join("home")),
        };
        let mut config = Config {
            source: Some(Source {
                manifest_url: "http://127.0.0.1:9/manifest.json".into(),
                public_key: String::new(),
                channel: String::new(),
            }),
            command_line: command_line.clone(),
            defaults: defaults.clone(),
            ..Config::default()
        };
        config.apply(settings::for_this_run(&settings, &defaults, &command_line));
        let tools = Tools::new(config);
        {
            let mut shared = tools.lock();
            shared.manifest =
                Some(manifest(&[("gogarden", "0.1.0", "stable"), ("gogarden", "0.2.0-nightly.1", "nightly")]));
            recompute(&mut shared);
        }
        let mut session = Session::new();
        *session.settings_mut() = settings;
        session.set_state(tools.clone());
        (registry, session, tools)
    }

    fn gogarden(tools: &Tools) -> ToolState {
        tools.snapshot().tools.into_iter().find(|tool| tool.id == "gogarden").unwrap().state
    }

    #[test]
    fn settings_get_and_set_through_commands() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, _) = settings_session(dir.path(), CommandLine::default());
        let got = registry.dispatch(&mut session, "toolbox.settings.get", serde_json::Value::Null).unwrap();
        assert_eq!(got["channel"], "stable");
        assert_eq!(got["updatesCheck"], "automatic");
        assert_eq!(got["shellScriptsEnabled"], true, "on by default");
        assert_eq!(got["toolsLocation"], dir.path().join("tools").display().to_string());

        let set = registry
            .dispatch(
                &mut session,
                "toolbox.settings.set",
                serde_json::json!({ "theme": "dark", "updatesSelf": "automatic" }),
            )
            .unwrap();
        assert_eq!(set["theme"], "dark");
        assert_eq!(set["updatesSelf"], "automatic");
        assert_eq!(set["channel"], "stable", "settings not given are unchanged");
        assert_eq!(session.settings().get::<String>(settings::THEME).as_deref(), Some("dark"));
        let again = registry.dispatch(&mut session, "toolbox.settings.get", serde_json::Value::Null).unwrap();
        assert_eq!(again, set);
    }

    #[test]
    fn invalid_settings_are_refused_by_the_command() {
        zen_i18n::register(LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, _) = settings_session(dir.path(), CommandLine::default());
        let error = registry
            .dispatch(&mut session, "toolbox.settings.set", serde_json::json!({ "channel": "beta" }))
            .unwrap_err();
        assert!(error.to_string().contains("“beta” is not a valid value of toolbox.channel"), "{error}");
        assert!(
            registry.dispatch(&mut session, "toolbox.settings.set", serde_json::json!({ "colour": "red" })).is_err()
        );
        assert_eq!(session.settings().iter().count(), 0);
    }

    #[test]
    fn a_channel_change_recomputes_the_versions() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, tools) = settings_session(dir.path(), CommandLine::default());
        assert_eq!(gogarden(&tools), ToolState::Available { version: "0.1.0".into() });
        registry.dispatch(&mut session, "toolbox.settings.set", serde_json::json!({ "channel": "nightly" })).unwrap();
        assert_eq!(gogarden(&tools), ToolState::Available { version: "0.2.0-nightly.1".into() });
        assert_eq!(tools.lock().config.source.as_ref().unwrap().channel, "nightly");
    }

    #[test]
    fn the_command_line_channel_wins_for_this_run() {
        let dir = tempfile::tempdir().unwrap();
        let command_line = CommandLine { channel: Some(Channel::Nightly), tools_dir: None };
        let (registry, mut session, tools) = settings_session(dir.path(), command_line);
        assert_eq!(gogarden(&tools), ToolState::Available { version: "0.2.0-nightly.1".into() });
        registry.dispatch(&mut session, "toolbox.settings.set", serde_json::json!({ "channel": "stable" })).unwrap();
        assert_eq!(gogarden(&tools), ToolState::Available { version: "0.2.0-nightly.1".into() });
        assert_eq!(session.settings().get::<String>(settings::CHANNEL).as_deref(), Some("stable"), "but it is saved");
    }

    #[test]
    fn a_new_tools_location_is_for_new_installations_only() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, tools) = settings_session(dir.path(), CommandLine::default());
        Installation::new(dir.path().join("tools")).install("rustronin", "0.1.0", &archive("rustronin")).unwrap();
        let elsewhere = dir.path().join("elsewhere");
        registry
            .dispatch(
                &mut session,
                "toolbox.settings.set",
                serde_json::json!({ "toolsLocation": elsewhere.display().to_string() }),
            )
            .unwrap();
        assert!(
            matches!(tools.snapshot().tools[0].state, ToolState::Installed { ref version, .. } if version == "0.1.0"),
            "an installed tool stays where it is"
        );
        assert_eq!(installation(&tools, "rustronin").unwrap().root(), dir.path().join("tools"));
        assert_eq!(installation(&tools, "gogarden").unwrap().root(), elsewhere);
    }

    #[test]
    fn manual_checks_wait_for_the_user() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, tools) = settings_session(dir.path(), CommandLine::default());
        {
            let mut shared = tools.lock();
            shared.manifest = None;
            shared.server = ServerStatus::Checking;
        }
        assert!(tools.check_due(std::time::Duration::from_secs(60)));
        registry
            .dispatch(&mut session, "toolbox.settings.set", serde_json::json!({ "updatesCheck": "manual" }))
            .unwrap();
        assert!(!tools.check_due(std::time::Duration::from_secs(60)));
        assert_eq!(tools.snapshot().server, ServerStatus::NotChecked);
        assert!(registry.is_available(&session, "toolbox.releases.refresh"), "the user can still check");
        registry
            .dispatch(&mut session, "toolbox.settings.set", serde_json::json!({ "updatesCheck": "automatic" }))
            .unwrap();
        assert!(tools.check_due(std::time::Duration::from_secs(60)));
        assert_eq!(tools.snapshot().server, ServerStatus::Checking);
    }

    fn launcher(dir: &std::path::Path, name: &str) -> PathBuf {
        dir.join(launchers::Shell::current().file_name(name))
    }

    fn set(registry: &Registry, session: &mut Session, params: serde_json::Value) {
        registry.dispatch(session, "toolbox.settings.set", params).unwrap();
    }

    #[test]
    fn shell_scripts_follow_the_settings() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = zen_core::Settings::default();
        settings.set(settings::SHELL_SCRIPTS_ENABLED, false).unwrap();
        let (registry, mut session, tools) = session_with(dir.path(), settings, CommandLine::default());
        Installation::new(dir.path().join("tools")).install("gogarden", "0.1.0", &archive("gogarden")).unwrap();
        let (bin, other) = (dir.path().join("bin"), dir.path().join("other"));
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(launcher(&other, "gogarden"), "#!/bin/sh\necho mine\n").unwrap();
        assert!(!bin.exists(), "nothing is written while shell scripts are off");

        set(&registry, &mut session, serde_json::json!({ "shellScriptsEnabled": true }));
        assert!(launcher(&bin, "gogarden").is_file(), "enabling generates at once");

        set(&registry, &mut session, serde_json::json!({ "shellScriptsLocation": other.display().to_string() }));
        assert!(!launcher(&bin, "gogarden").exists(), "the old location's launchers go");
        assert_eq!(std::fs::read_to_string(launcher(&other, "gogarden")).unwrap(), "#!/bin/sh\necho mine\n");
        assert_eq!(tools.take_messages(), Vec::<String>::new());

        let report = registry.dispatch(&mut session, "toolbox.shellScripts.generate", serde_json::json!({})).unwrap();
        assert_eq!(report["location"], serde_json::json!(other), "the setting is the default location");
        assert_eq!(report["skipped"], serde_json::json!([launcher(&other, "gogarden")]));

        set(&registry, &mut session, serde_json::json!({ "shellScriptsLocation": bin.display().to_string() }));
        assert!(launcher(&bin, "gogarden").is_file());
        set(&registry, &mut session, serde_json::json!({ "shellScriptsEnabled": false }));
        assert!(!launcher(&bin, "gogarden").exists(), "disabling removes the marked launchers");
        assert!(launcher(&other, "gogarden").is_file(), "a file Zen Toolbox did not write stays");
    }

    #[test]
    fn shell_scripts_are_on_by_default_and_follow_installs() {
        let dir = tempfile::tempdir().unwrap();
        Installation::new(dir.path().join("tools")).install("gogarden", "0.1.0", &archive("gogarden")).unwrap();
        let (registry, mut session, _) = settings_session(dir.path(), CommandLine::default());
        let bin = dir.path().join("bin");
        assert!(launcher(&bin, "gogarden").is_file(), "written at start without being asked");
        registry.run::<UninstallTool>(&mut session, ToolParams { id: "gogarden".into() }).unwrap();
        assert!(!launcher(&bin, "gogarden").exists(), "and removed on uninstall");
        assert_eq!(std::fs::read_dir(&bin).unwrap().count(), 0);
        assert!(!dir.path().join("home").exists(), "no shell profile is touched");
    }

    #[cfg(unix)]
    #[test]
    fn the_path_line_is_added_only_by_its_command() {
        zen_i18n::register(LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        Installation::new(dir.path().join("tools")).install("gogarden", "0.1.0", &archive("gogarden")).unwrap();
        let (registry, mut session, tools) = settings_session(dir.path(), CommandLine::default());
        let bin = dir.path().join("bin");

        let status = registry.dispatch(&mut session, "toolbox.shellScripts.status", serde_json::json!({})).unwrap();
        assert_eq!(status["onPath"], false);
        assert_eq!(status["inProfile"], false);
        assert_eq!(status["launchers"], serde_json::json!([launcher(&bin, "gogarden")]));
        assert_eq!(status["line"], format!("export PATH=\"{}:$PATH\"", bin.display()));
        assert_eq!(status["profiles"], serde_json::json!([home.join(".zprofile")]));
        assert!(!home.join(".zprofile").exists(), "asking changes nothing");

        let added = registry.dispatch(&mut session, "toolbox.shellScripts.addToPath", serde_json::json!({})).unwrap();
        assert_eq!(added["changed"], serde_json::json!([home.join(".zprofile")]));
        let profile = std::fs::read_to_string(home.join(".zprofile")).unwrap();
        assert!(profile.contains(added["line"].as_str().unwrap()), "{profile}");
        let status = registry.dispatch(&mut session, "toolbox.shellScripts.status", serde_json::json!({})).unwrap();
        assert_eq!(status["inProfile"], true);
        let shell = zen_update::scripts::user_shell();
        if shell.exists() && shell.file_name().is_some_and(|name| name == "zsh") {
            assert_eq!(status["onPath"], true, "a new terminal window finds the launchers");
        }
        let view = tools.shell_scripts();
        assert_eq!(view.status.map(|status| status.in_profile), Some(true), "the window shows the last check");
        assert_eq!(view.launchers, [launcher(&bin, "gogarden")]);
    }

    #[test]
    fn completions_reach_a_shell_file_only_through_their_command() {
        use serde_json::json;
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, _) = settings_session(dir.path(), CommandLine::default());
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let status = registry.dispatch(&mut session, "toolbox.shellScripts.status", json!({})).unwrap();
        assert_eq!(status["completions"], false);
        assert!(!home.join(".zprofile").exists());
        let added = registry.dispatch(&mut session, "toolbox.shellScripts.addCompletions", json!({})).unwrap();
        assert_eq!(added["changed"], json!([home.join(".zprofile")]));
        let status = registry.dispatch(&mut session, "toolbox.shellScripts.status", json!({})).unwrap();
        assert_eq!(status["completions"], true);
    }

    #[test]
    fn shell_scripts_are_generated_at_start_for_tools_in_previous_locations() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("old");
        Installation::new(&old).install("gogarden", "0.1.0", &archive("gogarden")).unwrap();
        let mut settings = zen_core::Settings::default();
        settings.set(settings::TOOLS_LOCATION, dir.path().join("new")).unwrap();
        settings.set(settings::PREVIOUS_LOCATIONS, [&old]).unwrap();
        settings.set(settings::SHELL_SCRIPTS_ENABLED, true).unwrap();
        let (_, _, tools) = session_with(dir.path(), settings, CommandLine::default());
        let garden = std::fs::read_to_string(launcher(&dir.path().join("bin"), "gogarden")).unwrap();
        assert!(garden.contains(old.to_str().unwrap()), "{garden}");
        assert!(matches!(gogarden(&tools), ToolState::Installed { .. }));
    }

    #[test]
    fn a_tools_dir_flag_leaves_the_launchers_alone() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let garden = launcher(&bin, "gogarden");
        std::fs::write(&garden, launchers::script("/opt/gogarden", launchers::Shell::current())).unwrap();
        let mut settings = zen_core::Settings::default();
        settings.set(settings::SHELL_SCRIPTS_ENABLED, true).unwrap();
        let command_line = CommandLine { channel: None, tools_dir: Some(dir.path().join("flag")) };
        let (registry, mut session, _) = session_with(dir.path(), settings, command_line);
        assert!(garden.is_file(), "a run with --tools-dir does not regenerate");
        set(&registry, &mut session, serde_json::json!({ "shellScriptsEnabled": false }));
        assert!(garden.is_file(), "nor remove");
    }

    #[test]
    fn shell_scripts_commands_refuse_relative_locations_and_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, _) = settings_session(dir.path(), CommandLine::default());
        for params in [serde_json::json!({ "location": "bin" }), serde_json::json!({ "locaton": "/tmp" })] {
            assert!(registry.dispatch(&mut session, "toolbox.shellScripts.generate", params.clone()).is_err());
            assert!(registry.dispatch(&mut session, "toolbox.shellScripts.remove", params).is_err());
        }
        assert!(!dir.path().join("bin").exists());
    }

    pub(crate) type Launched = Arc<Mutex<Vec<LaunchCommand>>>;

    pub(crate) fn installed(dir: &std::path::Path, ids: &[&str]) -> (Registry, Session, Launched) {
        zen_i18n::register(LOCALES);
        zen_i18n::set_language(Some("en"));
        let installation = Installation::new(dir.join("tools"));
        for id in ids {
            installation.install(id, "0.1.0", &archive(id)).unwrap();
        }
        let launched = Launched::default();
        let record = launched.clone();
        let tools = Tools::new(Config {
            tools_dir: Some(dir.join("tools")),
            data_dir: Some(dir.join("data")),
            ..Config::default()
        })
        .with_launcher(Arc::new(move |line: &LaunchCommand| {
            record.lock().unwrap().push(line.clone());
            Ok(())
        }));
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(tools);
        (registry, session, launched)
    }

    #[test]
    fn launching_checks_the_tool_and_the_project_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session, launched) = installed(dir.path(), &["gogarden"]);
        let launch = |session: &mut Session, params: serde_json::Value| {
            registry.dispatch(session, "toolbox.tool.launch", params).unwrap_err().to_string()
        };

        let project = dir.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let error = launch(&mut session, serde_json::json!({ "id": "rustronin", "path": project }));
        assert!(error.contains("RustRonin is not installed"), "{error}");

        let missing = dir.path().join("missing");
        let error = launch(&mut session, serde_json::json!({ "id": "gogarden", "path": missing }));
        assert!(error.contains("There is no folder") && error.contains("missing"), "{error}");

        let file = dir.path().join("main.go");
        std::fs::write(&file, "package main").unwrap();
        let error = launch(&mut session, serde_json::json!({ "id": "gogarden", "path": file }));
        assert!(error.contains("is not a folder") && error.contains("main.go"), "{error}");

        let error = launch(&mut session, serde_json::json!({ "id": "nope" }));
        assert!(error.contains("There is no tool"), "{error}");
        assert!(launched.lock().unwrap().is_empty());

        let inner = project.join("..").join("project");
        registry
            .dispatch(&mut session, "toolbox.tool.launch", serde_json::json!({ "id": "gogarden", "path": inner }))
            .unwrap();
        registry.dispatch(&mut session, "toolbox.tool.launch", serde_json::json!({ "id": "gogarden" })).unwrap();
        let launched = launched.lock().unwrap();
        assert_eq!(launched.len(), 2);
        assert!(launched.iter().all(|line| line.program.file_stem().is_some_and(|stem| stem == "gogarden")));
        assert_eq!(launched[0].args, [std::fs::canonicalize(&project).unwrap().into_os_string()]);
        assert!(launched[1].args.is_empty());
    }

    #[test]
    fn launch_parameters_keep_the_path_optional() {
        let old: LaunchParams = serde_json::from_value(serde_json::json!({ "id": "gogarden" })).unwrap();
        assert_eq!(old.path, None);
        let with: LaunchParams =
            serde_json::from_value(serde_json::json!({ "id": "gogarden", "path": "/work/app" })).unwrap();
        assert_eq!(with.path, Some(PathBuf::from("/work/app")));
        let schema = serde_json::to_value(schemars::schema_for!(LaunchParams)).unwrap();
        assert_eq!(schema["required"], serde_json::json!(["id"]));
        assert!(schema["properties"]["path"]["description"].as_str().unwrap().contains("project folder"));
    }

    fn sized_manifest(releases: &[(&str, &str, u64)]) -> Manifest {
        let mut manifest = Manifest { schema: zen_update::SCHEMA, products: Vec::new() };
        for &(id, version, size) in releases {
            manifest.add(
                id,
                id,
                zen_update::Release {
                    version: version.into(),
                    channel: "stable".into(),
                    date: "2026-10-19".into(),
                    notes: String::new(),
                    artifacts: vec![zen_update::Artifact {
                        platform: zen_update::current_platform(),
                        url: format!("artifacts/{id}/{version}/{id}.tar.gz"),
                        sha256: String::new(),
                        size,
                    }],
                },
            );
        }
        manifest
    }

    #[test]
    fn available_and_update_rows_carry_the_download_size_from_the_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let installation = Installation::new(dir.path());
        installation.install("gogarden", "0.38.6", &archive("gogarden")).unwrap();
        installation.install("rustronin", "0.2.0", &archive("rustronin")).unwrap();
        let tools = Tools::new(Config {
            tools_dir: Some(dir.path().to_owned()),
            self_version: "0.3.10".into(),
            self_executable: Some("/nonexistent/zen-toolbox".into()),
            ..Config::default()
        });
        {
            let mut shared = tools.lock();
            shared.manifest = Some(sized_manifest(&[
                ("gogarden", "0.38.7", 41_234_567),
                ("rustronin", "0.2.0", 50_000_000),
                ("webwave", "0.1.0", 37_600_000),
                (TOOLBOX_ID, "0.3.11", 14_200_000),
            ]));
            recompute(&mut shared);
        }
        let snapshot = tools.snapshot();
        let size = |id: &str| snapshot.tools.iter().find(|tool| tool.id == id).unwrap().download_size;
        assert_eq!(size("gogarden"), Some(41_234_567), "an update");
        assert_eq!(size("webwave"), Some(37_600_000), "an available tool");
        assert_eq!(size("rustronin"), None, "an up-to-date tool downloads nothing");
        assert_eq!(size("sqlscroll"), None);
        assert_eq!(snapshot.self_update_size, Some(14_200_000));
        assert_eq!(megabytes(41_234_567), 41);
        assert_eq!(megabytes(41_600_000), 42);
        assert_eq!(megabytes(120_000), 1, "never 0 MB");

        let listed = serde_json::to_value(tools.snapshot().tools).unwrap();
        let gogarden = listed.as_array().unwrap().iter().find(|tool| tool["id"] == "gogarden").unwrap();
        assert_eq!(gogarden["downloadSize"], 41_234_567, "toolbox.tools.list reports it to agents");
    }

    fn size_server(size: u64) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{BufRead, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = std::io::BufReader::new(stream);
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
                    line.clear();
                }
                counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut stream = reader.into_inner();
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n");
            }
        });
        (format!("http://{address}/releases/manifest.json"), requests)
    }

    #[test]
    fn a_manifest_without_sizes_asks_the_server_once() {
        let (manifest_url, requests) = size_server(41_234_567);
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::new(Config {
            source: Some(Source { manifest_url, public_key: String::new(), channel: "stable".into() }),
            tools_dir: Some(dir.path().to_owned()),
            ..Config::default()
        });
        {
            let mut shared = tools.lock();
            shared.manifest = Some(sized_manifest(&[("gogarden", "0.38.7", 0)]));
            recompute(&mut shared);
        }
        let size = || tools.snapshot().tools.iter().find(|tool| tool.id == "gogarden").unwrap().download_size;
        assert_eq!(size(), None, "unknown until the server answers");
        ask_sizes(&tools);
        let started = std::time::Instant::now();
        while size().is_none() && started.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(size(), Some(41_234_567));

        {
            let mut shared = tools.lock();
            shared.manifest = Some(sized_manifest(&[("gogarden", "0.38.7", 0)]));
            recompute(&mut shared);
        }
        ask_sizes(&tools);
        assert_eq!(size(), Some(41_234_567));
        assert_eq!(requests.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn the_login_item_follows_the_setting_and_reports_its_status() {
        let dir = tempfile::tempdir().unwrap();
        let item = login::LoginItem {
            dir: dir.path().join("LaunchAgents"),
            executable: dir.path().join("Zen Toolbox.app/Contents/MacOS/zen-toolbox"),
            format: login::Format::LaunchAgent,
        };
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        let tools = Tools::new(Config { login_item: Some(item.clone()), launch_at_login: true, ..Config::default() });
        session.set_state(tools.clone());
        assert!(item.is_registered(), "on by default: the LaunchAgent is written at start");
        assert_eq!(tools.snapshot().launch_at_login, Some(true));

        let status = registry.dispatch(&mut session, "toolbox.login.status", serde_json::Value::Null).unwrap();
        assert_eq!(status["enabled"], true);
        assert_eq!(status["registered"], true);
        assert_eq!(status["file"], item.file().display().to_string());

        registry.dispatch(&mut session, "toolbox.settings.set", serde_json::json!({ "launchAtLogin": false })).unwrap();
        assert!(!item.file().exists(), "turning it off removes the LaunchAgent");
        assert_eq!(session.settings().get::<bool>(settings::LAUNCH_AT_LOGIN), Some(false));
        let status = registry.dispatch(&mut session, "toolbox.login.status", serde_json::Value::Null).unwrap();
        assert_eq!((status["enabled"].as_bool(), status["registered"].as_bool()), (Some(false), Some(false)));

        registry.dispatch(&mut session, "toolbox.settings.set", serde_json::json!({ "launchAtLogin": true })).unwrap();
        assert!(item.is_registered());
        let read = registry.dispatch(&mut session, "toolbox.settings.get", serde_json::Value::Null).unwrap();
        assert_eq!(read["launchAtLogin"], true);
    }

    #[test]
    fn development_builds_and_the_demo_have_no_login_item() {
        let tools = Tools::new(Config { launch_at_login: true, ..Config::default() });
        assert_eq!(tools.snapshot().launch_at_login, None);
        let status = tools.login_status();
        assert!(!status.supported && !status.registered && status.file.is_none());
        assert_eq!(Tools::demo().snapshot().launch_at_login, None);
    }

    pub(crate) fn archive(id: &str) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, format!("bin/{}", zen_update::install::executable_name(id)), &b"x"[..])
            .unwrap();
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn agents_list_the_licences_and_a_development_build_has_none() {
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        let listed = registry.dispatch(&mut session, "toolbox.licences.list", serde_json::json!({})).unwrap();
        assert_eq!(listed, serde_json::json!({ "folder": null, "files": [] }));
    }
}

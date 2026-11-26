use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;
use crate::build_tools::BuildEnv;
use crate::notifications::{Action, Group, Kind, NewNotification};

pub const TOOL: &str = "projectTrust";

pub const LOCATIONS: &str = "trustedLocations";

pub const NOT_TRUSTED: &str = "notTrusted";

pub const AGENT_NOT_ALLOWED: &str = "agentNotAllowed";

pub const PERSON_ONLY: &[&str] = &[GrantTrust::ID, crate::file_watchers::EnableWatcher::ID];

const BUILD_FILES: &[&str] = &[
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "pom.xml",
    "build.xml",
    ".zen/runConfigurations.json",
    ".zen/go.json",
    crate::file_watchers::PROJECT_FILE,
    crate::build_triggers::FILE,
];

const CONFIGURATION_FOLDERS: &[&str] = &[".run", ".idea/runConfigurations"];

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(TrustState)?;
    registry.register(GrantTrust)?;
    registry.register(RevokeTrust)?;
    registry.register(GetLocations)?;
    registry.register(SetLocations)?;
    Ok(())
}

fn canonical(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    while !existing.exists() {
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name);
                existing = parent;
            }
            _ => return path.to_owned(),
        }
    }
    let mut found = std::fs::canonicalize(existing).unwrap_or_else(|_| existing.to_owned());
    found.extend(rest.into_iter().rev());
    found
}

fn store(env: &BuildEnv, root: &Path, trusted: bool) -> Result<(), CommandError> {
    env.set_project_settings(TOOL, root, &trusted)
        .map_err(|error| CommandError::new(t!("workbench-trust-write", error = error.as_str())))
}

#[cfg(test)]
pub(crate) fn trust_in_tests(env: &BuildEnv, root: &Path) {
    store(env, &canonical(root), true).unwrap();
}

#[cfg(test)]
pub(crate) fn trust_session_in_tests(session: &mut Session, root: &Path) {
    if session.state::<BuildEnv>().is_none() {
        session.set_state(BuildEnv::default());
    }
    trust_in_tests(&env(session), root);
}

#[cfg(test)]
pub(crate) fn revoke_in_tests(session: &mut Session, root: &Path) {
    if session.state::<BuildEnv>().is_none() {
        session.set_state(BuildEnv::default());
    }
    store(&env(session), &canonical(root), false).unwrap();
}

pub fn is_trusted(env: &BuildEnv, root: &Path) -> bool {
    let root = canonical(root);
    let projects: BTreeMap<String, bool> = env.settings(TOOL);
    root.ancestors().any(|folder| projects.get(folder.to_string_lossy().as_ref()).copied().unwrap_or(false))
        || locations(env).iter().any(|location| root.starts_with(location))
}

pub fn safe_environment() -> Vec<(OsString, OsString)> {
    vec![(OsString::from("GOTOOLCHAIN"), OsString::from("local"))]
}

pub fn locations(env: &BuildEnv) -> Vec<PathBuf> {
    env.settings(LOCATIONS)
}

pub fn covers_home(location: &Path) -> bool {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .is_some_and(|home| canonical(Path::new(&home)).starts_with(canonical(location)))
}

fn env(session: &Session) -> BuildEnv {
    session.state::<BuildEnv>().cloned().unwrap_or_else(BuildEnv::global)
}

pub fn trusted(session: &Session, root: &Path) -> bool {
    is_trusted(&env(session), root)
}

pub fn guard_agent_build(session: &Session, root: &Path) -> Result<(), CommandError> {
    if zen_core::macros::by_agent(session) && !trusted(session, root) {
        let message =
            format!("{} {}", t!("workbench-trust-agent-build", project = name(root)), t!("workbench-trust-agent-how"));
        return Err(CommandError::with_code(NOT_TRUSTED, message));
    }
    Ok(())
}

pub fn guard_agent_project(session: &Session) -> Result<(), CommandError> {
    match session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) {
        Some(root) => guard_agent_build(session, &root),
        None => Ok(()),
    }
}

pub(crate) fn trust_new_project(session: &Session, root: &Path) -> Result<(), CommandError> {
    store(&env(session), &canonical(root), true)
}

fn forget_launches(session: &Session) {
    if let Some(servers) = session.state::<crate::language::servers::LanguageServers>() {
        servers.forget_java_launches();
    }
}

pub fn guard_project_build(session: &mut Session, root: &Path) -> Result<(), CommandError> {
    if trusted(session, root) {
        return Ok(());
    }
    Err(refused(session, root, t!("workbench-trust-artifacts", project = name(root))))
}

pub(crate) fn guard_agent_data_folder<P: AsRef<Path>>(session: &Session, paths: &[P]) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    let Some(data) = env(session).data_dir().map(|data| crate::hidden_files::real_path(&data)) else {
        return Ok(());
    };
    let scratches = session.state::<crate::scratch::Scratches>();
    let project = session.state::<Workbench>().and_then(Workbench::project).map(crate::hidden_files::real_path);
    for path in paths {
        let path = path.as_ref();
        let real = crate::hidden_files::real_path(path);
        let own_project =
            project.as_ref().is_some_and(|project| project.starts_with(&data) && real.starts_with(project));
        if real.starts_with(&data) && !own_project && !scratches.is_some_and(|scratches| scratches.contains(path)) {
            return Err(CommandError::with_code(
                AGENT_NOT_ALLOWED,
                t!("workbench-trust-agent-data-folder", path = path.display().to_string()),
            ));
        }
    }
    Ok(())
}

pub fn guard_agent_build_settings<'a>(
    session: &Session,
    keys: impl IntoIterator<Item = &'a str>,
    choosing: &[&str],
) -> Result<(), CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    for key in keys {
        let key = key.replace('/', ".");
        let chooses = choosing.iter().any(|chosen| {
            *chosen == key || chosen.starts_with(&format!("{key}.")) || key.starts_with(&format!("{chosen}."))
        });
        if chooses {
            return Err(CommandError::with_code(
                AGENT_NOT_ALLOWED,
                t!("workbench-trust-agent-build-setting", key = key.as_str()),
            ));
        }
    }
    Ok(())
}

pub fn guard_build_load(session: &mut Session, root: &Path) -> Result<(), CommandError> {
    if trusted(session, root) {
        return Ok(());
    }
    Err(refused(session, root, t!("workbench-trust-build-load", project = name(root))))
}

pub fn guard_file_watchers(session: &mut Session, root: &Path) -> Result<(), CommandError> {
    if trusted(session, root) {
        return Ok(());
    }
    Err(refused(session, root, t!("workbench-trust-file-watchers", project = name(root))))
}

pub fn guard_run(
    session: &mut Session,
    root: &Path,
    configuration: &zen_run::Configuration,
) -> Result<(), CommandError> {
    if trusted(session, root) || crate::run::scratch_file_of(session, configuration).is_some() {
        return Ok(());
    }
    Err(refused(session, root, t!("workbench-trust-run", name = configuration.name.as_str())))
}

pub fn guard_project_classes(session: &mut Session, root: &Path) -> Result<(), CommandError> {
    if trusted(session, root) {
        return Ok(());
    }
    Err(refused(session, root, t!("workbench-trust-console", project = name(root))))
}

pub fn guard_debug(
    session: &mut Session,
    root: &Path,
    configuration: &zen_run::Configuration,
) -> Result<(), CommandError> {
    if trusted(session, root) {
        return Ok(());
    }
    Err(refused(session, root, t!("workbench-trust-run", name = configuration.name.as_str())))
}

pub fn guard_configuration(
    session: &mut Session,
    root: &Path,
    configuration: &zen_run::Configuration,
) -> Result<(), CommandError> {
    if trusted(session, root) {
        return Ok(());
    }
    let quiet = [crate::before_launch::TOOL_WINDOW, crate::before_launch::SAVE_FILES];
    if configuration.common.before_launch.iter().all(|task| quiet.contains(&task.kind.as_str())) {
        return Ok(());
    }
    Err(refused(session, root, t!("workbench-trust-before-launch", name = configuration.name.as_str())))
}

pub fn git_allowed(env: &BuildEnv, path: &Path) -> bool {
    if is_trusted(env, path) {
        return true;
    }
    let path = canonical(path);
    let projects: BTreeMap<String, bool> = env.settings(TOOL);
    let holds_trusted = |folder: &Path| folder.starts_with(&path);
    projects.iter().any(|(folder, on)| *on && holds_trusted(Path::new(folder)))
        || locations(env).iter().any(|location| holds_trusted(location))
}

pub fn install_git_gate(env: BuildEnv) {
    let projects = env.clone();
    zen_project::trust::set_gate(Some(std::sync::Arc::new(move |root: &Path| is_trusted(&projects, root))));
    zen_vcs::git::set_gate(Some(std::sync::Arc::new(move |path: &Path| git_allowed(&env, path))));
}

pub(crate) fn git_off(session: &Session) -> CommandError {
    let message = t!("vcs-error-safe-mode");
    if zen_core::macros::by_agent(session) {
        return CommandError::with_code(NOT_TRUSTED, format!("{message} {}", t!("workbench-trust-agent-how")));
    }
    CommandError::with_code(NOT_TRUSTED, message)
}

pub(crate) fn refused(session: &mut Session, root: &Path, message: String) -> CommandError {
    if zen_core::macros::by_agent(session) {
        return CommandError::with_code(NOT_TRUSTED, format!("{message} {}", t!("workbench-trust-agent-how")));
    }
    prompt(session, root);
    CommandError::with_code(NOT_TRUSTED, message)
}

pub fn has_build(root: &Path) -> bool {
    BUILD_FILES.iter().any(|file| root.join(file).is_file())
        || CONFIGURATION_FOLDERS.iter().any(|folder| root.join(folder).is_dir())
}

#[derive(Debug, Default)]
pub struct Answered;

pub(crate) fn project_opened(session: &mut Session, root: &Path) {
    if session.state::<Answered>().is_none() && has_build(root) && !trusted(session, root) {
        prompt(session, root);
    }
}

fn prompt(session: &mut Session, root: &Path) {
    let asked = session.state::<crate::notifications::Notifications>().is_some_and(|store| {
        store
            .newest_first()
            .any(|item| !item.read && item.actions.iter().any(|action| action.command == GrantTrust::ID))
    });
    if asked {
        return;
    }
    let notice =
        NewNotification::worded(Group::General, Kind::Warning, zen_i18n::Message::new("workbench-trust-prompt-body"))
            .titled_by(zen_i18n::Message::new("workbench-trust-prompt-title").with("project", name(root)))
            .action(Action::new("trust", t!("workbench-trust-action"), GrantTrust::ID, json!({ "path": root })));
    crate::notifications::post(session, notice);
}

fn name(root: &Path) -> String {
    root.file_name().map_or_else(|| root.display().to_string(), |name| name.to_string_lossy().into_owned())
}

fn root_of(session: &Session, path: Option<&Path>) -> Result<PathBuf, CommandError> {
    let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    match (path, project) {
        (Some(path), Some(project)) if path.is_relative() => Ok(project.join(path)),
        (Some(path), _) => Ok(path.to_owned()),
        (None, Some(project)) => Ok(project),
        (None, None) => Err(CommandError::new(t!("workbench-trust-no-project"))),
    }
}

/// Which project.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TrustParams {
    /// The project's folder; the open project when absent.
    pub path: Option<PathBuf>,
}

/// Whether a project is trusted: only a trusted project runs its build scripts by itself, Lombok as the Java
/// language server's agent, and annotation processors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrustInfo {
    pub path: PathBuf,
    pub trusted: bool,
}

pub struct TrustState;

impl Command for TrustState {
    type Params = TrustParams;
    type Output = TrustInfo;
    const ID: &'static str = "project.trust.state";
    const TITLE: &'static str = "Project Trust";
    const DESCRIPTION: &'static str = "Whether project `path` (the open one when absent) is trusted (ADR-0205, ADR-0248): it, a folder above it, or a trusted location. An untrusted project is in safe mode: files open and can be edited, but Git is off (no Git call at all), no language server, go.mod check or project model program starts, nothing runs or debugs (code `notTrusted`, for the person too), the IDE reloads no build by itself, and agents may not reload or run its builds (code `notTrusted`). Only the person trusts a project (project.trust.grant, the Trust and Open Project dialog, the safe mode banner, the notification's Trust Project); agents never can.";

    fn run(&self, session: &mut Session, params: TrustParams) -> Result<TrustInfo, CommandError> {
        let path = root_of(session, params.path.as_deref())?;
        Ok(TrustInfo { trusted: trusted(session, &path), path })
    }
}

pub struct GrantTrust;

impl Command for GrantTrust {
    type Params = TrustParams;
    type Output = TrustInfo;
    const ID: &'static str = "project.trust.grant";
    const TITLE: &'static str = "Trust Project";
    const DESCRIPTION: &'static str = "Trusts project `path` (the open one when absent) and the folders inside it, kept in the IDE's data folder (build-settings, `projectTrust`), never in the project: the language servers and everything else safe mode held back start without reopening the project, its build scripts run by themselves, and the Java language server starts with Lombok and annotation processors (after a restart of the server). Only the person trusts a project: refused for agents with code `agentNotAllowed`, also through a notification's action.";

    fn run(&self, session: &mut Session, params: TrustParams) -> Result<TrustInfo, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(AGENT_NOT_ALLOWED, t!("workbench-trust-agent")));
        }
        let path = canonical(&root_of(session, params.path.as_deref())?);
        if !path.is_dir() {
            return Err(CommandError::new(t!("workbench-trust-not-folder", path = path.display().to_string())));
        }
        store(&env(session), &path, true)?;
        forget_launches(session);
        Ok(TrustInfo { path, trusted: true })
    }
}

pub struct RevokeTrust;

impl Command for RevokeTrust {
    type Params = TrustParams;
    type Output = TrustInfo;
    const ID: &'static str = "project.trust.revoke";
    const TITLE: &'static str = "Revoke Project Trust";
    const DESCRIPTION: &'static str = "Stops trusting project `path` (the open one when absent), so its build scripts no longer run by themselves; agents may call it, as it only makes the project safer. A folder above it that is trusted still trusts it (answered as `trusted`).";

    fn run(&self, session: &mut Session, params: TrustParams) -> Result<TrustInfo, CommandError> {
        let path = canonical(&root_of(session, params.path.as_deref())?);
        let env = env(session);
        store(&env, &path, false)?;
        forget_launches(session);
        Ok(TrustInfo { trusted: is_trusted(&env, &path), path })
    }
}

/// A trusted location: every project under it is trusted without asking.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub path: PathBuf,
    pub covers_home: bool,
}

/// The trusted locations, in their order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocationList {
    pub locations: Vec<LocationInfo>,
}

fn location_list(env: &BuildEnv) -> LocationList {
    let info = |path: PathBuf| LocationInfo { covers_home: covers_home(&path), path };
    LocationList { locations: locations(env).into_iter().map(info).collect() }
}

/// No parameters.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
pub struct NoLocationParams {}

pub struct GetLocations;

impl Command for GetLocations {
    type Params = NoLocationParams;
    type Output = LocationList;
    const ID: &'static str = "project.trustedLocations.get";
    const TITLE: &'static str = "Trusted Locations";
    const DESCRIPTION: &'static str = "The trusted locations (Settings → Appearance & Behavior → Trusted Locations, ADR-0248), kept in the IDE's data folder: every project under one is trusted without asking. `coversHome` says that a location is the home folder or a folder above it, which turns the trust check off for nearly everything.";

    fn run(&self, session: &mut Session, _: NoLocationParams) -> Result<LocationList, CommandError> {
        Ok(location_list(&env(session)))
    }
}

/// The new list of trusted locations.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LocationsParams {
    /// Absolute folders, in the order to keep.
    pub locations: Vec<PathBuf>,
}

pub struct SetLocations;

impl Command for SetLocations {
    type Params = LocationsParams;
    type Output = LocationList;
    const ID: &'static str = "project.trustedLocations.set";
    const TITLE: &'static str = "Set Trusted Locations";
    const DESCRIPTION: &'static str = "Replaces the trusted locations with `locations`, in that order. A folder removed from the list stops trusting its projects, which ask again at their next open. Only the person adds a location: an agent may remove and reorder, as that only makes projects safer, and a new folder is refused with code `agentNotAllowed`.";

    fn run(&self, session: &mut Session, params: LocationsParams) -> Result<LocationList, CommandError> {
        let env = env(session);
        let mut chosen: Vec<PathBuf> = Vec::new();
        for path in &params.locations {
            if path.is_relative() {
                return Err(CommandError::new(t!("workbench-trust-not-folder", path = path.display().to_string())));
            }
            let path = canonical(path);
            if !chosen.contains(&path) {
                chosen.push(path);
            }
        }
        let kept = locations(&env);
        let added: Vec<&PathBuf> = chosen.iter().filter(|path| !kept.contains(path)).collect();
        if zen_core::macros::by_agent(session) && !added.is_empty() {
            return Err(CommandError::with_code(AGENT_NOT_ALLOWED, t!("workbench-trust-agent")));
        }
        if let Some(path) = added.iter().find(|path| !path.is_dir()) {
            return Err(CommandError::new(t!("workbench-trust-not-folder", path = path.display().to_string())));
        }
        env.set_settings(LOCATIONS, &chosen)
            .map_err(|error| CommandError::new(t!("workbench-trust-write", error = error.as_str())))?;
        forget_launches(session);
        Ok(location_list(&env))
    }
}

#[cfg(test)]
mod tests;

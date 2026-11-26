use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;
use crate::inspect_code::{inspection_id, known_inspections};
use crate::language::{Problem, Severity};

pub const PROJECT_FOLDER: &str = ".zen/inspectionProfiles";
pub const PROJECT_DEFAULT: &str = "Project Default";

const CHOICE_FILE: &str = "profiles_settings.json";
const EXTENSION: &str = "json";
const RECHECK: Duration = Duration::from_secs(2);

/// Where a profile is kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProfileLevel {
    /// In the project, `.zen/inspectionProfiles`, shared with whoever has the project.
    #[default]
    Project,
    /// In the IDE's data folder, for every project.
    Ide,
}

/// Names a profile.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileId {
    /// Its name.
    pub name: String,
    /// Where it is kept.
    #[serde(default)]
    pub level: ProfileLevel,
}

impl Default for ProfileId {
    fn default() -> Self {
        Self { name: PROJECT_DEFAULT.to_owned(), level: ProfileLevel::Project }
    }
}

/// What a profile changes about one inspection; what is absent stays as declared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InspectionOverride {
    /// Whether it runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// The severity of what it finds, instead of the one each finding comes with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
}

impl InspectionOverride {
    fn is_empty(self) -> bool {
        self.enabled.is_none() && self.severity.is_none()
    }
}

/// A profile's file: the name, and the inspections it changes by id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InspectionProfile {
    /// Its name.
    pub name: String,
    /// What it changes, by the id of the inspection.
    pub inspections: BTreeMap<String, InspectionOverride>,
}

impl InspectionProfile {
    fn named(name: &str) -> Self {
        Self { name: name.to_owned(), inspections: BTreeMap::new() }
    }

    pub fn override_of(&self, problem: &Problem) -> Option<InspectionOverride> {
        let by = |id: &str| self.inspections.get(id).copied();
        by(&inspection_id(problem))
            .or_else(|| problem.source.as_deref().and_then(by))
            .or_else(|| problem.code.as_deref().and_then(by))
    }

    pub fn adjust(&self, problem: &mut Problem) -> bool {
        let Some(setting) = self.override_of(problem) else { return true };
        if let Some(severity) = setting.severity {
            problem.severity = severity;
        }
        setting.enabled != Some(false)
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Choice {
    active: Option<ProfileId>,
}

type Stamp = Option<(u64, SystemTime)>;

#[derive(Default)]
pub struct InspectionProfiles {
    ide_folder: Option<PathBuf>,
    root: Option<PathBuf>,
    active: ProfileId,
    profile: InspectionProfile,
    stamps: (Stamp, Stamp),
    checked: Option<Instant>,
}

impl InspectionProfiles {
    pub fn set_ide_folder(&mut self, folder: PathBuf) {
        self.ide_folder = Some(folder);
    }

    pub fn active(&self) -> &ProfileId {
        &self.active
    }

    pub fn profile(&self) -> &InspectionProfile {
        &self.profile
    }
}

fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()?))
}

fn file_name(name: &str) -> String {
    let stem: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    format!("{stem}.{EXTENSION}")
}

fn read(file: &Path) -> Option<InspectionProfile> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

fn write(file: &Path, profile: &InspectionProfile) -> Result<(), CommandError> {
    let failed = |error: &dyn std::fmt::Display| {
        CommandError::new(t!("workbench-profile-write-failed", error = error.to_string()))
    };
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| failed(&error))?;
    }
    let text = serde_json::to_vec_pretty(profile).map_err(|error| failed(&error))?;
    zen_vfs::write_atomic(file, &text).map_err(|error| failed(&error))
}

fn root_of(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

fn folder_of(session: &Session, level: ProfileLevel) -> Result<PathBuf, CommandError> {
    match level {
        ProfileLevel::Project => root_of(session)
            .map(|root| root.join(PROJECT_FOLDER))
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-project"))),
        ProfileLevel::Ide => session
            .state::<InspectionProfiles>()
            .and_then(|profiles| profiles.ide_folder.clone())
            .ok_or_else(|| CommandError::new(t!("workbench-profile-no-ide-folder"))),
    }
}

fn file_of(session: &Session, id: &ProfileId) -> Result<PathBuf, CommandError> {
    Ok(folder_of(session, id.level)?.join(file_name(&id.name)))
}

fn profiles_in(folder: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == EXTENSION)
                && path.file_name().is_some_and(|name| name != CHOICE_FILE)
        })
        .filter_map(|path| read(&path))
        .map(|profile| profile.name)
        .collect()
}

fn is_builtin(id: &ProfileId) -> bool {
    id.level == ProfileLevel::Project && id.name == PROJECT_DEFAULT
}

fn exists(session: &Session, id: &ProfileId) -> bool {
    is_builtin(id) || file_of(session, id).is_ok_and(|file| file.is_file())
}

fn load(session: &Session, id: &ProfileId) -> Result<InspectionProfile, CommandError> {
    if !exists(session, id) {
        return Err(CommandError::new(t!("workbench-profile-unknown", name = id.name.as_str())));
    }
    let file = file_of(session, id)?;
    match read(&file) {
        Some(profile) => Ok(profile),
        None if file.is_file() => {
            Err(CommandError::new(t!("workbench-profile-unreadable", file = file.display().to_string())))
        }
        None => Ok(InspectionProfile::named(&id.name)),
    }
}

fn chosen(root: &Path) -> Option<ProfileId> {
    let bytes = std::fs::read(root.join(PROJECT_FOLDER).join(CHOICE_FILE)).ok()?;
    serde_json::from_slice::<Choice>(&bytes).ok()?.active
}

fn reload(session: &mut Session, root: Option<PathBuf>) {
    let wanted = root.as_deref().and_then(chosen).filter(|id| exists(session, id)).unwrap_or_default();
    let profile = load(session, &wanted).unwrap_or_else(|_| InspectionProfile::named(&wanted.name));
    let choice = root.as_ref().map(|root| root.join(PROJECT_FOLDER).join(CHOICE_FILE));
    let stamps = (choice.as_deref().and_then(stamp), file_of(session, &wanted).ok().and_then(|file| stamp(&file)));
    let state = session.state_mut::<InspectionProfiles>();
    state.root = root;
    state.active = wanted;
    state.profile = profile;
    state.stamps = stamps;
    state.checked = Some(Instant::now());
}

fn reload_project(session: &mut Session) {
    let root = root_of(session);
    reload(session, root);
}

pub(crate) fn refresh(session: &mut Session) {
    let root = root_of(session);
    let state = session.state::<InspectionProfiles>();
    let same_root = state.is_some_and(|state| state.root == root);
    if same_root && state.and_then(|state| state.checked).is_some_and(|checked| checked.elapsed() < RECHECK) {
        return;
    }
    let active = state.map(|state| state.active.clone()).unwrap_or_default();
    let choice = root.as_ref().map(|root| root.join(PROJECT_FOLDER).join(CHOICE_FILE));
    let stamps = (choice.as_deref().and_then(stamp), file_of(session, &active).ok().and_then(|file| stamp(&file)));
    if same_root && state.is_some_and(|state| state.stamps == stamps) {
        session.state_mut::<InspectionProfiles>().checked = Some(Instant::now());
        return;
    }
    reload(session, root);
}

pub(crate) fn apply(session: &Session, problems: &mut Vec<Problem>) {
    let Some(state) = session.state::<InspectionProfiles>() else { return };
    if !state.profile.inspections.is_empty() {
        problems.retain_mut(|problem| state.profile.adjust(problem));
    }
}

pub(crate) fn analyzer_switches(session: &mut Session) -> BTreeMap<String, bool> {
    refresh(session);
    let Some(state) = session.state::<InspectionProfiles>() else { return BTreeMap::new() };
    let switches = state.profile.inspections.iter();
    switches.filter_map(|(id, setting)| Some((id.clone(), setting.enabled?))).collect()
}

pub(crate) fn resolve(session: &mut Session, id: Option<&ProfileId>) -> Result<InspectionProfile, CommandError> {
    refresh(session);
    match id {
        Some(id) => load(session, id),
        None => Ok(session.state::<InspectionProfiles>().map(|state| state.profile.clone()).unwrap_or_default()),
    }
}

fn guard_write(session: &Session, file: &Path) -> Result<(), CommandError> {
    let root = root_of(session).unwrap_or_default();
    crate::hidden_files::guard_change(session, &root, &[file])
}

fn store(session: &mut Session, id: &ProfileId, profile: &InspectionProfile) -> Result<(), CommandError> {
    let file = file_of(session, id)?;
    guard_write(session, &file)?;
    write(&file, profile)?;
    reload_project(session);
    Ok(())
}

fn valid_name(name: &str) -> Result<String, CommandError> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(CommandError::new(t!("workbench-profile-bad-name")));
    }
    Ok(name.to_owned())
}

fn free(session: &Session, id: &ProfileId) -> Result<(), CommandError> {
    if exists(session, id) {
        return Err(CommandError::new(t!("workbench-profile-exists", name = id.name.as_str())));
    }
    Ok(())
}

fn chosen_or_active(session: &mut Session, id: Option<ProfileId>) -> ProfileId {
    refresh(session);
    id.unwrap_or_else(|| session.state::<InspectionProfiles>().map(|state| state.active.clone()).unwrap_or_default())
}

fn select(session: &mut Session, id: &ProfileId) -> Result<(), CommandError> {
    let root = root_of(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let failed = |error: &dyn std::fmt::Display| {
        CommandError::new(t!("workbench-profile-write-failed", error = error.to_string()))
    };
    let folder = root.join(PROJECT_FOLDER);
    guard_write(session, &folder.join(CHOICE_FILE))?;
    std::fs::create_dir_all(&folder).map_err(|error| failed(&error))?;
    let text = serde_json::to_vec_pretty(&Choice { active: Some(id.clone()) }).map_err(|error| failed(&error))?;
    zen_vfs::write_atomic(&folder.join(CHOICE_FILE), &text).map_err(|error| failed(&error))?;
    reload(session, Some(root));
    Ok(())
}

/// The profiles there are, and which one applies.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileList {
    /// The one the editor, the Problems tool window and Inspect Code use.
    pub active: ProfileId,
    /// Every profile, the project's first.
    pub profiles: Vec<ProfileId>,
}

pub struct ListProfiles;

impl Command for ListProfiles {
    type Params = NoParams;
    type Output = ProfileList;
    const ID: &'static str = "inspect.profile.list";
    const TITLE: &'static str = "List Inspection Profiles";
    const DESCRIPTION: &'static str = "The inspection profiles of the project (.zen/inspectionProfiles, always with Project \
         Default) and of the IDE, and the one that applies to the editor, the Problems tool window and Inspect Code.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ProfileList, CommandError> {
        let active = chosen_or_active(session, None);
        let mut profiles = vec![ProfileId::default()];
        for level in [ProfileLevel::Project, ProfileLevel::Ide] {
            let Ok(folder) = folder_of(session, level) else { continue };
            let mut names = profiles_in(&folder);
            names.sort_by_key(|name| name.to_lowercase());
            let found = names.into_iter().map(|name| ProfileId { name, level });
            profiles.extend(found.filter(|id| !is_builtin(id)));
        }
        Ok(ProfileList { active, profiles })
    }
}

/// An inspection in a profile.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInspection {
    /// Its id.
    pub id: String,
    /// The name shown.
    pub title: String,
    /// The group it is listed under.
    pub group: String,
    /// What it reports.
    pub description: String,
    /// Whether Code Cleanup applies its fixes.
    pub cleanup: bool,
    /// Whether it runs when the profile says nothing about it.
    pub enabled_by_default: bool,
    /// Whether it runs in this profile.
    pub enabled: bool,
    /// The severity this profile gives it; each finding keeps its own when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<Severity>,
    /// Whether this profile differs from the defaults here; the settings page shows it in blue.
    pub modified: bool,
}

/// One profile, with every inspection the IDE knows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    /// Which profile.
    pub profile: ProfileId,
    /// Its inspections, by group.
    pub inspections: Vec<ProfileInspection>,
    /// What it holds for inspections the IDE does not know, kept as it is.
    pub unknown: BTreeMap<String, InspectionOverride>,
}

/// Names a profile, the active one when empty.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProfileParams {
    /// The profile; the active one when absent.
    pub profile: Option<ProfileId>,
}

pub struct GetProfile;

impl Command for GetProfile {
    type Params = ProfileParams;
    type Output = ProfileView;
    const ID: &'static str = "inspect.profile.get";
    const TITLE: &'static str = "Show Inspection Profile";
    const DESCRIPTION: &'static str = "A profile's inspections by group, as the settings page shows them: each with its \
         description, whether Code Cleanup uses it, whether it is enabled, its severity and whether the profile changed it.";

    fn run(&self, session: &mut Session, params: ProfileParams) -> Result<ProfileView, CommandError> {
        let id = chosen_or_active(session, params.profile);
        let mut profile = load(session, &id)?;
        let mut inspections: Vec<ProfileInspection> = known_inspections(session)
            .into_iter()
            .map(|declared| {
                let setting = profile.inspections.remove(&declared.id).unwrap_or_default();
                let enabled_by_default = !declared.off;
                ProfileInspection {
                    enabled: setting.enabled.unwrap_or(enabled_by_default),
                    modified: setting.severity.is_some()
                        || setting.enabled.is_some_and(|enabled| enabled != enabled_by_default),
                    severity: setting.severity,
                    enabled_by_default,
                    id: declared.id,
                    title: declared.title,
                    group: declared.group,
                    description: declared.description,
                    cleanup: declared.cleanup,
                }
            })
            .collect();
        inspections.sort_by_key(|row| (row.group.to_lowercase(), row.title.to_lowercase()));
        Ok(ProfileView { profile: id, inspections, unknown: profile.inspections })
    }
}

pub struct SelectProfile;

/// What `inspect.profile.select` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectParams {
    /// The profile that applies from now on.
    pub profile: ProfileId,
}

impl Command for SelectProfile {
    type Params = SelectParams;
    type Output = ();
    const ID: &'static str = "inspect.profile.select";
    const TITLE: &'static str = "Select Inspection Profile";
    const DESCRIPTION: &'static str = "Picks the profile that applies to this project (kept in \
         .zen/inspectionProfiles/profiles_settings.json); a language server gets its analyzer switches at once.";

    fn run(&self, session: &mut Session, params: SelectParams) -> Result<(), CommandError> {
        if !exists(session, &params.profile) {
            return Err(CommandError::new(t!("workbench-profile-unknown", name = params.profile.name.as_str())));
        }
        select(session, &params.profile)
    }
}

/// What `inspect.profile.set` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SetParams {
    /// The profile; the active one when absent.
    pub profile: Option<ProfileId>,
    /// What to change, by inspection id. An override with nothing in it brings the inspection back to its defaults.
    pub inspections: BTreeMap<String, InspectionOverride>,
}

pub struct SetInspections;

impl Command for SetInspections {
    type Params = SetParams;
    type Output = ();
    const ID: &'static str = "inspect.profile.set";
    const TITLE: &'static str = "Change Inspection Profile";
    const DESCRIPTION: &'static str = "Sets whether inspections are enabled and at what severity in a profile; the others \
         stay as they are. It takes effect at once in the editor, the Problems tool window and Inspect Code, and analyzer \
         switches go to the language server.";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<(), CommandError> {
        let id = chosen_or_active(session, params.profile);
        let mut profile = load(session, &id)?;
        profile.name.clone_from(&id.name);
        for (inspection, setting) in params.inspections {
            if setting.is_empty() {
                profile.inspections.remove(&inspection);
            } else {
                profile.inspections.insert(inspection, setting);
            }
        }
        store(session, &id, &profile)
    }
}

pub struct RestoreProfile;

impl Command for RestoreProfile {
    type Params = ProfileParams;
    type Output = ();
    const ID: &'static str = "inspect.profile.restore";
    const TITLE: &'static str = "Restore Inspection Profile Defaults";
    const DESCRIPTION: &'static str = "Takes every change out of a profile, as the gear menu's Restore Defaults does.";

    fn run(&self, session: &mut Session, params: ProfileParams) -> Result<(), CommandError> {
        let id = chosen_or_active(session, params.profile);
        load(session, &id)?;
        store(session, &id, &InspectionProfile::named(&id.name))
    }
}

/// What `inspect.profile.copy` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyParams {
    /// The profile to copy; the active one when absent.
    #[serde(default)]
    pub profile: Option<ProfileId>,
    /// The copy's name and where it goes: another level is Copy to IDE or Copy to Project, the same level Duplicate.
    pub to: ProfileId,
}

pub struct CopyProfile;

impl Command for CopyProfile {
    type Params = CopyParams;
    type Output = ();
    const ID: &'static str = "inspect.profile.copy";
    const TITLE: &'static str = "Copy Inspection Profile";
    const DESCRIPTION: &'static str = "Copies a profile under a new name, in the project or the IDE: the gear menu's Copy to \
         IDE, Copy to Project and Duplicate. The copy does not become the active profile.";

    fn run(&self, session: &mut Session, params: CopyParams) -> Result<(), CommandError> {
        let id = chosen_or_active(session, params.profile);
        let to = ProfileId { name: valid_name(&params.to.name)?, level: params.to.level };
        free(session, &to)?;
        let mut profile = load(session, &id)?;
        profile.name.clone_from(&to.name);
        store(session, &to, &profile)
    }
}

/// What `inspect.profile.rename` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RenameParams {
    /// The profile; the active one when absent.
    #[serde(default)]
    pub profile: Option<ProfileId>,
    /// Its new name.
    pub name: String,
}

pub struct RenameProfile;

impl Command for RenameProfile {
    type Params = RenameParams;
    type Output = ();
    const ID: &'static str = "inspect.profile.rename";
    const TITLE: &'static str = "Rename Inspection Profile";
    const DESCRIPTION: &'static str = "Renames a profile; Project Default keeps its name.";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<(), CommandError> {
        let id = chosen_or_active(session, params.profile);
        if is_builtin(&id) {
            return Err(CommandError::new(t!("workbench-profile-builtin")));
        }
        let to = ProfileId { name: valid_name(&params.name)?, level: id.level };
        free(session, &to)?;
        let mut profile = load(session, &id)?;
        profile.name.clone_from(&to.name);
        guard_write(session, &file_of(session, &id)?)?;
        store(session, &to, &profile)?;
        remove(session, &id)?;
        if session.state::<InspectionProfiles>().is_some_and(|state| state.active == id) {
            select(session, &to)?;
        }
        reload_project(session);
        Ok(())
    }
}

fn remove(session: &Session, id: &ProfileId) -> Result<(), CommandError> {
    guard_write(session, &file_of(session, id)?)?;
    std::fs::remove_file(file_of(session, id)?)
        .map_err(|error| CommandError::new(t!("workbench-profile-write-failed", error = error.to_string())))
}

pub struct DeleteProfile;

impl Command for DeleteProfile {
    type Params = ProfileParams;
    type Output = ();
    const ID: &'static str = "inspect.profile.delete";
    const TITLE: &'static str = "Delete Inspection Profile";
    const DESCRIPTION: &'static str = "Deletes a profile; Project Default stays. When it was the active one, Project Default \
         takes over.";

    fn run(&self, session: &mut Session, params: ProfileParams) -> Result<(), CommandError> {
        let id = chosen_or_active(session, params.profile);
        if is_builtin(&id) {
            return Err(CommandError::new(t!("workbench-profile-builtin")));
        }
        load(session, &id)?;
        remove(session, &id)?;
        if session.state::<InspectionProfiles>().is_some_and(|state| state.active == id) {
            select(session, &ProfileId::default())?;
        }
        reload_project(session);
        Ok(())
    }
}

/// What `inspect.profile.export` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    /// The profile; the active one when absent.
    #[serde(default)]
    pub profile: Option<ProfileId>,
    /// The file to write.
    pub path: PathBuf,
}

pub struct ExportProfile;

impl Command for ExportProfile {
    type Params = ExportParams;
    type Output = ();
    const ID: &'static str = "inspect.profile.export";
    const TITLE: &'static str = "Export Inspection Profile";
    const DESCRIPTION: &'static str = "Writes a profile to a file, in the JSON of .zen/inspectionProfiles.";

    fn run(&self, session: &mut Session, params: ExportParams) -> Result<(), CommandError> {
        let id = chosen_or_active(session, params.profile);
        let mut profile = load(session, &id)?;
        profile.name.clone_from(&id.name);
        if zen_core::macros::by_agent(session) {
            let inside = root_of(session).is_some_and(|root| crate::inside_project(&root, &params.path));
            if !inside {
                return Err(CommandError::new(t!(
                    "workbench-error-outside-project",
                    path = params.path.display().to_string()
                )));
            }
            guard_write(session, &params.path)?;
        }
        write(&params.path, &profile)
    }
}

/// What `inspect.profile.import` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportParams {
    /// A file written by inspect.profile.export.
    pub path: PathBuf,
    /// Where the profile goes.
    #[serde(default)]
    pub level: ProfileLevel,
}

pub struct ImportProfile;

impl Command for ImportProfile {
    type Params = ImportParams;
    type Output = ProfileId;
    const ID: &'static str = "inspect.profile.import";
    const TITLE: &'static str = "Import Inspection Profile";
    const DESCRIPTION: &'static str = "Adds a profile from a file written by inspect.profile.export. A profile of that name \
         is left alone and the import is refused.";

    fn run(&self, session: &mut Session, params: ImportParams) -> Result<ProfileId, CommandError> {
        let unreadable =
            || CommandError::new(t!("workbench-profile-unreadable", file = params.path.display().to_string()));
        crate::hidden_files::guard_read(session, std::slice::from_ref(&params.path))?;
        let profile = read(&params.path).ok_or_else(unreadable)?;
        let id = ProfileId { name: valid_name(&profile.name).map_err(|_| unreadable())?, level: params.level };
        free(session, &id)?;
        store(session, &id, &profile)?;
        Ok(id)
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListProfiles)?;
    registry.register(GetProfile)?;
    registry.register(SelectProfile)?;
    registry.register(SetInspections)?;
    registry.register(RestoreProfile)?;
    registry.register(CopyProfile)?;
    registry.register(RenameProfile)?;
    registry.register(DeleteProfile)?;
    registry.register(ExportProfile)?;
    registry.register(ImportProfile)?;
    Ok(())
}

#[cfg(test)]
mod tests;

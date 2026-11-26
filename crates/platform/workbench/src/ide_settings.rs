pub use zen_vfs::zip;
pub(crate) use zip::utc;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::keymaps::{KeymapFile, SharedKeymaps};
use crate::plugin_manager::SharedPlugins;
use crate::themes::SharedThemes;
use zip::{Archive, Limits, ZipError, ZipWriter};

pub const MANIFEST: &str = "zen-settings.json";

pub const BACKUPS: &str = "settings-backups";

const INSTALLED: &str = "plugins/installed.json";
const FORMAT_VERSION: u32 = 1;
const JETBRAINS_MARKER: &str = "IntelliJ IDEA Global Settings";
const FOLDER_DEPTH: usize = 4;

/// A part of the IDE's settings that is exported, imported and restored as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Component {
    /// `settings.json` and `language-servers.json`.
    Settings,
    /// `keymap.json` and the named keymaps of `keymaps/`.
    Keymaps,
    /// The themes of `themes/`.
    Themes,
    /// `templates.json`, the live templates.
    Templates,
    /// Which plug-ins and built-in modules are on, and the list of installed plug-ins.
    Plugins,
}

impl Component {
    pub const ALL: [Component; 5] =
        [Component::Settings, Component::Keymaps, Component::Themes, Component::Templates, Component::Plugins];

    pub fn id(self) -> &'static str {
        match self {
            Component::Settings => "settings",
            Component::Keymaps => "keymaps",
            Component::Themes => "themes",
            Component::Templates => "templates",
            Component::Plugins => "plugins",
        }
    }

    fn files(self) -> &'static [&'static str] {
        match self {
            Component::Settings => &["settings.json", "language-servers.json"],
            Component::Keymaps => &["keymap.json"],
            Component::Themes => &[],
            Component::Templates => &["templates.json"],
            Component::Plugins => {
                &["modules.json", "plugins/enabled.json", "plugins/disabled.json", "plugins/ignored-extensions.json"]
            }
        }
    }

    fn folder(self) -> Option<&'static str> {
        match self {
            Component::Keymaps => Some("keymaps"),
            Component::Themes => Some("themes"),
            _ => None,
        }
    }

    fn of(name: &str) -> Option<Component> {
        if name == INSTALLED {
            return Some(Component::Plugins);
        }
        Component::ALL.into_iter().find(|component| {
            component.files().contains(&name)
                || component.folder().is_some_and(|folder| {
                    name.strip_prefix(folder)
                        .and_then(|rest| rest.strip_prefix('/'))
                        .is_some_and(|file| plain_file(file, ".json"))
                })
        })
    }

    fn of_jetbrains(name: &str) -> Option<Component> {
        let (folder, file) = name.split_once('/')?;
        match folder {
            "keymaps" if plain_file(file, ".xml") => Some(Component::Keymaps),
            "colors" if plain_file(file, ".icls") || plain_file(file, ".xml") => Some(Component::Themes),
            _ => None,
        }
    }
}

fn plain_file(file: &str, extension: &str) -> bool {
    !file.contains('/') && !file.starts_with('.') && file.len() > extension.len() && file.ends_with(extension)
}

#[derive(Clone)]
pub struct SettingsTarget {
    pub data: Option<PathBuf>,
    pub product: String,
    pub version: String,
    pub keymaps: Option<SharedKeymaps>,
    pub themes: Option<SharedThemes>,
    pub plugins: Option<SharedPlugins>,
}

impl SettingsTarget {
    fn data(&self) -> Result<&Path, CommandError> {
        self.data.as_deref().ok_or_else(|| CommandError::new(t!("workbench-ide-settings-error-no-data")))
    }

    fn installed(&self) -> Option<Vec<InstalledPlugin>> {
        let plugins = self.plugins.as_ref()?;
        Some(
            plugins
                .installed("")
                .plugins
                .into_iter()
                .filter_map(|plugin| Some(InstalledPlugin { version: plugin.installed?, id: plugin.id }))
                .collect(),
        )
    }
}

/// A plug-in in the exported list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InstalledPlugin {
    /// The plug-in's id.
    pub id: String,
    /// The version installed.
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub format_version: u32,
    pub kind: String,
    pub product: String,
    pub version: String,
    pub created: u64,
    pub components: Vec<Component>,
}

/// Where an import reads from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// Settings exported from a Zen IDE.
    Zen,
    /// A backup made by Restore Default Settings.
    Backup,
    /// Settings exported from a JetBrains IDE: its keymaps and colour schemes are read.
    JetBrains,
    /// One keymap or theme file: a JetBrains keymap `.xml`, a colour scheme `.icls` or `.xml`, or a theme `.json`.
    File,
}

impl SourceKind {
    fn adds_through_importers(self) -> bool {
        matches!(self, SourceKind::JetBrains | SourceKind::File)
    }
}

/// A component with the entries or files it has.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ComponentEntries {
    /// The component.
    pub id: Component,
    /// Its files or archive entries, relative, with `/` between folders.
    pub entries: Vec<String>,
}

/// An entry that is not imported, and why.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Skipped {
    /// The entry, relative, with `/` between folders.
    pub entry: String,
    /// Why it is skipped.
    pub reason: String,
}

/// What `settings.ide.components` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ComponentsReport {
    /// The IDE's data folder.
    pub data: PathBuf,
    /// The folder Restore Default Settings backs up into.
    pub backups: PathBuf,
    /// Every component with the files it has now.
    pub components: Vec<ComponentEntries>,
}

/// The parameters of `settings.ide.export`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExportParams {
    /// The ZIP file to write, an absolute path; a file there is replaced.
    pub path: PathBuf,
    /// The components to export; all of them when omitted.
    #[serde(default)]
    pub components: Option<Vec<Component>>,
}

/// What `settings.ide.export` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportReport {
    /// The ZIP file written.
    pub path: PathBuf,
    /// The components exported with their entries.
    pub components: Vec<ComponentEntries>,
    /// The archive's size in bytes.
    pub bytes: u64,
}

/// The parameters of `settings.ide.inspect`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InspectParams {
    /// A ZIP file or a folder of settings (a backup), an absolute path.
    pub path: PathBuf,
}

/// What `settings.ide.inspect` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    /// The ZIP file or folder.
    pub path: PathBuf,
    /// What wrote it.
    pub kind: SourceKind,
    /// The product that wrote it, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// The components that can be imported, with their entries.
    pub components: Vec<ComponentEntries>,
    /// The entries that cannot be imported, and why.
    pub skipped: Vec<Skipped>,
}

/// The parameters of `settings.ide.import`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportParams {
    /// A ZIP file or a folder of settings (a backup), an absolute path.
    pub path: PathBuf,
    /// The components to import; all those it has when omitted.
    #[serde(default)]
    pub components: Option<Vec<Component>>,
}

/// What `settings.ide.import` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// The ZIP file or folder.
    pub path: PathBuf,
    /// What wrote it.
    pub kind: SourceKind,
    /// The components imported with their entries.
    pub imported: Vec<ComponentEntries>,
    /// The entries not imported, and why.
    pub skipped: Vec<Skipped>,
    /// Whether the IDE must restart to use what was imported.
    pub restart_required: bool,
}

/// What `settings.ide.restoreDefaults` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    /// The folder the settings were copied to; Import Settings… reads it back.
    pub backup: PathBuf,
    /// The files backed up and removed, relative to the data folder.
    pub files: Vec<String>,
    /// Whether the IDE must restart to use the defaults: always.
    pub restart_required: bool,
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs())
}

fn regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
}

pub fn component_files(data: &Path, component: Component) -> Vec<String> {
    let mut files: Vec<String> =
        component.files().iter().filter(|file| regular_file(&data.join(file))).map(|&file| file.to_owned()).collect();
    if let Some(folder) = component.folder() {
        let mut named: Vec<String> = std::fs::read_dir(data.join(folder))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
                    .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
                    .filter(|name| plain_file(name, ".json"))
                    .map(|name| format!("{folder}/{name}"))
                    .collect()
            })
            .unwrap_or_default();
        named.sort();
        files.extend(named);
    }
    files
}

fn components(data: &Path) -> Vec<ComponentEntries> {
    Component::ALL.into_iter().map(|id| ComponentEntries { id, entries: component_files(data, id) }).collect()
}

fn megabytes(bytes: u64) -> u64 {
    bytes / (1024 * 1024)
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn absolute(path: &Path) -> Result<(), CommandError> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(CommandError::new(t!("workbench-ide-settings-error-relative", path = path_text(path))))
    }
}

fn zip_error(path: &Path, error: &ZipError) -> CommandError {
    let path = path_text(path);
    CommandError::new(match error {
        ZipError::NotZip => t!("workbench-ide-settings-error-not-zip", path = path),
        ZipError::Zip64 => t!("workbench-ide-settings-error-zip64", path = path),
        ZipError::TooLarge(limit) => {
            t!("workbench-ide-settings-error-too-large", path = path, megabytes = megabytes(*limit))
        }
        ZipError::TooManyEntries(limit) => {
            t!("workbench-ide-settings-error-too-many", path = path, count = limit.to_string())
        }
        ZipError::EntryTooLarge { name, limit } => t!(
            "workbench-ide-settings-error-entry-too-large",
            path = path,
            entry = name.as_str(),
            megabytes = megabytes(*limit)
        ),
        ZipError::Unsafe(name) => t!("workbench-ide-settings-error-unsafe", path = path, entry = name.as_str()),
        other => t!("workbench-ide-settings-error-damaged", path = path, error = other.to_string()),
    })
}

fn entry_reason(error: &ZipError) -> String {
    match error {
        ZipError::Encrypted => t!("workbench-ide-settings-skip-encrypted"),
        ZipError::Method(method) => t!("workbench-ide-settings-skip-method", method = *method),
        other => t!("workbench-ide-settings-skip-damaged", error = other.to_string()),
    }
}

enum Source {
    Zip(Archive),
    Folder(PathBuf),
}

struct Item {
    name: String,
    problem: Option<String>,
}

fn open(path: &Path) -> Result<(Source, Vec<Item>), CommandError> {
    absolute(path)?;
    let read_error = |error: std::io::Error| {
        CommandError::new(t!("workbench-ide-settings-error-read", path = path_text(path), error = error.to_string()))
    };
    let meta = std::fs::metadata(path).map_err(read_error)?;
    if meta.is_dir() {
        return Ok((Source::Folder(path.to_owned()), folder_items(path)?));
    }
    let limits = Limits::SETTINGS;
    if meta.len() > limits.archive {
        return Err(zip_error(path, &ZipError::TooLarge(limits.archive)));
    }
    let bytes = std::fs::read(path).map_err(read_error)?;
    let archive = Archive::parse(bytes, limits).map_err(|error| zip_error(path, &error))?;
    let items = archive
        .entries()
        .iter()
        .filter(|entry| !entry.folder)
        .map(|entry| Item {
            name: entry.name.clone(),
            problem: entry.link.then(|| t!("workbench-ide-settings-skip-link")),
        })
        .collect();
    Ok((Source::Zip(archive), items))
}

fn folder_items(root: &Path) -> Result<Vec<Item>, CommandError> {
    let limits = Limits::SETTINGS;
    let mut items = Vec::new();
    let mut folders = vec![(root.to_owned(), String::new(), 0usize)];
    while let Some((folder, prefix, depth)) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else { continue };
            if name.starts_with('.') || (depth == 0 && name == BACKUPS) {
                continue;
            }
            let relative = format!("{prefix}{name}");
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if depth + 1 < FOLDER_DEPTH {
                    folders.push((entry.path(), format!("{relative}/"), depth + 1));
                }
                continue;
            }
            let problem = if kind.is_symlink() {
                Some(t!("workbench-ide-settings-skip-link"))
            } else if entry.metadata().is_ok_and(|meta| meta.len() > limits.entry) {
                Some(t!("workbench-ide-settings-skip-too-large", megabytes = megabytes(limits.entry)))
            } else {
                None
            };
            items.push(Item { name: relative, problem });
            if items.len() > limits.entries {
                return Err(zip_error(root, &ZipError::TooManyEntries(limits.entries)));
            }
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(items)
}

fn read(source: &Source, name: &str) -> Result<Vec<u8>, String> {
    match source {
        Source::Zip(archive) => {
            let entry = archive.entries().iter().find(|entry| entry.name == name && !entry.folder);
            let entry = entry.ok_or_else(|| t!("workbench-ide-settings-skip-damaged", error = name))?;
            archive.read(entry).map_err(|error| entry_reason(&error))
        }
        Source::Folder(root) => std::fs::read(root.join(name))
            .map_err(|error| t!("workbench-ide-settings-skip-damaged", error = error.to_string())),
    }
}

struct Planned {
    component: Component,
    entry: String,
    data: Vec<u8>,
}

fn invalid(error: impl std::fmt::Display) -> String {
    t!("workbench-ide-settings-skip-invalid", error = error.to_string())
}

fn validate(component: Component, name: &str, data: &[u8]) -> Result<(), String> {
    let text = std::str::from_utf8(data).map_err(invalid)?;
    match (component, name) {
        (_, "settings.json" | "keymap.json") => {
            let value: serde_json::Value = serde_json::from_str(text).map_err(invalid)?;
            if value.is_object() { Ok(()) } else { Err(invalid("not a JSON object")) }
        }
        (Component::Keymaps, _) => serde_json::from_str::<KeymapFile>(text).map(drop).map_err(invalid),
        (Component::Themes, _) => zen_theme::parse(text).map(drop).map_err(invalid),
        _ => serde_json::from_str::<serde_json::Value>(text).map(drop).map_err(invalid),
    }
}

fn of_single_file(name: &str, text: &str) -> Option<Component> {
    let name = name.to_ascii_lowercase();
    if name.ends_with(".icls") || name.ends_with(".json") {
        return Some(Component::Themes);
    }
    if !name.ends_with(".xml") {
        return None;
    }
    if text.contains("<keymap") {
        Some(Component::Keymaps)
    } else {
        text.contains("<scheme").then_some(Component::Themes)
    }
}

fn plan_single_file(path: &Path) -> Result<Option<(Inspection, Vec<Planned>)>, CommandError> {
    absolute(path)?;
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_ascii_lowercase();
    if !path.is_file() || ![".xml", ".icls", ".json"].iter().any(|extension| lower.ends_with(extension)) {
        return Ok(None);
    }
    let unknown = || CommandError::new(t!("workbench-ide-settings-error-unknown", path = path_text(path)));
    let meta = std::fs::metadata(path).map_err(|_| unknown())?;
    if meta.len() > Limits::SETTINGS.entry {
        return Err(unknown());
    }
    let data = std::fs::read(path).map_err(|_| unknown())?;
    let text = std::str::from_utf8(&data).map_err(|_| unknown())?;
    let component = of_single_file(&name, text).ok_or_else(unknown)?;
    let inspection = Inspection {
        path: path.to_owned(),
        kind: SourceKind::File,
        product: None,
        components: vec![ComponentEntries { id: component, entries: vec![name.clone()] }],
        skipped: Vec::new(),
    };
    Ok(Some((inspection, vec![Planned { component, entry: name, data }])))
}

fn plan(target: &SettingsTarget, path: &Path) -> Result<(Inspection, Vec<Planned>), CommandError> {
    if let Some(found) = plan_single_file(path)? {
        return Ok(found);
    }
    let (source, items) = open(path)?;
    let manifest = items
        .iter()
        .find(|item| item.name == MANIFEST)
        .and_then(|_| read(&source, MANIFEST).ok())
        .and_then(|bytes| serde_json::from_slice::<Manifest>(&bytes).ok());
    let zen =
        manifest.is_some() || items.iter().any(|item| item.name == MANIFEST || Component::of(&item.name).is_some());
    let jetbrains = !zen
        && items.iter().any(|item| {
            Component::of_jetbrains(&item.name).is_some()
                || item.name == JETBRAINS_MARKER
                || item.name.starts_with("options/")
        });
    if !zen && !jetbrains {
        return Err(CommandError::new(t!("workbench-ide-settings-error-unknown", path = path_text(path))));
    }
    let kind = match &manifest {
        Some(manifest) if manifest.kind == "backup" => SourceKind::Backup,
        _ if zen => SourceKind::Zen,
        _ => SourceKind::JetBrains,
    };
    let installed = target.installed();
    let mut planned = Vec::new();
    let mut skipped = Vec::new();
    for item in &items {
        if item.name == MANIFEST {
            continue;
        }
        let component = if zen { Component::of(&item.name) } else { Component::of_jetbrains(&item.name) };
        let Some(component) = component else {
            let reason = if zen {
                t!("workbench-ide-settings-skip-unknown")
            } else {
                t!("workbench-ide-settings-skip-jetbrains")
            };
            skipped.push(Skipped { entry: item.name.clone(), reason });
            continue;
        };
        if let Some(problem) = &item.problem {
            skipped.push(Skipped { entry: item.name.clone(), reason: problem.clone() });
            continue;
        }
        let data = match read(&source, &item.name) {
            Ok(data) => data,
            Err(reason) => {
                skipped.push(Skipped { entry: item.name.clone(), reason });
                continue;
            }
        };
        if !zen {
            if std::str::from_utf8(&data).is_err() {
                skipped.push(Skipped { entry: item.name.clone(), reason: invalid("not UTF-8 text") });
                continue;
            }
        } else if item.name == INSTALLED {
            match serde_json::from_slice::<Vec<InstalledPlugin>>(&data) {
                Ok(listed) => {
                    if let Some(installed) = &installed {
                        for plugin in listed.iter().filter(|plugin| !installed.iter().any(|have| have.id == plugin.id))
                        {
                            skipped.push(Skipped {
                                entry: format!("{INSTALLED}: {}", plugin.id),
                                reason: t!(
                                    "workbench-ide-settings-skip-plugin",
                                    id = plugin.id.as_str(),
                                    version = plugin.version.as_str()
                                ),
                            });
                        }
                    }
                }
                Err(error) => skipped.push(Skipped { entry: item.name.clone(), reason: invalid(error) }),
            }
            continue;
        } else if let Err(reason) = validate(component, &item.name, &data) {
            skipped.push(Skipped { entry: item.name.clone(), reason });
            continue;
        }
        planned.push(Planned { component, entry: item.name.clone(), data });
    }
    let components = Component::ALL
        .into_iter()
        .filter_map(|id| {
            let entries: Vec<String> =
                planned.iter().filter(|planned| planned.component == id).map(|planned| planned.entry.clone()).collect();
            (!entries.is_empty()).then_some(ComponentEntries { id, entries })
        })
        .collect();
    let inspection = Inspection {
        path: path.to_owned(),
        kind,
        product: manifest.map(|manifest| manifest.product),
        components,
        skipped,
    };
    Ok((inspection, planned))
}

pub fn inspect(target: &SettingsTarget, path: &Path) -> Result<Inspection, CommandError> {
    plan(target, path).map(|(inspection, _)| inspection)
}

fn write_file(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    zen_vfs::write_atomic(path, data).map_err(|error| error.to_string())
}

fn add_entry(entries: &mut Vec<ComponentEntries>, id: Component, entry: String) {
    match entries.iter_mut().find(|entries| entries.id == id) {
        Some(entries) => entries.entries.push(entry),
        None => entries.push(ComponentEntries { id, entries: vec![entry] }),
    }
}

pub fn import(target: &SettingsTarget, params: &ImportParams) -> Result<ImportReport, CommandError> {
    let data = target.data()?.to_owned();
    let (inspection, planned) = plan(target, &params.path)?;
    let available: BTreeSet<Component> = inspection.components.iter().map(|entries| entries.id).collect();
    let chosen: BTreeSet<Component> = match &params.components {
        Some(chosen) => chosen.iter().copied().filter(|id| available.contains(id)).collect(),
        None => available.clone(),
    };
    if chosen.is_empty() {
        return Err(CommandError::new(t!("workbench-ide-settings-error-nothing", path = path_text(&params.path))));
    }
    let mut imported = Vec::new();
    let mut skipped = inspection.skipped;
    for planned in planned.into_iter().filter(|planned| chosen.contains(&planned.component)) {
        let Planned { component, entry, data: bytes } = planned;
        let result = if inspection.kind.adds_through_importers() {
            import_jetbrains(target, component, &params.path, &entry, &bytes)
        } else {
            write_file(&data.join(&entry), &bytes)
                .map_err(|error| t!("workbench-ide-settings-skip-write", error = error))
        };
        match result {
            Ok(()) => add_entry(&mut imported, component, entry),
            Err(reason) => skipped.push(Skipped { entry, reason }),
        }
    }
    let written = |id: Component| imported.iter().any(|entries: &ComponentEntries| entries.id == id);
    if !inspection.kind.adds_through_importers() {
        if written(Component::Keymaps)
            && let Some(keymaps) = &target.keymaps
        {
            crate::keymaps::lock(keymaps).reload();
        }
        if written(Component::Themes)
            && let Some(themes) = &target.themes
        {
            crate::themes::lock(themes).reload();
        }
    }
    let restart_required = !inspection.kind.adds_through_importers()
        && [Component::Settings, Component::Templates, Component::Plugins].into_iter().any(written);
    Ok(ImportReport { path: params.path.clone(), kind: inspection.kind, imported, skipped, restart_required })
}

fn import_jetbrains(
    target: &SettingsTarget,
    component: Component,
    archive: &Path,
    entry: &str,
    data: &[u8],
) -> Result<(), String> {
    let text = String::from_utf8_lossy(data);
    let source = archive.join(entry);
    match component {
        Component::Keymaps => {
            let keymaps = target.keymaps.as_ref().ok_or_else(|| t!("workbench-ide-settings-skip-jetbrains"))?;
            crate::keymaps::lock(keymaps).import_text(&source, &text).map(drop).map_err(|error| error.to_string())
        }
        Component::Themes => {
            let themes = target.themes.as_ref().ok_or_else(|| t!("workbench-ide-settings-skip-jetbrains"))?;
            crate::themes::lock(themes)
                .import_text(Path::new(entry), &text)
                .map(drop)
                .map_err(|error| crate::themes::message(&error).message)
        }
        _ => Err(t!("workbench-ide-settings-skip-jetbrains")),
    }
}

fn manifest(target: &SettingsTarget, kind: &str, components: Vec<Component>) -> Vec<u8> {
    let manifest = Manifest {
        format_version: FORMAT_VERSION,
        kind: kind.to_owned(),
        product: target.product.clone(),
        version: target.version.clone(),
        created: now(),
        components,
    };
    let mut text = serde_json::to_string_pretty(&manifest).unwrap_or_default();
    text.push('\n');
    text.into_bytes()
}

pub fn in_project_settings(path: &Path) -> bool {
    path.components().any(|part| matches!(part.as_os_str().to_str(), Some(".zen" | ".idea")))
}

pub fn export(target: &SettingsTarget, params: &ExportParams) -> Result<ExportReport, CommandError> {
    let data = target.data()?;
    let path = &params.path;
    absolute(path)?;
    if in_project_settings(path) {
        return Err(CommandError::new(t!("workbench-ide-settings-error-project", path = path_text(path))));
    }
    if path.is_dir() {
        return Err(CommandError::new(t!("workbench-ide-settings-error-folder", path = path_text(path))));
    }
    let chosen: Vec<Component> = match &params.components {
        Some(chosen) => Component::ALL.into_iter().filter(|id| chosen.contains(id)).collect(),
        None => Component::ALL.to_vec(),
    };
    if chosen.is_empty() {
        return Err(CommandError::new(t!("workbench-ide-settings-error-no-components")));
    }
    let write_error = |error: String| {
        CommandError::new(t!("workbench-ide-settings-error-write", path = path_text(path), error = error))
    };
    let mut writer = ZipWriter::new(now());
    writer
        .add(MANIFEST, &manifest(target, "export", chosen.clone()))
        .map_err(|error| write_error(error.to_string()))?;
    let mut components = Vec::new();
    for id in chosen {
        let mut entries = Vec::new();
        for file in component_files(data, id) {
            let bytes = std::fs::read(data.join(&file)).map_err(|error| {
                CommandError::new(t!(
                    "workbench-ide-settings-error-read",
                    path = path_text(&data.join(&file)),
                    error = error.to_string()
                ))
            })?;
            writer.add(&file, &bytes).map_err(|error| write_error(error.to_string()))?;
            entries.push(file);
        }
        if id == Component::Plugins
            && let Some(installed) = target.installed()
        {
            let mut text = serde_json::to_string_pretty(&installed).unwrap_or_default();
            text.push('\n');
            writer.add(INSTALLED, text.as_bytes()).map_err(|error| write_error(error.to_string()))?;
            entries.push(INSTALLED.to_owned());
        }
        components.push(ComponentEntries { id, entries });
    }
    let bytes = writer.finish();
    write_file(path, &bytes).map_err(write_error)?;
    Ok(ExportReport { path: path.clone(), components, bytes: bytes.len() as u64 })
}

fn backup_folder(data: &Path) -> PathBuf {
    let (year, month, day, hour, minute, second) = utc(now());
    let base = format!("{year:04}-{month:02}-{day:02}-{hour:02}{minute:02}{second:02}");
    let folder = data.join(BACKUPS);
    let mut candidate = folder.join(&base);
    let mut suffix = 1;
    while candidate.exists() {
        suffix += 1;
        candidate = folder.join(format!("{base}-{suffix}"));
    }
    candidate
}

pub fn restore_defaults(target: &SettingsTarget) -> Result<RestoreReport, CommandError> {
    let data = target.data()?;
    let backup = backup_folder(data);
    let mut files = Vec::new();
    let mut present = Vec::new();
    for id in Component::ALL {
        let found = component_files(data, id);
        if !found.is_empty() {
            present.push(id);
        }
        files.extend(found);
    }
    let copied = (|| -> Result<(), String> {
        std::fs::create_dir_all(&backup).map_err(|error| error.to_string())?;
        for file in &files {
            let bytes = std::fs::read(data.join(file)).map_err(|error| error.to_string())?;
            write_file(&backup.join(file), &bytes)?;
        }
        write_file(&backup.join(MANIFEST), &manifest(target, "backup", present))
    })();
    if let Err(error) = copied {
        let _ = std::fs::remove_dir_all(&backup);
        return Err(CommandError::new(t!(
            "workbench-ide-settings-error-backup",
            path = path_text(&backup),
            error = error
        )));
    }
    for file in &files {
        let _ = std::fs::remove_file(data.join(file));
    }
    for folder in Component::ALL.into_iter().filter_map(Component::folder) {
        let _ = std::fs::remove_dir(data.join(folder));
    }
    Ok(RestoreReport { backup, files, restart_required: true })
}

pub struct ListComponents(pub SettingsTarget);

impl Command for ListComponents {
    type Params = NoParams;
    type Output = ComponentsReport;
    const ID: &'static str = "settings.ide.components";
    const TITLE: &'static str = "IDE Settings Components";
    const DESCRIPTION: &'static str = "Lists the components of the IDE's own settings (settings, keymaps, themes, \
        templates, plugins) with the files each has in the data folder, the data folder and the folder Restore \
        Default Settings backs up into. Project settings (.zen, .idea) are never part of them.";

    fn run(&self, _: &mut Session, _: NoParams) -> Result<ComponentsReport, CommandError> {
        let data = self.0.data()?;
        Ok(ComponentsReport { data: data.to_owned(), backups: data.join(BACKUPS), components: components(data) })
    }
}

pub struct ExportSettings(pub SettingsTarget);

impl Command for ExportSettings {
    type Params = ExportParams;
    type Output = ExportReport;
    const ID: &'static str = "settings.ide.export";
    const TITLE: &'static str = "Export Settings…";
    const DESCRIPTION: &'static str = "Writes the chosen components of the IDE's settings (settings, keymaps, \
        themes, templates, plugins; all when omitted) into a ZIP file at an absolute path, replacing a file there. \
        The archive starts with zen-settings.json naming the product and the components. Refuses paths inside a \
        project's .zen or .idea folder.";

    fn run(&self, _: &mut Session, params: ExportParams) -> Result<ExportReport, CommandError> {
        export(&self.0, &params)
    }
}

pub struct InspectSettings(pub SettingsTarget);

impl Command for InspectSettings {
    type Params = InspectParams;
    type Output = Inspection;
    const ID: &'static str = "settings.ide.inspect";
    const TITLE: &'static str = "Inspect Settings to Import";
    const DESCRIPTION: &'static str = "Reads a ZIP file, folder or single file of settings without changing anything: a Zen IDE \
        export, a backup of Restore Default Settings, a JetBrains Export Settings ZIP (of which keymaps/*.xml \
        and colors/*.icls are read), or one file: a JetBrains keymap .xml, a colour scheme .icls or .xml, or a \
        theme .json. Answers its kind, the components it can give with their entries, and the \
        entries that would be skipped with why. Unsafe entry names (absolute, with ..), ZIP64 and archives over \
        64 MB, 4096 entries or 8 MB an entry are refused.";

    fn run(&self, _: &mut Session, params: InspectParams) -> Result<Inspection, CommandError> {
        inspect(&self.0, &params.path)
    }
}

fn person_only(session: &Session) -> Result<(), CommandError> {
    if zen_core::macros::by_agent(session) {
        return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("workbench-ide-settings-person-only")));
    }
    Ok(())
}

pub struct ImportSettings(pub SettingsTarget);

impl Command for ImportSettings {
    type Params = ImportParams;
    type Output = ImportReport;
    const ID: &'static str = "settings.ide.import";
    const TITLE: &'static str = "Import Settings…";
    const DESCRIPTION: &'static str = "Imports the chosen components (all it has when omitted) of a ZIP file, \
        folder or single file of settings into the IDE's data folder: a Zen IDE export or backup replaces the files \
        it holds; a JetBrains export or a single keymap or theme file adds new keymaps and themes. Answers what was imported, \
        what was skipped and why, and whether the IDE must restart (view.ide.restart) to use it. Never writes a \
        project's .zen or .idea. Only a person: it rewrites settings wholesale, past what set_settings refuses, \
        so an agent gets `agentNotAllowed` and can open the dialog with view.ideSettings.import.";

    fn run(&self, session: &mut Session, params: ImportParams) -> Result<ImportReport, CommandError> {
        person_only(session)?;
        import(&self.0, &params)
    }
}

pub struct RestoreDefaults(pub SettingsTarget);

impl Command for RestoreDefaults {
    type Params = NoParams;
    type Output = RestoreReport;
    const ID: &'static str = "settings.ide.restoreDefaults";
    const TITLE: &'static str = "Restore Default Settings…";
    const DESCRIPTION: &'static str = "Copies the IDE's settings (every component) to a new folder of \
        settings-backups in the data folder, then removes them, so that the IDE starts with the defaults after a \
        restart (view.ide.restart; the windowed action view.ideSettings.restoreDefaults restarts at once once the person \
        answers its dialog, which an agent cannot do). \
        settings.ide.import with the backup folder brings them back. Projects, recent projects, local history and \
        installed plug-ins stay. Only a person: it removes settings wholesale, past what set_settings refuses, so \
        an agent gets `agentNotAllowed`.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RestoreReport, CommandError> {
        person_only(session)?;
        restore_defaults(&self.0)
    }
}

pub fn register(registry: &mut Registry, target: SettingsTarget) -> Result<(), RegistryError> {
    registry.register(ListComponents(target.clone()))?;
    registry.register(ExportSettings(target.clone()))?;
    registry.register(InspectSettings(target.clone()))?;
    registry.register(ImportSettings(target.clone()))?;
    registry.register(RestoreDefaults(target))?;
    Ok(())
}

#[cfg(test)]
mod tests;

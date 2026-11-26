use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::keymap::{Binding, Keymap, KeymapImport, KeymapImporter, Keystroke, MACOS_KEYMAP};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

pub type SharedKeymaps = Arc<Mutex<KeymapStore>>;

pub const FORMAT_VERSION: u32 = 1;

const MAX_IMPORT: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum KeymapSource {
    Builtin,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapEntry {
    pub id: String,
    pub name: String,
    pub source: KeymapSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapFile {
    pub format_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default = "macos")]
    pub parent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default)]
    pub shortcuts: BTreeMap<String, Vec<String>>,
}

fn macos() -> String {
    MACOS_KEYMAP.to_owned()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapList {
    pub current: String,
    pub keymaps: Vec<KeymapEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapImported {
    pub id: String,
    #[serde(flatten)]
    pub report: KeymapImport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeymapCreated {
    pub id: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeymapError {
    Unknown(String),
    Builtin,
    Broken { id: String, error: String },
    EmptyName,
    EmptyAbbreviation,
    NoFolder,
    Read { path: PathBuf, error: String },
    UnknownFormat(PathBuf),
    Import { path: PathBuf, error: String },
    Write { path: PathBuf, error: String },
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeymapError::Unknown(id) => write!(f, "no keymap has the id `{id}`"),
            KeymapError::Builtin => f.write_str("the built-in keymap cannot be renamed or deleted"),
            KeymapError::Broken { id, error } => write!(f, "the keymap `{id}` cannot be used: {error}"),
            KeymapError::EmptyName => f.write_str("a keymap needs a name"),
            KeymapError::EmptyAbbreviation => f.write_str("an abbreviation needs a character other than a space"),
            KeymapError::NoFolder => f.write_str("there is no keymaps folder"),
            KeymapError::Read { path, error } => write!(f, "cannot read {}: {error}", path.display()),
            KeymapError::UnknownFormat(path) => write!(f, "{} is not a keymap of a known format", path.display()),
            KeymapError::Import { path, error } => write!(f, "cannot import {}: {error}", path.display()),
            KeymapError::Write { path, error } => write!(f, "cannot write {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for KeymapError {}

pub struct KeymapStore {
    data: Option<PathBuf>,
    importers: Vec<Box<dyn KeymapImporter>>,
    builtin: BTreeMap<String, Vec<Keystroke>>,
    files: BTreeMap<String, KeymapFile>,
    entries: Vec<KeymapEntry>,
    current: String,
    active: Keymap,
    abbreviations: BTreeMap<String, Vec<String>>,
    generation: u64,
    macros: Vec<String>,
    extra: Vec<Binding>,
}

impl KeymapStore {
    pub fn new(data: Option<PathBuf>, importers: Vec<Box<dyn KeymapImporter>>) -> KeymapStore {
        let mut store = KeymapStore {
            data,
            importers,
            builtin: BTreeMap::new(),
            files: BTreeMap::new(),
            entries: Vec::new(),
            current: MACOS_KEYMAP.to_owned(),
            active: Keymap::default_macos(),
            abbreviations: BTreeMap::new(),
            generation: 0,
            macros: Vec::new(),
            extra: Vec::new(),
        };
        store.scan();
        store.refresh();
        store
    }

    pub fn dir(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|data| data.join("keymaps"))
    }

    pub fn entries(&self) -> &[KeymapEntry] {
        &self.entries
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn active(&self) -> &Keymap {
        &self.active
    }

    pub fn keymap(&self, id: &str) -> Option<Keymap> {
        self.changes(id).map(|changes| {
            Keymap::default_macos()
                .with_macros(self.macros.iter().map(String::as_str))
                .with_bindings(self.extra.iter().cloned())
                .with_user_changes(changes)
        })
    }

    pub fn set_macros(&mut self, names: Vec<String>) {
        if self.macros != names {
            self.macros = names;
            self.refresh();
        }
    }

    pub fn set_extra(&mut self, extra: Vec<Binding>) {
        if self.extra != extra {
            self.extra = extra;
            self.refresh();
        }
    }

    pub fn move_action(&mut self, action: &str, to: Option<&str>) -> Result<(), KeymapError> {
        let ids: Vec<String> = self.entries.iter().map(|entry| entry.id.clone()).collect();
        for id in ids {
            let Some(mut changes) = self.changes(&id) else { continue };
            let Some(keys) = changes.remove(action) else { continue };
            if let Some(to) = to {
                changes.insert(to.to_owned(), keys);
            }
            self.set_changes_of(&id, changes)?;
        }
        Ok(())
    }

    pub fn abbreviations(&self) -> &BTreeMap<String, Vec<String>> {
        &self.abbreviations
    }

    pub fn add_abbreviation(&mut self, action: &str, abbreviation: &str) -> Result<(), KeymapError> {
        let abbreviation = abbreviation.trim();
        if abbreviation.is_empty() || action.trim().is_empty() {
            return Err(KeymapError::EmptyAbbreviation);
        }
        let mut abbreviations = self.abbreviations.clone();
        let list = abbreviations.entry(action.to_owned()).or_default();
        if !list.iter().any(|known| known == abbreviation) {
            list.push(abbreviation.to_owned());
        }
        self.save_abbreviations(abbreviations)
    }

    pub fn remove_abbreviation(&mut self, action: &str, abbreviation: &str) -> Result<(), KeymapError> {
        let mut abbreviations = self.abbreviations.clone();
        if let Some(list) = abbreviations.get_mut(action) {
            list.retain(|known| known != abbreviation.trim());
            if list.is_empty() {
                abbreviations.remove(action);
            }
        }
        self.save_abbreviations(abbreviations)
    }

    fn save_abbreviations(&mut self, abbreviations: BTreeMap<String, Vec<String>>) -> Result<(), KeymapError> {
        if let Some(data) = &self.data {
            let text = serde_json::to_string_pretty(&abbreviations).unwrap_or_default();
            write(&data.join("abbreviations.json"), &text)?;
        }
        self.abbreviations = abbreviations;
        self.generation += 1;
        Ok(())
    }

    pub fn select(&mut self, id: &str) -> Result<(), KeymapError> {
        self.usable(id)?;
        if self.current != id {
            id.clone_into(&mut self.current);
            self.refresh();
        }
        Ok(())
    }

    pub fn set_changes(&mut self, changes: BTreeMap<String, Vec<Keystroke>>) -> Result<(), KeymapError> {
        let current = self.current.clone();
        self.set_changes_of(&current, changes)
    }

    pub fn set_changes_of(&mut self, id: &str, changes: BTreeMap<String, Vec<Keystroke>>) -> Result<(), KeymapError> {
        self.usable(id)?;
        if id == MACOS_KEYMAP {
            if let Some(data) = &self.data {
                let path = data.join("keymap.json");
                let text = serde_json::to_string_pretty(&changes).unwrap_or_default();
                write(&path, &text)?;
            }
            self.builtin = changes;
        } else if let Some(file) = self.files.get(id) {
            let mut file = file.clone();
            file.shortcuts = to_text(&changes);
            self.write_file(&file)?;
            self.files.insert(id.to_owned(), file);
        }
        self.refresh();
        Ok(())
    }

    pub fn import(&mut self, path: &Path) -> Result<KeymapImported, KeymapError> {
        self.dir().ok_or(KeymapError::NoFolder)?;
        let read_error = |error: String| KeymapError::Read { path: path.to_owned(), error };
        let size = std::fs::metadata(path).map_err(|error| read_error(error.to_string()))?.len();
        if size > MAX_IMPORT {
            return Err(read_error(t!("workbench-keymap-error-too-large", megabytes = MAX_IMPORT / (1024 * 1024))));
        }
        let text = std::fs::read_to_string(path)
            .map_err(|error| KeymapError::Read { path: path.to_owned(), error: error.to_string() })?;
        self.import_text(path, &text)
    }

    pub fn import_text(&mut self, path: &Path, text: &str) -> Result<KeymapImported, KeymapError> {
        self.dir().ok_or(KeymapError::NoFolder)?;
        if text.len() as u64 > MAX_IMPORT {
            let error = t!("workbench-keymap-error-too-large", megabytes = MAX_IMPORT / (1024 * 1024));
            return Err(KeymapError::Read { path: path.to_owned(), error });
        }
        let importer = self
            .importers
            .iter()
            .find(|importer| importer.detects(text))
            .ok_or_else(|| KeymapError::UnknownFormat(path.to_owned()))?;
        let mut report = importer.import(text).map_err(|error| KeymapError::Import { path: path.to_owned(), error })?;
        if report.name.trim().is_empty() {
            report.name = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
        }
        let id = self.free_id(&report.name);
        let file = KeymapFile {
            format_version: FORMAT_VERSION,
            id: id.clone(),
            name: report.name.clone(),
            parent: macos(),
            source: Some(path.display().to_string()),
            shortcuts: to_text(&report.shortcuts),
        };
        self.write_file(&file)?;
        self.scan();
        self.refresh();
        Ok(KeymapImported { id, report })
    }

    pub fn duplicate(&mut self, id: &str, name: &str) -> Result<KeymapCreated, KeymapError> {
        self.dir().ok_or(KeymapError::NoFolder)?;
        let name = name.trim();
        if name.is_empty() {
            return Err(KeymapError::EmptyName);
        }
        let changes = self.usable(id)?;
        let new_id = self.free_id(name);
        let file = KeymapFile {
            format_version: FORMAT_VERSION,
            id: new_id.clone(),
            name: name.to_owned(),
            parent: macos(),
            source: self.files.get(id).and_then(|file| file.source.clone()),
            shortcuts: to_text(&changes),
        };
        self.write_file(&file)?;
        self.scan();
        self.refresh();
        Ok(KeymapCreated { id: new_id, name: name.to_owned() })
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), KeymapError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(KeymapError::EmptyName);
        }
        if id == MACOS_KEYMAP {
            return Err(KeymapError::Builtin);
        }
        self.usable(id)?;
        let Some(mut file) = self.files.get(id).cloned() else { return Err(KeymapError::Unknown(id.to_owned())) };
        name.clone_into(&mut file.name);
        self.write_file(&file)?;
        self.scan();
        self.refresh();
        Ok(())
    }

    pub fn delete(&mut self, id: &str) -> Result<(), KeymapError> {
        if id == MACOS_KEYMAP {
            return Err(KeymapError::Builtin);
        }
        let entry =
            self.entries.iter().find(|entry| entry.id == id).ok_or_else(|| KeymapError::Unknown(id.to_owned()))?;
        if let Some(path) = entry.path.clone() {
            match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(KeymapError::Write { path, error: error.to_string() });
                }
                _ => {}
            }
        }
        self.scan();
        if self.current == id {
            self.current = macos();
        }
        self.refresh();
        Ok(())
    }

    pub fn reload(&mut self) {
        self.scan();
        if !self.entries.iter().any(|entry| entry.id == self.current) {
            self.current = macos();
        }
        self.refresh();
    }

    fn changes(&self, id: &str) -> Option<BTreeMap<String, Vec<Keystroke>>> {
        if id == MACOS_KEYMAP {
            return Some(self.builtin.clone());
        }
        self.files.get(id).map(|file| from_text(&file.shortcuts))
    }

    fn usable(&self, id: &str) -> Result<BTreeMap<String, Vec<Keystroke>>, KeymapError> {
        self.changes(id).ok_or_else(|| match self.entries.iter().find(|entry| entry.id == id) {
            Some(KeymapEntry { error: Some(error), .. }) => {
                KeymapError::Broken { id: id.to_owned(), error: error.clone() }
            }
            _ => KeymapError::Unknown(id.to_owned()),
        })
    }

    fn refresh(&mut self) {
        self.active = self.keymap(&self.current).unwrap_or_else(Keymap::default_macos);
        self.generation += 1;
    }

    fn free_id(&self, name: &str) -> String {
        let base = id_from_name(name);
        let taken = |id: &str| {
            id == MACOS_KEYMAP
                || self.entries.iter().any(|entry| entry.id == id)
                || self.dir().is_some_and(|dir| dir.join(format!("{id}.json")).exists())
        };
        let mut id = base.clone();
        let mut suffix = 1;
        while taken(&id) {
            suffix += 1;
            id = format!("{base}-{suffix}");
        }
        id
    }

    fn write_file(&self, file: &KeymapFile) -> Result<(), KeymapError> {
        let dir = self.dir().ok_or(KeymapError::NoFolder)?;
        let mut text = serde_json::to_string_pretty(file).unwrap_or_default();
        text.push('\n');
        write(&dir.join(format!("{}.json", file.id)), &text)
    }

    fn scan(&mut self) {
        self.builtin = self
            .data
            .as_ref()
            .and_then(|data| std::fs::read(data.join("keymap.json")).ok())
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, Vec<String>>>(&bytes).ok())
            .map(|saved| from_text(&saved))
            .unwrap_or_default();
        self.abbreviations = self
            .data
            .as_ref()
            .and_then(|data| std::fs::read(data.join("abbreviations.json")).ok())
            .and_then(|bytes| serde_json::from_slice::<BTreeMap<String, Vec<String>>>(&bytes).ok())
            .unwrap_or_default();
        self.entries = vec![KeymapEntry {
            id: macos(),
            name: t!("workbench-keymap-macos"),
            source: KeymapSource::Builtin,
            path: None,
            imported_from: None,
            error: None,
        }];
        self.files.clear();
        let Some(dir) = self.dir() else { return };
        let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|entries| entries.filter_map(Result::ok).map(|entry| entry.path()).collect())
            .unwrap_or_default();
        paths.retain(|path| {
            let name = path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default();
            !name.starts_with('.') && name.ends_with(".json") && path.is_file()
        });
        paths.sort();
        for path in paths {
            let id = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let mut entry = KeymapEntry {
                id: id.clone(),
                name: id.clone(),
                source: KeymapSource::User,
                path: Some(path.clone()),
                imported_from: None,
                error: None,
            };
            let parsed = std::fs::read_to_string(&path)
                .map_err(|error| error.to_string())
                .and_then(|text| serde_json::from_str::<KeymapFile>(&text).map_err(|error| error.to_string()));
            match parsed {
                Err(error) => entry.error = Some(error),
                Ok(file) if file.format_version != FORMAT_VERSION => {
                    entry.error = Some(t!("workbench-keymap-error-version", version = file.format_version));
                }
                Ok(file) if !is_file_name(&id) || id == MACOS_KEYMAP => {
                    entry.name = file.name;
                    entry.error = Some(t!("workbench-keymap-error-bad-id", id = id.as_str()));
                }
                Ok(mut file) => {
                    entry.name.clone_from(&file.name);
                    entry.imported_from.clone_from(&file.source);
                    file.id.clone_from(&id);
                    self.files.insert(id, file);
                }
            }
            self.entries.push(entry);
        }
        self.entries[1..].sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.id.clone()));
    }
}

fn write(path: &Path, text: &str) -> Result<(), KeymapError> {
    let failed = |error: String| KeymapError::Write { path: path.to_owned(), error };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| failed(error.to_string()))?;
    }
    zen_vfs::write_atomic(path, text.as_bytes()).map_err(|error| failed(error.to_string()))
}

fn to_text(changes: &BTreeMap<String, Vec<Keystroke>>) -> BTreeMap<String, Vec<String>> {
    changes.iter().map(|(action, keys)| (action.clone(), keys.iter().map(ToString::to_string).collect())).collect()
}

fn from_text(saved: &BTreeMap<String, Vec<String>>) -> BTreeMap<String, Vec<Keystroke>> {
    saved
        .iter()
        .map(|(action, keys)| (action.clone(), keys.iter().filter_map(|key| Keystroke::parse(key)).collect()))
        .collect()
}

pub fn id_from_name(name: &str) -> String {
    let mut id = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    while id.ends_with('-') {
        id.pop();
    }
    id.truncate(64);
    if id.is_empty() { "keymap".to_owned() } else { id }
}

fn is_file_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The parameters of `ui.keymap.select` and `ui.keymap.delete`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct IdParams {
    /// The id of the keymap, from `ui.keymap.list`.
    pub id: String,
}

/// The parameters of `ui.keymap.import`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportParams {
    /// The keymap file, an absolute path: a keymap `.xml` exported from a JetBrains IDE.
    pub path: PathBuf,
    /// Whether to use the new keymap at once.
    #[serde(default)]
    pub select: bool,
}

/// The parameters of `ui.keymap.rename` and `ui.keymap.duplicate`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NameParams {
    /// The id of the keymap, from `ui.keymap.list`.
    pub id: String,
    /// The new name.
    pub name: String,
}

/// The parameters of `ui.keymap.setShortcut`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutParams {
    /// The action, as `find_action` or `get_keymap` names it (`editor.line.duplicate`), or a command id.
    pub action: String,
    /// Its keystrokes written as on macOS (`⇧⌘D`); an empty list removes them, none brings back the defaults.
    #[serde(default)]
    pub keystrokes: Option<Vec<String>>,
}

/// What `ui.keymap.setShortcut` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutSet {
    /// The keymap changed, the one in use.
    pub keymap: String,
    /// The action.
    pub action: String,
    /// Its keystrokes now.
    pub keystrokes: Vec<String>,
}

/// The parameters of `ui.keymap.addAbbreviation` and `ui.keymap.removeAbbreviation`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AbbreviationParams {
    /// The action, as `find_action` or `get_keymap` names it (`editor.line.duplicate`), or a command id.
    pub action: String,
    /// The abbreviation, e.g. `dl`.
    pub abbreviation: String,
}

/// What the abbreviation commands answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Abbreviations {
    /// The abbreviations of every action that has any, by action.
    pub abbreviations: BTreeMap<String, Vec<String>>,
}

pub fn lock(store: &SharedKeymaps) -> MutexGuard<'_, KeymapStore> {
    store.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub fn list(store: &KeymapStore) -> KeymapList {
    KeymapList { current: store.current().to_owned(), keymaps: store.entries().to_vec() }
}

pub fn select(store: &mut KeymapStore, id: &str) -> Result<(), CommandError> {
    store.select(id).map_err(|error| message(&error))
}

pub fn import(store: &mut KeymapStore, params: &ImportParams) -> Result<KeymapImported, CommandError> {
    let imported = store.import(&params.path).map_err(|error| message(&error))?;
    if params.select {
        select(store, &imported.id)?;
    }
    Ok(imported)
}

pub fn message(error: &KeymapError) -> CommandError {
    CommandError::new(match error {
        KeymapError::Unknown(id) => t!("workbench-keymap-error-unknown", id = id.as_str()),
        KeymapError::Builtin => t!("workbench-keymap-error-builtin"),
        KeymapError::Broken { id, error } => {
            t!("workbench-keymap-error-broken", id = id.as_str(), error = error.as_str())
        }
        KeymapError::EmptyName => t!("workbench-keymap-error-empty-name"),
        KeymapError::EmptyAbbreviation => t!("workbench-keymap-error-empty-abbreviation"),
        KeymapError::NoFolder => t!("workbench-keymap-error-no-folder"),
        KeymapError::Read { path, error } => {
            t!("workbench-keymap-error-read", path = path.display().to_string(), error = error.as_str())
        }
        KeymapError::UnknownFormat(path) => t!("workbench-keymap-error-format", path = path.display().to_string()),
        KeymapError::Import { path, error } => {
            t!("workbench-keymap-error-import", path = path.display().to_string(), error = error.as_str())
        }
        KeymapError::Write { path, error } => {
            t!("workbench-keymap-error-write", path = path.display().to_string(), error = error.as_str())
        }
    })
}

pub struct ListKeymaps(SharedKeymaps);

impl Command for ListKeymaps {
    type Params = NoParams;
    type Output = KeymapList;
    const ID: &'static str = "ui.keymap.list";
    const TITLE: &'static str = "Keymaps";
    const DESCRIPTION: &'static str = "Lists the keymaps (id, name, source builtin or user, the file, the file it was imported from and why it cannot be used) and the id of the one in use.";

    fn run(&self, _session: &mut Session, _params: NoParams) -> Result<KeymapList, CommandError> {
        Ok(list(&lock(&self.0)))
    }
}

pub struct SelectKeymap(SharedKeymaps);

impl Command for SelectKeymap {
    type Params = IdParams;
    type Output = NoParams;
    const ID: &'static str = "ui.keymap.select";
    const TITLE: &'static str = "Select Keymap";
    const DESCRIPTION: &'static str = "Uses a keymap in every window at once and remembers the choice.";

    fn run(&self, _session: &mut Session, params: IdParams) -> Result<NoParams, CommandError> {
        select(&mut lock(&self.0), &params.id)?;
        Ok(NoParams {})
    }
}

pub struct ImportKeymap(SharedKeymaps);

impl Command for ImportKeymap {
    type Params = ImportParams;
    type Output = KeymapImported;
    const ID: &'static str = "ui.keymap.import";
    const TITLE: &'static str = "Import Keymap";
    const DESCRIPTION: &'static str = "Converts a JetBrains keymap .xml into a named keymap of the keymaps folder, a taken id getting -2, -3 and so on; \
         returns its id, name, parent, shortcuts, how many actions were mapped, the entries that were not (actionId, reason, shortcut) and warnings. \
         With select, uses it at once.";

    fn run(&self, _session: &mut Session, params: ImportParams) -> Result<KeymapImported, CommandError> {
        import(&mut lock(&self.0), &params)
    }
}

pub struct DuplicateKeymap(SharedKeymaps);

impl Command for DuplicateKeymap {
    type Params = NameParams;
    type Output = KeymapCreated;
    const ID: &'static str = "ui.keymap.duplicate";
    const TITLE: &'static str = "Duplicate Keymap";
    const DESCRIPTION: &'static str =
        "Copies a keymap with its changes into a new named keymap called name; returns the new id and name.";

    fn run(&self, _session: &mut Session, params: NameParams) -> Result<KeymapCreated, CommandError> {
        lock(&self.0).duplicate(&params.id, &params.name).map_err(|error| message(&error))
    }
}

pub struct RenameKeymap(SharedKeymaps);

impl Command for RenameKeymap {
    type Params = NameParams;
    type Output = NoParams;
    const ID: &'static str = "ui.keymap.rename";
    const TITLE: &'static str = "Rename Keymap";
    const DESCRIPTION: &'static str =
        "Gives a named keymap a new name; its id stays. The built-in keymap cannot be renamed.";

    fn run(&self, _session: &mut Session, params: NameParams) -> Result<NoParams, CommandError> {
        lock(&self.0).rename(&params.id, &params.name).map_err(|error| message(&error))?;
        Ok(NoParams {})
    }
}

pub struct DeleteKeymap(SharedKeymaps);

impl Command for DeleteKeymap {
    type Params = IdParams;
    type Output = NoParams;
    const ID: &'static str = "ui.keymap.delete";
    const TITLE: &'static str = "Delete Keymap";
    const DESCRIPTION: &'static str = "Deletes a named keymap and its file; the macOS keymap takes over if it was in use. The built-in keymap cannot be deleted.";

    fn run(&self, _session: &mut Session, params: IdParams) -> Result<NoParams, CommandError> {
        lock(&self.0).delete(&params.id).map_err(|error| message(&error))?;
        Ok(NoParams {})
    }
}

pub struct ReloadKeymaps(SharedKeymaps);

impl Command for ReloadKeymaps {
    type Params = NoParams;
    type Output = NoParams;
    const ID: &'static str = "ui.keymap.reload";
    const TITLE: &'static str = "Reload Keymaps";
    const DESCRIPTION: &'static str =
        "Reads keymap.json and the keymaps folder again; a keymap in use that is gone gives way to macOS.";

    fn run(&self, _session: &mut Session, _params: NoParams) -> Result<NoParams, CommandError> {
        lock(&self.0).reload();
        Ok(NoParams {})
    }
}

pub struct SetShortcut(SharedKeymaps);

impl Command for SetShortcut {
    type Params = ShortcutParams;
    type Output = ShortcutSet;
    const ID: &'static str = "ui.keymap.setShortcut";
    const TITLE: &'static str = "Set Keyboard Shortcut";
    const DESCRIPTION: &'static str = "Sets an action's keystrokes in the keymap in use and saves them, as Add Keyboard \
         Shortcut in Settings → Keymap or ⌥⏎ on an action in Find Action does; an empty list removes them, null brings \
         back the defaults. A command that has no shortcut of its own runs from the new one as Find Action runs it.";

    fn run(&self, _session: &mut Session, params: ShortcutParams) -> Result<ShortcutSet, CommandError> {
        let action = params.action.trim();
        if action.is_empty() {
            return Err(CommandError::with_code("noAction", "an action is required"));
        }
        let keystrokes = match params.keystrokes {
            Some(texts) => Some(
                texts
                    .iter()
                    .map(|text| {
                        Keystroke::parse(text).ok_or_else(|| {
                            CommandError::with_code("badKeystroke", format!("`{text}` is not a keystroke, e.g. ⇧⌘D"))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            None => None,
        };
        let mut store = lock(&self.0);
        let mut changes = store.active().user_changes().clone();
        match keystrokes {
            Some(keys) => {
                changes.insert(action.to_owned(), keys);
            }
            None => {
                changes.remove(action);
            }
        }
        store.set_changes(changes).map_err(|error| message(&error))?;
        let keystrokes = store.active().keystrokes(action).iter().map(ToString::to_string).collect();
        Ok(ShortcutSet { keymap: store.current().to_owned(), action: action.to_owned(), keystrokes })
    }
}

pub struct ListAbbreviations(SharedKeymaps);

impl Command for ListAbbreviations {
    type Params = NoParams;
    type Output = Abbreviations;
    const ID: &'static str = "ui.keymap.abbreviations";
    const TITLE: &'static str = "Action Abbreviations";
    const DESCRIPTION: &'static str = "Lists the abbreviations of the actions (Settings → Keymap → Add Abbreviation), by \
         action: Find Action and Search Everywhere list an action first when the query is one of its abbreviations or \
         the start of one. They belong to no keymap.";

    fn run(&self, _session: &mut Session, _params: NoParams) -> Result<Abbreviations, CommandError> {
        Ok(Abbreviations { abbreviations: lock(&self.0).abbreviations().clone() })
    }
}

pub struct AddAbbreviation(SharedKeymaps);

impl Command for AddAbbreviation {
    type Params = AbbreviationParams;
    type Output = Abbreviations;
    const ID: &'static str = "ui.keymap.addAbbreviation";
    const TITLE: &'static str = "Add Abbreviation";
    const DESCRIPTION: &'static str = "Gives an action an abbreviation that Find Action matches, as Add Abbreviation in \
         Settings → Keymap does; saved at once in abbreviations.json. Answers with every abbreviation.";

    fn run(&self, _session: &mut Session, params: AbbreviationParams) -> Result<Abbreviations, CommandError> {
        let mut store = lock(&self.0);
        store.add_abbreviation(&params.action, &params.abbreviation).map_err(|error| message(&error))?;
        Ok(Abbreviations { abbreviations: store.abbreviations().clone() })
    }
}

pub struct RemoveAbbreviation(SharedKeymaps);

impl Command for RemoveAbbreviation {
    type Params = AbbreviationParams;
    type Output = Abbreviations;
    const ID: &'static str = "ui.keymap.removeAbbreviation";
    const TITLE: &'static str = "Remove Abbreviation";
    const DESCRIPTION: &'static str =
        "Takes an abbreviation from an action and saves the rest. Answers with every abbreviation.";

    fn run(&self, _session: &mut Session, params: AbbreviationParams) -> Result<Abbreviations, CommandError> {
        let mut store = lock(&self.0);
        store.remove_abbreviation(&params.action, &params.abbreviation).map_err(|error| message(&error))?;
        Ok(Abbreviations { abbreviations: store.abbreviations().clone() })
    }
}

pub fn register(registry: &mut Registry, store: &SharedKeymaps) -> Result<(), RegistryError> {
    registry.register(ListKeymaps(Arc::clone(store)))?;
    registry.register(SelectKeymap(Arc::clone(store)))?;
    registry.register(ImportKeymap(Arc::clone(store)))?;
    registry.register(DuplicateKeymap(Arc::clone(store)))?;
    registry.register(RenameKeymap(Arc::clone(store)))?;
    registry.register(DeleteKeymap(Arc::clone(store)))?;
    registry.register(ReloadKeymaps(Arc::clone(store)))?;
    registry.register(SetShortcut(Arc::clone(store)))?;
    registry.register(ListAbbreviations(Arc::clone(store)))?;
    registry.register(AddAbbreviation(Arc::clone(store)))?;
    registry.register(RemoveAbbreviation(Arc::clone(store)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zen_core::keymap::{Unmapped, UnmappedReason};

    use super::*;

    struct FakeImporter;

    impl KeymapImporter for FakeImporter {
        fn detects(&self, text: &str) -> bool {
            text.starts_with("<fake")
        }

        fn import(&self, text: &str) -> Result<KeymapImport, String> {
            if text.contains("broken") {
                return Err("broken on purpose".to_owned());
            }
            let name = text.split('"').nth(1).unwrap_or_default().to_owned();
            let shortcuts =
                [("editor.line.duplicate".to_owned(), vec![Keystroke::parse("⇧⌘D").unwrap_or_default()])].into();
            Ok(KeymapImport {
                name,
                parent: Some("Fake".to_owned()),
                shortcuts,
                mapped: 1,
                unmapped: vec![Unmapped {
                    action_id: "Nope".to_owned(),
                    reason: UnmappedReason::UnknownAction,
                    shortcut: None,
                }],
                warnings: Vec::new(),
            })
        }
    }

    fn setup(data: &Path) -> (Registry, SharedKeymaps, Session) {
        zen_i18n::register(crate::LOCALES);
        let store = Arc::new(Mutex::new(KeymapStore::new(Some(data.to_owned()), vec![Box::new(FakeImporter)])));
        let mut registry = Registry::new();
        register(&mut registry, &store).unwrap_or_else(|error| panic!("{error}"));
        (registry, store, Session::new())
    }

    fn source(dir: &Path, name: &str, text: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap_or_else(|error| panic!("{error}"));
        path
    }

    fn ids(registry: &Registry, session: &mut Session) -> Vec<String> {
        let listed = registry.dispatch(session, "ui.keymap.list", json!({})).unwrap_or_else(|error| panic!("{error}"));
        listed["keymaps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|keymap| keymap["id"].as_str().map(str::to_owned))
            .collect()
    }

    #[test]
    fn imports_lists_selects_renames_and_deletes() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let data = dir.path().join("data");
        let (registry, store, mut session) = setup(&data);
        let listed =
            registry.dispatch(&mut session, "ui.keymap.list", json!({})).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            listed,
            json!({ "current": "macos", "keymaps": [{ "id": "macos", "name": "macOS", "source": "builtin" }] })
        );

        let file = source(dir.path(), "mine.xml", r#"<fake name="My Keys!">"#);
        let imported = registry
            .dispatch(&mut session, "ui.keymap.import", json!({ "path": file }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(imported["id"], "my-keys");
        assert_eq!(imported["name"], "My Keys!");
        assert_eq!(imported["mapped"], 1);
        assert_eq!(imported["unmapped"], json!([{ "actionId": "Nope", "reason": "unknownAction" }]));
        assert_eq!(lock(&store).current(), "macos", "not selected without select");
        let written: Value =
            serde_json::from_slice(&std::fs::read(data.join("keymaps/my-keys.json")).unwrap_or_default())
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(written["formatVersion"], 1);
        assert_eq!(written["parent"], "macos");
        assert_eq!(written["shortcuts"], json!({ "editor.line.duplicate": ["⇧⌘D"] }));
        assert_eq!(written["source"], file.display().to_string());

        let again = registry
            .dispatch(&mut session, "ui.keymap.import", json!({ "path": file, "select": true }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(again["id"], "my-keys-2");
        assert_eq!(lock(&store).current(), "my-keys-2");
        assert_eq!(lock(&store).active().shortcut("editor.line.duplicate").as_deref(), Some("⇧⌘D"));

        registry
            .dispatch(&mut session, "ui.keymap.rename", json!({ "id": "my-keys-2", "name": "Renamed" }))
            .unwrap_or_else(|error| panic!("{error}"));
        let listed = list(&lock(&store));
        assert_eq!(listed.keymaps[2].name, "Renamed");
        assert_eq!(listed.keymaps[2].id, "my-keys-2", "the id stays");

        let error = registry.dispatch(&mut session, "ui.keymap.delete", json!({ "id": "macos" })).unwrap_err();
        assert!(error.to_string().contains(&t!("workbench-keymap-error-builtin")), "{error}");
        let error =
            registry.dispatch(&mut session, "ui.keymap.rename", json!({ "id": "macos", "name": "X" })).unwrap_err();
        assert!(error.to_string().contains(&t!("workbench-keymap-error-builtin")), "{error}");
        let error =
            registry.dispatch(&mut session, "ui.keymap.rename", json!({ "id": "my-keys", "name": "  " })).unwrap_err();
        assert!(error.to_string().contains(&t!("workbench-keymap-error-empty-name")), "{error}");

        registry
            .dispatch(&mut session, "ui.keymap.delete", json!({ "id": "my-keys-2" }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(lock(&store).current(), "macos", "the macOS keymap takes over");
        assert!(!data.join("keymaps/my-keys-2.json").exists());
        assert_eq!(ids(&registry, &mut session), ["macos", "my-keys"]);

        registry
            .dispatch(&mut session, "ui.keymap.select", json!({ "id": "my-keys" }))
            .unwrap_or_else(|error| panic!("{error}"));
        let duplicated = registry
            .dispatch(&mut session, "ui.keymap.duplicate", json!({ "id": "my-keys", "name": "My Keys" }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(duplicated, json!({ "id": "my-keys-2", "name": "My Keys" }));
        assert_eq!(
            lock(&store).keymap("my-keys-2").and_then(|keymap| keymap.shortcut("editor.line.duplicate")).as_deref(),
            Some("⇧⌘D")
        );
    }

    #[test]
    fn edits_go_to_the_file_of_the_keymap_in_use() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let data = dir.path().to_owned();
        std::fs::write(data.join("keymap.json"), r#"{"editor.line.delete": ["⌥⌘K", "bad⌘"]}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        let (_registry, store, _session) = setup(&data);
        let mut store = lock(&store);
        assert_eq!(store.active().keystrokes("editor.line.delete"), [Keystroke::parse("⌥⌘K").unwrap_or_default()]);
        let file = source(&data, "a.xml", r#"<fake name="A">"#);
        let imported = store.import(&file).unwrap_or_else(|error| panic!("{error}"));
        store.select(&imported.id).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(store.active().shortcut("editor.line.delete").as_deref(), Some("⌘⌫"), "named keymaps change macOS");

        let generation = store.generation();
        let change = BTreeMap::from([("file.document.save".to_owned(), Vec::new())]);
        store.set_changes(change.clone()).unwrap_or_else(|error| panic!("{error}"));
        assert!(store.generation() > generation);
        assert!(store.active().keystrokes("file.document.save").is_empty());
        let named: KeymapFile = serde_json::from_slice(&std::fs::read(data.join("keymaps/a.json")).unwrap_or_default())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(named.shortcuts, BTreeMap::from([("file.document.save".to_owned(), Vec::new())]));
        let builtin: Value = serde_json::from_slice(&std::fs::read(data.join("keymap.json")).unwrap_or_default())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(builtin, json!({ "editor.line.delete": ["⌥⌘K", "bad⌘"] }), "keymap.json is not touched");

        store.select(MACOS_KEYMAP).unwrap_or_else(|error| panic!("{error}"));
        store.set_changes(change).unwrap_or_else(|error| panic!("{error}"));
        let builtin: Value = serde_json::from_slice(&std::fs::read(data.join("keymap.json")).unwrap_or_default())
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(builtin, json!({ "file.document.save": [] }));
    }

    #[test]
    fn ids_never_leave_the_keymaps_folder() {
        assert_eq!(id_from_name("../../etc/passwd"), "etc-passwd");
        assert_eq!(id_from_name("Мой"), "keymap");
        assert_eq!(id_from_name("  Fixture  Keys  "), "fixture-keys");
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let data = dir.path().join("data");
        let (registry, store, mut session) = setup(&data);
        let file = source(dir.path(), "evil.xml", r#"<fake name="../../evil">"#);
        let imported = registry
            .dispatch(&mut session, "ui.keymap.import", json!({ "path": file }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(imported["id"], "evil");
        assert!(data.join("keymaps/evil.json").is_file());
        for id in ["../keymap", "../../evil", "/etc/passwd", ""] {
            assert!(registry.dispatch(&mut session, "ui.keymap.select", json!({ "id": id })).is_err(), "{id}");
            assert!(registry.dispatch(&mut session, "ui.keymap.delete", json!({ "id": id })).is_err(), "{id}");
        }
        assert!(data.join("keymaps/evil.json").is_file());
        let named = lock(&store).dir().unwrap_or_default().join("macos.json");
        std::fs::write(&named, r#"{"formatVersion":1,"id":"macos","name":"Fake","shortcuts":{}}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        lock(&store).reload();
        let listed = list(&lock(&store));
        let fake = listed.keymaps.iter().find(|entry| entry.path.as_deref() == Some(named.as_path()));
        assert!(fake.is_some_and(|entry| entry.error.is_some()), "{listed:?}");
    }

    #[test]
    fn broken_files_and_import_errors_are_messages() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let data = dir.path().join("data");
        std::fs::create_dir_all(data.join("keymaps")).unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(data.join("keymaps/broken.json"), "{").unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(data.join("keymaps/future.json"), r#"{"formatVersion":2,"id":"future","name":"F"}"#)
            .unwrap_or_else(|error| panic!("{error}"));
        std::fs::write(data.join("keymaps/.hidden.json"), "{").unwrap_or_else(|error| panic!("{error}"));
        let (registry, _store, mut session) = setup(&data);
        assert_eq!(ids(&registry, &mut session), ["macos", "broken", "future"]);
        let error = registry.dispatch(&mut session, "ui.keymap.select", json!({ "id": "broken" })).unwrap_err();
        assert!(error.to_string().contains("broken"), "{error}");
        let error = registry.dispatch(&mut session, "ui.keymap.select", json!({ "id": "nope" })).unwrap_err();
        assert!(error.to_string().contains("nope"), "{error}");
        let missing = dir.path().join("missing.xml");
        let error = registry.dispatch(&mut session, "ui.keymap.import", json!({ "path": missing })).unwrap_err();
        assert!(error.to_string().contains("missing.xml"), "{error}");
        let other = source(dir.path(), "other.xml", "<scheme/>");
        let error = registry.dispatch(&mut session, "ui.keymap.import", json!({ "path": other })).unwrap_err();
        assert!(error.to_string().contains("JetBrains"), "{error}");
        let broken = source(dir.path(), "broken.xml", "<fake broken>");
        let error = registry.dispatch(&mut session, "ui.keymap.import", json!({ "path": broken })).unwrap_err();
        assert!(error.to_string().contains("broken on purpose"), "{error}");
        registry
            .dispatch(&mut session, "ui.keymap.delete", json!({ "id": "broken" }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(ids(&registry, &mut session), ["macos", "future"]);
        let empty = source(dir.path(), "Unnamed.xml", r#"<fake name="">"#);
        let imported = registry
            .dispatch(&mut session, "ui.keymap.import", json!({ "path": empty }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(imported["name"], "Unnamed", "a keymap without a name takes the file's");
    }

    #[test]
    fn a_broken_keymap_in_use_stays_chosen_and_a_removed_one_gives_way() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let data = dir.path().join("data");
        let (registry, store, mut session) = setup(&data);
        let file = source(dir.path(), "a.xml", r#"<fake name="A">"#);
        let params = json!({ "path": file, "select": true });
        registry.dispatch(&mut session, "ui.keymap.import", params).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(lock(&store).current(), "a");
        std::fs::write(data.join("keymaps/a.json"), "{").unwrap_or_else(|error| panic!("{error}"));
        registry.dispatch(&mut session, "ui.keymap.reload", json!({})).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(lock(&store).current(), "a", "a typo in the file does not lose the choice");
        assert!(lock(&store).keymap("a").is_none(), "the macOS keymap works in its place");
        std::fs::remove_file(data.join("keymaps/a.json")).unwrap_or_else(|error| panic!("{error}"));
        registry.dispatch(&mut session, "ui.keymap.reload", json!({})).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(lock(&store).current(), MACOS_KEYMAP);
    }

    #[test]
    fn shortcuts_are_set_for_actions_and_commands_and_abbreviations_are_kept() {
        let dir = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let (registry, store, mut session) = setup(dir.path());
        let mut run = |id: &str, params: Value| registry.dispatch(&mut session, id, params);
        let set = run("ui.keymap.setShortcut", json!({ "action": "editor.line.delete", "keystrokes": ["⌥⌘K"] }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(set, json!({ "keymap": "macos", "action": "editor.line.delete", "keystrokes": ["⌥⌘K"] }));
        let set = run("ui.keymap.setShortcut", json!({ "action": "vcs.remote.fetchAll", "keystrokes": ["⌃⌥F"] }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(set["keystrokes"], json!(["⌃⌥F"]));
        let stroke = Keystroke::parse("⌃⌥F").unwrap_or_else(|| panic!("a keystroke"));
        let binding = lock(&store).active().lookup(&stroke, zen_core::keymap::Context::Global);
        assert_eq!(binding.map(|binding| binding.command).as_deref(), Some("vcs.remote.fetchAll"));
        let saved = std::fs::read_to_string(dir.path().join("keymap.json")).unwrap_or_default();
        assert!(saved.contains("vcs.remote.fetchAll") && saved.contains("⌥⌘K"), "{saved}");
        let error = run("ui.keymap.setShortcut", json!({ "action": "editor.line.delete", "keystrokes": ["nope"] }))
            .unwrap_err();
        assert!(error.to_string().contains("nope"), "{error}");
        run("ui.keymap.setShortcut", json!({ "action": "editor.line.delete" }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(lock(&store).active().shortcut("editor.line.delete").as_deref(), Some("⌘⌫"));

        let added =
            run("ui.keymap.addAbbreviation", json!({ "action": "editor.line.duplicate", "abbreviation": " dl " }))
                .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(added["abbreviations"], json!({ "editor.line.duplicate": ["dl"] }));
        run("ui.keymap.addAbbreviation", json!({ "action": "editor.line.duplicate", "abbreviation": "dl" }))
            .unwrap_or_else(|error| panic!("{error}"));
        run("ui.keymap.addAbbreviation", json!({ "action": "editor.line.duplicate", "abbreviation": "dup" }))
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(
            run("ui.keymap.addAbbreviation", json!({ "action": "editor.line.duplicate", "abbreviation": "  " }))
                .is_err()
        );
        let (registry2, _store2, mut session2) = setup(dir.path());
        let listed = registry2
            .dispatch(&mut session2, "ui.keymap.abbreviations", json!({}))
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(listed["abbreviations"], json!({ "editor.line.duplicate": ["dl", "dup"] }), "kept in the file");
        let removed = registry2
            .dispatch(
                &mut session2,
                "ui.keymap.removeAbbreviation",
                json!({ "action": "editor.line.duplicate", "abbreviation": "dl" }),
            )
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(removed["abbreviations"], json!({ "editor.line.duplicate": ["dup"] }));
    }
}

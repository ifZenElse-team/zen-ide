use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::format::id_from_name;
use crate::zen_dark::{BUILTIN_IDS, ZEN_DARK_ID, base_id, builtin, zen_dark};
use crate::zen_light::ZEN_LIGHT_ID;
use crate::{ImportReport, Resolution, Theme, ThemeImporter, jsonc, make_readable, parse, to_json};

pub trait ThemeFiles: Send + Sync {
    fn list(&self, dir: &Path) -> Result<Vec<PathBuf>, String>;

    fn read(&self, path: &Path) -> Result<String, String>;

    fn write(&self, path: &Path, text: &str) -> Result<(), String>;

    fn exists(&self, path: &Path) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeSource {
    Builtin,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThemeEntry {
    pub id: String,
    pub name: String,
    pub dark: bool,
    pub source: ThemeSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub id: String,
    pub name: String,
    pub mapped: usize,
    pub unmapped: Vec<String>,
    pub adjusted: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    pub follow_system: bool,
    pub system_dark: bool,
    pub dark: String,
    pub light: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    UnknownTheme(String),
    Broken { id: String, error: String },
    NoFolder,
    Read { path: PathBuf, error: String },
    UnknownFormat(PathBuf),
    Import { path: PathBuf, error: String },
    Write { path: PathBuf, error: String },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::UnknownTheme(id) => write!(f, "no theme has the id `{id}`"),
            StoreError::Broken { id, error } => write!(f, "the theme `{id}` cannot be used: {error}"),
            StoreError::NoFolder => f.write_str("there is no themes folder"),
            StoreError::Read { path, error } => write!(f, "cannot read {}: {error}", path.display()),
            StoreError::UnknownFormat(path) => write!(f, "{} is not a theme of a known format", path.display()),
            StoreError::Import { path, error } => write!(f, "cannot import {}: {error}", path.display()),
            StoreError::Write { path, error } => write!(f, "cannot write {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for StoreError {}

pub struct ThemeStore {
    dir: Option<PathBuf>,
    files: Box<dyn ThemeFiles>,
    importers: Vec<Box<dyn ThemeImporter>>,
    entries: Vec<ThemeEntry>,
    themes: BTreeMap<String, Theme>,
    current: String,
    chosen: String,
    follow_system: bool,
    system_dark: bool,
    preferred_dark: String,
    preferred_light: String,
    generation: u64,
}

impl ThemeStore {
    pub fn new(dir: Option<PathBuf>, files: Box<dyn ThemeFiles>, importers: Vec<Box<dyn ThemeImporter>>) -> ThemeStore {
        let mut store = ThemeStore {
            dir,
            files,
            importers,
            entries: Vec::new(),
            themes: BTreeMap::new(),
            current: ZEN_DARK_ID.to_owned(),
            chosen: ZEN_DARK_ID.to_owned(),
            follow_system: false,
            system_dark: true,
            preferred_dark: ZEN_DARK_ID.to_owned(),
            preferred_light: ZEN_LIGHT_ID.to_owned(),
            generation: 0,
        };
        store.scan();
        store
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub fn entries(&self) -> &[ThemeEntry] {
        &self.entries
    }

    pub fn current(&self) -> &str {
        &self.current
    }

    pub fn chosen(&self) -> &str {
        &self.chosen
    }

    pub fn appearance(&self) -> Appearance {
        Appearance {
            follow_system: self.follow_system,
            system_dark: self.system_dark,
            dark: self.preferred_dark.clone(),
            light: self.preferred_light.clone(),
        }
    }

    pub fn set_follow_system(
        &mut self,
        follow: bool,
        dark: Option<&str>,
        light: Option<&str>,
    ) -> Result<(), StoreError> {
        for id in [dark, light].into_iter().flatten() {
            self.usable(id)?;
        }
        if follow && !self.follow_system {
            let current = self.theme(&self.current).is_none_or(|theme| theme.dark);
            let slot = if current { &mut self.preferred_dark } else { &mut self.preferred_light };
            slot.clone_from(&self.current);
        }
        if let Some(dark) = dark {
            dark.clone_into(&mut self.preferred_dark);
        }
        if let Some(light) = light {
            light.clone_into(&mut self.preferred_light);
        }
        if self.follow_system != follow {
            self.follow_system = follow;
            self.generation += 1;
        }
        self.settle();
        Ok(())
    }

    pub fn set_system_dark(&mut self, dark: bool) {
        if self.system_dark != dark {
            self.system_dark = dark;
            self.settle();
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn theme(&self, id: &str) -> Option<Theme> {
        builtin(id).or_else(|| self.themes.get(id).cloned())
    }

    pub fn resolve(&self, id: &str) -> Option<Resolution> {
        let theme = self.theme(id)?;
        Some(theme.resolve(|parent| self.themes.get(parent).cloned()))
    }

    pub fn resolve_current(&self) -> Resolution {
        self.resolve(&self.current).unwrap_or_else(|| zen_dark().resolve(|_| None))
    }

    pub fn select(&mut self, id: &str) -> Result<(), StoreError> {
        let theme = self.usable(id)?;
        id.clone_into(&mut self.chosen);
        if self.follow_system {
            let slot = if theme.dark { &mut self.preferred_dark } else { &mut self.preferred_light };
            id.clone_into(slot);
        }
        self.settle();
        Ok(())
    }

    fn usable(&self, id: &str) -> Result<Theme, StoreError> {
        self.theme(id).ok_or_else(|| match self.entries.iter().find(|entry| entry.id == id) {
            Some(ThemeEntry { error: Some(error), .. }) => {
                StoreError::Broken { id: id.to_owned(), error: error.clone() }
            }
            _ => StoreError::UnknownTheme(id.to_owned()),
        })
    }

    fn settle(&mut self) {
        let wanted = match (self.follow_system, self.system_dark) {
            (false, _) => &self.chosen,
            (true, true) => &self.preferred_dark,
            (true, false) => &self.preferred_light,
        };
        if self.current != *wanted {
            self.current.clone_from(wanted);
            self.generation += 1;
        }
    }

    pub fn reload(&mut self) {
        self.scan();
        let gone = |store: &ThemeStore, id: &str| store.theme(id).is_none();
        if gone(self, &self.chosen) {
            let dark = self.entries.iter().find(|entry| entry.id == self.chosen).is_none_or(|entry| entry.dark);
            self.chosen = base_id(dark).to_owned();
        }
        if gone(self, &self.preferred_dark) {
            self.preferred_dark = ZEN_DARK_ID.to_owned();
        }
        if gone(self, &self.preferred_light) {
            self.preferred_light = ZEN_LIGHT_ID.to_owned();
        }
        let before = self.generation;
        self.settle();
        if self.generation == before {
            self.generation += 1;
        }
    }

    pub fn import(&mut self, path: &Path) -> Result<Imported, StoreError> {
        self.dir.as_ref().ok_or(StoreError::NoFolder)?;
        let text = self.files.read(path).map_err(|error| StoreError::Read { path: path.to_owned(), error })?;
        self.import_text(path, &text)
    }

    pub fn import_text(&mut self, path: &Path, text: &str) -> Result<Imported, StoreError> {
        let dir = self.dir.clone().ok_or(StoreError::NoFolder)?;
        let (report, foreign) = self.convert(path, text)?;
        let ImportReport { mut theme, mapped, unmapped } = report;
        let adjusted =
            if foreign { make_readable(&mut theme, |parent| self.themes.get(parent).cloned()) } else { Vec::new() };
        let base = if is_file_name(&theme.id) { theme.id.clone() } else { id_from_name(&theme.name) };
        let mut id = base.clone();
        let mut suffix = 1;
        while self.taken(&dir, &id) {
            suffix += 1;
            id = format!("{base}-{suffix}");
        }
        theme.id.clone_from(&id);
        theme.unknown_keys.clear();
        let target = dir.join(format!("{id}.json"));
        self.files.write(&target, &to_json(&theme)).map_err(|error| StoreError::Write { path: target, error })?;
        self.scan();
        self.generation += 1;
        Ok(Imported { id, name: theme.name, mapped, unmapped, adjusted })
    }

    fn convert(&self, path: &Path, text: &str) -> Result<(ImportReport, bool), StoreError> {
        let failed = |error: String| StoreError::Import { path: path.to_owned(), error };
        let ours = matches!(serde_json::from_str::<Value>(&jsonc::strip(text)), Ok(Value::Object(root)) if root.contains_key("formatVersion"));
        if ours {
            let theme = parse(text).map_err(|error| failed(error.to_string()))?;
            let mapped = theme.ui.len() + theme.editor.len() + theme.syntax.len() + theme.terminal.len();
            let unmapped = theme.unknown_keys.clone();
            return Ok((ImportReport { theme, mapped, unmapped }, false));
        }
        let name = path.to_string_lossy();
        let read = |other: &str| self.files.read(Path::new(other)).ok();
        let importer = self
            .importers
            .iter()
            .find(|importer| importer.detects_at(&name, text))
            .ok_or_else(|| StoreError::UnknownFormat(path.to_owned()))?;
        let report = importer.import_at(&name, text, &read).map_err(|error| failed(error.to_string()))?;
        Ok((report, true))
    }

    fn taken(&self, dir: &Path, id: &str) -> bool {
        builtin(id).is_some()
            || self.entries.iter().any(|entry| entry.id == id)
            || self.files.exists(&dir.join(format!("{id}.json")))
    }

    fn scan(&mut self) {
        self.entries = BUILTIN_IDS
            .iter()
            .filter_map(|id| builtin(id))
            .map(|theme| ThemeEntry {
                id: theme.id,
                name: theme.name,
                dark: theme.dark,
                source: ThemeSource::Builtin,
                path: None,
                error: None,
            })
            .collect();
        self.themes.clear();
        let Some(dir) = &self.dir else { return };
        let mut paths = self.files.list(dir).unwrap_or_default();
        paths.sort();
        for path in paths {
            let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
            let mut entry = ThemeEntry {
                id: stem.clone(),
                name: stem,
                dark: true,
                source: ThemeSource::User,
                path: Some(path.clone()),
                error: None,
            };
            let parsed = self.files.read(&path).and_then(|text| parse(&text).map_err(|error| error.to_string()));
            match parsed {
                Err(error) => entry.error = Some(error),
                Ok(theme) => {
                    entry.id.clone_from(&theme.id);
                    entry.name.clone_from(&theme.name);
                    entry.dark = theme.dark;
                    let first = self.entries.iter().find(|other| other.id == theme.id && other.error.is_none());
                    if builtin(&theme.id).is_some() {
                        entry.error = Some(format!("the id `{}` belongs to a built-in theme", theme.id));
                    } else if let Some(first) = first {
                        let first = first.path.as_deref().map(|path| path.display().to_string()).unwrap_or_default();
                        entry.error = Some(format!("the id `{}` is taken by {first}", theme.id));
                    } else {
                        self.themes.insert(theme.id.clone(), theme);
                    }
                }
            }
            self.entries.push(entry);
        }
    }
}

fn is_file_name(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

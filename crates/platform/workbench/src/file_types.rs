use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vfs::file_types::{self, Changes, FileType, Mapping};

use crate::Workbench;

pub const FILE_NAME: &str = "filetypes.json";

pub const OVERRIDES_FILE_NAME: &str = "overridefiletypes.json";

pub const TEXT: &str = file_types::TEXT;

pub const UNKNOWN_FILE_TYPE: &str = "unknownFileType";

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListFileTypes)?;
    registry.register(TypeOfFile)?;
    registry.register(AddPattern)?;
    registry.register(RemovePattern)?;
    registry.register(AddHashbang)?;
    registry.register(RemoveHashbang)?;
    registry.register(SetIgnored)?;
    registry.register(ResetFileTypes)?;
    registry.register(Associate)?;
    registry.register(OverrideFileType)?;
    registry.register(RevertOverride)?;
    registry.register(ListOverrides)?;
    Ok(())
}

pub fn platform_types() -> Vec<FileType> {
    let service = |id: &str, name: &str| FileType {
        id: id.to_owned(),
        name: name.to_owned(),
        localized: true,
        patterns: Vec::new(),
        hashbangs: Vec::new(),
        contents: Vec::new(),
    };
    vec![
        FileType {
            id: TEXT.to_owned(),
            name: "file-type-text".to_owned(),
            localized: true,
            patterns: vec!["*.txt".to_owned(), "*.text".to_owned()],
            hashbangs: Vec::new(),
            contents: Vec::new(),
        },
        service(file_types::DETECTED, "file-type-detected"),
        service(file_types::NATIVE, "file-type-native"),
    ]
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct SavedOverrides {
    files: Vec<SavedOverride>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SavedOverride {
    path: PathBuf,
    #[serde(rename = "type")]
    type_id: String,
}

impl SavedOverrides {
    fn read(bytes: &[u8]) -> BTreeMap<PathBuf, String> {
        let Ok(serde_json::Value::Object(json)) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return BTreeMap::new();
        };
        let Some(serde_json::Value::Array(items)) = json.get("files") else { return BTreeMap::new() };
        items
            .iter()
            .filter_map(|item| serde_json::from_value::<SavedOverride>(item.clone()).ok())
            .filter(|saved| saved.path.is_absolute() && !saved.type_id.trim().is_empty())
            .map(|saved| (saved.path, saved.type_id))
            .collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    patterns: Vec<SavedMapping>,
    removed_patterns: Vec<SavedMapping>,
    hashbangs: Vec<SavedMapping>,
    removed_hashbangs: Vec<SavedMapping>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ignored: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SavedMapping {
    #[serde(rename = "type")]
    type_id: String,
    pattern: String,
}

fn to_saved(list: &[Mapping]) -> Vec<SavedMapping> {
    list.iter()
        .map(|mapping| SavedMapping { type_id: mapping.type_id.clone(), pattern: mapping.pattern.clone() })
        .collect()
}

fn from_saved(list: Vec<SavedMapping>) -> Vec<Mapping> {
    list.into_iter().map(|mapping| Mapping { type_id: mapping.type_id, pattern: mapping.pattern }).collect()
}

impl Saved {
    fn read(bytes: &[u8]) -> Self {
        let Ok(serde_json::Value::Object(json)) = serde_json::from_slice::<serde_json::Value>(bytes) else {
            return Self::default();
        };
        let mappings = |key: &str| -> Vec<SavedMapping> {
            let Some(serde_json::Value::Array(items)) = json.get(key) else { return Vec::new() };
            items
                .iter()
                .filter_map(|item| serde_json::from_value::<SavedMapping>(item.clone()).ok())
                .filter(|mapping| usable(&mapping.pattern) && !mapping.type_id.trim().is_empty())
                .collect()
        };
        let ignored = match json.get("ignored") {
            Some(serde_json::Value::Array(items)) => Some(
                items
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .filter(|name| usable(name))
                    .map(str::to_owned)
                    .collect(),
            ),
            _ => None,
        };
        Self {
            patterns: mappings("patterns"),
            removed_patterns: mappings("removedPatterns"),
            hashbangs: mappings("hashbangs"),
            removed_hashbangs: mappings("removedHashbangs"),
            ignored,
        }
    }
}

fn usable(pattern: &str) -> bool {
    !pattern.trim().is_empty() && !pattern.contains('/')
}

impl From<&Changes> for Saved {
    fn from(changes: &Changes) -> Self {
        Self {
            patterns: to_saved(&changes.added),
            removed_patterns: to_saved(&changes.removed),
            hashbangs: to_saved(&changes.added_hashbangs),
            removed_hashbangs: to_saved(&changes.removed_hashbangs),
            ignored: changes.ignored.clone(),
        }
    }
}

impl From<Saved> for Changes {
    fn from(saved: Saved) -> Self {
        Self {
            added: from_saved(saved.patterns),
            removed: from_saved(saved.removed_patterns),
            added_hashbangs: from_saved(saved.hashbangs),
            removed_hashbangs: from_saved(saved.removed_hashbangs),
            ignored: saved.ignored,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct FileTypeStore {
    file: Option<PathBuf>,
    overrides: Option<PathBuf>,
    seen: u64,
}

impl FileTypeStore {
    pub fn load(file: Option<PathBuf>) -> Self {
        file_types::declare(platform_types());
        let saved = file
            .as_deref()
            .and_then(|file| std::fs::read(file).ok())
            .map(|bytes| Saved::read(&bytes))
            .unwrap_or_default();
        let changes = Changes::from(saved);
        if !changes.is_empty() {
            file_types::set_changes(changes);
        }
        let overrides = file.as_deref().map(|file| file.with_file_name(OVERRIDES_FILE_NAME));
        let own = overrides
            .as_deref()
            .and_then(|file| std::fs::read(file).ok())
            .map(|bytes| SavedOverrides::read(&bytes))
            .unwrap_or_default();
        if !own.is_empty() {
            file_types::set_overrides(own);
        }
        Self { file, overrides, seen: file_types::generation() }
    }

    fn save_overrides(&self, overrides: &BTreeMap<PathBuf, String>) -> Result<(), CommandError> {
        let Some(file) = &self.overrides else { return Ok(()) };
        if overrides.is_empty() {
            let _ = std::fs::remove_file(file);
            return Ok(());
        }
        let files = overrides
            .iter()
            .map(|(path, type_id)| SavedOverride { path: path.clone(), type_id: type_id.clone() })
            .collect();
        let mut json = serde_json::to_vec_pretty(&SavedOverrides { files }).unwrap_or_default();
        json.push(b'\n');
        zen_vfs::write_atomic(file, &json).map_err(|error| CommandError::new(error.to_string()))
    }

    fn save(&self, changes: &Changes) -> Result<(), CommandError> {
        let Some(file) = &self.file else { return Ok(()) };
        if changes.is_empty() {
            let _ = std::fs::remove_file(file);
            return Ok(());
        }
        let mut json = serde_json::to_vec_pretty(&Saved::from(changes)).unwrap_or_default();
        json.push(b'\n');
        zen_vfs::write_atomic(file, &json).map_err(|error| CommandError::new(error.to_string()))
    }
}

pub fn follow(session: &mut Session) {
    let generation = file_types::generation();
    let store = session.state_mut::<FileTypeStore>();
    if store.seen == generation {
        return;
    }
    store.seen = generation;
    crate::index_project(session);
}

fn change(session: &mut Session, edit: impl FnOnce(&mut file_types::Registry)) -> Result<(), CommandError> {
    let mut registry = (*file_types::current()).clone();
    edit(&mut registry);
    let changes = registry.changes().clone();
    file_types::set_changes(changes.clone());
    session.state::<FileTypeStore>().cloned().unwrap_or_default().save(&changes)?;
    follow(session);
    Ok(())
}

pub fn type_name(file_type: &FileType) -> String {
    if file_type.localized { t!(&file_type.name) } else { file_type.name.clone() }
}

/// One file type as the commands describe it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileTypeInfo {
    /// The stable id, e.g. `go` or a plug-in's id.
    pub id: String,
    /// The name shown, e.g. `Go`.
    pub name: String,
    /// The file name patterns it has now, e.g. `*.go`.
    pub patterns: Vec<String>,
    /// The hashbangs it has now, e.g. `bash`.
    pub hashbangs: Vec<String>,
    /// What the first bytes of its files hold, for files no pattern names: each marker as text,
    /// e.g. `<?xml` or `<!DOCTYPE (not <!DOCTYPE html)` (read-only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contents: Vec<String>,
    /// Whether the user changed its patterns or hashbangs.
    pub changed: bool,
}

/// What `editor.fileTypes.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileTypes {
    /// The types, in the order they were declared.
    pub types: Vec<FileTypeInfo>,
    /// The names of the files and folders the IDE ignores.
    pub ignored: Vec<String>,
    /// Whether the ignored names are the defaults.
    pub ignored_default: bool,
}

pub fn list() -> FileTypes {
    let registry = file_types::current();
    let types = registry
        .types()
        .into_iter()
        .map(|ty| FileTypeInfo {
            id: ty.declared.id.clone(),
            name: type_name(&ty.declared),
            changed: ty.patterns != ty.declared.patterns || ty.hashbangs != ty.declared.hashbangs,
            contents: ty.declared.contents.iter().map(describe_marker).collect(),
            patterns: ty.patterns,
            hashbangs: ty.hashbangs,
        })
        .collect();
    FileTypes { types, ignored: registry.ignored(), ignored_default: registry.changes().ignored.is_none() }
}

pub fn describe_marker(marker: &file_types::ContentMarker) -> String {
    let mut text = marker.starts.clone();
    if let Some(word) = &marker.first_tag {
        text.push_str(&format!(" … {word}"));
    }
    if !marker.unless.is_empty() {
        text.push_str(&format!(" (not {})", marker.unless.join(", ")));
    }
    text
}

pub struct ListFileTypes;

impl Command for ListFileTypes {
    type Params = NoParams;
    type Output = FileTypes;
    const ID: &'static str = "editor.fileTypes.list";
    const TITLE: &'static str = "File Types";
    const DESCRIPTION: &'static str = "Lists the file types (Settings → Editor → File Types) with their file name \
         patterns, hashbangs and content markers (`contents`, read-only: what the first bytes of a file no pattern \
         names hold, as XML's `<?xml`), and the names of the files and folders the IDE ignores.";

    fn run(&self, _session: &mut Session, _params: NoParams) -> Result<FileTypes, CommandError> {
        Ok(list())
    }
}

/// A path parameter.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct TypeOfParams {
    /// The file: absolute, or relative to the project.
    pub path: PathBuf,
}

/// What `editor.fileTypes.typeOf` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileTypeOf {
    /// The type's id; `None` when no type names the file.
    pub id: Option<String>,
    /// The type's name.
    pub name: Option<String>,
    /// Whether the type comes from the hashbang of the first line.
    pub by_hashbang: bool,
    /// Whether the type comes from what the file's first bytes hold (its content markers).
    pub by_content: bool,
    /// Whether the user gave the file this type itself (Override File Type).
    pub overridden: bool,
    /// Whether the editor highlights the file.
    pub highlighted: bool,
    /// The language server that serves the file, when one does.
    pub language_server: Option<String>,
    /// Whether the file or a folder above it in the project is ignored.
    pub ignored: bool,
}

fn head_of(session: &Session, path: &Path) -> Option<String> {
    let open = session.state::<Workbench>().and_then(|workbench| {
        let document = workbench.documents.iter().find(|document| document.path() == path)?;
        Some(file_types::head(document.text()).to_owned())
    });
    open.or_else(|| file_types::disk_head(path).map(|head| head.to_string()))
}

pub struct TypeOfFile;

impl Command for TypeOfFile {
    type Params = TypeOfParams;
    type Output = FileTypeOf;
    const ID: &'static str = "editor.fileTypes.typeOf";
    const TITLE: &'static str = "File Type of a File";
    const DESCRIPTION: &'static str = "The file type of a file: the one the user gave it (overridden), else by \
         its name patterns, else by the hashbang of its first line (byHashbang), else by what its first bytes hold \
         (byContent, e.g. XML's <?xml); and whether the IDE ignores it.";

    fn run(&self, session: &mut Session, params: TypeOfParams) -> Result<FileTypeOf, CommandError> {
        let path = match session.state::<Workbench>().and_then(|workbench| workbench.project.clone()) {
            Some(project) if params.path.is_relative() => project.join(&params.path),
            _ => params.path.clone(),
        };
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let registry = file_types::current();
        let own = registry.overridden(&path);
        let by_name = own.clone().or_else(|| registry.type_of(&name));
        let head = head_of(session, &path);
        let by_hashbang = if by_name.is_none() {
            head.as_deref().and_then(|head| registry.type_by_hashbang(file_types::first_line(head)))
        } else {
            None
        };
        let by_content = if by_name.is_none() && by_hashbang.is_none() {
            head.as_deref().and_then(|head| registry.type_by_content(head))
        } else {
            None
        };
        let known = registry.known_name_at(&path, head.as_deref());
        let highlighted = known.as_deref().is_some_and(|known| {
            session
                .state::<crate::brackets::Highlighters>()
                .and_then(|highlighters| highlighters.for_name(known))
                .is_some()
        });
        let language_server = session
            .state::<crate::language::Languages>()
            .and_then(|languages| languages.spec_for(&path))
            .map(|entry| entry.id().to_owned());
        let project = session.state::<Workbench>().and_then(|workbench| workbench.project.clone());
        let inside = project.as_deref().and_then(|project| path.strip_prefix(project).ok()).unwrap_or(Path::new(&name));
        let ignored = inside.components().any(|part| registry.is_ignored(&part.as_os_str().to_string_lossy()));
        let found = by_name.as_ref().or(by_hashbang.as_ref()).or(by_content.as_ref());
        Ok(FileTypeOf {
            id: found.map(|ty| ty.id.clone()),
            name: found.map(type_name),
            by_hashbang: by_hashbang.is_some(),
            by_content: by_content.is_some(),
            overridden: own.is_some(),
            highlighted,
            language_server,
            ignored,
        })
    }
}

/// A pattern or a hashbang of a type.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PatternParams {
    /// The type's id, as `editor.fileTypes.list` gives it.
    #[serde(rename = "type")]
    pub type_id: String,
    /// The pattern (`*.gotmpl`, `Jenkinsfile`) or the hashbang (`bash`).
    pub pattern: String,
}

/// What adding a pattern or a hashbang answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    /// The type the pattern was taken from, when another type had it.
    pub moved_from: Option<String>,
    /// That type's name.
    pub moved_from_name: Option<String>,
}

fn checked(params: &PatternParams) -> Result<String, CommandError> {
    if file_types::current().get(&params.type_id).is_none() {
        return Err(CommandError::new(t!("workbench-error-file-type-unknown", id = params.type_id.as_str())));
    }
    let pattern = params.pattern.trim();
    if pattern.is_empty() || pattern.contains('/') {
        return Err(CommandError::new(t!("workbench-error-file-type-pattern", pattern = params.pattern.as_str())));
    }
    Ok(pattern.to_owned())
}

pub struct AddPattern;

impl Command for AddPattern {
    type Params = PatternParams;
    type Output = Added;
    const ID: &'static str = "editor.fileTypes.addPattern";
    const TITLE: &'static str = "Add File Name Pattern";
    const DESCRIPTION: &'static str = "Gives a file type a file name pattern (`*` for any characters, `?` for one); \
         another type that had the pattern loses it (movedFrom). Open files change their highlighting and code \
         style at once; kept per IDE.";

    fn run(&self, session: &mut Session, params: PatternParams) -> Result<Added, CommandError> {
        let pattern = checked(&params)?;
        let mut moved = None;
        change(session, |registry| moved = registry.add_pattern(&params.type_id, &pattern))?;
        Ok(Added { moved_from_name: moved.as_ref().map(type_name), moved_from: moved.map(|ty| ty.id) })
    }
}

pub struct RemovePattern;

impl Command for RemovePattern {
    type Params = PatternParams;
    type Output = ();
    const ID: &'static str = "editor.fileTypes.removePattern";
    const TITLE: &'static str = "Remove File Name Pattern";
    const DESCRIPTION: &'static str = "Takes a file name pattern from a file type; its files get the type of another \
         pattern that names them, or none.";

    fn run(&self, session: &mut Session, params: PatternParams) -> Result<(), CommandError> {
        let pattern = checked(&params)?;
        change(session, |registry| registry.remove_pattern(&params.type_id, &pattern))
    }
}

pub struct AddHashbang;

impl Command for AddHashbang {
    type Params = PatternParams;
    type Output = Added;
    const ID: &'static str = "editor.fileTypes.addHashbang";
    const TITLE: &'static str = "Add HashBang Pattern";
    const DESCRIPTION: &'static str = "Gives a file type a hashbang: the interpreter of the first line (`bash` for \
         `#!/usr/bin/env bash`), which decides the type of a file no name pattern names.";

    fn run(&self, session: &mut Session, params: PatternParams) -> Result<Added, CommandError> {
        let pattern = checked(&params)?;
        let mut moved = None;
        change(session, |registry| moved = registry.add_hashbang(&params.type_id, &pattern))?;
        Ok(Added { moved_from_name: moved.as_ref().map(type_name), moved_from: moved.map(|ty| ty.id) })
    }
}

pub struct RemoveHashbang;

impl Command for RemoveHashbang {
    type Params = PatternParams;
    type Output = ();
    const ID: &'static str = "editor.fileTypes.removeHashbang";
    const TITLE: &'static str = "Remove HashBang Pattern";
    const DESCRIPTION: &'static str = "Takes a hashbang from a file type.";

    fn run(&self, session: &mut Session, params: PatternParams) -> Result<(), CommandError> {
        let pattern = checked(&params)?;
        change(session, |registry| registry.remove_hashbang(&params.type_id, &pattern))
    }
}

/// The parameters of `editor.fileTypes.setIgnored`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct IgnoredParams {
    /// The names, `*` for any characters and `?` for one, e.g. `.git`, `*.pyc`.
    pub patterns: Vec<String>,
}

pub struct SetIgnored;

impl Command for SetIgnored {
    type Params = IgnoredParams;
    type Output = FileTypes;
    const ID: &'static str = "editor.fileTypes.setIgnored";
    const TITLE: &'static str = "Set Ignored Files and Folders";
    const DESCRIPTION: &'static str = "Replaces the names of the files and folders the IDE ignores: they are not \
         shown in the Project tool window and not indexed, so Find in Files and Search Everywhere skip them. The \
         project is indexed again in the background; kept per IDE.";

    fn run(&self, session: &mut Session, params: IgnoredParams) -> Result<FileTypes, CommandError> {
        if let Some(bad) = params.patterns.iter().find(|pattern| pattern.contains('/')) {
            return Err(CommandError::new(t!("workbench-error-file-type-pattern", pattern = bad.as_str())));
        }
        change(session, |registry| registry.set_ignored(params.patterns))?;
        Ok(list())
    }
}

pub struct ResetFileTypes;

impl Command for ResetFileTypes {
    type Params = NoParams;
    type Output = FileTypes;
    const ID: &'static str = "editor.fileTypes.reset";
    const TITLE: &'static str = "Reset File Types";
    const DESCRIPTION: &'static str =
        "Gives every file type the patterns and hashbangs it declares and restores the default ignored names.";

    fn run(&self, session: &mut Session, _params: NoParams) -> Result<FileTypes, CommandError> {
        change(session, |registry| {
            *registry = file_types::Registry::new(registry.declared().to_vec(), Changes::default())
        })?;
        Ok(list())
    }
}

pub type Open = dyn Fn(&Path) -> Result<(), String> + Send + Sync;

#[derive(Clone)]
pub struct SystemOpener(Arc<Open>);

impl std::fmt::Debug for SystemOpener {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SystemOpener")
    }
}

impl Default for SystemOpener {
    fn default() -> Self {
        Self::system()
    }
}

impl SystemOpener {
    pub fn new(open: impl Fn(&Path) -> Result<(), String> + Send + Sync + 'static) -> Self {
        Self(Arc::new(open))
    }

    pub fn system() -> Self {
        Self::new(|path| {
            let program = match std::env::var_os("ZEN_SYSTEM_OPENER") {
                Some(program) => program,
                None if cfg!(test) => return Err("no system opener in tests".to_owned()),
                None if cfg!(target_os = "macos") => "open".into(),
                None if cfg!(target_os = "windows") => "explorer".into(),
                None => "xdg-open".into(),
            };
            let path = std::path::absolute(path).map_err(|error| error.to_string())?;
            #[cfg(not(target_arch = "wasm32"))]
            std::process::Command::new(&program)
                .arg(&path)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map_err(|error| error.to_string())?;
            #[cfg(target_arch = "wasm32")]
            let _ = (program, path);
            Ok(())
        })
    }

    pub fn open(&self, path: &Path) -> Result<(), String> {
        (self.0)(path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Opening {
    Application,
    Image,
    Text { typed: bool },
}

pub(crate) const HEAD: usize = 8192;

fn head(path: &Path) -> Vec<u8> {
    use std::io::Read;
    let mut head = Vec::with_capacity(HEAD);
    if let Ok(file) = std::fs::File::open(path) {
        let _ = file.take(HEAD as u64).read_to_end(&mut head);
    }
    head
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Utf16 {
    Little,
    Big,
}

impl Utf16 {
    pub fn charset(self) -> &'static str {
        match self {
            Self::Little => "UTF-16LE",
            Self::Big => "UTF-16BE",
        }
    }

    pub fn decode(self, bytes: &[u8]) -> String {
        let bytes = match (self, bytes) {
            (Self::Little, [0xff, 0xfe, rest @ ..]) | (Self::Big, [0xfe, 0xff, rest @ ..]) => rest,
            _ => bytes,
        };
        let units = bytes.as_chunks::<2>().0.iter().map(|pair| match self {
            Self::Little => u16::from_le_bytes(*pair),
            Self::Big => u16::from_be_bytes(*pair),
        });
        char::decode_utf16(units).map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER)).collect()
    }
}

pub fn utf16_of(head: &[u8]) -> Option<Utf16> {
    match head {
        [0xff, 0xfe, ..] => return Some(Utf16::Little),
        [0xfe, 0xff, ..] => return Some(Utf16::Big),
        _ => {}
    }
    let pairs = head.as_chunks::<2>().0;
    let count = pairs.len();
    if count < 2 {
        return None;
    }
    let (mut low, mut high) = (0usize, 0usize);
    for pair in pairs {
        match (pair[0], pair[1]) {
            (0, 0) => return None,
            (_, 0) => low += 1,
            (0, _) => high += 1,
            _ => {}
        }
    }
    let zeros = head.iter().filter(|byte| **byte == 0).count();
    match (low, high) {
        (low, 0) if low * 2 >= count && low == zeros => Some(Utf16::Little),
        (0, high) if high * 2 >= count && high == zeros => Some(Utf16::Big),
        _ => None,
    }
}

pub fn is_binary(head: &[u8]) -> bool {
    head.contains(&0) && utf16_of(head).is_none()
}

fn opening_of(file_type: Option<&FileType>) -> Option<Opening> {
    match file_type?.id.as_str() {
        file_types::NATIVE => Some(Opening::Application),
        _ => Some(Opening::Text { typed: true }),
    }
}

pub(crate) fn opening(path: &Path) -> Result<Opening, CommandError> {
    if std::fs::metadata(path).is_ok_and(|metadata| !metadata.is_file()) {
        let file = path.file_name().map_or_else(|| crate::display(path), |name| name.to_string_lossy().into_owned());
        return Err(CommandError::new(t!("workbench-error-not-a-regular-file", file = file.as_str())));
    }
    let registry = file_types::current();
    if let Some(own) = opening_of(registry.overridden(path).as_ref()) {
        return Ok(own);
    }
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let by_name = registry.type_of(&name);
    if by_name.as_ref().is_some_and(|ty| ty.id == file_types::NATIVE) {
        return Ok(Opening::Application);
    }
    if zen_image::opens_as_picture(path) {
        return Ok(Opening::Image);
    }
    if let Some(typed) = opening_of(by_name.as_ref()) {
        return Ok(typed);
    }
    let head = head(path);
    let line = file_types::first_line(&String::from_utf8_lossy(&head)).to_owned();
    if let Some(typed) = opening_of(registry.type_by_hashbang(&line).as_ref()) {
        return Ok(typed);
    }
    if is_binary(&head) {
        return Err(CommandError::with_code(
            UNKNOWN_FILE_TYPE,
            t!("workbench-error-unknown-file-type", file = name.as_str()),
        )
        .with_data(serde_json::json!({ "path": path, "patterns": suggested_patterns(&name) })));
    }
    Ok(Opening::Text { typed: false })
}

pub fn suggested_patterns(name: &str) -> Vec<String> {
    match name.rsplit_once('.').filter(|(stem, extension)| !stem.is_empty() && !extension.is_empty()) {
        Some((_, extension)) => vec![format!("*.{extension}"), name.to_owned()],
        None => vec![name.to_owned()],
    }
}

#[cfg(unix)]
fn is_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_program(_path: &Path) -> bool {
    false
}

pub(crate) fn open_in_application(session: &mut Session, path: &Path) -> Result<(), CommandError> {
    let opener = session.state::<SystemOpener>().cloned().unwrap_or_default();
    let file = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    if is_program(path) {
        return Err(CommandError::new(t!("workbench-error-program-in-application", file = file.as_str())));
    }
    opener.open(path).map_err(|error| {
        CommandError::new(t!("workbench-error-open-application", file = file.as_str(), error = error.as_str()))
    })?;
    session.state_mut::<Workbench>().messages.push(t!("workbench-opened-in-application", file = file.as_str()));
    Ok(())
}

/// The parameters of `editor.fileTypes.associate`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AssociateParams {
    /// The pattern: the file's name, or `*.<extension>`.
    pub pattern: String,
    /// The type's id as `editor.fileTypes.list` gives it: `detected` opens matching files as text when
    /// their content is text, `native` opens them in their application, any other id as that type.
    #[serde(rename = "type")]
    pub type_id: String,
    /// A file to open after, as the type now says.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

/// What `editor.fileTypes.associate` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Associated {
    /// The type the pattern was taken from, when another type had it.
    pub moved_from: Option<String>,
    /// That type's name.
    pub moved_from_name: Option<String>,
    /// Where the file was opened: `editor` or `application`; `None` without a file.
    pub opened: Option<String>,
}

pub struct Associate;

impl Command for Associate {
    type Params = AssociateParams;
    type Output = Associated;
    const ID: &'static str = "editor.fileTypes.associate";
    const TITLE: &'static str = "Associate with File Type…";
    const DESCRIPTION: &'static str = "Gives a file name pattern (`name.ext` or `*.ext`) a way to open: type \
         `detected` opens matching files as text when their content is text, `native` opens them in the system's \
         application for them, any other type id opens them as that type. With `path`, opens that file after. \
         Kept per IDE; the same as Settings → Editor → File Types.";

    fn run(&self, session: &mut Session, params: AssociateParams) -> Result<Associated, CommandError> {
        let pattern = checked(&PatternParams { type_id: params.type_id.clone(), pattern: params.pattern.clone() })?;
        let mut moved = None;
        change(session, |registry| moved = registry.add_pattern(&params.type_id, &pattern))?;
        let mut opened = None;
        if let Some(path) = &params.path {
            let path = session.state::<Workbench>().map_or_else(|| path.clone(), |workbench| workbench.resolve(path));
            let application = matches!(opening(&path)?, Opening::Application);
            crate::open_file(session, &path, true)?;
            opened = Some(if application { "application" } else { "editor" }.to_owned());
        }
        Ok(Associated { moved_from_name: moved.as_ref().map(type_name), moved_from: moved.map(|ty| ty.id), opened })
    }
}

/// The files the user gave a type of their own.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileOverride {
    /// The file.
    pub path: PathBuf,
    /// The type's id.
    #[serde(rename = "type")]
    pub type_id: String,
    /// The type's name; `None` while no module or plug-in declares the type.
    pub name: Option<String>,
}

pub fn overrides() -> Vec<FileOverride> {
    let registry = file_types::current();
    registry
        .overrides()
        .iter()
        .map(|(path, type_id)| FileOverride {
            path: path.clone(),
            type_id: type_id.clone(),
            name: registry.overridden(path).as_ref().map(type_name),
        })
        .collect()
}

/// The parameters of `editor.fileTypes.override`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OverrideParams {
    /// The files: absolute, or relative to the project.
    pub paths: Vec<PathBuf>,
    /// The type's id, as `editor.fileTypes.list` gives it, e.g. `text`.
    #[serde(rename = "type")]
    pub type_id: String,
}

/// The parameters of `editor.fileTypes.revertOverride`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct PathsParams {
    /// The files: absolute, or relative to the project.
    pub paths: Vec<PathBuf>,
}

fn resolved(session: &Session, paths: &[PathBuf]) -> Vec<PathBuf> {
    let workbench = session.state::<Workbench>();
    paths.iter().map(|path| workbench.map_or_else(|| path.clone(), |workbench| workbench.resolve(path))).collect()
}

fn change_overrides(
    session: &mut Session,
    edit: impl FnOnce(&mut BTreeMap<PathBuf, String>),
) -> Result<Vec<FileOverride>, CommandError> {
    let mut own = file_types::current().overrides().clone();
    edit(&mut own);
    file_types::set_overrides(own.clone());
    session.state::<FileTypeStore>().cloned().unwrap_or_default().save_overrides(&own)?;
    follow(session);
    Ok(overrides())
}

pub(crate) fn relocate_overrides(session: &mut Session, from: &Path, to: Option<&Path>) {
    let own = file_types::current().overrides().clone();
    if !own.keys().any(|path| path.starts_with(from)) {
        return;
    }
    let moved = own
        .into_iter()
        .filter_map(|(path, type_id)| match path.strip_prefix(from) {
            Ok(rest) => to.map(|to| (if rest.as_os_str().is_empty() { to.to_owned() } else { to.join(rest) }, type_id)),
            Err(_) => Some((path, type_id)),
        })
        .collect::<BTreeMap<_, _>>();
    file_types::set_overrides(moved.clone());
    if let Err(error) = session.state::<FileTypeStore>().cloned().unwrap_or_default().save_overrides(&moved) {
        session.state_mut::<Workbench>().messages.push(error.message);
    }
    follow(session);
}

pub struct OverrideFileType;

impl Command for OverrideFileType {
    type Params = OverrideParams;
    type Output = Vec<FileOverride>;
    const ID: &'static str = "editor.fileTypes.override";
    const TITLE: &'static str = "Override File Type";
    const DESCRIPTION: &'static str = "Gives the files (not their pattern) a file type of their own: other files \
         with the same name pattern keep theirs. Type `text` gives plain text, with no highlighting and no language \
         server. Open files change at once; kept per IDE by path. Answers every override.";

    fn run(&self, session: &mut Session, params: OverrideParams) -> Result<Vec<FileOverride>, CommandError> {
        if file_types::current().get(&params.type_id).is_none() {
            return Err(CommandError::new(t!("workbench-error-file-type-unknown", id = params.type_id.as_str())));
        }
        if params.type_id == file_types::DETECTED || params.type_id == file_types::NATIVE {
            return Err(CommandError::new(t!("workbench-error-file-type-override")));
        }
        let paths = resolved(session, &params.paths);
        if let Some(folder) = paths.iter().find(|path| path.is_dir()) {
            return Err(CommandError::new(t!("workbench-error-override-folder", path = crate::display(folder))));
        }
        change_overrides(session, |own| {
            for path in paths {
                own.insert(path, params.type_id.clone());
            }
        })
    }
}

pub struct RevertOverride;

impl Command for RevertOverride {
    type Params = PathsParams;
    type Output = Vec<FileOverride>;
    const ID: &'static str = "editor.fileTypes.revertOverride";
    const TITLE: &'static str = "Revert File Type Override";
    const DESCRIPTION: &'static str = "Takes the file type the user gave the files themselves away: they get the \
         type of their name pattern again. Answers the overrides left.";

    fn run(&self, session: &mut Session, params: PathsParams) -> Result<Vec<FileOverride>, CommandError> {
        let paths = resolved(session, &params.paths);
        change_overrides(session, |own| own.retain(|path, _| !paths.contains(path)))
    }
}

pub struct ListOverrides;

impl Command for ListOverrides {
    type Params = NoParams;
    type Output = Vec<FileOverride>;
    const ID: &'static str = "editor.fileTypes.overrides";
    const TITLE: &'static str = "File Type Overrides";
    const DESCRIPTION: &'static str =
        "Lists the files given a file type of their own (Override File Type), kept per IDE, with the types.";

    fn run(&self, _session: &mut Session, _params: NoParams) -> Result<Vec<FileOverride>, CommandError> {
        Ok(overrides())
    }
}

#[cfg(test)]
mod tests;

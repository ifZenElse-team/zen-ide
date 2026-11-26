use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallKind {
    Go,
    Npm,
    Cargo,
    Rustup,
    Archive,
    Manual,
}

impl InstallKind {
    pub fn tool(self) -> Option<&'static str> {
        match self {
            Self::Go => Some("go"),
            Self::Npm => Some("npm"),
            Self::Cargo => Some("cargo"),
            Self::Rustup => Some("rustup"),
            Self::Archive | Self::Manual => None,
        }
    }

    pub fn download(self) -> Option<&'static str> {
        match self {
            Self::Go => Some("https://go.dev/dl/"),
            Self::Npm => Some("https://nodejs.org/en/download"),
            Self::Cargo | Self::Rustup => Some("https://rustup.rs/"),
            Self::Archive | Self::Manual => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotInstallable {
    Manual(String),
    ToolMissing(InstallKind),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecipe {
    pub kind: InstallKind,
    #[serde(default)]
    pub package: String,
    #[serde(default)]
    pub hint: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub platforms: BTreeMap<String, ArchiveFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<ExtraFile>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveFile {
    pub url: String,
    pub sha256: String,
}

pub fn platform() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

impl InstallRecipe {
    pub fn archive_for(&self, platform: &str) -> Option<(&str, &str)> {
        match self.platforms.get(platform) {
            Some(file) => Some((file.url.as_str(), file.sha256.as_str())),
            None if !self.url.is_empty() && !self.sha256.is_empty() => Some((self.url.as_str(), self.sha256.as_str())),
            None => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtraFile {
    pub url: String,
    pub sha256: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerDeclaration {
    pub id: String,
    pub name: String,
    pub languages: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub language_ids: BTreeMap<String, String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watches: Vec<String>,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub version_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallRecipe>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub initialization_options: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inlay_hints: Option<InlayHintSettings>,
    #[serde(default = "finds_symbols", skip_serializing_if = "is_true")]
    pub workspace_symbols: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub optimize_imports: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub alongside: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub formats: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_on_save: Option<FixOnSave>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub settings: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_support: Option<SchemaSupport>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_project: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<zen_project::support::ProjectMarkers>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_save: Option<PowerSaveSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<ServerNotice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub virtual_documents: Option<VirtualDocuments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generate: Option<GenerateDeclaration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inspections: Vec<ServerInspection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inspection_settings: Option<InspectionSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suppression: Option<SuppressionDeclaration>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub diagnostics_settle_ms: u64,
}

fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInspection {
    pub id: String,
    pub description: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cleanup: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub off: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuppressionDeclaration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    pub separator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub statement: Option<SuppressionForm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub function: Option<SuppressionForm>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<SuppressionForm>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuppressionForm {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub line_end: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionSettings {
    pub section: String,
    pub path: String,
}

impl InspectionSettings {
    pub fn settings(&self, base: &Value, switches: &BTreeMap<&str, bool>) -> Value {
        let mut settings = if base.is_object() { base.clone() } else { Value::Object(serde_json::Map::new()) };
        for (id, value) in switches {
            set_path(&mut settings, &format!("{}.{id}", self.path), *value);
        }
        settings
    }

    pub fn without(&self, base: &Value) -> Value {
        let mut kept = base.clone();
        if let Value::Object(map) = &mut kept {
            map.remove(&self.path);
        }
        kept
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateDeclaration {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prompts: Vec<PromptGenerator>,
}

impl GenerateDeclaration {
    pub fn offers_kind(&self, kind: &str) -> bool {
        let under =
            |prefix: &str| kind == prefix || kind.strip_prefix(prefix).is_some_and(|rest| rest.starts_with('.'));
        if self.kinds.is_empty() { under("source") } else { self.kinds.iter().any(|prefix| under(prefix)) }
    }

    pub fn prompts_for(&self, command: &str, argument: &Value) -> Vec<&PromptGenerator> {
        self.prompts.iter().filter(|prompt| prompt.command == command && prompt.matches(argument)).collect()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptGenerator {
    pub command: String,
    #[serde(default, rename = "match", skip_serializing_if = "BTreeMap::is_empty")]
    pub matches: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub name: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub action: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub title: BTreeMap<String, String>,
    pub members: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lists: Vec<MemberList>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<PromptOption>,
    pub generate: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_empty: bool,
}

impl PromptGenerator {
    pub fn matches(&self, argument: &Value) -> bool {
        self.matches.iter().all(|(pointer, value)| argument.pointer(pointer) == Some(value))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberList {
    pub id: String,
    pub items: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub when: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub selected: Value,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub value: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptOption {
    pub id: String,
    pub label: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub when: String,
}

pub fn holds(value: &Value, condition: &str) -> bool {
    let (negated, pointer) = condition.strip_prefix('!').map_or((false, condition), |pointer| (true, pointer));
    let set = match value.pointer(pointer) {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Array(list)) => !list.is_empty(),
        Some(Value::String(text)) => !text.is_empty(),
        Some(_) => true,
    };
    set != negated
}

pub fn text_in(texts: &BTreeMap<String, String>, language: &str) -> String {
    let base = language.split(['-', '_']).next().unwrap_or(language);
    [language, base, "en"]
        .iter()
        .find_map(|code| texts.get(*code))
        .or_else(|| texts.values().next())
        .cloned()
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDocuments {
    pub schemes: Vec<String>,
    pub request: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub extension: String,
}

impl VirtualDocuments {
    pub fn serves(&self, uri: &str) -> bool {
        crate::position::scheme(uri).is_some_and(|scheme| self.schemes.iter().any(|known| known == scheme))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerNotice {
    pub text: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
}

impl ServerNotice {
    pub fn text_in(&self, language: &str) -> String {
        text_in(&self.text, language)
    }

    pub fn link(&self) -> Option<&str> {
        self.url.starts_with("https://").then_some(self.url.as_str())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchPlaces {
    pub server_folder: Option<PathBuf>,
    pub java_home: Option<PathBuf>,
    pub workspace_data: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchMissing {
    Server,
    Java(u32),
    Other(String),
}

impl ServerDeclaration {
    pub fn has_placeholders(&self) -> bool {
        self.command.contains("${") || self.args.iter().any(|arg| arg.contains("${"))
    }

    pub fn launch_line(&self, places: &LaunchPlaces) -> Result<(PathBuf, Vec<String>), LaunchMissing> {
        let program = expand_launch(&self.command, places, self.java)?;
        let args = self.args.iter().map(|arg| expand_launch(arg, places, self.java)).collect::<Result<Vec<_>, _>>()?;
        Ok((PathBuf::from(program), args))
    }

    pub fn unpacked_marker(&self, server_folder: &Path) -> Option<PathBuf> {
        let files = self.install.as_ref().map(|recipe| recipe.files.as_slice()).unwrap_or_default();
        if files.iter().any(|file| !server_folder.join(&file.path).is_file()) {
            return None;
        }
        let mut marker = None;
        for text in std::iter::once(&self.command).chain(&self.args) {
            let mut rest = text.as_str();
            while let Some(at) = rest.find(GLOB) {
                let after = &rest[at + GLOB.len()..];
                let end = after.find('}')?;
                let found = glob_in(server_folder, &after[..end])?;
                marker.get_or_insert(found);
                rest = &after[end + 1..];
            }
        }
        marker
    }
}

const GLOB: &str = "${glob:";

pub fn expand_launch_options(options: &Value, places: &LaunchPlaces, java: Option<u32>) -> Value {
    match options {
        Value::String(text) if text.contains("${") => {
            Value::String(expand_launch(text, places, java).unwrap_or_else(|_| text.clone()))
        }
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| expand_launch_options(item, places, java)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields.iter().map(|(name, value)| (name.clone(), expand_launch_options(value, places, java))).collect(),
        ),
        _ => options.clone(),
    }
}

fn expand_launch(text: &str, places: &LaunchPlaces, java: Option<u32>) -> Result<String, LaunchMissing> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find("${") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[at..]);
            return Ok(out);
        };
        let name = &after[..end];
        let path = |path: &Option<PathBuf>, missing: LaunchMissing| {
            path.as_ref().map(|path| path.to_string_lossy().into_owned()).ok_or(missing)
        };
        let value = match name {
            "serverFolder" => path(&places.server_folder, LaunchMissing::Server)?,
            "javaHome" => path(&places.java_home, LaunchMissing::Java(java.unwrap_or(21)))?,
            "workspaceData" => path(&places.workspace_data, LaunchMissing::Other(name.to_owned()))?,
            "os" => os_name().to_owned(),
            "archSuffix" => if cfg!(target_arch = "aarch64") && !cfg!(windows) { "_arm" } else { "" }.to_owned(),
            _ => match name.strip_prefix("glob:") {
                Some(pattern) => {
                    let folder = places.server_folder.as_deref().ok_or(LaunchMissing::Server)?;
                    glob_in(folder, pattern).ok_or(LaunchMissing::Server)?.to_string_lossy().into_owned()
                }
                None => format!("${{{name}}}"),
            },
        };
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

pub fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "mac"
    } else if cfg!(windows) {
        "win"
    } else {
        "linux"
    }
}

pub fn glob_in(folder: &Path, pattern: &str) -> Option<PathBuf> {
    let (parent, name) = pattern.rsplit_once('/').unwrap_or(("", pattern));
    let dir = if parent.is_empty() { folder.to_owned() } else { folder.join(parent) };
    let mut names: Vec<String> =
        std::fs::read_dir(&dir).ok()?.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names.into_iter().find(|candidate| glob(name, candidate)).map(|found| dir.join(found))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixOnSave {
    pub kind: String,
    pub setting: String,
}

fn finds_symbols() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerSaveSettings {
    pub section: String,
    pub settings: Value,
}

impl PowerSaveSettings {
    pub fn over(&self, base: &Value) -> Value {
        let mut settings = if base.is_object() { base.clone() } else { Value::Object(serde_json::Map::new()) };
        merge(&mut settings, &self.settings);
        settings
    }
}

fn merge(target: &mut Value, over: &Value) {
    match (target, over) {
        (Value::Object(target), Value::Object(over)) => {
            for (key, value) in over {
                match target.get_mut(key) {
                    Some(existing) if existing.is_object() && value.is_object() => merge(existing, value),
                    _ => {
                        target.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (target, over) => *target = over.clone(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaSupport {
    pub setting: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub register: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlayHintSettings {
    pub section: String,
    pub kinds: BTreeMap<String, Vec<String>>,
}

impl InlayHintSettings {
    pub fn settings(&self, base: &Value, on: impl Fn(&str) -> bool) -> Value {
        let mut settings = if base.is_object() { base.clone() } else { Value::Object(serde_json::Map::new()) };
        for (kind, paths) in &self.kinds {
            let value = on(kind);
            for path in paths {
                set_path(&mut settings, path, value);
            }
        }
        settings
    }
}

fn set_path(settings: &mut Value, path: &str, value: bool) {
    let mut node = settings;
    let mut names = path.split('.').peekable();
    while let Some(name) = names.next() {
        if !node.is_object() {
            *node = Value::Object(serde_json::Map::new());
        }
        let Some(object) = node.as_object_mut() else { return };
        if names.peek().is_none() {
            object.insert(name.to_owned(), Value::Bool(value));
            return;
        }
        node = object.entry(name.to_owned()).or_insert_with(|| Value::Object(serde_json::Map::new()));
    }
}

impl ServerDeclaration {
    pub fn parse_list(json: &str) -> Result<Vec<Self>, String> {
        serde_json::from_str(json).map_err(|error| format!("the language server list is invalid: {error}"))
    }

    pub fn optimize_imports_kinds(&self) -> Vec<String> {
        if self.optimize_imports.is_empty() {
            vec!["source.organizeImports".to_owned()]
        } else {
            self.optimize_imports.clone()
        }
    }

    pub fn serves(&self, file_name: &str) -> bool {
        file_matches(&self.extensions, &self.names, file_name)
    }

    pub fn hears_of(&self, file_name: &str) -> bool {
        self.serves(file_name) || file_matches(&[], &self.watches, file_name)
    }

    pub fn language_for(&self, file_name: &str) -> String {
        let extension = file_name.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase());
        if let Some(id) = extension.as_ref().and_then(|extension| self.language_ids.get(extension)) {
            return id.clone();
        }
        extension
            .and_then(|extension| self.languages.iter().find(|language| **language == extension).cloned())
            .or_else(|| self.languages.first().cloned())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPlugin {
    pub server: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub package: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<zen_project::support::ProjectMarkers>,
}

impl ServerPlugin {
    pub fn applies(&self, root: &Path) -> bool {
        self.when.as_ref().is_none_or(|when| when.matches(root))
    }

    pub fn package(&self) -> &str {
        if self.package.is_empty() { &self.name } else { &self.package }
    }

    pub fn parse_list(json: &str) -> Result<Vec<Self>, String> {
        if json.trim().is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(json).map_err(|error| format!("the server plug-in list is invalid: {error}"))
    }
}

pub fn file_matches(extensions: &[String], names: &[String], file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    let by_extension = lower
        .rsplit_once('.')
        .is_some_and(|(_, extension)| extensions.iter().any(|known| known.eq_ignore_ascii_case(extension)));
    by_extension || names.iter().any(|pattern| glob(&pattern.to_ascii_lowercase(), &lower))
}

fn glob(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, rest)) => {
            let Some(name) = name.strip_prefix(head) else { return false };
            if rest.is_empty() {
                return true;
            }
            name.char_indices().map(|(at, _)| at).chain(std::iter::once(name.len())).any(|at| glob(rest, &name[at..]))
        }
    }
}

pub trait ServerTools: Send + Sync {
    fn locate(&self, server: &ServerDeclaration) -> Option<PathBuf> {
        find_executable(&server.command, &search_folders())
    }

    fn environment(&self, _server: &ServerDeclaration) -> Vec<(OsString, OsString)> {
        default_environment()
    }

    fn install_command(&self, server: &ServerDeclaration) -> Result<Command, NotInstallable> {
        default_install_command(server)
    }

    fn install_command_into(&self, server: &ServerDeclaration, prefix: &Path) -> Result<Command, NotInstallable> {
        prefix_install_command(server, prefix)
    }

    fn refresh(&self) {}

    fn initialization_options(&self, server: &ServerDeclaration, _root: &Path) -> Value {
        server.initialization_options.clone()
    }

    fn project_environment(&self, _root: &Path) -> Vec<(String, String)> {
        Vec::new()
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemTools;

impl ServerTools for SystemTools {}

pub fn search_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = zen_env::path_folders();
    folders.extend(
        std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect::<Vec<_>>()).unwrap_or_default(),
    );
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from);
    folders.extend(usual_folders(home.as_deref()));
    let mut unique = Vec::new();
    for folder in folders {
        if !unique.contains(&folder) {
            unique.push(folder);
        }
    }
    unique
}

pub fn usual_folders(home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = ["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from).to_vec();
    if let Some(home) = home {
        for folder in [".cargo/bin", "go/bin", ".local/bin", ".npm-global/bin", ".volta/bin", ".bun/bin"] {
            folders.push(home.join(folder));
        }
        let mut node: Vec<PathBuf> = std::fs::read_dir(home.join(".nvm/versions/node"))
            .map(|entries| entries.flatten().map(|entry| entry.path().join("bin")).collect())
            .unwrap_or_default();
        node.sort();
        folders.extend(node.into_iter().rev());
    }
    folders.extend(["/usr/bin", "/bin"].map(PathBuf::from));
    folders
}

pub fn find_executable(command: &str, folders: &[PathBuf]) -> Option<PathBuf> {
    if command.is_empty() {
        return None;
    }
    let path = Path::new(command);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_owned());
    }
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{command}.exe"), format!("{command}.cmd"), command.to_owned()]
    } else {
        vec![command.to_owned()]
    };
    folders.iter().flat_map(|folder| names.iter().map(move |name| folder.join(name))).find(|path| path.is_file())
}

pub fn default_environment() -> Vec<(OsString, OsString)> {
    std::env::join_paths(search_folders()).map(|path| vec![(OsString::from("PATH"), path)]).unwrap_or_default()
}

pub fn default_install_command(server: &ServerDeclaration) -> Result<Command, NotInstallable> {
    install_command_with(server, None, &search_folders())
}

pub fn prefix_install_command(server: &ServerDeclaration, prefix: &Path) -> Result<Command, NotInstallable> {
    install_command_with(server, Some(prefix), &search_folders())
}

fn install_command_with(
    server: &ServerDeclaration,
    prefix: Option<&Path>,
    folders: &[PathBuf],
) -> Result<Command, NotInstallable> {
    let recipe = server.install.as_ref().ok_or_else(|| NotInstallable::Manual(String::new()))?;
    let prefix = prefix.map(|prefix| prefix.to_string_lossy().into_owned());
    let mut args: Vec<&str> = match (recipe.kind, &prefix) {
        (InstallKind::Go, _) => vec!["install"],
        (InstallKind::Npm, Some(prefix)) => vec!["install", "--prefix", prefix.as_str(), "--no-audit", "--no-fund"],
        (InstallKind::Npm, None) => vec!["install", "-g"],
        (InstallKind::Cargo, _) => vec!["install", "--locked"],
        (InstallKind::Rustup, _) => vec!["component", "add"],
        (InstallKind::Archive | InstallKind::Manual, _) => return Err(NotInstallable::Manual(recipe.hint.clone())),
    };
    args.extend(recipe.package.split_whitespace());
    let Some(tool) = recipe.kind.tool().filter(|_| !recipe.package.trim().is_empty()) else {
        return Err(NotInstallable::Manual(recipe.hint.clone()));
    };
    let program = find_executable(tool, folders).ok_or(NotInstallable::ToolMissing(recipe.kind))?;
    let mut command = Command::new(program);
    zen_env::apply(&mut command);
    command.args(args).envs(default_environment());
    Ok(command)
}

pub fn npm_prefix(folder: &Path, server: &ServerDeclaration) -> Option<PathBuf> {
    server.install.as_ref().filter(|recipe| matches!(recipe.kind, InstallKind::Npm | InstallKind::Archive))?;
    let name: String = server
        .id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    (!name.is_empty() && name != "." && name != "..").then(|| folder.join(name))
}

pub fn locate_in_prefix(server: &ServerDeclaration, prefix: &Path) -> Option<PathBuf> {
    if Path::new(&server.command).is_absolute() {
        return None;
    }
    let archive = server.install.as_ref().is_some_and(|recipe| recipe.kind == InstallKind::Archive);
    let folder = if archive { prefix.to_owned() } else { prefix.join("node_modules").join(".bin") };
    find_executable(&server.command, &[folder])
}

const NODE_MODULES: &str = "${nodeModules:";
const NODE_MODULES_FOLDER: &str = "${nodeModulesFolder:";

pub fn expand_options(options: &Value, root: &Path, program: Option<&Path>) -> Value {
    match options {
        Value::String(text) => {
            let variable = |prefix: &str| text.strip_prefix(prefix).and_then(|rest| rest.strip_suffix('}'));
            if let Some(path) = variable(NODE_MODULES) {
                Value::String(node_module(path, root, program).to_string_lossy().into_owned())
            } else if let Some(path) = variable(NODE_MODULES_FOLDER) {
                let file = node_module(path, root, program);
                let folder = file.parent().map_or(file.clone(), Path::to_owned);
                Value::String(folder.to_string_lossy().into_owned())
            } else {
                options.clone()
            }
        }
        Value::Array(items) => Value::Array(items.iter().map(|item| expand_options(item, root, program)).collect()),
        Value::Object(fields) => Value::Object(
            fields.iter().map(|(name, value)| (name.clone(), expand_options(value, root, program))).collect(),
        ),
        _ => options.clone(),
    }
}

fn node_module(path: &str, root: &Path, program: Option<&Path>) -> PathBuf {
    let path = path.trim_matches('/');
    let project = root.ancestors().map(|folder| folder.join("node_modules").join(path)).find(|found| found.exists());
    if let Some(found) = project {
        return found;
    }
    let programs = program.into_iter().flat_map(|program| {
        let real = std::fs::canonicalize(program).ok();
        std::iter::once(program.to_owned()).chain(real)
    });
    programs
        .flat_map(|program| program.ancestors().skip(1).map(Path::to_owned).collect::<Vec<_>>())
        .flat_map(|folder| [folder.join("node_modules").join(path), folder.join("lib").join("node_modules").join(path)])
        .find(|found| found.exists())
        .unwrap_or_else(|| root.join("node_modules").join(path))
}

pub fn read_version(program: &Path, args: &[String], env: &[(OsString, OsString)]) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    let mut command = Command::new(program);
    zen_env::apply(&mut command);
    let mut child = command
        .args(args)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    parse_version(&String::from_utf8_lossy(&output.stdout))
        .or_else(|| parse_version(&String::from_utf8_lossy(&output.stderr)))
}

pub fn parse_version(output: &str) -> Option<String> {
    let looks_like = |word: &str| {
        let digits = word.strip_prefix('v').unwrap_or(word);
        digits.starts_with(|c: char| c.is_ascii_digit()) && digits.contains('.')
    };
    output
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| c == ',' || c == '(' || c == ')' || c == ';'))
        .find(|word| looks_like(word))
        .map(str::to_owned)
        .or_else(|| output.lines().map(str::trim).find(|line| !line.is_empty()).map(str::to_owned))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gopls() -> ServerDeclaration {
        ServerDeclaration::parse_list(
            r#"[{ "id": "gopls", "name": "gopls", "languages": ["go"], "extensions": ["go"], "command": "gopls",
                 "versionArgs": ["version"], "install": { "kind": "go", "package": "golang.org/x/tools/gopls@latest" },
                 "initializationOptions": { "semanticTokens": true } }]"#,
        )
        .unwrap()
        .remove(0)
    }

    #[cfg(unix)]
    #[test]
    fn a_downloaded_archive_server_with_a_plain_command_is_found_in_its_own_folder() {
        let mut server = gopls();
        server.command = "terraform-ls".into();
        server.install = Some(InstallRecipe {
            kind: InstallKind::Archive,
            url: "https://example.org/terraform-ls.zip".into(),
            sha256: "ab".repeat(32),
            files: Vec::new(),
            package: String::new(),
            hint: String::new(),
            platforms: Default::default(),
        });
        let prefix = tempfile::tempdir().unwrap();
        assert_eq!(locate_in_prefix(&server, prefix.path()), None);
        let program = prefix.path().join("terraform-ls");
        std::fs::write(&program, "#!/bin/sh\n").unwrap();
        assert_eq!(locate_in_prefix(&server, prefix.path()), Some(program));
        server.install.as_mut().unwrap().kind = InstallKind::Npm;
        assert_eq!(locate_in_prefix(&server, prefix.path()), None, "an npm server lives in node_modules/.bin");
    }

    #[test]
    fn declarations_are_read_with_defaults() {
        let server = gopls();
        assert_eq!(server.args, Vec::<String>::new());
        assert_eq!(server.install.as_ref().unwrap().kind, InstallKind::Go);
        assert_eq!(server.initialization_options["semanticTokens"], true);
        assert!(ServerDeclaration::parse_list("[{}]").is_err());
        let json = serde_json::to_value(&server).unwrap();
        assert_eq!(json["versionArgs"][0], "version");
        assert!(json.get("names").is_none());
        assert!(json.get("watches").is_none());
        assert!(json.get("inlayHints").is_none());
        assert!(server.workspace_symbols, "a server finds symbols unless it says otherwise");
        assert!(json.get("workspaceSymbols").is_none());
    }

    #[test]
    fn a_server_hears_of_the_files_it_serves_and_watches() {
        let server = ServerDeclaration { watches: vec!["go.mod".to_owned(), "go.work".to_owned()], ..gopls() };
        assert!(server.hears_of("main.go"));
        assert!(server.hears_of("go.work") && server.hears_of("go.mod"));
        assert!(!server.serves("go.work"), "it is not opened in the server");
        assert!(!server.hears_of("Cargo.toml") && !gopls().hears_of("go.work"));
    }

    #[test]
    fn power_save_settings_are_laid_over_the_others() {
        let declared = ServerDeclaration::parse_list(
            r#"[{ "id": "gopls", "name": "gopls", "languages": ["go"], "command": "gopls",
                 "powerSave": { "section": "gopls", "settings": { "diagnosticsTrigger": "Save", "ui": { "a": 1 } } } }]"#,
        )
        .unwrap()
        .remove(0);
        let power_save = declared.power_save.clone().unwrap();
        assert_eq!(power_save.section, "gopls");
        let base = serde_json::json!({ "semanticTokens": true, "ui": { "b": 2 } });
        assert_eq!(
            power_save.over(&base),
            serde_json::json!({ "semanticTokens": true, "diagnosticsTrigger": "Save", "ui": { "a": 1, "b": 2 } })
        );
        assert_eq!(
            power_save.over(&Value::Null),
            serde_json::json!({ "diagnosticsTrigger": "Save", "ui": { "a": 1 } })
        );
        assert!(serde_json::to_value(gopls()).unwrap().get("powerSave").is_none());
    }

    #[test]
    fn inspection_switches_land_under_the_declared_path_and_leave_again() {
        let declared = InspectionSettings { section: "gopls".into(), path: "analyses".into() };
        let switches = BTreeMap::from([("shadow", true), ("unusedparams", false)]);
        let settings = declared.settings(&serde_json::json!({ "semanticTokens": true }), &switches);
        assert_eq!(
            settings,
            serde_json::json!({ "semanticTokens": true, "analyses": { "shadow": true, "unusedparams": false } })
        );
        assert_eq!(declared.without(&settings), serde_json::json!({ "semanticTokens": true }));
    }

    #[test]
    fn inlay_hint_kinds_switch_the_servers_settings() {
        let declared = ServerDeclaration::parse_list(
            r#"[{ "id": "gopls", "name": "gopls", "languages": ["go"], "command": "gopls",
                 "inlayHints": { "section": "gopls", "kinds": {
                     "parameterNames": ["hints.parameterNames"],
                     "variableTypes": ["hints.assignVariableTypes", "hints.rangeVariableTypes"] } } }]"#,
        )
        .unwrap()
        .remove(0);
        let hints = declared.inlay_hints.unwrap();
        assert_eq!(hints.section, "gopls");
        let settings = hints.settings(&serde_json::json!({ "semanticTokens": true }), |kind| kind == "parameterNames");
        assert_eq!(
            settings,
            serde_json::json!({ "semanticTokens": true, "hints": {
                "parameterNames": true, "assignVariableTypes": false, "rangeVariableTypes": false } })
        );
        assert_eq!(hints.settings(&Value::Null, |_| true)["hints"]["rangeVariableTypes"], true);
    }

    #[test]
    fn files_are_matched_by_extension_or_name() {
        let mut server = gopls();
        assert!(server.serves("main.go") && server.serves("MAIN.GO") && !server.serves("go.mod"));
        server.names = vec!["Dockerfile".to_owned(), "*.dockerfile".to_owned()];
        assert!(server.serves("dockerfile") && server.serves("app.Dockerfile") && !server.serves("Dockerfiles"));
        server.languages = vec!["json".to_owned(), "jsonc".to_owned()];
        assert_eq!(server.language_for("tsconfig.jsonc"), "jsonc");
        assert_eq!(server.language_for("a.json5"), "json");
        server.languages = vec!["typescript".to_owned(), "javascript".to_owned()];
        server.language_ids = [("tsx", "typescriptreact"), ("mjs", "javascript"), ("js", "javascript")]
            .map(|(extension, id)| (extension.to_owned(), id.to_owned()))
            .into();
        assert_eq!(server.language_for("App.TSX"), "typescriptreact");
        assert_eq!(server.language_for("a.mjs"), "javascript");
        assert_eq!(server.language_for("a.ts"), "typescript", "the first without an entry");
        assert_eq!(server.optimize_imports_kinds(), ["source.organizeImports"], "by default");
        server.optimize_imports = vec!["source.removeUnusedImports".to_owned(), "source.organizeImports".to_owned()];
        assert_eq!(server.optimize_imports_kinds()[0], "source.removeUnusedImports");
    }

    #[test]
    fn generate_offers_source_kinds_unless_it_names_its_own() {
        let mut generate = GenerateDeclaration::default();
        assert!(generate.offers_kind("source.generate.toString") && generate.offers_kind("source"));
        assert!(!generate.offers_kind("sourcery") && !generate.offers_kind("refactor.extract"));
        generate.kinds = vec!["source.addTest".to_owned()];
        assert!(generate.offers_kind("source.addTest") && !generate.offers_kind("source.doc"));
        let declared: GenerateDeclaration = serde_json::from_value(serde_json::json!({
            "prompts": [{
                "command": "java.action.generateToStringPrompt",
                "members": "java/checkToStringStatus",
                "lists": [{ "id": "fields", "items": "/fields/*", "label": "{/name}", "selected": "/isSelected" }],
                "generate": "java/generateToString",
                "params": { "context": "${context}", "fields": "${members:fields}" }
            }]
        }))
        .unwrap();
        let prompt = declared.prompts_for("java.action.generateToStringPrompt", &Value::Null)[0];
        assert_eq!(
            (prompt.members.as_str(), prompt.lists[0].items.as_str()),
            ("java/checkToStringStatus", "/fields/*")
        );
        assert!(declared.prompts_for("java.action.other", &Value::Null).is_empty());
    }

    #[test]
    fn a_prompt_is_chosen_by_its_command_and_the_values_its_argument_must_hold() {
        let declared: GenerateDeclaration = serde_json::from_value(serde_json::json!({
            "prompts": [
                { "command": "accessors", "match": { "/kind": 0 }, "name": { "en": "Getter" }, "members": "m", "generate": "g" },
                { "command": "accessors", "match": { "/kind": 1 }, "name": { "en": "Setter" }, "members": "m", "generate": "g" },
                { "command": "override", "action": "override", "members": "m", "generate": "g" },
                { "command": "override", "action": "implement", "members": "m", "generate": "g" }
            ]
        }))
        .unwrap();
        let names = |argument: Value| -> Vec<String> {
            declared.prompts_for("accessors", &argument).iter().map(|prompt| text_in(&prompt.name, "en")).collect()
        };
        assert_eq!(names(serde_json::json!({ "kind": 1, "range": {} })), ["Setter"]);
        assert!(names(serde_json::json!({ "kind": 2 })).is_empty());
        let actions: Vec<&str> =
            declared.prompts_for("override", &Value::Null).iter().map(|prompt| prompt.action.as_str()).collect();
        assert_eq!(actions, ["override", "implement"]);
    }

    #[test]
    fn a_condition_is_a_pointer_that_is_set_or_with_a_bang_unset() {
        let value = serde_json::json!({ "yes": true, "no": false, "empty": [], "name": "x", "zero": 0 });
        assert!(holds(&value, "/yes") && holds(&value, "/name") && holds(&value, "/zero"));
        assert!(!holds(&value, "/no") && !holds(&value, "/empty") && !holds(&value, "/missing"));
        assert!(holds(&value, "!/no") && holds(&value, "!/missing") && !holds(&value, "!/yes"));
    }

    #[test]
    fn programs_are_found_in_folders() {
        let temp = tempfile::tempdir().unwrap();
        let temp = temp.path();
        let bin = temp.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let name = if cfg!(windows) { "fake-ls.exe" } else { "fake-ls" };
        std::fs::write(bin.join(name), "").unwrap();
        let folders = vec![temp.join("missing"), bin.clone()];
        assert_eq!(find_executable("fake-ls", &folders), Some(bin.join(name)));
        assert_eq!(find_executable("other", &folders), None);
        assert_eq!(find_executable(bin.join(name).to_str().unwrap(), &[]), Some(bin.join(name)));
        assert_eq!(find_executable("", &folders), None);
    }

    #[test]
    fn the_usual_folders_include_the_tools_homes() {
        let folders = usual_folders(Some(Path::new("/home/u")));
        for folder in ["/opt/homebrew/bin", "/home/u/.cargo/bin", "/home/u/go/bin", "/usr/bin"] {
            assert!(folders.contains(&PathBuf::from(folder)), "{folder}");
        }
    }

    #[test]
    fn manual_servers_say_how_to_install_them() {
        let mut server = gopls();
        server.install = Some(InstallRecipe {
            kind: InstallKind::Manual,
            url: String::new(),
            sha256: String::new(),
            files: Vec::new(),
            package: String::new(),
            hint: "brew it".into(),
            platforms: Default::default(),
        });
        assert_eq!(default_install_command(&server).unwrap_err(), NotInstallable::Manual("brew it".into()));
        server.install = None;
        assert_eq!(default_install_command(&server).unwrap_err(), NotInstallable::Manual(String::new()));
        assert_eq!(InstallKind::Go.download(), Some("https://go.dev/dl/"));
        assert_eq!(InstallKind::Manual.tool(), None);
        assert_eq!(
            (InstallKind::Rustup.tool(), InstallKind::Rustup.download()),
            (Some("rustup"), Some("https://rustup.rs/"))
        );
        let rustup: InstallRecipe =
            serde_json::from_value(serde_json::json!({ "kind": "rustup", "package": "x" })).unwrap();
        assert_eq!(rustup.kind, InstallKind::Rustup);
    }

    #[test]
    fn options_name_node_modules_of_the_project_or_of_the_server() {
        let temp = tempfile::tempdir().unwrap();
        let temp = temp.path();
        let prefix = temp.join("prefix");
        let typescript = prefix.join("lib/node_modules/typescript/lib");
        std::fs::create_dir_all(&typescript).unwrap();
        std::fs::create_dir_all(prefix.join("bin")).unwrap();
        let program = prefix.join("bin/vue-language-server");
        std::fs::write(&program, "").unwrap();
        let project = temp.join("app");
        std::fs::create_dir_all(project.join("src")).unwrap();
        let options = serde_json::json!({
            "typescript": { "tsdk": "${nodeModules:typescript/lib}" },
            "vue": { "hybridMode": false },
            "list": ["${nodeModules:missing}", "plain ${nodeModules:x}"],
        });
        let expanded = expand_options(&options, &project.join("src"), Some(&program));
        assert_eq!(expanded["typescript"]["tsdk"], typescript.to_string_lossy().as_ref(), "next to the server");
        assert_eq!(expanded["vue"]["hybridMode"], false);
        assert_eq!(expanded["list"][0], project.join("src/node_modules/missing").to_string_lossy().as_ref());
        assert_eq!(expanded["list"][1], "plain ${nodeModules:x}", "only a whole string is a variable");
        let own = project.join("node_modules/typescript/lib");
        std::fs::create_dir_all(&own).unwrap();
        let expanded = expand_options(&options, &project.join("src"), Some(&program));
        assert_eq!(expanded["typescript"]["tsdk"], own.to_string_lossy().as_ref(), "the project's own comes first");
        assert_eq!(expand_options(&Value::Null, &project, None), Value::Null);

        std::fs::write(typescript.join("typescript.js"), "").unwrap();
        let options = serde_json::json!({ "tsdk": "${nodeModulesFolder:typescript/lib/typescript.js}" });
        let expanded = expand_options(&options, &project, Some(&program));
        assert_eq!(expanded["tsdk"], typescript.to_string_lossy().as_ref());
        std::fs::write(own.join("typescript.js"), "").unwrap();
        let expanded = expand_options(&options, &project, Some(&program));
        assert_eq!(expanded["tsdk"], own.to_string_lossy().as_ref());
    }

    #[test]
    fn several_packages_are_installed_together() {
        let mut server = gopls();
        server.install = Some(InstallRecipe {
            kind: InstallKind::Npm,
            url: String::new(),
            sha256: String::new(),
            files: Vec::new(),
            package: "@vue/language-server@2.2.12  typescript".into(),
            hint: String::new(),
            platforms: Default::default(),
        });
        if let Ok(command) = default_install_command(&server) {
            let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
            assert_eq!(args, ["install", "-g", "@vue/language-server@2.2.12", "typescript"]);
        }
        server.install = Some(InstallRecipe {
            kind: InstallKind::Npm,
            url: String::new(),
            sha256: String::new(),
            files: Vec::new(),
            package: "  ".into(),
            hint: "h".into(),
            platforms: Default::default(),
        });
        assert_eq!(default_install_command(&server).unwrap_err(), NotInstallable::Manual("h".into()));
    }

    fn npm_server() -> ServerDeclaration {
        ServerDeclaration {
            id: "vue-language-server".into(),
            command: "vue-language-server".into(),
            install: Some(InstallRecipe {
                kind: InstallKind::Npm,
                url: String::new(),
                sha256: String::new(),
                files: Vec::new(),
                package: "@vue/language-server@2.2.12 typescript@5".into(),
                hint: String::new(),
                platforms: Default::default(),
            }),
            ..gopls()
        }
    }

    fn fake_npm(temp: &Path) -> Vec<PathBuf> {
        let bin = temp.join("tools");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(if cfg!(windows) { "npm.cmd" } else { "npm" }), "").unwrap();
        vec![bin]
    }

    fn args(command: &Command) -> Vec<String> {
        command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn npm_servers_install_into_the_ides_prefix_never_globally() {
        let temp = tempfile::tempdir().unwrap();
        let folders = fake_npm(temp.path());
        let prefix = temp.path().join("data/language-servers/vue-language-server");
        let command = install_command_with(&npm_server(), Some(&prefix), &folders).unwrap();
        assert!(Path::new(command.get_program()).starts_with(&folders[0]), "the npm found");
        let prefix_text = prefix.to_string_lossy().into_owned();
        assert_eq!(
            args(&command),
            [
                "install",
                "--prefix",
                prefix_text.as_str(),
                "--no-audit",
                "--no-fund",
                "@vue/language-server@2.2.12",
                "typescript@5"
            ]
        );
        assert!(!args(&command).contains(&"-g".to_owned()));
        let global = install_command_with(&npm_server(), None, &folders).unwrap();
        assert_eq!(args(&global)[..2], ["install", "-g"]);
        let go = install_command_with(&gopls(), Some(&prefix), &fake_go(temp.path())).unwrap();
        assert_eq!(args(&go), ["install", "golang.org/x/tools/gopls@latest"], "only npm takes the prefix");
        assert_eq!(
            install_command_with(&npm_server(), Some(&prefix), &[]).unwrap_err(),
            NotInstallable::ToolMissing(InstallKind::Npm)
        );
    }

    fn fake_go(temp: &Path) -> Vec<PathBuf> {
        let bin = temp.join("go-tools");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(if cfg!(windows) { "go.exe" } else { "go" }), "").unwrap();
        vec![bin]
    }

    #[test]
    fn the_prefix_is_one_folder_per_npm_server_and_its_program_is_in_node_modules_bin() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("language-servers");
        let prefix = npm_prefix(&folder, &npm_server()).unwrap();
        assert_eq!(prefix, folder.join("vue-language-server"));
        assert_eq!(npm_prefix(&folder, &gopls()), None, "gopls is not an npm server");
        let odd = ServerDeclaration { id: "../x/y".into(), ..npm_server() };
        assert_eq!(npm_prefix(&folder, &odd), Some(folder.join(".._x_y")), "an id never leaves the folder");
        assert_eq!(locate_in_prefix(&npm_server(), &prefix), None);
        let bin = prefix.join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        let name = if cfg!(windows) { "vue-language-server.cmd" } else { "vue-language-server" };
        std::fs::write(bin.join(name), "").unwrap();
        assert_eq!(locate_in_prefix(&npm_server(), &prefix), Some(bin.join(name)));
    }

    #[test]
    fn typescript_comes_from_the_project_when_it_has_typescript_js_else_from_the_servers_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("language-servers/vue-language-server");
        let own = prefix.join("node_modules/typescript/lib");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("typescript.js"), "").unwrap();
        let bin = prefix.join("node_modules/.bin");
        std::fs::create_dir_all(&bin).unwrap();
        let program = bin.join("vue-language-server");
        std::fs::write(&program, "").unwrap();
        let options = serde_json::json!({ "tsdk": "${nodeModulesFolder:typescript/lib/typescript.js}" });
        let project = temp.path().join("app");
        let nested = project.join("packages/web");
        std::fs::create_dir_all(&nested).unwrap();
        let tsdk = |root: &Path| expand_options(&options, root, Some(&program))["tsdk"].as_str().unwrap().to_owned();
        assert_eq!(tsdk(&nested), own.to_string_lossy(), "no TypeScript in the project: the IDE's own");
        let seven = project.join("node_modules/typescript/lib");
        std::fs::create_dir_all(&seven).unwrap();
        std::fs::write(seven.join("tsc.js"), "").unwrap();
        assert_eq!(tsdk(&nested), own.to_string_lossy(), "TypeScript 7 has no typescript.js: the IDE's own");
        std::fs::write(seven.join("typescript.js"), "").unwrap();
        assert_eq!(tsdk(&nested), seven.to_string_lossy(), "the project's TypeScript 5, found up the folders, wins");
    }

    #[test]
    fn the_typescript_server_falls_back_to_its_own_tsserver_when_the_project_has_typescript_7() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path().join("language-servers/typescript-language-server");
        let own = prefix.join("node_modules/typescript/lib/tsserver.js");
        std::fs::create_dir_all(own.parent().unwrap()).unwrap();
        std::fs::write(&own, "").unwrap();
        let program = prefix.join("node_modules/.bin/typescript-language-server");
        std::fs::create_dir_all(program.parent().unwrap()).unwrap();
        std::fs::write(&program, "").unwrap();
        let options =
            serde_json::json!({ "tsserver": { "fallbackPath": "${nodeModules:typescript/lib/tsserver.js}" } });
        let project = temp.path().join("app");
        let seven = project.join("node_modules/typescript/lib");
        std::fs::create_dir_all(&seven).unwrap();
        std::fs::write(seven.join("tsc.js"), "").unwrap();
        let fallback = |root: &Path| {
            expand_options(&options, root, Some(&program))["tsserver"]["fallbackPath"].as_str().unwrap().to_owned()
        };
        assert_eq!(fallback(&project), own.to_string_lossy(), "TypeScript 7 has no tsserver.js: the server's own");
        std::fs::write(seven.join("tsserver.js"), "").unwrap();
        assert_eq!(fallback(&project), seven.join("tsserver.js").to_string_lossy(), "the project's TypeScript 5");
    }

    #[test]
    fn versions_are_read_from_output() {
        assert_eq!(parse_version("golang.org/x/tools/gopls v0.18.1\n  build info").as_deref(), Some("v0.18.1"));
        assert_eq!(parse_version("taplo 0.9.3").as_deref(), Some("0.9.3"));
        assert_eq!(parse_version("rust-analyzer 1.83.0 (90b35a6 2024-11-26)").as_deref(), Some("1.83.0"));
        assert_eq!(parse_version("\n  nightly build\n").as_deref(), Some("nightly build"));
        assert_eq!(parse_version(""), None);
    }

    fn jdtls() -> ServerDeclaration {
        let mut server = npm_server();
        server.id = "jdtls".into();
        server.command = "${javaHome}/bin/java".into();
        server.args = vec![
            "-jar".into(),
            "${glob:plugins/org.eclipse.equinox.launcher_*.jar}".into(),
            "-configuration".into(),
            "${workspaceData}/config".into(),
            "-Dosgi.sharedConfiguration.area=${serverFolder}/config_${os}${archSuffix}".into(),
        ];
        server.java = Some(25);
        server.install = Some(InstallRecipe {
            kind: InstallKind::Archive,
            url: "https://example.org/jdtls-1.0.tar.gz".into(),
            sha256: "ab".repeat(32),
            files: Vec::new(),
            package: String::new(),
            hint: String::new(),
            platforms: Default::default(),
        });
        server
    }

    #[test]
    fn placeholders_name_the_servers_folder_the_jdk_and_the_projects_data() {
        let folder = tempfile::tempdir().unwrap();
        let plugins = folder.path().join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        for name in
            ["org.eclipse.equinox.launcher.cocoa.macosx_1.jar", "org.eclipse.equinox.launcher_1.8.0.jar", "other.jar"]
        {
            std::fs::write(plugins.join(name), "").unwrap();
        }
        let server = jdtls();
        assert!(server.has_placeholders() && !npm_server().has_placeholders());
        let launcher = plugins.join("org.eclipse.equinox.launcher_1.8.0.jar");
        assert_eq!(server.unpacked_marker(folder.path()), Some(launcher.clone()));
        assert_eq!(server.unpacked_marker(&folder.path().join("missing")), None);
        assert_eq!(
            npm_prefix(Path::new("/data/language-servers"), &server),
            Some(PathBuf::from("/data/language-servers/jdtls"))
        );

        let places = LaunchPlaces {
            server_folder: Some(folder.path().to_owned()),
            java_home: Some(PathBuf::from("/jdk")),
            workspace_data: Some(PathBuf::from("/caches/shop")),
        };
        let (program, args) = server.launch_line(&places).unwrap();
        assert_eq!(program, PathBuf::from("/jdk/bin/java"));
        assert_eq!(args[1], launcher.to_string_lossy());
        assert_eq!(args[3], "/caches/shop/config");
        let suffix = if cfg!(target_arch = "aarch64") && !cfg!(windows) { "_arm" } else { "" };
        assert_eq!(
            args[4],
            format!("-Dosgi.sharedConfiguration.area={}/config_{}{suffix}", folder.path().display(), os_name())
        );

        let no_java = LaunchPlaces { java_home: None, ..places.clone() };
        assert_eq!(server.launch_line(&no_java), Err(LaunchMissing::Java(25)));
        let no_server = LaunchPlaces { server_folder: Some(folder.path().join("missing")), ..places };
        assert_eq!(server.launch_line(&no_server), Err(LaunchMissing::Server));
        assert!(matches!(install_command_with(&server, None, &[]), Err(NotInstallable::Manual(_))));
    }

    #[test]
    fn an_archive_recipe_reads_from_plugin_json() {
        let list = ServerDeclaration::parse_list(
            r#"[{"id":"jdtls","name":"Eclipse JDT Language Server","languages":["java"],"extensions":["java"],
                "command":"${javaHome}/bin/java","java":25,
                "install":{"kind":"archive","url":"https://download.eclipse.org/x.tar.gz","sha256":"00"}}]"#,
        )
        .unwrap();
        let recipe = list[0].install.as_ref().unwrap();
        assert_eq!(
            (recipe.kind, recipe.url.as_str(), recipe.sha256.as_str()),
            (InstallKind::Archive, "https://download.eclipse.org/x.tar.gz", "00")
        );
        assert_eq!(list[0].java, Some(25));
    }
}

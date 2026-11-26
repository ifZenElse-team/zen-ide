use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ide_settings::zip::{Archive, Limits};

pub const MAIN: &str = "META-INF/spring-configuration-metadata.json";

pub const ADDITIONAL: &str = "META-INF/additional-spring-configuration-metadata.json";

const LIMITS: Limits =
    Limits { archive: 1024 * 1024 * 1024, entries: 65_535, entry: 64 * 1024 * 1024, total: 4 * 1024 * 1024 * 1024 };

/// Why a property should no longer be used.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Deprecation {
    /// `warning` (still read) or `error` (no longer read).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// Why, in the library's words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The property to use instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    /// The version it was deprecated in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
}

impl Deprecation {
    pub fn is_error(&self) -> bool {
        self.level.as_deref() == Some("error")
    }
}

/// A value a property is known to take.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HintValue {
    /// The value.
    pub value: Value,
    /// What it means.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A configuration property, as the library's metadata declares it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Property {
    /// Its full name, e.g. `server.port`.
    pub name: String,
    /// The Java type of its value, e.g. `java.lang.Integer`.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub value_type: Option<String>,
    /// What it does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The class that declares it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,
    /// Its value when not set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_value: Option<Value>,
    /// Set when it is deprecated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecation: Option<Deprecation>,
    /// The values it is known to take.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<HintValue>,
    #[serde(skip_serializing)]
    #[schemars(skip)]
    deprecated: bool,
}

impl Property {
    pub fn takes_keys(&self) -> bool {
        self.value_type.as_deref().is_some_and(|ty| ty.contains('<') || ty.ends_with("[]"))
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self.value_type.as_deref(), Some("java.lang.Boolean" | "boolean"))
    }

    pub fn default_text(&self) -> Option<String> {
        match self.default_value.as_ref()? {
            Value::String(text) => Some(text.clone()),
            Value::Array(items) => Some(items.iter().map(value_text).collect::<Vec<_>>().join(",")),
            other => Some(other.to_string()),
        }
    }
}

pub fn value_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Hint {
    name: String,
    values: Vec<HintValue>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct File {
    properties: Vec<Property>,
    hints: Vec<Hint>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata {
    properties: BTreeMap<String, Property>,
    canonical: HashMap<String, String>,
}

pub fn canonical(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(open) = rest.find('[') {
        push_canonical(&mut out, &rest[..open]);
        let inner = &rest[open + 1..];
        let close = inner.find(']').unwrap_or(inner.len());
        let index = &inner[..close];
        if !index.is_empty() && !index.bytes().all(|byte| byte.is_ascii_digit()) {
            if !out.is_empty() && !out.ends_with('.') {
                out.push('.');
            }
            out.extend(index.chars().flat_map(char::to_lowercase));
        }
        rest = inner.get(close + 1..).unwrap_or("");
    }
    push_canonical(&mut out, rest);
    out.replace("..", ".").trim_matches('.').to_owned()
}

fn push_canonical(out: &mut String, part: &str) {
    out.extend(part.chars().filter(|c| *c != '-' && *c != '_').flat_map(char::to_lowercase));
}

impl Metadata {
    pub fn parse(json: &[u8]) -> Result<Self, String> {
        let file: File = serde_json::from_slice(json).map_err(|error| error.to_string())?;
        let mut metadata = Metadata::default();
        metadata.add(file);
        Ok(metadata)
    }

    fn add(&mut self, file: File) {
        for mut property in file.properties {
            if property.name.is_empty() {
                continue;
            }
            if property.deprecated && property.deprecation.is_none() {
                property.deprecation = Some(Deprecation::default());
            }
            match self.properties.get_mut(&property.name) {
                Some(known) => {
                    known.value_type = known.value_type.take().or(property.value_type);
                    known.description = known.description.take().or(property.description);
                    known.default_value = known.default_value.take().or(property.default_value);
                    known.deprecation = known.deprecation.take().or(property.deprecation);
                    known.source_type = known.source_type.take().or(property.source_type);
                    for value in property.values {
                        if !known.values.iter().any(|seen| seen.value == value.value) {
                            known.values.push(value);
                        }
                    }
                }
                None => {
                    self.canonical.entry(canonical(&property.name)).or_insert_with(|| property.name.clone());
                    self.properties.insert(property.name.clone(), property);
                }
            }
        }
        for hint in file.hints {
            let name = hint.name.strip_suffix(".values").unwrap_or(&hint.name);
            if let Some(property) = self.properties.get_mut(name) {
                for value in hint.values {
                    if !property.values.iter().any(|known| known.value == value.value) {
                        property.values.push(value);
                    }
                }
            }
        }
    }

    pub fn merge(&mut self, other: &Metadata) {
        let file = File { properties: other.properties.values().cloned().collect(), hints: Vec::new() };
        self.add(file);
    }

    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }

    pub fn len(&self) -> usize {
        self.properties.len()
    }

    pub fn properties(&self) -> impl Iterator<Item = &Property> {
        self.properties.values()
    }

    pub fn get(&self, name: &str) -> Option<&Property> {
        self.properties.get(name)
    }

    pub fn find(&self, key: &str) -> Option<&Property> {
        let wanted = canonical(key);
        if let Some(name) = self.canonical.get(&wanted) {
            return self.properties.get(name);
        }
        let mut prefix = wanted.as_str();
        while let Some((head, _)) = prefix.rsplit_once('.') {
            prefix = head;
            if let Some(property) = self.canonical.get(prefix).and_then(|name| self.properties.get(name))
                && property.takes_keys()
            {
                return Some(property);
            }
        }
        None
    }
}

type Stamp = (Option<SystemTime>, u64);

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

static JARS: Mutex<BTreeMap<PathBuf, (Stamp, Arc<Metadata>)>> = Mutex::new(BTreeMap::new());

pub fn of_jar(jar: &Path) -> Arc<Metadata> {
    let Some(stamp) = stamp(jar) else { return Arc::default() };
    if let Some((seen, metadata)) = JARS.lock().unwrap_or_else(PoisonError::into_inner).get(jar)
        && *seen == stamp
    {
        return Arc::clone(metadata);
    }
    let metadata = Arc::new(read_jar(jar));
    JARS.lock().unwrap_or_else(PoisonError::into_inner).insert(jar.to_owned(), (stamp, Arc::clone(&metadata)));
    metadata
}

fn read_jar(jar: &Path) -> Metadata {
    let Ok(bytes) = std::fs::read(jar) else { return Metadata::default() };
    let Ok(zip) = Archive::parse(bytes, LIMITS) else { return Metadata::default() };
    let mut metadata = Metadata::default();
    for name in [MAIN, ADDITIONAL] {
        if let Some(entry) = zip.entries().iter().find(|entry| entry.name == name)
            && let Ok(data) = zip.read(entry)
            && let Ok(read) = Metadata::parse(&data)
        {
            metadata.merge(&read);
        }
    }
    metadata
}

pub fn own_files(root: &Path, modules: &[PathBuf]) -> Vec<PathBuf> {
    let mut folders = vec![root.to_owned()];
    folders.extend(modules.iter().filter(|module| *module != root).cloned());
    folders
        .iter()
        .flat_map(|folder| {
            [
                folder.join("target/classes").join(MAIN),
                folder.join("build/classes/java/main").join(MAIN),
                folder.join("build/tmp/kapt3/classes/main").join(MAIN),
                folder.join("src/main/resources").join(ADDITIONAL),
            ]
        })
        .collect()
}

pub fn stamps(files: &[PathBuf]) -> Vec<Option<Stamp>> {
    files.iter().map(|file| stamp(file)).collect()
}

pub fn load(jars: &[PathBuf], own: &[PathBuf]) -> Metadata {
    let mut metadata = Metadata::default();
    for file in own {
        if let Ok(data) = std::fs::read(file)
            && let Ok(read) = Metadata::parse(&data)
        {
            metadata.merge(&read);
        }
    }
    for jar in jars {
        metadata.merge(&of_jar(jar));
    }
    metadata
}

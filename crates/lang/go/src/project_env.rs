use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_i18n::t;

pub const SHARED_FILE: &str = ".zen/go.json";

pub const PERSONAL_FILE: &str = "go-environment.json";

pub const READONLY_FLAG: &str = "-mod=readonly";

const REFUSED: &[&str] = &["GOROOT", "PATH"];

const PERSONAL_ONLY: &[&str] = &["GOENV", "GOTOOLCHAIN"];

/// Which layer of a project's Go environment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// `.zen/go.json` in the project, shared with everyone who has the repository.
    Project,
    /// Only on this computer, kept in the IDE's data folder.
    Personal,
}

/// One variable in effect for a project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    /// Its name, e.g. `GOPROXY`.
    pub name: String,
    /// Its value.
    pub value: String,
    /// The layer it comes from.
    pub source: Scope,
}

#[derive(Clone, Debug, Default)]
pub struct GoProjectEnv {
    personal_file: Option<PathBuf>,
}

pub fn parse(text: &str) -> Result<BTreeMap<String, String>, String> {
    crate::new_project::environment(text).map(|pairs| pairs.into_iter().collect())
}

pub fn text_of(variables: &BTreeMap<String, String>) -> String {
    variables.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; ")
}

fn has_credentials(value: &str) -> bool {
    value.split([',', '|', ' ']).any(|part| {
        part.split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or_default())
            .is_some_and(|authority| authority.contains('@'))
    })
}

pub fn redact(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail.find(['/', '?', '#', ',', '|', ' ']).unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(user) => {
                out.push_str("***");
                out.push_str(&tail[user..end]);
            }
            None => out.push_str(&tail[..end]),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

impl Variable {
    pub fn redacted(self) -> Self {
        Self { value: redact(&self.value), ..self }
    }
}

pub fn check(scope: Scope, name: &str, value: &str) -> Result<(), String> {
    let go_name = name
        .strip_prefix("CGO_")
        .or_else(|| name.strip_prefix("GO"))
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'));
    if REFUSED.contains(&name) {
        return Err(t!("go-env-error-refused", name = name));
    }
    if !go_name {
        return Err(t!("go-env-error-name", name = name));
    }
    if scope == Scope::Project && PERSONAL_ONLY.contains(&name) {
        return Err(t!("go-env-error-personal-only", name = name));
    }
    if scope == Scope::Project && has_credentials(value) {
        return Err(t!("go-env-error-credentials", name = name));
    }
    Ok(())
}

fn canonical(root: &Path) -> PathBuf {
    root.canonicalize().unwrap_or_else(|_| root.to_owned())
}

fn read_json(file: &Path) -> Value {
    std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null)
}

fn variables_of(value: &Value) -> BTreeMap<String, String> {
    value
        .as_object()
        .map(|object| {
            object.iter().filter_map(|(name, value)| Some((name.clone(), value.as_str()?.to_owned()))).collect()
        })
        .unwrap_or_default()
}

fn write_json(file: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    text.push('\n');
    std::fs::write(file, text).map_err(|error| error.to_string())
}

impl GoProjectEnv {
    pub fn new(personal_file: Option<PathBuf>) -> Self {
        Self { personal_file }
    }

    pub fn shared(&self, root: &Path) -> BTreeMap<String, String> {
        let variables = variables_of(&read_json(&root.join(SHARED_FILE))["env"]);
        variables.into_iter().filter(|(name, value)| check(Scope::Project, name, value).is_ok()).collect()
    }

    pub fn personal(&self, root: &Path) -> BTreeMap<String, String> {
        let Some(file) = &self.personal_file else { return BTreeMap::new() };
        let key = canonical(root).to_string_lossy().into_owned();
        let variables = variables_of(&read_json(file)["projects"][key.as_str()]);
        variables.into_iter().filter(|(name, value)| check(Scope::Personal, name, value).is_ok()).collect()
    }

    pub fn effective(&self, root: &Path, trusted: bool) -> Vec<Variable> {
        let mut found: BTreeMap<String, Variable> = BTreeMap::new();
        if trusted {
            for (name, value) in self.shared(root) {
                found.insert(name.clone(), Variable { name, value, source: Scope::Project });
            }
        }
        for (name, value) in self.personal(root) {
            found.insert(name.clone(), Variable { name, value, source: Scope::Personal });
        }
        found.into_values().collect()
    }

    pub fn variables(&self, root: &Path, trusted: bool) -> Vec<(String, String)> {
        self.effective(root, trusted).into_iter().map(|variable| (variable.name, variable.value)).collect()
    }

    pub fn set(&self, root: &Path, scope: Scope, variables: &BTreeMap<String, String>) -> Result<(), String> {
        for (name, value) in variables {
            check(scope, name, value)?;
        }
        let env: serde_json::Map<String, Value> =
            variables.iter().map(|(name, value)| (name.clone(), json!(value))).collect();
        let written = match scope {
            Scope::Project => {
                let file = root.join(SHARED_FILE);
                let mut document = read_json(&file);
                if !document.is_object() {
                    document = json!({ "version": 1 });
                }
                if env.is_empty()
                    && document
                        .as_object()
                        .is_some_and(|object| object.keys().all(|key| key == "version" || key == "env"))
                {
                    return match std::fs::remove_file(&file) {
                        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.to_string()),
                        _ => Ok(()),
                    };
                }
                document["env"] = Value::Object(env);
                write_json(&file, &document)
            }
            Scope::Personal => {
                let file = self.personal_file.as_ref().ok_or_else(|| t!("go-env-error-no-data"))?;
                let mut document = read_json(file);
                if !document.is_object() {
                    document = json!({ "version": 1 });
                }
                if !document["projects"].is_object() {
                    document["projects"] = json!({});
                }
                let key = canonical(root).to_string_lossy().into_owned();
                let projects = document["projects"].as_object_mut().expect("an object");
                if env.is_empty() {
                    projects.remove(&key);
                } else {
                    projects.insert(key, Value::Object(env));
                }
                write_json(file, &document)
            }
        };
        written.map_err(|error| t!("go-env-error-write", error = error))
    }
}

pub fn readonly_flags(env: &[(String, String)]) -> String {
    let flags = env.iter().rev().find(|(name, _)| name == "GOFLAGS").map(|(_, value)| value.trim().to_owned());
    match flags.filter(|flags| !flags.is_empty()) {
        Some(flags) => format!("{flags} {READONLY_FLAG}"),
        None => READONLY_FLAG.to_owned(),
    }
}

pub mod commands;

#[cfg(test)]
mod tests;

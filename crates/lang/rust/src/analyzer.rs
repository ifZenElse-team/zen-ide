use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};
use zen_i18n::t;

pub const PROJECT_FILE: &str = ".zen/rust-analyzer.json";

/// rust-analyzer's settings as a project or the user set them, and what it starts with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AnalyzerSettings {
    /// The defaults of the Rust plug-in, tuned for large workspaces.
    pub defaults: Value,
    /// The user's, kept in the IDE (`toolchain-rust.json`).
    pub ide: Value,
    /// The project's file, when a project was named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_file: Option<PathBuf>,
    /// The project's own, from `projectFile` (empty when it has none).
    pub project: Value,
    /// What rust-analyzer starts with in that project: the project's over the user's over the defaults.
    pub effective: Value,
    /// Why the project's file was not read, when it is broken.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(base), Value::Object(over)) => {
            for (key, value) in over {
                merge(base.entry(key.clone()).or_insert(Value::Null), value);
            }
        }
        (base, over) => *base = over.clone(),
    }
}

pub fn effective(defaults: &Value, ide: &Value, project: &Value) -> Value {
    let mut settings = if defaults.is_object() { defaults.clone() } else { Value::Object(Map::new()) };
    for over in [ide, project].into_iter().filter(|over| over.is_object()) {
        merge(&mut settings, over);
    }
    settings
}

pub fn get<'a>(settings: &'a Value, key: &str) -> Option<&'a Value> {
    key.split('.').try_fold(settings, |node, name| node.get(name))
}

pub fn set(settings: &mut Value, key: &str, value: Option<Value>) -> Result<(), String> {
    let names: Vec<&str> = key.split('.').collect();
    if names.iter().any(|name| name.trim().is_empty()) {
        return Err(t!("rust-analyzer-error-key", key = key));
    }
    match value {
        Some(value) => insert(settings, &names, value),
        None => remove(settings, &names),
    }
    Ok(())
}

fn insert(node: &mut Value, names: &[&str], value: Value) {
    if !node.is_object() {
        *node = Value::Object(Map::new());
    }
    let Value::Object(object) = node else { return };
    match names {
        [] => {}
        [last] => {
            object.insert((*last).to_owned(), value);
        }
        [first, rest @ ..] => insert(object.entry((*first).to_owned()).or_insert(Value::Null), rest, value),
    }
}

fn remove(node: &mut Value, names: &[&str]) {
    let Some(object) = node.as_object_mut() else { return };
    match names {
        [] => {}
        [last] => {
            object.remove(*last);
        }
        [first, rest @ ..] => {
            if let Some(child) = object.get_mut(*first) {
                remove(child, rest);
                if child.as_object().is_some_and(Map::is_empty) {
                    object.remove(*first);
                }
            }
        }
    }
}

pub fn coerce(value: Value) -> Value {
    if let Value::String(text) = &value
        && let Ok(parsed @ (Value::Bool(_) | Value::Number(_))) = serde_json::from_str::<Value>(text)
    {
        return parsed;
    }
    value
}

pub fn read_project(root: &Path) -> Result<Value, String> {
    let file = root.join(PROJECT_FILE);
    let text = match std::fs::read_to_string(&file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Value::Object(Map::new())),
        Err(error) => return Err(error.to_string()),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(value @ Value::Object(_)) => Ok(value),
        Ok(_) => Err(t!("rust-analyzer-error-file", file = file.display().to_string(), error = "{ }")),
        Err(error) => Err(t!("rust-analyzer-error-file", file = file.display().to_string(), error = error.to_string())),
    }
}

pub fn write_project(root: &Path, settings: &Value) -> Result<(), String> {
    let file = root.join(PROJECT_FILE);
    let saving = |error: std::io::Error| {
        t!("rust-toolchain-error-save", file = file.display().to_string(), error = error.to_string())
    };
    if settings.as_object().is_none_or(Map::is_empty) {
        return match std::fs::remove_file(&file) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(saving(error)),
            _ => Ok(()),
        };
    }
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(saving)?;
    }
    let mut text = serde_json::to_string_pretty(settings).unwrap_or_default();
    text.push('\n');
    std::fs::write(&file, text).map_err(saving)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_project_goes_over_the_user_over_the_defaults() {
        let defaults = json!({ "checkOnSave": true, "check": { "command": "check", "workspace": false } });
        let ide = json!({ "check": { "command": "clippy" }, "lru": { "capacity": 256 } });
        let project = json!({ "check": { "workspace": true }, "files": { "exclude": ["vendor"] } });
        assert_eq!(
            effective(&defaults, &ide, &project),
            json!({
                "checkOnSave": true,
                "check": { "command": "clippy", "workspace": true },
                "lru": { "capacity": 256 },
                "files": { "exclude": ["vendor"] }
            })
        );
        assert_eq!(effective(&Value::Null, &json!({}), &json!({})), json!({}));
        assert_eq!(effective(&defaults, &Value::Null, &Value::Null), defaults, "nothing set, the defaults");
    }

    #[test]
    fn dotted_keys_are_set_read_and_removed() {
        let mut settings = json!({});
        set(&mut settings, "check.workspace", Some(json!(false))).unwrap();
        set(&mut settings, "cachePriming.enable", Some(json!(true))).unwrap();
        assert_eq!(settings, json!({ "check": { "workspace": false }, "cachePriming": { "enable": true } }));
        assert_eq!(get(&settings, "check.workspace"), Some(&json!(false)));
        assert_eq!(get(&settings, "check.command"), None);
        set(&mut settings, "check.workspace", None).unwrap();
        assert_eq!(settings, json!({ "cachePriming": { "enable": true } }), "an emptied object goes too");
        set(&mut settings, "cachePriming.enable.deeper", Some(json!(1))).unwrap();
        assert_eq!(get(&settings, "cachePriming.enable.deeper"), Some(&json!(1)));
        zen_i18n::register(crate::LOCALES);
        assert!(set(&mut settings, "check..workspace", Some(json!(1))).is_err());
        assert!(set(&mut settings, "", Some(json!(1))).is_err());
    }

    #[test]
    fn the_page_sends_booleans_and_numbers_as_text() {
        assert_eq!(coerce(json!("false")), json!(false));
        assert_eq!(coerce(json!("256")), json!(256));
        assert_eq!(coerce(json!("clippy")), json!("clippy"));
        assert_eq!(coerce(json!(["target"])), json!(["target"]));
    }

    #[test]
    fn the_project_file_is_read_written_and_removed_when_empty() {
        zen_i18n::register(crate::LOCALES);
        let root = tempfile::tempdir().unwrap();
        assert_eq!(read_project(root.path()).unwrap(), json!({}), "no file, no settings");
        let settings = json!({ "check": { "workspace": false } });
        write_project(root.path(), &settings).unwrap();
        assert_eq!(read_project(root.path()).unwrap(), settings);
        assert!(std::fs::read_to_string(root.path().join(PROJECT_FILE)).unwrap().ends_with("}\n"));
        write_project(root.path(), &json!({})).unwrap();
        assert!(!root.path().join(PROJECT_FILE).exists());
        std::fs::write(root.path().join(PROJECT_FILE), "[1]").unwrap();
        assert!(read_project(root.path()).is_err());
        std::fs::write(root.path().join(PROJECT_FILE), "{ broken").unwrap();
        assert!(read_project(root.path()).unwrap_err().contains(PROJECT_FILE));
    }
}

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

pub const PUBLIC_FILE: &str = "http-client.env.json";

pub const PRIVATE_FILE: &str = "http-client.private.env.json";

pub const SHARED: &str = "$shared";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvError {
    pub file: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environments {
    pub folder: Option<PathBuf>,
    pub public: BTreeMap<String, BTreeMap<String, String>>,
    pub private: BTreeMap<String, BTreeMap<String, String>>,
}

impl Environments {
    pub fn find(file: &Path, root: Option<&Path>) -> Result<Self, EnvError> {
        let mut folder = file.parent();
        while let Some(current) = folder {
            let public = current.join(PUBLIC_FILE);
            let private = current.join(PRIVATE_FILE);
            if public.is_file() || private.is_file() {
                return Ok(Self { folder: Some(current.to_owned()), public: read(&public)?, private: read(&private)? });
            }
            if root.is_none_or(|root| current == root || !current.starts_with(root)) {
                break;
            }
            folder = current.parent();
        }
        Ok(Self::default())
    }

    pub fn from_texts(public: Option<&str>, private: Option<&str>) -> Result<Self, String> {
        Ok(Self {
            folder: None,
            public: public.map(environments).transpose()?.unwrap_or_default(),
            private: private.map(environments).transpose()?.unwrap_or_default(),
        })
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> =
            self.public.keys().chain(self.private.keys()).filter(|name| *name != SHARED).cloned().collect();
        names.sort();
        names.dedup();
        names
    }

    pub fn variables(&self, name: Option<&str>) -> BTreeMap<String, String> {
        let mut variables = BTreeMap::new();
        for file in [&self.public, &self.private] {
            if let Some(shared) = file.get(SHARED) {
                variables.extend(shared.clone());
            }
        }
        if let Some(name) = name {
            for file in [&self.public, &self.private] {
                if let Some(own) = file.get(name) {
                    variables.extend(own.clone());
                }
            }
        }
        variables
    }

    pub fn private_names(&self, name: Option<&str>) -> BTreeSet<String> {
        let mut private = BTreeSet::new();
        let mut layers = vec![(SHARED, &self.public, false), (SHARED, &self.private, true)];
        if let Some(name) = name {
            layers.extend([(name, &self.public, false), (name, &self.private, true)]);
        }
        for (environment, file, secret) in layers {
            for key in file.get(environment).into_iter().flat_map(BTreeMap::keys) {
                if secret {
                    private.insert(key.clone());
                } else {
                    private.remove(key);
                }
            }
        }
        private
    }
}

fn read(file: &Path) -> Result<BTreeMap<String, BTreeMap<String, String>>, EnvError> {
    match std::fs::read_to_string(file) {
        Ok(text) => environments(&text).map_err(|message| EnvError { file: file.to_owned(), message }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(EnvError { file: file.to_owned(), message: error.to_string() }),
    }
}

fn environments(text: &str) -> Result<BTreeMap<String, BTreeMap<String, String>>, String> {
    let value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let Value::Object(object) = value else {
        return Err("an environment file holds one JSON object".to_owned());
    };
    let mut found = BTreeMap::new();
    for (name, value) in object {
        let mut variables = BTreeMap::new();
        if let Value::Object(fields) = value {
            for (key, value) in fields {
                flatten(&key, &value, &mut variables);
            }
        }
        found.insert(name, variables);
    }
    Ok(found)
}

fn flatten(key: &str, value: &Value, into: &mut BTreeMap<String, String>) {
    match value {
        Value::String(text) => {
            into.insert(key.to_owned(), text.clone());
        }
        Value::Object(fields) => {
            for (inner, value) in fields {
                flatten(&format!("{key}.{inner}"), value, into);
            }
        }
        Value::Null => {}
        other => {
            into.insert(key.to_owned(), other.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_own_and_private_values() {
        let environments = Environments::from_texts(
            Some(r#"{"$shared": {"version": "v1", "host": "shared"}, "dev": {"host": "localhost:8080", "port": 8080, "debug": true, "auth": {"user": "ann"}}, "prod": {"host": "example.test"}}"#),
            Some(r#"{"dev": {"token": "secret", "host": "127.0.0.1"}, "local": {}}"#),
        )
        .unwrap();
        assert_eq!(environments.names(), ["dev", "local", "prod"]);
        let dev = environments.variables(Some("dev"));
        assert_eq!(dev["host"], "127.0.0.1", "private values win");
        assert_eq!(dev["port"], "8080");
        assert_eq!(dev["debug"], "true");
        assert_eq!(dev["auth.user"], "ann");
        assert_eq!(dev["token"], "secret");
        assert_eq!(dev["version"], "v1");
        let private: Vec<String> = environments.private_names(Some("dev")).into_iter().collect();
        assert_eq!(private, ["host", "token"]);
        assert!(environments.private_names(Some("prod")).is_empty());
        let none = environments.variables(None);
        assert_eq!(none.get("host").map(String::as_str), Some("shared"));
        assert!(!none.contains_key("token"));
    }

    #[test]
    fn bad_files_are_errors() {
        assert!(Environments::from_texts(Some("[1]"), None).is_err());
        assert!(Environments::from_texts(None, Some("{")).is_err());
    }

    #[test]
    fn found_in_the_nearest_folder_up_to_the_root() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("api/v1");
        std::fs::create_dir_all(&nested).unwrap();
        let file = nested.join("items.http");
        assert_eq!(Environments::find(&file, Some(root.path())).unwrap(), Environments::default());
        std::fs::write(root.path().join(PUBLIC_FILE), r#"{"dev": {"host": "root"}}"#).unwrap();
        let found = Environments::find(&file, Some(root.path())).unwrap();
        assert_eq!(found.folder.as_deref(), Some(root.path()));
        assert_eq!(found.variables(Some("dev"))["host"], "root");
        std::fs::write(root.path().join("api").join(PRIVATE_FILE), r#"{"dev": {"host": "api"}}"#).unwrap();
        let found = Environments::find(&file, Some(root.path())).unwrap();
        assert_eq!(found.folder, Some(root.path().join("api")), "the nearest folder wins");
        assert_eq!(found.variables(Some("dev"))["host"], "api");
        let outside = tempfile::tempdir().unwrap();
        let alone = outside.path().join("a.http");
        assert_eq!(Environments::find(&alone, Some(root.path())).unwrap(), Environments::default());
        std::fs::write(root.path().join(PUBLIC_FILE), "{").unwrap();
        let error = Environments::find(&root.path().join("x.http"), Some(root.path())).unwrap_err();
        assert_eq!(error.file, root.path().join(PUBLIC_FILE));
    }
}

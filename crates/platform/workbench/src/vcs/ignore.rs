use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::{changed, repository};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(AddToIgnore)
}

/// Files and folders to ignore.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct IgnoreParams {
    /// Absolute paths of files and folders inside the repository.
    pub paths: Vec<PathBuf>,
}

/// What was added.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Ignored {
    /// The `.gitignore` file written.
    pub file: PathBuf,
    /// The lines added (lines already there are not added again).
    pub added: Vec<String>,
}

pub struct AddToIgnore;

impl Command for AddToIgnore {
    type Params = IgnoreParams;
    type Output = Ignored;
    const ID: &'static str = "vcs.ignore.add";
    const TITLE: &'static str = "Add to .gitignore";
    const DESCRIPTION: &'static str = "Adds files and folders to the `.gitignore` at the root of the repository, each as a line \
         anchored to the root (`/build/`, `/notes.txt`); lines already there are not repeated. The \
         file is created when there is none.";

    fn run(&self, session: &mut Session, params: IgnoreParams) -> Result<Ignored, CommandError> {
        let repository = repository(session)?;
        let root = repository.root().to_owned();
        let file = root.join(".gitignore");
        let existing = std::fs::read_to_string(&file).unwrap_or_default();
        let mut lines: Vec<String> = Vec::new();
        for path in &params.paths {
            let line = ignore_line(&root, path).ok_or_else(|| {
                CommandError::new(t!("vcs-error-outside-repository", path = path.display().to_string()))
            })?;
            if !existing.lines().any(|known| known.trim() == line) && !lines.contains(&line) {
                lines.push(line);
            }
        }
        if !lines.is_empty() {
            crate::disk::edit_file(session, &file, |text| {
                let mut text = text.to_owned();
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                for line in &lines {
                    text.push_str(line);
                    text.push('\n');
                }
                Some(text)
            })?;
            changed(session);
        }
        Ok(Ignored { file, added: lines })
    }
}

fn ignore_line(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|part| match part {
            Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if parts.is_empty() || parts.first().is_some_and(|first| first == ".git") {
        return None;
    }
    let escaped: Vec<String> = parts
        .iter()
        .map(|part| {
            let mut out = String::new();
            for (index, c) in part.chars().enumerate() {
                if matches!(c, '*' | '?' | '[' | '\\') || (index == 0 && matches!(c, '#' | '!')) {
                    out.push('\\');
                }
                out.push(c);
            }
            out
        })
        .collect();
    let slash = if path.is_dir() { "/" } else { "" };
    Some(format!("/{}{slash}", escaped.join("/")))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    #[test]
    fn files_and_folders_go_into_the_root_gitignore_once() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::create_dir_all(root.join("build/out")).unwrap();
        std::fs::write(root.join("notes #1.txt"), "n").unwrap();
        std::fs::write(root.join(".gitignore"), "*.log").unwrap();
        let added = registry
            .dispatch(
                &mut session,
                "vcs.ignore.add",
                json!({ "paths": [root.join("build"), root.join("notes #1.txt")] }),
            )
            .unwrap();
        assert_eq!(added, json!({ "file": root.join(".gitignore"), "added": ["/build/", "/notes #1.txt"] }));
        assert_eq!(std::fs::read_to_string(root.join(".gitignore")).unwrap(), "*.log\n/build/\n/notes #1.txt\n");
        let again =
            registry.dispatch(&mut session, "vcs.ignore.add", json!({ "paths": [root.join("build")] })).unwrap();
        assert_eq!(again["added"], json!([]));
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let status = registry.dispatch(&mut session, "vcs.status.list", json!({})).unwrap();
        let listed: Vec<&str> =
            status["changes"].as_array().unwrap().iter().filter_map(|change| change["relative"].as_str()).collect();
        assert!(!listed.iter().any(|path| path.starts_with("build") || path.starts_with("notes")), "{listed:?}");
        assert!(registry.dispatch(&mut session, "vcs.ignore.add", json!({ "paths": ["/elsewhere/x"] })).is_err());
    }

    #[test]
    fn pattern_characters_are_escaped() {
        let root = Path::new("/r");
        assert_eq!(ignore_line(root, Path::new("/r/a*b/[x].txt")).as_deref(), Some("/a\\*b/\\[x].txt"));
        assert_eq!(ignore_line(root, Path::new("/r/!x")).as_deref(), Some("/\\!x"));
        assert_eq!(ignore_line(root, Path::new("/r")), None);
        assert_eq!(ignore_line(root, Path::new("/r/.git/config")), None);
    }
}

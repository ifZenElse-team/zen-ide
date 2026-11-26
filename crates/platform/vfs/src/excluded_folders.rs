use std::path::{Component, Path};
use std::sync::{PoisonError, RwLock};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ExcludedFolder {
    pub name: String,
    pub beside: String,
}

impl ExcludedFolder {
    pub fn new(name: &str, beside: &str) -> Self {
        Self { name: name.to_owned(), beside: beside.to_owned() }
    }
}

pub const DEFAULT: &[(&str, &str)] = &[("node_modules", "package.json")];

static DECLARED: RwLock<Vec<ExcludedFolder>> = RwLock::new(Vec::new());

pub fn declare(folder: ExcludedFolder) {
    let mut declared = DECLARED.write().unwrap_or_else(PoisonError::into_inner);
    if !declared.contains(&folder) {
        declared.push(folder);
    }
}

pub fn declared() -> Vec<ExcludedFolder> {
    let mut all: Vec<ExcludedFolder> = DEFAULT.iter().map(|(name, beside)| ExcludedFolder::new(name, beside)).collect();
    for folder in DECLARED.read().unwrap_or_else(PoisonError::into_inner).iter() {
        if !all.contains(folder) {
            all.push(folder.clone());
        }
    }
    all
}

pub fn is_candidate(name: &str) -> bool {
    DEFAULT.iter().any(|(declared, _)| *declared == name)
        || DECLARED.read().unwrap_or_else(PoisonError::into_inner).iter().any(|folder| folder.name == name)
}

pub fn is_excluded(path: &Path) -> bool {
    let Some(name) = path.file_name().map(|name| name.to_string_lossy()) else { return false };
    if !is_candidate(&name) {
        return false;
    }
    let Some(parent) = path.parent() else { return false };
    declared().iter().any(|folder| folder.name == name && parent.join(&folder.beside).is_file())
}

pub fn excludes(root: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(root) else { return false };
    let mut folder = root.to_owned();
    for component in relative.components() {
        let Component::Normal(name) = component else { continue };
        folder.push(name);
        if is_candidate(&name.to_string_lossy()) && is_excluded(&folder) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_is_excluded_only_beside_the_file_of_its_kind() {
        declare(ExcludedFolder::new("zt-out", "zt.toml"));
        declare(ExcludedFolder::new("zt-out", "zt.toml"));
        assert_eq!(declared().iter().filter(|folder| folder.name == "zt-out").count(), 1);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for folder in ["a/zt-out/deep", "b/zt-out", "c/node_modules/x", "d/node_modules"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(root.join("a/zt.toml"), "").unwrap();
        std::fs::write(root.join("c/package.json"), "{}").unwrap();
        assert!(is_excluded(&root.join("a/zt-out")));
        assert!(!is_excluded(&root.join("b/zt-out")), "no zt.toml beside it");
        assert!(is_excluded(&root.join("c/node_modules")), "a default");
        assert!(!is_excluded(&root.join("d/node_modules")));
        assert!(!is_excluded(&root.join("a")));
        assert!(excludes(root, &root.join("a/zt-out/deep/file.rs")));
        assert!(excludes(root, &root.join("a/zt-out")));
        assert!(!excludes(root, &root.join("b/zt-out/file.rs")));
        assert!(excludes(root, &root.join("c/node_modules/x/index.js")));
        assert!(!excludes(root, &root.join("d/node_modules/x/index.js")));
        assert!(!excludes(&root.join("a/zt-out"), &root.join("a/zt-out/deep")), "only below the root");
        assert!(!excludes(root, Path::new("/elsewhere/zt-out")));
    }
}

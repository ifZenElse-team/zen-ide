use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

#[derive(Debug)]
struct Level {
    dot_ignore: Gitignore,
    git_ignore: Gitignore,
    git_exclude: Gitignore,
    has_git: bool,
}

impl Level {
    fn read(folder: &Path) -> Self {
        let has_git = folder.join(".git").exists();
        let exclude = if has_git { Some(folder.join(".git/info/exclude")) } else { None };
        Self {
            dot_ignore: matcher(folder, Some(folder.join(".ignore"))),
            git_ignore: matcher(folder, Some(folder.join(".gitignore"))),
            git_exclude: matcher(folder, exclude),
            has_git,
        }
    }
}

fn matcher(folder: &Path, file: Option<PathBuf>) -> Gitignore {
    let Some(file) = file.filter(|file| file.is_file()) else { return Gitignore::empty() };
    let mut builder = GitignoreBuilder::new(folder);
    let _ = builder.add(file);
    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

fn verdict<T>(found: Match<T>) -> Option<bool> {
    match found {
        Match::None => None,
        Match::Ignore(_) => Some(true),
        Match::Whitelist(_) => Some(false),
    }
}

#[derive(Debug)]
pub(crate) struct GitIgnores {
    root: PathBuf,
    global: Gitignore,
    levels: Mutex<HashMap<PathBuf, Arc<Level>>>,
}

impl GitIgnores {
    pub(crate) fn new(root: &Path) -> Self {
        Self { root: root.to_owned(), global: GitignoreBuilder::new(root).build_global().0, levels: Mutex::default() }
    }

    pub(crate) fn forget(&self) {
        self.levels.lock().unwrap_or_else(PoisonError::into_inner).clear();
    }

    pub(crate) fn ignores(&self, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(&self.root) else { return false };
        let parts: Vec<_> = relative.components().collect();
        let mut current = self.root.clone();
        for (at, part) in parts.iter().enumerate() {
            current.push(part);
            let is_dir = at + 1 < parts.len() || current.is_dir();
            if self.matched(&current, is_dir) {
                return true;
            }
        }
        false
    }

    fn level(&self, folder: &Path) -> Arc<Level> {
        let mut levels = self.levels.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(levels.entry(folder.to_owned()).or_insert_with(|| Arc::new(Level::read(folder))))
    }

    fn matched(&self, path: &Path, is_dir: bool) -> bool {
        let (mut dot_ignore, mut git_ignore, mut git_exclude) = (None, None, None);
        let mut saw_git = false;
        for folder in path.ancestors().skip(1) {
            let level = self.level(folder);
            dot_ignore = dot_ignore.or_else(|| verdict(level.dot_ignore.matched(path, is_dir)));
            if !saw_git {
                git_ignore = git_ignore.or_else(|| verdict(level.git_ignore.matched(path, is_dir)));
                git_exclude = git_exclude.or_else(|| verdict(level.git_exclude.matched(path, is_dir)));
            }
            saw_git = saw_git || level.has_git;
        }
        dot_ignore
            .or(git_ignore)
            .or(git_exclude)
            .or_else(|| verdict(self.global.matched(path, is_dir)))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_ignored_as_the_walk_ignores_them() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("p");
        for folder in ["target/debug", "src/gen", "keep/target", ".git/info", "logs"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(dir.path().join(".gitignore"), "*.tmp\n").unwrap();
        std::fs::write(root.join(".gitignore"), "/target\n*.log\n!keep.log\nsrc/gen/\n").unwrap();
        std::fs::write(root.join("logs/.gitignore"), "!special.log\n").unwrap();
        std::fs::write(root.join(".git/info/exclude"), "local.txt\n").unwrap();
        std::fs::write(root.join(".ignore"), "scratch/\n").unwrap();
        let ignores = GitIgnores::new(&root);
        assert!(ignores.ignores(&root.join("target")));
        assert!(ignores.ignores(&root.join("target/debug/deps/lib.rlib")), "inside an ignored folder");
        assert!(!ignores.ignores(&root.join("keep/target/x.rs")), "an anchored pattern");
        assert!(ignores.ignores(&root.join("a.log")));
        assert!(!ignores.ignores(&root.join("keep.log")), "a negation");
        assert!(!ignores.ignores(&root.join("logs/special.log")), "a deeper file wins");
        assert!(ignores.ignores(&root.join("src/gen/x.go")), "a folder pattern");
        assert!(!ignores.ignores(&root.join("src/main.go")));
        assert!(ignores.ignores(&root.join("local.txt")), ".git/info/exclude");
        assert!(ignores.ignores(&root.join("scratch/x")), ".ignore");
        assert!(!ignores.ignores(&root.join("x.tmp")), "not a .gitignore above the repository");
        let inner = GitIgnores::new(&root.join("logs"));
        assert!(inner.ignores(&root.join("logs/a.log")), "a .gitignore above the project, in its repository");
        assert!(!inner.ignores(&root.join("logs/special.log")));
        assert!(!ignores.ignores(Path::new("/elsewhere/target/x")), "outside the project");

        std::fs::write(root.join(".gitignore"), "").unwrap();
        assert!(ignores.ignores(&root.join("target/x")), "read once");
        ignores.forget();
        assert!(!ignores.ignores(&root.join("target/x")), "read again");
    }
}

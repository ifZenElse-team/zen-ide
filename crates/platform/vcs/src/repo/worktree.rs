use std::path::{Path, PathBuf};

use super::Repository;
use crate::git::{self, Progress};
use crate::{Submodule, VcsError, Worktree};

fn folder_argument(path: &Path) -> Result<String, VcsError> {
    let path = std::path::absolute(path).map_err(|error| VcsError::Git(error.to_string()))?;
    Ok(path.to_string_lossy().into_owned())
}

fn same_folder(a: &Path, b: &Path) -> bool {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_owned());
    a == b || canonical(a) == canonical(b)
}

impl Repository {
    pub fn worktrees(&self) -> Result<Vec<Worktree>, VcsError> {
        let output = git::run(&self.root, &["worktree", "list", "--porcelain", "-z"], None)?;
        let mut worktrees = crate::parse_worktree_list(&output.stdout);
        for worktree in &mut worktrees {
            if let Ok(path) = worktree.path.canonicalize() {
                worktree.path = path;
            }
        }
        Ok(worktrees)
    }

    pub fn worktree(&self, path: &Path) -> Result<Worktree, VcsError> {
        self.worktrees()?
            .into_iter()
            .find(|worktree| same_folder(&worktree.path, path))
            .ok_or_else(|| VcsError::Git(format!("'{}' is not a worktree of this repository", path.display())))
    }

    pub fn add_worktree(&self, path: &Path, revision: &str, new_branch: Option<&str>) -> Result<Worktree, VcsError> {
        let revision = revision.trim();
        if revision.is_empty() || revision.starts_with('-') {
            return Err(VcsError::Git(format!("invalid revision '{revision}'")));
        }
        let folder = folder_argument(path)?;
        let mut args = vec!["worktree", "add", "--quiet"];
        if let Some(name) = new_branch {
            super::valid_name(name, "branch")?;
            args.extend(["-b", name]);
        }
        args.extend(["--", folder.as_str(), revision]);
        git::run(&self.root, &args, None)?;
        self.worktree(Path::new(&folder))
    }

    pub fn remove_worktree(&self, path: &Path, force: bool) -> Result<(), VcsError> {
        let worktree = self.worktree(path)?;
        if worktree.main {
            return Err(VcsError::Git("the main worktree cannot be removed".to_owned()));
        }
        let folder = worktree.path.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.extend(["--", folder.as_str()]);
        git::run(&self.root, &args, None).map(drop)
    }

    pub fn lock_worktree(&self, path: &Path, lock: bool, reason: Option<&str>) -> Result<(), VcsError> {
        let worktree = self.worktree(path)?;
        if worktree.main {
            return Err(VcsError::Git("the main worktree cannot be locked".to_owned()));
        }
        let folder = worktree.path.to_string_lossy().into_owned();
        let reason =
            reason.map(str::trim).filter(|reason| !reason.is_empty()).map(|reason| format!("--reason={reason}"));
        let mut args = vec!["worktree", if lock { "lock" } else { "unlock" }];
        if lock && let Some(reason) = &reason {
            args.push(reason.as_str());
        }
        args.extend(["--", folder.as_str()]);
        git::run(&self.root, &args, None).map(drop)
    }

    pub fn worktree_changes(path: &Path) -> Result<usize, VcsError> {
        let repository = Repository::discover(path).ok_or(VcsError::NotARepository)?;
        Ok(repository.status()?.changes.len())
    }

    pub fn worktrees_stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let Ok(entries) = std::fs::read_dir(self.common_dir.join("worktrees")) else { return 0 };
        let mut folders: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
        folders.sort();
        for folder in folders {
            for name in ["HEAD", "index", "locked", "gitdir"] {
                let path = folder.join(name);
                path.hash(&mut hasher);
                if let Ok(metadata) = std::fs::metadata(&path) {
                    metadata.modified().ok().hash(&mut hasher);
                    metadata.len().hash(&mut hasher);
                }
            }
        }
        hasher.finish()
    }

    pub fn submodules(&self) -> Result<Vec<Submodule>, VcsError> {
        if !self.root.join(".gitmodules").is_file() {
            return Ok(Vec::new());
        }
        let output = git::run(&self.root, &["submodule", "status"], None)?;
        let mut submodules = crate::parse_submodule_status(&output.stdout);
        let config = git::run(&self.root, &["config", "--file", ".gitmodules", "--null", "--list"], None)
            .map(|output| output.stdout)
            .unwrap_or_default();
        let mut entries: Vec<(String, String, String)> = Vec::new();
        for entry in config.split('\0').filter(|entry| !entry.is_empty()) {
            let (key, value) = entry.split_once('\n').unwrap_or((entry, ""));
            let Some(rest) = key.strip_prefix("submodule.") else { continue };
            let Some((name, field)) = rest.rsplit_once('.') else { continue };
            entries.push((name.to_owned(), field.to_owned(), value.to_owned()));
        }
        for submodule in &mut submodules {
            if let Some((name, ..)) =
                entries.iter().find(|(_, field, value)| field == "path" && *value == submodule.path)
            {
                submodule.name = name.clone();
            }
            submodule.url = entries
                .iter()
                .find(|(name, field, _)| *name == submodule.name && field == "url")
                .map(|(.., url)| url.clone());
            if submodule.recorded.is_none() {
                let spec = &super::literal(&[submodule.path.as_str()])[0];
                let staged = git::run(&self.root, &["ls-files", "--stage", "--", spec], None)
                    .map(|output| output.stdout)
                    .unwrap_or_default();
                submodule.recorded = staged
                    .lines()
                    .find(|line| line.starts_with("160000 "))
                    .and_then(|line| line.split_whitespace().nth(1))
                    .map(str::to_owned);
            }
        }
        Ok(submodules)
    }

    pub fn update_submodules(
        &self,
        paths: &[String],
        init: bool,
        recursive: bool,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<(), VcsError> {
        let mut args = vec!["submodule", "update", "--progress"];
        if init {
            args.push("--init");
        }
        if recursive {
            args.push("--recursive");
        }
        args.push("--");
        let specs = super::literal(paths);
        args.extend(specs.iter().map(String::as_str));
        git::run_with_progress(&self.root, &args, None, progress).map(drop)
    }

    pub fn submodule(&self, path: &str) -> Result<Submodule, VcsError> {
        self.submodules()?
            .into_iter()
            .find(|submodule| submodule.path == path.trim_end_matches('/'))
            .ok_or_else(|| VcsError::Git(format!("'{path}' is not a submodule of this repository")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SubmoduleState;

    const ENVIRONMENT: [(&str, &str); 9] = [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_AUTHOR_NAME", "Ada"),
        ("GIT_AUTHOR_EMAIL", "ada@example.com"),
        ("GIT_COMMITTER_NAME", "Ada"),
        ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ("GIT_CONFIG_COUNT", "1"),
        ("GIT_CONFIG_KEY_0", "protocol.file.allow"),
        ("GIT_CONFIG_VALUE_0", "always"),
    ];

    fn git(dir: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git").current_dir(dir).args(args).envs(ENVIRONMENT).output().unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn repository(dir: &Path) -> Repository {
        git::set_environment(&ENVIRONMENT);
        git(dir, &["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        git(dir, &["add", "a.txt"]);
        git(dir, &["commit", "-q", "-m", "First"]);
        Repository::discover(dir).unwrap()
    }

    #[test]
    fn worktrees_are_listed_made_locked_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("main");
        std::fs::create_dir(&root).unwrap();
        let repository = repository(&root);
        let only = repository.worktrees().unwrap();
        assert_eq!(only.len(), 1);
        assert!(only[0].main);
        assert_eq!((only[0].path.as_path(), only[0].branch.as_deref()), (root.as_path(), Some("main")));
        let stamp = repository.worktrees_stamp();

        git(&root, &["branch", "existing"]);
        let made = repository.add_worktree(&base.join("feature"), "main", Some("feature")).unwrap();
        assert_eq!((made.branch.as_deref(), made.main), (Some("feature"), false));
        assert_eq!(made.head, repository.head().unwrap().commit);
        repository.add_worktree(&base.join("existing"), "existing", None).unwrap();
        assert!(repository.add_worktree(&base.join("again"), "existing", None).is_err(), "checked out elsewhere");
        assert!(repository.add_worktree(&base.join("bad"), "--force", None).is_err(), "never an option");
        assert_ne!(repository.worktrees_stamp(), stamp, "a worktree changes it");
        let all = repository.worktrees().unwrap();
        let branches: Vec<Option<&str>> = all.iter().map(|worktree| worktree.branch.as_deref()).collect();
        assert_eq!(branches, [Some("main"), Some("existing"), Some("feature")], "the linked ones by folder");

        assert_eq!(Repository::worktree_changes(&base.join("feature")).unwrap(), 0);
        std::fs::write(base.join("feature/a.txt"), "two\n").unwrap();
        std::fs::write(base.join("feature/new.txt"), "new\n").unwrap();
        assert_eq!(Repository::worktree_changes(&base.join("feature")).unwrap(), 2);
        assert_eq!(Repository::worktree_changes(&root).unwrap(), 0);

        repository.lock_worktree(&base.join("feature"), true, Some("on a disk")).unwrap();
        assert_eq!(repository.worktree(&base.join("feature")).unwrap().locked.as_deref(), Some("on a disk"));
        assert!(repository.remove_worktree(&base.join("feature"), true).is_err(), "locked");
        repository.lock_worktree(&base.join("feature"), false, None).unwrap();
        assert_eq!(repository.worktree(&base.join("feature")).unwrap().locked, None);
        assert!(repository.remove_worktree(&base.join("feature"), false).is_err(), "it has changes");
        repository.remove_worktree(&base.join("feature"), true).unwrap();
        assert!(!base.join("feature").exists());
        assert!(repository.remove_worktree(&root, true).is_err(), "never the main one");
        assert!(repository.lock_worktree(&root, true, None).is_err());
        assert!(repository.worktree(&base.join("nowhere")).is_err());
        assert!(git(&root, &["branch", "--list", "feature"]).contains("feature"));
    }

    #[test]
    fn submodules_are_listed_with_their_state_and_updated() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        let library = base.join("library");
        std::fs::create_dir(&library).unwrap();
        repository(&library);
        std::fs::write(library.join("a.txt"), "two\n").unwrap();
        git(&library, &["commit", "-q", "-am", "Second"]);
        let root = base.join("app");
        std::fs::create_dir(&root).unwrap();
        let repository = repository(&root);
        assert!(repository.submodules().unwrap().is_empty(), "none yet");
        git(&root, &["submodule", "add", "-q", library.to_str().unwrap(), "libs/lib"]);
        git(&root, &["submodule", "add", "-q", "--name", "other-name", library.to_str().unwrap(), "other"]);
        git(&root, &["commit", "-q", "-m", "Submodules"]);
        let recorded = git(&library, &["rev-parse", "HEAD"]).trim().to_owned();

        let listed = repository.submodules().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!((listed[0].path.as_str(), listed[0].state), ("libs/lib", SubmoduleState::Current));
        assert_eq!(listed[0].recorded.as_deref(), Some(recorded.as_str()));
        assert_eq!(listed[0].url.as_deref(), library.to_str());
        assert_eq!((listed[1].name.as_str(), listed[1].path.as_str()), ("other-name", "other"));

        git(&root.join("libs/lib"), &["checkout", "-q", "HEAD~1"]);
        git(&root, &["submodule", "deinit", "-q", "other"]);
        let moved = repository.submodule("libs/lib").unwrap();
        assert_eq!(moved.state, SubmoduleState::Moved);
        assert_eq!(moved.recorded.as_deref(), Some(recorded.as_str()));
        assert_ne!(moved.checked_out, moved.recorded);
        let other = repository.submodule("other").unwrap();
        assert_eq!((other.state, other.checked_out.as_deref()), (SubmoduleState::Uninitialized, None));

        repository.update_submodules(&["libs/lib".to_owned()], false, true, &mut |_| {}).unwrap();
        assert_eq!(repository.submodule("libs/lib").unwrap().state, SubmoduleState::Current);
        assert_eq!(repository.submodule("other").unwrap().state, SubmoduleState::Uninitialized, "only the one asked");
        repository.update_submodules(&[], true, true, &mut |_| {}).unwrap();
        assert_eq!(repository.submodule("other").unwrap().state, SubmoduleState::Current);
        assert!(root.join("other/a.txt").exists());
        assert!(repository.submodule("nothing").is_err());
    }

    #[test]
    fn worktree_and_submodule_listings_are_read() {
        let list = "worktree /repo\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
                    worktree /wt/a\0HEAD 2222222222222222222222222222222222222222\0detached\0locked\0\0\
                    worktree /wt/b\0HEAD 3333333333333333333333333333333333333333\0branch refs/heads/feat/b\0\
                    locked on a\ndisk\0prunable gitdir file points to non-existent location\0\0";
        let worktrees = crate::parse_worktree_list(list);
        assert_eq!(worktrees.len(), 3);
        assert!(worktrees[0].main && !worktrees[1].main);
        assert_eq!((worktrees[1].branch.as_deref(), worktrees[1].locked.as_deref()), (None, Some("")));
        assert_eq!(worktrees[2].branch.as_deref(), Some("feat/b"));
        assert_eq!(worktrees[2].locked.as_deref(), Some("on a\ndisk"));
        assert!(worktrees[2].prunable.is_some());
        let bare = crate::parse_worktree_list("worktree /repo.git\0bare\0\0worktree /wt\0HEAD 1234\0detached\0\0");
        assert_eq!(bare.len(), 1);
        assert_eq!(bare[0].path, PathBuf::from("/wt"));

        let status = " 1111111111111111111111111111111111111111 libs/a (heads/main)\n\
                      -2222222222222222222222222222222222222222 libs/b\n\
                      +3333333333333333333333333333333333333333 with space/c (v1.0-2-gabc)\n\
                      U0000000000000000000000000000000000000000 d\n";
        let submodules = crate::parse_submodule_status(status);
        let states: Vec<(&str, SubmoduleState)> =
            submodules.iter().map(|submodule| (submodule.path.as_str(), submodule.state)).collect();
        assert_eq!(
            states,
            [
                ("libs/a", SubmoduleState::Current),
                ("libs/b", SubmoduleState::Uninitialized),
                ("with space/c", SubmoduleState::Moved),
                ("d", SubmoduleState::Conflicted),
            ]
        );
        assert_eq!(submodules[1].recorded.as_deref(), Some("2222222222222222222222222222222222222222"));
        assert_eq!(submodules[2].checked_out.as_deref(), Some("3333333333333333333333333333333333333333"));
        assert_eq!(submodules[2].recorded, None, "filled in from the index");
    }

    #[test]
    fn a_folder_is_never_taken_for_an_option() {
        let folder = folder_argument(Path::new("--force")).unwrap();
        assert!(Path::new(&folder).is_absolute(), "{folder}");
    }
}

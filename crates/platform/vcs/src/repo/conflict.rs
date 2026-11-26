use std::path::Path;

use super::{Repository, error};
use crate::git;
use crate::{Applying, Blob, Conflicted, InProgress, VcsError};

const KEEP_MESSAGE: (&str, &str) = ("GIT_EDITOR", ":");

impl Repository {
    pub fn conflicts(&self) -> Result<Vec<Conflicted>, VcsError> {
        let output = git::run(&self.root, &["ls-files", "-u", "-z"], None)?;
        let mut files: Vec<Conflicted> = Vec::new();
        for record in output.stdout.split('\0').filter(|record| !record.is_empty()) {
            let Some((info, path)) = record.split_once('\t') else { continue };
            let mut fields = info.split(' ');
            let (Some(mode), Some(id), Some(stage)) = (fields.next(), fields.next(), fields.next()) else { continue };
            let Ok(mode) = u32::from_str_radix(mode, 8) else { continue };
            let blob = Some(Blob { mode, id: id.to_owned() });
            let file = match files.last_mut() {
                Some(last) if last.path == path => last,
                _ => {
                    files.push(Conflicted { path: path.to_owned(), ..Conflicted::default() });
                    files.last_mut().expect("just pushed")
                }
            };
            match stage {
                "1" => file.base = blob,
                "2" => file.ours = blob,
                "3" => file.theirs = blob,
                _ => {}
            }
        }
        Ok(files)
    }

    pub fn blob(&self, id: &str) -> Result<Vec<u8>, VcsError> {
        let repo = self.open()?;
        let id = gix::ObjectId::from_hex(id.as_bytes()).map_err(error)?;
        Ok(repo.find_object(id).map_err(error)?.detach().data)
    }

    pub fn resolve_with(&self, relative: &str, text: &[u8]) -> Result<(), VcsError> {
        let file = inside(&self.root, relative)?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(error)?;
        }
        std::fs::write(&file, text).map_err(|failure| VcsError::Git(format!("{}: {failure}", file.display())))?;
        git::run(&self.root, &["add", "--", &super::literal(&[relative])[0]], None).map(drop)
    }

    pub fn resolve_with_side(&self, relative: &str, ours: bool) -> Result<(), VcsError> {
        let file = inside(&self.root, relative)?;
        let conflicts = self.conflicts()?;
        let Some(conflict) = conflicts.iter().find(|conflict| conflict.path == relative) else {
            return Err(VcsError::Git(format!("{relative} has no conflicts")));
        };
        let side = if ours { &conflict.ours } else { &conflict.theirs };
        if side.is_some() {
            let option = if ours { "--ours" } else { "--theirs" };
            git::run(&self.root, &["checkout", option, "--", &super::literal(&[relative])[0]], None)?;
            git::run(&self.root, &["add", "--", &super::literal(&[relative])[0]], None).map(drop)
        } else {
            self.resolve_deleted(relative, &file)
        }
    }

    pub fn resolve_deleting(&self, relative: &str) -> Result<(), VcsError> {
        let file = inside(&self.root, relative)?;
        self.resolve_deleted(relative, &file)
    }

    fn resolve_deleted(&self, relative: &str, file: &Path) -> Result<(), VcsError> {
        match std::fs::remove_file(file) {
            Ok(()) => {}
            Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => {}
            Err(failure) => return Err(VcsError::Git(format!("{}: {failure}", file.display()))),
        }
        let spec = &super::literal(&[relative])[0];
        git::run(&self.root, &["rm", "--cached", "--quiet", "--ignore-unmatch", "--", spec], None).map(drop)
    }

    pub fn in_progress_info(&self) -> Option<InProgress> {
        let operation = self.in_progress()?;
        let read =
            |name: &str| std::fs::read_to_string(self.git_dir.join(name)).ok().map(|text| text.trim().to_owned());
        let first_line = |text: String| text.lines().next().map(str::to_owned).filter(|line| !line.is_empty());
        let (theirs, name) = match operation {
            Applying::Merge => {
                let message = read("MERGE_MSG").unwrap_or_default();
                (read("MERGE_HEAD").and_then(first_line), merged_name(&message, || self.pulled_remotes()))
            }
            Applying::CherryPick => (read("CHERRY_PICK_HEAD").and_then(first_line), None),
            Applying::Revert => (read("REVERT_HEAD").and_then(first_line), None),
            Applying::Rebase => (self.rebase_progress().and_then(|progress| progress.stopped_at), None),
            Applying::StashApply | Applying::StashPop | Applying::Unshelve | Applying::Patch => (None, None),
        };
        Some(InProgress { operation, theirs, theirs_name: name })
    }

    fn pulled_remotes(&self) -> Vec<(String, String)> {
        let mut remotes = self.remote_urls().unwrap_or_default();
        let upstream = self.open().ok().and_then(|repo| {
            let head = repo.head_name().ok()??;
            let remote = repo.branch_remote_name(head.as_ref().shorten(), gix::remote::Direction::Fetch)?;
            Some(remote.as_bstr().to_string())
        });
        if let Some(upstream) = upstream {
            remotes.sort_by_key(|(name, _)| *name != upstream);
        }
        remotes
    }

    pub fn continue_operation(&self) -> Result<String, VcsError> {
        let Some(operation) = self.in_progress() else {
            return Err(VcsError::Git("no merge, cherry-pick, revert or rebase is in progress".to_owned()));
        };
        if operation == Applying::Rebase {
            return self.rebase_continue();
        }
        let files = self.conflicted_files();
        if !files.is_empty() {
            return Err(VcsError::Conflicts { operation, files });
        }
        let verb = match operation {
            Applying::Merge => "merge",
            Applying::Revert => "revert",
            _ => "cherry-pick",
        };
        let empty = git::run(&self.root, &["diff", "--cached", "--quiet"], None).is_ok();
        let step = if empty && operation != Applying::Merge { "--skip" } else { "--continue" };
        let result = git::run_with_environment(&self.root, &[verb, step], None, &[KEEP_MESSAGE]);
        self.stopping(operation, result)?;
        Ok(self.head()?.commit.unwrap_or_default())
    }

    pub fn abort_operation(&self) -> Result<(), VcsError> {
        let verb = match self.in_progress() {
            Some(Applying::Rebase) => return self.rebase_abort(),
            Some(Applying::Merge) => "merge",
            Some(Applying::CherryPick) => "cherry-pick",
            Some(Applying::Revert) => "revert",
            _ => return Err(VcsError::Git("no merge, cherry-pick, revert or rebase is in progress".to_owned())),
        };
        git::run(&self.root, &[verb, "--abort"], None).map(drop)
    }

    pub fn stash_paths(&self, commit: &str) -> Result<Vec<String>, VcsError> {
        let commit = self.resolve(commit)?;
        let base = format!("{commit}^1");
        let changed = git::run(&self.root, &["diff", "--name-only", "-z", "--no-renames", &base, &commit], None)?;
        let mut paths: Vec<String> =
            changed.stdout.split('\0').filter(|path| !path.is_empty()).map(str::to_owned).collect();
        let untracked = format!("{commit}^3");
        if self.resolve(&untracked).is_ok() {
            let listed = git::run(&self.root, &["ls-tree", "-r", "-z", "--name-only", &untracked], None)?;
            paths.extend(listed.stdout.split('\0').filter(|path| !path.is_empty()).map(str::to_owned));
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    pub fn drop_stash_commit(&self, commit: &str) -> Result<(), VcsError> {
        match self.stashes()?.into_iter().find(|stash| stash.commit == commit) {
            Some(stash) => self.stash_drop(stash.index),
            None => Ok(()),
        }
    }

    pub fn forget_in_index(&self, paths: &[String]) -> Result<(), VcsError> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut args = vec!["update-index", "--force-remove", "--"];
        args.extend(paths.iter().map(String::as_str));
        git::run(&self.root, &args, None).map(drop)
    }
}

fn merged_name(message: &str, remotes: impl FnOnce() -> Vec<(String, String)>) -> Option<String> {
    let line = message.lines().next()?;
    let start = line.find('\'')? + 1;
    let end = start + line[start..].find('\'')?;
    let name = Some(line[start..end].to_owned()).filter(|name| !name.is_empty())?;
    let Some(address) = line[end + 1..].strip_prefix(" of ") else { return Some(name) };
    let remotes = remotes();
    let named = |address: &str| {
        let address = comparable_address(address);
        remotes.iter().find(|(_, url)| comparable_address(url) == address)
    };
    let remote = named(address).or_else(|| address.rsplit_once(" into ").and_then(|(address, _)| named(address)));
    Some(match remote {
        Some((remote, _)) => format!("{remote}/{name}"),
        None => name,
    })
}

fn comparable_address(address: &str) -> &str {
    let address = address.trim();
    let (address, host_end) = match address.split_once("://") {
        Some((_, rest)) => (rest, rest.find('/')),
        None => (address, address.find(['/', ':'])),
    };
    let host_end = host_end.unwrap_or(address.len());
    let address = match address[..host_end].rfind('@') {
        Some(at) => &address[at + 1..],
        None => address,
    };
    let address = address.trim_end_matches('/');
    address.strip_suffix(".git").unwrap_or(address).trim_end_matches('/')
}

pub(super) fn inside(root: &Path, relative: &str) -> Result<std::path::PathBuf, VcsError> {
    let fine = !relative.is_empty()
        && !relative.starts_with('/')
        && !(cfg!(windows) && (relative.contains('\\') || relative.contains(':')))
        && relative.split('/').all(|part| !part.is_empty() && part != "." && part != "..");
    if !fine {
        return Err(VcsError::Git(format!("'{relative}' is not a file of the repository")));
    }
    Ok(relative.split('/').fold(root.to_path_buf(), |path, part| path.join(part)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repository() -> (tempfile::TempDir, Repository) {
        git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git::run(&root, &["init", "-q", "-b", "main"], None).unwrap();
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        git::run(&root, &["add", "a.txt"], None).unwrap();
        git::run(&root, &["commit", "-q", "-m", "First"], None).unwrap();
        let repository = Repository::discover(&root).unwrap();
        (dir, repository)
    }

    fn run(repository: &Repository, args: &[&str]) -> String {
        git::run(repository.root(), args, None).unwrap().stdout
    }

    fn commit(repository: &Repository, file: &str, text: &str, subject: &str) -> String {
        std::fs::write(repository.root().join(file), text).unwrap();
        run(repository, &["add", "-A"]);
        run(repository, &["commit", "-q", "-m", subject]);
        repository.head().unwrap().commit.unwrap()
    }

    fn diverged(repository: &Repository) -> String {
        run(repository, &["switch", "-q", "-c", "feature"]);
        let feature = commit(repository, "a.txt", "one\nfeature\nthree\n", "Feature");
        run(repository, &["switch", "-q", "main"]);
        commit(repository, "a.txt", "one\nmain\nthree\n", "Main");
        feature
    }

    #[test]
    fn a_merge_stopped_on_conflicts_lists_its_stages_and_continues_once_resolved() {
        let (_dir, repository) = repository();
        let feature = diverged(&repository);
        assert!(matches!(repository.merge("feature"), Err(VcsError::Conflicts { .. })));
        let conflicts = repository.conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        let file = &conflicts[0];
        assert_eq!(file.path, "a.txt");
        let text =
            |blob: &Option<Blob>| String::from_utf8(repository.blob(&blob.as_ref().unwrap().id).unwrap()).unwrap();
        assert_eq!(
            (text(&file.base), text(&file.ours), text(&file.theirs)),
            ("one\ntwo\nthree\n".to_owned(), "one\nmain\nthree\n".to_owned(), "one\nfeature\nthree\n".to_owned())
        );
        let info = repository.in_progress_info().unwrap();
        assert_eq!(info.operation, Applying::Merge);
        assert_eq!((info.theirs.as_deref(), info.theirs_name.as_deref()), (Some(feature.as_str()), Some("feature")));
        assert!(matches!(repository.continue_operation(), Err(VcsError::Conflicts { .. })));
        repository.resolve_with("a.txt", b"one\nmain and feature\nthree\n").unwrap();
        assert!(repository.conflicts().unwrap().is_empty());
        let head = repository.continue_operation().unwrap();
        assert_eq!(repository.in_progress(), None);
        let parents = run(&repository, &["log", "-1", "--format=%P %s"]);
        assert!(parents.contains(&feature) && parents.trim().ends_with("Merge branch 'feature'"), "{parents}");
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(head.as_str()));
        assert_eq!(std::fs::read_to_string(repository.root().join("a.txt")).unwrap(), "one\nmain and feature\nthree\n");
        assert!(repository.continue_operation().is_err(), "nothing in progress");
    }

    #[test]
    fn a_cherry_pick_and_a_revert_continue_or_abort() {
        let (_dir, repository) = repository();
        let feature = diverged(&repository);
        let main = repository.head().unwrap().commit.unwrap();
        assert!(matches!(repository.cherry_pick(&feature, None), Err(VcsError::Conflicts { .. })));
        let info = repository.in_progress_info().unwrap();
        assert_eq!((info.operation, info.theirs.as_deref()), (Applying::CherryPick, Some(feature.as_str())));
        repository.abort_operation().unwrap();
        assert_eq!(repository.in_progress(), None);
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(main.as_str()));
        assert_eq!(std::fs::read_to_string(repository.root().join("a.txt")).unwrap(), "one\nmain\nthree\n");
        assert!(repository.cherry_pick(&feature, None).is_err());
        repository.resolve_with_side("a.txt", false).unwrap();
        repository.continue_operation().unwrap();
        assert_eq!(run(&repository, &["log", "-1", "--format=%s"]).trim(), "Feature");
        assert_eq!(std::fs::read_to_string(repository.root().join("a.txt")).unwrap(), "one\nfeature\nthree\n");
        assert!(matches!(repository.revert(&main, None), Err(VcsError::Conflicts { .. })));
        assert_eq!(repository.in_progress_info().unwrap().operation, Applying::Revert);
        repository.resolve_with("a.txt", b"one\ntwo\nthree\n").unwrap();
        repository.continue_operation().unwrap();
        assert_eq!(repository.in_progress(), None);
        assert!(run(&repository, &["log", "-1", "--format=%s"]).starts_with("Revert \"Main\""));
        let head = commit(&repository, "a.txt", "one\nlocal\nthree\n", "Local");
        assert!(matches!(repository.revert(&feature, None), Err(VcsError::Conflicts { .. })));
        repository.resolve_with_side("a.txt", true).unwrap();
        repository.continue_operation().unwrap();
        assert_eq!(repository.in_progress(), None);
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(head.as_str()));
    }

    #[test]
    fn a_rebase_continues_and_a_merge_aborts_through_the_same_calls() {
        let (_dir, repository) = repository();
        let feature = diverged(&repository);
        run(&repository, &["switch", "-q", "feature"]);
        assert!(matches!(repository.rebase("main"), Err(VcsError::Conflicts { .. })));
        let info = repository.in_progress_info().unwrap();
        assert_eq!((info.operation, info.theirs.as_deref()), (Applying::Rebase, Some(feature.as_str())));
        repository.resolve_with_side("a.txt", false).unwrap();
        repository.continue_operation().unwrap();
        assert_eq!(repository.in_progress(), None);
        assert_eq!(run(&repository, &["log", "--format=%s"]).lines().collect::<Vec<_>>(), ["Feature", "Main", "First"]);
        run(&repository, &["switch", "-q", "-c", "again", "feature~1"]);
        commit(&repository, "a.txt", "one\nagain\nthree\n", "Again");
        assert!(repository.rebase("feature").is_err());
        repository.resolve_with_side("a.txt", true).unwrap();
        repository.continue_operation().unwrap();
        assert_eq!(repository.in_progress(), None);
        assert_eq!(run(&repository, &["log", "-1", "--format=%s"]).trim(), "Feature");
        run(&repository, &["switch", "-q", "feature"]);
        run(&repository, &["switch", "-q", "main"]);
        commit(&repository, "a.txt", "one\nmain again\nthree\n", "Main again");
        let head = repository.head().unwrap().commit.unwrap();
        assert!(repository.merge("feature").is_err());
        repository.abort_operation().unwrap();
        assert_eq!(repository.in_progress(), None);
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(head.as_str()));
        assert!(repository.abort_operation().is_err(), "nothing in progress");
    }

    #[test]
    fn a_file_deleted_on_one_side_is_kept_or_deleted() {
        let (_dir, repository) = repository();
        run(&repository, &["switch", "-q", "-c", "feature"]);
        run(&repository, &["rm", "-q", "a.txt"]);
        run(&repository, &["commit", "-q", "-m", "Delete"]);
        run(&repository, &["switch", "-q", "main"]);
        commit(&repository, "a.txt", "one\nchanged\nthree\n", "Change");
        assert!(repository.merge("feature").is_err());
        let conflicts = repository.conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        assert!(conflicts[0].base.is_some() && conflicts[0].ours.is_some() && conflicts[0].theirs.is_none());
        repository.resolve_with_side("a.txt", false).unwrap();
        assert!(!repository.root().join("a.txt").exists());
        assert!(repository.conflicts().unwrap().is_empty());
        repository.abort_operation().unwrap();
        assert_eq!(std::fs::read_to_string(repository.root().join("a.txt")).unwrap(), "one\nchanged\nthree\n");
        assert!(repository.merge("feature").is_err());
        repository.resolve_with_side("a.txt", true).unwrap();
        repository.continue_operation().unwrap();
        assert_eq!(std::fs::read_to_string(repository.root().join("a.txt")).unwrap(), "one\nchanged\nthree\n");
        assert!(repository.resolve_with("../outside", b"x").is_err(), "never outside the working tree");
    }

    #[test]
    fn a_pull_stopped_on_conflicts_names_the_remote_branch_it_merges() {
        let (_dir, repository) = repository();
        let remote = tempfile::tempdir().unwrap();
        let bare = remote.path().canonicalize().unwrap().join("origin.git");
        git::run(remote.path(), &["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()], None).unwrap();
        run(&repository, &["remote", "add", "backup", bare.to_str().unwrap()]);
        run(&repository, &["remote", "add", "origin", bare.to_str().unwrap()]);
        run(&repository, &["checkout", "-q", "-b", "topic"]);
        commit(&repository, "a.txt", "one\ntheirs\nthree\n", "Theirs");
        run(&repository, &["push", "-q", "-u", "origin", "topic"]);
        run(&repository, &["reset", "-q", "--hard", "HEAD~1"]);
        commit(&repository, "a.txt", "one\nours\nthree\n", "Ours");
        assert!(repository.update(crate::PullMode::Merge, &mut |_| {}).is_err());
        let info = repository.in_progress_info().unwrap();
        assert_eq!((info.operation, info.theirs_name.as_deref()), (Applying::Merge, Some("origin/topic")));
    }

    #[test]
    fn a_pulled_branch_is_named_after_the_remote_with_its_address() {
        let remotes = [
            ("origin".to_owned(), "https://ada:secret@example.com/zen/ide.git".to_owned()),
            ("fork".to_owned(), "git@example.org:ada/ide.git/".to_owned()),
        ];
        let named = |message: &str| merged_name(message, || remotes.to_vec());
        assert_eq!(named("Merge branch 'main' of https://example.com/zen/ide").as_deref(), Some("origin/main"));
        assert_eq!(named("Merge branch 'dev' of example.org:ada/ide\n").as_deref(), Some("fork/dev"));
        assert_eq!(named("Merge branch 'main' of https://elsewhere.net/x").as_deref(), Some("main"));
        assert_eq!(named("Merge branch 'feature'").as_deref(), Some("feature"));
        assert_eq!(named("Merge branch 'dev' of https://example.com/zen/ide into dev").as_deref(), Some("origin/dev"));
        assert_eq!(named("Merge branch 'dev' of ssh://git@example.com/zen/ide.git").as_deref(), Some("origin/dev"));
        let twins = [("backup".to_owned(), "/srv/ide".to_owned()), ("origin".to_owned(), "/srv/ide.git".to_owned())];
        assert_eq!(merged_name("Merge branch 'main' of /srv/ide", || twins.to_vec()).as_deref(), Some("backup/main"));
    }

    #[test]
    fn a_stash_lists_the_files_it_changes_and_is_dropped_by_its_commit() {
        let (_dir, repository) = repository();
        std::fs::write(repository.root().join("a.txt"), "one\nstashed\nthree\n").unwrap();
        std::fs::write(repository.root().join("new.txt"), "new\n").unwrap();
        let stash = repository.stash_push(None, false, true).unwrap().unwrap();
        assert_eq!(repository.stash_paths(&stash.commit).unwrap(), ["a.txt", "new.txt"]);
        commit(&repository, "a.txt", "one\ncommitted\nthree\n", "Second");
        assert!(matches!(repository.stash_apply(0, true), Err(VcsError::Conflicts { .. })));
        assert_eq!(repository.in_progress(), None, "git keeps no stash in progress");
        assert_eq!(repository.stashes().unwrap().len(), 1, "git keeps a stash that conflicted");
        repository.forget_in_index(&["a.txt".to_owned()]).unwrap();
        assert!(repository.conflicts().unwrap().is_empty());
        repository.drop_stash_commit(&stash.commit).unwrap();
        assert!(repository.stashes().unwrap().is_empty());
        repository.drop_stash_commit(&stash.commit).unwrap();
        assert_eq!(
            merged_name("Merge remote-tracking branch 'origin/main'\n", Vec::new).as_deref(),
            Some("origin/main")
        );
        assert_eq!(merged_name("Merge commit 'abc'", Vec::new), Some("abc".into()));
        assert_eq!(merged_name("Merge", Vec::new), None);
    }

    #[test]
    fn accepting_a_side_of_a_name_with_brackets_leaves_matching_files_alone() {
        let (_dir, repository) = repository();
        let name = "[ab].txt";
        commit(&repository, name, "one\ntwo\nthree\n", "Brackets");
        run(&repository, &["switch", "-q", "-c", "feature"]);
        commit(&repository, name, "one\nfeature\nthree\n", "Feature");
        run(&repository, &["switch", "-q", "main"]);
        commit(&repository, name, "one\nmain\nthree\n", "Main");
        let _ = git::run(repository.root(), &["merge", "--no-edit", "feature"], None);
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "mine\n").unwrap();
        repository.resolve_with_side(name, true).unwrap();
        assert_eq!(std::fs::read_to_string(root.join(name)).unwrap(), "one\nmain\nthree\n");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "mine\n");
    }

    #[cfg(unix)]
    #[test]
    fn continuing_never_opens_an_editor() {
        use std::os::unix::fs::PermissionsExt;

        let (_dir, repository) = repository();
        let seen = repository.git_dir.join("editor-seen");
        let hook = repository.git_dir.join("hooks/prepare-commit-msg");
        std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
        std::fs::write(&hook, format!("#!/bin/sh\nprintf %s \"$GIT_EDITOR\" > '{}'\n", seen.display())).unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        diverged(&repository);
        assert!(matches!(repository.merge("feature"), Err(VcsError::Conflicts { .. })));
        repository.resolve_with("a.txt", b"one\nresolved\nthree\n").unwrap();
        repository.continue_operation().unwrap();
        assert_eq!(std::fs::read_to_string(seen).unwrap(), ":");
    }
}

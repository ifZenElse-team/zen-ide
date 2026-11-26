use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;

use super::{Repository, error};
use crate::git;
use crate::{Applying, Blob, FileState, LINK_MODE, Move, RefState, VcsError};

impl Repository {
    pub fn ref_state(&self) -> Result<RefState, VcsError> {
        let head = self.head()?;
        let output = git::run(
            &self.root,
            &["for-each-ref", "--format=%(refname)%00%(objectname)%00%(upstream:short)", "refs/heads"],
            None,
        )?;
        let (mut branches, mut upstreams) = (BTreeMap::new(), BTreeMap::new());
        for line in output.stdout.lines() {
            let mut fields = line.split('\0');
            let (Some(name), Some(id)) = (fields.next(), fields.next()) else { continue };
            let Some(name) = name.strip_prefix("refs/heads/") else { continue };
            branches.insert(name.to_owned(), id.to_owned());
            if let Some(upstream) = fields.next().filter(|upstream| !upstream.is_empty()) {
                upstreams.insert(name.to_owned(), upstream.to_owned());
            }
        }
        Ok(RefState { branch: head.branch, head: head.commit, branches, upstreams })
    }

    pub fn in_progress(&self) -> Option<Applying> {
        let exists = |name: &str| self.git_dir.join(name).exists();
        if exists("rebase-merge") || exists("rebase-apply") {
            Some(Applying::Rebase)
        } else if exists("MERGE_HEAD") {
            Some(Applying::Merge)
        } else if exists("CHERRY_PICK_HEAD") {
            Some(Applying::CherryPick)
        } else if exists("REVERT_HEAD") {
            Some(Applying::Revert)
        } else {
            None
        }
    }

    pub fn head_log_len(&self) -> u64 {
        std::fs::metadata(self.git_dir.join("logs/HEAD")).map_or(0, |metadata| metadata.len())
    }

    pub fn head_log_since(&self, len: u64) -> Vec<String> {
        let Ok(log) = std::fs::read(self.git_dir.join("logs/HEAD")) else { return Vec::new() };
        let Some(added) = usize::try_from(len).ok().and_then(|len| log.get(len..)) else { return Vec::new() };
        added
            .to_str_lossy()
            .lines()
            .filter_map(|line| line.split_once('\t').map(|(_, message)| message.trim().to_owned()))
            .filter(|message| !message.is_empty())
            .collect()
    }

    pub fn file_states(&self, paths: &[String], store: Option<&Path>) -> Result<Vec<FileState>, VcsError> {
        let repo = self.open()?;
        let index = repo.index_or_empty().map_err(error)?;
        let kind = repo.object_hash();
        let mut states = Vec::with_capacity(paths.len());
        for path in paths {
            let index = match index.entry_by_path(path.as_bytes().as_bstr()) {
                Some(entry) => {
                    let blob = Blob { mode: entry.mode.bits(), id: entry.id.to_string() };
                    if let Some(store) = store {
                        keep_object(&repo, store, &blob.id)?;
                    }
                    Some(blob)
                }
                None => None,
            };
            let worktree = match worktree_file(&self.root, path)? {
                Some((mode, data)) => {
                    let id = gix::objs::compute_hash(kind, gix::objs::Kind::Blob, &data).map_err(error)?.to_string();
                    if let Some(store) = store {
                        keep(store, &id, &data)?;
                    }
                    Some(Blob { mode, id })
                }
                None => None,
            };
            states.push(FileState { path: path.clone(), index, worktree });
        }
        Ok(states)
    }

    pub fn clean_file_states(
        &self,
        revision: Option<&str>,
        paths: &[String],
        store: Option<&Path>,
    ) -> Result<Vec<FileState>, VcsError> {
        let repo = self.open()?;
        let tree = match revision {
            Some(revision) => {
                Some(repo.find_commit(super::resolve(&repo, revision)?).map_err(error)?.tree().map_err(error)?)
            }
            None => None,
        };
        let mut states = Vec::with_capacity(paths.len());
        for path in paths {
            let entry = match &tree {
                Some(tree) => tree.lookup_entry_by_path(Path::new(path)).map_err(error)?,
                None => None,
            };
            let blob = entry.and_then(|entry| {
                use gix::objs::tree::EntryKind;
                let mode = match entry.mode().kind() {
                    EntryKind::Blob => 0o100644,
                    EntryKind::BlobExecutable => 0o100755,
                    EntryKind::Link => LINK_MODE,
                    EntryKind::Tree | EntryKind::Commit => return None,
                };
                Some(Blob { mode, id: entry.object_id().to_string() })
            });
            if let (Some(store), Some(blob)) = (store, &blob) {
                keep_object(&repo, store, &blob.id)?;
            }
            states.push(FileState { path: path.clone(), index: blob.clone(), worktree: blob });
        }
        Ok(states)
    }

    pub fn restore_files(&self, states: &[FileState], store: &Path) -> Result<(), VcsError> {
        for state in states {
            let inside = !state.path.is_empty()
                && !state.path.starts_with('/')
                && !(cfg!(windows) && (state.path.contains('\\') || state.path.contains(':')))
                && state.path.split('/').all(|part| !part.is_empty() && part != "." && part != "..");
            let ids = [&state.index, &state.worktree].into_iter().flatten();
            if !inside || ids.clone().any(|blob| gix::ObjectId::from_hex(blob.id.as_bytes()).is_err()) {
                return Err(VcsError::Git(format!("cannot restore '{}'", state.path)));
            }
        }
        let current = self.file_states(&states.iter().map(|state| state.path.clone()).collect::<Vec<_>>(), None)?;
        let repo = self.open()?;
        let mut index_info = String::new();
        for (state, now) in states.iter().zip(&current) {
            if state.worktree != now.worktree {
                let file = repo_path(&self.root, &state.path);
                remove(&file)?;
                if let Some(blob) = &state.worktree {
                    let data = content(&repo, store, &blob.id, &state.path)?;
                    write_file(&file, blob.mode, &data)?;
                }
            }
            if state.index != now.index {
                match &state.index {
                    Some(blob) => {
                        if gix::ObjectId::from_hex(blob.id.as_bytes()).map_or(true, |id| !repo.has_object(id)) {
                            let kept = store.join(&blob.id);
                            if !kept.exists() {
                                return Err(VcsError::Git(format!("the content of {} is no longer kept", state.path)));
                            }
                            let kept = kept.to_string_lossy().into_owned();
                            git::run(&self.root, &["hash-object", "-w", "--no-filters", "--", kept.as_str()], None)?;
                        }
                        index_info.push_str(&format!("{:o} {}\t{}\0", blob.mode, blob.id, state.path));
                    }
                    None => {
                        let zero = "0".repeat(repo.object_hash().len_in_hex());
                        index_info.push_str(&format!("0 {zero}\t{}\0", state.path));
                    }
                }
            }
        }
        if !index_info.is_empty() {
            git::run(&self.root, &["update-index", "-z", "--index-info"], Some(&index_info))?;
        }
        self.refresh_index();
        Ok(())
    }

    fn refresh_index(&self) {
        // WHY: git exits with an error when some files differ, which is no failure.
        let _ = git::run(&self.root, &["update-index", "-q", "--refresh"], None);
    }

    pub fn move_refs(&self, from: &RefState, to: &RefState, mode: Move, action: &str) -> Result<(), VcsError> {
        let environment = [("GIT_REFLOG_ACTION", action)];
        let run = |args: &[&str]| git::run_with_environment(&self.root, args, None, &environment).map(drop);
        self.refresh_index();
        let target = match &from.branch {
            Some(branch) => to.branches.get(branch),
            None if to.branch.is_none() => to.head.as_ref(),
            None => None,
        };
        if let Some(target) = target
            && from.head.as_ref() != Some(target)
        {
            run(&["reset", "-q", mode.option(), target.as_str(), "--"])?;
        }
        let mut names: Vec<&String> = from.branches.keys().chain(to.branches.keys()).collect();
        names.sort();
        names.dedup();
        for name in names {
            if from.branch.as_ref() == Some(name) {
                continue;
            }
            let (Some(new), old) = (to.branches.get(name), from.branches.get(name)) else { continue };
            if old == Some(new) {
                continue;
            }
            let full = format!("refs/heads/{name}");
            let old = old.cloned().unwrap_or_else(|| "0".repeat(new.len()));
            run(&["update-ref", "-m", action, full.as_str(), new.as_str(), old.as_str()])?;
            if !from.branches.contains_key(name)
                && let Some(upstream) = to.upstreams.get(name)
            {
                let _ = run(&["branch", "--set-upstream-to", upstream.as_str(), name.as_str()]);
            }
        }
        if from.branch != to.branch {
            match (&to.branch, &to.head) {
                (Some(branch), _) => run(&["switch", "-q", "--no-guess", branch.as_str()])?,
                (None, Some(head)) => run(&["switch", "-q", "--detach", head.as_str()])?,
                (None, None) => {}
            }
        }
        for (name, old) in &from.branches {
            if !to.branches.contains_key(name) {
                let full = format!("refs/heads/{name}");
                run(&["update-ref", "-m", action, "-d", full.as_str(), old.as_str()])?;
            }
        }
        Ok(())
    }
}

fn worktree_file(root: &Path, path: &str) -> Result<Option<(u32, Vec<u8>)>, VcsError> {
    let file = repo_path(root, path);
    let metadata = match std::fs::symlink_metadata(&file) {
        Ok(metadata) => metadata,
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(failure) => return Err(VcsError::Git(format!("{}: {failure}", file.display()))),
    };
    let failed = |failure: std::io::Error| VcsError::Git(format!("{}: {failure}", file.display()));
    if metadata.file_type().is_symlink() {
        let target = std::fs::read_link(&file).map_err(failed)?;
        return Ok(Some((LINK_MODE, target.to_string_lossy().replace('\\', "/").into_bytes())));
    }
    if !metadata.is_file() {
        return Ok(None);
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 != 0 { 0o100755 } else { 0o100644 }
    };
    #[cfg(not(unix))]
    let mode = 0o100644;
    Ok(Some((mode, std::fs::read(&file).map_err(failed)?)))
}

fn keep(store: &Path, id: &str, data: &[u8]) -> Result<(), VcsError> {
    let file = store.join(id);
    if file.exists() {
        return Ok(());
    }
    let failed = |failure: std::io::Error| VcsError::Git(format!("{}: {failure}", file.display()));
    std::fs::create_dir_all(store).map_err(failed)?;
    // WHY: written aside and renamed, so that a crash never leaves a kept content half there.
    let partial = store.join(format!("{id}.partial"));
    std::fs::write(&partial, data).map_err(failed)?;
    std::fs::rename(&partial, &file).map_err(failed)
}

fn keep_object(repo: &gix::Repository, store: &Path, id: &str) -> Result<(), VcsError> {
    if store.join(id).exists() {
        return Ok(());
    }
    let oid = gix::ObjectId::from_hex(id.as_bytes()).map_err(error)?;
    let object = repo.find_object(oid).map_err(error)?;
    keep(store, id, &object.data)
}

fn content(repo: &gix::Repository, store: &Path, id: &str, path: &str) -> Result<Vec<u8>, VcsError> {
    if let Ok(data) = std::fs::read(store.join(id)) {
        return Ok(data);
    }
    gix::ObjectId::from_hex(id.as_bytes())
        .ok()
        .and_then(|oid| repo.find_object(oid).ok())
        .map(|object| object.detach().data)
        .ok_or_else(|| VcsError::Git(format!("the content of {path} is no longer kept")))
}

fn repo_path(root: &Path, path: &str) -> PathBuf {
    path.split('/').filter(|part| !part.is_empty()).fold(root.to_owned(), |file, part| file.join(part))
}

fn remove(file: &PathBuf) -> Result<(), VcsError> {
    match std::fs::remove_file(file) {
        Ok(()) => Ok(()),
        Err(failure) if failure.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(failure) => Err(VcsError::Git(format!("{}: {failure}", file.display()))),
    }
}

fn write_file(file: &Path, mode: u32, data: &[u8]) -> Result<(), VcsError> {
    let failed = |failure: std::io::Error| VcsError::Git(format!("{}: {failure}", file.display()));
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    #[cfg(unix)]
    if mode == LINK_MODE {
        let target = PathBuf::from(String::from_utf8_lossy(data).into_owned());
        return std::os::unix::fs::symlink(target, file).map_err(failed);
    }
    std::fs::write(file, data).map_err(failed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = if mode == 0o100755 { 0o755 } else { 0o644 };
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(permissions)).map_err(failed)?;
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
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
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git::run(&root, &["add", "a.txt"], None).unwrap();
        git::run(&root, &["commit", "-q", "-m", "First"], None).unwrap();
        let repository = Repository::discover(&root).unwrap();
        (dir, repository)
    }

    #[test]
    fn files_are_kept_and_put_back_in_the_index_and_the_working_tree() {
        let (_dir, repository) = repository();
        let store = tempfile::tempdir().unwrap();
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "staged\n").unwrap();
        git::run(&root, &["add", "a.txt"], None).unwrap();
        std::fs::write(root.join("a.txt"), "staged, then edited\n").unwrap();
        std::fs::write(root.join("new.txt"), "unversioned\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("a.txt", root.join("link")).unwrap();
        let paths = ["a.txt", "new.txt", "link", "gone.txt"].map(str::to_owned);
        let before = repository.file_states(&paths, Some(store.path())).unwrap();
        assert!(before[0].index.is_some() && before[0].index != before[0].worktree, "staged and edited");
        assert_eq!((before[1].index.is_none(), before[1].worktree.is_some()), (true, true));
        assert_eq!(before[3], FileState { path: "gone.txt".to_owned(), index: None, worktree: None });
        #[cfg(unix)]
        assert_eq!(before[2].worktree.as_ref().unwrap().mode, LINK_MODE);

        let clean = repository.clean_file_states(Some("HEAD"), &paths, Some(store.path())).unwrap();
        assert_eq!(clean[0].index, clean[0].worktree);
        assert_eq!((clean[1].index.is_none(), clean[1].worktree.is_none()), (true, true));
        git::run(&root, &["reset", "-q", "--hard"], None).unwrap();
        std::fs::remove_file(root.join("new.txt")).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(root.join("link")).unwrap();
        let now = repository.file_states(&paths, None).unwrap();
        assert!(now.iter().zip(&clean).all(|(now, clean)| now.same(clean)), "clean is what a hard reset leaves");

        repository.restore_files(&before, store.path()).unwrap();
        let restored = repository.file_states(&paths, None).unwrap();
        assert!(restored.iter().zip(&before).all(|(restored, before)| restored.same(before)), "{restored:?}");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "staged, then edited\n");
        let staged = git::run(&root, &["show", ":a.txt"], None).unwrap().stdout;
        assert_eq!(staged, "staged\n");
        assert_eq!(std::fs::read_to_string(root.join("new.txt")).unwrap(), "unversioned\n");
        #[cfg(unix)]
        assert_eq!(std::fs::read_link(root.join("link")).unwrap(), Path::new("a.txt"));

        let outside =
            FileState { path: "../outside.txt".to_owned(), index: None, worktree: before[0].worktree.clone() };
        assert!(repository.restore_files(&[outside], store.path()).is_err());
        let blob = Blob { mode: 0o100644, id: "../../secret".to_owned() };
        let odd = FileState { path: "a.txt".to_owned(), index: None, worktree: Some(blob) };
        assert!(repository.restore_files(&[odd], store.path()).is_err());
    }

    #[test]
    fn refs_move_both_ways_and_operations_in_progress_are_seen() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let len = repository.head_log_len();
        let before = repository.ref_state().unwrap();
        assert_eq!(before.branch.as_deref(), Some("main"));
        git::run(&root, &["switch", "-q", "-c", "feature"], None).unwrap();
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        git::run(&root, &["commit", "-q", "-am", "Second"], None).unwrap();
        assert_eq!(repository.head_log_since(len), ["checkout: moving from main to feature", "commit: Second"]);
        let after = repository.ref_state().unwrap();
        assert_eq!(after.branches.len(), 2);

        repository.move_refs(&after, &before, Move::Keep, "zen undo").unwrap();
        assert_eq!(repository.ref_state().unwrap(), before, "feature is gone, main checked out");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\n");
        repository.move_refs(&before, &after, Move::Keep, "zen redo").unwrap();
        assert_eq!(repository.ref_state().unwrap(), after);
        assert_eq!(repository.head_log_since(repository.head_log_len()), Vec::<String>::new());

        assert_eq!(repository.in_progress(), None);
        std::fs::write(repository.git_dir.join("MERGE_HEAD"), after.head.clone().unwrap()).unwrap();
        assert_eq!(repository.in_progress(), Some(Applying::Merge));
    }
}

use std::collections::BTreeMap;
use std::path::Path;

use super::{Repository, changed_files, error, read_commit, resolve};
use crate::{ChangedFile, FileRevision, FileStatus, VcsError};

impl Repository {
    pub fn diff_revisions(&self, from: &str, to: Option<&str>) -> Result<Vec<ChangedFile>, VcsError> {
        let repo = self.open()?;
        let from_tree = repo.find_commit(resolve(&repo, from)?).map_err(error)?.tree().map_err(error)?;
        if let Some(to) = to {
            let to_tree = repo.find_commit(resolve(&repo, to)?).map_err(error)?.tree().map_err(error)?;
            let changes = repo.diff_tree_to_tree(&from_tree, &to_tree, None).map_err(error)?;
            return Ok(changed_files(&changes));
        }
        let mut candidates: BTreeMap<String, Option<String>> = BTreeMap::new();
        let mut note = |path: String, old_path: Option<String>| {
            if let Some(old) = &old_path {
                candidates.entry(old.clone()).or_insert(None);
            }
            let entry = candidates.entry(path).or_insert(None);
            if old_path.is_some() {
                *entry = old_path;
            }
        };
        if !repo.head().map_err(error)?.is_unborn() {
            let head = repo.head_tree().map_err(error)?;
            for file in changed_files(&repo.diff_tree_to_tree(&from_tree, &head, None).map_err(error)?) {
                note(file.path, file.old_path);
            }
        }
        for change in self.status()?.changes {
            if change.status != FileStatus::Ignored {
                note(change.path, change.old_path);
            }
        }
        let mut files: BTreeMap<String, ChangedFile> = BTreeMap::new();
        let mut added: Vec<(String, Option<String>, Vec<u8>)> = Vec::new();
        let mut deleted: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for (path, old_path) in candidates {
            let before = content(&from_tree, &path)?;
            let after = in_working_tree(&self.root, &path);
            let status = match (before, after) {
                (None, None) => continue,
                (Some(before), None) => {
                    deleted.insert(path.clone(), before);
                    FileStatus::Deleted
                }
                (None, Some(after)) => {
                    added.push((path.clone(), old_path, after));
                    FileStatus::Added
                }
                (Some(before), Some(after)) if before == after => continue,
                (Some(_), Some(_)) => FileStatus::Modified,
            };
            files.insert(path.clone(), ChangedFile { path, status, old_path: None });
        }
        for (path, hint, text) in added {
            let old = hint.filter(|old| deleted.contains_key(old)).or_else(|| {
                deleted.iter().find(|(_, before)| !text.is_empty() && **before == text).map(|(old, _)| old.clone())
            });
            if let Some(old) = old {
                deleted.remove(&old);
                files.remove(&old);
                if let Some(file) = files.get_mut(&path) {
                    file.status = FileStatus::Renamed;
                    file.old_path = Some(old);
                }
            }
        }
        Ok(files.into_values().collect())
    }

    pub fn file_at(&self, revision: &str, relative: &str) -> Result<Option<Vec<u8>>, VcsError> {
        let repo = self.open()?;
        let tree = repo.find_commit(resolve(&repo, revision)?).map_err(error)?.tree().map_err(error)?;
        content(&tree, relative)
    }

    pub fn file_history(
        &self,
        relative: &str,
        revision: Option<&str>,
        skip: usize,
        limit: usize,
    ) -> Result<(Vec<FileRevision>, bool), VcsError> {
        let repo = self.open()?;
        let start = match revision {
            Some(revision) => resolve(&repo, revision)?,
            None if repo.head().map_err(error)?.is_unborn() => return Ok((Vec::new(), false)),
            None => repo.head_id().map_err(error)?.detach(),
        };
        let walk = repo
            .rev_walk([start])
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .all()
            .map_err(error)?;
        let mut path = relative.to_owned();
        let mut found = Vec::new();
        let mut skipped = 0;
        for info in walk {
            let info = info.map_err(error)?;
            let commit = repo.find_commit(info.id).map_err(error)?;
            let tree = commit.tree().map_err(error)?;
            let here = entry_id(&tree, &path);
            let parents = commit
                .parent_ids()
                .map(|parent| repo.find_commit(parent.detach()).and_then(|parent| parent.tree()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(error)?;
            if parents.iter().any(|parent| entry_id(parent, &path) == here) || (parents.is_empty() && here.is_none()) {
                continue;
            }
            let before = parents.first().and_then(|parent| entry_id(parent, &path));
            let (status, old_path) = match (before, here) {
                (_, None) => (FileStatus::Deleted, None),
                (Some(_), Some(_)) => (FileStatus::Modified, None),
                (None, Some(_)) => match parents.first().map(|parent| renamed_from(&repo, parent, &tree, &path)) {
                    Some(Ok(Some(old))) => (FileStatus::Renamed, Some(old)),
                    Some(Err(failed)) => return Err(failed),
                    _ => (FileStatus::Added, None),
                },
            };
            let at = path.clone();
            if let Some(old) = &old_path {
                path.clone_from(old);
            }
            if skipped < skip {
                skipped += 1;
                continue;
            }
            if found.len() >= limit.max(1) {
                return Ok((found, true));
            }
            found.push(FileRevision { commit: read_commit(&repo, info.id)?, path: at, status, old_path });
        }
        Ok((found, false))
    }
}

fn entry_id(tree: &gix::Tree<'_>, path: &str) -> Option<gix::ObjectId> {
    let entry = tree.lookup_entry_by_path(Path::new(path)).ok().flatten()?;
    (!entry.mode().is_tree()).then(|| entry.object_id())
}

fn content(tree: &gix::Tree<'_>, path: &str) -> Result<Option<Vec<u8>>, VcsError> {
    let Some(entry) = tree.lookup_entry_by_path(Path::new(path)).map_err(error)? else { return Ok(None) };
    if !entry.mode().is_blob_or_symlink() {
        return Ok(None);
    }
    Ok(Some(entry.object().map_err(error)?.detach().data))
}

fn in_working_tree(root: &Path, path: &str) -> Option<Vec<u8>> {
    let file = root.join(path);
    let metadata = std::fs::symlink_metadata(&file).ok()?;
    if metadata.file_type().is_symlink() {
        return std::fs::read_link(&file).ok().map(|target| target.to_string_lossy().replace('\\', "/").into_bytes());
    }
    if !metadata.is_file() {
        return None;
    }
    std::fs::read(file).ok()
}

fn renamed_from(
    repo: &gix::Repository,
    parent: &gix::Tree<'_>,
    tree: &gix::Tree<'_>,
    path: &str,
) -> Result<Option<String>, VcsError> {
    let changes = repo.diff_tree_to_tree(parent, tree, None).map_err(error)?;
    Ok(changed_files(&changes)
        .into_iter()
        .find(|file| file.path == path && file.status == FileStatus::Renamed)
        .and_then(|file| file.old_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git;

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

    fn commit(repository: &Repository, subject: &str) -> String {
        run(repository, &["add", "-A"]);
        run(repository, &["commit", "-q", "-m", subject]);
        repository.head().unwrap().commit.unwrap()
    }

    fn write(repository: &Repository, file: &str, text: &str) {
        let path = repository.root().join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn listed(files: &[ChangedFile]) -> Vec<(&str, FileStatus, Option<&str>)> {
        files.iter().map(|file| (file.path.as_str(), file.status, file.old_path.as_deref())).collect()
    }

    #[test]
    fn two_commits_differ_by_every_file_changed_between_them() {
        let (_dir, repository) = repository();
        let first = repository.head().unwrap().commit.unwrap();
        write(&repository, "b.txt", "bee\n");
        commit(&repository, "Add b");
        write(&repository, "a.txt", "one\n2\nthree\n");
        std::fs::remove_file(repository.root().join("b.txt")).unwrap();
        write(&repository, "src/c.txt", "sea\n");
        let last = commit(&repository, "Change a, drop b, add c");
        let files = repository.diff_revisions(&first, Some(&last)).unwrap();
        assert_eq!(listed(&files), [("a.txt", FileStatus::Modified, None), ("src/c.txt", FileStatus::Added, None)]);
        let back = repository.diff_revisions(&last, Some(&first)).unwrap();
        assert_eq!(listed(&back), [("a.txt", FileStatus::Modified, None), ("src/c.txt", FileStatus::Deleted, None)]);
        assert_eq!(repository.file_at(&first, "a.txt").unwrap().as_deref(), Some(&b"one\ntwo\nthree\n"[..]));
        assert_eq!(repository.file_at(&first, "src/c.txt").unwrap(), None);
        assert_eq!(repository.file_at("main", "src/c.txt").unwrap().as_deref(), Some(&b"sea\n"[..]));
        assert!(repository.diff_revisions("no-such-branch", None).is_err());
    }

    #[test]
    fn a_commit_and_the_working_tree_differ_by_committed_and_uncommitted_changes() {
        let (_dir, repository) = repository();
        let first = repository.head().unwrap().commit.unwrap();
        write(&repository, "b.txt", "bee\n");
        write(&repository, "gone.txt", "soon gone\n");
        commit(&repository, "Add b and gone");
        write(&repository, "a.txt", "one\nTWO\nthree\n");
        run(&repository, &["add", "a.txt"]);
        write(&repository, "b.txt", "bee\n");
        std::fs::remove_file(repository.root().join("gone.txt")).unwrap();
        write(&repository, "new.txt", "new\n");
        let files = repository.diff_revisions(&first, None).unwrap();
        assert_eq!(
            listed(&files),
            [
                ("a.txt", FileStatus::Modified, None),
                ("b.txt", FileStatus::Added, None),
                ("new.txt", FileStatus::Added, None),
            ],
            "gone.txt came and went; b.txt as committed is new since the first commit"
        );
        let head = repository.head().unwrap().commit.unwrap();
        let files = repository.diff_revisions(&head, None).unwrap();
        assert_eq!(
            listed(&files),
            [
                ("a.txt", FileStatus::Modified, None),
                ("gone.txt", FileStatus::Deleted, None),
                ("new.txt", FileStatus::Added, None),
            ]
        );
        run(&repository, &["mv", "b.txt", "moved.txt"]);
        let files = repository.diff_revisions(&head, None).unwrap();
        assert!(listed(&files).contains(&("moved.txt", FileStatus::Renamed, Some("b.txt"))), "{files:?}");
        assert!(!files.iter().any(|file| file.path == "b.txt"));
    }

    #[test]
    fn a_files_history_lists_the_commits_that_changed_it_through_renames() {
        let (_dir, repository) = repository();
        let first = repository.head().unwrap().commit.unwrap();
        write(&repository, "other.txt", "unrelated\n");
        commit(&repository, "Other file");
        write(&repository, "a.txt", "one\ntwo\nthree\nfour\n");
        let second = commit(&repository, "Second line");
        run(&repository, &["mv", "a.txt", "renamed.txt"]);
        let renamed = commit(&repository, "Rename");
        write(&repository, "renamed.txt", "one\ntwo\nthree\nfour\nfive\n");
        let last = commit(&repository, "Five");
        let (history, more) = repository.file_history("renamed.txt", None, 0, 100).unwrap();
        assert!(!more);
        let listed: Vec<(&str, &str, FileStatus, Option<&str>)> = history
            .iter()
            .map(|entry| (entry.commit.id.as_str(), entry.path.as_str(), entry.status, entry.old_path.as_deref()))
            .collect();
        assert_eq!(
            listed,
            [
                (last.as_str(), "renamed.txt", FileStatus::Modified, None),
                (renamed.as_str(), "renamed.txt", FileStatus::Renamed, Some("a.txt")),
                (second.as_str(), "a.txt", FileStatus::Modified, None),
                (first.as_str(), "a.txt", FileStatus::Added, None),
            ],
            "the commit of other.txt is left out"
        );
        assert_eq!(history[0].commit.subject, "Five");
        let (page, more) = repository.file_history("renamed.txt", None, 1, 2).unwrap();
        assert!(more);
        assert_eq!(page.iter().map(|entry| entry.commit.id.clone()).collect::<Vec<_>>(), [renamed, second.clone()]);
        let (old, _) = repository.file_history("a.txt", Some(&second), 0, 10).unwrap();
        assert_eq!(old.len(), 2);
        assert!(repository.file_history("nothing.txt", None, 0, 10).unwrap().0.is_empty());
    }

    #[test]
    fn a_merge_that_took_the_file_from_a_side_is_not_in_its_history() {
        let (_dir, repository) = repository();
        run(&repository, &["switch", "-q", "-c", "feature"]);
        write(&repository, "a.txt", "one\nfeature\nthree\n");
        let feature = commit(&repository, "Feature");
        run(&repository, &["switch", "-q", "main"]);
        write(&repository, "b.txt", "main\n");
        commit(&repository, "Main");
        run(&repository, &["merge", "-q", "--no-ff", "-m", "Merge", "feature"]);
        let (history, _) = repository.file_history("a.txt", None, 0, 10).unwrap();
        let subjects: Vec<&str> = history.iter().map(|entry| entry.commit.subject.as_str()).collect();
        assert_eq!(subjects, ["Feature", "First"]);
        assert_eq!(history[0].commit.id, feature);
    }
}

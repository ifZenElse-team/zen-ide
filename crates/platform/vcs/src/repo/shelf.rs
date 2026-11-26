use std::path::Path;

use super::Repository;
use crate::git;
use crate::{Applying, VcsError};

impl Repository {
    pub fn write_patch(&self, paths: &[String], patch: &Path) -> Result<(), VcsError> {
        if paths.is_empty() {
            return Err(VcsError::Git("no files to shelve".to_owned()));
        }
        let patch = &std::path::absolute(patch).map_err(|error| VcsError::Git(error.to_string()))?;
        let index = self.git_dir.join("zen-shelve-index");
        let index_path = index.to_string_lossy().into_owned();
        let environment = [("GIT_INDEX_FILE", index_path.as_str())];
        let unborn = self.open()?.head().map_err(super::error)?.is_unborn();
        let specs = super::literal(paths);
        let run = |args: &[&str]| git::run_with_environment(&self.root, args, None, &environment);
        let result = (|| {
            if unborn {
                run(&["read-tree", "--empty"])?;
            } else {
                run(&["read-tree", "HEAD"])?;
            }
            let mut add = vec!["add", "--all", "--force", "--"];
            add.extend(specs.iter().map(String::as_str));
            run(&add)?;
            let output = format!("--output={}", patch.display());
            let mut diff = vec!["diff", "--cached", "--binary", "--full-index", "-M", "--no-color", "--no-ext-diff"];
            diff.extend(["--no-textconv", "--no-relative", "--src-prefix=a/", "--dst-prefix=b/"]);
            diff.push(output.as_str());
            diff.push("--");
            diff.extend(specs.iter().map(String::as_str));
            run(&diff).map(drop)
        })();
        let _ = std::fs::remove_file(&index);
        result
    }

    pub fn apply_patch(&self, patch: &Path, include: &[String]) -> Result<(), VcsError> {
        let patch = &std::path::absolute(patch).map_err(|error| VcsError::Git(error.to_string()))?;
        let file = patch.to_string_lossy().into_owned();
        let includes: Vec<String> = include.iter().map(|path| format!("--include={}", glob_escape(path))).collect();
        let args = |extra: &[&'static str]| {
            let mut args: Vec<&str> = vec!["apply", "--binary", "--whitespace=nowarn"];
            args.extend(extra);
            args.extend(includes.iter().map(String::as_str));
            args.push(file.as_str());
            args
        };
        if git::run(&self.root, &args(&["--check"]), None).is_ok() {
            return git::run(&self.root, &args(&[]), None).map(drop);
        }
        match git::run(&self.root, &args(&["--3way"]), None) {
            Ok(_) => {
                let touched: Vec<String> = self
                    .patch_files(patch)?
                    .into_iter()
                    .filter(|path| include.is_empty() || include.contains(path))
                    .collect();
                if !touched.is_empty() {
                    let specs = super::literal(&touched);
                    let mut reset = vec!["reset", "--quiet", "--"];
                    reset.extend(specs.iter().map(String::as_str));
                    let _ = git::run(&self.root, &reset, None);
                }
                Ok(())
            }
            Err(failure) => {
                let files = self.conflicted_files();
                if files.is_empty() {
                    Err(failure)
                } else {
                    Err(VcsError::Conflicts { operation: Applying::Unshelve, files })
                }
            }
        }
    }

    pub fn patch_files(&self, patch: &Path) -> Result<Vec<String>, VcsError> {
        let patch = std::path::absolute(patch).map_err(|error| VcsError::Git(error.to_string()))?;
        let file = patch.to_string_lossy().into_owned();
        let output = git::run(&self.root, &["apply", "--numstat", "-z", file.as_str()], None)?;
        let mut files = Vec::new();
        let mut fields = output.stdout.split('\0').filter(|field| !field.is_empty());
        while let Some(field) = fields.next() {
            let mut parts = field.splitn(3, '\t');
            let (_, _, path) = (parts.next(), parts.next(), parts.next().unwrap_or_default());
            if path.is_empty() {
                let _from = fields.next();
                if let Some(to) = fields.next() {
                    files.push(to.to_owned());
                }
            } else {
                files.push(path.to_owned());
            }
        }
        Ok(files)
    }
}

fn glob_escape(path: &str) -> String {
    let mut escaped = String::with_capacity(path.len());
    for c in path.chars() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
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
        std::fs::write(root.join("b.bin"), [0u8, 1, 2, 3]).unwrap();
        git::run(&root, &["add", "-A"], None).unwrap();
        git::run(&root, &["commit", "-q", "-m", "First"], None).unwrap();
        let repository = Repository::discover(&root).unwrap();
        (dir, repository)
    }

    fn paths(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_patch_keeps_staged_unstaged_new_and_binary_changes_and_applies_back() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let store = tempfile::tempdir().unwrap();
        let patch = store.path().join("1.patch");
        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        git::run(&root, &["add", "a.txt"], None).unwrap();
        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\nfour\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        std::fs::write(root.join("b.bin"), [9u8, 0, 9]).unwrap();
        let index_before = git::run(&root, &["diff", "--cached", "--name-only"], None).unwrap().stdout;
        repository.write_patch(&paths(&["a.txt", "new.txt", "b.bin"]), &patch).unwrap();
        let after = git::run(&root, &["diff", "--cached", "--name-only"], None).unwrap().stdout;
        assert_eq!(after, index_before, "the repository's index is left alone");
        let mut files = repository.patch_files(&patch).unwrap();
        files.sort();
        assert_eq!(files, ["a.txt", "b.bin", "new.txt"]);

        repository.rollback(&paths(&["a.txt", "new.txt", "b.bin"]), true).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n");
        assert!(!root.join("new.txt").exists());
        repository.apply_patch(&patch, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nTWO\nthree\nfour\n");
        assert_eq!(std::fs::read_to_string(root.join("new.txt")).unwrap(), "new\n");
        assert_eq!(std::fs::read(root.join("b.bin")).unwrap(), [9u8, 0, 9]);
        let staged = git::run(&root, &["diff", "--cached", "--name-only"], None).unwrap().stdout;
        assert!(staged.trim().is_empty(), "applied as unstaged changes: {staged}");
    }

    #[test]
    fn a_patch_of_a_name_with_brackets_keeps_that_file_only() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let store = tempfile::tempdir().unwrap();
        let patch = store.path().join("1.patch");
        std::fs::create_dir_all(root.join("[ab]")).unwrap();
        std::fs::create_dir_all(root.join("a")).unwrap();
        std::fs::write(root.join("[ab]/f.txt"), "one\n").unwrap();
        std::fs::write(root.join("a/f.txt"), "two\n").unwrap();
        repository.write_patch(&paths(&["[ab]/f.txt"]), &patch).unwrap();
        assert_eq!(repository.patch_files(&patch).unwrap(), ["[ab]/f.txt"]);
    }

    #[test]
    fn only_the_included_files_are_applied() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let store = tempfile::tempdir().unwrap();
        let patch = store.path().join("1.patch");
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        repository.write_patch(&paths(&["a.txt", "new.txt"]), &patch).unwrap();
        repository.rollback(&paths(&["a.txt", "new.txt"]), true).unwrap();
        repository.apply_patch(&patch, &paths(&["new.txt"])).unwrap();
        assert!(root.join("new.txt").exists());
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n", "left out");
    }

    #[test]
    fn a_patch_merges_three_way_and_stops_on_conflicts() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let store = tempfile::tempdir().unwrap();
        let patch = store.path().join("1.patch");
        std::fs::write(root.join("a.txt"), "one\nmine\nthree\n").unwrap();
        repository.write_patch(&paths(&["a.txt"]), &patch).unwrap();
        repository.rollback(&paths(&["a.txt"]), false).unwrap();
        std::fs::write(root.join("a.txt"), "one\ntheirs\nthree\n").unwrap();
        git::run(&root, &["commit", "-q", "-am", "Theirs"], None).unwrap();
        let failed = repository.apply_patch(&patch, &[]).unwrap_err();
        assert_eq!(failed, VcsError::Conflicts { operation: Applying::Unshelve, files: paths(&["a.txt"]) });
        let text = std::fs::read_to_string(root.join("a.txt")).unwrap();
        assert!(text.contains("<<<<<<<") && text.contains("mine") && text.contains("theirs"), "{text}");

        git::run(&root, &["reset", "-q", "--hard"], None).unwrap();
        std::fs::write(root.join("a.txt"), "zero\none\ntheirs\nthree\n").unwrap();
        git::run(&root, &["commit", "-q", "-am", "Zero"], None).unwrap();
        std::fs::write(root.join("a.txt"), "zero\none\ntheirs\nthree\nfour\n").unwrap();
        repository.write_patch(&paths(&["a.txt"]), &patch).unwrap();
        repository.rollback(&paths(&["a.txt"]), false).unwrap();
        std::fs::write(root.join("a.txt"), "ZERO\none\ntheirs\nthree\n").unwrap();
        git::run(&root, &["commit", "-q", "-am", "Upper"], None).unwrap();
        repository.apply_patch(&patch, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "ZERO\none\ntheirs\nthree\nfour\n");
        let staged = git::run(&root, &["diff", "--cached", "--name-only"], None).unwrap().stdout;
        assert!(staged.trim().is_empty(), "{staged}");
    }

    #[test]
    fn a_rename_is_kept_whole() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let store = tempfile::tempdir().unwrap();
        let patch = store.path().join("1.patch");
        git::run(&root, &["mv", "a.txt", "c.txt"], None).unwrap();
        repository.write_patch(&paths(&["a.txt", "c.txt"]), &patch).unwrap();
        assert_eq!(repository.patch_files(&patch).unwrap(), ["c.txt"]);
        repository.rollback(&paths(&["a.txt", "c.txt"]), true).unwrap();
        assert!(root.join("a.txt").exists() && !root.join("c.txt").exists());
        repository.apply_patch(&patch, &[]).unwrap();
        assert!(!root.join("a.txt").exists() && root.join("c.txt").exists());
        assert!(glob_escape("a[1]*.go") == "a\\[1\\]\\*.go");
    }

    #[test]
    fn shelving_never_touches_the_repositorys_refs() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let store = tempfile::tempdir().unwrap();
        let refs = || git::run(&root, &["for-each-ref"], None).unwrap().stdout;
        let before = refs();
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        repository.write_patch(&paths(&["a.txt"]), &store.path().join("1.patch")).unwrap();
        assert_eq!(refs(), before);
    }

    #[test]
    fn a_patch_named_relative_to_the_ide_is_written_where_it_was_named() {
        let (_dir, repository) = repository();
        std::fs::write(repository.root().join("a.txt"), "changed\n").unwrap();
        let kept = tempfile::NamedTempFile::new_in(".").unwrap();
        let relative = Path::new(kept.path().file_name().unwrap());
        repository.write_patch(&paths(&["a.txt"]), relative).unwrap();
        assert!(!std::fs::read(kept.path()).unwrap().is_empty());
    }

    #[test]
    fn a_text_conversion_filter_does_not_reach_the_patch() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let store = tempfile::tempdir().unwrap();
        let patch = store.path().join("1.patch");
        std::fs::write(root.join(".gitattributes"), "*.txt diff=upper\n").unwrap();
        git::run(&root, &["add", ".gitattributes"], None).unwrap();
        git::run(&root, &["commit", "-q", "-m", "Attributes"], None).unwrap();
        git::run(&root, &["config", "diff.upper.textconv", "tr a-z A-Z <"], None).unwrap();
        std::fs::write(root.join("a.txt"), "one\nchanged\nthree\n").unwrap();
        repository.write_patch(&paths(&["a.txt"]), &patch).unwrap();
        repository.rollback(&paths(&["a.txt"]), true).unwrap();
        repository.apply_patch(&patch, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nchanged\nthree\n");
    }
}

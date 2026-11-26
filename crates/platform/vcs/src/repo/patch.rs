use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::Repository;
use crate::git;
use crate::{Commit, VcsError};

static TEMPORARY: AtomicU64 = AtomicU64::new(0);

impl Repository {
    fn temporary(&self, name: &str) -> PathBuf {
        let n = TEMPORARY.fetch_add(1, Ordering::Relaxed);
        self.git_dir.join(format!("zen-{name}-{}-{n}", std::process::id()))
    }

    fn read_output(&self, file: &Path) -> Result<Vec<u8>, VcsError> {
        let bytes = std::fs::read(file).map_err(|error| VcsError::Git(error.to_string()));
        let _ = std::fs::remove_file(file);
        bytes
    }

    pub fn changes_patch(&self, paths: &[String]) -> Result<Vec<u8>, VcsError> {
        let file = self.temporary("patch");
        let written = self.write_patch(paths, &file);
        let bytes = self.read_output(&file);
        written?;
        bytes
    }

    pub fn commit_patch(&self, revision: &str) -> Result<(Commit, Vec<u8>), VcsError> {
        let id = self.resolve(revision)?;
        let (commit, _) = self.commit_details(&id)?;
        let file = self.temporary("patch");
        let output = format!("--output={}", file.display());
        let args = [
            "show",
            "--pretty=format:",
            "--binary",
            "--full-index",
            "-M",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-relative",
            "--diff-merges=first-parent",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            output.as_str(),
            id.as_str(),
        ];
        let shown = git::run(&self.root, &args, None);
        let bytes = self.read_output(&file);
        shown?;
        Ok((commit, bytes?))
    }

    pub fn author_ident(&self) -> Option<String> {
        let output = git::run(&self.root, &["var", "GIT_AUTHOR_IDENT"], None).ok()?;
        let ident = output.stdout.trim();
        let end = ident.rfind('>')?;
        Some(ident[..=end].to_owned())
    }

    pub fn blob_named(&self, id: &str) -> Option<Vec<u8>> {
        if id.len() < 4 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let object = format!("{id}^{{blob}}");
        let full = git::run(&self.root, &["rev-parse", "--verify", "--quiet", object.as_str()], None).ok()?;
        self.blob(full.stdout.trim()).ok()
    }

    pub fn apply_git_patch(&self, patch: &[u8]) -> Result<(), VcsError> {
        let file = self.temporary("apply");
        std::fs::write(&file, patch).map_err(|error| VcsError::Git(error.to_string()))?;
        let name = file.to_string_lossy().into_owned();
        let result =
            git::run(&self.root, &["apply", "--binary", "--whitespace=nowarn", "--check", name.as_str()], None)
                .and_then(|_| git::run(&self.root, &["apply", "--binary", "--whitespace=nowarn", name.as_str()], None));
        let _ = std::fs::remove_file(&file);
        result.map(drop)
    }

    pub fn leave_conflicted(
        &self,
        relative: &str,
        sides: [Option<&[u8]>; 3],
        mode: &str,
        text: &[u8],
    ) -> Result<(), VcsError> {
        let file = super::conflict::inside(&self.root, relative)?;
        let mut info = format!("0 {}\t{}\n", "0".repeat(40), crate::patch::quote_path(relative));
        for (stage, side) in sides.iter().enumerate() {
            let Some(content) = side else { continue };
            let blob = self.temporary("blob");
            std::fs::write(&blob, content).map_err(|error| VcsError::Git(error.to_string()))?;
            let name = blob.to_string_lossy().into_owned();
            let hashed = git::run(&self.root, &["hash-object", "-w", "--no-filters", "--", name.as_str()], None);
            let _ = std::fs::remove_file(&blob);
            let id = hashed?.stdout.trim().to_owned();
            info.push_str(&format!("{mode} {id} {}\t{}\n", stage + 1, crate::patch::quote_path(relative)));
        }
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|error| VcsError::Git(error.to_string()))?;
        }
        std::fs::write(&file, text).map_err(|error| VcsError::Git(format!("{}: {error}", file.display())))?;
        git::run(&self.root, &["update-index", "--index-info"], Some(&info)).map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch;

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

    fn check(repository: &Repository, patch: &[u8], extra: &[&str]) -> Result<(), VcsError> {
        let file = repository.temporary("check");
        std::fs::write(&file, patch).unwrap();
        let name = file.to_string_lossy().into_owned();
        let mut args = vec!["apply", "--check"];
        args.extend(extra);
        args.push(name.as_str());
        let result = git::run(repository.root(), &args, None).map(drop);
        let _ = std::fs::remove_file(&file);
        result
    }

    #[test]
    fn local_changes_and_commits_become_patches_git_applies() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(root.join("new file.txt"), "new\n").unwrap();
        std::fs::write(root.join("b.bin"), [9u8, 0, 9]).unwrap();
        let paths: Vec<String> = ["a.txt", "new file.txt", "b.bin"].iter().map(|path| (*path).to_owned()).collect();
        let bytes = repository.changes_patch(&paths).unwrap();
        let parsed = patch::parse(&bytes).unwrap().strip(1).unwrap();
        assert_eq!(parsed.files.len(), 3);
        let written = parsed.to_bytes();
        repository.rollback(&paths, true).unwrap();
        check(&repository, &written, &[]).unwrap();
        repository.apply_git_patch(&written).unwrap();
        assert_eq!(std::fs::read(root.join("b.bin")).unwrap(), [9u8, 0, 9]);
        assert_eq!(std::fs::read_to_string(root.join("new file.txt")).unwrap(), "new\n");
        check(&repository, &parsed.reversed().to_bytes(), &[]).unwrap();

        git::run(&root, &["add", "-A"], None).unwrap();
        git::run(&root, &["commit", "-q", "-m", "Second", "-m", "Why it changed."], None).unwrap();
        let (commit, bytes) = repository.commit_patch("HEAD").unwrap();
        assert_eq!(commit.subject, "Second");
        let parsed = patch::parse(&bytes).unwrap();
        assert_eq!(parsed.files.len(), 3);
        assert!(repository.author_ident().unwrap().starts_with("Ada <ada@example.com>"));
        let first = repository.commit_patch("HEAD~1").unwrap().1;
        assert_eq!(patch::parse(&first).unwrap().files.len(), 2, "a root commit against nothing");
        let blob = parsed.files.iter().find(|file| file.path() == "b/a.txt").unwrap().old_blob().unwrap().to_owned();
        assert_eq!(repository.blob_named(&blob[..10]).unwrap(), b"one\ntwo\nthree\n");
        assert_eq!(repository.blob_named("zzzz"), None);
        assert_eq!(repository.blob_named("ffffffff"), None);
    }

    #[test]
    fn a_file_left_conflicted_is_listed_with_its_three_versions() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "one\nmine\nthree\n").unwrap();
        let sides =
            [Some(&b"one\ntwo\nthree\n"[..]), Some(&b"one\nmine\nthree\n"[..]), Some(&b"one\nTWO\nthree\n"[..])];
        let text = b"one\n<<<<<<< Current\nmine\n=======\nTWO\n>>>>>>> Patch\nthree\n";
        repository.leave_conflicted("a.txt", sides, "100644", text).unwrap();
        let conflicts = repository.conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, "a.txt");
        let theirs = conflicts[0].theirs.clone().unwrap();
        assert_eq!(repository.blob(&theirs.id).unwrap(), b"one\nTWO\nthree\n");
        assert_eq!(std::fs::read(root.join("a.txt")).unwrap(), text);
        assert!(repository.leave_conflicted("../x", sides, "100644", text).is_err());
    }
}

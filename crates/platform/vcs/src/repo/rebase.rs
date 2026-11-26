use std::path::{Path, PathBuf};

use super::{Repository, read_commit, resolve};
use crate::git;
use crate::{Applying, Commit, RebaseAction, RebaseProgress, RebaseScript, VcsError};

const FOLDER: &str = "zen-rebase";

impl Repository {
    pub fn rebase_commits(&self, upstream: &str, tip: &str) -> Result<Vec<Commit>, VcsError> {
        let (upstream, tip) = (self.resolve(upstream)?, self.resolve(tip)?);
        let range = format!("{upstream}...{tip}");
        let output = git::run(
            &self.root,
            &["rev-list", "--reverse", "--no-merges", "--right-only", "--cherry-pick", range.as_str()],
            None,
        )?;
        let repo = self.open()?;
        output
            .stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                let id = gix::ObjectId::from_hex(line.trim().as_bytes()).map_err(super::error)?;
                read_commit(&repo, id)
            })
            .collect()
    }

    pub fn oldest_first(&self, ids: &[String]) -> Result<Vec<String>, VcsError> {
        let mut depths = Vec::with_capacity(ids.len());
        for id in ids {
            let id = self.resolve(id)?;
            let output = git::run(&self.root, &["rev-list", "--count", id.as_str()], None)?;
            depths.push((output.stdout.trim().parse::<usize>().unwrap_or(0), id));
        }
        depths.sort_by_key(|(depth, _)| *depth);
        Ok(depths.into_iter().map(|(_, id)| id).collect())
    }

    pub fn interactive_rebase(&self, script: &RebaseScript) -> Result<String, VcsError> {
        if let Some(operation) = self.in_progress() {
            return Err(VcsError::Git(format!("a {} is in progress: finish or abort it first", operation.as_str())));
        }
        let upstream = self.resolve(&script.upstream)?;
        let onto = self.resolve(&script.onto)?;
        if let Some(branch) = &script.branch {
            self.local_branch(branch)?;
        }
        let repo = self.open()?;
        let mut todo = String::new();
        let mut kept = false;
        let mut messages = Vec::new();
        for step in &script.steps {
            let id = resolve(&repo, step.commit.trim())?.to_string();
            if step.action.melds() && !kept {
                return Err(VcsError::Git(format!("{} {id} has no commit before it to go into", step.action.as_str())));
            }
            kept |= step.action != RebaseAction::Drop;
            if step.action == RebaseAction::Reword
                && let Some(message) = step.message.as_ref().map(|message| message.trim()).filter(|m| !m.is_empty())
            {
                messages.push((id.clone(), format!("{message}\n")));
            }
            todo.push_str(&format!("{} {id}\n", step.action.as_str()));
        }
        if script.steps.is_empty() {
            todo.push_str("noop\n");
        }
        let folder = self.rebase_folder();
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).map_err(super::error)?;
        let written = (|| -> std::io::Result<()> {
            std::fs::write(folder.join("todo"), &todo)?;
            for (id, message) in &messages {
                std::fs::write(folder.join(format!("message-{id}")), message)?;
            }
            std::fs::write(folder.join("editor.sh"), editor_script(&folder, &self.git_dir.join("rebase-merge")))
        })();
        written.map_err(super::error)?;
        let sequence_editor = format!("cp {}", quoted(&folder.join("todo")));
        let editor = format!("sh {}", quoted(&folder.join("editor.sh")));
        let mut args = vec!["rebase", "-i", "--onto", onto.as_str(), upstream.as_str()];
        if let Some(branch) = &script.branch {
            args.push(branch.as_str());
        }
        let environment = [("GIT_SEQUENCE_EDITOR", sequence_editor.as_str()), ("GIT_EDITOR", editor.as_str())];
        let result = git::run_with_environment(&self.root, &args, None, &environment);
        self.after_rebase(result)
    }

    pub fn rebase_continue(&self) -> Result<String, VcsError> {
        if self.in_progress() != Some(Applying::Rebase) {
            return Err(VcsError::Git("no rebase is in progress".to_owned()));
        }
        let files = self.conflicted_files();
        if !files.is_empty() {
            return Err(VcsError::Conflicts { operation: Applying::Rebase, files });
        }
        let script = self.rebase_folder().join("editor.sh");
        let editor = if script.exists() { format!("sh {}", quoted(&script)) } else { ":".to_owned() };
        let result = git::run_with_environment(&self.root, &["rebase", "--continue"], None, &[("GIT_EDITOR", &editor)]);
        self.after_rebase(result)
    }

    pub fn rebase_abort(&self) -> Result<(), VcsError> {
        if self.in_progress() != Some(Applying::Rebase) {
            return Err(VcsError::Git("no rebase is in progress".to_owned()));
        }
        git::run(&self.root, &["rebase", "--abort"], None)?;
        let _ = std::fs::remove_dir_all(self.rebase_folder());
        Ok(())
    }

    pub fn rebase_progress(&self) -> Option<RebaseProgress> {
        let read = |folder: &Path, name: &str| {
            std::fs::read_to_string(folder.join(name)).ok().map(|text| text.trim().to_owned())
        };
        let merge = self.git_dir.join("rebase-merge");
        let apply = self.git_dir.join("rebase-apply");
        let (folder, step, total) = if merge.is_dir() {
            (merge.clone(), "msgnum", "end")
        } else if apply.is_dir() {
            (apply.clone(), "next", "last")
        } else {
            return None;
        };
        let number = |name: &str| read(&folder, name).and_then(|text| text.parse().ok()).unwrap_or(0);
        Some(RebaseProgress {
            branch: read(&folder, "head-name").and_then(|name| name.strip_prefix("refs/heads/").map(str::to_owned)),
            onto: read(&folder, "onto").filter(|onto| !onto.is_empty()),
            step: number(step),
            total: number(total),
            stopped_at: read(&folder, "stopped-sha").filter(|id| !id.is_empty()),
            interactive: folder.join("interactive").exists(),
        })
    }

    fn rebase_folder(&self) -> PathBuf {
        self.git_dir.join(FOLDER)
    }

    fn after_rebase(&self, result: Result<git::Output, VcsError>) -> Result<String, VcsError> {
        let result = self.stopping(Applying::Rebase, result);
        if self.in_progress() != Some(Applying::Rebase) {
            let _ = std::fs::remove_dir_all(self.rebase_folder());
        }
        result?;
        Ok(self.head()?.commit.unwrap_or_default())
    }
}

fn quoted(path: &Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    format!("'{}'", path.replace('\'', "'\\''"))
}

fn editor_script(folder: &Path, rebase: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         # Written by Zen IDE for an interactive rebase; removed when it is over.\n\
         line=$(tail -n 1 {done} 2>/dev/null)\n\
         id=${{line#* }}\n\
         id=${{id%% *}}\n\
         message={folder}/message-$id\n\
         if [ -n \"$id\" ] && [ -f \"$message\" ]; then cat \"$message\" > \"$1\"; fi\n\
         exit 0\n",
        done = quoted(&rebase.join("done")),
        folder = quoted(folder),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::RebaseStep;

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

    fn commit(repository: &Repository, file: &str, text: &str, subject: &str) -> String {
        let root = repository.root();
        std::fs::write(root.join(file), text).unwrap();
        git::run(root, &["add", file], None).unwrap();
        git::run(root, &["commit", "-q", "-m", subject], None).unwrap();
        repository.head().unwrap().commit.unwrap()
    }

    fn subjects(repository: &Repository, range: &str) -> Vec<String> {
        let output = git::run(repository.root(), &["log", "--format=%s", range], None).unwrap();
        output.stdout.lines().map(str::to_owned).collect()
    }

    fn step(action: RebaseAction, commit: &str) -> RebaseStep {
        RebaseStep { action, commit: commit.to_owned(), message: None }
    }

    #[test]
    fn an_interactive_rebase_rewords_squashes_reorders_and_drops() {
        let (_dir, repository) = repository();
        let base = repository.head().unwrap().commit.unwrap();
        let b = commit(&repository, "b.txt", "b\n", "B");
        let c = commit(&repository, "c.txt", "c\n", "C");
        let d = commit(&repository, "d.txt", "d\n", "D");
        let e = commit(&repository, "e.txt", "e\n", "E");
        let listed: Vec<String> = repository.rebase_commits(&base, "HEAD").unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(listed, [b.clone(), c.clone(), d.clone(), e.clone()], "the oldest first");

        let script = RebaseScript {
            upstream: base.clone(),
            onto: base.clone(),
            branch: None,
            steps: vec![
                RebaseStep {
                    action: RebaseAction::Reword,
                    commit: d.clone(),
                    message: Some("D, reworded\n\nWhy.".into()),
                },
                step(RebaseAction::Pick, &b),
                step(RebaseAction::Squash, &e),
                step(RebaseAction::Drop, &c),
            ],
        };
        repository.interactive_rebase(&script).unwrap();
        assert_eq!(subjects(&repository, "HEAD"), ["B", "D, reworded", "First"]);
        let squashed = git::run(repository.root(), &["log", "-1", "--format=%B"], None).unwrap().stdout;
        assert_eq!(squashed.trim(), "B\n\nE", "a squash joins the messages");
        let reworded = git::run(repository.root(), &["log", "-1", "--format=%B", "HEAD~1"], None).unwrap().stdout;
        assert_eq!(reworded.trim(), "D, reworded\n\nWhy.");
        assert!(!repository.root().join("c.txt").exists(), "C is dropped");
        assert!(repository.root().join("e.txt").exists(), "E is in B now");
        assert!(!repository.git_dir.join(FOLDER).exists(), "the prepared files go");
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("main"));
    }

    #[test]
    fn a_fixup_keeps_the_first_message_and_a_meld_needs_a_commit_before_it() {
        let (_dir, repository) = repository();
        let base = repository.head().unwrap().commit.unwrap();
        let b = commit(&repository, "b.txt", "b\n", "B");
        let c = commit(&repository, "c.txt", "c\n", "C");
        let bad = RebaseScript {
            upstream: base.clone(),
            onto: base.clone(),
            branch: None,
            steps: vec![step(RebaseAction::Drop, &b), step(RebaseAction::Squash, &c)],
        };
        assert!(repository.interactive_rebase(&bad).is_err());
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(c.as_str()), "nothing moved");
        let script = RebaseScript {
            upstream: base.clone(),
            onto: base,
            branch: None,
            steps: vec![step(RebaseAction::Pick, &b), step(RebaseAction::Fixup, &c)],
        };
        repository.interactive_rebase(&script).unwrap();
        assert_eq!(subjects(&repository, "HEAD"), ["B", "First"]);
        assert_eq!(git::run(repository.root(), &["log", "-1", "--format=%B"], None).unwrap().stdout.trim(), "B");
    }

    #[test]
    fn a_branch_is_rebased_onto_another_and_several_commits_are_cherry_picked() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        git::run(&root, &["switch", "-q", "-c", "feature"], None).unwrap();
        let f1 = commit(&repository, "f1.txt", "1\n", "F1");
        let f2 = commit(&repository, "f2.txt", "2\n", "F2");
        git::run(&root, &["switch", "-q", "main"], None).unwrap();
        commit(&repository, "m.txt", "m\n", "M");
        let listed = repository.rebase_commits("main", "feature").unwrap();
        assert_eq!(listed.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["F1", "F2"]);
        let script = RebaseScript {
            upstream: "main".into(),
            onto: "main".into(),
            branch: Some("feature".into()),
            steps: vec![step(RebaseAction::Pick, &f2), step(RebaseAction::Pick, &f1)],
        };
        repository.interactive_rebase(&script).unwrap();
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("feature"));
        assert_eq!(subjects(&repository, "HEAD"), ["F1", "F2", "M", "First"]);

        git::run(&root, &["switch", "-q", "main"], None).unwrap();
        let head = repository.head().unwrap().commit.unwrap();
        let script = RebaseScript {
            upstream: head.clone(),
            onto: head,
            branch: None,
            steps: vec![step(RebaseAction::Pick, &f1), step(RebaseAction::Pick, &f2)],
        };
        repository.interactive_rebase(&script).unwrap();
        assert_eq!(subjects(&repository, "HEAD"), ["F2", "F1", "M", "First"]);
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("main"));
    }

    #[test]
    fn conflicts_stop_the_rebase_until_it_continues_or_aborts() {
        let (_dir, repository) = repository();
        let root = repository.root().to_owned();
        let base = repository.head().unwrap().commit.unwrap();
        let b = commit(&repository, "a.txt", "b\n", "B");
        let c = commit(&repository, "c.txt", "c\n", "C");
        let d = commit(&repository, "a.txt", "d\n", "D");
        let script = RebaseScript {
            upstream: base.clone(),
            onto: base.clone(),
            branch: None,
            steps: vec![
                step(RebaseAction::Drop, &b),
                step(RebaseAction::Pick, &c),
                RebaseStep { action: RebaseAction::Reword, commit: d.clone(), message: Some("D, reworded".into()) },
            ],
        };
        let error = repository.interactive_rebase(&script).unwrap_err();
        assert_eq!(error, VcsError::Conflicts { operation: Applying::Rebase, files: vec!["a.txt".to_owned()] });
        let progress = repository.rebase_progress().unwrap();
        assert_eq!((progress.branch.as_deref(), progress.step, progress.total), (Some("main"), 3, 3));
        assert_eq!(progress.stopped_at.as_deref(), Some(d.as_str()));
        assert!(progress.interactive);
        assert!(matches!(repository.rebase_continue(), Err(VcsError::Conflicts { .. })));
        repository.rebase_abort().unwrap();
        assert_eq!(repository.in_progress(), None);
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(d.as_str()));
        assert!(!repository.git_dir.join(FOLDER).exists());

        assert!(repository.interactive_rebase(&script).is_err());
        std::fs::write(root.join("a.txt"), "resolved\n").unwrap();
        git::run(&root, &["add", "a.txt"], None).unwrap();
        repository.rebase_continue().unwrap();
        assert_eq!(repository.in_progress(), None);
        assert_eq!(subjects(&repository, "HEAD"), ["D, reworded", "C", "First"]);
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "resolved\n");
        assert!(repository.rebase_progress().is_none());
        assert!(repository.rebase_continue().is_err(), "nothing in progress any more");
    }

    #[test]
    fn paths_are_quoted_for_the_shell() {
        assert_eq!(quoted(Path::new("/tmp/it's here")), "'/tmp/it'\\''s here'");
        let script = editor_script(Path::new("/r/.git/zen-rebase"), Path::new("/r/.git/rebase-merge"));
        assert!(script.contains("tail -n 1 '/r/.git/rebase-merge/done'"), "{script}");
        assert!(script.contains("message='/r/.git/zen-rebase'/message-$id"), "{script}");
    }
}

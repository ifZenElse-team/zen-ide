use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;

use crate::git::{self, Progress};
use crate::history::{CommitRecord, Oid};
use crate::{
    Applying, BlameLine, Branch, Change, ChangedFile, Commit, FileStatus, Head, LogEntry, LogQuery, Outgoing, RefKind,
    RefLabel, ResetMode, Status, Tag, VcsError,
};

mod compare;
mod conflict;
mod patch;
mod rebase;
mod shelf;
mod undo;
mod worktree;

#[derive(Clone, Debug)]
pub struct Repository {
    root: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
}

fn error(error: impl std::fmt::Display) -> VcsError {
    VcsError::Git(error.to_string())
}

impl Repository {
    pub fn discover(path: &Path) -> Option<Self> {
        if !git::allowed(path) {
            return None;
        }
        let repo = gix::discover(path).ok()?;
        let root = repo.workdir()?.to_owned();
        let root = root.canonicalize().unwrap_or(root);
        Some(Self { root, git_dir: repo.git_dir().to_owned(), common_dir: repo.common_dir().to_owned() })
    }

    fn open(&self) -> Result<gix::Repository, VcsError> {
        if !git::allowed(&self.root) {
            return Err(VcsError::NotTrusted);
        }
        gix::open(&self.git_dir).map_err(error)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn stamp(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut file = |path: PathBuf| {
            path.hash(&mut hasher);
            if let Ok(metadata) = std::fs::metadata(&path) {
                metadata.modified().ok().hash(&mut hasher);
                metadata.len().hash(&mut hasher);
            }
        };
        for name in ["HEAD", "index", "logs/HEAD", "MERGE_HEAD", "FETCH_HEAD", "ORIG_HEAD"] {
            file(self.git_dir.join(name));
        }
        for name in ["packed-refs", "config"] {
            file(self.common_dir.join(name));
        }
        file(self.root.join(".gitmodules"));
        if let Ok(modules) = std::fs::read_dir(self.common_dir.join("modules")) {
            for module in modules.flatten() {
                file(module.path().join("HEAD"));
            }
        }
        let mut folders = vec![self.common_dir.join("refs/heads"), self.common_dir.join("refs/remotes")];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    folders.push(path);
                } else {
                    file(path);
                }
            }
        }
        self.worktrees_stamp().hash(&mut hasher);
        hasher.finish()
    }

    pub fn refs_stamp(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut file = |path: PathBuf| {
            path.hash(&mut hasher);
            if let Ok(metadata) = std::fs::metadata(&path) {
                metadata.modified().ok().hash(&mut hasher);
                metadata.len().hash(&mut hasher);
            }
        };
        file(self.git_dir.join("HEAD"));
        file(self.git_dir.join("logs/HEAD"));
        for name in ["packed-refs", "logs/refs/stash", "refs/stash"] {
            file(self.common_dir.join(name));
        }
        let mut folders = vec![
            self.common_dir.join("refs/heads"),
            self.common_dir.join("refs/remotes"),
            self.common_dir.join("refs/tags"),
        ];
        while let Some(folder) = folders.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    folders.push(path);
                } else {
                    file(path);
                }
            }
        }
        hasher.finish()
    }

    pub fn resolve(&self, revision: &str) -> Result<String, VcsError> {
        let revision = revision.trim();
        if revision.is_empty() || revision.starts_with('-') {
            return Err(VcsError::Git(format!("invalid revision '{revision}'")));
        }
        let repo = self.open()?;
        Ok(resolve(&repo, revision)?.to_string())
    }

    pub fn stashes(&self) -> Result<Vec<crate::Stash>, VcsError> {
        let reflog = std::fs::read(self.common_dir.join("logs/refs/stash")).unwrap_or_default();
        let mut stashes = crate::parse_stash_reflog(&String::from_utf8_lossy(&reflog));
        if stashes.is_empty() {
            return Ok(stashes);
        }
        let repo = self.open()?;
        for stash in &mut stashes {
            let Ok(id) = gix::ObjectId::from_hex(stash.commit.as_bytes()) else { continue };
            if let Ok(commit) = repo.find_commit(id)
                && let Some(parent) = commit.parent_ids().next()
            {
                stash.base = parent.to_string();
            }
        }
        Ok(stashes)
    }

    pub fn stash_push(
        &self,
        message: Option<&str>,
        staged_only: bool,
        untracked: bool,
    ) -> Result<Option<crate::Stash>, VcsError> {
        self.stash_push_except(message, staged_only, untracked, &[])
    }

    pub fn stash_push_except(
        &self,
        message: Option<&str>,
        staged_only: bool,
        untracked: bool,
        except: &[String],
    ) -> Result<Option<crate::Stash>, VcsError> {
        let before = self.stashes()?.into_iter().next().map(|stash| stash.commit);
        let mut args = vec!["stash".to_owned(), "push".to_owned(), "--quiet".to_owned()];
        if staged_only {
            args.push("--staged".to_owned());
        } else if untracked {
            args.push("--include-untracked".to_owned());
        }
        if let Some(message) = message.map(str::trim).filter(|message| !message.is_empty()) {
            args.push(format!("--message={message}"));
        }
        if !except.is_empty() {
            args.push("--".to_owned());
            args.push(":(top)".to_owned());
            args.extend(except.iter().map(|file| format!(":(top,exclude,literal){file}")));
        }
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        git::run(&self.root, &args, None)?;
        let top = self.stashes()?.into_iter().next();
        Ok(top.filter(|top| Some(&top.commit) != before.as_ref()))
    }

    pub fn stash_apply(&self, index: usize, pop: bool) -> Result<(), VcsError> {
        let name = format!("stash@{{{index}}}");
        let (verb, operation) = if pop { ("pop", Applying::StashPop) } else { ("apply", Applying::StashApply) };
        self.stopping(operation, git::run(&self.root, &["stash", verb, "--quiet", name.as_str()], None)).map(drop)
    }

    pub fn stash_drop(&self, index: usize) -> Result<(), VcsError> {
        let name = format!("stash@{{{index}}}");
        git::run(&self.root, &["stash", "drop", "--quiet", name.as_str()], None).map(drop)
    }

    pub fn stash_rename(&self, index: usize, text: &str) -> Result<crate::Stash, VcsError> {
        if text.trim().is_empty() {
            return Err(VcsError::Git("a stash needs a message".to_owned()));
        }
        let stashes = self.stashes()?;
        let Some(target) = stashes.get(index).cloned() else {
            return Err(VcsError::Git(format!("there is no stash@{{{index}}}")));
        };
        let message = crate::renamed_stash_message(&target.message, &text.replace(['\r', '\n'], " "));
        self.write_stash_message(index, &message)?;
        Ok(crate::Stash { message, ..target })
    }

    fn write_stash_message(&self, index: usize, message: &str) -> Result<(), VcsError> {
        let failed = |error: std::io::Error| VcsError::Git(error.to_string());
        let lock = self.common_dir.join("refs/stash.lock");
        std::fs::OpenOptions::new().write(true).create_new(true).open(&lock).map_err(failed)?;
        let log = self.common_dir.join("logs/refs/stash");
        let next = self.common_dir.join("logs/refs/stash.lock");
        let written = std::fs::read(&log).map_err(failed).and_then(|reflog| {
            let renamed = crate::renamed_in_stash_reflog(&reflog, index, message)
                .ok_or_else(|| VcsError::Git(format!("there is no stash@{{{index}}}")))?;
            std::fs::write(&next, renamed).and_then(|()| std::fs::rename(&next, &log)).map_err(failed)
        });
        let _ = std::fs::remove_file(&next);
        let _ = std::fs::remove_file(&lock);
        written
    }

    pub fn head(&self) -> Result<Head, VcsError> {
        let repo = self.open()?;
        let head = repo.head().map_err(error)?;
        Ok(Head {
            branch: head.referent_name().map(|name| name.shorten().to_str_lossy().into_owned()),
            commit: head.id().map(|id| id.to_string()),
        })
    }

    pub fn status(&self) -> Result<Status, VcsError> {
        use gix::status::index_worktree::iter::Summary;
        use gix::status::{Item, UntrackedFiles};

        let repo = self.open()?;
        let iter = repo
            .status(gix::progress::Discard)
            .map_err(error)?
            .untracked_files(UntrackedFiles::Files)
            .dirwalk_options(|options| options.emit_ignored(Some(gix::dir::walk::EmissionMode::CollapseDirectory)))
            .index_worktree_rewrites(None)
            .index_worktree_submodules(None)
            .into_iter(None)
            .map_err(error)?;
        let mut changes: std::collections::BTreeMap<String, Change> = std::collections::BTreeMap::new();
        let mut ignored = Vec::new();
        for item in iter {
            let item = item.map_err(error)?;
            let path = item.location().to_str_lossy().into_owned();
            match &item {
                Item::TreeIndex(change) => {
                    use gix::diff::index::ChangeRef;
                    let (status, old_path) = match change {
                        ChangeRef::Addition { .. } => (FileStatus::Added, None),
                        ChangeRef::Deletion { .. } => (FileStatus::Deleted, None),
                        ChangeRef::Modification { .. } => (FileStatus::Modified, None),
                        ChangeRef::Rewrite { source_location, .. } => {
                            (FileStatus::Renamed, Some(source_location.to_str_lossy().into_owned()))
                        }
                    };
                    let entry = changes.entry(path.clone()).or_insert(Change {
                        path,
                        status,
                        old_path: None,
                        staged: true,
                        index: None,
                        worktree: None,
                    });
                    entry.status = status;
                    entry.old_path = old_path.or(entry.old_path.take());
                    entry.staged = true;
                    entry.index = Some(status);
                    if let Some(old) = entry.old_path.clone() {
                        changes.remove(&old);
                    }
                }
                Item::IndexWorktree(worktree) => {
                    if let gix::status::index_worktree::Item::DirectoryContents { entry, .. } = worktree
                        && let gix::dir::entry::Status::Ignored(_) = entry.status
                    {
                        ignored.push(path);
                        continue;
                    }
                    let Some(summary) = worktree.summary() else { continue };
                    let status = match summary {
                        Summary::Added => FileStatus::Unversioned,
                        Summary::IntentToAdd => FileStatus::Added,
                        Summary::Removed => FileStatus::Deleted,
                        Summary::Modified | Summary::TypeChange => FileStatus::Modified,
                        Summary::Renamed | Summary::Copied => FileStatus::Renamed,
                        Summary::Conflict => FileStatus::Conflicted,
                    };
                    match changes.get_mut(&path) {
                        Some(existing) => {
                            existing.worktree = Some(status);
                            if matches!(status, FileStatus::Conflicted)
                                || (status == FileStatus::Deleted && existing.status != FileStatus::Added)
                            {
                                existing.status = status;
                            }
                        }
                        None => {
                            let change = Change {
                                path: path.clone(),
                                status,
                                old_path: None,
                                staged: false,
                                index: None,
                                worktree: Some(status),
                            };
                            changes.insert(path, change);
                        }
                    }
                }
            }
        }
        let mut changes: Vec<Change> = changes.into_values().collect();
        for change in &mut changes {
            if change.status == FileStatus::Conflicted {
                change.index = None;
                change.worktree = Some(FileStatus::Conflicted);
            }
        }
        ignored.sort();
        Ok(Status { changes, ignored })
    }

    pub fn branches(&self) -> Result<Vec<Branch>, VcsError> {
        let repo = self.open()?;
        let current = repo.head().map_err(error)?.referent_name().map(|name| name.as_bstr().to_owned());
        let platform = repo.references().map_err(error)?;
        let mut branches = Vec::new();
        for (remote, iter) in
            [(false, platform.local_branches().map_err(error)?), (true, platform.remote_branches().map_err(error)?)]
        {
            for reference in iter {
                let Ok(mut reference) = reference else { continue };
                let full = reference.name().to_owned();
                let name = reference.name().shorten().to_str_lossy().into_owned();
                if remote && name.ends_with("/HEAD") {
                    continue;
                }
                let Ok(target) = reference.peel_to_id() else { continue };
                let target = target.detach();
                let mut branch = Branch {
                    current: !remote && current.as_ref().is_some_and(|current| current == full.as_bstr()),
                    name,
                    remote,
                    commit: target.to_string(),
                    upstream: None,
                    ahead: 0,
                    behind: 0,
                };
                if !remote
                    && let Some(Ok(upstream)) =
                        repo.branch_remote_tracking_ref_name(full.as_ref(), gix::remote::Direction::Fetch)
                {
                    branch.upstream = Some(upstream.as_ref().shorten().to_str_lossy().into_owned());
                    if let Ok(Some(mut upstream)) = repo.try_find_reference(upstream.as_ref())
                        && let Ok(upstream) = upstream.peel_to_id()
                    {
                        let upstream = upstream.detach();
                        branch.ahead = count(&repo, target, upstream);
                        branch.behind = count(&repo, upstream, target);
                    }
                }
                branches.push(branch);
            }
        }
        branches.sort_by(|a, b| (a.remote, &a.name).cmp(&(b.remote, &b.name)));
        Ok(branches)
    }

    pub fn tags(&self) -> Result<Vec<Tag>, VcsError> {
        let repo = self.open()?;
        let platform = repo.references().map_err(error)?;
        let mut tags = Vec::new();
        for reference in platform.tags().map_err(error)? {
            let Ok(mut reference) = reference else { continue };
            let name = reference.name().shorten().to_str_lossy().into_owned();
            let Ok(target) = reference.peel_to_id() else { continue };
            tags.push(Tag { name, commit: target.detach().to_string() });
        }
        tags.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(tags)
    }

    pub fn recent_branches(&self, limit: usize) -> Result<Vec<String>, VcsError> {
        let repo = self.open()?;
        let reflog = std::fs::read_to_string(self.git_dir.join("logs").join("HEAD")).unwrap_or_default();
        let exists =
            |name: &str| repo.try_find_reference(format!("refs/heads/{name}").as_str()).ok().flatten().is_some();
        let current =
            repo.head().map_err(error)?.referent_name().map(|name| name.shorten().to_str_lossy().into_owned());
        let mut recent: Vec<String> = Vec::new();
        for name in current.into_iter().chain(checked_out(&reflog)) {
            if recent.len() >= limit {
                break;
            }
            if !recent.contains(&name) && exists(&name) {
                recent.push(name);
            }
        }
        Ok(recent)
    }

    pub fn checkout_revision(&self, revision: &str) -> Result<(), VcsError> {
        let revision = revision.trim();
        if revision.is_empty() || revision.starts_with('-') {
            return Err(VcsError::Git(format!("invalid revision '{revision}'")));
        }
        git::run(&self.root, &["switch", "--detach", revision], None).map(drop)
    }

    pub fn file_at_head(&self, relative: &str) -> Result<Option<Vec<u8>>, VcsError> {
        let repo = self.open()?;
        if repo.head().map_err(error)?.is_unborn() {
            return Ok(None);
        }
        let tree = repo.head_tree().map_err(error)?;
        let Some(entry) = tree.lookup_entry_by_path(Path::new(relative)).map_err(error)? else { return Ok(None) };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        Ok(Some(entry.object().map_err(error)?.detach().data))
    }

    pub fn count_between(&self, from: &str, to: &str) -> Result<usize, VcsError> {
        let repo = self.open()?;
        let id = |text: &str| -> Result<gix::ObjectId, VcsError> {
            Ok(repo.rev_parse_single(text).map_err(error)?.detach())
        };
        Ok(count(&repo, id(to)?, id(from)?))
    }

    pub fn outgoing(&self) -> Result<Outgoing, VcsError> {
        self.outgoing_of(None)
    }

    pub fn outgoing_of(&self, branch: Option<&str>) -> Result<Outgoing, VcsError> {
        let repo = self.open()?;
        let (full, tip) = match branch {
            None => {
                let head = repo.head().map_err(error)?;
                let Some(full) = head.referent_name().map(|name| name.to_owned()) else {
                    return Err(VcsError::Detached);
                };
                (full, head.id().map(|id| id.detach()))
            }
            Some(name) => {
                let mut reference = local_branch(&repo, name)?;
                (reference.name().to_owned(), reference.peel_to_id().ok().map(|id| id.detach()))
            }
        };
        let branch = full.as_ref().shorten().to_str_lossy().into_owned();
        let configured = repo.branch_remote_name(full.as_ref().shorten(), gix::remote::Direction::Push);
        let remote = match configured {
            Some(name) => name.as_bstr().to_str_lossy().into_owned(),
            None => default_remote(&repo)?,
        };
        let target = repo
            .branch_remote_ref_name(full.as_ref(), gix::remote::Direction::Push)
            .and_then(Result::ok)
            .map(|name| name.as_ref().shorten().to_str_lossy().into_owned())
            .unwrap_or_else(|| branch.clone());
        let tracking = format!("refs/remotes/{remote}/{target}");
        let known = repo
            .try_find_reference(tracking.as_str())
            .ok()
            .flatten()
            .and_then(|mut reference| reference.peel_to_id().ok().map(|id| id.detach()));
        let mut commits = Vec::new();
        if let Some(tip) = tip {
            let walk = repo.rev_walk([tip]).sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ));
            let walk = match known {
                Some(known) => walk.with_hidden([known]),
                None => walk.with_hidden(remote_tips(&repo)),
            };
            for info in walk.all().map_err(error)?.take(1000) {
                let info = info.map_err(error)?;
                commits.push(read_commit(&repo, info.id)?);
            }
        }
        let remote_tip = known.map(|known| known.to_string());
        Ok(Outgoing { branch, remote, target, new_branch: known.is_none(), remote_tip, commits })
    }

    pub fn create_branch(&self, name: &str, start: Option<&str>, checkout: bool) -> Result<(), VcsError> {
        valid_name(name, "branch")?;
        let mut args = if checkout { vec!["switch", "-c", name] } else { vec!["branch", name] };
        if let Some(start) = start {
            if start.starts_with('-') {
                return Err(VcsError::InvalidName { name: start.to_owned(), what: "revision" });
            }
            args.push(start);
        }
        git::run(&self.root, &args, None).map(drop)
    }

    pub fn checkout(&self, name: &str) -> Result<(), VcsError> {
        valid_name(name, "branch")?;
        let repo = self.open()?;
        let local = repo.try_find_reference(format!("refs/heads/{name}").as_str()).ok().flatten().is_some();
        let remote = repo.try_find_reference(format!("refs/remotes/{name}").as_str()).ok().flatten().is_some();
        if !local && remote {
            let short = name.split_once('/').map_or(name, |(_, rest)| rest);
            let exists = repo.try_find_reference(format!("refs/heads/{short}").as_str()).ok().flatten().is_some();
            if exists {
                return git::run(&self.root, &["switch", short], None).map(drop);
            }
            return git::run(&self.root, &["switch", "-c", short, "--track", name], None).map(drop);
        }
        git::run(&self.root, &["switch", name], None).map(drop)
    }

    pub fn commit(&self, message: &str, paths: &[String], amend: bool) -> Result<String, VcsError> {
        self.commit_as(message, paths, amend, None)
    }

    pub fn commit_as(
        &self,
        message: &str,
        paths: &[String],
        amend: bool,
        author: Option<&str>,
    ) -> Result<String, VcsError> {
        let specs = literal(paths);
        if !paths.is_empty() {
            let mut add = vec!["add", "--all", "--"];
            add.extend(specs.iter().map(String::as_str));
            git::run(&self.root, &add, None)?;
        }
        let mut args = vec!["commit", "--quiet", "--file", "-", "--only"];
        if amend {
            args.push("--amend");
        }
        let author = author.map(|author| format!("--author={author}"));
        args.extend(author.as_deref());
        args.push("--");
        args.extend(specs.iter().map(String::as_str));
        git::run(&self.root, &args, Some(message))?;
        Ok(self.head()?.commit.unwrap_or_default())
    }

    pub fn rollback(&self, paths: &[String], delete_added: bool) -> Result<(), VcsError> {
        let (mut known, mut added) = (Vec::new(), Vec::new());
        for path in paths {
            if self.file_at_head(path)?.is_some() {
                known.push(path.as_str());
            } else {
                added.push(path.as_str());
            }
        }
        if !known.is_empty() {
            let specs = literal(&known);
            let mut args = vec!["restore", "--source=HEAD", "--staged", "--worktree", "--"];
            args.extend(specs.iter().map(String::as_str));
            git::run(&self.root, &args, None)?;
        }
        if !added.is_empty() {
            let specs = literal(&added);
            let mut args = vec!["rm", "--cached", "--quiet", "--force", "--ignore-unmatch", "--"];
            args.extend(specs.iter().map(String::as_str));
            git::run(&self.root, &args, None)?;
            if delete_added {
                for path in added {
                    let file = self.deletable(path)?;
                    std::fs::remove_file(&file)
                        .map_err(|error| VcsError::Git(format!("{}: {error}", file.display())))?;
                }
            }
        }
        Ok(())
    }

    pub fn commit_staged(&self, message: &str, amend: bool) -> Result<String, VcsError> {
        self.commit_staged_as(message, amend, None)
    }

    pub fn commit_staged_as(&self, message: &str, amend: bool, author: Option<&str>) -> Result<String, VcsError> {
        let mut args = vec!["commit", "--quiet", "--file", "-"];
        if amend {
            args.push("--amend");
        }
        let author = author.map(|author| format!("--author={author}"));
        args.extend(author.as_deref());
        git::run(&self.root, &args, Some(message))?;
        Ok(self.head()?.commit.unwrap_or_default())
    }

    pub fn stage(&self, paths: &[String]) -> Result<(), VcsError> {
        let specs = literal(paths);
        let mut args = vec!["add", "--all", "--"];
        if paths.is_empty() {
            args.push(":/");
        }
        args.extend(specs.iter().map(String::as_str));
        git::run(&self.root, &args, None).map(drop)
    }

    pub fn unstage(&self, paths: &[String]) -> Result<(), VcsError> {
        let unborn = self.open()?.head().map_err(error)?.is_unborn();
        let mut args = if unborn {
            vec!["rm", "--cached", "-r", "--quiet", "--ignore-unmatch", "--"]
        } else {
            vec!["reset", "--quiet", "--"]
        };
        if paths.is_empty() {
            args.push(":/");
        }
        let specs = literal(paths);
        args.extend(specs.iter().map(String::as_str));
        git::run(&self.root, &args, None).map(drop)
    }

    pub fn file_in_index(&self, relative: &str) -> Result<Option<Vec<u8>>, VcsError> {
        let repo = self.open()?;
        let index = repo.index_or_empty().map_err(error)?;
        let Some(entry) = index.entry_by_path(relative.as_bytes().as_bstr()) else { return Ok(None) };
        if entry.mode.contains(gix::index::entry::Mode::COMMIT) || entry.mode.contains(gix::index::entry::Mode::DIR) {
            return Ok(None);
        }
        Ok(Some(repo.find_object(entry.id).map_err(error)?.detach().data))
    }

    pub fn set_file_in_index(&self, relative: &str, content: &str) -> Result<(), VcsError> {
        let mode = {
            let repo = self.open()?;
            let index = repo.index_or_empty().map_err(error)?;
            index.entry_by_path(relative.as_bytes().as_bstr()).map(|entry| entry.mode.bits())
        };
        let mode = mode.unwrap_or_else(|| if self.executable(relative) { 0o100755 } else { 0o100644 });
        let path = format!("--path={relative}");
        let id = git::run(&self.root, &["hash-object", "-w", "--stdin", path.as_str()], Some(content))?;
        let info = format!("{mode:o},{},{relative}", id.stdout.trim());
        git::run(&self.root, &["update-index", "--add", "--cacheinfo", info.as_str()], None).map(drop)
    }

    fn executable(&self, relative: &str) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let file = relative.split('/').fold(self.root.clone(), |file, part| file.join(part));
            std::fs::metadata(file).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
        }
        #[cfg(not(unix))]
        {
            let _ = relative;
            false
        }
    }

    pub fn discard(&self, paths: &[String]) -> Result<(), VcsError> {
        let paths: Vec<String> = if paths.is_empty() {
            self.status()?.changes.into_iter().filter(|change| change.worktree.is_some()).map(|c| c.path).collect()
        } else {
            paths.to_vec()
        };
        let (mut known, mut unversioned) = (Vec::new(), Vec::new());
        for path in &paths {
            if self.file_in_index(path)?.is_some() {
                known.push(path.as_str());
            } else {
                unversioned.push(path.as_str());
            }
        }
        if !known.is_empty() {
            let specs = literal(&known);
            let mut args = vec!["restore", "--worktree", "--"];
            args.extend(specs.iter().map(String::as_str));
            git::run(&self.root, &args, None)?;
        }
        for path in unversioned {
            let file = self.deletable(path)?;
            match std::fs::remove_file(&file) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(VcsError::Git(format!("{}: {error}", file.display()))),
            }
        }
        Ok(())
    }

    fn deletable(&self, relative: &str) -> Result<PathBuf, VcsError> {
        let file = conflict::inside(&self.root, relative)?;
        let outside = || VcsError::Git(format!("'{relative}' is not a file of the repository"));
        let root = self.root.canonicalize().map_err(|_| outside())?;
        let parent = file.parent().ok_or_else(outside)?;
        match parent.canonicalize() {
            Ok(parent) if !parent.starts_with(&root) => Err(outside()),
            _ => Ok(file),
        }
    }

    pub fn last_message(&self) -> Result<Option<String>, VcsError> {
        let repo = self.open()?;
        let Ok(id) = repo.head_id() else { return Ok(None) };
        let commit = repo.find_commit(id.detach()).map_err(error)?;
        let message = commit.message_raw().map_err(error)?;
        Ok(Some(message.to_str_lossy().trim_end().to_owned()))
    }

    pub fn log(&self, query: &LogQuery) -> Result<Vec<LogEntry>, VcsError> {
        let repo = self.open()?;
        let labels = labels(&repo);
        let wanted: Vec<&String> =
            query.branch.iter().chain(&query.branches).filter(|branch| !branch.is_empty()).collect();
        let mut tips: Vec<gix::ObjectId> = if wanted.is_empty() {
            labels.iter().map(|(id, _)| *id).collect()
        } else {
            wanted.iter().map(|branch| resolve(&repo, branch)).collect::<Result<_, _>>()?
        };
        tips.sort();
        tips.dedup();
        if tips.is_empty() {
            return Ok(Vec::new());
        }
        let hidden =
            query.exclude.iter().filter(|revision| !revision.is_empty()).map(|revision| resolve(&repo, revision));
        let hidden: Vec<gix::ObjectId> = hidden.collect::<Result<_, _>>()?;
        let walk = repo
            .rev_walk(tips)
            .with_hidden(hidden)
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .all()
            .map_err(error)?;
        let filter = crate::filter::CommitFilter::new(query)?;
        let paths: Vec<&Path> =
            query.path.iter().chain(&query.paths).filter(|path| !path.is_empty()).map(Path::new).collect();
        let mut entries = Vec::new();
        let mut skipped = 0;
        for info in walk {
            let info = info.map_err(error)?;
            if !paths.is_empty() && !paths.iter().any(|path| touches(&repo, info.id, path)) {
                continue;
            }
            let commit = read_commit(&repo, info.id)?;
            if !filter.matches(&commit) {
                continue;
            }
            if skipped < query.skip {
                skipped += 1;
                continue;
            }
            let at: Vec<&(gix::ObjectId, RefLabel)> = labels.iter().filter(|(id, _)| *id == info.id).collect();
            let refs = at.iter().map(|(_, label)| label.name.clone()).collect();
            let labels = at.into_iter().map(|(_, label)| label.clone()).collect();
            entries.push(LogEntry { commit, refs, labels });
            if entries.len() >= query.limit.max(1) {
                break;
            }
        }
        let entries = children_first(entries);
        if !query.topological {
            return Ok(entries);
        }
        let shape: Vec<(String, Vec<String>)> =
            entries.iter().map(|entry| (entry.commit.id.clone(), entry.commit.parents.clone())).collect();
        let order = crate::graph::topological(&shape);
        let mut slots: Vec<Option<LogEntry>> = entries.into_iter().map(Some).collect();
        Ok(order.into_iter().filter_map(|at| slots[at].take()).collect())
    }

    pub fn read_history(&self, sink: &mut dyn FnMut(CommitRecord<'_>) -> bool) -> Result<(), VcsError> {
        let mut repo = self.open()?;
        repo.object_cache_size_if_unset(16 * 1024 * 1024);
        let mut tips: Vec<gix::ObjectId> = labels(&repo).into_iter().map(|(id, _)| id).collect();
        tips.sort();
        tips.dedup();
        if tips.is_empty() {
            return Ok(());
        }
        let walk = repo
            .rev_walk(tips)
            .sorting(gix::revision::walk::Sorting::ByCommitTime(
                gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
            ))
            .all()
            .map_err(error)?;
        let mut parents: Vec<Oid> = Vec::new();
        for info in walk {
            let info = info.map_err(error)?;
            let commit = repo.find_commit(info.id).map_err(error)?;
            let author = commit.author().map_err(error)?.trim();
            let time = author.time().map_err(error)?;
            let message = commit.message().map_err(error)?;
            let (name, email) = (author.name.to_str_lossy(), author.email.to_str_lossy());
            let subject = message.title.to_str_lossy();
            let body = message.body.map(|body| body.to_str_lossy()).unwrap_or_default();
            parents.clear();
            parents.extend(info.parent_ids.iter().map(|parent| oid(parent)));
            let record = CommitRecord {
                id: oid(&info.id),
                parents: &parents,
                author: &name,
                email: &email,
                time: time.seconds,
                offset: time.offset,
                subject: subject.trim(),
                body: body.trim(),
            };
            if !sink(record) {
                break;
            }
        }
        Ok(())
    }

    pub fn ref_labels(&self) -> Result<Vec<(Oid, RefLabel)>, VcsError> {
        let repo = self.open()?;
        Ok(labels(&repo).into_iter().map(|(id, label)| (oid(&id), label)).collect())
    }

    pub fn refs_containing(&self, id: &str) -> Result<Vec<RefLabel>, VcsError> {
        let output = git::run(
            &self.root,
            &["for-each-ref", "--contains", id, "--format=%(refname)", "refs/heads", "refs/remotes", "refs/tags"],
            None,
        )?;
        let mut found: Vec<RefLabel> = output
            .stdout
            .lines()
            .filter_map(|line| {
                let (kind, name) = if let Some(name) = line.strip_prefix("refs/heads/") {
                    (RefKind::Local, name)
                } else if let Some(name) = line.strip_prefix("refs/remotes/") {
                    (RefKind::Remote, name)
                } else {
                    (RefKind::Tag, line.strip_prefix("refs/tags/")?)
                };
                (!name.ends_with("/HEAD")).then(|| RefLabel { name: name.to_owned(), kind })
            })
            .collect();
        found.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
        Ok(found)
    }

    pub fn user(&self) -> (Option<String>, Option<String>) {
        let read = |key: &str| {
            git::run(&self.root, &["config", "--get", key], None)
                .ok()
                .map(|output| output.stdout.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        (read("user.name"), read("user.email"))
    }

    pub fn update_branch(&self, name: &str, progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        let branches = self.branches()?;
        let branch = branches
            .iter()
            .find(|branch| !branch.remote && branch.name == name)
            .ok_or_else(|| VcsError::Git(format!("no local branch {name}")))?;
        if branch.current {
            return self.update(crate::PullMode::Merge, progress);
        }
        let upstream = branch.upstream.as_deref().ok_or(VcsError::NoRemote)?;
        let (remote, target) = upstream.split_once('/').ok_or(VcsError::NoRemote)?;
        valid_name(remote, "remote")?;
        let refspec = format!("refs/heads/{target}:refs/heads/{name}");
        git::run_with_progress(&self.root, &["fetch", "--progress", remote, refspec.as_str()], None, progress).map(drop)
    }

    pub fn cherry_pick(&self, id: &str, mainline: Option<u32>) -> Result<String, VcsError> {
        self.apply(Applying::CherryPick, id, mainline)
    }

    pub fn revert(&self, id: &str, mainline: Option<u32>) -> Result<String, VcsError> {
        self.apply(Applying::Revert, id, mainline)
    }

    fn apply(&self, operation: Applying, id: &str, mainline: Option<u32>) -> Result<String, VcsError> {
        let commit = self.resolve(id)?;
        let parents = {
            let repo = self.open()?;
            let oid = gix::ObjectId::from_hex(commit.as_bytes()).map_err(error)?;
            repo.find_commit(oid).map_err(error)?.parent_ids().count()
        };
        let mainline = mainline.unwrap_or(1).to_string();
        let mut args = vec![operation.as_str()];
        if operation == Applying::Revert {
            args.push("--no-edit");
        }
        if parents > 1 {
            args.extend(["-m", mainline.as_str()]);
        }
        args.push(commit.as_str());
        self.stopping(operation, git::run(&self.root, &args, None))?;
        Ok(self.head()?.commit.unwrap_or_default())
    }

    pub fn conflicted_files(&self) -> Vec<String> {
        git::run(&self.root, &["diff", "--name-only", "-z", "--diff-filter=U"], None)
            .map(|output| output.stdout.split('\0').map(str::to_owned).filter(|path| !path.is_empty()).collect())
            .unwrap_or_default()
    }

    pub fn is_ancestor(&self, ancestor: &str, descendant: &str) -> Result<bool, VcsError> {
        let (ancestor, descendant) = (self.resolve(ancestor)?, self.resolve(descendant)?);
        Ok(git::run(&self.root, &["merge-base", "--is-ancestor", ancestor.as_str(), descendant.as_str()], None).is_ok())
    }

    pub fn reset(&self, revision: &str, mode: ResetMode) -> Result<String, VcsError> {
        let commit = self.resolve(revision)?;
        git::run(&self.root, &["reset", "-q", mode.option(), commit.as_str()], None)?;
        Ok(self.head()?.commit.unwrap_or_default())
    }

    pub fn create_tag(&self, name: &str, revision: Option<&str>, message: Option<&str>) -> Result<String, VcsError> {
        let name = name.trim();
        if name.is_empty() || name.starts_with('-') {
            return Err(VcsError::Git(format!("'{name}' is not a valid tag name")));
        }
        let commit = self.resolve(revision.unwrap_or("HEAD"))?;
        let mut args = vec!["tag"];
        if let Some(message) = message.filter(|message| !message.trim().is_empty()) {
            args.extend(["-a", "-F", "-"]);
            args.extend(["--", name, commit.as_str()]);
            git::run(&self.root, &args, Some(message))?;
        } else {
            args.extend(["--", name, commit.as_str()]);
            git::run(&self.root, &args, None)?;
        }
        Ok(commit)
    }

    pub fn commit_details(&self, id: &str) -> Result<(Commit, Vec<ChangedFile>), VcsError> {
        let repo = self.open()?;
        let id = resolve(&repo, id)?;
        let commit = repo.find_commit(id).map_err(error)?;
        let tree = commit.tree().map_err(error)?;
        let parent = match commit.parent_ids().next() {
            Some(parent) => Some(repo.find_commit(parent.detach()).map_err(error)?.tree().map_err(error)?),
            None => None,
        };
        let changes = repo.diff_tree_to_tree(parent.as_ref(), &tree, None).map_err(error)?;
        let files = changed_files(&changes);
        Ok((read_commit(&repo, id)?, files))
    }

    pub fn commit_files(&self, id: &str) -> Result<Vec<String>, VcsError> {
        let repo = self.open()?;
        let id = resolve(&repo, id)?;
        let tree = repo.find_commit(id).map_err(error)?.tree().map_err(error)?;
        let mut recorder = gix::traverse::tree::Recorder::default();
        tree.traverse().breadthfirst(&mut recorder).map_err(error)?;
        let mut files: Vec<String> = recorder
            .records
            .into_iter()
            .filter(|entry| entry.mode.is_blob_or_symlink())
            .map(|entry| entry.filepath.to_str_lossy().into_owned())
            .collect();
        files.sort();
        Ok(files)
    }

    pub fn file_in_commit(&self, id: &str, relative: &str) -> Result<crate::Sides, VcsError> {
        let repo = self.open()?;
        let id = resolve(&repo, id)?;
        let commit = repo.find_commit(id).map_err(error)?;
        let read = |commit: &gix::Commit<'_>| -> Result<Option<Vec<u8>>, VcsError> {
            let tree = commit.tree().map_err(error)?;
            let Some(entry) = tree.lookup_entry_by_path(Path::new(relative)).map_err(error)? else { return Ok(None) };
            if !entry.mode().is_blob() {
                return Ok(None);
            }
            Ok(Some(entry.object().map_err(error)?.detach().data))
        };
        let after = read(&commit)?;
        let before = match commit.parent_ids().next() {
            Some(parent) => read(&repo.find_commit(parent.detach()).map_err(error)?)?,
            None => None,
        };
        Ok((before, after))
    }

    pub fn blame(&self, relative: &str) -> Result<Vec<BlameLine>, VcsError> {
        let repo = self.open()?;
        let head = repo.head_id().map_err(error)?.detach();
        let options = gix::repository::blame_file::Options {
            rewrites: Some(gix::diff::Rewrites::default()),
            ..Default::default()
        };
        let outcome = repo.blame_file(relative.into(), head, options).map_err(error)?;
        let mut commits: std::collections::HashMap<gix::ObjectId, BlameLine> = std::collections::HashMap::new();
        let mut lines = Vec::new();
        for entry in &outcome.entries {
            if let std::collections::hash_map::Entry::Vacant(vacant) = commits.entry(entry.commit_id) {
                let commit = read_commit(&repo, entry.commit_id)?;
                vacant.insert(BlameLine {
                    commit: commit.id,
                    author: commit.author,
                    time: commit.time,
                    offset: commit.offset,
                    subject: commit.subject,
                });
            }
            let line = &commits[&entry.commit_id];
            let range = entry.range_in_blamed_file();
            if lines.len() < range.end {
                lines.resize(range.end, None);
            }
            for index in range {
                lines[index] = Some(line.clone());
            }
        }
        Ok(lines
            .into_iter()
            .map(|line| {
                line.unwrap_or_else(|| BlameLine {
                    commit: String::new(),
                    author: String::new(),
                    time: 0,
                    offset: 0,
                    subject: String::new(),
                })
            })
            .collect())
    }

    pub fn merge(&self, name: &str) -> Result<(), VcsError> {
        self.resolve(name)?;
        self.stopping(Applying::Merge, git::run(&self.root, &["merge", "--no-edit", name], None)).map(drop)
    }

    pub fn rebase(&self, onto: &str) -> Result<String, VcsError> {
        self.resolve(onto)?;
        if self.head()?.branch.is_none() {
            return Err(VcsError::Detached);
        }
        self.stopping(Applying::Rebase, git::run(&self.root, &["rebase", onto], None))?;
        Ok(self.head()?.commit.unwrap_or_default())
    }

    pub fn merge_into(&self, name: &str, target: &str) -> Result<(), VcsError> {
        self.resolve(name)?;
        self.switch_to(target)?;
        self.merge(name)
    }

    pub fn rebase_branch(&self, branch: &str, onto: &str) -> Result<String, VcsError> {
        self.resolve(onto)?;
        self.local_branch(branch)?;
        if self.head()?.branch.as_deref() == Some(branch) {
            return self.rebase(onto);
        }
        self.stopping(Applying::Rebase, git::run(&self.root, &["rebase", onto, branch], None))?;
        Ok(self.head()?.commit.unwrap_or_default())
    }

    pub fn has_tracked_changes(&self) -> Result<bool, VcsError> {
        let output = git::run(&self.root, &["status", "--porcelain", "--untracked-files=no"], None)?;
        Ok(!output.stdout.trim().is_empty())
    }

    fn switch_to(&self, name: &str) -> Result<(), VcsError> {
        self.local_branch(name)?;
        if self.head()?.branch.as_deref() != Some(name) {
            git::run(&self.root, &["switch", "--no-guess", name], None)?;
        }
        Ok(())
    }

    fn local_branch(&self, name: &str) -> Result<(), VcsError> {
        valid_name(name, "branch")?;
        let repo = self.open()?;
        match repo.try_find_reference(format!("refs/heads/{name}").as_str()) {
            Ok(Some(_)) => Ok(()),
            _ => Err(VcsError::Git(format!("no local branch {name}"))),
        }
    }

    pub(super) fn stopping(
        &self,
        operation: Applying,
        result: Result<git::Output, VcsError>,
    ) -> Result<git::Output, VcsError> {
        result.map_err(|failure| {
            let files = self.conflicted_files();
            if files.is_empty() { failure } else { VcsError::Conflicts { operation, files } }
        })
    }

    pub fn fast_forward(&self, name: &str, onto: Option<&str>) -> Result<String, VcsError> {
        let branches = self.branches()?;
        let branch = branches
            .iter()
            .find(|branch| !branch.remote && branch.name == name)
            .ok_or_else(|| VcsError::Git(format!("no local branch {name}")))?;
        let target_name = match onto.map(str::trim).filter(|onto| !onto.is_empty()) {
            Some(onto) => onto.to_owned(),
            None => branch.upstream.clone().ok_or(VcsError::NoRemote)?,
        };
        let target = self.resolve(&target_name)?;
        if target == branch.commit {
            return Ok(target);
        }
        if !self.is_ancestor(&branch.commit, &target)? {
            return Err(VcsError::NotFastForward { branch: name.to_owned(), target: target_name });
        }
        if branch.current {
            git::run(&self.root, &["merge", "--ff-only", "--no-edit", target.as_str()], None)?;
        } else {
            let full = format!("refs/heads/{name}");
            let message = format!("fast-forward: {target_name}");
            // WHY: with the old value, so that a branch moved meanwhile is left alone.
            git::run(&self.root, &["update-ref", "-m", &message, &full, &target, &branch.commit], None)?;
        }
        Ok(target)
    }

    pub fn delete_branch(&self, name: &str, force: bool) -> Result<(), VcsError> {
        valid_name(name, "branch")?;
        git::run(&self.root, &["branch", if force { "-D" } else { "-d" }, name], None).map(drop)
    }

    pub fn rename_branch(&self, name: &str, new_name: &str) -> Result<(), VcsError> {
        let new_name = new_name.trim();
        valid_name(new_name, "branch")?;
        let repo = self.open()?;
        local_branch(&repo, name)?;
        git::run(&self.root, &["branch", "-m", name, new_name], None).map(drop)
    }

    pub fn set_upstream(&self, name: &str, upstream: Option<&str>) -> Result<(), VcsError> {
        let repo = self.open()?;
        local_branch(&repo, name)?;
        match upstream.map(str::trim).filter(|upstream| !upstream.is_empty()) {
            Some(upstream) => {
                valid_name(upstream, "branch")?;
                let option = format!("--set-upstream-to={upstream}");
                git::run(&self.root, &["branch", option.as_str(), name], None).map(drop)
            }
            None => match git::run(&self.root, &["branch", "--unset-upstream", name], None) {
                Ok(_) => Ok(()),
                Err(_) if self.branches()?.iter().any(|branch| branch.name == name && branch.upstream.is_none()) => {
                    Ok(())
                }
                Err(failure) => Err(failure),
            },
        }
    }

    pub fn delete_tag(&self, name: &str) -> Result<(), VcsError> {
        valid_name(name, "tag")?;
        git::run(&self.root, &["tag", "-d", name], None).map(drop)
    }

    pub fn remotes(&self) -> Result<Vec<String>, VcsError> {
        let repo = self.open()?;
        let mut names: Vec<String> = repo.remote_names().iter().map(|name| name.to_str_lossy().into_owned()).collect();
        names.sort();
        Ok(names)
    }

    pub fn remote_urls(&self) -> Result<Vec<(String, String)>, VcsError> {
        let repo = self.open()?;
        let mut urls = Vec::new();
        for name in self.remotes()? {
            let Ok(remote) = repo.find_remote(name.as_str()) else { continue };
            if let Some(url) = remote.url(gix::remote::Direction::Fetch) {
                urls.push((name, url.to_bstring().to_string()));
            }
        }
        Ok(urls)
    }

    pub fn default_remote(&self) -> Result<String, VcsError> {
        default_remote(&self.open()?)
    }

    pub fn push_tag(
        &self,
        name: &str,
        remote: Option<&str>,
        progress: &mut dyn FnMut(Progress),
    ) -> Result<String, VcsError> {
        valid_name(name, "tag")?;
        let repo = self.open()?;
        if repo.try_find_reference(format!("refs/tags/{name}").as_str()).map_err(error)?.is_none() {
            return Err(VcsError::Git(format!("no tag {name}")));
        }
        let remote = match remote.map(str::trim).filter(|remote| !remote.is_empty()) {
            Some(remote) => remote.to_owned(),
            None => default_remote(&repo)?,
        };
        valid_name(&remote, "remote")?;
        let refspec = format!("refs/tags/{name}:refs/tags/{name}");
        git::run_with_progress(&self.root, &["push", "--progress", "--porcelain", &remote, &refspec], None, progress)?;
        Ok(remote)
    }

    pub fn delete_remote_branch(&self, name: &str, progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        let repo = self.open()?;
        let remote = repo
            .remote_names()
            .iter()
            .map(|remote| remote.to_str_lossy().into_owned())
            .filter(|remote| name.strip_prefix(remote.as_str()).is_some_and(|rest| rest.starts_with('/')))
            .max_by_key(String::len)
            .ok_or_else(|| VcsError::Git(format!("no remote branch {name}")))?;
        let branch = &name[remote.len() + 1..];
        valid_name(&remote, "remote")?;
        valid_name(branch, "branch")?;
        let refspec = format!(":refs/heads/{branch}");
        git::run_with_progress(&self.root, &["push", "--progress", "--porcelain", &remote, &refspec], None, progress)
            .map(drop)
    }

    pub fn fetch(&self, progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        git::run_with_progress(&self.root, &["fetch", "--all", "--prune", "--progress"], None, progress).map(drop)
    }

    pub fn update(&self, mode: crate::PullMode, progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        git::run_with_progress(&self.root, &crate::pull_args(mode), None, progress).map(drop)
    }

    pub fn push(&self, outgoing: &Outgoing, force: bool, progress: &mut dyn FnMut(Progress)) -> Result<(), VcsError> {
        valid_name(&outgoing.remote, "remote")?;
        let refspec = format!("refs/heads/{}:refs/heads/{}", outgoing.branch, outgoing.target);
        let mut args = vec!["push", "--progress", "--porcelain"];
        if outgoing.new_branch {
            args.push("--set-upstream");
        }
        let lease = match outgoing.remote_tip {
            Some(_) => format!("--force-with-lease=refs/heads/{}", outgoing.target),
            None => format!("--force-with-lease=refs/heads/{}:", outgoing.target),
        };
        if force {
            args.extend([lease.as_str(), "--force-if-includes"]);
        }
        args.extend([outgoing.remote.as_str(), refspec.as_str()]);
        git::run_with_progress(&self.root, &args, None, progress).map(drop)
    }
}

fn checked_out(reflog: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in reflog.lines().rev() {
        let Some((_, message)) = line.split_once('\t') else { continue };
        let Some(moves) = message.strip_prefix("checkout: moving from ") else { continue };
        let Some((from, to)) = moves.rsplit_once(" to ") else { continue };
        for name in [to, from] {
            if !names.iter().any(|known| known == name) {
                names.push(name.to_owned());
            }
        }
    }
    names
}

fn children_first(entries: Vec<LogEntry>) -> Vec<LogEntry> {
    let order = {
        let index: std::collections::HashMap<&str, usize> =
            entries.iter().enumerate().map(|(at, entry)| (entry.commit.id.as_str(), at)).collect();
        crate::history::children_first(entries.len(), |at| {
            entries[at].commit.parents.iter().filter_map(|parent| index.get(parent.as_str()).copied())
        })
    };
    let mut slots: Vec<Option<LogEntry>> = entries.into_iter().map(Some).collect();
    order.into_iter().filter_map(|at| slots[at].take()).collect()
}

fn local_branch<'r>(repo: &'r gix::Repository, name: &str) -> Result<gix::Reference<'r>, VcsError> {
    if name.is_empty() || name.starts_with('-') {
        return Err(VcsError::Git(format!("invalid branch name '{name}'")));
    }
    repo.try_find_reference(format!("refs/heads/{name}").as_str())
        .map_err(error)?
        .ok_or_else(|| VcsError::Git(format!("no local branch {name}")))
}

fn default_remote(repo: &gix::Repository) -> Result<String, VcsError> {
    let names = repo.remote_names();
    if names.iter().any(|name| name.as_bstr() == "origin") {
        Ok("origin".to_owned())
    } else if let Some(name) = names.iter().next() {
        Ok(name.to_str_lossy().into_owned())
    } else {
        Err(VcsError::NoRemote)
    }
}

fn valid_name(name: &str, what: &'static str) -> Result<(), VcsError> {
    if name.is_empty() || name.starts_with('-') || name.chars().any(char::is_whitespace) {
        return Err(VcsError::InvalidName { name: name.to_owned(), what });
    }
    Ok(())
}

fn literal<S: AsRef<str>>(paths: &[S]) -> Vec<String> {
    paths.iter().map(|path| format!(":(literal){}", path.as_ref())).collect()
}

fn resolve(repo: &gix::Repository, revision: &str) -> Result<gix::ObjectId, VcsError> {
    let id = repo.rev_parse_single(revision).map_err(error)?;
    let object = id.object().map_err(error)?;
    Ok(object.peel_to_commit().map_err(error)?.id)
}

fn labels(repo: &gix::Repository) -> Vec<(gix::ObjectId, RefLabel)> {
    let mut labels = Vec::new();
    if let Ok(head) = repo.head_id() {
        labels.push((head.detach(), RefLabel { name: "HEAD".to_owned(), kind: RefKind::Head }));
    }
    let Ok(platform) = repo.references() else { return labels };
    let kinds = [
        (platform.local_branches(), RefKind::Local),
        (platform.remote_branches(), RefKind::Remote),
        (platform.tags(), RefKind::Tag),
    ];
    for (iter, kind) in kinds {
        let Ok(iter) = iter else { continue };
        for mut reference in iter.flatten() {
            let name = reference.name().shorten().to_str_lossy().into_owned();
            if name.ends_with("/HEAD") {
                continue;
            }
            if let Ok(id) = reference.peel_to_id() {
                labels.push((id.detach(), RefLabel { name, kind }));
            }
        }
    }
    labels
}

fn oid(id: &gix::oid) -> Oid {
    let mut bytes = [0u8; 20];
    let slice = id.as_bytes();
    let length = slice.len().min(20);
    bytes[..length].copy_from_slice(&slice[..length]);
    Oid(bytes)
}

pub fn local_offset() -> i32 {
    gix::date::Time::now_local_or_utc().offset
}

fn touches(repo: &gix::Repository, id: gix::ObjectId, path: &Path) -> bool {
    let entry = |id: gix::ObjectId| -> Option<gix::ObjectId> {
        let commit = repo.find_commit(id).ok()?;
        let tree = commit.tree().ok()?;
        tree.lookup_entry_by_path(path).ok().flatten().map(|entry| entry.object_id())
    };
    let parent = repo.find_commit(id).ok().and_then(|commit| commit.parent_ids().next().map(|parent| parent.detach()));
    entry(id) != parent.and_then(entry)
}

fn count(repo: &gix::Repository, tip: gix::ObjectId, hidden: gix::ObjectId) -> usize {
    repo.rev_walk([tip]).with_hidden([hidden]).all().map_or(0, |walk| walk.filter(Result::is_ok).count())
}

fn remote_tips(repo: &gix::Repository) -> Vec<gix::ObjectId> {
    let Ok(platform) = repo.references() else { return Vec::new() };
    let Ok(iter) = platform.remote_branches() else { return Vec::new() };
    iter.flatten().filter_map(|mut reference| reference.peel_to_id().ok().map(|id| id.detach())).collect()
}

pub(crate) fn changed_files(changes: &[gix::object::tree::diff::ChangeDetached]) -> Vec<ChangedFile> {
    use gix::object::tree::diff::ChangeDetached;
    let mut files: Vec<ChangedFile> = changes
        .iter()
        .filter(|change| !change.entry_mode().is_tree())
        .map(|change| {
            let (location, status, old_path) = match change {
                ChangeDetached::Addition { location, .. } => (location, FileStatus::Added, None),
                ChangeDetached::Deletion { location, .. } => (location, FileStatus::Deleted, None),
                ChangeDetached::Modification { location, .. } => (location, FileStatus::Modified, None),
                ChangeDetached::Rewrite { source_location, location, .. } => {
                    (location, FileStatus::Renamed, Some(source_location.to_str_lossy().into_owned()))
                }
            };
            ChangedFile { path: location.to_str_lossy().into_owned(), status, old_path }
        })
        .collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

pub(crate) fn read_commit(repo: &gix::Repository, id: gix::ObjectId) -> Result<Commit, VcsError> {
    let commit = repo.find_commit(id).map_err(error)?;
    let author = commit.author().map_err(error)?.trim();
    let time = author.time().map_err(error)?;
    let message = commit.message().map_err(error)?;
    Ok(Commit {
        id: id.to_string(),
        parents: commit.parent_ids().map(|parent| parent.to_string()).collect(),
        author: author.name.to_str_lossy().into_owned(),
        email: author.email.to_str_lossy().into_owned(),
        time: time.seconds,
        offset: time.offset,
        subject: message.title.to_str_lossy().trim().to_owned(),
        body: message.body.map(|body| body.to_str_lossy().trim().to_owned()).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .output()
            .unwrap();
        assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    }

    #[test]
    fn stashes_revisions_and_the_refs_stamp() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "First"]);
        let repository = Repository::discover(&root).unwrap();
        assert!(repository.stashes().unwrap().is_empty(), "no stash yet");
        let head = repository.head().unwrap().commit.unwrap();
        assert_eq!(repository.resolve("main").unwrap(), head);
        assert_eq!(repository.resolve(&head[..7]).unwrap(), head, "the start of an id");
        assert!(repository.resolve("--all").is_err(), "never an option");
        assert!(repository.resolve("nothing-here").is_err());

        let refs = repository.refs_stamp();
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        git(&root, &["add", "a.txt"]);
        assert_eq!(repository.refs_stamp(), refs, "staging leaves the refs alone");
        git(&root, &["stash", "push", "-q", "-m", "Halfway"]);
        std::fs::write(root.join("a.txt"), "three\n").unwrap();
        git(&root, &["stash", "push", "-q"]);
        assert_ne!(repository.refs_stamp(), refs, "a stash changes it");
        let stashes = repository.stashes().unwrap();
        assert_eq!(stashes.len(), 2);
        assert_eq!(stashes[0].index, 0);
        assert!(stashes[0].message.starts_with("WIP on main"), "{}", stashes[0].message);
        assert_eq!(stashes[1].message, "On main: Halfway");
        assert!(stashes.iter().all(|stash| stash.base == head), "both made on the first commit");
        git(&root, &["tag", "v1"]);
        assert_eq!(repository.resolve("v1").unwrap(), head);
    }

    #[test]
    fn names_that_read_as_options_never_reach_git() {
        let (_dir, repository, _) = two_branches();
        for name in ["-D", "--force", "-", " "] {
            assert!(repository.create_branch(name, Some("main"), false).is_err(), "{name}");
            assert!(repository.checkout(name).is_err(), "{name}");
        }
        assert!(repository.branches().unwrap().iter().any(|branch| branch.name == "main"), "not deleted");
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "kept\n").unwrap();
        assert!(repository.create_branch("new", Some("--discard-changes"), true).is_err());
        assert_eq!(read(&repository, "a.txt"), "kept\n");
        repository.create_branch("new", Some("main"), false).unwrap();
        git(&root, &["switch", "-q", "new"]);
        git(&root, &["switch", "-q", "main"]);
        assert!(repository.checkout("-").is_err(), "not the previous branch");
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("main"));
        git(&root, &["tag", "v1"]);
        let refused = repository.push_tag("v1", Some("--upload-pack=x"), &mut |_| {});
        assert!(matches!(refused, Err(VcsError::InvalidName { what: "remote", .. })), "{refused:?}");
    }

    #[test]
    fn a_remote_named_like_an_option_in_the_config_never_reaches_git() {
        let (_dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        let marker = root.join("ran");
        let evil = format!("--upload-pack=touch {}", marker.display());
        git(&root, &["config", &format!("remote.{evil}.url"), "/nowhere"]);
        git(&root, &["config", &format!("remote.{evil}.fetch"), "+refs/heads/*:refs/remotes/x/*"]);
        let outgoing = Outgoing {
            branch: "main".to_owned(),
            remote: evil.clone(),
            target: "main".to_owned(),
            new_branch: true,
            remote_tip: None,
            commits: Vec::new(),
        };
        let refused = repository.push(&outgoing, false, &mut |_| {});
        assert!(matches!(refused, Err(VcsError::InvalidName { what: "remote", .. })), "{refused:?}");
        assert!(!marker.exists());
    }

    #[test]
    fn discarding_never_deletes_outside_the_repository() {
        let (dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        let outside = dir.path().parent().unwrap().join(format!("zen-outside-{}", std::process::id()));
        std::fs::create_dir_all(&outside).unwrap();
        let victim = outside.join("x.txt");
        std::fs::write(&victim, "precious\n").unwrap();
        let up = format!("../{}/x.txt", outside.file_name().unwrap().to_str().unwrap());
        assert!(repository.discard(std::slice::from_ref(&up)).is_err());
        assert!(repository.rollback(std::slice::from_ref(&up), true).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
            assert!(repository.discard(&["link/x.txt".to_owned()]).is_err());
        }
        assert!(victim.exists());
        std::fs::remove_dir_all(&outside).unwrap();
    }

    fn with_a_bracket_folder() -> (tempfile::TempDir, Repository) {
        let (dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        for folder in ["app/[id]", "app/i", "app/d"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
            std::fs::write(root.join(folder).join("page.tsx"), "base\n").unwrap();
        }
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Pages"]);
        for folder in ["app/[id]", "app/i", "app/d"] {
            std::fs::write(root.join(folder).join("page.tsx"), "edited\n").unwrap();
        }
        (dir, repository)
    }

    const BRACKETS: &str = "app/[id]/page.tsx";

    #[test]
    fn rollback_of_a_name_with_brackets_touches_that_file_only() {
        let (_dir, repository) = with_a_bracket_folder();
        repository.rollback(&[BRACKETS.to_owned()], false).unwrap();
        assert_eq!(read(&repository, BRACKETS), "base\n");
        assert_eq!(read(&repository, "app/i/page.tsx"), "edited\n");
        assert_eq!(read(&repository, "app/d/page.tsx"), "edited\n");
    }

    #[test]
    fn discard_stage_and_unstage_take_a_name_with_brackets_literally() {
        let (_dir, repository) = with_a_bracket_folder();
        let path = [BRACKETS.to_owned()];
        repository.stage(&path).unwrap();
        assert_eq!(parts_of(&repository, "app/i/page.tsx").unwrap().0, None, "not staged");
        repository.stage(&["app/i/page.tsx".to_owned()]).unwrap();
        repository.unstage(&path).unwrap();
        assert!(parts_of(&repository, "app/i/page.tsx").unwrap().0.is_some(), "still staged");
        repository.discard(&path).unwrap();
        assert_eq!(read(&repository, BRACKETS), "base\n");
        assert_eq!(read(&repository, "app/d/page.tsx"), "edited\n");
    }

    #[test]
    fn a_partial_commit_of_a_name_with_brackets_commits_that_file_only() {
        let (_dir, repository) = with_a_bracket_folder();
        repository.commit("One", &[BRACKETS.to_owned()], false).unwrap();
        let changed = git::run(repository.root(), &["show", "--name-only", "--format="], None).unwrap().stdout;
        assert_eq!(changed.trim(), BRACKETS);
    }

    #[test]
    fn the_reflog_names_the_branches_checked_out_newest_first() {
        let reflog = "0 1 Ada <a@e> 1 +0000\tcommit (initial): init\n\
                      1 1 Ada <a@e> 2 +0000\tcheckout: moving from main to feature/x\n\
                      1 2 Ada <a@e> 3 +0000\tcommit: second\n\
                      2 2 Ada <a@e> 4 +0000\tcheckout: moving from feature/x to main\n\
                      2 2 Ada <a@e> 5 +0000\tcheckout: moving from main to 1234abcd\n";
        assert_eq!(checked_out(reflog), ["1234abcd", "main", "feature/x"]);
        assert!(checked_out("").is_empty());
    }

    #[test]
    fn tags_recent_branches_and_detached_checkouts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "First"]);
        git(&root, &["tag", "v1"]);
        git(&root, &["tag", "-a", "v0", "-m", "Annotated"]);
        let repository = Repository::discover(&root).unwrap();
        assert_eq!(repository.recent_branches(5).unwrap(), ["main"], "the current branch, without a checkout");
        repository.create_branch("feature/x", None, true).unwrap();
        repository.create_branch("gone", None, true).unwrap();
        repository.checkout("main").unwrap();
        repository.delete_branch("gone", false).unwrap();
        assert_eq!(repository.recent_branches(5).unwrap(), ["main", "feature/x"]);
        assert_eq!(repository.recent_branches(1).unwrap(), ["main"]);

        let tags = repository.tags().unwrap();
        let head = repository.head().unwrap().commit.unwrap();
        let names: Vec<(&str, &str)> = tags.iter().map(|tag| (tag.name.as_str(), tag.commit.as_str())).collect();
        assert_eq!(names, [("v0", head.as_str()), ("v1", head.as_str())], "an annotated tag names its commit");

        repository.checkout_revision("v1").unwrap();
        let detached = repository.head().unwrap();
        assert_eq!((detached.branch, detached.commit.as_deref()), (None, Some(head.as_str())));
        assert!(repository.checkout_revision("--orphan").is_err());
        assert!(repository.checkout_revision("no-such-revision").is_err());
        assert_eq!(repository.recent_branches(5).unwrap(), ["main", "feature/x"], "detached: no current branch");
    }

    #[test]
    fn log_filters_details_and_blame() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "First"]);
        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        git(&root, &["add", "-A"]);
        git(
            &root,
            &["-c", "user.name=Grace", "commit", "-q", "-m", "Second change", "--author", "Grace <grace@example.com>"],
        );
        git(&root, &["tag", "v1"]);
        let repository = Repository::discover(&root).unwrap();
        let all = repository.log(&LogQuery { limit: 10, ..LogQuery::default() }).unwrap();
        let subjects: Vec<&str> = all.iter().map(|entry| entry.commit.subject.as_str()).collect();
        assert_eq!(subjects, ["Second change", "First"]);
        assert!(all[0].refs.contains(&"v1".to_owned()));
        let by = |query: LogQuery| -> Vec<String> {
            repository
                .log(&LogQuery { limit: 10, ..query })
                .unwrap()
                .into_iter()
                .map(|entry| entry.commit.subject)
                .collect()
        };
        assert_eq!(by(LogQuery { author: Some("grace".into()), ..LogQuery::default() }), ["Second change"]);
        assert_eq!(by(LogQuery { text: Some("first".into()), ..LogQuery::default() }), ["First"]);
        assert_eq!(by(LogQuery { path: Some("b.txt".into()), ..LogQuery::default() }), ["Second change"]);
        assert_eq!(by(LogQuery { skip: 1, ..LogQuery::default() }), ["First"]);
        assert_eq!(by(LogQuery { branch: Some("HEAD~1".into()), ..LogQuery::default() }), ["First"]);

        let (commit, files) = repository.commit_details(&all[0].commit.id).unwrap();
        assert_eq!(commit.author, "Grace");
        let listed: Vec<(&str, FileStatus)> = files.iter().map(|file| (file.path.as_str(), file.status)).collect();
        assert_eq!(listed, [("a.txt", FileStatus::Modified), ("b.txt", FileStatus::Added)]);
        let (before, after) = repository.file_in_commit(&all[0].commit.id, "a.txt").unwrap();
        assert_eq!((before.as_deref(), after.as_deref()), (Some(&b"one\ntwo\n"[..]), Some(&b"one\nTWO\nthree\n"[..])));

        let blame = repository.blame("a.txt").unwrap();
        let authors: Vec<&str> = blame.iter().map(|line| line.author.as_str()).collect();
        assert_eq!(authors, ["Ada", "Grace", "Grace"]);
        assert_eq!(blame[1].subject, "Second change");
    }

    #[test]
    fn every_file_of_a_commit_is_listed_with_its_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(root.join("src/deep")).unwrap();
        std::fs::write(root.join("README.md"), "read me\n").unwrap();
        std::fs::write(root.join("src/main.go"), "package main\n").unwrap();
        std::fs::write(root.join("src/deep/util.go"), "package deep\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["-c", "user.name=Ada", "-c", "user.email=ada@example.com", "commit", "-q", "-m", "First"]);
        std::fs::remove_file(root.join("README.md")).unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["-c", "user.name=Ada", "-c", "user.email=ada@example.com", "commit", "-q", "-m", "Second"]);
        let repository = Repository::discover(&root).unwrap();
        assert_eq!(repository.commit_files("HEAD~1").unwrap(), ["README.md", "src/deep/util.go", "src/main.go"]);
        assert_eq!(repository.commit_files("HEAD").unwrap(), ["src/deep/util.go", "src/main.go"]);
        assert!(repository.commit_files("no-such-revision").is_err());
    }

    #[test]
    fn every_ref_labels_its_commit_and_the_log_filters_by_branches_and_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        git(&root, &["init", "-q", "-b", "main"]);
        let commit = |file: &str, message: &str| {
            std::fs::create_dir_all(root.join(file).parent().unwrap()).unwrap();
            std::fs::write(root.join(file), message).unwrap();
            git(&root, &["add", "-A"]);
            git(&root, &["commit", "-q", "-m", message]);
        };
        commit("a.txt", "Base");
        git(&root, &["tag", "v1"]);
        git(&root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
        git(&root, &["push", "-q", "-u", "origin", "main"]);
        git(&root, &["switch", "-q", "-c", "feature/x"]);
        commit("docs/x.md", "Feature");
        git(&root, &["switch", "-q", "main"]);
        commit("b.txt", "Ahead");
        let repository = Repository::discover(&root).unwrap();
        let all = repository.log(&LogQuery { limit: 10, ..LogQuery::default() }).unwrap();
        let labels = |subject: &str| -> Vec<(String, RefKind)> {
            let entry = all.iter().find(|entry| entry.commit.subject == subject).unwrap();
            entry.labels.iter().map(|label| (label.name.clone(), label.kind)).collect()
        };
        assert_eq!(labels("Ahead"), [("HEAD".to_owned(), RefKind::Head), ("main".to_owned(), RefKind::Local)]);
        assert_eq!(labels("Feature"), [("feature/x".to_owned(), RefKind::Local)]);
        assert_eq!(labels("Base"), [("origin/main".to_owned(), RefKind::Remote), ("v1".to_owned(), RefKind::Tag)]);
        let ahead = all.iter().find(|entry| entry.commit.subject == "Ahead").unwrap();
        assert_eq!(ahead.refs, ["HEAD", "main"], "the names too");

        let by = |query: LogQuery| -> Vec<String> {
            let entries = repository.log(&LogQuery { limit: 10, ..query }).unwrap();
            entries.into_iter().map(|entry| entry.commit.subject).collect()
        };
        let branches = |names: &[&str]| names.iter().map(|name| name.to_string()).collect::<Vec<_>>();
        let mut both = by(LogQuery { branches: branches(&["main", "feature/x"]), ..LogQuery::default() });
        both.sort();
        assert_eq!(both, ["Ahead", "Base", "Feature"]);
        assert_eq!(
            by(LogQuery { branches: branches(&["feature/x"]), exclude: branches(&["HEAD"]), ..LogQuery::default() }),
            ["Feature"],
            "what the branch adds to the current one"
        );
        assert_eq!(by(LogQuery { paths: branches(&["docs", "b.txt"]), ..LogQuery::default() }).len(), 2);
        assert_eq!(by(LogQuery { paths: branches(&["docs"]), ..LogQuery::default() }), ["Feature"]);
        assert_eq!(by(LogQuery { topological: true, ..LogQuery::default() }).len(), 3);
        assert!(repository.log(&LogQuery { text: Some("(".into()), regex: true, ..LogQuery::default() }).is_err());

        let base = all.iter().find(|entry| entry.commit.subject == "Base").unwrap().commit.id.clone();
        let containing: Vec<String> =
            repository.refs_containing(&base).unwrap().into_iter().map(|label| label.name).collect();
        assert_eq!(containing, ["feature/x", "main", "origin/main", "v1"]);
    }

    #[test]
    fn reads_head_status_files_and_branches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        assert!(Repository::discover(&root).unwrap().head().unwrap().commit.is_none(), "no commit yet");
        std::fs::create_dir(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.go"), "package a\n").unwrap();
        std::fs::write(root.join("gone.txt"), "x\n").unwrap();
        std::fs::write(root.join("old.txt"), "moved\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "First\n\nThe body."]);

        let repository = Repository::discover(&root.join("src")).unwrap();
        assert_eq!(repository.root(), root);
        let head = repository.head().unwrap();
        assert_eq!(head.branch.as_deref(), Some("main"));
        assert_eq!(head.commit.as_ref().map(String::len), Some(40));
        assert_eq!(repository.file_at_head("src/a.go").unwrap().as_deref(), Some(&b"package a\n"[..]));
        assert_eq!(repository.file_at_head("none.go").unwrap(), None);

        let stamp = repository.stamp();
        std::fs::write(root.join("src/a.go"), "package b\n").unwrap();
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        std::fs::write(root.join("new.txt"), "n\n").unwrap();
        git(&root, &["mv", "old.txt", "renamed.txt"]);
        let status = repository.status().unwrap();
        let listed: Vec<(&str, FileStatus, bool)> =
            status.changes.iter().map(|change| (change.path.as_str(), change.status, change.staged)).collect();
        assert_eq!(
            listed,
            [
                ("gone.txt", FileStatus::Deleted, false),
                ("new.txt", FileStatus::Unversioned, false),
                ("renamed.txt", FileStatus::Renamed, true),
                ("src/a.go", FileStatus::Modified, false),
            ]
        );
        assert_ne!(repository.stamp(), stamp, "the index changed");

        repository.create_branch("feature", None, false).unwrap();
        let branches = repository.branches().unwrap();
        let names: Vec<(&str, bool)> = branches.iter().map(|branch| (branch.name.as_str(), branch.current)).collect();
        assert_eq!(names, [("feature", false), ("main", true)]);
        let commit =
            read_commit(&repository.open().unwrap(), repository.open().unwrap().head_id().unwrap().detach()).unwrap();
        assert_eq!(
            (commit.subject.as_str(), commit.body.as_str(), commit.author.as_str()),
            ("First", "The body.", "Ada")
        );
    }

    fn isolate() {
        git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
        ]);
    }

    fn two_branches() -> (tempfile::TempDir, Repository, [String; 3]) {
        isolate();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "one\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "First"]);
        git(&root, &["switch", "-q", "-c", "feature"]);
        std::fs::write(root.join("f.txt"), "feature\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Feature"]);
        git(&root, &["switch", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "one\ntwo\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Second"]);
        let repository = Repository::discover(&root).unwrap();
        let ids = ["main~1", "main", "feature"].map(|revision| repository.resolve(revision).unwrap());
        (dir, repository, ids)
    }

    #[test]
    fn cherry_picks_and_reverts_make_new_commits() {
        let (_dir, repository, [first, second, feature]) = two_branches();
        let root = repository.root().to_owned();
        assert!(repository.is_ancestor(&first, "HEAD").unwrap());
        assert!(!repository.is_ancestor(&feature, "HEAD").unwrap());

        let picked = repository.cherry_pick(&feature[..8], None).unwrap();
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(picked.as_str()));
        assert_eq!(std::fs::read_to_string(root.join("f.txt")).unwrap(), "feature\n");
        let (commit, _) = repository.commit_details(&picked).unwrap();
        assert_eq!((commit.subject.as_str(), commit.parents.as_slice()), ("Feature", [second.clone()].as_slice()));

        let reverted = repository.revert(&picked, None).unwrap();
        let (commit, _) = repository.commit_details(&reverted).unwrap();
        assert_eq!(commit.subject, "Revert \"Feature\"");
        assert!(!root.join("f.txt").exists(), "the revert takes the file away again");

        git(&root, &["merge", "-q", "--no-ff", "-m", "Merge feature", "feature"]);
        let merge = repository.resolve("HEAD").unwrap();
        let undone = repository.revert(&merge, None).unwrap();
        assert_eq!(repository.commit_details(&undone).unwrap().0.subject, "Revert \"Merge feature\"");
        assert!(!root.join("f.txt").exists());

        assert!(repository.cherry_pick("--abort", None).is_err(), "a revision is never an option");
        assert!(repository.revert("no-such-commit", None).is_err());
        assert!(repository.conflicted_files().is_empty());
    }

    #[test]
    fn a_conflict_leaves_the_operation_in_progress_and_names_the_files() {
        let (_dir, repository, [first, _, _]) = two_branches();
        let root = repository.root().to_owned();
        git(&root, &["switch", "-q", "-c", "other", &first]);
        std::fs::write(root.join("a.txt"), "uno\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Other"]);
        let other = repository.resolve("HEAD").unwrap();
        git(&root, &["switch", "-q", "main"]);

        let stopped = repository.cherry_pick(&other, None).unwrap_err();
        assert_eq!(stopped, VcsError::Conflicts { operation: Applying::CherryPick, files: vec!["a.txt".to_owned()] });
        assert!(
            repository.git_dir.join("CHERRY_PICK_HEAD").exists(),
            "the cherry-pick is in progress, as git leaves it"
        );
        assert!(std::fs::read_to_string(root.join("a.txt")).unwrap().contains("<<<<<<<"));
        let status = repository.status().unwrap();
        assert_eq!(status.changes[0].status, FileStatus::Conflicted);
        git(&root, &["cherry-pick", "--abort"]);

        let revert = repository.revert("main~1", None).unwrap_err();
        assert!(matches!(revert, VcsError::Conflicts { operation: Applying::Revert, .. }), "{revert:?}");
        assert!(repository.git_dir.join("REVERT_HEAD").exists());
        assert!(revert.to_string().starts_with("revert stopped with conflicts in a.txt"));
    }

    #[test]
    fn resets_move_the_branch_soft_mixed_and_hard() {
        let (_dir, repository, [first, second, _]) = two_branches();
        let root = repository.root().to_owned();
        let staged = |repository: &Repository| {
            repository
                .status()
                .unwrap()
                .changes
                .iter()
                .map(|change| (change.path.clone(), change.staged))
                .collect::<Vec<_>>()
        };

        assert_eq!(repository.reset(&first, ResetMode::Soft).unwrap(), first);
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("main"), "the branch moves, HEAD stays on it");
        assert_eq!(staged(&repository), [("a.txt".to_owned(), true)], "soft: the difference is staged");

        repository.reset(&second, ResetMode::Hard).unwrap();
        assert_eq!(repository.reset(&first, ResetMode::Mixed).unwrap(), first);
        assert_eq!(staged(&repository), [("a.txt".to_owned(), false)], "mixed: unstaged");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\n");

        std::fs::write(root.join("a.txt"), "scratch\n").unwrap();
        assert_eq!(repository.reset(&first, ResetMode::Hard).unwrap(), first);
        assert!(staged(&repository).is_empty(), "hard: nothing is left");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\n");
        assert!(repository.reset("-q", ResetMode::Hard).is_err());
    }

    #[test]
    fn tags_are_created_lightweight_or_annotated() {
        let (_dir, repository, [first, second, _]) = two_branches();
        assert_eq!(repository.create_tag("v1", None, None).unwrap(), second);
        assert_eq!(repository.create_tag("v0", Some(&first[..7]), Some("The first one\n\n-m is text")).unwrap(), first);
        let tags: Vec<(String, String)> =
            repository.tags().unwrap().into_iter().map(|tag| (tag.name, tag.commit)).collect();
        assert_eq!(tags, [("v0".to_owned(), first.clone()), ("v1".to_owned(), second)]);
        let message = git::run(repository.root(), &["tag", "-l", "--format=%(contents)", "v0"], None).unwrap().stdout;
        assert_eq!(message.trim(), "The first one\n\n-m is text");
        assert!(repository.create_tag("v1", None, None).is_err(), "taken");
        assert!(repository.create_tag(" ", None, None).is_err());
        assert!(repository.create_tag("-d", None, None).is_err());
        assert!(repository.create_tag("bad name", None, None).is_err());
    }

    fn with_remote() -> (tempfile::TempDir, tempfile::TempDir, Repository, [String; 3]) {
        let (dir, repository, ids) = two_branches();
        let remote = tempfile::tempdir().unwrap();
        git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
        let root = repository.root().to_owned();
        git(&root, &["remote", "add", "origin", remote.path().to_str().unwrap()]);
        git(&root, &["push", "-q", "-u", "origin", "main", "feature"]);
        (dir, remote, repository, ids)
    }

    fn upstream_of(repository: &Repository, name: &str) -> Option<String> {
        repository.branches().unwrap().into_iter().find(|branch| branch.name == name).and_then(|branch| branch.upstream)
    }

    #[test]
    fn branches_are_renamed_and_given_upstreams() {
        let (_dir, _remote, repository, [_, _, feature]) = with_remote();
        assert_eq!(upstream_of(&repository, "feature").as_deref(), Some("origin/feature"));
        repository.rename_branch("feature", " topic ").unwrap();
        let branches = repository.branches().unwrap();
        let topic = branches.iter().find(|branch| branch.name == "topic").unwrap();
        assert_eq!(topic.commit, feature);
        assert_eq!(topic.upstream.as_deref(), Some("origin/feature"), "the upstream goes with the name");
        assert!(!branches.iter().any(|branch| branch.name == "feature" && !branch.remote));
        assert!(repository.rename_branch("topic", "main").is_err(), "taken");
        assert!(repository.rename_branch("nope", "other").is_err());
        assert!(repository.rename_branch("topic", "-f").is_err());
        assert!(repository.rename_branch("topic", "two words").is_err());
        repository.rename_branch("main", "trunk").unwrap();
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("trunk"), "the current one too");

        repository.set_upstream("topic", Some("origin/main")).unwrap();
        assert_eq!(upstream_of(&repository, "topic").as_deref(), Some("origin/main"));
        repository.set_upstream("topic", None).unwrap();
        assert_eq!(upstream_of(&repository, "topic"), None);
        repository.set_upstream("topic", None).unwrap();
        assert!(repository.set_upstream("topic", Some("origin/nope")).is_err());
        assert!(repository.set_upstream("nope", Some("origin/main")).is_err());
    }

    #[test]
    fn branches_fast_forward_to_their_upstream_or_are_refused() {
        let (_dir, remote, repository, [first, second, _]) = with_remote();
        let root = repository.root().to_owned();
        let other = tempfile::tempdir().unwrap();
        git(other.path(), &["clone", "-q", remote.path().to_str().unwrap(), "clone"]);
        let clone = other.path().join("clone");
        std::fs::write(clone.join("c.txt"), "c\n").unwrap();
        git(&clone, &["add", "-A"]);
        git(&clone, &["commit", "-q", "-m", "Third"]);
        git(&clone, &["push", "-q", "origin", "main"]);
        git(&root, &["fetch", "-q", "origin"]);
        let third = repository.resolve("origin/main").unwrap();

        assert_eq!(repository.fast_forward("main", None).unwrap(), third);
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(third.as_str()));
        assert!(root.join("c.txt").exists());
        assert_eq!(repository.fast_forward("main", None).unwrap(), third, "up to date: nothing to do");

        git(&root, &["branch", "old", &first]);
        assert_eq!(repository.fast_forward("old", Some("main~1")).unwrap(), second);
        assert_eq!(repository.resolve("old").unwrap(), second);
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("main"));
        let refused = repository.fast_forward("feature", Some("main")).unwrap_err();
        assert_eq!(refused, VcsError::NotFastForward { branch: "feature".to_owned(), target: "main".to_owned() });
        assert_eq!(repository.fast_forward("old", None).unwrap_err(), VcsError::NoRemote, "no upstream");
        assert!(repository.fast_forward("nope", None).is_err());
    }

    #[test]
    fn merges_and_rebases_stop_on_conflicts_as_git_leaves_them() {
        let (_dir, repository, [first, _, feature]) = two_branches();
        let root = repository.root().to_owned();
        let head = repository.rebase("feature").unwrap();
        assert_eq!(repository.head().unwrap().commit.as_deref(), Some(head.as_str()));
        assert!(repository.is_ancestor(&feature, "HEAD").unwrap());
        assert_eq!(repository.commit_details("HEAD").unwrap().0.subject, "Second");

        git(&root, &["switch", "-q", "-c", "other", &first]);
        std::fs::write(root.join("a.txt"), "uno\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Other"]);
        git(&root, &["switch", "-q", "main"]);
        let merge = repository.merge("other").unwrap_err();
        assert_eq!(merge, VcsError::Conflicts { operation: Applying::Merge, files: vec!["a.txt".to_owned()] });
        assert!(repository.git_dir.join("MERGE_HEAD").exists(), "the merge is in progress");
        git(&root, &["merge", "--abort"]);

        let rebase = repository.rebase("other").unwrap_err();
        assert_eq!(rebase, VcsError::Conflicts { operation: Applying::Rebase, files: vec!["a.txt".to_owned()] });
        assert!(
            repository.git_dir.join("rebase-merge").exists() || repository.git_dir.join("rebase-apply").exists(),
            "the rebase is in progress"
        );
        git(&root, &["rebase", "--abort"]);
        assert!(repository.rebase("--root").is_err(), "a revision is never an option");
        assert!(repository.merge("no-such-branch").is_err());
        assert!(repository.conflicted_files().is_empty());
    }

    #[test]
    fn merges_into_and_rebases_of_another_branch_check_it_out_first() {
        let (_dir, repository, [first, second, feature]) = two_branches();
        let root = repository.root().to_owned();
        assert!(!repository.has_tracked_changes().unwrap());
        std::fs::write(root.join("new.txt"), "untracked\n").unwrap();
        assert!(!repository.has_tracked_changes().unwrap(), "untracked files do not count");
        std::fs::remove_file(root.join("new.txt")).unwrap();
        std::fs::write(root.join("a.txt"), "edited\n").unwrap();
        assert!(repository.has_tracked_changes().unwrap());
        git(&root, &["checkout", "-q", "--", "a.txt"]);

        repository.merge_into("main", "feature").unwrap();
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("feature"));
        assert!(repository.is_ancestor(&second, "feature").unwrap());
        assert!(repository.is_ancestor(&feature, "feature").unwrap());
        assert_eq!(repository.resolve("main").unwrap(), second, "main stays where it was");
        git(&root, &["switch", "-q", "main"]);
        git(&root, &["branch", "topic", &first]);
        repository.merge_into("topic", "main").unwrap();
        assert_eq!(repository.resolve("main").unwrap(), second, "topic is already in main");
        git(&root, &["update-ref", "refs/remotes/origin/guess", &first]);
        assert!(repository.merge_into("main", "guess").is_err());
        assert!(repository.merge_into("main", "--orphan").is_err());
        assert!(repository.merge_into("nope", "topic").is_err());
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("main"));

        let head = repository.rebase_branch("topic", "feature").unwrap();
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("topic"));
        assert_eq!(repository.resolve("topic").unwrap(), head);
        assert!(repository.is_ancestor("feature", "topic").unwrap());
        git(&root, &["switch", "-q", "-c", "late", &first]);
        std::fs::write(root.join("l.txt"), "late\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Late"]);
        repository.rebase_branch("late", "main").unwrap();
        assert!(repository.is_ancestor("main", "late").unwrap());
        assert!(repository.rebase_branch("origin/guess", "main").is_err(), "a local branch only");
        assert!(repository.rebase_branch("late", "nope").is_err());
    }

    #[test]
    fn tags_and_remote_branches_are_pushed_and_deleted_on_a_remote() {
        let (_dir, remote, repository, [first, ..]) = with_remote();
        let bare = remote.path();
        let on_remote = |name: &str| {
            std::process::Command::new("git")
                .current_dir(bare)
                .args(["rev-parse", "--verify", "-q", name])
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .output()
                .unwrap()
                .status
                .success()
        };
        repository.create_tag("v1", Some(&first), Some("One")).unwrap();
        assert!(!on_remote("refs/tags/v1"));
        assert_eq!(repository.push_tag("v1", None, &mut |_| {}).unwrap(), "origin");
        assert!(on_remote("refs/tags/v1"));
        assert!(repository.push_tag("v2", None, &mut |_| {}).is_err(), "no such tag");
        repository.delete_tag("v1").unwrap();
        assert!(repository.tags().unwrap().is_empty());
        assert!(on_remote("refs/tags/v1"), "deleted here only");
        assert!(repository.delete_tag("v1").is_err());

        git(repository.root(), &["branch", "topic", &first]);
        let outgoing = repository.outgoing_of(Some("topic")).unwrap();
        assert_eq!((outgoing.branch.as_str(), outgoing.remote.as_str()), ("topic", "origin"));
        assert!(outgoing.new_branch);
        repository.push(&outgoing, false, &mut |_| {}).unwrap();
        assert!(on_remote("refs/heads/topic"));
        assert_eq!(upstream_of(&repository, "topic").as_deref(), Some("origin/topic"));
        assert!(repository.outgoing_of(Some("nope")).is_err());

        repository.delete_remote_branch("origin/feature", &mut |_| {}).unwrap();
        assert!(!on_remote("refs/heads/feature"));
        let remote_branches: Vec<String> = repository
            .branches()
            .unwrap()
            .into_iter()
            .filter(|branch| branch.remote)
            .map(|branch| branch.name)
            .collect();
        assert_eq!(remote_branches, ["origin/main", "origin/topic"], "the remote-tracking branch goes too");
        assert!(repository.delete_remote_branch("upstream/main", &mut |_| {}).is_err(), "no such remote");
        assert_eq!(repository.remotes().unwrap(), ["origin"]);
    }

    fn read(repository: &Repository, file: &str) -> String {
        std::fs::read_to_string(repository.root().join(file)).unwrap_or_default()
    }

    #[test]
    fn stashes_are_pushed_all_or_staged_only_and_applied_popped_and_dropped() {
        let (_dir, repository, [_, second, _]) = two_branches();
        assert_eq!(repository.stash_push(None, false, true).unwrap(), None, "nothing to put aside");

        std::fs::write(repository.root().join("a.txt"), "changed\n").unwrap();
        std::fs::write(repository.root().join("new.txt"), "new\n").unwrap();
        let stash = repository.stash_push(Some("  -Halfway "), false, true).unwrap().unwrap();
        assert_eq!(
            (stash.index, stash.message.as_str(), stash.base.as_str()),
            (0, "On main: -Halfway", second.as_str())
        );
        assert!(repository.status().unwrap().changes.is_empty(), "the working tree is clean");
        assert!(!repository.root().join("new.txt").exists(), "the untracked file went with it");

        std::fs::write(repository.root().join("a.txt"), "staged\n").unwrap();
        git(repository.root(), &["add", "a.txt"]);
        std::fs::write(repository.root().join("f.txt"), "unstaged\n").unwrap();
        git(repository.root(), &["add", "f.txt"]);
        std::fs::write(repository.root().join("f.txt"), "unstaged, later\n").unwrap();
        git(repository.root(), &["reset", "-q", "f.txt"]);
        let staged = repository.stash_push(None, true, false).unwrap().unwrap();
        assert!(staged.message.starts_with("WIP on main"), "{}", staged.message);
        assert_eq!(read(&repository, "a.txt"), "one\ntwo\n", "the staged change is put aside");
        assert_eq!(read(&repository, "f.txt"), "unstaged, later\n", "the rest stays");
        std::fs::remove_file(repository.root().join("f.txt")).unwrap();
        assert_eq!(repository.stashes().unwrap().len(), 2);

        repository.stash_apply(1, false).unwrap();
        assert_eq!((read(&repository, "a.txt"), read(&repository, "new.txt")), ("changed\n".into(), "new\n".into()));
        assert_eq!(repository.stashes().unwrap().len(), 2, "apply keeps it");
        git(repository.root(), &["reset", "-q", "--hard"]);
        git(repository.root(), &["clean", "-q", "-f"]);
        repository.stash_apply(0, true).unwrap();
        assert_eq!(read(&repository, "a.txt"), "staged\n");
        let left = repository.stashes().unwrap();
        assert_eq!((left.len(), left[0].message.as_str()), (1, "On main: -Halfway"), "pop drops it");
        repository.stash_drop(0).unwrap();
        assert!(repository.stashes().unwrap().is_empty());
        assert!(repository.stash_drop(0).is_err(), "no stash left");
        assert!(repository.stash_apply(3, false).is_err());
    }

    #[test]
    fn a_stash_that_conflicts_stops_and_is_kept() {
        let (_dir, repository, _) = two_branches();
        std::fs::write(repository.root().join("a.txt"), "one\nmine\n").unwrap();
        repository.stash_push(Some("mine"), false, true).unwrap().unwrap();
        std::fs::write(repository.root().join("a.txt"), "one\ntheirs\n").unwrap();
        git(repository.root(), &["commit", "-q", "-am", "Theirs"]);
        let stopped = repository.stash_apply(0, true).unwrap_err();
        assert_eq!(stopped, VcsError::Conflicts { operation: Applying::StashPop, files: vec!["a.txt".to_owned()] });
        assert_eq!(repository.stashes().unwrap().len(), 1, "git keeps a stash that did not pop cleanly");
        assert!(read(&repository, "a.txt").contains("<<<<<<<"), "the markers are left in the file");
        git(repository.root(), &["reset", "-q", "--hard"]);
        std::fs::write(repository.root().join("a.txt"), "in the way\n").unwrap();
        let refused = repository.stash_apply(0, false).unwrap_err();
        assert!(matches!(refused, VcsError::Git(_)), "{refused:?}");
        assert_eq!(read(&repository, "a.txt"), "in the way\n");
    }

    #[test]
    fn renaming_a_stash_keeps_the_order_the_times_and_the_changes() {
        let (_dir, repository, _) = two_branches();
        for (day, text) in ["oldest", "middle", "newest"].into_iter().enumerate() {
            std::fs::write(repository.root().join("a.txt"), format!("{text}\n")).unwrap();
            let commit = git::run(repository.root(), &["stash", "create"], None).unwrap().stdout;
            git::run(repository.root(), &["reset", "-q", "--hard"], None).unwrap();
            let date = format!("@{} +0000", 1_600_000_000 + day * 86_400);
            let message = format!("--message=On main: {text}");
            let args = ["stash", "store", message.as_str(), commit.trim()];
            git::run_with_environment(repository.root(), &args, None, &[("GIT_COMMITTER_DATE", date.as_str())])
                .unwrap();
        }
        let before = repository.stashes().unwrap();
        assert_eq!(before[2].time, 1_600_000_000, "made in the past");
        let renamed = repository.stash_rename(1, "  the middle one ").unwrap();
        assert_eq!((renamed.index, renamed.message.as_str()), (1, "On main: the middle one"));
        let after = repository.stashes().unwrap();
        let messages: Vec<&str> = after.iter().map(|stash| stash.message.as_str()).collect();
        assert_eq!(messages, ["On main: newest", "On main: the middle one", "On main: oldest"]);
        for (old, new) in before.iter().zip(&after) {
            assert_eq!((&old.commit, &old.base, old.time), (&new.commit, &new.base, new.time), "{}", new.message);
        }
        assert!(repository.stash_rename(1, " ").is_err(), "a message is needed");
        assert!(repository.stash_rename(7, "nope").is_err());
        assert_eq!(repository.stashes().unwrap(), after, "a refused rename changes nothing");
    }

    fn parts_of(repository: &Repository, path: &str) -> Option<(Option<FileStatus>, Option<FileStatus>)> {
        let status = repository.status().unwrap();
        status.changes.into_iter().find(|change| change.path == path).map(|change| (change.index, change.worktree))
    }

    #[test]
    fn files_are_staged_unstaged_and_their_parts_listed_apart() {
        let (_dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        assert_eq!(parts_of(&repository, "a.txt"), Some((None, Some(FileStatus::Modified))));
        assert_eq!(parts_of(&repository, "new.txt"), Some((None, Some(FileStatus::Unversioned))));
        repository.stage(&["a.txt".to_owned(), "new.txt".to_owned()]).unwrap();
        assert_eq!(parts_of(&repository, "a.txt"), Some((Some(FileStatus::Modified), None)));
        assert_eq!(parts_of(&repository, "new.txt"), Some((Some(FileStatus::Added), None)));
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        assert_eq!(parts_of(&repository, "a.txt"), Some((Some(FileStatus::Modified), Some(FileStatus::Modified))));
        assert_eq!(repository.file_in_index("a.txt").unwrap().as_deref(), Some(&b"one\ntwo\nthree\n"[..]));
        assert_eq!(repository.file_in_index("none.txt").unwrap(), None);
        repository.unstage(&["new.txt".to_owned()]).unwrap();
        assert_eq!(parts_of(&repository, "new.txt"), Some((None, Some(FileStatus::Unversioned))));
        repository.stage(&[]).unwrap();
        assert!(repository.status().unwrap().changes.iter().all(|change| change.worktree.is_none()));
        repository.unstage(&[]).unwrap();
        assert!(repository.status().unwrap().changes.iter().all(|change| change.index.is_none()));
        std::fs::remove_file(root.join("a.txt")).unwrap();
        repository.stage(&["a.txt".to_owned()]).unwrap();
        assert_eq!(parts_of(&repository, "a.txt"), Some((Some(FileStatus::Deleted), None)));
    }

    #[test]
    fn files_are_unstaged_before_the_first_commit() {
        isolate();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        std::fs::write(root.join("a.txt"), "a\n").unwrap();
        let repository = Repository::discover(&root).unwrap();
        repository.stage(&[]).unwrap();
        assert_eq!(parts_of(&repository, "a.txt"), Some((Some(FileStatus::Added), None)));
        repository.unstage(&["a.txt".to_owned()]).unwrap();
        assert_eq!(parts_of(&repository, "a.txt"), Some((None, Some(FileStatus::Unversioned))));
        repository.stage(&[]).unwrap();
        repository.unstage(&[]).unwrap();
        assert_eq!(parts_of(&repository, "a.txt"), Some((None, Some(FileStatus::Unversioned))));
    }

    #[test]
    fn some_lines_are_staged_and_only_what_is_staged_is_committed() {
        let (_dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "ONE\ntwo\nthree\n").unwrap();
        let index = String::from_utf8(repository.file_in_index("a.txt").unwrap().unwrap()).unwrap();
        let worktree = read(&repository, "a.txt");
        let staged = crate::diff::combine(&index, &worktree, &|_| false, &|line| line == 2);
        repository.set_file_in_index("a.txt", &staged).unwrap();
        assert_eq!(parts_of(&repository, "a.txt"), Some((Some(FileStatus::Modified), Some(FileStatus::Modified))));
        repository.commit_staged("Add three", false).unwrap();
        let committed = |repository: &Repository| String::from_utf8(repository.file_at_head("a.txt").unwrap().unwrap());
        assert_eq!(committed(&repository).unwrap(), "one\ntwo\nthree\n");
        assert_eq!(read(&repository, "a.txt"), "ONE\ntwo\nthree\n", "the rest stays in the working tree");
        assert_eq!(repository.last_message().unwrap().as_deref(), Some("Add three"));
        assert!(repository.commit_staged("Nothing", false).is_err(), "nothing staged");
        repository.stage(&["a.txt".to_owned()]).unwrap();
        repository.commit_staged("Add three and shout", true).unwrap();
        assert_eq!(repository.last_message().unwrap().as_deref(), Some("Add three and shout"));
        assert_eq!(repository.count_between("main~1", "main").unwrap(), 1, "amended, not added");
        assert_eq!(committed(&repository).unwrap(), "ONE\ntwo\nthree\n");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(root.join("run.sh"), "echo 1\n").unwrap();
            std::fs::set_permissions(root.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
            repository.set_file_in_index("run.sh", "echo 1\n").unwrap();
            let listed = git::run(&root, &["ls-files", "-s", "run.sh"], None).unwrap();
            assert!(listed.stdout.starts_with("100755"), "{}", listed.stdout);
        }
    }

    #[test]
    fn discarding_keeps_what_is_staged_and_deletes_unversioned_files() {
        let (_dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "one\ntwo\nstaged\n").unwrap();
        repository.stage(&["a.txt".to_owned()]).unwrap();
        std::fs::write(root.join("a.txt"), "one\ntwo\nstaged\nnot staged\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        repository.discard(&["a.txt".to_owned(), "new.txt".to_owned()]).unwrap();
        assert_eq!(read(&repository, "a.txt"), "one\ntwo\nstaged\n", "back to what is staged");
        assert!(!root.join("new.txt").exists());
        assert_eq!(parts_of(&repository, "a.txt"), Some((Some(FileStatus::Modified), None)));
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(root.join("other.txt"), "other\n").unwrap();
        repository.discard(&[]).unwrap();
        assert_eq!(read(&repository, "a.txt"), "one\ntwo\nstaged\n");
        assert!(!root.join("other.txt").exists());
    }

    #[test]
    fn a_tag_message_starting_with_a_dash_stays_a_message() {
        let (_dir, repository, _) = two_branches();
        repository.create_tag("v1", None, Some("--force")).unwrap();
        let message = git::run(repository.root(), &["tag", "-l", "--format=%(contents)", "v1"], None).unwrap().stdout;
        assert_eq!(message.trim(), "--force");
    }

    #[test]
    fn a_stash_message_starting_with_a_dash_stays_a_message() {
        let (_dir, repository, _) = two_branches();
        std::fs::write(repository.root().join("a.txt"), "changed\n").unwrap();
        let stash = repository.stash_push(Some("--staged"), false, false).unwrap().unwrap();
        assert!(stash.message.ends_with("--staged"), "{}", stash.message);
    }

    #[test]
    fn a_revision_starting_with_a_dash_is_refused() {
        let (_dir, repository, _) = two_branches();
        repository.checkout("feature").unwrap();
        assert_eq!(repository.checkout_revision("-"), Err(VcsError::Git("invalid revision '-'".to_owned())));
        assert_eq!(repository.head().unwrap().branch.as_deref(), Some("feature"));
    }

    #[test]
    fn a_name_that_is_not_ascii_comes_back_as_it_is_in_a_conflict() {
        let (_dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        let name = "файл.txt";
        std::fs::write(root.join(name), "base\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Base"]);
        git(&root, &["switch", "-q", "-c", "other"]);
        std::fs::write(root.join(name), "theirs\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Theirs"]);
        git(&root, &["switch", "-q", "-"]);
        std::fs::write(root.join(name), "ours\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Ours"]);
        let _ = git::run(&root, &["merge", "--no-edit", "other"], None);
        assert_eq!(repository.conflicted_files(), [name]);
    }

    #[test]
    fn a_stash_with_an_empty_message_counts_as_git_counts_it() {
        let (_dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        for text in ["first", "second"] {
            std::fs::write(root.join("a.txt"), format!("{text}\n")).unwrap();
            git(&root, &["stash", "push", "-q", "-m", text]);
        }
        let commit = git::run(&root, &["rev-parse", "stash@{1}"], None).unwrap().stdout;
        git(&root, &["stash", "store", "-m", "", commit.trim()]);
        let listed = git::run(&root, &["stash", "list", "--format=%H"], None).unwrap().stdout;
        let ours: Vec<String> = repository.stashes().unwrap().into_iter().map(|stash| stash.commit).collect();
        assert_eq!(ours, listed.lines().collect::<Vec<_>>());
        repository.stash_rename(0, "named").unwrap();
        let subject = git::run(&root, &["stash", "list", "-1", "--format=%gs"], None).unwrap().stdout;
        assert_eq!(subject.trim(), "named");
    }

    #[test]
    fn renaming_a_stash_touches_nothing_when_it_is_refused() {
        let (_dir, repository, _) = two_branches();
        let root = repository.root().to_owned();
        for text in ["first", "second", "third"] {
            std::fs::write(root.join("a.txt"), format!("{text}\n")).unwrap();
            git(&root, &["stash", "push", "-q", "-m", text]);
        }
        let before = repository.stashes().unwrap();
        let lock = root.join(".git/refs/stash.lock");
        std::fs::write(&lock, "").unwrap();
        assert!(repository.stash_rename(2, "renamed").is_err());
        std::fs::remove_file(&lock).unwrap();
        assert_eq!(repository.stashes().unwrap(), before);

        repository.stash_rename(2, "line\nbreak").unwrap();
        let after = repository.stashes().unwrap();
        assert_eq!(after[2].message, "On main: line break");
        assert!(!root.join(".git/refs/stash.lock").exists() && !root.join(".git/logs/refs/stash.lock").exists());
        let listed = git::run(&root, &["stash", "list", "--format=%H %gs"], None).unwrap().stdout;
        assert_eq!(listed.lines().nth(2), Some(format!("{} On main: line break", after[2].commit).as_str()));
    }

    fn pushed_clones() -> (tempfile::TempDir, Repository, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let top = dir.path().canonicalize().unwrap();
        git(&top, &["init", "-q", "--bare", "-b", "main", "remote.git"]);
        git(&top, &["clone", "-q", "remote.git", "mine"]);
        let mine = top.join("mine");
        git(&mine, &["switch", "-q", "-c", "main"]);
        std::fs::write(mine.join("a.txt"), "one\n").unwrap();
        git(&mine, &["add", "-A"]);
        git(&mine, &["commit", "-q", "-m", "First"]);
        std::fs::write(mine.join("a.txt"), "two\n").unwrap();
        git(&mine, &["commit", "-q", "-a", "-m", "Second"]);
        git(&mine, &["push", "-q", "-u", "origin", "main"]);
        git(&top, &["clone", "-q", "remote.git", "theirs"]);
        let theirs = top.join("theirs");
        std::fs::write(theirs.join("b.txt"), "b\n").unwrap();
        git(&theirs, &["add", "-A"]);
        git(&theirs, &["commit", "-q", "-m", "Theirs"]);
        let repository = Repository::discover(&mine).unwrap();
        (dir, repository, theirs)
    }

    fn remote_tip(repository: &Repository) -> String {
        let bare = repository.root().parent().unwrap().join("remote.git");
        git::run(&bare, &["rev-parse", "refs/heads/main"], None).unwrap().stdout.trim().to_owned()
    }

    #[test]
    fn force_push_replaces_only_what_was_seen_and_brought_in() {
        let (_dir, repository, theirs) = pushed_clones();
        let root = repository.root().to_owned();
        std::fs::write(root.join("a.txt"), "amended\n").unwrap();
        git(&root, &["commit", "-q", "-a", "--amend", "-m", "Amended"]);
        let amended = repository.head().unwrap().commit.unwrap();
        repository.push(&repository.outgoing().unwrap(), true, &mut |_| {}).unwrap();
        assert_eq!(remote_tip(&repository), amended, "a rewritten branch of mine is replaced");

        git(&theirs, &["pull", "-q", "--rebase", "origin", "main"]);
        git(&theirs, &["push", "-q", "origin", "HEAD:main"]);
        let others = remote_tip(&repository);
        std::fs::write(root.join("a.txt"), "three\n").unwrap();
        git(&root, &["commit", "-q", "-a", "--amend", "-m", "Again"]);
        let not_fetched = repository.push(&repository.outgoing().unwrap(), true, &mut |_| {});
        assert!(not_fetched.is_err() && remote_tip(&repository) == others, "not fetched yet");

        git(&root, &["fetch", "-q", "origin"]);
        let not_brought_in = repository.push(&repository.outgoing().unwrap(), true, &mut |_| {});
        assert!(not_brought_in.is_err(), "fetched, but never in my branch");
        assert_eq!(remote_tip(&repository), others, "their commit stays");

        git(&root, &["merge", "-q", "-X", "ours", "--no-edit", "origin/main"]);
        let rewritten = repository.head().unwrap().commit.unwrap();
        repository.push(&repository.outgoing().unwrap(), true, &mut |_| {}).unwrap();
        assert_eq!(remote_tip(&repository), rewritten, "brought in, then pushed");
    }

    #[test]
    fn outgoing_names_the_known_tip_of_the_remote_branch() {
        let (_dir, repository, _) = pushed_clones();
        let outgoing = repository.outgoing().unwrap();
        assert_eq!(outgoing.remote_tip.as_deref(), Some(remote_tip(&repository).as_str()));
        let root = repository.root().to_owned();
        git(&root, &["switch", "-q", "-c", "new"]);
        let new = repository.outgoing().unwrap();
        assert_eq!((new.remote_tip, new.new_branch), (None, true));
        repository.push(&repository.outgoing().unwrap(), true, &mut |_| {}).unwrap();
        let pushed = git::run(&root, &["ls-remote", "--heads", "origin", "new"], None).unwrap().stdout;
        assert!(!pushed.is_empty(), "a new branch pushes with force too");
    }
}

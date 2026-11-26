pub mod avatars;
mod background;
pub mod branches;
pub mod changelists;
pub mod commit;
pub mod compare;
pub mod conflict;
pub mod drag;
pub mod graph_search;
pub mod history;
pub mod ignore;
pub mod index;
pub mod log;
pub mod log_view;
pub mod operations;
pub mod patch;
pub mod rebase;
pub mod remote;
pub mod shelf;
pub mod stash;
pub mod workspace;
pub mod workspace_columns;
pub mod workspace_pull;
pub mod workspace_view;
pub mod worktree;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::{Branch, Change, FileStatus, Head, Repository, Status};

use crate::Workbench;

pub use background::off_the_frame;
pub use branches::{BranchInfo, BranchList, TagInfo};
pub use operations::ClipboardRequest;
pub use remote::{OperationInfo, Outgoing};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(RepositoryInfoCommand)?;
    registry.register(StatusList)?;
    registry.register(StatusRefresh)?;
    branches::register(registry)?;
    changelists::register(registry)?;
    commit::register(registry)?;
    compare::register(registry)?;
    conflict::register(registry)?;
    drag::register(registry)?;
    graph_search::register(registry)?;
    avatars::register(registry)?;
    history::register(registry)?;
    ignore::register(registry)?;
    index::register(registry)?;
    log::register(registry)?;
    log_view::register(registry)?;
    operations::register(registry)?;
    patch::register(registry)?;
    rebase::register(registry)?;
    remote::register(registry)?;
    shelf::register(registry)?;
    stash::register(registry)?;
    workspace::register(registry)?;
    workspace_columns::register(registry)?;
    workspace_pull::register(registry)?;
    workspace_view::register(registry)?;
    worktree::register(registry)?;
    Ok(())
}

pub type Waker = Arc<dyn Fn() + Send + Sync>;

#[derive(Default)]
pub struct Vcs {
    found_for: Option<PathBuf>,
    git_on: Option<bool>,
    repository: Option<Arc<Repository>>,
    shared: Arc<Shared>,
    lines: commit::LineCache,
    blames: log::Blames,
    log_view: log_view::LogView,
    graph: workspace::Graphs,
    off_the_frame: bool,
    finishing: Option<background::Finishing>,
}

#[derive(Default)]
struct Shared {
    snapshot: Mutex<Snapshot>,
    operation: Mutex<Option<OperationInfo>>,
    waker: Mutex<Option<Waker>>,
    worktrees: Mutex<worktree::WorktreeCache>,
    submodules: Mutex<worktree::SubmoduleCache>,
}

impl Shared {
    fn snapshot(&self) -> MutexGuard<'_, Snapshot> {
        self.snapshot.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn operation(&self) -> MutexGuard<'_, Option<OperationInfo>> {
        self.operation.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn wake(&self) {
        let waker = self.waker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        if let Some(waker) = waker {
            waker();
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Snapshot {
    head: Head,
    status: Status,
    branches: Vec<Branch>,
    stamp: u64,
    refs_stamp: u64,
    revision: u64,
    computing: bool,
    again: bool,
    error: Option<String>,
}

impl Vcs {
    pub fn changed_favorites(&self) -> Option<Vec<String>> {
        self.log_view.changed_favorites()
    }

    pub fn log_view(&self) -> &log_view::LogView {
        &self.log_view
    }

    pub fn graph_focus(&self) -> Option<(PathBuf, graph_search::GraphFocus)> {
        let root = self.repository.as_ref()?.root().to_owned();
        Some((root, self.graph.slot().focus.clone()))
    }

    pub fn set_waker(&mut self, waker: Waker) {
        *self.shared.waker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(waker);
    }

    fn repository_for(&mut self, project: Option<&Path>) -> Option<Arc<Repository>> {
        let git_on = project.is_none_or(zen_vcs::git::allowed);
        if self.found_for.as_deref() != project || self.git_on != Some(git_on) {
            self.found_for = project.map(Path::to_owned);
            self.git_on = Some(git_on);
            self.repository = project.and_then(Repository::discover).map(Arc::new);
            let waker = self.shared.waker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
            self.shared = Arc::default();
            self.graph = workspace::Graphs::default();
            if let Some(waker) = waker {
                self.set_waker(waker);
            }
            if self.repository.is_some() {
                self.refresh();
            }
        }
        self.repository.clone()
    }

    fn refresh(&self) {
        let Some(repository) = self.repository.clone() else { return };
        {
            let mut snapshot = self.shared.snapshot();
            if snapshot.computing {
                snapshot.again = true;
                return;
            }
            snapshot.computing = true;
        }
        let shared = Arc::clone(&self.shared);
        let compute = move || {
            loop {
                let stamp = repository.stamp();
                let head = repository.head();
                let status = repository.status();
                let branches = repository.branches();
                let again = {
                    let mut snapshot = shared.snapshot();
                    match (head, status, branches) {
                        (Ok(head), Ok(status), Ok(branches)) => {
                            snapshot.head = head;
                            snapshot.status = status;
                            snapshot.branches = branches;
                            snapshot.error = None;
                        }
                        (head, status, branches) => {
                            let error = head.err().or(status.err()).or(branches.err());
                            snapshot.error = error.map(|error| error.to_string());
                        }
                    }
                    snapshot.stamp = stamp;
                    snapshot.refs_stamp = stamp;
                    snapshot.revision += 1;
                    let again = std::mem::take(&mut snapshot.again);
                    snapshot.computing = again;
                    again
                };
                shared.wake();
                if !again {
                    break;
                }
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(compute);
        #[cfg(target_arch = "wasm32")]
        compute();
    }

    fn settle(&self) {
        for _ in 0..2000 {
            if !self.shared.snapshot().computing {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}

pub(crate) fn message(error: &zen_vcs::VcsError) -> String {
    use zen_vcs::VcsError;
    match error {
        VcsError::Git(message) => message.clone(),
        VcsError::NotARepository => t!("vcs-error-no-repository"),
        VcsError::NotTrusted => t!("vcs-error-safe-mode"),
        VcsError::NoGit(reason) => t!("vcs-error-no-git", reason = reason.as_str()),
        VcsError::Unsupported => t!("vcs-error-unsupported"),
        VcsError::Detached => t!("vcs-error-detached"),
        VcsError::NoRemote => t!("vcs-error-no-remote"),
        VcsError::Conflicts { operation, files } => {
            const NAMED: usize = 5;
            let mut names = files.iter().take(NAMED).cloned().collect::<Vec<_>>().join(", ");
            if files.len() > NAMED {
                names.push_str(&t!("vcs-conflicts-more", count = files.len() - NAMED));
            }
            let key = match operation {
                zen_vcs::Applying::CherryPick => "vcs-error-cherry-pick-conflicts",
                zen_vcs::Applying::Revert => "vcs-error-revert-conflicts",
                zen_vcs::Applying::Merge => "vcs-error-merge-conflicts",
                zen_vcs::Applying::Rebase => "vcs-error-rebase-conflicts",
                zen_vcs::Applying::StashApply => "vcs-error-stash-apply-conflicts",
                zen_vcs::Applying::StashPop => "vcs-error-stash-pop-conflicts",
                zen_vcs::Applying::Unshelve => "vcs-error-unshelve-conflicts",
                zen_vcs::Applying::Patch => "vcs-error-patch-conflicts",
            };
            t!(key, count = files.len(), files = names.as_str())
        }
        VcsError::NotFastForward { branch, target } => {
            t!("vcs-error-not-fast-forward", branch = branch.as_str(), target = target.as_str())
        }
        VcsError::InvalidName { name, what } => {
            let key = match *what {
                "tag" => "vcs-error-invalid-tag",
                "remote" => "vcs-error-invalid-remote",
                "revision" => "vcs-error-invalid-revision",
                _ => "vcs-error-invalid-branch",
            };
            t!(key, name = name.as_str())
        }
    }
}

pub(crate) fn reported(session: &mut Session, error: &zen_vcs::VcsError) -> CommandError {
    let error = command_error(error);
    if error.code.as_deref() == Some(CONFLICTS) {
        session.state_mut::<workspace::GitWorkspace>().report_conflicts(error.message.clone());
    }
    error
}

pub const CONFLICTS: &str = "conflicts";

pub(crate) fn command_error(error: &zen_vcs::VcsError) -> CommandError {
    match error {
        zen_vcs::VcsError::Conflicts { .. } => CommandError::with_code(CONFLICTS, message(error)),
        _ => CommandError::new(message(error)),
    }
}

pub(crate) fn repository(session: &mut Session) -> Result<Arc<Repository>, CommandError> {
    let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    if project.as_deref().is_some_and(|project| !zen_vcs::git::allowed(project)) {
        return Err(crate::trust::git_off(session));
    }
    session
        .state_mut::<Vcs>()
        .repository_for(project.as_deref())
        .ok_or_else(|| CommandError::new(t!("vcs-error-no-repository")))
}

fn vcs(session: &mut Session) -> &mut Vcs {
    let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    let vcs = session.state_mut::<Vcs>();
    vcs.repository_for(project.as_deref());
    vcs
}

fn fresh_refs(vcs: &Vcs) -> MutexGuard<'_, Snapshot> {
    let mut snapshot = vcs.shared.snapshot();
    if let Some(repository) = &vcs.repository {
        let stamp = repository.stamp();
        if stamp != snapshot.refs_stamp {
            if let (Ok(head), Ok(branches)) = (repository.head(), repository.branches()) {
                snapshot.head = head;
                snapshot.branches = branches;
            }
            snapshot.refs_stamp = stamp;
        }
    }
    snapshot
}

pub(crate) fn changed(session: &mut Session) {
    vcs(session).refresh();
}

/// Where the repository is and which branch it is on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RepositoryInfo {
    /// The repository's root folder; `None` when the project is not in a git repository.
    pub root: Option<PathBuf>,
    /// The current branch; `None` when `HEAD` is detached or there is no repository.
    pub branch: Option<String>,
    /// The commit `HEAD` is at, as hex; `None` before the first commit.
    pub commit: Option<String>,
    /// What the branch widget shows: the branch, or the short commit id.
    pub label: String,
    /// The remote branch the current one tracks (`origin/main`).
    pub upstream: Option<String>,
    /// Commits to push.
    pub ahead: usize,
    /// Commits to pull.
    pub behind: usize,
    /// Whether the status is being computed.
    pub computing: bool,
    /// Raised whenever the status is computed again (see vcs.status.list).
    pub revision: u64,
    /// Why reading the repository failed, if it did.
    pub error: Option<String>,
}

pub struct RepositoryInfoCommand;

impl Command for RepositoryInfoCommand {
    type Params = NoParams;
    type Output = RepositoryInfo;
    const ID: &'static str = "vcs.repository.info";
    const TITLE: &'static str = "Repository";
    const DESCRIPTION: &'static str = "Tells whether the project is in a git repository, its root, the current branch (or the \
         commit of a detached HEAD), its upstream and how many commits are to push and to pull. In safe mode Git is \
         off: no root, and `error` says why and what to do.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<RepositoryInfo, CommandError> {
        let project = session.state::<Workbench>().and_then(Workbench::project);
        if project.is_some_and(|project| !zen_vcs::git::allowed(project)) {
            let error = Some(crate::trust::git_off(session).message);
            return Ok(RepositoryInfo { error, ..RepositoryInfo::default() });
        }
        let vcs = vcs(session);
        let Some(repository) = vcs.repository.clone() else { return Ok(RepositoryInfo::default()) };
        let snapshot = fresh_refs(vcs);
        let current = snapshot.branches.iter().find(|branch| branch.current && !branch.remote);
        Ok(RepositoryInfo {
            root: Some(repository.root().to_owned()),
            branch: snapshot.head.branch.clone(),
            commit: snapshot.head.commit.clone(),
            label: snapshot.head.label(),
            upstream: current.and_then(|branch| branch.upstream.clone()),
            ahead: current.map_or(0, |branch| branch.ahead),
            behind: current.map_or(0, |branch| branch.behind),
            computing: snapshot.computing,
            revision: snapshot.revision,
            error: snapshot.error.clone(),
        })
    }
}

/// One changed file, as commands report it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ChangeInfo {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the repository's root, with `/`.
    pub relative: String,
    /// `added`, `modified`, `deleted`, `renamed`, `conflicted` or `unversioned`.
    pub status: String,
    /// Where a renamed file was, relative.
    pub old_path: Option<String>,
    /// Whether some of the change is staged.
    pub staged: bool,
    /// The staged part (the index against HEAD): `added`, `modified`, `deleted` or `renamed`;
    /// `None` when nothing of it is staged.
    pub staged_status: Option<String>,
    /// The unstaged part (the working tree against the index): `modified`, `deleted`,
    /// `unversioned` or `conflicted`; `None` when all of it is staged. A file changed again after
    /// staging has both.
    pub unstaged_status: Option<String>,
}

impl ChangeInfo {
    fn new(root: &Path, change: &Change) -> Self {
        ChangeInfo {
            path: absolute(root, &change.path),
            relative: change.path.clone(),
            status: change.status.as_str().to_owned(),
            old_path: change.old_path.clone(),
            staged: change.staged,
            staged_status: change.index.map(|status| status.as_str().to_owned()),
            unstaged_status: change.worktree.map(|status| status.as_str().to_owned()),
        }
    }
}

pub(crate) fn absolute(root: &Path, relative: &str) -> PathBuf {
    relative.split('/').filter(|part| !part.is_empty()).fold(root.to_owned(), |path, part| path.join(part))
}

/// The status of the working tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusReport {
    /// Changed, added, deleted, conflicted and unversioned files, sorted by path.
    pub changes: Vec<ChangeInfo>,
    /// Ignored files and folders, as absolute paths (a wholly ignored folder is listed once).
    pub ignored: Vec<PathBuf>,
    /// Raised by every computation, so that views can cache what they derive from it.
    pub revision: u64,
    /// Whether it is being computed again.
    pub computing: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileStatuses {
    files: BTreeMap<PathBuf, FileStatus>,
    ignored: Vec<PathBuf>,
    changed_folders: std::collections::BTreeSet<PathBuf>,
}

impl FileStatuses {
    pub fn new(report: &StatusReport) -> Self {
        let mut statuses = FileStatuses { ignored: report.ignored.clone(), ..FileStatuses::default() };
        for change in &report.changes {
            let status = match change.status.as_str() {
                "added" => FileStatus::Added,
                "deleted" => FileStatus::Deleted,
                "renamed" => FileStatus::Renamed,
                "conflicted" => FileStatus::Conflicted,
                "unversioned" => FileStatus::Unversioned,
                _ => FileStatus::Modified,
            };
            statuses.files.insert(change.path.clone(), status);
            if status != FileStatus::Unversioned {
                statuses.changed_folders.extend(change.path.ancestors().skip(1).map(Path::to_owned));
            }
        }
        statuses
    }

    pub fn of(&self, path: &Path) -> Option<FileStatus> {
        if let Some(status) = self.files.get(path) {
            return Some(*status);
        }
        if self.ignored.iter().any(|ignored| path.starts_with(ignored)) {
            return Some(FileStatus::Ignored);
        }
        self.changed_folders.contains(path).then_some(FileStatus::Modified)
    }
}

pub struct StatusList;

impl Command for StatusList {
    type Params = NoParams;
    type Output = StatusReport;
    const ID: &'static str = "vcs.status.list";
    const TITLE: &'static str = "Local Changes";
    const DESCRIPTION: &'static str = "Lists the files that differ from the last commit (added, modified, deleted, renamed, \
         conflicted, unversioned) and the ignored ones, as last computed; vcs.status.refresh computes \
         it again. Empty when the project is not in a git repository.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<StatusReport, CommandError> {
        let unsaved: Vec<(PathBuf, String)> = session
            .state::<Workbench>()
            .map(|workbench| {
                workbench
                    .documents()
                    .iter()
                    .filter(|document| document.is_dirty() && document.image().is_none())
                    .map(|document| (document.path().to_owned(), document.text().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        let vcs = vcs(session);
        let Some(repository) = vcs.repository.clone() else { return Ok(StatusReport::default()) };
        let root = repository.root();
        let snapshot = vcs.shared.snapshot();
        let mut changes: Vec<ChangeInfo> =
            snapshot.status.changes.iter().map(|change| ChangeInfo::new(root, change)).collect();
        for (path, text) in unsaved {
            let Ok(relative) = path.strip_prefix(root) else { continue };
            let relative =
                relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            let Ok(Some(head)) = repository.file_at_head(&relative) else { continue };
            let listed = changes.iter().position(|change| change.path == path);
            if head == text.as_bytes() {
                if let Some(index) =
                    listed.filter(|index| changes[*index].status == "modified" && !changes[*index].staged)
                {
                    changes.remove(index);
                }
            } else if let Some(index) = listed {
                let change = &mut changes[index];
                if change.unstaged_status.is_none()
                    && repository
                        .file_in_index(&relative)
                        .ok()
                        .flatten()
                        .is_some_and(|staged| staged != text.as_bytes())
                {
                    change.unstaged_status = Some("modified".to_owned());
                }
            } else {
                changes.push(ChangeInfo {
                    path,
                    relative,
                    status: "modified".to_owned(),
                    old_path: None,
                    staged: false,
                    staged_status: None,
                    unstaged_status: Some("modified".to_owned()),
                });
            }
        }
        changes.sort_by(|a, b| a.relative.cmp(&b.relative));
        Ok(StatusReport {
            changes,
            ignored: snapshot.status.ignored.iter().map(|path| absolute(root, path)).collect(),
            revision: snapshot.revision,
            computing: snapshot.computing,
        })
    }
}

/// When to compute the status again.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RefreshParams {
    /// Only if the repository changed (`HEAD`, the index or a branch moved): what the window asks
    /// every few seconds, cheaply. Otherwise always (after files changed).
    pub if_changed: bool,
    /// Wait until it is computed.
    pub wait: bool,
}

pub struct StatusRefresh;

impl Command for StatusRefresh {
    type Params = RefreshParams;
    type Output = ();
    const ID: &'static str = "vcs.status.refresh";
    const TITLE: &'static str = "Refresh";
    const DESCRIPTION: &'static str = "Computes the status of the working tree and the branches again, in the background unless \
         `wait` is set; with `ifChanged`, only when HEAD, the index or a branch changed.";

    fn run(&self, session: &mut Session, params: RefreshParams) -> Result<(), CommandError> {
        let vcs = vcs(session);
        let Some(repository) = vcs.repository.clone() else { return Ok(()) };
        let stale = !params.if_changed || {
            let snapshot = vcs.shared.snapshot();
            !snapshot.computing && snapshot.stamp != repository.stamp()
        };
        if stale {
            vcs.refresh();
        }
        if params.wait {
            vcs.settle();
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod testing {

    use std::path::Path;
    use std::process::Command;

    pub(crate) fn isolate() {
        zen_i18n::register(crate::LOCALES);
        zen_vcs::git::set_environment(&[
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_AUTHOR_NAME", "Ada Lovelace"),
            ("GIT_AUTHOR_EMAIL", "ada@example.com"),
            ("GIT_COMMITTER_NAME", "Ada Lovelace"),
            ("GIT_COMMITTER_EMAIL", "ada@example.com"),
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_CONFIG_KEY_0", "protocol.file.allow"),
            ("GIT_CONFIG_VALUE_0", "always"),
        ]);
    }

    pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "Ada Lovelace")
            .env("GIT_AUTHOR_EMAIL", "ada@example.com")
            .env("GIT_COMMITTER_NAME", "Ada Lovelace")
            .env("GIT_COMMITTER_EMAIL", "ada@example.com")
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "protocol.file.allow")
            .env("GIT_CONFIG_VALUE_0", "always")
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    pub(crate) fn fast_import(stream: &str) -> tempfile::TempDir {
        isolate();
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        let mut child = Command::new("git")
            .current_dir(dir.path())
            .args(["fast-import", "--quiet"])
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(stream.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        git(dir.path(), &["reset", "-q", "--hard"]);
        dir
    }

    pub(crate) fn imported_commit(
        branch: &str,
        mark: usize,
        (name, user): (&str, &str),
        time: i64,
        message: &str,
        parents: &[usize],
        file: &str,
    ) -> String {
        let mut commit = format!(
            "commit {branch}\nmark :{mark}\nauthor {name} <{user}@example.com> {time} +0000\ncommitter {name} <{user}@example.com> {time} +0000\ndata {}\n{message}\n",
            message.len()
        );
        for (at, parent) in parents.iter().enumerate() {
            commit.push_str(&format!("{} :{parent}\n", if at == 0 { "from" } else { "merge" }));
        }
        let content = format!("{mark}\n");
        commit.push_str(&format!("M 644 inline {file}\ndata {}\n{content}\n", content.len()));
        commit
    }

    pub(crate) fn large_history(count: usize) -> tempfile::TempDir {
        const PEOPLE: [(&str, &str); 8] = [
            ("Ada Lovelace", "ada"),
            ("Grace Hopper", "grace"),
            ("Alan Turing", "alan"),
            ("Edsger Dijkstra", "edsger"),
            ("Barbara Liskov", "barbara"),
            ("Donald Knuth", "donald"),
            ("Margaret Hamilton", "margaret"),
            ("Ken Thompson", "ken"),
        ];
        const AREAS: [&str; 6] = ["editor", "vcs", "lsp", "ui", "build", "docs"];
        let mut stream = String::with_capacity(count * 400);
        let mut mark = 0;
        let mut main: Option<usize> = None;
        let mut time = 1_600_000_000i64;
        let mut next = |branch: &str, parents: &[usize], stream: &mut String| -> usize {
            mark += 1;
            time += 60;
            let area = AREAS[mark % AREAS.len()];
            let mut message = format!("{area}: change number {mark} of the {area} module");
            if mark % 3 == 0 {
                message.push_str(&format!(
                    "\n\nThe {area} module changes so that the case {mark} is handled as well.\nIt was \
                     reported on the tracker and is covered by a test now."
                ));
            }
            if mark % 17 == 0 {
                message.push_str(
                    "\n\nA longer explanation of what was wrong, how it was found and why it is fixed this way. ",
                );
                message.push_str(&"More detail on the change and on what it does not change. ".repeat(4));
            }
            let file = format!("src/{area}/file{}.txt", mark % 50);
            stream.push_str(&imported_commit(
                branch,
                mark,
                PEOPLE[mark % PEOPLE.len()],
                time,
                &message,
                parents,
                &file,
            ));
            mark
        };
        let mut made = 0;
        let mut branches = 0;
        let mut tags = 0;
        while made < count {
            let tip = next("refs/heads/main", &main.into_iter().collect::<Vec<_>>(), &mut stream);
            main = Some(tip);
            made += 1;
            if made % 40 == 0 && made + 8 < count {
                branches += 1;
                let name = format!("refs/heads/feature/{}", branches % 12);
                let mut side = tip;
                for _ in 0..(branches % 4 + 1) {
                    side = next(&name, &[side], &mut stream);
                    made += 1;
                }
                let tip = next("refs/heads/main", &[tip], &mut stream);
                made += 1;
                let merged = next("refs/heads/main", &[tip, side], &mut stream);
                main = Some(merged);
                made += 1;
            }
            if made / 1000 > tags {
                tags = made / 1000;
                stream.push_str(&format!("reset refs/tags/v{tags}\nfrom :{}\n\n", main.unwrap()));
            }
        }
        stream.push_str(&format!("reset refs/remotes/origin/main\nfrom :{}\n\n", main.unwrap()));
        fast_import(&stream)
    }

    pub(crate) fn test_repository() -> tempfile::TempDir {
        isolate();
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(dir.path().join("a.txt"), "one\ntwo\nthree\n").unwrap();
        git(dir.path(), &["add", "a.txt"]);
        git(dir.path(), &["commit", "-q", "-m", "First"]);
        dir
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::testing::*;
    use super::*;

    pub(crate) fn session(dir: &Path) -> (Registry, Session) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir })).unwrap();
        (registry, session)
    }

    #[test]
    fn status_lists_changes_and_ignored_files_and_follows_refreshes() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join(".gitignore"), "out/\n").unwrap();
        git(&root, &["add", ".gitignore"]);
        git(&root, &["commit", "-q", "-m", "Ignore"]);
        std::fs::write(root.join("a.txt"), "one\n2\nthree\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        std::fs::write(root.join("staged.txt"), "staged\n").unwrap();
        git(&root, &["add", "staged.txt"]);
        std::fs::create_dir(root.join("out")).unwrap();
        std::fs::write(root.join("out/build.bin"), "x").unwrap();
        let (registry, mut session) = session(&root);
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let status = registry.dispatch(&mut session, "vcs.status.list", json!({})).unwrap();
        let changes: Vec<(String, String)> = status["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|change| {
                (change["relative"].as_str().unwrap().to_owned(), change["status"].as_str().unwrap().to_owned())
            })
            .collect();
        assert_eq!(
            changes,
            [
                ("a.txt".to_owned(), "modified".to_owned()),
                ("new.txt".to_owned(), "unversioned".to_owned()),
                ("staged.txt".to_owned(), "added".to_owned()),
            ]
        );
        assert_eq!(status["ignored"], json!([root.join("out")]));

        let report: StatusReport = StatusReport {
            changes: vec![ChangeInfo {
                path: root.join("src/a.go"),
                relative: "src/a.go".into(),
                status: "modified".into(),
                old_path: None,
                staged: false,
                staged_status: None,
                unstaged_status: Some("modified".into()),
            }],
            ignored: vec![root.join("out")],
            revision: 1,
            computing: false,
        };
        let statuses = FileStatuses::new(&report);
        assert_eq!(statuses.of(&root.join("src/a.go")), Some(FileStatus::Modified));
        assert_eq!(statuses.of(&root.join("src")), Some(FileStatus::Modified), "a folder with changes");
        assert_eq!(statuses.of(&root.join("out/x/y.bin")), Some(FileStatus::Ignored));
        assert_eq!(statuses.of(&root.join("b.go")), None);

        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Everything"]);
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "ifChanged": true, "wait": true })).unwrap();
        let status = registry.dispatch(&mut session, "vcs.status.list", json!({})).unwrap();
        assert_eq!(status["changes"], json!([]));
    }

    #[test]
    fn unsaved_changes_count_as_the_editor_shows_them() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let file = root.join("a.txt");
        let (registry, mut session) = session(&root);
        let statuses = |registry: &Registry, session: &mut Session| -> Vec<String> {
            registry.dispatch(session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
            let status = registry.dispatch(session, "vcs.status.list", json!({})).unwrap();
            status["changes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|change| change["status"].as_str().unwrap().to_owned())
                .collect()
        };
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 0 }] })).unwrap();
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "zero\n" })).unwrap();
        assert_eq!(statuses(&registry, &mut session), ["modified"], "an unsaved change shows at once");
        registry.dispatch(&mut session, "file.document.save", json!({})).unwrap();
        assert_eq!(statuses(&registry, &mut session), ["modified"]);
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        assert!(statuses(&registry, &mut session).is_empty(), "the editor's text is the committed one");
    }

    #[test]
    fn commands_answer_with_a_branch_checked_out_since_the_last_refresh() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = session(&root);
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        git(&root, &["switch", "-q", "-c", "moved"]);
        let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
        assert_eq!(info["branch"], "moved", "{info}");
    }

    #[test]
    fn a_project_outside_a_repository_has_no_status() {
        isolate();
        let dir = tempfile::tempdir().unwrap();
        let (registry, mut session) = session(dir.path());
        let info = registry.dispatch(&mut session, "vcs.repository.info", json!({})).unwrap();
        assert_eq!(info["root"], json!(null));
        assert_eq!(registry.dispatch(&mut session, "vcs.status.list", json!({})).unwrap()["changes"], json!([]));
        assert!(registry.dispatch(&mut session, "vcs.branch.list", json!({})).is_err());
    }
}

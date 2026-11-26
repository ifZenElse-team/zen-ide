use std::fmt;

pub mod diff;
pub mod filter;
pub mod git;
pub mod graph;
pub mod history;
pub mod merge;
pub mod patch;
#[cfg(not(target_arch = "wasm32"))]
mod repo;
#[cfg(target_arch = "wasm32")]
#[path = "unsupported.rs"]
mod repo;

pub use repo::{Repository, local_offset};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PullMode {
    #[default]
    Merge,
    FastForwardOnly,
    Rebase,
}

pub fn pull_args(mode: PullMode) -> Vec<&'static str> {
    let modes: &[&str] = match mode {
        PullMode::Merge => &["--no-rebase", "--ff"],
        PullMode::FastForwardOnly => &["--no-rebase", "--ff-only"],
        PullMode::Rebase => &["--rebase"],
    };
    let mut args = vec!["pull"];
    args.extend_from_slice(modes);
    args.extend(["--no-edit", "--progress"]);
    args
}

pub fn parse_stash_reflog(reflog: &str) -> Vec<Stash> {
    let mut stashes: Vec<Stash> = reflog.lines().filter_map(parse_stash_line).collect();
    stashes.reverse();
    for (index, stash) in stashes.iter_mut().enumerate() {
        stash.index = index;
    }
    stashes
}

fn parse_stash_line(line: &str) -> Option<Stash> {
    let (head, message) = line.split_once('\t').unwrap_or((line, ""));
    let mut fields = head.split(' ');
    let _old = fields.next()?;
    let commit = fields.next()?.to_owned();
    let after_email = head.rsplit_once('>').map(|(_, rest)| rest.trim())?;
    let (time, zone) = after_email.split_once(' ')?;
    let zone_is_valid =
        zone.len() == 5 && zone.starts_with(['+', '-']) && zone[1..].bytes().all(|byte| byte.is_ascii_digit());
    if !zone_is_valid {
        return None;
    }
    let time = time.parse().ok()?;
    Some(Stash { index: 0, commit, base: String::new(), message: message.trim().to_owned(), time })
}

pub fn renamed_in_stash_reflog(reflog: &[u8], index: usize, message: &str) -> Option<Vec<u8>> {
    let mut lines: Vec<Vec<u8>> = reflog.split(|byte| *byte == b'\n').map(<[u8]>::to_vec).collect();
    let line =
        lines.iter_mut().filter(|line| parse_stash_line(&String::from_utf8_lossy(line)).is_some()).rev().nth(index)?;
    let head = line.iter().position(|byte| *byte == b'\t').unwrap_or(line.len());
    line.truncate(head);
    line.push(b'\t');
    line.extend_from_slice(message.as_bytes());
    Some(lines.join(&b'\n'))
}

fn stash_parts(message: &str) -> Option<(&str, &str)> {
    message.strip_prefix("WIP on ").or_else(|| message.strip_prefix("On ")).and_then(|rest| rest.split_once(": "))
}

pub fn stash_text(message: &str) -> &str {
    stash_parts(message).map_or(message, |(_, text)| text)
}

pub fn renamed_stash_message(old: &str, text: &str) -> String {
    let text = text.trim();
    match stash_parts(old) {
        Some((branch, _)) => format!("On {branch}: {text}"),
        None => text.to_owned(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VcsError {
    NotARepository,
    NoGit(String),
    Git(String),
    Unsupported,
    Detached,
    NoRemote,
    Conflicts { operation: Applying, files: Vec<String> },
    NotFastForward { branch: String, target: String },
    InvalidName { name: String, what: &'static str },
    NotTrusted,
}

impl fmt::Display for VcsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VcsError::NotARepository => f.write_str("not a git repository"),
            VcsError::NoGit(error) => write!(f, "cannot run git: {error}"),
            VcsError::Git(message) => f.write_str(message),
            VcsError::Unsupported => f.write_str("version control is not available here"),
            VcsError::Detached => f.write_str("HEAD is detached: check out a branch first"),
            VcsError::NoRemote => f.write_str("the repository has no remote"),
            VcsError::Conflicts { operation, files } => {
                write!(f, "{} stopped with conflicts in {}", operation.as_str(), files.join(", "))
            }
            VcsError::NotFastForward { branch, target } => {
                write!(f, "{branch} cannot be fast-forwarded to {target}: it has commits {target} does not have")
            }
            VcsError::InvalidName { name, what } => write!(f, "'{name}' is not a valid {what} name"),
            VcsError::NotTrusted => f.write_str("Git is off in a project that is not trusted"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Applying {
    CherryPick,
    Revert,
    Merge,
    Rebase,
    StashApply,
    StashPop,
    Unshelve,
    Patch,
}

impl Applying {
    pub fn as_str(self) -> &'static str {
        match self {
            Applying::CherryPick => "cherry-pick",
            Applying::Revert => "revert",
            Applying::Merge => "merge",
            Applying::Rebase => "rebase",
            Applying::StashApply => "stash apply",
            Applying::StashPop => "stash pop",
            Applying::Unshelve => "unshelve",
            Applying::Patch => "apply patch",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RebaseAction {
    #[default]
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl RebaseAction {
    pub fn as_str(self) -> &'static str {
        match self {
            RebaseAction::Pick => "pick",
            RebaseAction::Reword => "reword",
            RebaseAction::Squash => "squash",
            RebaseAction::Fixup => "fixup",
            RebaseAction::Drop => "drop",
        }
    }

    pub fn melds(self) -> bool {
        matches!(self, RebaseAction::Squash | RebaseAction::Fixup)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RebaseStep {
    pub action: RebaseAction,
    pub commit: String,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RebaseScript {
    pub upstream: String,
    pub onto: String,
    pub branch: Option<String>,
    pub steps: Vec<RebaseStep>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RebaseProgress {
    pub branch: Option<String>,
    pub onto: Option<String>,
    pub step: usize,
    pub total: usize,
    pub stopped_at: Option<String>,
    pub interactive: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    #[default]
    Mixed,
    Hard,
}

impl ResetMode {
    pub fn option(self) -> &'static str {
        match self {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefState {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub branches: std::collections::BTreeMap<String, String>,
    pub upstreams: std::collections::BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Move {
    Soft,
    Mixed,
    #[default]
    Keep,
}

impl Move {
    pub fn option(self) -> &'static str {
        match self {
            Move::Soft => "--soft",
            Move::Mixed => "--mixed",
            Move::Keep => "--keep",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blob {
    pub mode: u32,
    pub id: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conflicted {
    pub path: String,
    pub base: Option<Blob>,
    pub ours: Option<Blob>,
    pub theirs: Option<Blob>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InProgress {
    pub operation: Applying,
    pub theirs: Option<String>,
    pub theirs_name: Option<String>,
}

pub const LINK_MODE: u32 = 0o120000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileState {
    pub path: String,
    pub index: Option<Blob>,
    pub worktree: Option<Blob>,
}

impl FileState {
    pub fn same(&self, other: &FileState) -> bool {
        let worktree = match (&self.worktree, &other.worktree) {
            (Some(a), Some(b)) => {
                a.id == b.id && (a.mode == LINK_MODE) == (b.mode == LINK_MODE) && (!cfg!(unix) || a.mode == b.mode)
            }
            (None, None) => true,
            _ => false,
        };
        self.path == other.path && self.index == other.index && worktree
    }
}

impl std::error::Error for VcsError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Conflicted,
    Unversioned,
    Ignored,
}

impl FileStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            FileStatus::Added => "added",
            FileStatus::Modified => "modified",
            FileStatus::Deleted => "deleted",
            FileStatus::Renamed => "renamed",
            FileStatus::Conflicted => "conflicted",
            FileStatus::Unversioned => "unversioned",
            FileStatus::Ignored => "ignored",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub path: String,
    pub status: FileStatus,
    pub old_path: Option<String>,
    pub staged: bool,
    pub index: Option<FileStatus>,
    pub worktree: Option<FileStatus>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub changes: Vec<Change>,
    pub ignored: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Head {
    pub branch: Option<String>,
    pub commit: Option<String>,
}

impl Head {
    pub fn label(&self) -> String {
        match (&self.branch, &self.commit) {
            (Some(branch), _) => branch.clone(),
            (None, Some(commit)) => commit.chars().take(8).collect(),
            (None, None) => String::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub remote: bool,
    pub current: bool,
    pub commit: String,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub commit: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub offset: i32,
    pub subject: String,
    pub body: String,
}

impl Commit {
    pub fn message(&self) -> String {
        if self.body.is_empty() { self.subject.clone() } else { format!("{}\n\n{}", self.subject, self.body) }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LogQuery {
    pub branch: Option<String>,
    pub author: Option<String>,
    pub text: Option<String>,
    pub path: Option<String>,
    pub branches: Vec<String>,
    pub exclude: Vec<String>,
    pub authors: Vec<String>,
    pub regex: bool,
    pub match_case: bool,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub paths: Vec<String>,
    pub topological: bool,
    pub skip: usize,
    pub limit: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    pub commit: Commit,
    pub refs: Vec<String>,
    pub labels: Vec<RefLabel>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RefKind {
    Head,
    Local,
    Remote,
    Tag,
}

impl RefKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RefKind::Head => "head",
            RefKind::Local => "local",
            RefKind::Remote => "remote",
            RefKind::Tag => "tag",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub status: FileStatus,
    pub old_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRevision {
    pub commit: Commit,
    pub path: String,
    pub status: FileStatus,
    pub old_path: Option<String>,
}

pub type Sides = (Option<Vec<u8>>, Option<Vec<u8>>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameLine {
    pub commit: String,
    pub author: String,
    pub time: i64,
    pub offset: i32,
    pub subject: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stash {
    pub index: usize,
    pub commit: String,
    pub base: String,
    pub message: String,
    pub time: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub branch: String,
    pub remote: String,
    pub target: String,
    pub new_branch: bool,
    pub remote_tip: Option<String>,
    pub commits: Vec<Commit>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Worktree {
    pub path: std::path::PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub main: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
}

pub fn parse_worktree_list(output: &str) -> Vec<Worktree> {
    let mut records: Vec<(Worktree, bool)> = Vec::new();
    for field in output.split('\0').filter(|field| !field.is_empty()) {
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        if key == "worktree" {
            let main = records.is_empty();
            records.push((Worktree { path: value.into(), main, ..Worktree::default() }, false));
            continue;
        }
        let Some((worktree, bare)) = records.last_mut() else { continue };
        match key {
            "HEAD" => worktree.head = Some(value.to_owned()).filter(|id| id.bytes().any(|byte| byte != b'0')),
            "branch" => worktree.branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_owned()),
            "bare" => *bare = true,
            "locked" => worktree.locked = Some(value.to_owned()),
            "prunable" => worktree.prunable = Some(value.to_owned()),
            _ => {}
        }
    }
    records.into_iter().filter(|(_, bare)| !bare).map(|(worktree, _)| worktree).collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SubmoduleState {
    #[default]
    Uninitialized,
    Current,
    Moved,
    Conflicted,
}

impl SubmoduleState {
    pub fn as_str(self) -> &'static str {
        match self {
            SubmoduleState::Uninitialized => "uninitialized",
            SubmoduleState::Current => "current",
            SubmoduleState::Moved => "moved",
            SubmoduleState::Conflicted => "conflicted",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Submodule {
    pub name: String,
    pub path: String,
    pub url: Option<String>,
    pub recorded: Option<String>,
    pub checked_out: Option<String>,
    pub state: SubmoduleState,
}

pub fn parse_submodule_status(output: &str) -> Vec<Submodule> {
    output
        .lines()
        .filter(|line| line.len() > 1)
        .filter_map(|line| {
            let (mark, rest) = line.split_at(1);
            let (id, rest) = rest.split_once(' ')?;
            let path = match rest.rfind(" (") {
                Some(at) if rest.ends_with(')') => &rest[..at],
                _ => rest,
            };
            let id = Some(id.to_owned()).filter(|id| id.bytes().any(|byte| byte != b'0'));
            let (state, recorded, checked_out) = match mark {
                "-" => (SubmoduleState::Uninitialized, id, None),
                "+" => (SubmoduleState::Moved, None, id),
                "U" => (SubmoduleState::Conflicted, None, None),
                _ => (SubmoduleState::Current, id.clone(), id),
            };
            Some(Submodule { name: path.to_owned(), path: path.to_owned(), url: None, recorded, checked_out, state })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stash_reflog_lists_the_newest_first() {
        let reflog = "0000 aaaa Ada <ada@example.com> 1700000000 +0100\tOn main: first\n\
                      aaaa bbbb Ada <ada@example.com> 1700000100 +0100\tWIP on main: 1234567 Subject\n\
                      a line without a tab\n";
        let stashes = parse_stash_reflog(reflog);
        assert_eq!(stashes.len(), 2);
        assert_eq!((stashes[0].index, stashes[0].commit.as_str()), (0, "bbbb"));
        assert_eq!(stashes[0].message, "WIP on main: 1234567 Subject");
        assert_eq!(stashes[0].time, 1_700_000_100);
        assert_eq!((stashes[1].index, stashes[1].message.as_str()), (1, "On main: first"));
        assert!(parse_stash_reflog("").is_empty());
    }

    #[test]
    fn a_stash_with_an_empty_message_keeps_the_indexes_of_the_others() {
        let reflog = "0000 aaaa Ada <ada@example.com> 1700000000 +0100\tOn main: first\n\
                      aaaa bbbb Ada <ada@example.com> 1700000100 +0100\n\
                      bbbb cccc Ada <ada@example.com> 1700000200 +0100\tOn main: third\n";
        let stashes = parse_stash_reflog(reflog);
        let listed: Vec<(usize, &str)> = stashes.iter().map(|stash| (stash.index, stash.commit.as_str())).collect();
        assert_eq!(listed, [(0, "cccc"), (1, "bbbb"), (2, "aaaa")]);
        assert_eq!(stashes[1].message, "");
    }

    #[test]
    fn a_line_git_itself_skips_takes_no_place_in_the_stash_list() {
        let reflog = "0000 aaaa Ada <ada@example.com> 1700000000 +0100\tOn main: first\n\
                      garbage\n\
                      aaaa bbbb Ada <ada@example.com> 1700000100 x0100\tno time zone\n\
                      aaaa bbbb no email\tno email\n\
                      bbbb cccc Ada <ada@example.com> 1700000200 +0100\tOn main: third\n";
        let commits: Vec<String> = parse_stash_reflog(reflog).into_iter().map(|stash| stash.commit).collect();
        assert_eq!(commits, ["cccc", "aaaa"]);
    }

    #[test]
    fn renaming_in_the_reflog_changes_one_line_and_counts_like_the_list() {
        let reflog = b"0000 aaaa Ada <a@e> 1700000000 +0100\tOn main: caf\xe9\n\
                       garbage\n\
                       aaaa bbbb Ada <a@e> 1700000100 +0100\n\
                       bbbb cccc Ada <a@e> 1700000200 +0100\tOn main: third\n";
        let renamed = renamed_in_stash_reflog(reflog, 1, "On main: named").unwrap();
        let expected = b"0000 aaaa Ada <a@e> 1700000000 +0100\tOn main: caf\xe9\n\
                         garbage\n\
                         aaaa bbbb Ada <a@e> 1700000100 +0100\tOn main: named\n\
                         bbbb cccc Ada <a@e> 1700000200 +0100\tOn main: third\n";
        assert_eq!(renamed, expected);
        assert_eq!(renamed_in_stash_reflog(reflog, 3, "x"), None);
    }

    #[test]
    fn a_renamed_stash_keeps_the_branch_it_was_made_on() {
        assert_eq!(stash_text("On main: Halfway"), "Halfway");
        assert_eq!(stash_text("WIP on feature/x: 1234567 Subject"), "1234567 Subject");
        assert_eq!(stash_text("my own message"), "my own message");
        assert_eq!(renamed_stash_message("On main: Halfway", " Almost done "), "On main: Almost done");
        assert_eq!(renamed_stash_message("WIP on dev: 1234567 Subject", "Later"), "On dev: Later");
        assert_eq!(renamed_stash_message("stored by hand", "Later"), "Later");
    }

    #[test]
    fn a_pull_spells_out_its_mode_so_that_the_users_settings_change_nothing() {
        let spelled = |mode, wanted: &[&str]| {
            let args = pull_args(mode);
            assert!(wanted.iter().all(|flag| args.contains(flag)), "{args:?}");
        };
        spelled(PullMode::Merge, &["--no-rebase", "--ff"]);
        spelled(PullMode::FastForwardOnly, &["--no-rebase", "--ff-only"]);
        spelled(PullMode::Rebase, &["--rebase"]);
    }
}

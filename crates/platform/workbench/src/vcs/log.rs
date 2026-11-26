use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::diff;

use super::commit::{DiffView, diff_view};
use super::{absolute, message, repository, vcs};
use crate::Workbench;

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListLog)?;
    registry.register(CommitDetails)?;
    registry.register(CommitDiff)?;
    registry.register(Annotate)?;
    registry.register(GetBlame)?;
    Ok(())
}

/// Which commits to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LogParams {
    /// Only this branch's history (or any revision: `HEAD~3`, an id); every branch when absent.
    pub branch: Option<String>,
    /// Only commits by this author (part of the name or e-mail, any case).
    pub author: Option<String>,
    /// Only commits whose message contains this (any case) or whose id starts with it.
    pub text: Option<String>,
    /// Only commits that changed this file or folder (absolute or relative to the repository).
    pub path: Option<PathBuf>,
    /// Only the history of these branches or revisions (together with `branch`).
    pub branches: Vec<String>,
    /// Leave out the history of these revisions (`HEAD`: what the branches add to the current one).
    pub exclude: Vec<String>,
    /// Only commits by one of these authors (together with `author`).
    pub users: Vec<String>,
    /// `text` is a regular expression.
    pub regex: bool,
    /// `text` matches in the same case only.
    pub match_case: bool,
    /// Only commits authored at or after this time, in seconds since 1970.
    pub since: Option<i64>,
    /// Only commits authored at or before this time, in seconds since 1970.
    pub until: Option<i64>,
    /// Only commits that changed one of these files or folders (together with `path`).
    pub paths: Vec<PathBuf>,
    /// `date` (newest first, the default) or `topological` (each branch's commits together).
    pub sort: LogSort,
    /// List what this tab of the Log shows (see vcs.log.view): its filters replace the ones above.
    pub tab: Option<usize>,
    /// Commits to leave out first, for paging.
    pub skip: usize,
    /// The most to list; 300 when absent.
    pub limit: Option<usize>,
}

/// The order of the log.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LogSort {
    /// Newest first.
    #[default]
    Date,
    /// Each branch's commits together, children before parents.
    Topological,
}

/// A branch, a tag or `HEAD` at a commit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RefLabelInfo {
    /// `HEAD`, `main`, `origin/main`, `v1.0`.
    pub name: String,
    /// `head`, `local`, `remote` or `tag`.
    pub kind: String,
}

impl From<&zen_vcs::RefLabel> for RefLabelInfo {
    fn from(label: &zen_vcs::RefLabel) -> Self {
        RefLabelInfo { name: label.name.clone(), kind: label.kind.as_str().to_owned() }
    }
}

/// A line of the log's graph in one row: from a lane to a lane, in a colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphEdge {
    /// The lane it starts in (at the row's top, or its middle).
    pub from: usize,
    /// The lane it ends in (at the row's middle, or its bottom).
    pub to: usize,
    /// The colour index.
    pub color: usize,
}

/// A commit of the log.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LogCommit {
    /// The full id, as hex.
    pub id: String,
    /// The first eight characters of the id.
    pub short_id: String,
    /// The parents' ids.
    pub parents: Vec<String>,
    /// The author's name.
    pub author: String,
    /// The author's e-mail.
    pub email: String,
    /// When it was authored, in seconds since 1970.
    pub time: i64,
    /// The author's time zone, in seconds east of UTC: dates show in it.
    pub offset: i32,
    /// The message's first line.
    pub subject: String,
    /// The rest of the message.
    pub body: String,
    /// `HEAD`, branches and tags at this commit.
    pub refs: Vec<String>,
    /// The same, with what each is: `HEAD`, a local or remote branch, a tag.
    pub labels: Vec<RefLabelInfo>,
    /// The graph: the dot's lane and colour, and the lines of the row's upper and lower halves.
    pub lane: usize,
    /// The dot's colour index.
    pub color: usize,
    /// Lines in the upper half.
    pub up: Vec<GraphEdge>,
    /// Lines in the lower half.
    pub down: Vec<GraphEdge>,
}

/// The log.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LogList {
    /// The commits, newest first.
    pub commits: Vec<LogCommit>,
    /// Whether there may be more after them (ask with `skip`).
    pub more: bool,
    /// Why the filters could not be applied (a regular expression that is not valid); nothing is
    /// listed then.
    pub error: Option<String>,
    /// The local time zone's offset from UTC, in seconds east: dates show in it, as relative ones
    /// (Today, Yesterday) when recent.
    pub local_offset: i32,
}

pub struct ListLog;

impl Command for ListLog {
    type Params = LogParams;
    type Output = LogList;
    const ID: &'static str = "vcs.log.list";
    const TITLE: &'static str = "Log";
    const DESCRIPTION: &'static str = "Lists commits newest first (or topologically), from every branch or some, filtered by \
         message text (plain, regex, match case) or id, authors, dates and changed paths, with the branches and tags \
         at each (`labels`) and the graph's lanes; `tab` lists what a tab of the Log shows (vcs.log.view).";

    fn run(&self, session: &mut Session, mut params: LogParams) -> Result<LogList, CommandError> {
        let repository = repository(session)?;
        if let Some(tab) = params.tab {
            let view = &vcs(session).log_view;
            let tab = view.tabs().get(tab).ok_or_else(|| CommandError::new(t!("vcs-error-no-log-tab")))?;
            tab.filters.clone().apply_to(&mut params);
        }
        let relative = |path: &PathBuf| super::commit::relative(&repository, path);
        let path = params.path.as_ref().map(relative).transpose()?;
        let paths = params.paths.iter().map(relative).collect::<Result<Vec<_>, _>>()?;
        let limit = params.limit.unwrap_or(300).max(1);
        let query = zen_vcs::LogQuery {
            branch: params.branch.filter(|branch| !branch.is_empty()),
            author: params.author,
            text: params.text,
            path,
            branches: params.branches,
            exclude: params.exclude,
            authors: params.users,
            regex: params.regex,
            match_case: params.match_case,
            since: params.since,
            until: params.until,
            paths,
            topological: params.sort == LogSort::Topological,
            skip: params.skip,
            limit: limit + 1,
        };
        let local_offset = zen_vcs::local_offset();
        let mut entries = match repository.log(&query) {
            Ok(entries) => entries,
            Err(zen_vcs::VcsError::Git(error))
                if query.regex && zen_vcs::filter::CommitFilter::new(&query).is_err() =>
            {
                return Ok(LogList { error: Some(error), local_offset, ..LogList::default() });
            }
            Err(error) => return Err(CommandError::new(message(&error))),
        };
        let more = entries.len() > limit;
        entries.truncate(limit);
        let shape: Vec<(String, Vec<String>)> =
            entries.iter().map(|entry| (entry.commit.id.clone(), entry.commit.parents.clone())).collect();
        let rows = zen_vcs::graph::layout(&shape);
        let edges = |edges: &[zen_vcs::graph::Edge]| {
            edges.iter().map(|edge| GraphEdge { from: edge.from, to: edge.to, color: edge.color }).collect()
        };
        let commits = entries
            .into_iter()
            .zip(rows)
            .map(|(entry, row)| LogCommit {
                short_id: entry.commit.id.chars().take(8).collect(),
                id: entry.commit.id,
                parents: entry.commit.parents,
                author: entry.commit.author,
                email: entry.commit.email,
                time: entry.commit.time,
                offset: entry.commit.offset,
                subject: entry.commit.subject,
                body: entry.commit.body,
                refs: entry.refs,
                labels: entry.labels.iter().map(RefLabelInfo::from).collect(),
                lane: row.lane,
                color: row.color,
                up: edges(&row.up),
                down: edges(&row.down),
            })
            .collect();
        Ok(LogList { commits, more, error: None, local_offset })
    }
}

/// A commit to look at.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitParams {
    /// The commit: its id (or the start of it), a branch, `HEAD`.
    pub id: String,
}

/// A file a commit changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitFile {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the repository.
    pub relative: String,
    /// `added`, `modified`, `deleted` or `renamed`.
    pub status: String,
    /// Where a renamed file was, relative.
    pub old_path: Option<String>,
}

/// A commit with its message and the files it changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitDetailsInfo {
    /// The full id.
    pub id: String,
    /// The parents' ids.
    pub parents: Vec<String>,
    /// The author's name and e-mail.
    pub author: String,
    /// The author's e-mail.
    pub email: String,
    /// When it was authored, in seconds since 1970.
    pub time: i64,
    /// The author's time zone, in seconds east of UTC: dates show in it.
    pub offset: i32,
    /// The message's first line, and the rest.
    pub subject: String,
    /// The rest of the message.
    pub body: String,
    /// The files it changed, compared with its first parent.
    pub files: Vec<CommitFile>,
    /// The branches (local first, then remote) and tags whose history has the commit.
    pub contained_in: Vec<RefLabelInfo>,
}

pub struct CommitDetails;

impl Command for CommitDetails {
    type Params = CommitParams;
    type Output = CommitDetailsInfo;
    const ID: &'static str = "vcs.commit.details";
    const TITLE: &'static str = "Commit Details";
    const DESCRIPTION: &'static str = "A commit's author, date, full message, the files it changed (added, modified, deleted, renamed) and the \
         branches and tags that contain it.";

    fn run(&self, session: &mut Session, params: CommitParams) -> Result<CommitDetailsInfo, CommandError> {
        let repository = repository(session)?;
        let (commit, files) =
            repository.commit_details(&params.id).map_err(|error| CommandError::new(message(&error)))?;
        let root = repository.root();
        let contained_in = super::workspace::refs_containing(session, &repository, &commit.id)
            .unwrap_or_else(|| repository.refs_containing(&commit.id).unwrap_or_default());
        Ok(CommitDetailsInfo {
            contained_in: contained_in.iter().map(RefLabelInfo::from).collect(),
            id: commit.id,
            parents: commit.parents,
            author: commit.author,
            email: commit.email,
            time: commit.time,
            offset: commit.offset,
            subject: commit.subject,
            body: commit.body,
            files: files
                .into_iter()
                .map(|file| CommitFile {
                    path: absolute(root, &file.path),
                    status: file.status.as_str().to_owned(),
                    relative: file.path,
                    old_path: file.old_path,
                })
                .collect(),
        })
    }
}

/// A file of a commit.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitFileParams {
    /// The commit.
    pub id: String,
    /// The file (absolute or relative to the repository).
    pub path: PathBuf,
}

pub struct CommitDiff;

impl Command for CommitDiff {
    type Params = CommitFileParams;
    type Output = DiffView;
    const ID: &'static str = "vcs.diff.commit";
    const TITLE: &'static str = "Show Diff";
    const DESCRIPTION: &'static str =
        "Compares a file in a commit with the same file in the commit's first parent: both texts and the changes.";

    fn run(&self, session: &mut Session, params: CommitFileParams) -> Result<DiffView, CommandError> {
        let repository = repository(session)?;
        let relative = super::commit::relative(&repository, &params.path)?;
        let (before, after) =
            repository.file_in_commit(&params.id, &relative).map_err(|error| CommandError::new(message(&error)))?;
        let short: String = params.id.chars().take(8).collect();
        Ok(diff_view(
            absolute(repository.root(), &relative),
            relative,
            (t!("vcs-diff-parent", commit = short.as_str()), short.clone()),
            &before.unwrap_or_default(),
            &after.unwrap_or_default(),
        ))
    }
}

#[derive(Default)]
pub(super) struct Blames {
    annotated: BTreeSet<PathBuf>,
    computed: HashMap<PathBuf, (u64, BlameSlot)>,
}

type BlameSlot = Arc<Mutex<Option<Result<Vec<zen_vcs::BlameLine>, String>>>>;

/// Whether to show or hide annotations.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotateParams {
    /// The file, absolute or relative to the project; the active document when absent.
    pub path: Option<PathBuf>,
    /// Show (true) or hide (false) the annotations; toggles when absent.
    pub show: Option<bool>,
}

/// Whether a file is annotated.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct AnnotateState {
    /// The file.
    pub path: PathBuf,
    /// Whether its annotations are shown.
    pub shown: bool,
}

pub struct Annotate;

impl Command for Annotate {
    type Params = AnnotateParams;
    type Output = AnnotateState;
    const ID: &'static str = "vcs.blame.annotate";
    const TITLE: &'static str = "Annotate with Git Blame";
    const DESCRIPTION: &'static str = "Shows (or hides, or toggles) the editor's blame annotations of a file (the active one by \
         default): who last changed each line and when; vcs.blame.get reads them.";

    fn run(&self, session: &mut Session, params: AnnotateParams) -> Result<AnnotateState, CommandError> {
        let path = file_or_active(session, params.path)?;
        repository(session)?;
        let blames = &mut vcs(session).blames;
        let shown = params.show.unwrap_or(!blames.annotated.contains(&path));
        if shown {
            blames.annotated.insert(path.clone());
        } else {
            blames.annotated.remove(&path);
        }
        Ok(AnnotateState { path, shown })
    }
}

fn file_or_active(session: &Session, path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    let workbench = session.state::<Workbench>();
    match path {
        Some(path) if path.is_relative() => match workbench.and_then(Workbench::project) {
            Some(project) => Ok(lexically_joined(project, &path)),
            None => Ok(path),
        },
        Some(path) => Ok(path),
        None => workbench
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::new(t!("vcs-error-no-file"))),
    }
}

fn lexically_joined(base: &Path, path: &Path) -> PathBuf {
    let mut joined = base.to_path_buf();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                joined.pop();
            }
            other => joined.push(other),
        }
    }
    joined
}

/// Which file's blame to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BlameParams {
    /// The file, absolute or relative to the project; the active document when absent.
    pub path: Option<PathBuf>,
    /// Compute it even when the file is not annotated (agents).
    pub force: bool,
    /// Wait until it is computed.
    pub wait: bool,
}

/// One line's annotation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LineAnnotation {
    /// The commit that last changed the line.
    pub commit: String,
    /// Its first eight characters.
    pub short_id: String,
    /// The commit's author.
    pub author: String,
    /// When it was authored, in seconds since 1970.
    pub time: i64,
    /// The author's time zone, in seconds east of UTC: dates show in it.
    pub offset: i32,
    /// The commit message's first line.
    pub subject: String,
    /// How old the commit is among the file's commits, from 0 (the newest) to 100 (the oldest):
    /// the annotations are shaded by it, the newest brightest.
    pub age: u8,
}

pub fn set_ages(lines: &mut [Option<LineAnnotation>]) {
    let mut times: Vec<i64> = lines.iter().flatten().map(|line| line.time).collect();
    times.sort_unstable_by(|a, b| b.cmp(a));
    times.dedup();
    let last = times.len().saturating_sub(1).max(1);
    for line in lines.iter_mut().flatten() {
        let rank = times.iter().position(|time| *time == line.time).unwrap_or(0);
        line.age = u8::try_from(rank * 100 / last).unwrap_or(100);
    }
}

/// The blame of a file, per line of the document as the editor has it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BlameInfo {
    /// The file.
    pub path: PathBuf,
    /// Whether its annotations are shown.
    pub shown: bool,
    /// Whether blame is still being computed.
    pub pending: bool,
    /// Why blame failed (an untracked file, say).
    pub error: Option<String>,
    /// Per line of the document: who last changed it; `None` for lines changed since the last
    /// commit.
    pub lines: Vec<Option<LineAnnotation>>,
}

pub struct GetBlame;

impl Command for GetBlame {
    type Params = BlameParams;
    type Output = BlameInfo;
    const ID: &'static str = "vcs.blame.get";
    const TITLE: &'static str = "Blame";
    const DESCRIPTION: &'static str = "Who last changed each line of a file (the active one by default), and when, mapped onto the \
         lines as the editor has them; computed in the background, only for annotated files unless \
         `force` is set.";

    fn run(&self, session: &mut Session, params: BlameParams) -> Result<BlameInfo, CommandError> {
        let path = file_or_active(session, params.path)?;
        let repository = repository(session)?;
        let shown = vcs(session).blames.annotated.contains(&path);
        let mut info = BlameInfo { path: path.clone(), shown, ..BlameInfo::default() };
        if !shown && !params.force {
            return Ok(info);
        }
        let stamp = repository.stamp();
        let slot = {
            let blames = &mut vcs(session).blames;
            let fresh = blames.computed.get(&path).is_some_and(|(at, _)| *at == stamp);
            if !fresh {
                let slot: BlameSlot = Arc::default();
                let relative = super::commit::relative(&repository, &path)?;
                let compute = {
                    let (slot, repository) = (Arc::clone(&slot), Arc::clone(&repository));
                    let waker =
                        vcs(session).shared.waker.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
                    move || {
                        let result = repository.blame(&relative).map_err(|error| message(&error));
                        *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
                        if let Some(waker) = waker {
                            waker();
                        }
                    }
                };
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::spawn(compute);
                #[cfg(target_arch = "wasm32")]
                compute();
                vcs(session).blames.computed.insert(path.clone(), (stamp, slot));
            }
            Arc::clone(&vcs(session).blames.computed[&path].1)
        };
        if params.wait {
            for _ in 0..2000 {
                if slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).is_some() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
        let result = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
        let lines = match result {
            None => {
                info.pending = true;
                return Ok(info);
            }
            Some(Err(error)) => {
                info.error = Some(error);
                return Ok(info);
            }
            Some(Ok(lines)) => lines,
        };
        let annotation = |line: &zen_vcs::BlameLine| {
            (!line.commit.is_empty()).then(|| LineAnnotation {
                short_id: line.commit.chars().take(8).collect(),
                commit: line.commit.clone(),
                author: line.author.clone(),
                time: line.time,
                offset: line.offset,
                subject: line.subject.clone(),
                age: 0,
            })
        };
        let document = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .map(|document| document.text().to_owned());
        let base = repository
            .file_at_head(&super::commit::relative(&repository, &path)?)
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok());
        info.lines = match (document, base) {
            (Some(text), Some(base)) if text != base => {
                let count = diff::lines(&text).len();
                let mut mapped = vec![None; count];
                let (mut old, mut new) = (0, 0);
                for hunk in diff::hunks(&base, &text).iter().chain(std::iter::once(&diff::Hunk {
                    before: diff::lines(&base).len()..diff::lines(&base).len(),
                    after: count..count,
                })) {
                    while new < hunk.after.start && old < hunk.before.start {
                        mapped[new] = lines.get(old).and_then(annotation);
                        old += 1;
                        new += 1;
                    }
                    old = hunk.before.end;
                    new = hunk.after.end;
                }
                mapped
            }
            _ => lines.iter().map(annotation).collect(),
        };
        set_ages(&mut info.lines);
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    #[test]
    fn the_log_lists_filters_and_details_commits() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["switch", "-q", "-c", "feature"]);
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        git(&root, &["add", "b.txt"]);
        git(&root, &["commit", "-q", "-m", "Add b"]);
        git(&root, &["switch", "-q", "main"]);
        let (registry, mut session) = super::super::tests::session(&root);
        let log = registry.dispatch(&mut session, "vcs.log.list", json!({})).unwrap();
        let subjects: Vec<&str> =
            log["commits"].as_array().unwrap().iter().map(|c| c["subject"].as_str().unwrap()).collect();
        assert_eq!(subjects, ["Add b", "First"], "every branch");
        assert_eq!(log["commits"][0]["refs"], json!(["feature"]));
        assert_eq!(log["more"], false);
        let main = registry.dispatch(&mut session, "vcs.log.list", json!({ "branch": "main" })).unwrap();
        assert_eq!(main["commits"].as_array().unwrap().len(), 1);
        let paged = registry.dispatch(&mut session, "vcs.log.list", json!({ "limit": 1 })).unwrap();
        assert_eq!(paged["more"], true);
        let by_path = registry.dispatch(&mut session, "vcs.log.list", json!({ "path": root.join("b.txt") })).unwrap();
        assert_eq!(by_path["commits"][0]["subject"], "Add b");

        let id = log["commits"][0]["id"].as_str().unwrap().to_owned();
        let details = registry.dispatch(&mut session, "vcs.commit.details", json!({ "id": id })).unwrap();
        assert_eq!(
            details["files"],
            json!([{ "path": root.join("b.txt"), "relative": "b.txt", "status": "added", "oldPath": null }])
        );
        let view = registry.dispatch(&mut session, "vcs.diff.commit", json!({ "id": id, "path": "b.txt" })).unwrap();
        assert_eq!((view["before"].as_str(), view["after"].as_str()), (Some(""), Some("b\n")));
        assert!(registry.dispatch(&mut session, "vcs.commit.details", json!({ "id": "nope" })).is_err());
    }

    #[test]
    fn blame_annotates_the_lines_as_the_editor_has_them() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let file = root.join("a.txt");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let hidden = registry.dispatch(&mut session, "vcs.blame.get", json!({})).unwrap();
        assert_eq!((hidden["shown"].as_bool(), hidden["lines"].as_array().map(Vec::len)), (Some(false), Some(0)));
        let state = registry.dispatch(&mut session, "vcs.blame.annotate", json!({})).unwrap();
        assert_eq!(state["shown"], true, "toggled on");
        let blame = registry.dispatch(&mut session, "vcs.blame.get", json!({ "wait": true })).unwrap();
        let lines = blame["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 3);
        assert_eq!((lines[0]["author"].as_str(), lines[0]["subject"].as_str()), (Some("Ada Lovelace"), Some("First")));
        let workbench = session.state_mut::<Workbench>();
        workbench.active_document_mut().unwrap().replace_ranges(vec![(0..0, "zero\n".to_owned())]);
        let blame = registry.dispatch(&mut session, "vcs.blame.get", json!({ "wait": true })).unwrap();
        let lines = blame["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 4);
        assert!(lines[0].is_null());
        assert_eq!(lines[1]["subject"], "First");
        let state = registry.dispatch(&mut session, "vcs.blame.annotate", json!({ "show": false })).unwrap();
        assert_eq!(state["shown"], false);
    }

    #[test]
    fn blame_takes_a_path_relative_to_the_project() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": root.join("a.txt") })).unwrap();
        registry.dispatch(&mut session, "vcs.blame.annotate", json!({})).unwrap();
        let blame = registry.dispatch(&mut session, "vcs.blame.get", json!({ "path": "a.txt", "wait": true })).unwrap();
        assert_eq!((blame["path"].as_str(), blame["shown"].as_bool()), (root.join("a.txt").to_str(), Some(true)));
        assert_eq!(blame["lines"][0]["subject"], "First", "{blame}");
        let blame = registry.dispatch(&mut session, "vcs.blame.get", json!({ "path": "./x/../a.txt" })).unwrap();
        assert_eq!(blame["path"].as_str(), root.join("a.txt").to_str());

        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/b.txt"), "b\n").unwrap();
        git(&root, &["add", "sub/b.txt"]);
        git(&root, &["commit", "-q", "-m", "Add b"]);
        let (registry, mut session) = super::super::tests::session(&root.join("sub"));
        let params = json!({ "path": "b.txt", "force": true, "wait": true });
        let blame = registry.dispatch(&mut session, "vcs.blame.get", params).unwrap();
        assert_eq!(blame["path"].as_str(), root.join("sub/b.txt").to_str());
        assert_eq!(blame["lines"][0]["subject"], "Add b", "{blame}");
    }

    #[test]
    fn blame_shades_the_lines_by_the_age_of_their_commits() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let file = root.join("a.txt");
        for (day, text) in [(1, "one\nsecond\nthree\n"), (2, "one\nsecond\nthird\n")] {
            std::fs::write(&file, text).unwrap();
            git(&root, &["add", "a.txt"]);
            let date = format!("2030-01-0{day}T12:00:00Z");
            git(&root, &["commit", "-q", "-m", &format!("Day {day}"), "--date", &date]);
        }
        let (registry, mut session) = super::super::tests::session(&root);
        let params = json!({ "path": file, "force": true, "wait": true });
        let blame = registry.dispatch(&mut session, "vcs.blame.get", params).unwrap();
        let ages: Vec<u64> =
            blame["lines"].as_array().unwrap().iter().map(|line| line["age"].as_u64().unwrap()).collect();
        assert_eq!(ages, [100, 50, 0], "the first commit's line is the oldest, the last one's the newest");
    }

    #[test]
    fn ages_rank_the_commits_newest_first() {
        let at = |time: i64| Some(LineAnnotation { time, ..LineAnnotation::default() });
        let mut lines = vec![at(10), None, at(30), at(20), at(30)];
        set_ages(&mut lines);
        let ages: Vec<Option<u8>> = lines.iter().map(|line| line.as_ref().map(|line| line.age)).collect();
        assert_eq!(ages, [Some(100), None, Some(0), Some(50), Some(0)]);
        let mut one = vec![at(5), at(5)];
        set_ages(&mut one);
        assert_eq!(one.iter().flatten().map(|line| line.age).collect::<Vec<_>>(), [0, 0], "one commit is the newest");
    }
}

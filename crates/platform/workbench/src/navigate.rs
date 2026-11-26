use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};

use crate::Workbench;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GoToFile)?;
    registry.register(RefreshIndex)?;
    registry.register(RecentFiles)?;
    registry.register(RestoreRecent)?;
    registry.register(RemoveRecent)?;
    registry.register(MatchNames)?;
    registry.register(RecentLocations)?;
    registry.register(RemoveLocation)?;
    Ok(())
}

/// What to look for.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct GoToFileParams {
    /// A file name, a part of it or its camel humps (`edvi` finds `editor_view.rs`), with `*`, folders before `/`, and `:line[:column]`.
    pub query: String,
    /// How many results at most; 50 when absent.
    pub limit: Option<usize>,
}

/// One file found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct FileMatch {
    /// The absolute path.
    pub path: PathBuf,
    /// The path relative to the project.
    pub relative: String,
    /// How it matches: `exact`, `prefix`, `humps` or `middle`.
    pub kind: String,
    /// The matched characters of `relative`, as `[start, end)` character indexes.
    pub ranges: Vec<[usize; 2]>,
}

/// The files found, and whether the index was still being built.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GoToFileResult {
    /// The matches, best first.
    pub files: Vec<FileMatch>,
    /// Whether the project is still being indexed (the list may be incomplete).
    pub indexing: bool,
    /// The line the query ended with (`main.go:42`), one-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column the query ended with (`main.go:42:7`), one-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

pub struct GoToFile;

impl Command for GoToFile {
    type Params = GoToFileParams;
    type Output = GoToFileResult;
    const ID: &'static str = "navigate.file.goTo";
    const TITLE: &'static str = "Go to File";
    const DESCRIPTION: &'static str = "Finds project files by name with the shared name matcher (navigate.names.match): \
         the name, a prefix, camel humps (`mgo` finds `main.go`), `*`, folders before `/` (`internal/ca`); a `:line[:column]` \
         suffix comes back as line and column. Returns the best matches with their matched ranges; open files rank higher \
         among equals (⇧⌘O).";

    fn run(&self, session: &mut Session, params: GoToFileParams) -> Result<GoToFileResult, CommandError> {
        let workbench = session.state::<Workbench>();
        let location = zen_index::Pattern::path(&params.query).location();
        let (line, column) = (location.map(|at| at.line), location.and_then(|at| at.column));
        let Some(index) = workbench.and_then(|workbench| workbench.index.as_ref()) else {
            return Ok(GoToFileResult { files: Vec::new(), indexing: false, line, column });
        };
        let mut recent: Vec<PathBuf> = Vec::new();
        if let Some(workbench) = workbench {
            if let Some(active) = workbench.active_document() {
                recent.push(active.path().to_owned());
            }
            recent.extend(workbench.documents().iter().rev().map(|document| document.path().to_owned()));
        }
        let files = index
            .search(&params.query, params.limit.unwrap_or(50), &recent)
            .into_iter()
            .map(|found| FileMatch {
                path: found.path,
                relative: found.relative,
                kind: found.kind.as_str().to_owned(),
                ranges: spans(&found.ranges),
            })
            .collect();
        Ok(GoToFileResult { files, indexing: !index.is_ready(), line, column })
    }
}

/// Paths that changed on disk.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RefreshParams {
    /// The changed files and folders, absolute.
    pub paths: Vec<PathBuf>,
}

pub struct RefreshIndex;

impl Command for RefreshIndex {
    type Params = RefreshParams;
    type Output = usize;
    const ID: &'static str = "file.index.refresh";
    const TITLE: &'static str = "Refresh File Index";
    const DESCRIPTION: &'static str = "Updates the project's file index after files were created, renamed or removed; returns how many files it holds.";

    fn run(&self, session: &mut Session, params: RefreshParams) -> Result<usize, CommandError> {
        let Some(index) = session.state::<Workbench>().and_then(|workbench| workbench.index.as_ref()) else {
            return Ok(0);
        };
        index.set_paused(crate::power_save::is_on(session));
        index.refresh(&params.paths);
        Ok(index.len())
    }
}

/// A recent file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RecentFile {
    /// The absolute path.
    pub path: PathBuf,
    /// The path relative to the project.
    pub relative: String,
    /// Whether it is open in a tab.
    pub open: bool,
    /// The characters of `relative` the query matched, as `[start, end)` character indexes; empty without a query.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<[usize; 2]>,
}

/// Which recent files.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RecentFilesParams {
    /// Keeps the files whose name matches, as Go to File matches (folders before `/`); every file when absent.
    pub query: Option<String>,
    /// Show edited only: the files edited while the project is open (⌘E pressed again in the popup).
    pub edited_only: bool,
}

pub struct RecentFiles;

impl Command for RecentFiles {
    type Params = RecentFilesParams;
    type Output = Vec<RecentFile>;
    const ID: &'static str = "navigate.recent.files";
    const TITLE: &'static str = "Recent Files";
    const DESCRIPTION: &'static str = "Returns the files shown last in this project, most recent first, and whether each is \
         open (⌘E); with editedOnly, only the files edited while the project is open (Show edited only); with \
         `query`, only those it matches by the shared name matcher, with the matched ranges.";

    fn run(&self, session: &mut Session, params: RecentFilesParams) -> Result<Vec<RecentFile>, CommandError> {
        let edited = if params.edited_only {
            Some(crate::history::with_history(session, |history, _| history.edited_files().to_vec()))
        } else {
            None
        };
        let Some(workbench) = session.state::<Workbench>() else { return Ok(Vec::new()) };
        let project = workbench.project().map(Path::to_owned).unwrap_or_default();
        let pattern = zen_index::Pattern::path(params.query.as_deref().unwrap_or_default());
        Ok(workbench
            .recent_files()
            .iter()
            .filter(|path| path.exists())
            .filter(|path| edited.as_ref().is_none_or(|edited| edited.contains(path)))
            .filter_map(|path| {
                let relative = path.strip_prefix(&project).unwrap_or(path).to_string_lossy().replace('\\', "/");
                let found = pattern.matches_path(&relative)?;
                Some(RecentFile {
                    path: path.clone(),
                    open: workbench.documents().iter().any(|document| document.path() == path),
                    ranges: spans(&found.ranges),
                    relative,
                })
            })
            .collect())
    }
}

pub fn spans(ranges: &[std::ops::Range<usize>]) -> Vec<[usize; 2]> {
    ranges.iter().map(|range| [range.start, range.end]).collect()
}

/// How a query of `navigate.names.match` splits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NameMode {
    /// One part: plain names, as actions.
    #[default]
    Names,
    /// File paths relative to a project: folders before `/`.
    Files,
    /// Qualified names (`model.Store.Get`): owners before `.`, `::` or `/`.
    Symbols,
}

/// The names to match.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct MatchNamesParams {
    /// The query, as typed in Search Everywhere.
    pub query: String,
    /// The names to match.
    pub names: Vec<String>,
    /// How the query splits; `names` when absent.
    #[serde(default)]
    pub mode: NameMode,
    /// The names of `names` that are not in the project (the SDK's, a library's): among matches of the same kind they come after the project's.
    #[serde(default)]
    pub libraries: Vec<String>,
}

/// One name the query matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NameMatchItem {
    /// The name as given.
    pub name: String,
    /// How it matches: `exact`, `prefix`, `humps` or `middle`.
    pub kind: String,
    /// How well within its kind; higher is better.
    pub degree: i64,
    /// The matched characters of `name`, as `[start, end)` character indexes.
    pub ranges: Vec<[usize; 2]>,
}

/// What `navigate.names.match` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct MatchNamesResult {
    /// The names that match, best first.
    pub matches: Vec<NameMatchItem>,
    /// The line the query ended with (`main.go:42`), one-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// The column the query ended with, one-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
}

pub struct MatchNames;

impl Command for MatchNames {
    type Params = MatchNamesParams;
    type Output = MatchNamesResult;
    const ID: &'static str = "navigate.names.match";
    const TITLE: &'static str = "Match Names";
    const DESCRIPTION: &'static str = "Matches names as Go to File, Go to Class, Go to Symbol, Find Action and Recent Files \
         do: ignoring case, a capital starts a camel hump (`NPE`, `nPE`, `nullpoex` find `NullPointerException`), `*` is \
         any characters, a space a word boundary, a space at the end the end of the name; `/` (and `.` for symbols) puts \
         owners before the name; a `:line[:column]` suffix comes back apart. Returns the matches best first (exact, prefix, \
         humps, middle; within each the project's before those in `libraries`, then the closer match, then the shorter name) with their matched ranges.";

    fn run(&self, _: &mut Session, params: MatchNamesParams) -> Result<MatchNamesResult, CommandError> {
        let pattern = match params.mode {
            NameMode::Files => zen_index::Pattern::path(&params.query),
            NameMode::Symbols => zen_index::Pattern::qualified(&params.query),
            NameMode::Names => zen_index::Pattern::path(&params.query.replace(['/', '\\'], "")),
        };
        let mut found: Vec<(zen_index::Rank, NameMatchItem)> = params
            .names
            .into_iter()
            .filter_map(|name| {
                let last = name.rsplit(['/', '.']).next().unwrap_or(&name).chars().count();
                let (found, length) = match params.mode {
                    NameMode::Files => {
                        (pattern.matches_path(&name)?, name.rsplit('/').next().unwrap_or(&name).chars().count())
                    }
                    NameMode::Symbols => (pattern.matches_qualified(&name, &[])?, last),
                    NameMode::Names => (pattern.name().matches(&name)?, name.chars().count()),
                };
                let rank = zen_index::Rank::new(&found, length).in_project(!params.libraries.contains(&name));
                let kind = found.kind.as_str().to_owned();
                Some((rank, NameMatchItem { name, kind, degree: found.degree, ranges: spans(&found.ranges) }))
            })
            .collect();
        found.sort_by_key(|item| item.0);
        let location = pattern.location();
        Ok(MatchNamesResult {
            matches: found.into_iter().map(|(_, item)| item).collect(),
            line: location.map(|at| at.line),
            column: location.and_then(|at| at.column),
        })
    }
}

/// Recent files to bring back.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RestoreParams {
    /// Most recent first.
    pub paths: Vec<PathBuf>,
}

pub struct RestoreRecent;

impl Command for RestoreRecent {
    type Params = RestoreParams;
    type Output = ();
    const ID: &'static str = "navigate.recent.restore";
    const TITLE: &'static str = "Restore Recent Files";
    const DESCRIPTION: &'static str =
        "Sets the recent files of the project (most recent first), as kept from the last session.";

    fn run(&self, session: &mut Session, params: RestoreParams) -> Result<(), CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let mut recent = params.paths;
        for path in std::mem::take(&mut workbench.recent) {
            if !recent.contains(&path) {
                recent.push(path);
            }
        }
        recent.truncate(50);
        workbench.recent = recent;
        Ok(())
    }
}

/// A recent file to forget.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RemoveRecentParams {
    /// The file, absolute.
    pub path: PathBuf,
}

/// What Delete from Recent Files did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct RemovedRecent {
    /// Whether the file was among the recent files.
    pub listed: bool,
    /// Whether its tabs were closed.
    pub closed: bool,
    /// Whether its unsaved changes were saved before its tabs closed.
    pub saved: bool,
}

pub struct RemoveRecent;

impl Command for RemoveRecent {
    type Params = RemoveRecentParams;
    type Output = RemovedRecent;
    const ID: &'static str = "navigate.recent.remove";
    const TITLE: &'static str = "Delete from Recent Files";
    const DESCRIPTION: &'static str = "Removes a file from the recent files (the Switcher and Recent Files no longer list \
         it) and closes its tabs in every editor group. Unsaved changes are saved first, as Save All saves them; when \
         the file cannot be saved, nothing changes and the error says why. Returns whether it was listed, whether tabs \
         were closed and whether unsaved changes were saved.";

    fn run(&self, session: &mut Session, params: RemoveRecentParams) -> Result<RemovedRecent, CommandError> {
        let path = params.path;
        let saved = save_before_closing(session, &path)?;
        let origin = crate::history::caret(session);
        crate::history::closing(session);
        let workbench = session.state_mut::<Workbench>();
        let listed = workbench.recent.contains(&path);
        workbench.recent.retain(|recent| *recent != path);
        let closed = workbench.groups.contains(&path);
        if closed {
            workbench.groups.retain(|tab| tab != path);
            workbench.forget_document(&path);
            workbench.sync_groups();
        }
        crate::history::left_file(session, origin.filter(|origin| origin.path == path));
        Ok(RemovedRecent { listed, closed, saved })
    }
}

fn save_before_closing(session: &mut Session, path: &Path) -> Result<bool, CommandError> {
    let dirty = session
        .state::<Workbench>()
        .is_some_and(|workbench| workbench.documents.iter().any(|document| document.path == path && document.dirty));
    if !dirty {
        return Ok(false);
    }
    crate::language::format::before_save(session, path);
    let workbench = session.state_mut::<Workbench>();
    let result =
        workbench.documents.iter_mut().filter(|document| document.path == path && document.dirty).try_for_each(
            |document| {
                let name = document.name();
                document.save().map_err(|error| CommandError::new(crate::closing::save_failed(&name, &error)))
            },
        );
    crate::local_history::record_saves(session);
    result.map(|()| true)
}

pub const SNIPPET_LINES: u32 = 5;

/// Which recent locations.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RecentLocationsParams {
    /// Show edited only: the places where the text was edited (⇧⌘E pressed again in the popup).
    pub edited_only: bool,
    /// Keeps the places whose code, file name or breadcrumbs contain it, ignoring case; all when empty.
    pub query: String,
}

/// A line of a recent location's code.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct SnippetLine {
    /// Its number, one-based.
    pub line: u32,
    /// Its text, without the line break.
    pub text: String,
}

/// A place of Recent Locations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecentLocation {
    /// The file, absolute.
    pub path: PathBuf,
    /// The file relative to the project.
    pub relative: String,
    /// The place, a character offset in the file's text.
    pub offset: usize,
    /// Its line, one-based.
    pub line: u32,
    /// When it was last visited or edited, in seconds since 1970.
    pub time: u64,
    /// Whether the text was edited there.
    pub edited: bool,
    /// The declarations around it, outermost first, as the breadcrumbs name them.
    pub breadcrumbs: Vec<String>,
    /// The lines of code around it ([`SNIPPET_LINES`]).
    pub snippet: Vec<SnippetLine>,
}

pub struct RecentLocations;

impl Command for RecentLocations {
    type Params = RecentLocationsParams;
    type Output = Vec<RecentLocation>;
    const ID: &'static str = "navigate.recent.locations";
    const TITLE: &'static str = "Recent Locations";
    const DESCRIPTION: &'static str = "Returns the places navigation visited and the places edited in this project, most \
         recent first (⇧⌘E): each with its file, line, time, whether it was edited, its breadcrumbs and the lines of code \
         around it. editedOnly keeps the edited places; query keeps those whose code, file name or breadcrumbs contain it. \
         Go to one with navigate.place.goTo.";

    fn run(&self, session: &mut Session, params: RecentLocationsParams) -> Result<Vec<RecentLocation>, CommandError> {
        let places = crate::history::with_history(session, |history, _| history.places().to_vec());
        let project = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned).unwrap_or_default();
        let query = params.query.trim().to_lowercase();
        let mut found = Vec::new();
        for place in places.into_iter().rev().filter(|place| place.edited || !params.edited_only) {
            let spot = place.spot;
            let Some(text) = file_text(session, &spot.path) else { continue };
            let symbols = crate::language::structure::resolve(session, &spot.path, std::time::Duration::ZERO).symbols;
            let breadcrumbs: Vec<String> = crate::language::breadcrumbs::crumbs(&symbols, spot.offset)
                .into_iter()
                .map(|crumb| crumb.name)
                .collect();
            let total = text.lines().count() as u32;
            let first = spot.line.saturating_sub(SNIPPET_LINES / 2).min(total.saturating_sub(SNIPPET_LINES - 1)).max(1);
            let snippet: Vec<SnippetLine> = text
                .lines()
                .enumerate()
                .skip(first as usize - 1)
                .take(SNIPPET_LINES as usize)
                .map(|(index, line)| SnippetLine { line: index as u32 + 1, text: line.to_owned() })
                .collect();
            let relative = spot.path.strip_prefix(&project).unwrap_or(&spot.path).to_string_lossy().into_owned();
            let name = relative.rsplit('/').next().unwrap_or(&relative).to_lowercase();
            let matches = query.is_empty()
                || name.contains(&query)
                || breadcrumbs.iter().any(|crumb| crumb.to_lowercase().contains(&query))
                || snippet.iter().any(|line| line.text.to_lowercase().contains(&query));
            if matches {
                found.push(RecentLocation {
                    path: spot.path,
                    relative,
                    offset: spot.offset,
                    line: spot.line,
                    time: place.time,
                    edited: place.edited,
                    breadcrumbs,
                    snippet,
                });
            }
        }
        Ok(found)
    }
}

fn file_text(session: &Session, path: &Path) -> Option<String> {
    let open = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .filter(|document| document.image().is_none())
        .map(|document| document.text().to_owned());
    open.or_else(|| std::fs::read_to_string(path).ok())
}

/// A recent location to forget.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RemoveLocationParams {
    /// The file, absolute.
    pub path: PathBuf,
    /// The line, one-based.
    pub line: u32,
}

pub struct RemoveLocation;

impl Command for RemoveLocation {
    type Params = RemoveLocationParams;
    type Output = bool;
    const ID: &'static str = "navigate.recent.removeLocation";
    const TITLE: &'static str = "Remove from Recent Locations";
    const DESCRIPTION: &'static str = "Forgets the places on a line of a file: Recent Locations no longer lists them and \
         Back, Forward, Last and Next Edit Location no longer go there. Returns whether there was one.";

    fn run(&self, session: &mut Session, params: RemoveLocationParams) -> Result<bool, CommandError> {
        Ok(crate::history::with_history(session, |history, _| history.remove(&params.path, params.line)))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn finds_files_and_follows_changes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("internal/cart")).unwrap();
        std::fs::write(dir.path().join("internal/cart/cart.go"), "").unwrap();
        std::fs::write(dir.path().join("main.go"), "").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let mut found = json!(null);
        for _ in 0..200 {
            found = registry.dispatch(&mut session, "navigate.file.goTo", json!({ "query": "cart" })).unwrap();
            if found["indexing"] == false {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(found["files"][0]["relative"], "internal/cart/cart.go");
        std::fs::write(dir.path().join("checkout.go"), "").unwrap();
        let count = registry
            .dispatch(&mut session, "file.index.refresh", json!({ "paths": [dir.path().join("checkout.go")] }))
            .unwrap();
        assert_eq!(count, 3);
        let found = registry.dispatch(&mut session, "navigate.file.goTo", json!({ "query": "check:3:2" })).unwrap();
        assert_eq!(found["files"][0]["relative"], "checkout.go");
        assert_eq!(found["files"][0]["kind"], "prefix");
        assert_eq!(found["files"][0]["ranges"], json!([[0, 5]]));
        assert_eq!((found["line"].as_u64(), found["column"].as_u64()), (Some(3), Some(2)));
        let found = registry.dispatch(&mut session, "navigate.file.goTo", json!({ "query": "int/ca" })).unwrap();
        assert_eq!(found["files"][0]["ranges"], json!([[0, 3], [14, 16]]));

        for file in ["main.go", "checkout.go", "main.go"] {
            registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join(file) })).unwrap();
        }
        let recent = registry.dispatch(&mut session, "navigate.recent.files", json!({})).unwrap();
        assert_eq!(recent[0]["relative"], "main.go");
        assert_eq!(recent[1]["relative"], "checkout.go");
        assert_eq!(recent[0]["open"], true);
        let filtered = registry.dispatch(&mut session, "navigate.recent.files", json!({ "query": "chec" })).unwrap();
        assert_eq!(filtered.as_array().map(Vec::len), Some(1), "{filtered}");
        assert_eq!(filtered[0]["relative"], "checkout.go");
        assert_eq!(filtered[0]["ranges"], json!([[0, 4]]));
        let none = registry.dispatch(&mut session, "navigate.recent.files", json!({ "query": "ain" })).unwrap();
        assert_eq!(none.as_array().map(Vec::len), Some(1), "a part of a word: main.go");
        let none = registry.dispatch(&mut session, "navigate.recent.files", json!({ "query": "mgx" })).unwrap();
        assert_eq!(none, json!([]));
        let edited = registry.dispatch(&mut session, "navigate.recent.files", json!({ "editedOnly": true })).unwrap();
        assert_eq!(edited, json!([]), "nothing was edited yet");
    }

    #[test]
    fn names_match_as_every_search_by_name() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let names = json!(["NullPointerException", "NumberFormatException", "Nope"]);
        for query in ["NPE", "nPE", "nullpoex"] {
            let found = registry
                .dispatch(&mut session, "navigate.names.match", json!({ "query": query, "names": names }))
                .unwrap();
            assert_eq!(found["matches"].as_array().map(Vec::len), Some(1), "{query}: {found}");
            assert_eq!(found["matches"][0]["name"], "NullPointerException");
            assert_eq!(found["matches"][0]["kind"], "humps");
        }
        let found = registry
            .dispatch(
                &mut session,
                "navigate.names.match",
                json!({ "query": "no", "names": ["Nope", "NotNow", "Nonono", "Ano"] }),
            )
            .unwrap();
        let order: Vec<&str> =
            found["matches"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
        assert_eq!(order, ["Nope", "NotNow", "Nonono", "Ano"]);
        assert_eq!(found["matches"][3]["kind"], "middle");
        let files = json!({ "query": "cmd/mgo:12:4", "names": ["cmd/main.go", "main.go", "cmd/app/mango.go"], "mode": "files" });
        let found = registry.dispatch(&mut session, "navigate.names.match", files).unwrap();
        assert_eq!((found["line"].as_u64(), found["column"].as_u64()), (Some(12), Some(4)));
        let order: Vec<&str> =
            found["matches"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
        assert_eq!(order, ["cmd/main.go", "cmd/app/mango.go"]);
        assert_eq!(found["matches"][0]["ranges"], json!([[0, 3], [4, 5], [9, 11]]));
        let symbols =
            json!({ "query": "St.get", "names": ["Store.GetItem", "Cart.Get", "Store.Put"], "mode": "symbols" });
        let found = registry.dispatch(&mut session, "navigate.names.match", symbols).unwrap();
        assert_eq!(found["matches"].as_array().map(Vec::len), Some(1));
        assert_eq!(found["matches"][0]["ranges"], json!([[0, 2], [6, 9]]));
        let cart = json!({ "query": "Cart", "names": ["CartPool", "CartStore", "MyCart", "Cart"], "libraries": ["CartPool", "Cart"] });
        let found = registry.dispatch(&mut session, "navigate.names.match", cart).unwrap();
        let order: Vec<&str> =
            found["matches"].as_array().unwrap().iter().map(|m| m["name"].as_str().unwrap()).collect();
        assert_eq!(order, ["Cart", "CartStore", "CartPool", "MyCart"], "the project's before a shorter SDK name");
        let wrong =
            registry.dispatch(&mut session, "navigate.names.match", json!({ "query": "a", "names": [], "mode": "x" }));
        assert!(wrong.is_err());
    }

    const MAIN: &str = "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfmt.Println(\"привет\")\n}\n\nfunc helper() int {\n\treturn 1\n}\n";

    fn project() -> (Registry, Session, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("main.go"), MAIN).unwrap();
        std::fs::write(dir.path().join("util.go"), "package main\n\nvar answer = 42\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        (registry, session, dir)
    }

    fn char_at(text: &str, needle: &str) -> usize {
        text[..text.find(needle).unwrap()].chars().count()
    }

    #[test]
    fn recent_files_show_edited_only_lists_the_files_edited() {
        let (registry, mut session, dir) = project();
        let (main, util) = (dir.path().join("main.go"), dir.path().join("util.go"));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "// x\n" })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": util })).unwrap();
        let all = registry.dispatch(&mut session, "navigate.recent.files", json!({})).unwrap();
        assert_eq!(all.as_array().unwrap().len(), 2);
        let edited = registry.dispatch(&mut session, "navigate.recent.files", json!({ "editedOnly": true })).unwrap();
        assert_eq!(edited.as_array().unwrap().len(), 1, "{edited}");
        assert_eq!(edited[0]["relative"], "main.go");
    }

    #[test]
    fn delete_from_recent_files_forgets_the_file_and_closes_its_tab() {
        let (registry, mut session, dir) = project();
        let (main, util) = (dir.path().join("main.go"), dir.path().join("util.go"));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": util })).unwrap();
        let removed = registry.dispatch(&mut session, "navigate.recent.remove", json!({ "path": main })).unwrap();
        assert_eq!(removed, json!({ "listed": true, "closed": true, "saved": false }));
        let recent = registry.dispatch(&mut session, "navigate.recent.files", json!({})).unwrap();
        let listed: Vec<&str> =
            recent.as_array().unwrap().iter().filter_map(|file| file["relative"].as_str()).collect();
        assert_eq!(listed, ["util.go"]);
        let workbench = session.state::<Workbench>().unwrap();
        assert!(!workbench.groups().contains(&main), "its tab is closed");
        assert_eq!(workbench.active_document().map(|document| document.path().to_owned()), Some(util.clone()));

        let removed = registry.dispatch(&mut session, "navigate.recent.remove", json!({ "path": util })).unwrap();
        assert_eq!(removed, json!({ "listed": true, "closed": true, "saved": false }));
        assert!(session.state::<Workbench>().unwrap().active_document().is_none(), "the last tab closed");
        let again = registry.dispatch(&mut session, "navigate.recent.remove", json!({ "path": util })).unwrap();
        assert_eq!(again, json!({ "listed": false, "closed": false, "saved": false }));
    }

    #[test]
    fn delete_from_recent_files_saves_unsaved_changes_before_closing_the_tab() {
        let (registry, mut session, dir) = project();
        let (main, util) = (dir.path().join("main.go"), dir.path().join("util.go"));
        registry.dispatch(&mut session, "file.document.open", json!({ "path": main })).unwrap();
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "// kept\n" })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": util })).unwrap();
        let removed = registry.dispatch(&mut session, "navigate.recent.remove", json!({ "path": main })).unwrap();
        assert_eq!(removed, json!({ "listed": true, "closed": true, "saved": true }));
        assert_eq!(std::fs::read_to_string(&main).unwrap(), format!("// kept\n{MAIN}"), "the edit is on disk");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "// lost?\n" })).unwrap();
            let folder = |mode| std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(mode)).unwrap();
            folder(0o555);
            let refused = registry.dispatch(&mut session, "navigate.recent.remove", json!({ "path": util }));
            folder(0o755);
            assert!(refused.is_err(), "a file that cannot be saved stays open: {refused:?}");
            let workbench = session.state::<Workbench>().unwrap();
            assert!(workbench.groups().contains(&util), "its tab stays");
            assert!(workbench.active_document().is_some_and(|document| document.is_dirty()), "with its changes");
            let recent = registry.dispatch(&mut session, "navigate.recent.files", json!({})).unwrap();
            assert!(recent.to_string().contains("util.go"), "and it is still listed: {recent}");
        }
    }

    #[test]
    fn recent_locations_list_visited_and_edited_places_newest_first_with_their_code() {
        let (registry, mut session, dir) = project();
        let (main, util) = (dir.path().join("main.go"), dir.path().join("util.go"));
        let in_helper = char_at(MAIN, "return 1");
        registry.dispatch(&mut session, "navigate.place.goTo", json!({ "path": main, "offset": in_helper })).unwrap();
        registry.dispatch(&mut session, "navigate.place.goTo", json!({ "path": util, "offset": 15 })).unwrap();
        let list = registry.dispatch(&mut session, "navigate.recent.locations", json!({})).unwrap();
        let places: Vec<(String, u64)> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|place| (place["relative"].as_str().unwrap().to_owned(), place["line"].as_u64().unwrap()))
            .collect();
        assert_eq!(places, [("util.go".to_owned(), 3), ("main.go".to_owned(), 10)], "{list}");
        let helper = &list[1];
        assert_eq!(helper["edited"], false);
        assert!(helper["time"].as_u64().unwrap() > 1_700_000_000);
        let lines: Vec<u64> =
            helper["snippet"].as_array().unwrap().iter().map(|l| l["line"].as_u64().unwrap()).collect();
        assert_eq!(lines, [7, 8, 9, 10, 11], "five lines around the place, the last ones at the end");
        assert_eq!(helper["snippet"][3]["text"], "\treturn 1");

        registry
            .dispatch(
                &mut session,
                "navigate.place.goTo",
                json!({ "path": main, "offset": char_at(MAIN, "fmt.Println") }),
            )
            .unwrap();
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "x" })).unwrap();
        let edited =
            registry.dispatch(&mut session, "navigate.recent.locations", json!({ "editedOnly": true })).unwrap();
        assert_eq!(edited.as_array().unwrap().len(), 1, "{edited}");
        assert_eq!((edited[0]["relative"].as_str(), edited[0]["line"].as_u64()), (Some("main.go"), Some(6)));
        assert_eq!(edited[0]["edited"], true);
        let all = registry.dispatch(&mut session, "navigate.recent.locations", json!({})).unwrap();
        assert_eq!(all[0]["line"], 6, "the newest first");
        assert_eq!(all.as_array().unwrap().len(), 3, "one place per line: {all}");
    }

    #[test]
    fn recent_locations_filter_by_code_file_name_and_breadcrumbs() {
        let (registry, mut session, dir) = project();
        let (main, util) = (dir.path().join("main.go"), dir.path().join("util.go"));
        registry
            .dispatch(&mut session, "navigate.place.goTo", json!({ "path": main, "offset": char_at(MAIN, "return 1") }))
            .unwrap();
        registry.dispatch(&mut session, "navigate.place.goTo", json!({ "path": util, "offset": 15 })).unwrap();
        let find = |session: &mut Session, query: &str| -> Vec<String> {
            let list = registry.dispatch(session, "navigate.recent.locations", json!({ "query": query })).unwrap();
            list.as_array().unwrap().iter().map(|place| place["relative"].as_str().unwrap().to_owned()).collect()
        };
        assert_eq!(find(&mut session, "ANSWER"), ["util.go"], "the code, ignoring case");
        assert_eq!(find(&mut session, "main.g"), ["main.go"], "the file name");
        assert_eq!(find(&mut session, "func helper"), ["main.go"], "the code around the place");
        assert!(find(&mut session, "nowhere").is_empty());
    }

    #[test]
    fn removing_a_location_also_takes_it_out_of_back() {
        let (registry, mut session, dir) = project();
        let (main, util) = (dir.path().join("main.go"), dir.path().join("util.go"));
        registry
            .dispatch(&mut session, "navigate.place.goTo", json!({ "path": main, "offset": char_at(MAIN, "return 1") }))
            .unwrap();
        registry.dispatch(&mut session, "navigate.place.goTo", json!({ "path": util, "offset": 15 })).unwrap();
        let removed = registry
            .dispatch(&mut session, "navigate.recent.removeLocation", json!({ "path": main, "line": 10 }))
            .unwrap();
        assert_eq!(removed, true);
        let list = registry.dispatch(&mut session, "navigate.recent.locations", json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
        let history = registry.dispatch(&mut session, "navigate.history.list", json!({})).unwrap();
        assert!(
            history["back"].as_array().unwrap().iter().all(|spot| spot["line"] != 10),
            "Back no longer goes there: {history}"
        );
        let again = registry
            .dispatch(&mut session, "navigate.recent.removeLocation", json!({ "path": main, "line": 10 }))
            .unwrap();
        assert_eq!(again, false);
    }
}

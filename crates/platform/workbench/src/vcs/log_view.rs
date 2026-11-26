use std::collections::BTreeSet;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use super::log::{LogParams, LogSort};
use super::{repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ViewLog)?;
    registry.register(FilterLog)?;
    registry.register(NewTab)?;
    registry.register(CloseTab)?;
    registry.register(SelectTab)?;
    registry.register(SelectCommit)?;
    registry.register(FavoriteBranch)?;
    registry.register(Favorites)?;
    registry.register(RecentPaths)?;
    Ok(())
}

const DEFAULT_FAVORITES: [&str; 4] = ["main", "master", "origin/main", "origin/master"];

const RECENT_PATHS: usize = 5;

/// The filters of a Log tab.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LogFilters {
    /// Text or hash: commits whose message contains it or whose id starts with it.
    pub text: String,
    /// The text is a regular expression (the `.*` toggle).
    pub regex: bool,
    /// The text matches in the same case only (the `Cc` toggle).
    pub match_case: bool,
    /// Only the history of these branches (or `HEAD`, or revisions); every branch when empty.
    pub branches: Vec<String>,
    /// Leave out the history of these revisions (`HEAD`: what the branches add to the current one).
    pub exclude: Vec<String>,
    /// Only commits by one of these authors (part of a name or an e-mail).
    pub users: Vec<String>,
    /// Only commits authored at or after this time, in seconds since 1970.
    pub since: Option<i64>,
    /// Only commits authored at or before this time, in seconds since 1970.
    pub until: Option<i64>,
    /// Only commits that changed one of these files or folders (relative to the repository, `/`).
    pub paths: Vec<String>,
    /// The order: `date` or `topological`.
    pub sort: LogSort,
}

impl LogFilters {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.branches.is_empty()
            && self.exclude.is_empty()
            && self.users.is_empty()
            && self.since.is_none()
            && self.until.is_none()
            && self.paths.is_empty()
    }

    pub fn apply_to(self, params: &mut LogParams) {
        params.branch = None;
        params.author = None;
        params.path = None;
        params.text = Some(self.text).filter(|text| !text.trim().is_empty());
        params.regex = self.regex;
        params.match_case = self.match_case;
        params.branches = self.branches;
        params.exclude = self.exclude;
        params.users = self.users;
        params.since = self.since;
        params.until = self.until;
        params.paths = self.paths.into_iter().map(PathBuf::from).collect();
        params.sort = self.sort;
    }

    fn patch(&mut self, patch: FilterPatch, root: Option<&std::path::Path>) {
        if patch.reset {
            *self = LogFilters { sort: self.sort, ..LogFilters::default() };
        }
        if let Some(text) = patch.text {
            self.text = text;
        }
        if let Some(regex) = patch.regex {
            self.regex = regex;
        }
        if let Some(match_case) = patch.match_case {
            self.match_case = match_case;
        }
        if let Some(branches) = patch.branches {
            self.branches = dedup(branches);
        }
        if let Some(exclude) = patch.exclude {
            self.exclude = dedup(exclude);
        }
        if let Some(users) = patch.users {
            self.users = dedup(users);
        }
        if let Some(date) = patch.date {
            self.since = date.since;
            self.until = date.until;
        }
        if let Some(paths) = patch.paths {
            let relative = |path: PathBuf| -> String {
                let path = match root {
                    Some(root) if path.is_absolute() => path.strip_prefix(root).map(PathBuf::from).unwrap_or(path),
                    _ => path,
                };
                path.to_string_lossy().replace('\\', "/").trim_matches('/').to_owned()
            };
            self.paths = dedup(paths.into_iter().map(relative).filter(|path| !path.is_empty()).collect());
        }
        if let Some(sort) = patch.sort {
            self.sort = sort;
        }
    }
}

fn dedup(list: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    list.into_iter().filter(|item| !item.trim().is_empty() && seen.insert(item.clone())).collect()
}

/// A tab of the Log.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LogTab {
    /// Its name; the first tab is the Log, the others are named after their filters when absent.
    pub title: Option<String>,
    /// What it shows.
    pub filters: LogFilters,
    /// The selected commit's id.
    pub selected: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogView {
    tabs: Vec<LogTab>,
    active: usize,
    favorites: Option<BTreeSet<String>>,
    revision: u64,
    user: Option<(Option<String>, Option<String>)>,
    recent_paths: Vec<Vec<String>>,
}

impl Default for LogView {
    fn default() -> Self {
        LogView {
            tabs: vec![LogTab::default()],
            active: 0,
            favorites: None,
            revision: 1,
            user: None,
            recent_paths: Vec::new(),
        }
    }
}

impl LogView {
    pub fn tabs(&self) -> &[LogTab] {
        &self.tabs
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn recent_paths(&self) -> &[Vec<String>] {
        &self.recent_paths
    }

    fn remember_paths(&mut self, paths: &[String]) {
        if paths.is_empty() {
            return;
        }
        self.recent_paths.retain(|known| known.as_slice() != paths);
        self.recent_paths.insert(0, paths.to_vec());
        self.recent_paths.truncate(RECENT_PATHS);
    }

    fn tab_mut(&mut self, tab: Option<usize>) -> Result<&mut LogTab, CommandError> {
        let index = tab.unwrap_or(self.active);
        self.revision += 1;
        self.tabs.get_mut(index).ok_or_else(|| CommandError::new(t!("vcs-error-no-log-tab")))
    }

    pub fn changed_favorites(&self) -> Option<Vec<String>> {
        self.favorites.as_ref().map(|favorites| favorites.iter().cloned().collect())
    }

    pub(super) fn renamed_branch(&mut self, name: &str, new_name: &str) {
        let mut favorites: BTreeSet<String> = self.favorites().into_iter().collect();
        if favorites.remove(name) {
            favorites.insert(new_name.to_owned());
            self.favorites = Some(favorites);
            self.revision += 1;
        }
    }

    fn favorites(&self) -> Vec<String> {
        match &self.favorites {
            Some(favorites) => favorites.iter().cloned().collect(),
            None => DEFAULT_FAVORITES.iter().map(|name| name.to_string()).collect(),
        }
    }

    fn info(&self) -> LogViewInfo {
        let (user_name, user_email) = self.user.clone().unwrap_or_default();
        LogViewInfo {
            tabs: self.tabs.clone(),
            active: self.active,
            favorites: self.favorites(),
            revision: self.revision,
            user_name,
            user_email,
            recent_paths: self.recent_paths.clone(),
        }
    }
}

/// The Log's state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LogViewInfo {
    /// The tabs: the Log first, then the ones opened with `+` (vcs.logTab.new).
    pub tabs: Vec<LogTab>,
    /// The index of the tab shown.
    pub active: usize,
    /// The favourite branches (★), local and remote, by name.
    pub favorites: Vec<String>,
    /// Raised by every change of this state.
    pub revision: u64,
    /// Who commits in this repository (git's `user.name`): "me" of the User filter, and whose
    /// commits the Log shows in bold.
    pub user_name: Option<String>,
    /// Their e-mail (`user.email`).
    pub user_email: Option<String>,
    /// The sets of paths the Paths filter was given, the latest first (its recent entries).
    pub recent_paths: Vec<Vec<String>>,
}

pub struct ViewLog;

impl Command for ViewLog {
    type Params = NoParams;
    type Output = LogViewInfo;
    const ID: &'static str = "vcs.log.view";
    const TITLE: &'static str = "Log Tabs";
    const DESCRIPTION: &'static str = "What the Git tool window's Log shows: its tabs with each one's filters (text or hash, regex, \
         match case, branches, users, dates, paths, order) and selected commit, the active tab, the favourite \
         branches and who commits here. vcs.log.list with `tab` lists a tab's commits.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<LogViewInfo, CommandError> {
        let repository = repository(session).ok();
        let view = &mut vcs(session).log_view;
        if view.user.is_none()
            && let Some(repository) = repository
        {
            view.user = Some(repository.user());
        }
        Ok(view.info())
    }
}

/// A date range: either end may be open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DateRange {
    /// From this time on, in seconds since 1970.
    pub since: Option<i64>,
    /// Up to this time, in seconds since 1970.
    pub until: Option<i64>,
}

/// Filters to change; the absent ones stay as they are.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterPatch {
    /// Clear every filter first (the order stays).
    pub reset: bool,
    /// Text or hash; "" clears it.
    pub text: Option<String>,
    /// The text is a regular expression.
    pub regex: Option<bool>,
    /// The text matches in the same case only.
    pub match_case: Option<bool>,
    /// Only these branches' history; [] for every branch.
    pub branches: Option<Vec<String>>,
    /// Leave out these revisions' history; [] for none.
    pub exclude: Option<Vec<String>>,
    /// Only these authors' commits; [] for everyone's.
    pub users: Option<Vec<String>>,
    /// Only commits in this date range; {} for any date.
    pub date: Option<DateRange>,
    /// Only commits that changed these files or folders (absolute or relative); [] for any.
    pub paths: Option<Vec<PathBuf>>,
    /// The order: `date` or `topological`.
    pub sort: Option<LogSort>,
}

/// Which tab to filter, and how.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterParams {
    /// The tab; the active one when absent.
    pub tab: Option<usize>,
    /// The filters to change.
    #[serde(flatten)]
    pub filters: FilterPatch,
}

pub struct FilterLog;

impl Command for FilterLog {
    type Params = FilterParams;
    type Output = LogTab;
    const ID: &'static str = "vcs.log.filter";
    const TITLE: &'static str = "Filter Log";
    const DESCRIPTION: &'static str = "Changes the filters of a Log tab (the active one by default): text or hash (`regex`, \
         `matchCase`), branches, users, a date range, paths and the order (`sort`); the filters not given stay, \
         `reset` clears them all first. Answers with the tab.";

    fn run(&self, session: &mut Session, params: FilterParams) -> Result<LogTab, CommandError> {
        let root = repository(session).ok().map(|repository| repository.root().to_owned());
        let paths_given = params.filters.paths.is_some();
        let view = &mut vcs(session).log_view;
        let tab = view.tab_mut(params.tab)?;
        tab.filters.patch(params.filters, root.as_deref());
        let tab = tab.clone();
        if paths_given {
            view.remember_paths(&tab.filters.paths);
        }
        Ok(tab)
    }
}

/// A new Log tab.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NewTabParams {
    /// Its name; named after its filters when absent.
    pub title: Option<String>,
    /// Its filters; none when absent.
    #[serde(flatten)]
    pub filters: FilterPatch,
}

pub struct NewTab;

impl Command for NewTab {
    type Params = NewTabParams;
    type Output = LogViewInfo;
    const ID: &'static str = "vcs.logTab.new";
    const TITLE: &'static str = "New Log Tab";
    const DESCRIPTION: &'static str = "Opens a new tab in the Git tool window's Log with the given filters (as vcs.log.filter \
         takes them) and shows it; Compare with Current opens one with a branch and `exclude: [\"HEAD\"]`.";

    fn run(&self, session: &mut Session, params: NewTabParams) -> Result<LogViewInfo, CommandError> {
        let root = repository(session).ok().map(|repository| repository.root().to_owned());
        let view = &mut vcs(session).log_view;
        let mut tab = LogTab { title: params.title.filter(|title| !title.trim().is_empty()), ..LogTab::default() };
        tab.filters.patch(params.filters, root.as_deref());
        view.remember_paths(&tab.filters.paths);
        view.tabs.push(tab);
        view.active = view.tabs.len() - 1;
        view.revision += 1;
        Ok(view.info())
    }
}

/// A Log tab.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabParams {
    /// Its index in vcs.log.view's `tabs`.
    pub index: usize,
}

pub struct CloseTab;

impl Command for CloseTab {
    type Params = TabParams;
    type Output = LogViewInfo;
    const ID: &'static str = "vcs.logTab.close";
    const TITLE: &'static str = "Close Log Tab";
    const DESCRIPTION: &'static str = "Closes a tab of the Git tool window's Log opened with `+`; the first tab, the Log, cannot be \
         closed.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<LogViewInfo, CommandError> {
        let view = &mut vcs(session).log_view;
        if params.index == 0 || params.index >= view.tabs.len() {
            return Err(CommandError::new(t!("vcs-error-no-log-tab")));
        }
        view.tabs.remove(params.index);
        if view.active >= params.index {
            view.active = view.active.saturating_sub(1);
        }
        view.revision += 1;
        Ok(view.info())
    }
}

pub struct SelectTab;

impl Command for SelectTab {
    type Params = TabParams;
    type Output = LogViewInfo;
    const ID: &'static str = "vcs.logTab.select";
    const TITLE: &'static str = "Select Log Tab";
    const DESCRIPTION: &'static str = "Shows a tab of the Git tool window's Log (the tab dropdown).";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<LogViewInfo, CommandError> {
        let view = &mut vcs(session).log_view;
        if params.index >= view.tabs.len() {
            return Err(CommandError::new(t!("vcs-error-no-log-tab")));
        }
        view.active = params.index;
        view.revision += 1;
        Ok(view.info())
    }
}

/// A commit to select.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectCommitParams {
    /// The commit: an id, a branch, a tag or another revision (Go to Hash/Branch/Tag).
    pub id: String,
    /// The tab; the active one when absent.
    pub tab: Option<usize>,
}

pub struct SelectCommit;

impl Command for SelectCommit {
    type Params = SelectCommitParams;
    type Output = LogTab;
    const ID: &'static str = "vcs.log.select";
    const TITLE: &'static str = "Go to Hash/Branch/Tag";
    const DESCRIPTION: &'static str = "Selects a commit in a Log tab (the active one by default) by its id, a branch, a tag or any \
         revision, and shows its details; vcs.commit.details reads them.";

    fn run(&self, session: &mut Session, params: SelectCommitParams) -> Result<LogTab, CommandError> {
        let repository = repository(session)?;
        let id = params.id.trim();
        if id.is_empty() {
            return Err(CommandError::new(t!("vcs-error-revision")));
        }
        let (commit, _) = repository.commit_details(id).map_err(|error| CommandError::new(super::message(&error)))?;
        let tab = vcs(session).log_view.tab_mut(params.tab)?;
        tab.selected = Some(commit.id);
        Ok(tab.clone())
    }
}

/// A branch to mark or unmark.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FavoriteParams {
    /// The branch: `main`, `origin/main`.
    pub name: String,
    /// Mark (true) or unmark (false); toggles when absent.
    #[serde(default)]
    pub favorite: Option<bool>,
}

/// Whether a branch is a favourite.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FavoriteState {
    /// The branch.
    pub name: String,
    /// Whether it is a favourite now.
    pub favorite: bool,
}

pub struct FavoriteBranch;

impl Command for FavoriteBranch {
    type Params = FavoriteParams;
    type Output = FavoriteState;
    const ID: &'static str = "vcs.branch.favorite";
    const TITLE: &'static str = "Mark as Favorite";
    const DESCRIPTION: &'static str = "Marks a branch as a favourite (★ in the branches panel, listed first in the Branch filter), \
         unmarks it, or toggles it when `favorite` is absent. main and master are favourites until unmarked.";

    fn run(&self, session: &mut Session, params: FavoriteParams) -> Result<FavoriteState, CommandError> {
        let view = &mut vcs(session).log_view;
        let mut favorites: BTreeSet<String> = view.favorites().into_iter().collect();
        let favorite = params.favorite.unwrap_or(!favorites.contains(&params.name));
        if favorite {
            favorites.insert(params.name.clone());
        } else {
            favorites.remove(&params.name);
        }
        view.favorites = Some(favorites);
        view.revision += 1;
        Ok(FavoriteState { name: params.name, favorite })
    }
}

/// The favourite branches to set.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FavoritesParams {
    /// Replace the favourites with these; only read them when absent.
    pub names: Option<Vec<String>>,
}

pub struct Favorites;

impl Command for Favorites {
    type Params = FavoritesParams;
    type Output = Vec<String>;
    const ID: &'static str = "vcs.branch.favorites";
    const TITLE: &'static str = "Favorite Branches";
    const DESCRIPTION: &'static str =
        "The favourite branches by name; `names` replaces them all (the IDE restores the remembered ones this way).";

    fn run(&self, session: &mut Session, params: FavoritesParams) -> Result<Vec<String>, CommandError> {
        let view = &mut vcs(session).log_view;
        if let Some(names) = params.names {
            view.favorites = Some(names.into_iter().filter(|name| !name.trim().is_empty()).collect());
            view.revision += 1;
        }
        Ok(view.favorites())
    }
}

/// The recent paths to set.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RecentPathsParams {
    /// Replace the recent sets of paths with these, the latest first; only read them when absent.
    pub paths: Option<Vec<Vec<String>>>,
}

pub struct RecentPaths;

impl Command for RecentPaths {
    type Params = RecentPathsParams;
    type Output = Vec<Vec<String>>;
    const ID: &'static str = "vcs.log.recentPaths";
    const TITLE: &'static str = "Recent Log Paths";
    const DESCRIPTION: &'static str = "The sets of paths the Log's Paths filter was given, the latest first (at most 5; \
         vcs.log.filter with `paths` records them); `paths` replaces them all (the IDE restores the remembered ones \
         this way).";

    fn run(&self, session: &mut Session, params: RecentPathsParams) -> Result<Vec<Vec<String>>, CommandError> {
        let view = &mut vcs(session).log_view;
        if let Some(sets) = params.paths {
            view.recent_paths.clear();
            for set in sets.into_iter().rev() {
                view.remember_paths(&dedup(set));
            }
            view.revision += 1;
        }
        Ok(view.recent_paths.clone())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;

    #[test]
    fn tabs_filters_selection_and_favourites() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        git(&root, &["switch", "-q", "-c", "feature/x"]);
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/b.md"), "b\n").unwrap();
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-q", "-m", "Add docs"]);
        git(&root, &["switch", "-q", "main"]);
        let (registry, mut session) = super::super::tests::session(&root);
        let mut run = |command: &str, params: serde_json::Value| registry.dispatch(&mut session, command, params);

        let view = run("vcs.log.view", json!({})).unwrap();
        assert_eq!(view["tabs"].as_array().unwrap().len(), 1, "the Log");
        assert_eq!(view["favorites"], json!(["main", "master", "origin/main", "origin/master"]));

        let tab = run("vcs.log.filter", json!({ "text": "add docs", "matchCase": true })).unwrap();
        assert_eq!(tab["filters"]["text"], "add docs");
        let listed = run("vcs.log.list", json!({ "tab": 0 })).unwrap();
        assert_eq!(listed["commits"].as_array().unwrap().len(), 0, "\"add docs\" is not \"Add docs\"");
        run("vcs.log.filter", json!({ "matchCase": false, "paths": [root.join("docs")] })).unwrap();
        let listed = run("vcs.log.list", json!({ "tab": 0 })).unwrap();
        assert_eq!(listed["commits"][0]["subject"], "Add docs");
        assert_eq!(listed["commits"][0]["labels"], json!([{ "name": "feature/x", "kind": "local" }]));
        let view = run("vcs.log.view", json!({})).unwrap();
        assert_eq!(view["tabs"][0]["filters"]["paths"], json!(["docs"]), "kept relative");
        let tab = run("vcs.log.filter", json!({ "reset": true, "regex": true, "text": "^F" })).unwrap();
        assert_eq!(tab["filters"]["paths"], json!([]));
        let listed = run("vcs.log.list", json!({ "tab": 0 })).unwrap();
        assert_eq!(listed["commits"][0]["subject"], "First");
        let invalid =
            run("vcs.log.filter", json!({ "text": "(" })).and_then(|_| run("vcs.log.list", json!({ "tab": 0 })));
        assert!(invalid.unwrap()["error"].is_string(), "an invalid expression is reported, not listed");

        let view = run("vcs.logTab.new", json!({ "branches": ["feature/x"], "exclude": ["HEAD"] })).unwrap();
        assert_eq!(view["active"], 1);
        let listed = run("vcs.log.list", json!({ "tab": 1 })).unwrap();
        let subjects: Vec<&str> =
            listed["commits"].as_array().unwrap().iter().map(|c| c["subject"].as_str().unwrap()).collect();
        assert_eq!(subjects, ["Add docs"]);
        let tab = run("vcs.log.select", json!({ "id": "feature/x" })).unwrap();
        assert_eq!(tab["selected"], listed["commits"][0]["id"]);
        assert!(run("vcs.logTab.close", json!({ "index": 0 })).is_err(), "the Log stays");
        let view = run("vcs.logTab.close", json!({ "index": 1 })).unwrap();
        assert_eq!((view["active"].as_u64(), view["tabs"].as_array().unwrap().len()), (Some(0), 1));
        assert!(run("vcs.logTab.select", json!({ "index": 3 })).is_err());

        let state = run("vcs.branch.favorite", json!({ "name": "main" })).unwrap();
        assert_eq!(state["favorite"], false);
        run("vcs.branch.favorite", json!({ "name": "feature/x", "favorite": true })).unwrap();
        let favorites = run("vcs.branch.favorites", json!({})).unwrap();
        assert_eq!(favorites, json!(["feature/x", "master", "origin/main", "origin/master"]));
        let favorites = run("vcs.branch.favorites", json!({ "names": ["main"] })).unwrap();
        assert_eq!(favorites, json!(["main"]));

        let details = run("vcs.commit.details", json!({ "id": "main" })).unwrap();
        assert_eq!(
            details["containedIn"],
            json!([{ "name": "feature/x", "kind": "local" }, { "name": "main", "kind": "local" }])
        );
    }

    #[test]
    fn the_paths_filter_remembers_its_recent_paths() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        let mut run = |command: &str, params: serde_json::Value| registry.dispatch(&mut session, command, params);

        run("vcs.log.filter", json!({ "paths": ["docs"] })).unwrap();
        run("vcs.log.filter", json!({ "paths": [root.join("a.txt"), "docs"] })).unwrap();
        run("vcs.log.filter", json!({ "paths": [] })).unwrap();
        run("vcs.log.filter", json!({ "paths": ["docs"] })).unwrap();
        run("vcs.log.filter", json!({ "text": "x" })).unwrap();
        let view = run("vcs.log.view", json!({})).unwrap();
        assert_eq!(view["recentPaths"], json!([["docs"], ["a.txt", "docs"]]));
        run("vcs.logTab.new", json!({ "paths": ["src"] })).unwrap();
        for name in ["b", "c", "d", "e"] {
            run("vcs.log.filter", json!({ "paths": [name] })).unwrap();
        }
        let recent = run("vcs.log.recentPaths", json!({})).unwrap();
        assert_eq!(recent, json!([["e"], ["d"], ["c"], ["b"], ["src"]]));
        let recent = run("vcs.log.recentPaths", json!({ "paths": [["x", "x", ""], ["y"]] })).unwrap();
        assert_eq!(recent, json!([["x"], ["y"]]));
        assert_eq!(run("vcs.log.view", json!({})).unwrap()["recentPaths"], recent);
    }
}

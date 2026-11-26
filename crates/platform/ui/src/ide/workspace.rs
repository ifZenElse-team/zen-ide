use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zen_core::{Registry, Session};
use zen_run::Configuration;
use zen_workbench::Workbench;
use zen_workbench::bookmarks::{BookmarkInfo, Bookmarks};
use zen_workbench::debug::{BreakpointInfo, Breakpoints, Debugger, ExceptionBreakpointInfo, FunctionBreakpointInfo};
use zen_workbench::run::Runs;
use zen_workbench::run_anything::AnythingHistory;
use zen_workbench::run_services::RunServices;
use zen_workbench::search_scopes::{ScopeSelection, SearchScopes};
use zen_workbench::terminal::{SavedLayout, SavedTab, Terminals};
use zen_workbench::vcs::graph_search::GraphFocus;
use zen_workbench::vcs::workspace::GitWorkspace;
use zen_workbench::vcs::workspace_columns::ColumnSettings;
use zen_workbench::vcs::workspace_pull::PullMode;

use super::IdeView;
use super::inspect_window::InspectOptions;
use super::todo_window::TodoOptions;
use super::tool_windows::ToolWindowLayout;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ProjectLayout {
    pub(super) documents: Vec<PathBuf>,
    pub(super) active: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_splits")]
    pub(super) splits: Option<zen_workbench::groups::EditorGroups>,
    #[serde(skip_serializing_if = "Vec::is_empty", deserialize_with = "lenient_list")]
    pub(super) detached_windows: Vec<zen_workbench::groups::DetachedWindow>,
    pub(super) expanded: Vec<PathBuf>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) hide_excluded_files: bool,
    pub(super) project_panel: bool,
    pub(super) structure_panel: bool,
    pub(super) structure: StructureOptions,
    pub(super) project_view: ProjectViewOptions,
    pub(super) hierarchy: HierarchyOptions,
    #[serde(deserialize_with = "lenient_default")]
    pub(super) todo: TodoOptions,
    #[serde(deserialize_with = "lenient_default")]
    pub(super) inspect: InspectOptions,
    pub(super) terminal_open: bool,
    pub(super) terminal_tabs: usize,
    pub(super) terminal: Vec<SavedTab>,
    pub(super) recent_files: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty", deserialize_with = "lenient_list")]
    pub(super) closed_tabs: Vec<zen_workbench::tabs::ClosedTab>,
    #[serde(skip_serializing_if = "Vec::is_empty", deserialize_with = "lenient_list")]
    pub(super) folds: Vec<zen_workbench::folding::FileFolds>,
    pub(super) run: RunLayout,
    pub(super) debug: DebugLayout,
    #[serde(skip_serializing_if = "BookmarksLayout::is_empty", deserialize_with = "lenient_bookmarks")]
    pub(super) bookmarks: BookmarksLayout,
    pub(super) git: GitLayout,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub(super) tool_windows: Option<ToolWindowLayout>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub(super) search_scope: Option<ScopeSelection>,
    #[serde(skip_serializing_if = "Vec::is_empty", deserialize_with = "zen_workbench::workspaces::lenient_entries")]
    pub(super) workspaces: Vec<zen_workbench::workspaces::SavedWorkspace>,
}

fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

fn lenient_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    Ok(lenient(deserializer)?.unwrap_or_default())
}

fn lenient_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    Ok(lenient(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct GitLayout {
    pub(super) favorites: Option<Vec<String>>,
    pub(super) branches_hidden: bool,
    pub(super) flat_branches: bool,
    pub(super) flat_changes: bool,
    pub(super) tabs: Vec<zen_workbench::vcs::log_view::LogTab>,
    pub(super) active_tab: usize,
    pub(super) recent_paths: Vec<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) commit_message_height: Option<f32>,
}

impl GitLayout {
    fn log(vcs: &zen_workbench::vcs::Vcs) -> (Vec<zen_workbench::vcs::log_view::LogTab>, usize, Vec<Vec<String>>) {
        let view = vcs.log_view();
        let tabs = view
            .tabs()
            .iter()
            .map(|tab| zen_workbench::vcs::log_view::LogTab { selected: None, ..tab.clone() })
            .collect();
        (tabs, view.active(), view.recent_paths().to_vec())
    }
}

pub(super) fn filter_params(filters: &zen_workbench::vcs::log_view::LogFilters) -> serde_json::Value {
    serde_json::json!({
        "reset": true,
        "text": filters.text,
        "regex": filters.regex,
        "matchCase": filters.match_case,
        "branches": filters.branches,
        "exclude": filters.exclude,
        "users": filters.users,
        "date": { "since": filters.since, "until": filters.until },
        "paths": filters.paths,
        "sort": filters.sort,
    })
}

fn restore_log(registry: &Registry, session: &mut Session, git: &GitLayout) {
    for (index, tab) in git.tabs.iter().enumerate() {
        let mut params = filter_params(&tab.filters);
        if index == 0 {
            params["tab"] = serde_json::json!(0);
            let _ = registry.dispatch(session, "vcs.log.filter", params);
        } else {
            params["title"] = serde_json::json!(tab.title);
            let _ = registry.dispatch(session, "vcs.logTab.new", params);
        }
    }
    if !git.tabs.is_empty() {
        let index = if git.active_tab < git.tabs.len() { git.active_tab } else { 0 };
        let _ = registry.dispatch(session, "vcs.logTab.select", serde_json::json!({ "index": index }));
    }
    let _ = registry.dispatch(session, "vcs.log.recentPaths", serde_json::json!({ "paths": git.recent_paths }));
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct DebugLayout {
    pub(super) breakpoints: Vec<BreakpointInfo>,
    pub(super) watches: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) functions: Vec<FunctionBreakpointInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) exceptions: Vec<ExceptionBreakpointInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) exception_filters: Vec<zen_workbench::debug::ExceptionBreakpointsFilter>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) expressions: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct BookmarksLayout {
    pub(super) lines: Vec<BookmarkInfo>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) files: Vec<FileBookmark>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) lists: Vec<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) replace_without_asking: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub(super) group_by_file: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FileBookmark {
    #[serde(flatten)]
    pub(super) bookmark: BookmarkInfo,
    pub(super) index: usize,
}

impl BookmarksLayout {
    fn is_empty(&self) -> bool {
        self.lines.is_empty()
            && self.files.is_empty()
            && self.lists.is_empty()
            && !self.replace_without_asking
            && !self.group_by_file
    }

    fn of(saved: Vec<BookmarkInfo>) -> (Vec<BookmarkInfo>, Vec<FileBookmark>) {
        let mut placed: Vec<(Option<String>, usize)> = Vec::new();
        let (mut lines, mut files) = (Vec::new(), Vec::new());
        for bookmark in saved {
            let index = match placed.iter_mut().find(|(list, _)| *list == bookmark.list) {
                Some((_, count)) => {
                    *count += 1;
                    *count - 1
                }
                None => {
                    placed.push((bookmark.list.clone(), 1));
                    0
                }
            };
            if bookmark.line.is_some() {
                lines.push(bookmark);
            } else {
                files.push(FileBookmark { bookmark, index });
            }
        }
        (lines, files)
    }

    fn saved(&self) -> Vec<BookmarkInfo> {
        fn list_in<'a>(
            lists: &'a mut Vec<(Option<String>, Vec<BookmarkInfo>)>,
            name: &Option<String>,
        ) -> &'a mut Vec<BookmarkInfo> {
            let at = match lists.iter().position(|(list, _)| list == name) {
                Some(at) => at,
                None => {
                    lists.push((name.clone(), Vec::new()));
                    lists.len() - 1
                }
            };
            &mut lists[at].1
        }
        let mut lists: Vec<(Option<String>, Vec<BookmarkInfo>)> = Vec::new();
        for bookmark in self.lines.iter().filter(|bookmark| bookmark.line.is_some()) {
            list_in(&mut lists, &bookmark.list).push(bookmark.clone());
        }
        let mut files: Vec<&FileBookmark> = self.files.iter().filter(|file| file.bookmark.line.is_none()).collect();
        files.sort_by_key(|file| file.index);
        for file in files {
            let bookmarks = list_in(&mut lists, &file.bookmark.list);
            bookmarks.insert(file.index.min(bookmarks.len()), file.bookmark.clone());
        }
        lists.into_iter().flat_map(|(_, bookmarks)| bookmarks).collect()
    }
}

fn lenient_bookmarks<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<BookmarksLayout, D::Error> {
    Ok(lenient(deserializer)?.unwrap_or_default())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct RunLayout {
    pub(super) selected: Option<String>,
    pub(super) temporary: Vec<Configuration>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) anything: Vec<String>,
    #[serde(skip_serializing_if = "RunServices::is_empty")]
    pub(super) services: RunServices,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct StructureOptions {
    pub(super) sort_alphabetically: bool,
    pub(super) show_fields: bool,
    pub(super) single_click: bool,
    pub(super) follow_caret: bool,
}

impl Default for StructureOptions {
    fn default() -> Self {
        Self { sort_alphabetically: false, show_fields: true, single_click: true, follow_caret: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ProjectViewOptions {
    pub(super) always_select_opened: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct HierarchyOptions {
    pub(super) scope: zen_workbench::language::hierarchy::HierarchyScope,
    pub(super) sort_alphabetically: bool,
    pub(super) single_click: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct WindowLayout {
    pub(super) fullscreen: bool,
    pub(super) maximized: bool,
    pub(super) position: Option<[f32; 2]>,
    pub(super) size: Option<[f32; 2]>,
    pub(super) screen: Option<[f32; 2]>,
}

fn lenient_splits<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<zen_workbench::groups::EditorGroups>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct RepositoryLayout {
    pub(super) graph_focus: GraphFocus,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub(super) columns: Option<ColumnSettings>,
    #[serde(deserialize_with = "lenient", skip_serializing_if = "Option::is_none")]
    pub(super) pull: Option<PullMode>,
}

impl RepositoryLayout {
    fn is_empty(&self) -> bool {
        self.graph_focus.is_empty() && self.columns.is_none() && self.pull.is_none()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Workspace {
    pub(super) projects: BTreeMap<PathBuf, ProjectLayout>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) repositories: BTreeMap<PathBuf, RepositoryLayout>,
    pub(super) window: WindowLayout,
    pub(super) active_tab: Option<PathBuf>,
}

impl Workspace {
    pub(super) fn load(path: &Path) -> Self {
        std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    pub(super) fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = zen_vfs::write_atomic(path, &serde_json::to_vec_pretty(self).unwrap_or_default());
    }
}

fn main_documents(workbench: Option<&Workbench>) -> Vec<PathBuf> {
    let Some(workbench) = workbench else { return Vec::new() };
    let groups = workbench.groups().groups();
    workbench
        .documents()
        .iter()
        .map(|document| document.path().to_owned())
        .filter(|path| groups.iter().any(|group| group.tabs.contains(path)))
        .collect()
}

pub(super) fn capture(view: &IdeView, session: &mut Session, project: &Path) -> ProjectLayout {
    let debug = debug_layout(session);
    let bookmarks = bookmarks_layout(session);
    let workspaces = view.saved_workspaces(session);
    let workbench = session.state::<Workbench>();
    let terminal = session.state::<Terminals>().map(Terminals::save).unwrap_or_default();
    let tools = view.kept_layout();
    ProjectLayout {
        documents: main_documents(workbench),
        active: workbench.and_then(|workbench| {
            let groups = workbench.groups();
            let shown = groups.group(groups.main_focused())?.active_path()?;
            main_documents(Some(workbench)).iter().position(|path| path == shown)
        }),
        splits: workbench
            .map(Workbench::groups)
            .filter(|groups| groups.is_split() || groups.has_pinned())
            .map(|groups| groups.main_only()),
        detached_windows: workbench.map(|workbench| workbench.groups().windows().to_vec()).unwrap_or_default(),
        expanded: view.expanded.iter().filter(|path| path.as_path() != project).cloned().collect(),
        hide_excluded_files: !view.show_excluded,
        project_panel: tools.is_visible("project"),
        structure_panel: tools.is_visible("structure"),
        structure: view.structure_window.options,
        project_view: ProjectViewOptions { always_select_opened: view.always_select_opened },
        hierarchy: session
            .state::<zen_workbench::language::hierarchy::Hierarchies>()
            .map(|hierarchies| HierarchyOptions {
                scope: hierarchies.scope(),
                sort_alphabetically: hierarchies.sort(),
                single_click: hierarchies.single_click(),
            })
            .unwrap_or_default(),
        todo: view.todo_window.options.clone(),
        inspect: view.inspect_window.options.clone(),
        terminal_open: tools.is_visible("terminal"),
        terminal_tabs: terminal.len(),
        terminal,
        recent_files: workbench.map(|workbench| workbench.recent_files().to_vec()).unwrap_or_default(),
        closed_tabs: workbench.map(|workbench| workbench.closed_tabs().to_vec()).unwrap_or_default(),
        folds: workbench.map(Workbench::fold_states).unwrap_or_default(),
        run: RunLayout {
            anything: session.state::<AnythingHistory>().map(|history| history.entries().to_vec()).unwrap_or_default(),
            services: session.state::<RunServices>().cloned().unwrap_or_default(),
            ..session.state::<Runs>().map(run_layout).unwrap_or_default()
        },
        debug,
        bookmarks,
        git: {
            let vcs = session.state::<zen_workbench::vcs::Vcs>();
            let (tabs, active_tab, recent_paths) = vcs.map(GitLayout::log).unwrap_or_default();
            GitLayout {
                favorites: vcs.and_then(zen_workbench::vcs::Vcs::changed_favorites),
                tabs,
                active_tab,
                recent_paths,
                commit_message_height: view.commit_window.message_height,
                ..view.git_window.layout()
            }
        },
        tool_windows: Some(ToolWindowLayout { active: None, ..tools }),
        search_scope: search_scope(session),
        workspaces,
    }
}

fn restored_active_tab(session: &Session, layout: &ProjectLayout) -> Option<usize> {
    let groups = session.state::<Workbench>()?.groups();
    let tabs = &groups.group(groups.focused())?.tabs;
    let active = layout.active.filter(|active| *active < layout.documents.len())?;
    let (before, from_active) = layout.documents.split_at(active);
    from_active.iter().chain(before.iter().rev()).find_map(|path| tabs.iter().position(|tab| tab == path))
}

pub(super) fn restore(view: &mut IdeView, registry: &Registry, session: &mut Session, layout: &ProjectLayout) {
    restore_folds(registry, session, &layout.folds);
    for document in layout.documents.iter().filter(|path| path.is_file()) {
        let params = serde_json::json!({ "path": document, "editorOnly": true });
        let _ = registry.dispatch(session, "file.document.open", params);
    }
    if let Some(index) = restored_active_tab(session, layout) {
        let _ = registry.dispatch(session, "editor.tab.select", serde_json::json!({ "index": index }));
    }
    if let Some(splits) = &layout.splits {
        session.state_mut::<Workbench>().restore_groups(splits.clone());
    }
    view.restore_workspaces(session, &layout.workspaces);
    session.state_mut::<Workbench>().restore_closed_tabs(layout.closed_tabs.clone());
    for window in &layout.detached_windows {
        for document in window.group.tabs.iter().filter(|path| path.is_file()) {
            let params = serde_json::json!({ "path": document, "editorOnly": true });
            let _ = registry.dispatch(session, "file.document.open", params);
        }
    }
    session.state_mut::<Workbench>().restore_windows(layout.detached_windows.clone());
    let _ = registry.dispatch(session, "navigate.history.clear", serde_json::Value::Null);
    let recent: Vec<&PathBuf> = layout.recent_files.iter().filter(|path| path.is_file()).collect();
    let _ = registry.dispatch(session, "navigate.recent.restore", serde_json::json!({ "paths": recent }));
    view.expanded
        .extend(layout.expanded.iter().filter(|path| path.is_dir() || super::libraries_tree::is_group(path)).cloned());
    view.structure_window.options = layout.structure;
    view.set_always_select_opened(layout.project_view.always_select_opened);
    let hierarchy = layout.hierarchy;
    let _ = registry.dispatch(
        session,
        "navigate.hierarchy.setOptions",
        serde_json::json!({
            "scope": hierarchy.scope,
            "sort": hierarchy.sort_alphabetically,
            "singleClick": hierarchy.single_click,
        }),
    );
    view.todo_window.options = layout.todo.clone();
    view.inspect_window.options = layout.inspect.clone();
    view.show_excluded = !layout.hide_excluded_files;
    view.tool_windows = match &layout.tool_windows {
        Some(saved) => ToolWindowLayout::reopened(saved.clone()),
        None => {
            let left = if layout.structure_panel { "structure" } else { "project" };
            let left = (layout.structure_panel || layout.project_panel).then_some(left);
            let terminal = layout.terminal_open.then_some("terminal");
            ToolWindowLayout::from_shown(&[left, terminal].into_iter().flatten().collect::<Vec<_>>())
        }
    };
    view.sync_tool_flags();
    let tabs = if layout.terminal.is_empty() {
        let root = zen_workbench::terminal::default_cwd(session);
        let tab = SavedTab { name: None, layout: SavedLayout::Pane { cwd: root, session: None, terminal: None } };
        vec![tab; layout.terminal_tabs]
    } else {
        layout.terminal.clone()
    };
    session.state_mut::<Terminals>().restore(tabs);
    restore_run(session, &layout.run);
    view.run_widget.invalidate();
    restore_debug(registry, session, &layout.debug);
    restore_bookmarks(session, &layout.bookmarks);
    view.git_window.restore(&layout.git);
    view.commit_window.message_height = layout.git.commit_message_height;
    if let Some(favorites) = &layout.git.favorites {
        let _ = registry.dispatch(session, "vcs.branch.favorites", serde_json::json!({ "names": favorites }));
    }
    restore_log(registry, session, &layout.git);
    restore_search_scope(registry, session, layout.search_scope.as_ref());
}

pub(super) fn search_scope(session: &Session) -> Option<ScopeSelection> {
    session
        .state::<SearchScopes>()
        .map(|scopes| scopes.selection().clone())
        .filter(|selection| *selection != ScopeSelection::default())
}

pub(super) fn restore_search_scope(registry: &Registry, session: &mut Session, selection: Option<&ScopeSelection>) {
    if let Some(selection) = selection {
        let _ = registry.dispatch(session, "search.scope.select", serde_json::json!(selection));
    }
}

fn restore_folds(registry: &Registry, session: &mut Session, folds: &[zen_workbench::folding::FileFolds]) {
    let files: Vec<&zen_workbench::folding::FileFolds> = folds.iter().filter(|file| file.path.is_file()).collect();
    if !files.is_empty() {
        let _ = registry.dispatch(session, "editor.fold.remember", serde_json::json!({ "files": files }));
    }
}

pub(super) fn capture_repository(session: &Session, repositories: &mut BTreeMap<PathBuf, RepositoryLayout>) {
    let Some((root, focus)) = session.state::<zen_workbench::vcs::Vcs>().and_then(zen_workbench::vcs::Vcs::graph_focus)
    else {
        return;
    };
    let workspace = session.state::<GitWorkspace>();
    let columns = workspace.map(|workspace| workspace.view().columns.clone()).filter(|columns| !columns.is_default());
    let pull = workspace.map(GitWorkspace::pull_mode).filter(|mode| *mode != PullMode::default());
    let layout = RepositoryLayout { graph_focus: focus, columns, pull };
    if layout.is_empty() {
        repositories.remove(&root);
    } else {
        repositories.insert(root, layout);
    }
}

pub(super) fn restore_repository(
    registry: &Registry,
    session: &mut Session,
    repositories: &BTreeMap<PathBuf, RepositoryLayout>,
) {
    if repositories.is_empty() {
        return;
    }
    let info = registry.dispatch(session, "vcs.repository.info", serde_json::json!({})).ok();
    let Some(root) = info.as_ref().and_then(|info| info["root"].as_str()).map(PathBuf::from) else { return };
    let Some(layout) = repositories.get(&root) else { return };
    if !layout.graph_focus.is_empty() {
        let set = serde_json::json!({ "set": layout.graph_focus });
        let _ = registry.dispatch(session, "vcs.graph.focus", set);
    }
    if let Some(columns) = &layout.columns {
        let _ = registry.dispatch(session, "vcs.workspaceColumns.set", serde_json::json!({ "set": columns }));
    }
    if let Some(mode) = layout.pull {
        let _ = registry.dispatch(session, "vcs.workspace.pullDefault", serde_json::json!({ "mode": mode }));
    }
}

pub(super) fn debug_layout(session: &mut Session) -> DebugLayout {
    zen_workbench::debug::sync_soon(session);
    let breakpoints = session.state::<Breakpoints>().cloned().unwrap_or_default();
    DebugLayout {
        breakpoints: breakpoints.all(),
        watches: session.state::<Debugger>().map(|debugger| debugger.watches().to_vec()).unwrap_or_default(),
        functions: breakpoints.saved_functions(),
        exceptions: breakpoints.exception_rows().to_vec(),
        exception_filters: breakpoints.known_filters().to_vec(),
        expressions: session.state::<Debugger>().map(|debugger| debugger.history().to_vec()).unwrap_or_default(),
    }
}

pub(super) fn restore_debug(registry: &Registry, session: &mut Session, debug: &DebugLayout) {
    let kept = debug.breakpoints.iter().filter(|breakpoint| breakpoint.path.is_file()).cloned().collect();
    let breakpoints = session.state_mut::<Breakpoints>();
    breakpoints.restore(kept);
    breakpoints.restore_functions(debug.functions.clone());
    breakpoints.restore_exceptions(debug.exceptions.clone(), debug.exception_filters.clone());
    zen_workbench::debug::sync(session);
    for expression in &debug.watches {
        let _ = registry.dispatch(session, "debug.watch.add", serde_json::json!({ "expression": expression }));
    }
    let limit = session
        .settings()
        .get_or(zen_workbench::debug::RECENT_EXPRESSIONS, zen_workbench::debug::RECENT_EXPRESSIONS_DEFAULT);
    session.state_mut::<Debugger>().set_history(debug.expressions.clone(), limit);
}

pub(super) fn bookmarks_layout(session: &mut Session) -> BookmarksLayout {
    zen_workbench::bookmarks::sync(session);
    let bookmarks = session.state::<Bookmarks>();
    let (lines, files) = BookmarksLayout::of(bookmarks.map(Bookmarks::saved).unwrap_or_default());
    BookmarksLayout {
        lines,
        files,
        lists: bookmarks.map(|bookmarks| bookmarks.lists().to_vec()).unwrap_or_default(),
        replace_without_asking: bookmarks.is_some_and(Bookmarks::replace_without_asking),
        group_by_file: bookmarks.is_some_and(Bookmarks::group_by_file),
    }
}

pub(super) fn restore_bookmarks(session: &mut Session, layout: &BookmarksLayout) {
    zen_workbench::bookmarks::restore(session, layout.saved(), layout.lists.clone());
    let bookmarks = session.state_mut::<Bookmarks>();
    bookmarks.set_replace_without_asking(layout.replace_without_asking);
    bookmarks.set_group_by_file(layout.group_by_file);
}

pub(super) fn run_layout(runs: &Runs) -> RunLayout {
    RunLayout {
        selected: runs.selected().map(str::to_owned),
        temporary: runs.temporary().to_vec(),
        anything: Vec::new(),
        services: RunServices::default(),
    }
}

pub(super) fn restore_run(session: &mut Session, run: &RunLayout) {
    let runs = session.state_mut::<Runs>();
    runs.set_selected(run.selected.clone());
    runs.set_temporary(run.temporary.clone());
    session.state_mut::<AnythingHistory>().set_entries(run.anything.clone());
    session.set_state(run.services.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("workspace.json");
        let mut workspace = Workspace::default();
        workspace.window.fullscreen = true;
        workspace.active_tab = Some(dir.path().to_owned());
        workspace.projects.insert(
            dir.path().to_owned(),
            ProjectLayout { terminal_open: true, terminal_tabs: 2, project_panel: true, ..ProjectLayout::default() },
        );
        workspace.save(&file);
        assert_eq!(Workspace::load(&file), workspace);
        assert_eq!(Workspace::load(&dir.path().join("missing.json")), Workspace::default());
    }

    #[test]
    fn the_folds_of_the_files_round_trip_through_the_workbench() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "projectPanel": true, "folds": 7 }"#).unwrap();
        assert!(layout.folds.is_empty() && layout.project_panel, "a broken list loses only itself");
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "package main\n\nfunc main() {\n\tprintln()\n}\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let open = |registry: &Registry| {
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
            session
        };
        let mut session = open(&registry);
        registry.dispatch(&mut session, "file.document.open", serde_json::json!({ "path": file })).unwrap();
        let folds = registry.dispatch(&mut session, "editor.fold.list", serde_json::json!({})).unwrap();
        let start = serde_json::json!({ "start": folds[0]["start"] });
        registry.dispatch(&mut session, "editor.fold.toggle", start).unwrap();
        let captured = session.state::<Workbench>().unwrap().fold_states();
        let gone = zen_workbench::folding::FileFolds {
            path: dir.path().join("gone.go"),
            folds: Vec::new(),
            manual: Vec::new(),
        };
        let layout = ProjectLayout { folds: [captured.clone(), vec![gone]].concat(), ..ProjectLayout::default() };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(json["folds"][0]["folds"][0], serde_json::json!({ "startLine": 2, "endLine": 4 }));
        let read: ProjectLayout = serde_json::from_value(json).unwrap();
        let mut restarted = open(&registry);
        restore_folds(&registry, &mut restarted, &read.folds);
        registry.dispatch(&mut restarted, "file.document.open", serde_json::json!({ "path": file })).unwrap();
        let restored = restarted.state::<Workbench>().unwrap().fold_states();
        assert_eq!(restored, captured, "the file opens folded as it was left; the file that is gone is dropped");
        let older: ProjectLayout = serde_json::from_value(serde_json::json!({
            "documents": [file],
            "folds": [{ "path": file, "folds": [{ "startLine": 2, "endLine": 4 }] }],
        }))
        .unwrap();
        assert_eq!(older.folds[0].manual, [], "a workspace saved before manual regions loads");
        let mut restarted = open(&registry);
        restore_folds(&registry, &mut restarted, &older.folds);
        registry.dispatch(&mut restarted, "file.document.open", serde_json::json!({ "path": file })).unwrap();
        assert_eq!(restarted.state::<Workbench>().unwrap().fold_states(), captured);
    }

    #[test]
    fn the_splits_are_optional_and_a_broken_tree_loses_only_them() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "documents": ["/a.txt"], "active": 0 }"#).unwrap();
        assert_eq!(layout.splits, None, "older files have no splits");
        assert!(serde_json::to_value(&layout).unwrap().get("splits").is_none(), "an editor not split writes none");
        let layout: ProjectLayout =
            serde_json::from_str(r#"{ "documents": ["/a.txt"], "projectPanel": true, "splits": { "root": 7 } }"#)
                .unwrap();
        assert_eq!(layout.splits, None);
        assert!(layout.project_panel, "the rest of the layout stays");
    }

    #[test]
    fn the_structure_panel_is_closed_in_older_files() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "projectPanel": true }"#).unwrap();
        assert!(layout.project_panel);
        assert!(!layout.structure_panel);
        let layout = ProjectLayout { structure_panel: true, ..ProjectLayout::default() };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(json["structurePanel"], true);
        assert_eq!(serde_json::from_value::<ProjectLayout>(json).unwrap(), layout);
    }

    #[test]
    fn the_structure_toggles_survive_and_default_in_older_files() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "projectPanel": true }"#).unwrap();
        assert_eq!(layout.structure, StructureOptions::default());
        assert!(layout.structure.show_fields && layout.structure.single_click && layout.structure.follow_caret);
        assert!(!layout.structure.sort_alphabetically);
        let layout: ProjectLayout = serde_json::from_str(r#"{ "structure": { "sortAlphabetically": true } }"#).unwrap();
        assert!(layout.structure.sort_alphabetically && layout.structure.show_fields);
        let options = StructureOptions { show_fields: false, follow_caret: false, ..StructureOptions::default() };
        let layout = ProjectLayout { structure: options, ..ProjectLayout::default() };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(json["structure"]["showFields"], false);
        assert_eq!(serde_json::from_value::<ProjectLayout>(json).unwrap(), layout);
    }

    #[test]
    fn always_select_opened_file_round_trips_and_is_off_in_older_files() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "projectPanel": true }"#).unwrap();
        assert!(!layout.project_view.always_select_opened);
        let layout = ProjectLayout {
            project_view: ProjectViewOptions { always_select_opened: true },
            ..ProjectLayout::default()
        };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(json["projectView"]["alwaysSelectOpened"], true);
        assert_eq!(serde_json::from_value::<ProjectLayout>(json).unwrap(), layout);
    }

    #[test]
    fn the_log_tabs_and_their_filters_round_trip() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "git": { "flatBranches": true } }"#).unwrap();
        assert!(layout.git.tabs.is_empty(), "older files have no tabs");
        let dir = tempfile::tempdir().unwrap();
        let session_with = |registry: &Registry| {
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
            session
        };
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let log = |session: &Session| GitLayout::log(session.state::<zen_workbench::vcs::Vcs>().unwrap());

        let mut session = session_with(&registry);
        let mut run =
            |command: &str, params: serde_json::Value| registry.dispatch(&mut session, command, params).unwrap();
        run(
            "vcs.log.filter",
            serde_json::json!({ "users": ["Ada"], "date": { "since": 86_400 }, "sort": "topological" }),
        );
        run("vcs.logTab.new", serde_json::json!({ "title": "Docs", "paths": ["docs"], "regex": true, "text": "^Fix" }));
        run("vcs.logTab.new", serde_json::json!({ "branches": ["main"], "exclude": ["HEAD"] }));
        run("vcs.logTab.select", serde_json::json!({ "index": 1 }));
        run("vcs.log.filter", serde_json::json!({ "tab": 2, "paths": ["cmd", "go.mod"] }));
        let (tabs, active_tab, recent_paths) = log(&session);
        let git = GitLayout { tabs, active_tab, recent_paths, ..GitLayout::default() };
        let json = serde_json::to_value(ProjectLayout { git: git.clone(), ..ProjectLayout::default() }).unwrap();
        assert_eq!(json["git"]["tabs"][1]["filters"]["paths"], serde_json::json!(["docs"]));
        let read: ProjectLayout = serde_json::from_value(json).unwrap();
        assert_eq!(read.git, git);

        let mut restored = session_with(&registry);
        restore_log(&registry, &mut restored, &read.git);
        assert_eq!(
            log(&restored),
            (git.tabs.clone(), 1, vec![vec!["cmd".to_owned(), "go.mod".to_owned()], vec!["docs".to_owned()]])
        );
    }

    #[test]
    fn bookmarks_saved_before_lists_had_an_order_keep_the_project_list_as_the_default() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.go");
        std::fs::write(&main, "package main\n\nfunc main() {}\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        let written_before = serde_json::json!({ "bookmarks": {
            "lines": [
                { "path": main, "line": 0, "list": "Zed" },
                { "path": main, "line": 1, "mnemonic": "4", "list": "Later" },
                { "path": main, "line": 2 },
            ],
            "replaceWithoutAsking": true,
        } });
        let layout: ProjectLayout = serde_json::from_value(written_before).unwrap();
        restore_bookmarks(&mut session, &layout.bookmarks);
        let listed = registry.dispatch(&mut session, "navigate.bookmark.list", serde_json::json!({})).unwrap();
        let project = dir.path().file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(listed["defaultList"], serde_json::json!(project));
        assert_eq!(listed["lists"], serde_json::json!(["Later", "Zed"]));
        assert_eq!(listed["bookmarks"][1]["mnemonic"], "4");
        assert_eq!(listed["replaceWithoutAsking"], true);
        let saved = serde_json::to_value(bookmarks_layout(&mut session)).unwrap();
        assert_eq!(saved["lines"][0], serde_json::json!({ "path": main, "line": 2 }), "the default list unnamed");
        assert_eq!(saved["lists"], serde_json::json!([project, "Later", "Zed"]));
    }

    #[test]
    fn file_bookmarks_keep_their_place_and_an_older_build_still_reads_the_line_bookmarks() {
        let dir = tempfile::tempdir().unwrap();
        let (main, folder) = (dir.path().join("main.go"), dir.path().join("pkg"));
        std::fs::write(&main, "package main\n\nfunc main() {}\n").unwrap();
        std::fs::create_dir(&folder).unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let run = |session: &mut Session, command: &str, params: serde_json::Value| {
            registry.dispatch(session, command, params).unwrap()
        };
        let mut session = Session::new();
        run(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() }));
        run(&mut session, "navigate.bookmark.toggle", serde_json::json!({ "path": main, "line": 1 }));
        run(&mut session, "navigate.bookmark.toggleFiles", serde_json::json!({ "paths": [folder] }));
        run(&mut session, "navigate.bookmark.toggle", serde_json::json!({ "path": main, "line": 3 }));
        let mnemonic = serde_json::json!({ "path": main, "whole": true, "mnemonic": "m", "description": "entry" });
        run(&mut session, "navigate.bookmark.setMnemonic", mnemonic);
        run(
            &mut session,
            "navigate.bookmark.moveToList",
            serde_json::json!({ "path": main, "whole": true, "list": "Later" }),
        );
        let json = serde_json::to_value(ProjectLayout {
            bookmarks: bookmarks_layout(&mut session),
            ..ProjectLayout::default()
        })
        .unwrap();
        assert_eq!(
            json["bookmarks"]["lines"],
            serde_json::json!([{ "path": main, "line": 0 }, { "path": main, "line": 2 }]),
            "only line bookmarks in lines"
        );
        assert_eq!(
            json["bookmarks"]["files"],
            serde_json::json!([
                { "path": folder, "folder": true, "index": 1 },
                { "path": main, "mnemonic": "M", "description": "entry", "list": "Later", "index": 0 },
            ])
        );

        #[derive(Deserialize)]
        struct OlderLine {
            line: u32,
        }
        #[derive(Default, Deserialize)]
        #[serde(default)]
        struct OlderBookmarks {
            lines: Vec<OlderLine>,
            lists: Vec<String>,
        }
        #[derive(Deserialize)]
        struct OlderLayout {
            #[serde(deserialize_with = "lenient_older")]
            bookmarks: OlderBookmarks,
        }
        fn lenient_older<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<OlderBookmarks, D::Error> {
            Ok(lenient(deserializer)?.unwrap_or_default())
        }
        let older: OlderLayout = serde_json::from_value(json.clone()).unwrap();
        let lines: Vec<u32> = older.bookmarks.lines.iter().map(|line| line.line).collect();
        assert_eq!(lines, [0, 2], "a build before file bookmarks keeps every line bookmark");
        assert_eq!(older.bookmarks.lists.len(), 2);

        let read: ProjectLayout = serde_json::from_value(json).unwrap();
        let mut restored = Session::new();
        run(&mut restored, "file.project.open", serde_json::json!({ "path": dir.path() }));
        restore_bookmarks(&mut restored, &read.bookmarks);
        let listed = run(&mut restored, "navigate.bookmark.list", serde_json::json!({}));
        let order = |listed: &serde_json::Value, at: usize| -> Vec<String> {
            listed["groups"][at]["bookmarks"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| format!("{}:{}", b["path"].as_str().unwrap().rsplit('/').next().unwrap(), b["line"]))
                .collect()
        };
        assert_eq!(order(&listed, 0), ["main.go:1", "pkg:null", "main.go:3"], "the folder between the lines");
        assert_eq!(order(&listed, 1), ["main.go:null"]);
        assert_eq!(listed["groups"][1]["bookmarks"][0]["mnemonic"], "M");
        std::fs::remove_dir(&folder).unwrap();
        let mut gone = Session::new();
        run(&mut gone, "file.project.open", serde_json::json!({ "path": dir.path() }));
        restore_bookmarks(&mut gone, &read.bookmarks);
        let listed = run(&mut gone, "navigate.bookmark.list", serde_json::json!({}));
        assert_eq!(order(&listed, 0), ["main.go:1", "main.go:3"], "a folder that is gone is left out");
    }

    #[test]
    fn the_bookmarks_round_trip_and_those_of_gone_files_are_dropped() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "bookmarks": 7 }"#).unwrap();
        assert_eq!(layout.bookmarks, BookmarksLayout::default(), "a broken entry loses only the bookmarks");
        assert!(serde_json::to_value(&layout).unwrap().get("bookmarks").is_none());
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main.go");
        std::fs::write(&main, "package main\n\nfunc main() {}\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        let run = |session: &mut Session, command: &str, params: serde_json::Value| {
            registry.dispatch(session, command, params).unwrap()
        };
        run(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() }));
        run(&mut session, "navigate.bookmark.toggle", serde_json::json!({ "path": main, "line": 1 }));
        let params = serde_json::json!({ "path": main, "line": 3, "mnemonic": "4", "description": "entry" });
        run(&mut session, "navigate.bookmark.setMnemonic", params);
        run(&mut session, "navigate.bookmark.options", serde_json::json!({ "replaceWithoutAsking": true }));
        run(&mut session, "navigate.bookmarkList.create", serde_json::json!({ "name": "Empty" }));
        run(&mut session, "navigate.bookmark.options", serde_json::json!({ "groupByFile": true }));
        let mut saved = bookmarks_layout(&mut session);
        let gone = BookmarkInfo { path: dir.path().join("gone.go"), ..saved.lines[0].clone() };
        saved.lines.push(gone);
        let json =
            serde_json::to_value(ProjectLayout { bookmarks: saved.clone(), ..ProjectLayout::default() }).unwrap();
        assert_eq!(json["bookmarks"]["lines"][1]["mnemonic"], "4");
        assert_eq!(json["bookmarks"]["replaceWithoutAsking"], true);
        assert_eq!(json["bookmarks"]["groupByFile"], true);
        let project = dir.path().file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(json["bookmarks"]["lists"], serde_json::json!([project, "Empty"]), "an empty list is kept too");
        let read: ProjectLayout = serde_json::from_value(json).unwrap();

        let mut restored = Session::new();
        run(&mut restored, "file.project.open", serde_json::json!({ "path": dir.path() }));
        restore_bookmarks(&mut restored, &read.bookmarks);
        let listed = run(&mut restored, "navigate.bookmark.list", serde_json::json!({}));
        assert_eq!(
            listed["bookmarks"],
            serde_json::json!([{ "path": main, "line": 1 }, { "path": main, "line": 3, "mnemonic": "4", "description": "entry" }])
        );
        assert_eq!(listed["replaceWithoutAsking"], true);
        assert_eq!(listed["groupByFile"], true);
        assert_eq!(listed["lists"], serde_json::json!(["Empty"]));
    }

    #[test]
    fn the_breakpoints_and_watches_round_trip_through_the_debugger() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "projectPanel": true }"#).unwrap();
        assert_eq!(layout.debug, DebugLayout::default(), "older files have no debugger state");
        let breakpoint: BreakpointInfo = serde_json::from_str(r#"{ "path": "/a.go", "line": 3 }"#).unwrap();
        assert!(breakpoint.enabled, "a breakpoint is enabled by default");

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "package main\n\n\n\n\n\n\n\n").unwrap();
        let at = |line: u32, enabled: bool, condition: Option<&str>| -> BreakpointInfo {
            let mut info = serde_json::json!({ "path": file, "line": line, "enabled": enabled });
            if let Some(condition) = condition {
                info["condition"] = serde_json::json!(condition);
            }
            serde_json::from_value(info).unwrap()
        };
        let gone = BreakpointInfo { path: dir.path().join("gone.go"), ..at(1, true, None) };
        let logging: BreakpointInfo = serde_json::from_value(serde_json::json!({
            "path": file, "line": 5, "suspend": "none", "logHit": true, "dependsOn": { "path": file, "line": 3 },
        }))
        .unwrap();
        let function: FunctionBreakpointInfo =
            serde_json::from_value(serde_json::json!({ "name": "main.run" })).unwrap();
        let exception: ExceptionBreakpointInfo =
            serde_json::from_value(serde_json::json!({ "filter": "uncaught", "label": "Uncaught" })).unwrap();
        let debug = DebugLayout {
            breakpoints: vec![at(3, true, None), gone, logging.clone(), at(7, false, Some("n > 1"))],
            watches: vec!["n".to_owned(), "items[0]".to_owned()],
            functions: vec![function],
            exceptions: vec![exception],
            exception_filters: Vec::new(),
            expressions: vec!["len(items)".to_owned(), "n".to_owned()],
        };
        let json = serde_json::to_value(ProjectLayout { debug: debug.clone(), ..ProjectLayout::default() }).unwrap();
        assert_eq!(json["debug"]["expressions"][0], "len(items)", "the recent expressions, the newest first");
        assert_eq!(json["debug"]["breakpoints"][3]["condition"], "n > 1");
        assert_eq!(json["debug"]["breakpoints"][2]["dependsOn"]["line"], 3);
        assert_eq!(json["debug"]["functions"][0]["name"], "main.run");
        assert!(json["debug"]["breakpoints"][0].get("verified").is_none(), "what the debugger says is not saved");
        assert_eq!(json["debug"]["watches"][1], "items[0]");
        assert_eq!(serde_json::from_value::<ProjectLayout>(json).unwrap().debug, debug);

        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
        assert_eq!(debug_layout(&mut session), DebugLayout::default());
        restore_debug(&registry, &mut session, &debug);
        let listed = registry.dispatch(&mut session, "debug.breakpoint.list", serde_json::json!({})).unwrap();
        assert_eq!(
            listed["breakpoints"].as_array().map(Vec::len),
            Some(3),
            "the breakpoint of a file that is gone is dropped"
        );
        let captured = debug_layout(&mut session);
        assert_eq!(captured.breakpoints, [at(3, true, None), logging, at(7, false, Some("n > 1"))]);
        assert_eq!(captured.watches, debug.watches);
        assert_eq!(captured.expressions, debug.expressions);
        let recent = registry.dispatch(&mut session, "debug.expression.history", serde_json::json!({})).unwrap();
        assert_eq!(recent["expressions"], serde_json::json!(["len(items)", "n"]));
        assert_eq!((captured.functions, captured.exceptions), (debug.functions, debug.exceptions));
    }

    #[test]
    fn the_search_scope_round_trips() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "searchScope": { "area": 7 } }"#).unwrap();
        assert_eq!(layout.search_scope, None, "a scope the file spells wrongly is left out");
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        assert_eq!(search_scope(&session), None);
        let chosen = serde_json::json!({ "area": "directory", "directory": "internal", "recursive": false, "everywhere": "allPlaces" });
        registry.dispatch(&mut session, "search.scope.select", chosen).unwrap();
        let layout = ProjectLayout { search_scope: search_scope(&session), ..ProjectLayout::default() };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(json["searchScope"]["directory"], "internal");
        let read: ProjectLayout = serde_json::from_value(json).unwrap();
        let mut reopened = Session::new();
        restore_search_scope(&registry, &mut reopened, read.search_scope.as_ref());
        assert_eq!(search_scope(&reopened), layout.search_scope);
        assert_eq!(search_scope(&reopened).unwrap().everywhere, "allPlaces");
    }

    #[test]
    fn the_run_selection_and_temporary_configurations_round_trip() {
        let layout: ProjectLayout = serde_json::from_str(r#"{ "projectPanel": true }"#).unwrap();
        assert_eq!(layout.run, RunLayout::default());
        let temporary = Configuration {
            id: "fake.run-4".to_owned(),
            name: "TestOne".to_owned(),
            kind: zen_run::fake::KIND.to_owned(),
            temporary: true,
            ..Configuration::default()
        };
        let run = RunLayout {
            selected: Some("fake.run-4".to_owned()),
            temporary: vec![temporary.clone()],
            anything: Vec::new(),
            services: RunServices { kinds: vec![zen_run::fake::KIND.to_owned()], ..RunServices::default() },
        };
        let layout = ProjectLayout { run: run.clone(), ..ProjectLayout::default() };
        let json = serde_json::to_value(&layout).unwrap();
        assert_eq!(json["run"]["selected"], "fake.run-4");
        assert_eq!(json["run"]["temporary"][0]["name"], "TestOne");
        assert_eq!(serde_json::from_value::<ProjectLayout>(json).unwrap(), layout);

        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let open = |registry: &Registry| {
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", serde_json::json!({ "path": dir.path() })).unwrap();
            session
        };
        let mut session = open(&registry);
        let saved = zen_run::fake::FakeProvider::configuration("saved");
        registry
            .dispatch(&mut session, "run.configuration.create", serde_json::json!({ "configuration": saved }))
            .unwrap();
        restore_run(&mut session, &run);
        let listed = registry.dispatch(&mut session, "run.configuration.list", serde_json::json!({})).unwrap();
        assert_eq!(listed["configurations"].as_array().map(Vec::len), Some(2));
        assert_eq!(listed["configurations"][1]["id"], "fake.run-4");
        assert_eq!(listed["selected"], "fake.run-4");
        assert_eq!(
            session.state::<RunServices>().map(|services| services.kinds.clone()),
            Some(run.services.kinds.clone())
        );
        let file = zen_run::RunConfigurations::read(dir.path()).unwrap();
        assert_eq!(file.configurations.len(), 1, "the temporary configuration stays out of the file");
        assert_eq!(file.selected, None);

        let mut other = file.configurations[0].clone();
        other.name = "other".to_owned();
        let params = serde_json::json!({ "configuration": other, "select": true });
        let other = registry.dispatch(&mut session, "run.configuration.create", params).unwrap();
        let captured = run_layout(session.state::<Runs>().unwrap());
        assert_eq!(captured.selected.as_deref(), other["id"].as_str());
        assert_eq!(captured.temporary, [temporary]);
        let mut restarted = open(&registry);
        restore_run(&mut restarted, &captured);
        let listed = registry.dispatch(&mut restarted, "run.configuration.list", serde_json::json!({})).unwrap();
        assert_eq!(listed["selected"], other["id"], "the selection survives a restart");
        let recent = RunLayout { anything: vec!["go test ./...".to_owned(), "make".to_owned()], ..captured.clone() };
        let saved: RunLayout = serde_json::from_value(serde_json::to_value(&recent).unwrap()).unwrap();
        restore_run(&mut restarted, &saved);
        let history = restarted.state::<AnythingHistory>().unwrap().entries().to_vec();
        assert_eq!(history, ["go test ./...", "make"], "Run Anything's recent commands survive a restart");
        assert!(serde_json::to_value(RunLayout::default()).unwrap().get("anything").is_none());
        assert_eq!(listed["configurations"].as_array().map(Vec::len), Some(3));
    }
}

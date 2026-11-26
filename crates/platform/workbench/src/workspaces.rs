use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::groups::{EditorGroup, GroupId, SplitDirection, SplitNode};
use crate::{Workbench, closing, history};

pub const FILE_KIND: &str = "file";

const TABS_LISTED: usize = 50;

/// A workspace as its feature registers it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    /// The stable id, such as `database`.
    pub id: String,
    /// The name people see, such as "Database".
    pub title: String,
    /// The name of its icon, such as `database`.
    pub icon: String,
}

/// An inner tab of a workspace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InnerTab {
    /// The tab's id, unique in its workspace.
    pub id: String,
    /// What the tab shows: `file`, or a kind the feature draws (`console`, `objectEditor`…).
    pub kind: String,
    /// The title on the tab.
    pub title: String,
    /// The file a text tab shows, a document of the workspace's editor group; absent for a tab the feature draws.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Whether people can close it.
    pub closable: bool,
    /// A small piece of data the feature keeps for the tab (never data rows or secrets).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
    /// The workspace's editor group the tab is in (`panes` of `workspace.host.state`); 0 until it has one.
    #[serde(default)]
    pub group: GroupId,
}

#[derive(Clone, Debug, Default)]
struct Tabs {
    tabs: Vec<InnerTab>,
    active: Option<String>,
    shown: BTreeMap<GroupId, String>,
    seen: Option<(GroupId, PathBuf, u64)>,
    next: u64,
}

struct Pane {
    id: GroupId,
    files: Vec<PathBuf>,
    shown: Option<PathBuf>,
}

impl Tabs {
    fn keep(&mut self, pinned: &[InnerTab]) {
        for (at, tab) in pinned.iter().enumerate() {
            match self.position(&tab.id) {
                Some(found) => {
                    let mut kept = self.tabs.remove(found);
                    kept.kind.clone_from(&tab.kind);
                    kept.title.clone_from(&tab.title);
                    kept.closable = tab.closable;
                    self.tabs.insert(at.min(self.tabs.len()), kept);
                }
                None => self.tabs.insert(at.min(self.tabs.len()), tab.clone()),
            }
        }
    }

    fn position(&self, id: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == id)
    }

    fn new_id(&mut self) -> String {
        loop {
            self.next += 1;
            let id = format!("t{}", self.next);
            if self.position(&id).is_none() {
                return id;
            }
        }
    }

    fn find(&self, group: GroupId, path: &Path) -> Option<&InnerTab> {
        self.tabs.iter().find(|tab| tab.group == group && tab.path.as_deref() == Some(path))
    }

    fn activate(&mut self, id: &str) {
        if let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) {
            self.shown.insert(tab.group, tab.id.clone());
            self.active = Some(tab.id.clone());
        }
    }

    fn in_group(&self, group: GroupId) -> Vec<&InnerTab> {
        self.tabs.iter().filter(|tab| tab.group == group).collect()
    }

    fn follow(&mut self, panes: &[Pane], front: Option<GroupId>, (counter, focused): (u64, GroupId)) {
        let holds = |group: GroupId, path: &Path| {
            panes.iter().any(|pane| pane.id == group && pane.files.iter().any(|file| file == path))
        };
        for at in 0..self.tabs.len() {
            let Some(path) = self.tabs[at].path.clone() else { continue };
            if holds(self.tabs[at].group, &path) {
                continue;
            }
            let free = panes.iter().find(|pane| pane.files.contains(&path) && self.find(pane.id, &path).is_none());
            if let Some(pane) = free {
                self.tabs[at].group = pane.id;
            }
        }
        self.tabs.retain(|tab| tab.path.as_ref().is_none_or(|path| holds(tab.group, path)));
        for pane in panes {
            for file in &pane.files {
                if self.find(pane.id, file).is_none() {
                    let id = self.new_id();
                    let tab = InnerTab {
                        id,
                        kind: FILE_KIND.to_owned(),
                        title: file_title(file),
                        path: Some(file.clone()),
                        closable: true,
                        data: serde_json::Value::Null,
                        group: pane.id,
                    };
                    self.tabs.push(tab);
                }
            }
        }
        let Some(front) =
            front.filter(|front| panes.iter().any(|pane| pane.id == *front)).or(panes.first().map(|pane| pane.id))
        else {
            if self.active.as_ref().is_none_or(|active| self.position(active).is_none()) {
                self.active = self.tabs.last().map(|tab| tab.id.clone());
            }
            return;
        };
        for tab in &mut self.tabs {
            if !panes.iter().any(|pane| pane.id == tab.group) {
                tab.group = front;
            }
        }
        if let Some(active) = self.active.clone() {
            self.activate(&active);
        }
        if let Some(pane) = panes.iter().find(|pane| pane.id == focused) {
            let now = pane.shown.as_ref().map(|shown| (pane.id, shown.clone(), counter));
            if self.seen != now {
                self.seen = now;
                if let Some(id) =
                    pane.shown.as_ref().and_then(|shown| self.find(pane.id, shown)).map(|tab| tab.id.clone())
                {
                    self.shown.insert(pane.id, id);
                }
            }
        }
        let tabs = &self.tabs;
        self.shown.retain(|group, id| tabs.iter().any(|tab| tab.id == *id && tab.group == *group));
        for pane in panes {
            if self.shown.contains_key(&pane.id) {
                continue;
            }
            let file = pane.shown.as_ref().and_then(|shown| self.find(pane.id, shown));
            if let Some(tab) = file.or_else(|| self.in_group(pane.id).last().copied()) {
                self.shown.insert(pane.id, tab.id.clone());
            }
        }
        self.active = self.shown.get(&front).cloned();
    }
}

pub type AcceptFile = fn(&Session, &Path) -> bool;

pub type CloseGuard = fn(&mut Session, &str, &InnerTab) -> Result<(), CommandError>;

pub type TabLabel = fn(&Session, &Path) -> Option<String>;

pub type OpenHook = fn(&mut Session) -> Result<(), CommandError>;

pub type CloseHook = fn(&mut Session);

#[derive(Clone, Debug, Default)]
pub struct Workspaces {
    close_guards: Vec<(String, CloseGuard)>,
    open_hooks: Vec<(String, OpenHook)>,
    close_hooks: Vec<(String, CloseHook)>,
    opened: BTreeSet<String>,
    labels: Vec<(String, TabLabel)>,
    hosted: Vec<(String, Vec<String>)>,
    accepts: Vec<(String, AcceptFile)>,
    restoring: bool,
    registered: Vec<WorkspaceInfo>,
    pinned: BTreeMap<String, Vec<InnerTab>>,
    tabs: BTreeMap<String, Tabs>,
    data: Option<PathBuf>,
    pending: Vec<SavedWorkspace>,
    remembered: BTreeMap<PathBuf, Vec<SavedWorkspace>>,
    left: Vec<(PathBuf, Vec<SavedWorkspace>)>,
}

impl Workspaces {
    pub fn guard_close(&mut self, workspace: &str, guard: CloseGuard) {
        self.close_guards.retain(|(known, _)| known != workspace);
        self.close_guards.push((workspace.to_owned(), guard));
    }

    pub fn on_open(&mut self, workspace: &str, hook: OpenHook) {
        self.open_hooks.retain(|(known, _)| known != workspace);
        self.open_hooks.push((workspace.to_owned(), hook));
    }

    pub fn on_close(&mut self, workspace: &str, hook: CloseHook) {
        self.close_hooks.retain(|(known, _)| known != workspace);
        self.close_hooks.push((workspace.to_owned(), hook));
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.opened.contains(id)
    }

    pub fn open_workspaces(&self) -> Vec<&WorkspaceInfo> {
        self.registered.iter().filter(|info| self.opened.contains(&info.id)).collect()
    }

    pub fn accept_files(&mut self, workspace: &str, accept: AcceptFile) {
        self.accepts.retain(|(known, _)| known != workspace);
        self.accepts.push((workspace.to_owned(), accept));
    }

    pub fn put_tab(&mut self, id: &str, tab: InnerTab) {
        let tabs = self.tabs.entry(id.to_owned()).or_default();
        match tabs.tabs.iter_mut().find(|known| known.id == tab.id) {
            Some(known) => {
                known.kind = tab.kind;
                known.title = tab.title;
                known.closable = tab.closable;
            }
            None => tabs.tabs.push(tab),
        }
    }

    pub fn remove_tab(&mut self, id: &str, tab: &str) -> bool {
        let Some(tabs) = self.tabs.get_mut(id) else { return false };
        let Some(at) = tabs.position(tab) else { return false };
        let removed = tabs.tabs.remove(at);
        if tabs.active.as_deref() == Some(tab) {
            let before =
                tabs.tabs[..at].iter().rev().find(|known| known.group == removed.group).map(|known| known.id.clone());
            let after = tabs.tabs[at..].iter().find(|known| known.group == removed.group).map(|known| known.id.clone());
            tabs.active = None;
            tabs.shown.remove(&removed.group);
            if let Some(next) = before.or(after) {
                tabs.activate(&next);
            }
        }
        true
    }

    pub fn label_tabs(&mut self, workspace: &str, label: TabLabel) {
        self.labels.retain(|(known, _)| known != workspace);
        self.labels.push((workspace.to_owned(), label));
    }

    pub fn host_files(&mut self, workspace: &str, extensions: &[&str]) {
        self.hosted.retain(|(known, _)| known != workspace);
        self.hosted.push((workspace.to_owned(), extensions.iter().map(|extension| (*extension).to_owned()).collect()));
    }

    pub fn hosts(&self, workspace: &str, path: &Path) -> bool {
        self.hosted.iter().find(|(known, _)| known == workspace).is_none_or(|(_, extensions)| {
            path.extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extensions.iter().any(|known| known.eq_ignore_ascii_case(extension)))
        })
    }

    pub fn register(&mut self, info: WorkspaceInfo) {
        match self.registered.iter_mut().find(|known| known.id == info.id) {
            Some(known) => *known = info,
            None => self.registered.push(info),
        }
    }

    pub fn pin(&mut self, id: &str, tab: InnerTab) {
        let pinned = self.pinned.entry(id.to_owned()).or_default();
        pinned.retain(|known| known.id != tab.id);
        pinned.push(tab);
    }

    pub fn is_pinned(&self, id: &str, tab: &str) -> bool {
        self.pinned.get(id).is_some_and(|pinned| pinned.iter().any(|known| known.id == tab))
    }

    pub fn unregister(&mut self, id: &str) {
        self.registered.retain(|known| known.id != id);
    }

    pub fn registered(&self) -> &[WorkspaceInfo] {
        &self.registered
    }

    pub fn info(&self, id: &str) -> Option<&WorkspaceInfo> {
        self.registered.iter().find(|known| known.id == id)
    }

    pub fn tabs(&self, id: &str) -> &[InnerTab] {
        self.tabs.get(id).map_or(&[], |tabs| tabs.tabs.as_slice())
    }

    pub fn active_tab(&self, id: &str) -> Option<&InnerTab> {
        let tabs = self.tabs.get(id)?;
        tabs.active.as_ref().and_then(|active| tabs.tabs.iter().find(|tab| tab.id == *active))
    }

    pub fn shown_in(&self, id: &str, group: GroupId) -> Option<&InnerTab> {
        let tabs = self.tabs.get(id)?;
        tabs.shown.get(&group).and_then(|shown| tabs.tabs.iter().find(|tab| tab.id == *shown))
    }

    pub fn accepts(&self, id: &str, session: &Session, path: &Path) -> bool {
        self.accepts.iter().any(|(known, accept)| known == id && accept(session, path))
    }

    pub fn set_data_dir(&mut self, data: PathBuf) {
        self.data = Some(data);
    }

    pub fn data_dir(&self) -> Option<&Path> {
        self.data.as_deref()
    }

    pub fn consoles_folder(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|data| data.join("consoles"))
    }

    pub fn remember_project(&mut self, project: PathBuf, saved: Vec<SavedWorkspace>) {
        self.remembered.insert(project, saved);
    }

    pub fn take_left(&mut self) -> Vec<(PathBuf, Vec<SavedWorkspace>)> {
        std::mem::take(&mut self.left)
    }

    pub fn set_tab_data(&mut self, id: &str, path: &Path, data: serde_json::Value) {
        let Some(tabs) = self.tabs.get_mut(id) else { return };
        for tab in tabs.tabs.iter_mut().filter(|tab| tab.path.as_deref() == Some(path)) {
            tab.data = data.clone();
        }
    }

    pub fn set_kind_data(&mut self, id: &str, tab: &str, data: serde_json::Value) {
        if let Some(known) = self.tabs.get_mut(id).and_then(|tabs| tabs.tabs.iter_mut().find(|known| known.id == tab)) {
            known.data = data;
        }
    }

    pub fn is_console(&self, path: &Path) -> bool {
        self.consoles_folder().is_some_and(|folder| path.starts_with(&folder) && path != folder)
    }
}

pub fn expanded(session: &Session) -> Option<String> {
    let id = session.state::<Workbench>()?.groups().expanded()?.to_owned();
    session.state::<Workspaces>()?.info(&id).map(|info| info.id.clone())
}

pub fn in_front(session: &Session) -> Option<(WorkspaceInfo, Option<InnerTab>)> {
    let id = expanded(session)?;
    let workspaces = session.state::<Workspaces>()?;
    Some((workspaces.info(&id)?.clone(), workspaces.active_tab(&id).cloned()))
}

pub fn tab_in_front(session: &Session) -> Option<(WorkspaceInfo, InnerTab)> {
    match in_front(session)? {
        (info, Some(tab)) if tab.path.is_none() => Some((info, tab)),
        _ => None,
    }
}

pub fn refusal(session: &Session, command: &str, params: &serde_json::Value) -> Option<String> {
    let acts_on_text = command.starts_with("editor.")
        && !command.starts_with("editor.split.")
        && !command.starts_with("editor.tab.")
        && params.get("path").is_none_or(serde_json::Value::is_null);
    if !acts_on_text {
        return None;
    }
    let (info, tab) = tab_in_front(session)?;
    Some(t!("workspace-error-not-text", command = command, tab = tab.title.as_str(), workspace = info.title.as_str()))
}

pub fn sync(session: &mut Session) {
    apply_pending(session);
    for _ in 0..4 {
        follow_groups(session);
        if !drop_empty_groups(session) {
            break;
        }
    }
    retitle(session);
}

fn follow_groups(session: &mut Session) {
    let (counter, focused) =
        session.state::<Workbench>().map_or((0, 1), |workbench| (workbench.shown(), workbench.groups().focused()));
    let owned: Vec<(String, Vec<Pane>, GroupId)> = session
        .state::<Workbench>()
        .map(|workbench| {
            workbench
                .groups()
                .owned_groups()
                .iter()
                .map(|owned| {
                    let panes = owned
                        .groups()
                        .iter()
                        .map(|group| Pane {
                            id: group.id,
                            files: group.tabs.clone(),
                            shown: group.active_path().map(Path::to_owned),
                        })
                        .collect();
                    (owned.workspace.clone(), panes, owned.focused)
                })
                .collect()
        })
        .unwrap_or_default();
    let workspaces = session.state_mut::<Workspaces>();
    let ids: Vec<String> = workspaces.registered.iter().map(|info| info.id.clone()).collect();
    for id in ids {
        let (panes, front) = owned
            .iter()
            .find(|(workspace, ..)| *workspace == id)
            .map(|(_, panes, front)| (panes.as_slice(), Some(*front)))
            .unwrap_or((&[], None));
        let pinned = workspaces.pinned.get(&id).cloned().unwrap_or_default();
        let tabs = workspaces.tabs.entry(id).or_default();
        tabs.keep(&pinned);
        tabs.follow(panes, front, (counter, focused));
    }
}

fn drop_empty_groups(session: &mut Session) -> bool {
    let Some(workspaces) = session.state::<Workspaces>().filter(|workspaces| !workspaces.restoring) else {
        return false;
    };
    let Some(workbench) = session.state::<Workbench>() else { return false };
    let empty: Vec<GroupId> = workbench
        .groups()
        .owned_groups()
        .iter()
        .filter(|owned| owned.is_split() && workspaces.info(&owned.workspace).is_some())
        .flat_map(|owned| {
            let tabs = workspaces.tabs.get(&owned.workspace);
            owned
                .groups()
                .into_iter()
                .filter(move |group| tabs.is_none_or(|tabs| tabs.in_group(group.id).is_empty()))
                .map(|group| group.id)
        })
        .collect();
    let workbench = session.state_mut::<Workbench>();
    let mut dropped = false;
    for id in empty {
        dropped |= workbench.groups.remove_owned(id);
    }
    if dropped {
        workbench.sync_groups();
    }
    dropped
}

fn file_title(path: &Path) -> String {
    path.file_name().map_or_else(|| crate::display(path), |name| name.to_string_lossy().into())
}

fn retitle(session: &mut Session) {
    let Some(workspaces) = session.state::<Workspaces>() else { return };
    let mut titles = Vec::new();
    for (workspace, tabs) in &workspaces.tabs {
        let label = workspaces.labels.iter().find(|(known, _)| known == workspace).map(|(_, label)| *label);
        let files: Vec<(&str, &Path, String)> = tabs
            .tabs
            .iter()
            .filter(|tab| tab.kind == FILE_KIND)
            .filter_map(|tab| Some((tab.id.as_str(), tab.path.as_deref()?, file_title(tab.path.as_deref()?))))
            .collect();
        for (id, path, name) in &files {
            let twins = files.iter().any(|(_, other_path, other)| other == name && other_path != path);
            let title = if twins {
                let folder = label.and_then(|label| label(session, path)).or_else(|| {
                    path.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned())
                });
                match folder {
                    Some(folder) => format!("{name} ({folder})"),
                    None => name.clone(),
                }
            } else {
                name.clone()
            };
            titles.push((workspace.clone(), (*id).to_owned(), title));
        }
    }
    let workspaces = session.state_mut::<Workspaces>();
    for (workspace, id, title) in titles {
        let tab = workspaces.tabs.get_mut(&workspace).and_then(|tabs| tabs.tabs.iter_mut().find(|tab| tab.id == id));
        if let Some(tab) = tab.filter(|tab| tab.title != title) {
            tab.title = title;
        }
    }
}

pub fn save_consoles(session: &mut Session) -> (Vec<PathBuf>, Vec<(PathBuf, String)>) {
    let dirty = dirty_consoles(session);
    if dirty.is_empty() {
        return (Vec::new(), Vec::new());
    }
    closing::save_documents(session, &dirty)
}

pub fn dirty_console_versions(session: &Session) -> Vec<(PathBuf, u64)> {
    let (Some(workspaces), Some(workbench)) = (session.state::<Workspaces>(), session.state::<Workbench>()) else {
        return Vec::new();
    };
    workbench
        .documents
        .iter()
        .filter(|document| document.is_dirty() && workspaces.is_console(&document.path))
        .map(|document| (document.path.clone(), document.version()))
        .collect()
}

fn dirty_consoles(session: &Session) -> Vec<PathBuf> {
    dirty_console_versions(session).into_iter().map(|(path, _)| path).collect()
}

/// An inner tab as `workspace.json` keeps it: what it shows, never data rows, cell values or secrets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SavedTab {
    /// The tab's id.
    pub id: String,
    /// What the tab shows: `file`, or a kind the feature draws.
    pub kind: String,
    /// The title on the tab.
    pub title: String,
    /// The file of a text tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Whether people can close it.
    #[serde(default = "closable_by_default")]
    pub closable: bool,
    /// The feature's small data for the tab.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
    /// A text tab's caret, as a character offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caret: Option<usize>,
    /// The first line a text tab showed, from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_line: Option<usize>,
    /// The workspace's editor group it was in, as `layout` numbers them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<GroupId>,
}

fn closable_by_default() -> bool {
    true
}

/// A workspace as `workspace.json` keeps it across restarts (ADR-0163, decision 7): no data and no secrets.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SavedWorkspace {
    /// The workspace's id.
    pub id: String,
    /// Whether it was expanded.
    pub expanded: bool,
    /// Whether it was open in the window, its tab in the editor tab strip; an expanded one is open.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub open: bool,
    /// The main editor group collapsing gives the keyboard back to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_to: Option<GroupId>,
    /// Its inner tabs in order; a tab that cannot be read drops itself.
    #[serde(deserialize_with = "lenient_entries")]
    pub tabs: Vec<SavedTab>,
    /// The id of the active tab.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    /// The sizes of the feature's panels, by name, as the feature reports them.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", deserialize_with = "lenient_sizes")]
    pub panels: BTreeMap<String, f32>,
    /// How its area was split into editor groups, without their tabs; absent when it was not split.
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient_layout")]
    pub layout: Option<SplitNode>,
    /// The id of the tab each of its editor groups showed.
    #[serde(skip_serializing_if = "Vec::is_empty", deserialize_with = "lenient_entries")]
    pub shown: Vec<String>,
}

fn lenient_layout<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<SplitNode>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

fn shape(node: &SplitNode) -> SplitNode {
    match node {
        SplitNode::Group(group) => SplitNode::Group(EditorGroup { id: group.id, ..EditorGroup::default() }),
        SplitNode::Split { direction, ratio, first, second } => SplitNode::Split {
            direction: *direction,
            ratio: *ratio,
            first: Box::new(shape(first)),
            second: Box::new(shape(second)),
        },
    }
}

pub fn lenient_entries<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let entries = match value {
        serde_json::Value::Array(entries) => entries,
        _ => Vec::new(),
    };
    Ok(entries.into_iter().filter_map(|entry| serde_json::from_value(entry).ok()).collect())
}

fn lenient_sizes<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<BTreeMap<String, f32>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    let sizes = value.as_object().into_iter().flatten().filter_map(|(name, size)| {
        let size = size.as_f64().filter(|size| size.is_finite() && *size >= 0.0)?;
        Some((name.clone(), size as f32))
    });
    Ok(sizes.collect())
}

pub fn saved(session: &mut Session) -> Vec<SavedWorkspace> {
    sync(session);
    let expanded = expanded(session);
    let return_to = session.state::<Workbench>().map(|workbench| workbench.groups().main_focused());
    let Some(workspaces) = session.state::<Workspaces>() else { return Vec::new() };
    let workbench = session.state::<Workbench>();
    let caret = |path: &Path| {
        let document = workbench?.documents.iter().find(|document| document.path == path)?;
        Some(document.selections().primary().head)
    };
    let owned =
        |id: &str| workbench.and_then(|workbench| workbench.groups().owned_of(id)).filter(|owned| owned.is_split());
    let mut saved: Vec<SavedWorkspace> = workspaces
        .registered
        .iter()
        .filter_map(|info| {
            let open = expanded.as_deref() == Some(info.id.as_str());
            let tabs = workspaces.tabs(&info.id);
            if tabs.iter().all(|tab| workspaces.is_pinned(&info.id, &tab.id)) && !open && !workspaces.is_open(&info.id)
            {
                return None;
            }
            let tabs = tabs
                .iter()
                .map(|tab| SavedTab {
                    id: tab.id.clone(),
                    kind: tab.kind.clone(),
                    title: tab.title.clone(),
                    path: tab.path.clone(),
                    closable: tab.closable,
                    data: tab.data.clone(),
                    caret: tab.path.as_deref().and_then(caret),
                    first_line: None,
                    group: owned(&info.id).map(|_| tab.group),
                })
                .collect();
            let split = owned(&info.id);
            let shown = split
                .map(|split| {
                    split
                        .groups()
                        .iter()
                        .filter_map(|group| Some(workspaces.shown_in(&info.id, group.id)?.id.clone()))
                        .collect()
                })
                .unwrap_or_default();
            Some(SavedWorkspace {
                id: info.id.clone(),
                expanded: open,
                open: open || workspaces.is_open(&info.id),
                return_to,
                tabs,
                active: workspaces.active_tab(&info.id).map(|tab| tab.id.clone()),
                panels: BTreeMap::new(),
                layout: split.map(|split| shape(&split.root)),
                shown,
            })
        })
        .collect();
    saved.extend(workspaces.pending.iter().cloned());
    saved
}

pub fn restore(session: &mut Session, saved: Vec<SavedWorkspace>) {
    let workspaces = session.state_mut::<Workspaces>();
    for entry in saved.into_iter().filter(|entry| !entry.id.is_empty()) {
        workspaces.pending.retain(|pending| pending.id != entry.id);
        workspaces.pending.push(entry);
    }
    apply_pending(session);
}

pub(crate) fn leave(session: &mut Session, project: &Path) {
    if session.state::<Workspaces>().is_none() {
        return;
    }
    save_consoles(session);
    let kept = saved(session);
    let workspaces = session.state_mut::<Workspaces>();
    workspaces.remembered.insert(project.to_owned(), kept.clone());
    workspaces.left.retain(|(left, _)| left != project);
    workspaces.left.push((project.to_owned(), kept));
    workspaces.tabs.clear();
    workspaces.pending.clear();
    workspaces.opened.clear();
}

pub(crate) fn arrive(session: &mut Session, project: &Path) {
    let kept = session.state::<Workspaces>().and_then(|workspaces| workspaces.remembered.get(project).cloned());
    let Some(mut kept) = kept else { return };
    for workspace in &mut kept {
        workspace.expanded = false;
        workspace.return_to = None;
    }
    restore(session, kept);
}

fn apply_pending(session: &mut Session) {
    let ready: Vec<SavedWorkspace> = {
        let Some(workspaces) = session.state::<Workspaces>() else { return };
        if workspaces.pending.is_empty() {
            return;
        }
        workspaces.pending.iter().filter(|entry| workspaces.info(&entry.id).is_some()).cloned().collect()
    };
    if ready.is_empty() {
        return;
    }
    session.state_mut::<Workspaces>().pending.retain(|entry| !ready.iter().any(|ready| ready.id == entry.id));
    for entry in ready {
        apply(session, entry);
    }
}

fn apply(session: &mut Session, entry: SavedWorkspace) {
    let focused = session.state::<Workbench>().map(|workbench| workbench.groups().focused());
    owned_group(session, &entry.id);
    let renumbered = match &entry.layout {
        Some(layout) => session.state_mut::<Workbench>().groups.lay_out(&entry.id, layout),
        None => Vec::new(),
    };
    let group = owned_group(session, &entry.id);
    let placed = |tab: &SavedTab| {
        tab.group
            .and_then(|old| renumbered.iter().find(|(known, _)| *known == old).map(|(_, new)| *new))
            .unwrap_or(group)
    };
    session.state_mut::<Workspaces>().restoring = true;
    let mut kept = Vec::new();
    for mut tab in entry.tabs {
        let pane = placed(&tab);
        tab.group = Some(pane);
        if let Some(path) = &tab.path {
            let hosted = session.state::<Workspaces>().is_none_or(|workspaces| workspaces.hosts(&entry.id, path));
            if !hosted || !path.is_file() || crate::open_file_in(session, path, false, Some(pane)).is_err() {
                continue;
            }
            if let Some(caret) = tab.caret {
                let workbench = session.state_mut::<Workbench>();
                if let Some(document) = workbench.documents.iter_mut().find(|document| document.path == *path) {
                    let caret = caret.min(document.snapshot().len_chars());
                    document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
                }
            }
        }
        kept.push(tab);
    }
    let active = entry
        .active
        .filter(|active| kept.iter().any(|tab| tab.id == *active))
        .or_else(|| kept.last().map(|tab| tab.id.clone()));
    let mut shown: BTreeMap<GroupId, String> = BTreeMap::new();
    for id in entry.shown.iter().chain(active.iter()) {
        if let Some(tab) = kept.iter().find(|tab| tab.id == *id) {
            shown.insert(tab.group.unwrap_or(group), tab.id.clone());
        }
    }
    let group = active.as_ref().and_then(|active| kept.iter().find(|tab| tab.id == *active)?.group).unwrap_or(group);
    let workbench = session.state_mut::<Workbench>();
    for (pane, id) in &shown {
        let file = kept.iter().find(|tab| tab.id == *id).and_then(|tab| tab.path.as_ref());
        let index = file.and_then(|file| workbench.groups.group(*pane)?.tabs.iter().position(|tab| tab == file));
        if let Some(index) = index {
            workbench.groups.select(*pane, index);
        }
    }
    let seen = workbench
        .groups
        .group(group)
        .and_then(EditorGroup::active_path)
        .map(|path| (group, path.to_owned(), workbench.shown));
    let next = kept.iter().filter_map(|tab| tab.id.strip_prefix('t')?.parse::<u64>().ok()).max().unwrap_or(0);
    let tabs = kept
        .into_iter()
        .map(|tab| InnerTab {
            id: tab.id,
            kind: tab.kind,
            title: tab.title,
            path: tab.path,
            closable: tab.closable,
            data: tab.data,
            group: tab.group.unwrap_or(group),
        })
        .collect();
    let restored = Tabs { tabs, active, shown, seen, next };
    let workspaces = session.state_mut::<Workspaces>();
    workspaces.restoring = false;
    workspaces.tabs.insert(entry.id.clone(), restored);
    if entry.open || entry.expanded {
        workspaces.opened.insert(entry.id.clone());
    }
    let target = if entry.expanded {
        group
    } else {
        let workbench = session.state::<Workbench>();
        let main = workbench.map_or(1, |workbench| workbench.groups().main_focused());
        let wanted = entry.return_to.filter(|wanted| {
            workbench.is_some_and(|workbench| {
                workbench.groups().group(*wanted).is_some() && workbench.groups().owner(*wanted).is_none()
            })
        });
        match focused.filter(|focused| *focused != group) {
            Some(focused) if wanted.is_none() => focused,
            _ => wanted.unwrap_or(main),
        }
    };
    focus(session, target);
}

pub(crate) fn closed(session: &mut Session, path: &Path, text: &str) {
    let Some(folder) = session.state::<Workspaces>().and_then(Workspaces::consoles_folder) else { return };
    if !path.starts_with(&folder) || path == folder || !text.trim().is_empty() {
        return;
    }
    if !std::fs::read_to_string(path).is_ok_and(|saved| saved.trim().is_empty()) {
        return;
    }
    let workbench = session.state_mut::<Workbench>();
    if workbench.documents.iter().any(|document| document.path == path) {
        return;
    }
    if std::fs::remove_file(path).is_ok() {
        workbench.recent.retain(|recent| recent != path);
        workbench.revision += 1;
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListWorkspaces)?;
    registry.register(WorkspaceStateCommand)?;
    registry.register(Expand)?;
    registry.register(Collapse)?;
    registry.register(Toggle)?;
    registry.register(CloseWorkspace)?;
    registry.register(OpenTab)?;
    registry.register(SelectTab)?;
    registry.register(CloseTab)?;
    registry.register(MoveTab)?;
    registry.register(SplitTab)?;
    Ok(())
}

fn unknown(id: &str) -> CommandError {
    CommandError::new(t!("workspace-error-unknown", id = id))
}

fn known(session: &Session, id: &str) -> Result<(), CommandError> {
    match session.state::<Workspaces>().and_then(|workspaces| workspaces.info(id)) {
        Some(_) => Ok(()),
        None => Err(unknown(id)),
    }
}

fn focus(session: &mut Session, group: GroupId) {
    let origin = history::caret(session);
    let workbench = session.state_mut::<Workbench>();
    if workbench.groups.focus(group) {
        workbench.sync_groups();
        workbench.shown += 1;
    }
    let mark = workbench.groups.owner(group).map(str::to_owned).map(|workspace| {
        let shown = workbench.groups.group(group).and_then(EditorGroup::active_path).map(Path::to_owned);
        (workspace, shown.map(|shown| (group, shown, workbench.shown)))
    });
    if let Some((workspace, seen)) = mark {
        let workspaces = session.state_mut::<Workspaces>();
        workspaces.opened.insert(workspace.clone());
        workspaces.tabs.entry(workspace).or_default().seen = seen;
    }
    history::left_file(session, origin);
}

fn owned_group(session: &mut Session, id: &str) -> GroupId {
    session.state_mut::<Workbench>().groups.own(id)
}

fn expand(session: &mut Session, id: &str) -> Result<(), CommandError> {
    known(session, id)?;
    open(session, id)?;
    let group = owned_group(session, id);
    focus(session, group);
    sync(session);
    Ok(())
}

fn open(session: &mut Session, id: &str) -> Result<(), CommandError> {
    let Some(workspaces) = session.state::<Workspaces>() else { return Ok(()) };
    if workspaces.is_open(id) {
        return Ok(());
    }
    let hook = workspaces.open_hooks.iter().find(|(known, _)| known == id).map(|(_, hook)| *hook);
    if let Some(hook) = hook {
        hook(session)?;
    }
    session.state_mut::<Workspaces>().opened.insert(id.to_owned());
    Ok(())
}

pub fn bring(session: &mut Session, id: &str, tab: &str) {
    if known(session, id).is_err() {
        return;
    }
    session.state_mut::<Workspaces>().opened.insert(id.to_owned());
    let fallback = owned_group(session, id);
    sync(session);
    let tabs = session.state_mut::<Workspaces>().tabs.entry(id.to_owned()).or_default();
    tabs.activate(tab);
    let group = tabs.tabs.iter().find(|known| known.id == tab).map(|known| known.group);
    focus(session, group.filter(|group| *group != 0).unwrap_or(fallback));
    sync(session);
}

pub fn close(session: &mut Session, id: &str) {
    if expanded(session).as_deref() == Some(id) {
        collapse(session);
    }
    let Some(workspaces) = session.state::<Workspaces>() else { return };
    if !workspaces.is_open(id) {
        return;
    }
    let hook = workspaces.close_hooks.iter().find(|(known, _)| known == id).map(|(_, hook)| *hook);
    session.state_mut::<Workspaces>().opened.remove(id);
    if let Some(hook) = hook {
        hook(session);
    }
}

fn collapse(session: &mut Session) {
    let main = session.state::<Workbench>().map(|workbench| workbench.groups().main_focused());
    if let Some(main) = main.filter(|_| expanded(session).is_some()) {
        focus(session, main);
    }
    sync(session);
}

/// A registered workspace in a list.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSummary {
    /// The stable id.
    pub id: String,
    /// The name people see.
    pub title: String,
    /// The name of its icon.
    pub icon: String,
    /// Whether it is expanded over the editor area.
    pub expanded: bool,
    /// Whether it is open in the window: its tab shows in the editor tab strip.
    pub open: bool,
    /// How many inner tabs it has.
    pub tabs: usize,
}

/// The registered workspaces.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceList {
    /// Every registered workspace.
    pub workspaces: Vec<WorkspaceSummary>,
    /// The id of the expanded one, if any.
    pub expanded: Option<String>,
}

pub struct ListWorkspaces;

impl Command for ListWorkspaces {
    type Params = NoParams;
    type Output = WorkspaceList;
    const ID: &'static str = "workspace.host.list";
    const TITLE: &'static str = "List Workspaces";
    const DESCRIPTION: &'static str = "Lists the registered workspaces (full tools that expand over the editor area, such as Database): each one's id, title, icon, whether it is expanded and how many inner tabs it has.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<WorkspaceList, CommandError> {
        sync(session);
        let expanded = expanded(session);
        let workspaces = session
            .state::<Workspaces>()
            .map(|workspaces| {
                workspaces
                    .registered
                    .iter()
                    .map(|info| WorkspaceSummary {
                        id: info.id.clone(),
                        title: info.title.clone(),
                        icon: info.icon.clone(),
                        expanded: expanded.as_deref() == Some(info.id.as_str()),
                        open: workspaces.is_open(&info.id),
                        tabs: workspaces.tabs(&info.id).len(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(WorkspaceList { workspaces, expanded })
    }
}

/// Which workspace.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceParams {
    /// The workspace's id, from `workspace.host.list`.
    pub id: String,
}

/// Which workspace, and how many of its tabs to list.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StateParams {
    /// The workspace's id, from `workspace.host.list`.
    pub id: String,
    /// The most tabs to list; 50 when absent.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// One editor group of a workspace's area.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PaneState {
    /// The group's id (`group` of an inner tab).
    pub group: GroupId,
    /// The id of the tab it shows.
    pub active: Option<String>,
}

/// A workspace as it is now.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceState {
    /// The stable id.
    pub id: String,
    /// The name people see.
    pub title: String,
    /// The name of its icon.
    pub icon: String,
    /// Whether it is expanded: its editor group has the keyboard.
    pub expanded: bool,
    /// Whether it is open in the window: its tab shows in the editor tab strip.
    pub open: bool,
    /// Its editor group that has, or last had, the keyboard (`editor.split.list`, under `workspaces`); absent until it first expands.
    pub group: Option<GroupId>,
    /// Its editor groups, left to right and top to bottom, more than one when its area is split.
    pub panes: Vec<PaneState>,
    /// The main editor group that collapsing gives the keyboard back to.
    pub return_to: GroupId,
    /// The id of the active inner tab.
    pub active: Option<String>,
    /// The inner tabs in order, at most `limit` of them.
    pub tabs: Vec<InnerTab>,
    /// How many inner tabs it has.
    pub total: usize,
    /// Whether tabs were left out because of `limit`.
    pub truncated: bool,
}

#[derive(Clone, Default)]
pub struct AgentHiddenTabs(pub Option<HiddenTabFn>);

pub type HiddenTabFn = Arc<dyn Fn(&Session, &InnerTab) -> bool + Send + Sync>;

fn hidden_from_agent(session: &Session, tab: &InnerTab) -> bool {
    zen_core::macros::by_agent(session)
        && session
            .state::<AgentHiddenTabs>()
            .and_then(|hidden| hidden.0.clone())
            .is_some_and(|hidden| hidden(session, tab))
}

fn state(session: &mut Session, id: &str, limit: Option<usize>) -> Result<WorkspaceState, CommandError> {
    sync(session);
    let expanded = expanded(session);
    let (group, return_to) = session
        .state::<Workbench>()
        .map(|workbench| (workbench.groups().owned(id).map(|group| group.id), workbench.groups().main_focused()))
        .unwrap_or((None, 1));
    let groups: Vec<GroupId> = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.groups().owned_of(id))
        .map(|owned| owned.groups().iter().map(|group| group.id).collect())
        .unwrap_or_default();
    let workspaces = session.state::<Workspaces>().ok_or_else(|| unknown(id))?;
    let info = workspaces.info(id).ok_or_else(|| unknown(id))?;
    let panes = groups
        .into_iter()
        .map(|group| PaneState {
            group,
            active: workspaces
                .shown_in(id, group)
                .filter(|tab| !hidden_from_agent(session, tab))
                .map(|tab| tab.id.clone()),
        })
        .collect();
    let all: Vec<&InnerTab> = workspaces.tabs(id).iter().filter(|tab| !hidden_from_agent(session, tab)).collect();
    let active = workspaces.active_tab(id).filter(|tab| !hidden_from_agent(session, tab)).map(|tab| tab.id.clone());
    let limit = limit.unwrap_or(TABS_LISTED);
    Ok(WorkspaceState {
        id: info.id.clone(),
        title: info.title.clone(),
        icon: info.icon.clone(),
        expanded: expanded.as_deref() == Some(id),
        open: workspaces.is_open(id),
        group,
        panes,
        return_to,
        active,
        tabs: all.iter().take(limit).map(|tab| (*tab).clone()).collect(),
        total: all.len(),
        truncated: all.len() > limit,
    })
}

pub struct WorkspaceStateCommand;

impl Command for WorkspaceStateCommand {
    type Params = StateParams;
    type Output = WorkspaceState;
    const ID: &'static str = "workspace.host.state";
    const TITLE: &'static str = "Workspace State";
    const DESCRIPTION: &'static str = "Returns a workspace's state: whether it is expanded, its editor group with the keyboard and every one of its groups with the tab each shows (`panes`, more than one when its area is split), the main group collapsing returns to, its inner tabs in order (id, kind, title, the file of a text tab, whether it closes, kind data, its group; at most `limit`, with `truncated`) and the active one.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<WorkspaceState, CommandError> {
        state(session, &params.id, params.limit)
    }
}

pub struct Expand;

impl Command for Expand {
    type Params = WorkspaceParams;
    type Output = WorkspaceState;
    const ID: &'static str = "workspace.host.expand";
    const TITLE: &'static str = "Expand Workspace";
    const DESCRIPTION: &'static str = "Expands a workspace over the editor area: its editor group takes the keyboard and its tabs come back as they were left; the main editor is kept untouched beneath. Answers with the workspace's state.";

    fn run(&self, session: &mut Session, params: WorkspaceParams) -> Result<WorkspaceState, CommandError> {
        expand(session, &params.id)?;
        state(session, &params.id, None)
    }
}

/// Which workspace to collapse.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CollapseParams {
    /// The workspace's id; the expanded one when absent. Nothing happens when that one is not expanded.
    pub id: Option<String>,
}

pub struct Collapse;

impl Command for Collapse {
    type Params = CollapseParams;
    type Output = WorkspaceList;
    const ID: &'static str = "workspace.host.collapse";
    const TITLE: &'static str = "Collapse Workspace";
    const DESCRIPTION: &'static str = "Collapses the expanded workspace (or the one named, when it is expanded): the main editor group that had the keyboard gets it back, and every tab of the workspace stays open. Answers with the workspaces.";

    fn run(&self, session: &mut Session, params: CollapseParams) -> Result<WorkspaceList, CommandError> {
        if let Some(id) = &params.id {
            known(session, id)?;
        }
        if params.id.is_none() || params.id == expanded(session) {
            collapse(session);
        }
        ListWorkspaces.run(session, NoParams {})
    }
}

pub struct Toggle;

impl Command for Toggle {
    type Params = WorkspaceParams;
    type Output = WorkspaceState;
    const ID: &'static str = "workspace.host.toggle";
    const TITLE: &'static str = "Toggle Workspace";
    const DESCRIPTION: &'static str = "Expands a workspace over the editor area, or collapses it when it is expanded, as its button on the right stripe does. Answers with the workspace's state.";

    fn run(&self, session: &mut Session, params: WorkspaceParams) -> Result<WorkspaceState, CommandError> {
        known(session, &params.id)?;
        if expanded(session).as_deref() == Some(params.id.as_str()) {
            collapse(session);
        } else {
            expand(session, &params.id)?;
        }
        state(session, &params.id, None)
    }
}

pub struct CloseWorkspace;

impl Command for CloseWorkspace {
    type Params = WorkspaceParams;
    type Output = WorkspaceList;
    const ID: &'static str = "workspace.host.close";
    const TITLE: &'static str = "Close Workspace";
    const DESCRIPTION: &'static str = "Closes a workspace in the window, as the × on its tab in the editor tab strip does: collapses it when it is expanded and takes its tab out of the strip. Its inner tabs stay and come back when it expands again. Answers with the workspaces.";

    fn run(&self, session: &mut Session, params: WorkspaceParams) -> Result<WorkspaceList, CommandError> {
        known(session, &params.id)?;
        close(session, &params.id);
        ListWorkspaces.run(session, NoParams {})
    }
}

/// An inner tab to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenTabParams {
    /// The workspace's id, from `workspace.host.list`.
    pub workspace: String,
    /// The file of a text tab: absolute, or relative to the project; it opens as a document of the workspace's editor group.
    pub path: Option<PathBuf>,
    /// The tab's kind; `file` for a text tab when absent. A tab without a file needs one.
    pub kind: Option<String>,
    /// The title on the tab; a text tab's file name when absent. A tab without a file needs one.
    pub title: Option<String>,
    /// The tab's id: a tab with this id is brought to front (and updated); a new id when absent.
    pub id: Option<String>,
    /// Whether people can close it; true when absent.
    pub closable: Option<bool>,
    /// A small piece of data for the feature's view (never data rows or secrets).
    pub data: Option<serde_json::Value>,
    /// The workspace's editor group to open it in; the one that has, or last had, the keyboard when absent.
    pub group: Option<GroupId>,
}

pub struct OpenTab;

impl Command for OpenTab {
    type Params = OpenTabParams;
    type Output = InnerTab;
    const ID: &'static str = "workspace.tab.open";
    const TITLE: &'static str = "Open Workspace Tab";
    const DESCRIPTION: &'static str = "Opens an inner tab of a workspace, or brings one with the same file or id to front, and expands the workspace: with `path` a text tab, the file opened in the workspace's editor group as any file is; without it a tab of `kind` the feature draws. A file the workspace does not host (the Database workspace hosts `.sql` files) is refused with code `notHosted`: it belongs in the editor (file.document.open). Answers with the tab.";

    fn run(&self, session: &mut Session, params: OpenTabParams) -> Result<InnerTab, CommandError> {
        open_tab(session, params, false)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn open_code_tab(session: &mut Session, params: OpenTabParams) -> Result<InnerTab, CommandError> {
    open_tab(session, params, true)
}

fn open_tab(session: &mut Session, params: OpenTabParams, any_file: bool) -> Result<InnerTab, CommandError> {
    known(session, &params.workspace)?;
    let mut group = owned_group(session, &params.workspace);
    if let Some(wanted) = params.group {
        let theirs = session
            .state::<Workbench>()
            .is_some_and(|workbench| workbench.groups().owner(wanted) == Some(params.workspace.as_str()));
        if !theirs {
            return Err(CommandError::new(t!("workbench-error-no-group", group = wanted)));
        }
        group = wanted;
    }
    let tab_id = match &params.path {
        Some(path) => {
            let path = session.state_mut::<Workbench>().resolve(path);
            let hosted = any_file
                || session.state::<Workspaces>().is_none_or(|workspaces| workspaces.hosts(&params.workspace, &path));
            if !hosted {
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                return Err(CommandError::with_code("notHosted", t!("workspace-error-not-hosted", name = name)));
            }
            if params.group.is_none() {
                let holder = session.state::<Workbench>().and_then(|workbench| {
                    let owned = workbench.groups().owned_of(&params.workspace)?;
                    owned.groups().iter().find(|pane| pane.tabs.contains(&path)).map(|pane| pane.id)
                });
                group = holder.unwrap_or(group);
            }
            let origin = history::caret(session);
            crate::open_file_in(session, &path, false, Some(group))?;
            history::left_file(session, origin);
            sync(session);
            let tabs = session.state::<Workspaces>().map(|workspaces| workspaces.tabs(&params.workspace));
            let tab =
                tabs.and_then(|tabs| tabs.iter().find(|tab| tab.path.as_ref() == Some(&path) && tab.group == group));
            tab.map(|tab| tab.id.clone()).ok_or_else(|| unknown(&params.workspace))?
        }
        None => {
            let existing = params.id.clone().filter(|id| {
                session
                    .state::<Workspaces>()
                    .is_some_and(|workspaces| workspaces.tabs(&params.workspace).iter().any(|tab| tab.id == *id))
            });
            match existing {
                Some(id) => id,
                None => {
                    let (Some(kind), Some(title)) = (params.kind.clone(), params.title.clone()) else {
                        return Err(CommandError::new(t!("workspace-error-tab-kind")));
                    };
                    let tabs = session.state_mut::<Workspaces>().tabs.entry(params.workspace.clone()).or_default();
                    let id = params.id.clone().unwrap_or_else(|| tabs.new_id());
                    let tab = InnerTab {
                        id: id.clone(),
                        kind,
                        title,
                        path: None,
                        closable: true,
                        data: serde_json::Value::Null,
                        group,
                    };
                    tabs.tabs.push(tab);
                    id
                }
            }
        }
    };
    let tabs = session.state_mut::<Workspaces>().tabs.entry(params.workspace.clone()).or_default();
    let at = tabs.position(&tab_id).ok_or_else(|| unknown(&params.workspace))?;
    let tab = &mut tabs.tabs[at];
    if let Some(kind) = params.kind {
        tab.kind = kind;
    }
    if let Some(title) = params.title {
        tab.title = title;
    }
    if let Some(closable) = params.closable {
        tab.closable = closable;
    }
    if let Some(data) = params.data {
        tab.data = data;
    }
    if params.group.is_some() && tab.path.is_none() {
        tab.group = group;
    }
    let group = if tab.group == 0 { group } else { tab.group };
    let opened = tab.clone();
    tabs.activate(&tab_id);
    focus(session, group);
    sync(session);
    Ok(opened)
}

/// Which inner tab.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabParams {
    /// The workspace's id, from `workspace.host.list`.
    pub workspace: String,
    /// The tab's id, from `workspace.host.state`.
    pub tab: String,
}

fn find_tab(session: &mut Session, workspace: &str, tab: &str) -> Result<InnerTab, CommandError> {
    known(session, workspace)?;
    sync(session);
    session
        .state::<Workspaces>()
        .and_then(|workspaces| workspaces.tabs(workspace).iter().find(|known| known.id == tab).cloned())
        .ok_or_else(|| CommandError::new(t!("workspace-error-no-tab", tab = tab)))
}

fn show_tab(session: &mut Session, workspace: &str, tab: &InnerTab) {
    let group = match tab.group {
        0 => owned_group(session, workspace),
        group => group,
    };
    if let Some(path) = &tab.path {
        let index = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.groups().group(group)?.tabs.iter().position(|known| known == path));
        if let Some(index) = index {
            let origin = history::caret(session);
            let workbench = session.state_mut::<Workbench>();
            workbench.groups.select(group, index);
            workbench.sync_groups();
            workbench.shown += 1;
            workbench.touch_recent();
            history::left_file(session, origin);
        }
    }
    session.state_mut::<Workspaces>().tabs.entry(workspace.to_owned()).or_default().activate(&tab.id);
    focus(session, group);
    sync(session);
}

pub struct SelectTab;

impl Command for SelectTab {
    type Params = TabParams;
    type Output = WorkspaceState;
    const ID: &'static str = "workspace.tab.select";
    const TITLE: &'static str = "Select Workspace Tab";
    const DESCRIPTION: &'static str = "Makes an inner tab of a workspace active and expands the workspace; a text tab's file becomes the active document. Answers with the workspace's state.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<WorkspaceState, CommandError> {
        let tab = find_tab(session, &params.workspace, &params.tab)?;
        if hidden_from_agent(session, &tab) {
            return Err(CommandError::with_code("agentNotAllowed", t!("workspace-tab-hidden-from-agent")));
        }
        show_tab(session, &params.workspace, &tab);
        state(session, &params.workspace, None)
    }
}

pub struct CloseTab;

impl Command for CloseTab {
    type Params = TabParams;
    type Output = WorkspaceState;
    const ID: &'static str = "workspace.tab.close";
    const TITLE: &'static str = "Close Workspace Tab";
    const DESCRIPTION: &'static str = "Closes an inner tab of a workspace. A text tab's unsaved changes are saved first, as Close Tab saves them; closing an empty console deletes its file. A tab that does not close is refused with the reason. Answers with the workspace's state.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<WorkspaceState, CommandError> {
        close_inner(session, &params.workspace, &params.tab)?;
        state(session, &params.workspace, None)
    }
}

pub(crate) fn close_inner(session: &mut Session, workspace: &str, tab: &str) -> Result<(), CommandError> {
    let found = find_tab(session, workspace, tab)?;
    if !found.closable {
        return Err(CommandError::new(t!("workspace-error-not-closable", title = found.title.as_str())));
    }
    let guard = session.state::<Workspaces>().and_then(|workspaces| {
        workspaces.close_guards.iter().find(|(known, _)| known == workspace).map(|(_, guard)| *guard)
    });
    if let Some(guard) = guard {
        guard(session, workspace, &found)?;
    }
    close_inner_unguarded(session, workspace, tab)
}

pub fn close_inner_unguarded(session: &mut Session, workspace: &str, tab: &str) -> Result<(), CommandError> {
    let tab = find_tab(session, workspace, tab)?;
    if !tab.closable {
        return Err(CommandError::new(t!("workspace-error-not-closable", title = tab.title.as_str())));
    }
    let at = session
        .state::<Workspaces>()
        .and_then(|workspaces| {
            let tabs = workspaces.tabs(workspace).iter().filter(|known| known.group == tab.group);
            tabs.enumerate().find(|(_, known)| known.id == tab.id).map(|(at, _)| at)
        })
        .unwrap_or(0);
    let was_active = session
        .state::<Workspaces>()
        .and_then(|workspaces| workspaces.shown_in(workspace, tab.group))
        .is_some_and(|active| active.id == tab.id);
    match &tab.path {
        Some(path) => {
            let group = if tab.group == 0 { owned_group(session, workspace) } else { tab.group };
            let index = session
                .state::<Workbench>()
                .and_then(|workbench| workbench.groups().group(group)?.tabs.iter().position(|known| known == path));
            if let Some(index) = index {
                closing::close_tab(session, Some(group), Some(index))?;
            }
        }
        None => {
            let tabs = session.state_mut::<Workspaces>().tabs.entry(workspace.to_owned()).or_default();
            tabs.tabs.retain(|known| known.id != tab.id);
        }
    }
    sync(session);
    if was_active {
        let next = session.state::<Workspaces>().and_then(|workspaces| {
            let tabs: Vec<&InnerTab> =
                workspaces.tabs(workspace).iter().filter(|known| known.group == tab.group).collect();
            tabs.get(at.min(tabs.len().saturating_sub(1))).map(|next| (*next).clone())
        });
        if let Some(next) = next {
            let front =
                session.state::<Workbench>().is_some_and(|workbench| workbench.groups().focused() == next.group);
            if front {
                show_tab(session, workspace, &next);
            } else {
                session.state_mut::<Workspaces>().tabs.entry(workspace.to_owned()).or_default().activate(&next.id);
                sync(session);
            }
        }
    }
    Ok(())
}

/// Where an inner tab goes.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveTabParams {
    /// The workspace's id, from `workspace.host.list`.
    pub workspace: String,
    /// The tab's id, from `workspace.host.state`.
    pub tab: String,
    /// Its new place among the tabs of its editor group, from 0; at the end of the group it goes to when absent.
    #[serde(default)]
    pub to: Option<usize>,
    /// The editor group it goes to: another group of the workspace, or a main editor group for a project file the workspace lets go there (`editor.split.list`).
    #[serde(default)]
    pub group: Option<GroupId>,
}

pub struct MoveTab;

impl Command for MoveTab {
    type Params = MoveTabParams;
    type Output = WorkspaceState;
    const ID: &'static str = "workspace.tab.move";
    const TITLE: &'static str = "Move Workspace Tab";
    const DESCRIPTION: &'static str = "Moves an inner tab of a workspace along the tab strip of its editor group, as dragging it does, or with `group` to another of the workspace's groups, which takes the keyboard and shows it. With a main editor group, a project file the workspace lets go there (the Database workspace: `.sql` scripts, not consoles) moves to the code editor and the workspace collapses; anything else is refused with the reason (code `notMovable`). Answers with the workspace's state.";

    fn run(&self, session: &mut Session, params: MoveTabParams) -> Result<WorkspaceState, CommandError> {
        let mut tab = find_tab(session, &params.workspace, &params.tab)?;
        if let Some(group) = params.group.filter(|group| *group != tab.group) {
            move_to_group(session, &params.workspace, &tab, group)?;
            let owner =
                session.state::<Workbench>().and_then(|workbench| workbench.groups().owner(group).map(str::to_owned));
            if owner.as_deref() != Some(params.workspace.as_str()) {
                return state(session, &params.workspace, None);
            }
            tab = find_tab(session, &params.workspace, &params.tab)
                .or_else(|_| moved_file(session, &params.workspace, &tab, group))?;
        }
        if let Some(to) = params.to {
            place(session, &params.workspace, &tab.id, to);
        }
        state(session, &params.workspace, None)
    }
}

fn moved_file(session: &Session, workspace: &str, tab: &InnerTab, group: GroupId) -> Result<InnerTab, CommandError> {
    let tabs = session.state::<Workspaces>().map(|workspaces| workspaces.tabs(workspace)).unwrap_or_default();
    tabs.iter()
        .find(|known| known.group == group && known.path.is_some() && known.path == tab.path)
        .cloned()
        .ok_or_else(|| CommandError::new(t!("workspace-error-no-tab", tab = tab.id.as_str())))
}

fn place(session: &mut Session, workspace: &str, tab: &str, to: usize) {
    let tabs = session.state_mut::<Workspaces>().tabs.entry(workspace.to_owned()).or_default();
    let Some(at) = tabs.position(tab) else { return };
    let moved = tabs.tabs.remove(at);
    let group = moved.group;
    let peers: Vec<usize> =
        tabs.tabs.iter().enumerate().filter(|(_, known)| known.group == group).map(|(index, _)| index).collect();
    let index = match peers.get(to) {
        Some(index) => *index,
        None => peers.last().map_or(tabs.tabs.len(), |last| last + 1),
    };
    tabs.tabs.insert(index.min(tabs.tabs.len()), moved);
    let order: Vec<PathBuf> =
        tabs.tabs.iter().filter(|known| known.group == group).filter_map(|known| known.path.clone()).collect();
    let workbench = session.state_mut::<Workbench>();
    for (to, path) in order.iter().enumerate() {
        let at = workbench.groups.group(group).and_then(|group| group.tabs.iter().position(|tab| tab == path));
        if let Some(at) = at.filter(|at| *at != to) {
            workbench.groups.reorder(group, at, to);
        }
    }
}

fn not_movable(message: String) -> CommandError {
    CommandError::with_code("notMovable", message)
}

pub fn check_move(session: &Session, path: &Path, (from, to): (GroupId, GroupId)) -> Result<(), CommandError> {
    let Some(workbench) = session.state::<Workbench>() else { return Ok(()) };
    let (from, to) = (workbench.groups().owner(from), workbench.groups().owner(to));
    let workspaces = session.state::<Workspaces>();
    let title = |id: &str| {
        workspaces.and_then(|workspaces| workspaces.info(id)).map_or_else(|| id.to_owned(), |info| info.title.clone())
    };
    let name = file_title(path);
    match (from, to) {
        (None, None) => Ok(()),
        (Some(from), Some(to)) if from == to => Ok(()),
        (Some(_), Some(_)) => Err(not_movable(t!("workspace-error-move-between"))),
        (None, Some(id)) | (Some(id), None) => {
            if workspaces.is_some_and(|workspaces| workspaces.accepts(id, session, path)) {
                return Ok(());
            }
            if workspaces.is_some_and(|workspaces| workspaces.is_console(path)) {
                return Err(not_movable(t!("workspace-error-console-stays", file = name.as_str())));
            }
            Err(not_movable(t!("workspace-error-not-taken", file = name.as_str(), workspace = title(id).as_str())))
        }
    }
}

pub fn group_for(session: &mut Session, workspace: &str) -> Result<GroupId, CommandError> {
    known(session, workspace)?;
    Ok(owned_group(session, workspace))
}

fn move_to_group(session: &mut Session, workspace: &str, tab: &InnerTab, group: GroupId) -> Result<(), CommandError> {
    let owner = session.state::<Workbench>().and_then(|workbench| {
        let groups = workbench.groups();
        groups.group(group)?;
        Some(groups.owner(group).map(str::to_owned))
    });
    let Some(owner) = owner else { return Err(CommandError::new(t!("workbench-error-no-group", group = group))) };
    let Some(path) = tab.path.clone() else {
        if owner.as_deref() != Some(workspace) {
            return Err(not_movable(t!("workspace-error-tab-stays", tab = tab.title.as_str())));
        }
        let tabs = session.state_mut::<Workspaces>().tabs.entry(workspace.to_owned()).or_default();
        if let Some(known) = tabs.tabs.iter_mut().find(|known| known.id == tab.id) {
            known.group = group;
        }
        tabs.activate(&tab.id);
        focus(session, group);
        sync(session);
        return Ok(());
    };
    check_move(session, &path, (tab.group, group))?;
    let origin = history::caret(session);
    let workbench = session.state_mut::<Workbench>();
    let index = workbench.groups.group(tab.group).and_then(|from| from.tabs.iter().position(|known| *known == path));
    let Some(index) = index else { return Err(CommandError::new(t!("workspace-error-no-tab", tab = tab.id.as_str()))) };
    if !workbench.groups.move_tab(tab.group, index, group, None) {
        return Err(CommandError::new(t!("workbench-error-no-group", group = group)));
    }
    workbench.sync_groups();
    workbench.shown += 1;
    workbench.touch_recent();
    history::left_file(session, origin);
    sync(session);
    Ok(())
}

pub(crate) fn split_group(
    session: &mut Session,
    workspace: &str,
    (group, index): (GroupId, Option<usize>),
    direction: SplitDirection,
    moved: bool,
) -> Result<(), CommandError> {
    known(session, workspace)?;
    sync(session);
    let path = index.and_then(|index| {
        session.state::<Workbench>().and_then(|workbench| workbench.groups().group(group)?.tabs.get(index).cloned())
    });
    let tab = session.state::<Workspaces>().and_then(|workspaces| match &path {
        Some(path) => {
            workspaces.tabs(workspace).iter().find(|tab| tab.group == group && tab.path.as_ref() == Some(path)).cloned()
        }
        None => workspaces.shown_in(workspace, group).cloned(),
    });
    let Some(tab) = tab else { return Err(CommandError::new(t!("workbench-error-no-split"))) };
    split_tab(session, workspace, &tab, direction, moved)
}

fn split_tab(
    session: &mut Session,
    workspace: &str,
    tab: &InnerTab,
    direction: SplitDirection,
    moved: bool,
) -> Result<(), CommandError> {
    let moving = moved || tab.path.is_none();
    let others = session.state::<Workspaces>().map_or(0, |workspaces| {
        workspaces.tabs(workspace).iter().filter(|known| known.group == tab.group && known.id != tab.id).count()
    });
    if moving && others == 0 {
        return Err(CommandError::new(t!("workspace-error-split-alone", tab = tab.title.as_str())));
    }
    let origin = history::caret(session);
    let workbench = session.state_mut::<Workbench>();
    let Some(new) = workbench.groups.split_empty(tab.group, direction) else {
        return Err(CommandError::new(t!("workbench-error-no-split")));
    };
    match &tab.path {
        Some(path) => {
            let index =
                workbench.groups.group(tab.group).and_then(|from| from.tabs.iter().position(|known| known == path));
            match index.filter(|_| moving) {
                Some(index) => {
                    workbench.groups.move_tab(tab.group, index, new, None);
                }
                None => {
                    workbench.groups.open_in(path, new);
                }
            }
        }
        None => {
            let tabs = session.state_mut::<Workspaces>().tabs.entry(workspace.to_owned()).or_default();
            if let Some(known) = tabs.tabs.iter_mut().find(|known| known.id == tab.id) {
                known.group = new;
            }
            tabs.activate(&tab.id);
        }
    }
    let workbench = session.state_mut::<Workbench>();
    workbench.sync_groups();
    workbench.shown += 1;
    history::left_file(session, origin);
    focus(session, new);
    sync(session);
    Ok(())
}

/// Which inner tab to split off, and how.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitTabParams {
    /// The workspace's id, from `workspace.host.list`.
    pub workspace: String,
    /// The tab's id, from `workspace.host.state`.
    pub tab: String,
    /// Where the new editor group goes: `right` or `down`.
    pub direction: SplitDirection,
    /// Split and Move: a text tab leaves its old group; a tab the feature draws always does.
    #[serde(default, rename = "move")]
    pub moved: bool,
}

pub struct SplitTab;

impl Command for SplitTab {
    type Params = SplitTabParams;
    type Output = WorkspaceState;
    const ID: &'static str = "workspace.tab.split";
    const TITLE: &'static str = "Split Workspace Tab";
    const DESCRIPTION: &'static str = "Splits the editor group of a workspace's inner tab, right or down, as Split Right and Split Down do in the main editor: a new group of the workspace shows the tab and takes the keyboard. A text tab stays in its old group too unless `move`; a tab the feature draws always moves, and is refused when it is the only tab of its group. Answers with the workspace's state.";

    fn run(&self, session: &mut Session, params: SplitTabParams) -> Result<WorkspaceState, CommandError> {
        let tab = find_tab(session, &params.workspace, &params.tab)?;
        if hidden_from_agent(session, &tab) {
            return Err(CommandError::with_code("agentNotAllowed", t!("workspace-tab-hidden-from-agent")));
        }
        split_tab(session, &params.workspace, &tab, params.direction, params.moved)?;
        state(session, &params.workspace, None)
    }
}

#[cfg(test)]
mod tests;

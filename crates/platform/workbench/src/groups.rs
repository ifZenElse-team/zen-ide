use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::tabs::{ActivateOnClose, EditorTabSettings, OverLimit};
use crate::{Workbench, history};

pub type GroupId = u64;

pub const OWNED_FIRST: GroupId = 1 << 32;

pub const WINDOW_FIRST: GroupId = 1 << 40;

pub const OPEN_IN_MAIN_WINDOW: &str = "editor.detachedWindow.openInMain";

const RATIO: std::ops::RangeInclusive<f32> = 0.1..=0.9;

/// Where a split puts the new editor group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SplitDirection {
    /// Side by side, the new group on the right.
    Right,
    /// One above the other, the new group below.
    Down,
}

/// One editor group: a row of tabs and the editor of the active one.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EditorGroup {
    /// The group's id.
    pub id: GroupId,
    /// The files of its tabs, in order.
    pub tabs: Vec<PathBuf>,
    /// The index of the active tab.
    pub active: Option<usize>,
    /// The files of its pinned tabs, which stay at the start of the row.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub pinned: Vec<PathBuf>,
    /// The file of its preview tab, the one tab the next previewed file takes over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<PathBuf>,
    #[serde(skip)]
    pub(crate) used: Vec<PathBuf>,
    #[serde(skip)]
    pub(crate) order: TabOrder,
}

impl PartialEq for EditorGroup {
    fn eq(&self, other: &Self) -> bool {
        (self.id, &self.tabs, self.active, &self.pinned, &self.preview)
            == (other.id, &other.tabs, other.active, &other.pinned, &other.preview)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TabOrder {
    sorted: bool,
    pins_stay: bool,
}

impl EditorGroup {
    pub fn active_path(&self) -> Option<&Path> {
        self.active.and_then(|index| self.tabs.get(index)).map(PathBuf::as_path)
    }

    fn activate(&mut self, path: &Path) {
        self.active = self.tabs.iter().position(|tab| tab == path);
        self.note_use(path);
    }

    fn note_use(&mut self, path: &Path) {
        self.used.retain(|used| used != path);
        self.used.push(path.to_owned());
    }

    fn least_used(&self) -> impl Iterator<Item = &PathBuf> {
        let unused = self.tabs.iter().filter(|tab| !self.used.contains(tab));
        unused.chain(self.used.iter().filter(|used| self.tabs.contains(used)))
    }

    fn after_close(&self, index: usize, activate: ActivateOnClose) -> usize {
        let last = self.tabs.len() - 1;
        match activate {
            ActivateOnClose::Left => index.saturating_sub(1).min(last),
            ActivateOnClose::Right => index.min(last),
            ActivateOnClose::Recent => self
                .used
                .iter()
                .rev()
                .find_map(|used| self.tabs.iter().position(|tab| tab == used))
                .unwrap_or(index.saturating_sub(1).min(last)),
        }
    }

    fn insertion(&self, at_end: bool) -> usize {
        match self.active.filter(|_| !at_end) {
            Some(active) => (active + 1).min(self.tabs.len()),
            None => self.tabs.len(),
        }
    }

    pub fn is_preview(&self, path: &Path) -> bool {
        self.preview.as_deref() == Some(path)
    }

    fn has(&self, path: &Path) -> bool {
        self.tabs.iter().any(|tab| tab == path)
    }

    pub(crate) fn has_tab(&self, path: &Path) -> bool {
        self.has(path)
    }

    pub fn is_pinned(&self, path: &Path) -> bool {
        self.pinned.iter().any(|pinned| pinned == path)
    }

    pub fn is_sorted(&self) -> bool {
        self.order.sorted
    }

    pub fn pinned_count(&self) -> usize {
        self.tabs.iter().filter(|tab| self.is_pinned(tab)).count()
    }

    fn tidy(&mut self) {
        let tabs = &self.tabs;
        self.pinned.retain(|pinned| tabs.contains(pinned));
        self.preview = self.preview.take().filter(|preview| tabs.contains(preview));
        let active = self.active_path().map(Path::to_owned);
        if self.order.sorted {
            self.tabs.sort_by_cached_key(|tab| sort_key(tab));
        }
        if !self.order.pins_stay {
            let (mut pinned, rest): (Vec<PathBuf>, Vec<PathBuf>) =
                std::mem::take(&mut self.tabs).into_iter().partition(|tab| self.pinned.contains(tab));
            pinned.extend(rest);
            self.tabs = pinned;
        }
        if let Some(active) = active {
            self.active = self.tabs.iter().position(|tab| *tab == active);
        }
    }

    fn ordered_by(&mut self, order: TabOrder) {
        if self.order != order {
            self.order = order;
            self.tidy();
        }
    }
}

fn sort_key(tab: &Path) -> (String, PathBuf) {
    let name = tab.file_name().map(|name| name.to_string_lossy().to_lowercase()).unwrap_or_default();
    (name, tab.to_owned())
}

/// A node of the split tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SplitNode {
    /// An editor group.
    Group(EditorGroup),
    /// Two parts side by side or one above the other.
    #[serde(rename_all = "camelCase")]
    Split {
        /// Where the second part is.
        direction: SplitDirection,
        /// The share of the first part, from 0.1 to 0.9.
        ratio: f32,
        /// The left or upper part.
        first: Box<SplitNode>,
        /// The right or lower part.
        second: Box<SplitNode>,
    },
}

impl SplitNode {
    fn leaves<'a>(&'a self, out: &mut Vec<&'a EditorGroup>) {
        match self {
            SplitNode::Group(group) => out.push(group),
            SplitNode::Split { first, second, .. } => {
                first.leaves(out);
                second.leaves(out);
            }
        }
    }

    fn group(&self, id: GroupId) -> Option<&EditorGroup> {
        match self {
            SplitNode::Group(group) => (group.id == id).then_some(group),
            SplitNode::Split { first, second, .. } => first.group(id).or_else(|| second.group(id)),
        }
    }

    fn group_mut(&mut self, id: GroupId) -> Option<&mut EditorGroup> {
        match self {
            SplitNode::Group(group) => (group.id == id).then_some(group),
            SplitNode::Split { first, second, .. } => first.group_mut(id).or_else(|| second.group_mut(id)),
        }
    }

    fn first_id(&self) -> GroupId {
        match self {
            SplitNode::Group(group) => group.id,
            SplitNode::Split { first, .. } => first.first_id(),
        }
    }

    fn each_group(&mut self, visit: &mut impl FnMut(&mut EditorGroup)) {
        match self {
            SplitNode::Group(group) => visit(group),
            SplitNode::Split { first, second, .. } => {
                first.each_group(visit);
                second.each_group(visit);
            }
        }
    }

    fn take_tabs(self, out: &mut Vec<(PathBuf, bool)>) {
        match self {
            SplitNode::Group(group) => {
                let pinned = group.pinned;
                out.extend(group.tabs.into_iter().map(|tab| {
                    let pin = pinned.contains(&tab);
                    (tab, pin)
                }));
            }
            SplitNode::Split { first, second, .. } => {
                first.take_tabs(out);
                second.take_tabs(out);
            }
        }
    }

    fn without(self, id: GroupId) -> Option<SplitNode> {
        match self {
            SplitNode::Group(group) if group.id == id => None,
            SplitNode::Group(group) => Some(SplitNode::Group(group)),
            SplitNode::Split { direction, ratio, first, second } => match (first.without(id), second.without(id)) {
                (Some(first), Some(second)) => {
                    Some(SplitNode::Split { direction, ratio, first: Box::new(first), second: Box::new(second) })
                }
                (Some(only), None) | (None, Some(only)) => Some(only),
                (None, None) => None,
            },
        }
    }

    fn split_at(
        &mut self,
        id: GroupId,
        direction: SplitDirection,
        new: Box<EditorGroup>,
    ) -> Result<(), Box<EditorGroup>> {
        match self {
            SplitNode::Group(group) if group.id == id => {
                let old = std::mem::take(group);
                *self = SplitNode::Split {
                    direction,
                    ratio: 0.5,
                    first: Box::new(SplitNode::Group(old)),
                    second: Box::new(SplitNode::Group(*new)),
                };
                Ok(())
            }
            SplitNode::Group(_) => Err(new),
            SplitNode::Split { first, second, .. } => {
                first.split_at(id, direction, new).or_else(|new| second.split_at(id, direction, new))
            }
        }
    }

    fn unsplit(&mut self, id: GroupId) -> bool {
        let SplitNode::Split { first, second, .. } = self else { return false };
        let is = |node: &SplitNode| matches!(node, SplitNode::Group(group) if group.id == id);
        if is(first) || is(second) {
            let (kept, other) = if is(first) { (first, second) } else { (second, first) };
            let SplitNode::Group(mut group) = std::mem::replace(kept.as_mut(), placeholder()) else {
                return false;
            };
            let mut tabs = Vec::new();
            std::mem::replace(other.as_mut(), placeholder()).take_tabs(&mut tabs);
            merge(&mut group, tabs);
            *self = SplitNode::Group(group);
            return true;
        }
        first.unsplit(id) || second.unsplit(id)
    }

    fn resize(&mut self, before: GroupId, wanted: f32) -> bool {
        let SplitNode::Split { ratio, first, second, .. } = self else { return false };
        if second.first_id() == before {
            *ratio = wanted.clamp(*RATIO.start(), *RATIO.end());
            return true;
        }
        first.resize(before, wanted) || second.resize(before, wanted)
    }

    fn relocate(&mut self, from: &Path, to: &Path) {
        match self {
            SplitNode::Group(group) => relocate_tabs(group, from, to),
            SplitNode::Split { first, second, .. } => {
                first.relocate(from, to);
                second.relocate(from, to);
            }
        }
    }

    fn retain(&mut self, keep: &impl Fn(&Path) -> bool) {
        match self {
            SplitNode::Group(group) => retain_tabs(group, keep),
            SplitNode::Split { first, second, .. } => {
                first.retain(keep);
                second.retain(keep);
            }
        }
    }
}

fn placeholder() -> SplitNode {
    SplitNode::Group(EditorGroup::default())
}

fn merge(group: &mut EditorGroup, tabs: Vec<(PathBuf, bool)>) {
    for (tab, pinned) in tabs {
        if pinned && !group.is_pinned(&tab) {
            group.pinned.push(tab.clone());
        }
        if !group.tabs.contains(&tab) {
            group.tabs.push(tab);
        }
    }
    if group.active.is_none() && !group.tabs.is_empty() {
        group.active = Some(0);
    }
    group.tidy();
}

/// The editor groups a workspace owns, kept outside the main split tree: one, or several in a split tree of their own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OwnedGroup {
    /// The id of the workspace that owns the groups (`workspace.host.list`).
    pub workspace: String,
    /// Its groups in their split tree, each with its stable id, its tabs and its active tab.
    pub root: SplitNode,
    /// The id of its group that has, or last had, the keyboard.
    pub focused: GroupId,
}

impl OwnedGroup {
    fn new(workspace: &str, id: GroupId) -> Self {
        let group = EditorGroup { id, ..EditorGroup::default() };
        Self { workspace: workspace.to_owned(), root: SplitNode::Group(group), focused: id }
    }

    pub fn group(&self) -> &EditorGroup {
        self.root.group(self.focused).unwrap_or_else(|| self.groups()[0])
    }

    pub fn groups(&self) -> Vec<&EditorGroup> {
        let mut out = Vec::new();
        self.root.leaves(&mut out);
        out
    }

    pub fn is_split(&self) -> bool {
        matches!(self.root, SplitNode::Split { .. })
    }
}

/// Where a detached editor window is on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowBounds {
    /// The left edge, in points.
    pub x: f32,
    /// The top edge, in points.
    pub y: f32,
    /// The width of its content, in points.
    pub width: f32,
    /// The height of its content, in points.
    pub height: f32,
}

impl WindowBounds {
    fn usable(self) -> bool {
        [self.x, self.y, self.width, self.height].iter().all(|value| value.is_finite())
            && self.width >= 1.0
            && self.height >= 1.0
    }
}

/// An editor tab row in an operating system window of its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetachedWindow {
    /// Its editor group: the tabs and the active tab.
    pub group: EditorGroup,
    /// Where it is; the system chooses when absent.
    pub bounds: Option<WindowBounds>,
}

/// The editor groups of a window in their split tree, the groups workspaces own, and the focused one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditorGroups {
    /// The split tree.
    root: SplitNode,
    /// The id of the group that has the keyboard: its active tab is the active document.
    focused: GroupId,
    /// The main group that last had the keyboard: new tabs go there, and collapsing a workspace focuses it.
    #[serde(default)]
    main_focused: GroupId,
    /// The groups workspaces own, outside the split tree; never removed when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    workspaces: Vec<OwnedGroup>,
    #[serde(skip)]
    #[schemars(skip)]
    policy: EditorTabSettings,
    /// The editor windows detached from the main window.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    windows: Vec<DetachedWindow>,
}

impl Default for EditorGroups {
    fn default() -> Self {
        Self {
            root: SplitNode::Group(EditorGroup { id: 1, ..EditorGroup::default() }),
            focused: 1,
            main_focused: 1,
            workspaces: Vec::new(),
            policy: EditorTabSettings::default(),
            windows: Vec::new(),
        }
    }
}

impl EditorGroups {
    pub fn root(&self) -> &SplitNode {
        &self.root
    }

    pub fn focused(&self) -> GroupId {
        self.focused
    }

    pub fn focused_group(&self) -> &EditorGroup {
        self.group(self.focused).unwrap_or_else(|| self.groups()[0])
    }

    pub fn main_focused(&self) -> GroupId {
        if self.root.group(self.main_focused).is_some() {
            self.main_focused
        } else if self.root.group(self.focused).is_some() {
            self.focused
        } else {
            self.root.first_id()
        }
    }

    pub fn is_split(&self) -> bool {
        matches!(self.root, SplitNode::Split { .. })
    }

    pub fn groups(&self) -> Vec<&EditorGroup> {
        let mut out = Vec::new();
        self.root.leaves(&mut out);
        out
    }

    pub fn group(&self, id: GroupId) -> Option<&EditorGroup> {
        self.root
            .group(id)
            .or_else(|| self.workspaces.iter().find_map(|owned| owned.root.group(id)))
            .or_else(|| self.window(id).map(|window| &window.group))
    }

    pub fn windows(&self) -> &[DetachedWindow] {
        &self.windows
    }

    pub fn window(&self, id: GroupId) -> Option<&DetachedWindow> {
        self.windows.iter().find(|window| window.group.id == id)
    }

    pub fn detach(&mut self, from: GroupId, index: usize, bounds: Option<WindowBounds>) -> Option<GroupId> {
        let group = self.group(from)?;
        let path = group.tabs.get(index)?.clone();
        if self.window(from).is_some() && group.tabs.len() == 1 {
            return None;
        }
        let id = self.windows.iter().map(|window| window.group.id).max().unwrap_or(WINDOW_FIRST - 1) + 1;
        self.close(from, index);
        let group = EditorGroup { id, tabs: vec![path], active: Some(0), ..EditorGroup::default() };
        self.windows.push(DetachedWindow { group, bounds: bounds.filter(|bounds| bounds.usable()) });
        self.set_focus(id);
        Some(id)
    }

    pub fn close_window(&mut self, id: GroupId) -> bool {
        let Some(at) = self.windows.iter().position(|window| window.group.id == id) else { return false };
        let window = self.windows.remove(at);
        let tabs = window.group.tabs.iter().map(|tab| (tab.clone(), window.group.is_pinned(tab))).collect();
        merge(self.main_mut(), tabs);
        if self.focused == id {
            self.set_focus(self.main_focused());
        }
        true
    }

    fn remove_window(&mut self, id: GroupId) {
        self.windows.retain(|window| window.group.id != id);
        if self.focused == id {
            self.set_focus(self.main_focused());
        }
    }

    pub fn set_window_bounds(&mut self, id: GroupId, bounds: WindowBounds) -> bool {
        let found = self.windows.iter_mut().find(|window| window.group.id == id);
        let Some(window) = found.filter(|window| window.bounds != Some(bounds) && bounds.usable()) else {
            return false;
        };
        window.bounds = Some(bounds);
        true
    }

    pub fn with_windows(mut self, windows: Vec<DetachedWindow>, open: impl Fn(&Path) -> bool) -> Self {
        let mut seen: Vec<GroupId> = Vec::new();
        for mut window in windows {
            let id = window.group.id;
            let active = window.group.active_path().map(Path::to_owned);
            window.group.tabs.retain(|tab| open(tab));
            if id < WINDOW_FIRST || seen.contains(&id) || window.group.tabs.is_empty() {
                continue;
            }
            seen.push(id);
            window.group.active =
                active.and_then(|path| window.group.tabs.iter().position(|tab| *tab == path)).or(Some(0));
            window.bounds = window.bounds.filter(|bounds| bounds.usable());
            self.windows.push(window);
        }
        let held: Vec<PathBuf> = self.windows.iter().flat_map(|window| window.group.tabs.clone()).collect();
        self.root.retain(&|tab: &Path| !held.iter().any(|path| path == tab));
        self
    }

    fn group_mut(&mut self, id: GroupId) -> Option<&mut EditorGroup> {
        match self.root.group_mut(id) {
            Some(group) => Some(group),
            None => self
                .workspaces
                .iter_mut()
                .find_map(|owned| owned.root.group_mut(id))
                .or_else(|| self.windows.iter_mut().map(|window| &mut window.group).find(|group| group.id == id)),
        }
    }

    pub fn owned_groups(&self) -> &[OwnedGroup] {
        &self.workspaces
    }

    pub fn owned(&self, workspace: &str) -> Option<&EditorGroup> {
        self.owned_of(workspace).map(OwnedGroup::group)
    }

    pub fn owned_of(&self, workspace: &str) -> Option<&OwnedGroup> {
        self.workspaces.iter().find(|owned| owned.workspace == workspace)
    }

    fn owning_mut(&mut self, id: GroupId) -> Option<&mut OwnedGroup> {
        self.workspaces.iter_mut().find(|owned| owned.root.group(id).is_some())
    }

    pub fn owner(&self, id: GroupId) -> Option<&str> {
        self.workspaces.iter().find(|owned| owned.root.group(id).is_some()).map(|owned| owned.workspace.as_str())
    }

    pub fn is_split_here(&self) -> bool {
        match self.workspaces.iter().find(|owned| owned.root.group(self.focused).is_some()) {
            Some(owned) => owned.is_split(),
            None => self.is_split(),
        }
    }

    pub fn expanded(&self) -> Option<&str> {
        self.owner(self.focused)
    }

    pub fn own(&mut self, workspace: &str) -> GroupId {
        if let Some(group) = self.owned(workspace) {
            return group.id;
        }
        let id = self.next_owned_id();
        self.workspaces.push(OwnedGroup::new(workspace, id));
        id
    }

    fn next_owned_id(&self) -> GroupId {
        let owned = self.workspaces.iter().flat_map(|owned| owned.groups()).map(|group| group.id);
        owned.max().unwrap_or(OWNED_FIRST - 1) + 1
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.all().any(|group| group.has(path))
    }

    pub fn crosses_workspaces(&self, from: GroupId, to: GroupId) -> bool {
        self.owner(from) != self.owner(to)
    }

    fn all(&self) -> impl Iterator<Item = &EditorGroup> {
        let windows = self.windows.iter().map(|window| &window.group);
        self.groups().into_iter().chain(self.workspaces.iter().flat_map(OwnedGroup::groups)).chain(windows)
    }

    fn ids(&self) -> Vec<GroupId> {
        self.groups().iter().map(|group| group.id).collect()
    }

    fn next_id(&self) -> GroupId {
        self.ids().into_iter().max().unwrap_or(0) + 1
    }

    fn set_focus(&mut self, id: GroupId) {
        self.focused = id;
        if self.root.group(id).is_some() {
            self.main_focused = id;
        } else if let Some(owned) = self.owning_mut(id) {
            owned.focused = id;
        }
    }

    fn main_mut(&mut self) -> &mut EditorGroup {
        let id = self.main_focused();
        self.main_focused = id;
        self.root.group_mut(id).expect("the main focused group exists")
    }

    pub(crate) fn follow_policy(&mut self, policy: &EditorTabSettings) {
        self.policy = policy.clone();
        let order = TabOrder { sorted: policy.sort_alphabetically, pins_stay: !policy.keep_pinned_left };
        self.root.each_group(&mut |group| group.ordered_by(order));
        for owned in &mut self.workspaces {
            owned.root.each_group(&mut |group| group.ordered_by(order));
        }
    }

    fn add_tab(&mut self, id: GroupId, path: &Path) {
        let at_end = self.policy.open_at_end;
        let Some(group) = self.group_mut(id) else { return };
        let at = group.insertion(at_end);
        group.tabs.insert(at, path.to_owned());
        group.tidy();
    }

    pub fn open(&mut self, path: &Path) {
        let main = self.main_focused();
        let holder = if self.group(self.focused).is_some_and(|group| group.has(path)) {
            Some(self.focused)
        } else if self.root.group(main).is_some_and(|group| group.has(path)) {
            Some(main)
        } else {
            self.all().find(|group| group.has(path)).map(|group| group.id)
        };
        let id = match holder {
            Some(id) => id,
            None => {
                let id = self.main_mut().id;
                self.add_tab(id, path);
                id
            }
        };
        self.set_focus(id);
        if let Some(group) = self.group_mut(id) {
            group.activate(path);
        }
    }

    pub fn open_in(&mut self, path: &Path, id: GroupId) -> bool {
        let Some(group) = self.group(id) else { return false };
        if !group.has(path) {
            self.add_tab(id, path);
        }
        if let Some(group) = self.group_mut(id) {
            group.activate(path);
        }
        self.set_focus(id);
        true
    }

    pub fn select(&mut self, id: GroupId, index: usize) -> bool {
        let Some(group) = self.group_mut(id).filter(|group| index < group.tabs.len()) else { return false };
        group.active = Some(index);
        let path = group.tabs[index].clone();
        group.note_use(&path);
        self.set_focus(id);
        true
    }

    pub fn focus(&mut self, id: GroupId) -> bool {
        let found = self.group(id).is_some();
        if found {
            self.set_focus(id);
        }
        found
    }

    pub fn focus_next(&mut self, forward: bool) -> bool {
        let here = self
            .workspaces
            .iter()
            .find(|owned| owned.root.group(self.focused).is_some() && owned.is_split())
            .map(|owned| owned.groups().iter().map(|group| group.id).collect::<Vec<_>>());
        if let Some(ids) = here {
            let at = ids.iter().position(|id| *id == self.focused).unwrap_or(0);
            let next = if forward { (at + 1) % ids.len() } else { (at + ids.len() - 1) % ids.len() };
            self.set_focus(ids[next]);
            return true;
        }
        let ids = self.ids();
        let from = if self.root.group(self.focused).is_some() { self.focused } else { self.main_focused() };
        let Some(at) = ids.iter().position(|id| *id == from) else { return false };
        let next = if forward { (at + 1) % ids.len() } else { (at + ids.len() - 1) % ids.len() };
        self.set_focus(ids[next]);
        ids.len() > 1
    }

    pub fn split(
        &mut self,
        id: GroupId,
        index: Option<usize>,
        direction: SplitDirection,
        moved: bool,
    ) -> Option<GroupId> {
        let group = self.group(id)?;
        let index = index.or(group.active).filter(|index| *index < group.tabs.len())?;
        if moved && group.tabs.len() < 2 {
            return None;
        }
        let path = group.tabs[index].clone();
        let owned = self.owner(id).is_some();
        let new_id = if owned { self.next_owned_id() } else { self.next_id() };
        let new = EditorGroup { id: new_id, tabs: vec![path], active: Some(0), ..EditorGroup::default() };
        match self.owning_mut(id) {
            Some(owner) => owner.root.split_at(id, direction, Box::new(new)).ok()?,
            None => self.root.split_at(id, direction, Box::new(new)).ok()?,
        }
        if moved {
            self.close(id, index);
        }
        self.set_focus(new_id);
        Some(new_id)
    }

    pub fn split_empty(&mut self, id: GroupId, direction: SplitDirection) -> Option<GroupId> {
        let new_id = self.next_owned_id();
        let owner = self.owning_mut(id)?;
        owner.root.split_at(id, direction, Box::new(EditorGroup { id: new_id, ..EditorGroup::default() })).ok()?;
        self.set_focus(new_id);
        Some(new_id)
    }

    pub fn remove_owned(&mut self, id: GroupId) -> bool {
        let Some(owner) = self.owning_mut(id).filter(|owner| owner.is_split()) else { return false };
        let ids: Vec<GroupId> = owner.groups().iter().map(|group| group.id).collect();
        let at = ids.iter().position(|other| *other == id).unwrap_or(0);
        let tabs = owner
            .root
            .group(id)
            .map(|group| group.tabs.iter().map(|tab| (tab.clone(), group.is_pinned(tab))).collect())
            .unwrap_or_default();
        let Some(root) = std::mem::replace(&mut owner.root, placeholder()).without(id) else { return false };
        owner.root = root;
        let near = owner.groups()[at.saturating_sub(1).min(owner.groups().len() - 1)].id;
        if owner.focused == id {
            owner.focused = near;
        }
        if let Some(group) = owner.root.group_mut(near) {
            merge(group, tabs);
        }
        if self.focused == id {
            self.set_focus(near);
        }
        true
    }

    pub fn lay_out(&mut self, workspace: &str, layout: &SplitNode) -> Vec<(GroupId, GroupId)> {
        let busy =
            self.owned_of(workspace).is_some_and(|owned| owned.groups().iter().any(|group| !group.tabs.is_empty()));
        if busy || !matches!(layout, SplitNode::Split { .. }) {
            return Vec::new();
        }
        let old: Vec<GroupId> = self
            .owned_of(workspace)
            .map(|owned| owned.groups().iter().map(|group| group.id).collect())
            .unwrap_or_default();
        self.workspaces.retain(|owned| owned.workspace != workspace);
        let mut next = self.next_owned_id();
        let mut pairs = Vec::new();
        let mut root = layout.clone();
        renumber_owned(&mut root, &mut next, &mut pairs);
        let first = pairs[0].1;
        self.workspaces.push(OwnedGroup { workspace: workspace.to_owned(), root, focused: first });
        if old.contains(&self.focused) {
            self.focused = first;
        }
        pairs
    }

    pub fn close(&mut self, id: GroupId, index: usize) -> Option<PathBuf> {
        let activate = self.policy.activate_on_close;
        let group = self.group_mut(id).filter(|group| index < group.tabs.len())?;
        let closed = group.tabs.remove(index);
        group.pinned.retain(|pinned| *pinned != closed);
        group.used.retain(|used| *used != closed);
        group.preview = group.preview.take().filter(|preview| *preview != closed);
        group.active = match group.active {
            _ if group.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => {
                let next = group.after_close(index, activate);
                let path = group.tabs[next].clone();
                group.note_use(&path);
                Some(next)
            }
            Some(active) => Some(active),
            None => None,
        };
        let emptied = group.tabs.is_empty();
        if emptied && self.window(id).is_some() {
            self.remove_window(id);
        } else if emptied && self.is_split() && self.root.group(id).is_some() {
            self.remove(id);
        }
        Some(closed)
    }

    fn remove(&mut self, id: GroupId) {
        let ids = self.ids();
        let at = ids.iter().position(|other| *other == id).unwrap_or(0);
        if let Some(root) = std::mem::replace(&mut self.root, placeholder()).without(id) {
            self.root = root;
        }
        let ids = self.ids();
        let near = ids[at.saturating_sub(1).min(ids.len() - 1)];
        if self.root.group(self.main_focused).is_none() {
            self.main_focused = near;
        }
        if self.focused == id || self.group(self.focused).is_none() {
            self.set_focus(near);
        }
    }

    pub fn unsplit(&mut self) -> bool {
        let focused = self.focused;
        if let Some(owner) = self.owning_mut(focused) {
            return owner.root.unsplit(focused);
        }
        let id = self.main_focused();
        self.root.unsplit(id)
    }

    pub fn unsplit_all(&mut self) -> bool {
        let focused = self.focused;
        if let Some(owner) = self.owning_mut(focused) {
            if !owner.is_split() {
                return false;
            }
            let Some(mut group) = owner.root.group(focused).cloned() else { return false };
            let mut tabs = Vec::new();
            std::mem::replace(&mut owner.root, placeholder()).take_tabs(&mut tabs);
            merge(&mut group, tabs);
            owner.root = SplitNode::Group(group);
            return true;
        }
        if !self.is_split() {
            return false;
        }
        let Some(mut group) = self.root.group(self.main_focused()).cloned() else { return false };
        let mut tabs = Vec::new();
        std::mem::replace(&mut self.root, placeholder()).take_tabs(&mut tabs);
        merge(&mut group, tabs);
        let id = group.id;
        self.root = SplitNode::Group(group);
        self.main_focused = id;
        if self.group(self.focused).is_none() {
            self.focused = id;
        }
        true
    }

    pub fn move_tab(&mut self, from: GroupId, index: usize, to: GroupId, at: Option<usize>) -> bool {
        if from == to || self.group(to).is_none() {
            return false;
        }
        let Some(path) = self.group(from).and_then(|group| group.tabs.get(index)).cloned() else { return false };
        let pinned = self.group(from).is_some_and(|group| group.is_pinned(&path));
        self.close(from, index);
        let Some(target) = self.group_mut(to) else { return false };
        if !target.has(&path) {
            let at = at.unwrap_or(target.tabs.len()).min(target.tabs.len());
            target.tabs.insert(at, path.clone());
        }
        if pinned && !target.is_pinned(&path) {
            target.pinned.push(path.clone());
        }
        target.activate(&path);
        target.tidy();
        self.set_focus(to);
        true
    }

    pub fn reorder(&mut self, id: GroupId, index: usize, to: usize) -> bool {
        let Some(group) = self.group_mut(id).filter(|group| index < group.tabs.len()) else { return false };
        let active = group.active_path().map(Path::to_owned);
        let tab = group.tabs.remove(index);
        let to = to.min(group.tabs.len());
        group.tabs.insert(to, tab);
        if let Some(active) = active {
            group.activate(&active);
        }
        group.tidy();
        true
    }

    pub fn move_in_row(&mut self, id: GroupId, index: usize, to: usize) -> Option<usize> {
        let group = self.group(id).filter(|group| index < group.tabs.len() && !group.order.sorted)?;
        let pinned = group.pinned_count();
        let to = match (group.order.pins_stay, group.is_pinned(&group.tabs[index])) {
            (true, _) => to,
            (false, true) => to.min(pinned.saturating_sub(1)),
            (false, false) => to.max(pinned),
        };
        let to = to.min(group.tabs.len() - 1);
        self.reorder(id, index, to).then_some(to)
    }

    pub fn pin(&mut self, id: GroupId, index: usize, pinned: Option<bool>) -> Option<bool> {
        let group = self.group_mut(id).filter(|group| index < group.tabs.len())?;
        let path = group.tabs[index].clone();
        let pin = pinned.unwrap_or(!group.is_pinned(&path));
        group.pinned.retain(|other| *other != path);
        if pin {
            group.pinned.push(path.clone());
        }
        if pin {
            group.preview = group.preview.take().filter(|preview| *preview != path);
        }
        if !group.order.pins_stay {
            let active = group.active_path().map(Path::to_owned);
            let tab = group.tabs.remove(index);
            let first_free = group.tabs.iter().take_while(|tab| group.pinned.contains(tab)).count();
            group.tabs.insert(first_free, tab);
            if let Some(active) = active {
                group.activate(&active);
            }
        }
        group.tidy();
        Some(pin)
    }

    pub(crate) fn mark_preview(&mut self, id: GroupId, path: &Path) -> Option<PathBuf> {
        let group = self.group_mut(id)?;
        let old = group.preview.take().filter(|old| old != path);
        group.preview = Some(path.to_owned());
        let old = old?;
        let from = group.tabs.iter().position(|tab| *tab == old)?;
        let to = group.tabs.iter().position(|tab| tab == path)?;
        group.tabs.swap(from, to);
        group.tidy();
        group.activate(path);
        Some(old)
    }

    pub(crate) fn close_path(&mut self, id: GroupId, path: &Path) {
        let index = self.group(id).and_then(|group| group.tabs.iter().position(|tab| tab == path));
        if let Some(index) = index {
            self.close(id, index);
        }
    }

    pub(crate) fn keep_open(&mut self, id: GroupId, path: &Path) -> bool {
        let Some(group) = self.group_mut(id).filter(|group| group.is_preview(path)) else { return false };
        group.preview = None;
        true
    }

    pub(crate) fn keep_everywhere(&mut self, path: &Path) {
        let ids: Vec<GroupId> = self.all().map(|group| group.id).collect();
        for id in ids {
            self.keep_open(id, path);
        }
    }

    pub(crate) fn replace_tab(&mut self, id: GroupId, old: &Path, new: &Path) -> bool {
        let Some(group) = self.group_mut(id) else { return false };
        let (Some(from), Some(to)) =
            (group.tabs.iter().position(|tab| tab == old), group.tabs.iter().position(|tab| tab == new))
        else {
            return false;
        };
        group.tabs.swap(from, to);
        group.tidy();
        group.activate(new);
        true
    }

    pub(crate) fn to_close_for_limit(&self, id: GroupId, dirty: impl Fn(&Path) -> bool) -> Option<usize> {
        let group = self.group(id)?;
        let closable = group.tabs.iter().filter(|tab| !group.is_pinned(tab)).count();
        let pinned = group.tabs.len() - closable;
        if group.tabs.len() <= self.policy.limit.max(1) || closable == 0 || pinned >= self.policy.limit.max(1) {
            return None;
        }
        let active = group.active_path();
        let candidates = || group.least_used().filter(|tab| !group.is_pinned(tab) && Some(tab.as_path()) != active);
        let unchanged = (self.policy.over_limit == OverLimit::CloseUnchanged)
            .then(|| candidates().find(|tab| !dirty(tab)))
            .flatten();
        let path = unchanged.or_else(|| candidates().next())?;
        group.tabs.iter().position(|tab| tab == path)
    }

    pub fn opposite(&self, id: GroupId) -> Option<GroupId> {
        let ids: Vec<GroupId> = match self.workspaces.iter().find(|owned| owned.root.group(id).is_some()) {
            Some(owned) => owned.groups().iter().map(|group| group.id).collect(),
            None => self.ids(),
        };
        let at = ids.iter().position(|other| *other == id)?;
        (ids.len() > 1).then(|| ids[(at + 1) % ids.len()])
    }

    pub fn has_pinned(&self) -> bool {
        self.all().any(|group| !group.pinned.is_empty())
    }

    pub fn resize(&mut self, before: GroupId, ratio: f32) -> bool {
        ratio.is_finite()
            && (self.root.resize(before, ratio)
                || self.workspaces.iter_mut().any(|owned| owned.root.resize(before, ratio)))
    }

    pub fn relocate(&mut self, from: &Path, to: &Path) {
        self.root.relocate(from, to);
        for owned in &mut self.workspaces {
            owned.root.relocate(from, to);
        }
        for window in &mut self.windows {
            relocate_tabs(&mut window.group, from, to);
        }
    }

    pub fn retain(&mut self, keep: impl Fn(&Path) -> bool) {
        self.root.retain(&keep);
        for owned in &mut self.workspaces {
            owned.root.retain(&keep);
        }
        for window in &mut self.windows {
            retain_tabs(&mut window.group, &keep);
        }
        let empty: Vec<GroupId> =
            self.windows.iter().filter(|window| window.group.tabs.is_empty()).map(|window| window.group.id).collect();
        for id in empty {
            self.remove_window(id);
        }
        while self.is_split() {
            let Some(empty) = self.groups().iter().find(|group| group.tabs.is_empty()).map(|group| group.id) else {
                break;
            };
            self.remove(empty);
        }
    }

    pub fn main_only(&self) -> Self {
        let focused = self.main_focused();
        Self {
            root: self.root.clone(),
            focused,
            main_focused: focused,
            workspaces: Vec::new(),
            policy: self.policy.clone(),
            windows: Vec::new(),
        }
    }

    pub fn restored(mut self, open: impl Fn(&Path) -> bool) -> Self {
        let focused_tabs = self.root.group(self.focused).map(|group| group.tabs.clone());
        let main_tabs = self.root.group(self.main_focused).map(|group| group.tabs.clone());
        let mut seen: Vec<GroupId> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        self.workspaces.retain(|owned| {
            let ids: Vec<GroupId> = owned.groups().iter().map(|group| group.id).collect();
            let fresh = ids.iter().all(|id| *id >= OWNED_FIRST && !seen.contains(id))
                && !owned.workspace.is_empty()
                && !names.contains(&owned.workspace);
            seen.extend(ids);
            names.push(owned.workspace.clone());
            fresh
        });
        let mut window_ids: Vec<GroupId> = Vec::new();
        self.windows.retain(|window| {
            let id = window.group.id;
            let fresh = id >= WINDOW_FIRST && !window_ids.contains(&id);
            window_ids.push(id);
            fresh
        });
        let held_focus =
            (self.owner(self.focused).is_some() || self.window(self.focused).is_some()).then_some(self.focused);
        self.retain(open);
        for owned in &mut self.workspaces {
            fix_active(&mut owned.root);
            if owned.root.group(owned.focused).is_none() {
                owned.focused = owned.groups()[0].id;
            }
        }
        for window in &mut self.windows {
            let group = &mut window.group;
            group.active = group.active.filter(|index| *index < group.tabs.len()).or(Some(0));
            window.bounds = window.bounds.filter(|bounds| bounds.usable());
        }
        let held_focus = held_focus.filter(|id| self.group(*id).is_some());
        let mut next = 0;
        let (mut focused, mut main) = (None, None);
        renumber(&mut self.root, &mut next, &mut |group| {
            if focused.is_none() && focused_tabs.as_ref() == Some(&group.tabs) {
                focused = Some(group.id);
            }
            if main.is_none() && main_tabs.as_ref() == Some(&group.tabs) {
                main = Some(group.id);
            }
        });
        let first = self.root.first_id();
        self.main_focused = main.or(focused).unwrap_or(first);
        self.focused = held_focus.or(focused).unwrap_or(self.main_focused);
        self
    }
}

fn renumber_owned(node: &mut SplitNode, next: &mut GroupId, pairs: &mut Vec<(GroupId, GroupId)>) {
    match node {
        SplitNode::Group(group) => {
            pairs.push((group.id, *next));
            *group = EditorGroup { id: *next, ..EditorGroup::default() };
            *next += 1;
        }
        SplitNode::Split { ratio, first, second, .. } => {
            *ratio = if ratio.is_finite() { ratio.clamp(*RATIO.start(), *RATIO.end()) } else { 0.5 };
            renumber_owned(first, next, pairs);
            renumber_owned(second, next, pairs);
        }
    }
}

fn fix_active(node: &mut SplitNode) {
    match node {
        SplitNode::Group(group) => {
            group.active = group.active.filter(|index| *index < group.tabs.len()).or(group.tabs.first().map(|_| 0));
        }
        SplitNode::Split { ratio, first, second, .. } => {
            *ratio = if ratio.is_finite() { ratio.clamp(*RATIO.start(), *RATIO.end()) } else { 0.5 };
            fix_active(first);
            fix_active(second);
        }
    }
}

fn relocate_tabs(group: &mut EditorGroup, from: &Path, to: &Path) {
    let others = group.pinned.iter_mut().chain(group.preview.iter_mut()).chain(group.used.iter_mut());
    for tab in group.tabs.iter_mut().chain(others) {
        if let Ok(rest) = tab.strip_prefix(from) {
            *tab = if rest.as_os_str().is_empty() { to.to_owned() } else { to.join(rest) };
        }
    }
}

fn retain_tabs(group: &mut EditorGroup, keep: &impl Fn(&Path) -> bool) {
    let active = group.active_path().map(Path::to_owned);
    group.tabs.retain(|tab| keep(tab));
    let tabs = &group.tabs;
    group.pinned.retain(|pinned| tabs.contains(pinned));
    group.preview = group.preview.take().filter(|preview| tabs.contains(preview));
    group.active = active
        .and_then(|path| group.tabs.iter().position(|tab| *tab == path))
        .or_else(|| group.tabs.len().checked_sub(1));
}

fn renumber(node: &mut SplitNode, next: &mut GroupId, seen: &mut impl FnMut(&EditorGroup)) {
    match node {
        SplitNode::Group(group) => {
            *next += 1;
            group.id = *next;
            group.active = group.active.filter(|index| *index < group.tabs.len()).or(group.tabs.first().map(|_| 0));
            seen(group);
        }
        SplitNode::Split { ratio, first, second, .. } => {
            *ratio = if ratio.is_finite() { ratio.clamp(*RATIO.start(), *RATIO.end()) } else { 0.5 };
            renumber(first, next, seen);
            renumber(second, next, seen);
        }
    }
}

impl Workbench {
    pub fn groups(&self) -> &EditorGroups {
        &self.groups
    }

    pub fn present(&mut self, id: GroupId) {
        let path = self.groups.group(id).and_then(EditorGroup::active_path).map(Path::to_owned);
        self.active = path.and_then(|path| self.documents.iter().position(|document| document.path == path));
        if let Some(document) = self.active.and_then(|index| self.documents.get_mut(index)) {
            document.show_in(id);
        }
        self.settle_previews();
    }

    pub(crate) fn settle_previews(&mut self) {
        let edited: Vec<PathBuf> = self
            .groups
            .all()
            .filter_map(|group| group.preview.clone())
            .filter(|preview| self.documents.iter().any(|document| document.path == *preview && document.was_edited()))
            .collect();
        for path in edited {
            self.groups.keep_everywhere(&path);
        }
    }

    pub fn restore_groups(&mut self, groups: EditorGroups) {
        let documents = &self.documents;
        self.groups = groups.restored(|path| documents.iter().any(|document| document.path == path));
        self.sync_groups();
    }

    pub fn restore_windows(&mut self, windows: Vec<DetachedWindow>) {
        let documents = &self.documents;
        let groups = std::mem::take(&mut self.groups);
        self.groups = groups.with_windows(windows, |path| documents.iter().any(|document| document.path == path));
        self.sync_groups();
    }

    pub(crate) fn sync_groups(&mut self) {
        let missing: Vec<PathBuf> = self
            .documents
            .iter()
            .filter(|document| !self.groups.contains(&document.path))
            .map(|document| document.path.clone())
            .collect();
        if !missing.is_empty() {
            let main = self.groups.main_mut();
            let active = main.active_path().map(Path::to_owned);
            main.tabs.extend(missing);
            match active {
                Some(active) => main.activate(&active),
                None => main.active = Some(0),
            }
        }
        let groups = &self.groups;
        for document in &mut self.documents {
            let path = document.path.clone();
            document.retain_views(|id| groups.group(id).is_some_and(|group| group.tabs.contains(&path)));
        }
        self.present(self.groups.focused);
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(SplitRight)?;
    registry.register(SplitDown)?;
    registry.register(OpenInRightSplit)?;
    registry.register(Unsplit)?;
    registry.register(UnsplitAll)?;
    registry.register(NextSplitter)?;
    registry.register(PreviousSplitter)?;
    registry.register(FocusGroup)?;
    registry.register(MoveTab)?;
    registry.register(ResizeSplit)?;
    registry.register(ListGroups)?;
    registry.register(MoveToOpposite)?;
    registry.register(OpenTabInNewWindow)?;
    registry.register(CloseWindow)?;
    registry.register(ListWindows)?;
    registry.register(SetWindowBounds)?;
    Ok(())
}

/// What to split.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SplitParams {
    /// The group to split; the focused one when absent.
    pub group: Option<GroupId>,
    /// The tab the new group shows; the group's active tab when absent.
    pub index: Option<usize>,
    /// Split and Move: the tab leaves the old group.
    #[serde(rename = "move")]
    pub moved: bool,
}

pub type GroupsOutput = EditorGroups;

fn change(
    session: &mut Session,
    change: impl FnOnce(&mut EditorGroups) -> bool,
    refused: impl FnOnce() -> String,
) -> Result<EditorGroups, CommandError> {
    let origin = history::caret(session);
    let workbench = session.state_mut::<Workbench>();
    if !change(&mut workbench.groups) {
        return Err(CommandError::new(refused()));
    }
    workbench.sync_groups();
    workbench.shown += 1;
    workbench.touch_recent();
    let groups = workbench.groups.clone();
    history::left_file(session, origin);
    Ok(groups)
}

fn split(session: &mut Session, params: SplitParams, direction: SplitDirection) -> Result<EditorGroups, CommandError> {
    let owned = session.state::<Workbench>().and_then(|workbench| {
        let id = params.group.unwrap_or(workbench.groups.focused);
        workbench.groups.owner(id).map(|workspace| (workspace.to_owned(), id))
    });
    if let Some((workspace, id)) = owned {
        crate::workspaces::split_group(session, &workspace, (id, params.index), direction, params.moved)?;
        return Ok(session.state::<Workbench>().map(|workbench| workbench.groups.clone()).unwrap_or_default());
    }
    change(
        session,
        |groups| {
            let id = params.group.unwrap_or(groups.focused);
            groups.split(id, params.index, direction, params.moved).is_some()
        },
        || t!("workbench-error-no-split"),
    )
}

fn has_tab(session: &Session) -> bool {
    crate::workspaces::in_front(session).is_some_and(|(_, tab)| tab.is_some())
        || session.state::<Workbench>().is_some_and(|workbench| workbench.groups.focused_group().active.is_some())
}

fn is_split(session: &Session) -> bool {
    session.state::<Workbench>().is_some_and(|workbench| workbench.groups.is_split_here())
}

fn has_splitter(session: &Session) -> bool {
    session
        .state::<Workbench>()
        .is_some_and(|workbench| workbench.groups.is_split_here() || workbench.groups.is_split())
}

pub struct SplitRight;

impl Command for SplitRight {
    type Params = SplitParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.right";
    const TITLE: &'static str = "Split Right";
    const DESCRIPTION: &'static str = "Splits the editor side by side: a new group on the right shows the tab (the active one by default), and takes the focus; with move, the tab leaves the old group. While a workspace is expanded it splits the workspace's area, and a tab its feature draws always moves. Answers with the groups.";

    fn is_available(&self, session: &Session) -> bool {
        has_tab(session)
    }

    fn run(&self, session: &mut Session, params: SplitParams) -> Result<EditorGroups, CommandError> {
        split(session, params, SplitDirection::Right)
    }
}

pub struct OpenInRightSplit;

impl Command for OpenInRightSplit {
    type Params = crate::PathParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.openRight";
    const TITLE: &'static str = "Open in Right Split";
    const DESCRIPTION: &'static str = "Opens a file in a new editor group on the right of the focused one, which takes the \
         focus; a file that was not open in the focused group is not left there. Answers with the groups.";

    fn run(&self, session: &mut Session, params: crate::PathParams) -> Result<EditorGroups, CommandError> {
        let origin = history::caret(session);
        let path = session.state_mut::<Workbench>().spelled(&params.path);
        let was_open =
            session.state::<Workbench>().is_some_and(|workbench| workbench.groups.focused_group().tabs.contains(&path));
        crate::open_document(session, &path)?;
        let groups = split(session, SplitParams { group: None, index: None, moved: !was_open }, SplitDirection::Right)?;
        history::left_file(session, origin);
        Ok(groups)
    }
}

pub struct SplitDown;

impl Command for SplitDown {
    type Params = SplitParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.down";
    const TITLE: &'static str = "Split Down";
    const DESCRIPTION: &'static str = "Splits the editor one above the other: a new group below shows the tab (the active one by default), and takes the focus; with move, the tab leaves the old group. While a workspace is expanded it splits the workspace's area, and a tab its feature draws always moves. Answers with the groups.";

    fn is_available(&self, session: &Session) -> bool {
        has_tab(session)
    }

    fn run(&self, session: &mut Session, params: SplitParams) -> Result<EditorGroups, CommandError> {
        split(session, params, SplitDirection::Down)
    }
}

pub struct Unsplit;

impl Command for Unsplit {
    type Params = NoParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.unsplit";
    const TITLE: &'static str = "Unsplit";
    const DESCRIPTION: &'static str = "Removes the split the focused group is in, in the expanded workspace's area or the main editor: the group takes its place, with the other part's tabs after its own.";

    fn is_available(&self, session: &Session) -> bool {
        is_split(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<EditorGroups, CommandError> {
        change(session, EditorGroups::unsplit, || t!("workbench-error-not-split"))
    }
}

pub struct UnsplitAll;

impl Command for UnsplitAll {
    type Params = NoParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.unsplitAll";
    const TITLE: &'static str = "Unsplit All";
    const DESCRIPTION: &'static str = "Removes every split of the expanded workspace's area, else of the main editor: one group keeps every tab, the focused group's first.";

    fn is_available(&self, session: &Session) -> bool {
        is_split(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<EditorGroups, CommandError> {
        change(session, EditorGroups::unsplit_all, || t!("workbench-error-not-split"))
    }
}

pub struct NextSplitter;

impl Command for NextSplitter {
    type Params = NoParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.next";
    const TITLE: &'static str = "Goto Next Splitter";
    const DESCRIPTION: &'static str = "Gives the keyboard to the next editor group, left to right and top to bottom, going round: among the expanded workspace's groups when its area is split, else among the main groups.";

    fn is_available(&self, session: &Session) -> bool {
        has_splitter(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<EditorGroups, CommandError> {
        change(session, |groups| groups.focus_next(true), || t!("workbench-error-not-split"))
    }
}

pub struct PreviousSplitter;

impl Command for PreviousSplitter {
    type Params = NoParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.previous";
    const TITLE: &'static str = "Goto Previous Splitter";
    const DESCRIPTION: &'static str = "Gives the keyboard to the previous editor group, going round.";

    fn is_available(&self, session: &Session) -> bool {
        has_splitter(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<EditorGroups, CommandError> {
        change(session, |groups| groups.focus_next(false), || t!("workbench-error-not-split"))
    }
}

/// Which group.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct GroupParams {
    /// The group's id, from `editor.split.list`.
    pub group: GroupId,
}

pub struct FocusGroup;

impl Command for FocusGroup {
    type Params = GroupParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.focus";
    const TITLE: &'static str = "Focus Editor Group";
    const DESCRIPTION: &'static str = "Gives the keyboard to an editor group: its active tab becomes the active document. Focusing a workspace's group expands the workspace; focusing a main group collapses it.";

    fn run(&self, session: &mut Session, params: GroupParams) -> Result<EditorGroups, CommandError> {
        change(session, |groups| groups.focus(params.group), || t!("workbench-error-no-group", group = params.group))
    }
}

/// Which tab goes where.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveTabParams {
    /// The group the tab is in.
    pub from: GroupId,
    /// The tab's index in it.
    pub index: usize,
    /// The group it goes to; with `workspace` instead, that workspace's group that last had the keyboard.
    #[serde(default)]
    pub to: Option<GroupId>,
    /// The workspace it goes to (`workspace.host.list`), when `to` is absent; the workspace expands.
    #[serde(default)]
    pub workspace: Option<String>,
    /// Its index there; at the end when absent.
    #[serde(default)]
    pub at: Option<usize>,
}

pub struct MoveTab;

impl Command for MoveTab {
    type Params = MoveTabParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.moveTab";
    const TITLE: &'static str = "Move Tab to Group";
    const DESCRIPTION: &'static str = "Moves a tab to another editor group, which takes the focus and shows it; a main group left without tabs goes. \
         A project's file a workspace takes (the Database workspace: `.sql` scripts) moves between the main editor and that workspace; any other move between a workspace and the main editor, or between two workspaces, is refused with the reason (code `notMovable`).";

    fn run(&self, session: &mut Session, params: MoveTabParams) -> Result<EditorGroups, CommandError> {
        let to = match (params.to, &params.workspace) {
            (Some(to), _) => to,
            (None, Some(workspace)) => crate::workspaces::group_for(session, workspace)?,
            (None, None) => return Err(CommandError::new(t!("workbench-error-move-where"))),
        };
        let moving = session.state::<Workbench>().and_then(|workbench| {
            let groups = &workbench.groups;
            groups.group(to)?;
            let path = groups.group(params.from)?.tabs.get(params.index)?.clone();
            groups.crosses_workspaces(params.from, to).then_some(path)
        });
        if let Some(path) = moving {
            crate::workspaces::check_move(session, &path, (params.from, to))?;
        }
        change(
            session,
            |groups| groups.move_tab(params.from, params.index, to, params.at),
            || t!("workbench-error-no-group", group = to),
        )
    }
}

/// Where a divider goes.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ResizeParams {
    /// The group just right of or below the divider.
    pub before: GroupId,
    /// The share of the part left of or above the divider, from 0.1 to 0.9.
    pub ratio: f32,
}

pub struct ResizeSplit;

impl Command for ResizeSplit {
    type Params = ResizeParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.resize";
    const TITLE: &'static str = "Resize Split";
    const DESCRIPTION: &'static str =
        "Moves the divider just left of or above an editor group: the part before it takes `ratio` of the split.";

    fn run(&self, session: &mut Session, params: ResizeParams) -> Result<EditorGroups, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        if !workbench.groups.resize(params.before, params.ratio) {
            return Err(CommandError::new(t!("workbench-error-no-group", group = params.before)));
        }
        Ok(workbench.groups.clone())
    }
}

pub struct MoveToOpposite;

impl Command for MoveToOpposite {
    type Params = crate::CloseParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.moveToOpposite";
    const TITLE: &'static str = "Move to Opposite Group";
    const DESCRIPTION: &'static str = "Moves a tab (the focused group's active one by default) to the next editor group of \
         the same split, which takes the focus and shows it; a pinned tab stays pinned there. Refused while the editor is \
         not split. Answers with the groups.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| {
            let groups = &workbench.groups;
            groups.focused_group().active.is_some() && groups.opposite(groups.focused).is_some()
        })
    }

    fn run(&self, session: &mut Session, params: crate::CloseParams) -> Result<EditorGroups, CommandError> {
        change(
            session,
            |groups| {
                let from = params.group.unwrap_or(groups.focused);
                let index = params.index.or_else(|| groups.group(from).and_then(|group| group.active));
                match (groups.opposite(from), index) {
                    (Some(to), Some(index)) => groups.move_tab(from, index, to, None),
                    _ => false,
                }
            },
            || t!("workbench-error-no-opposite"),
        )
    }
}

pub struct ListGroups;

impl Command for ListGroups {
    type Params = NoParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.split.list";
    const TITLE: &'static str = "List Editor Groups";
    const DESCRIPTION: &'static str = "Returns the split tree: each group's id, tabs and active tab, how the splits divide the editor, the focused group and the main group that last had the keyboard (`mainFocused`), and under `workspaces` the groups each workspace owns, outside the main split tree, in a split tree of their own (`root`) with the one that last had the keyboard (`focused`).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<EditorGroups, CommandError> {
        Ok(session.state::<Workbench>().map(|workbench| workbench.groups.clone()).unwrap_or_default())
    }
}

/// The tab that goes into a new window.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenTabParams {
    /// The tab's group; the focused one when absent.
    pub group: Option<GroupId>,
    /// The tab's index in it; the active tab when absent.
    pub index: Option<usize>,
    /// Where the window goes; the system chooses when absent.
    pub bounds: Option<WindowBounds>,
}

pub struct OpenTabInNewWindow;

impl Command for OpenTabInNewWindow {
    type Params = OpenTabParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.window.openTab";
    const TITLE: &'static str = "Open Tab in New Window";
    const DESCRIPTION: &'static str = "Moves an editor tab into a new operating system window with its own tab row, \
         which takes the focus: the focused group's active tab by default. The window shares the document with every \
         other view of the file. `editor.split.moveTab` moves tabs in and out of it (its `group` in \
         `editor.window.list`), and closing its last tab closes it. Refused for the only tab of a detached window.";

    fn run(&self, session: &mut Session, params: OpenTabParams) -> Result<EditorGroups, CommandError> {
        change(
            session,
            |groups| {
                let from = params.group.unwrap_or_else(|| groups.focused());
                let index = params.index.or_else(|| groups.group(from).and_then(|group| group.active));
                index.is_some_and(|index| groups.detach(from, index, params.bounds).is_some())
            },
            || t!("workbench-error-no-tab-to-detach"),
        )
    }
}

/// A detached editor window.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowParams {
    /// The window's group, as `editor.window.list` gives it.
    pub group: GroupId,
}

pub struct CloseWindow;

impl Command for CloseWindow {
    type Params = WindowParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.window.close";
    const TITLE: &'static str = "Close Detached Editor Window";
    const DESCRIPTION: &'static str =
        "Closes a detached editor window; its tabs go back to the main window, so nothing unsaved is lost.";

    fn run(&self, session: &mut Session, params: WindowParams) -> Result<EditorGroups, CommandError> {
        change(
            session,
            |groups| groups.close_window(params.group),
            || t!("workbench-error-no-group", group = params.group),
        )
    }
}

/// The detached editor windows.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowList {
    /// Each window's group and place on the screen.
    pub windows: Vec<DetachedWindow>,
}

pub struct ListWindows;

impl Command for ListWindows {
    type Params = NoParams;
    type Output = WindowList;
    const ID: &'static str = "editor.window.list";
    const TITLE: &'static str = "List Detached Editor Windows";
    const DESCRIPTION: &'static str = "Returns the detached editor windows: each one's group, with its tabs and \
         active tab, and its place on the screen.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<WindowList, CommandError> {
        let windows = session.state::<Workbench>().map(|workbench| workbench.groups.windows.clone());
        Ok(WindowList { windows: windows.unwrap_or_default() })
    }
}

/// A new place for a detached editor window.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowBoundsParams {
    /// The window's group, as `editor.window.list` gives it.
    pub group: GroupId,
    /// Where it is now.
    pub bounds: WindowBounds,
}

pub struct SetWindowBounds;

impl Command for SetWindowBounds {
    type Params = WindowBoundsParams;
    type Output = GroupsOutput;
    const ID: &'static str = "editor.window.setBounds";
    const TITLE: &'static str = "Remember Detached Editor Window Bounds";
    const DESCRIPTION: &'static str = "Records where a detached editor window is, which it comes back to when the \
         project opens again. The window itself does not move: this is what the window reports.";

    fn run(&self, session: &mut Session, params: WindowBoundsParams) -> Result<EditorGroups, CommandError> {
        session.state_mut::<Workbench>().groups.set_window_bounds(params.group, params.bounds);
        Ok(session.state::<Workbench>().map(|workbench| workbench.groups.clone()).unwrap_or_default())
    }
}

#[cfg(test)]
mod tests;

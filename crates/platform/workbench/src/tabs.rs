use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::groups::{EditorGroup, GroupId};
use crate::{Workbench, history};

pub const CLOSED_KEPT: usize = 30;

pub const SETTINGS: &str = "editor.tabs";

pub const MAX_TAB_LIMIT: usize = 999;

/// Where the row of editor tabs goes: the page's Tab placement.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TabPlacement {
    /// Above the text.
    #[default]
    Top,
    /// Below the text.
    Bottom,
    /// In a column left of the text.
    Left,
    /// In a column right of the text.
    Right,
    /// No tabs.
    None,
}

/// How tabs that do not fit in one row are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TabRows {
    /// One row that scrolls.
    #[default]
    Scroll,
    /// One row whose tabs get narrower.
    Squeeze,
    /// As many rows as the tabs need.
    Multiple,
}

/// Where a tab's close button goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CloseButton {
    /// Before the file icon.
    Left,
    /// After the name.
    #[default]
    Right,
    /// No close button.
    None,
}

/// Which tab closes first when a group goes over the tab limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum OverLimit {
    /// The least recently used tab without unsaved changes, then the least recently used one.
    CloseUnchanged,
    /// The least recently used tab.
    #[default]
    CloseUnused,
}

/// Which tab becomes active when the active tab closes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ActivateOnClose {
    /// The tab on its left.
    #[default]
    Left,
    /// The tab on its right.
    Right,
    /// The tab used most recently before it.
    Recent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EditorTabSettings {
    pub placement: TabPlacement,
    pub rows: TabRows,
    pub pinned_row: bool,
    pub show_icon: bool,
    pub show_extension: bool,
    pub show_directory: bool,
    pub mark_modified: bool,
    pub full_path_on_hover: bool,
    pub close_button: CloseButton,
    pub sort_alphabetically: bool,
    pub open_at_end: bool,
    pub limit: usize,
    pub over_limit: OverLimit,
    pub activate_on_close: ActivateOnClose,
    pub preview: bool,
    pub keep_pinned_left: bool,
    pub declaration_in_same_tab: bool,
}

impl Default for EditorTabSettings {
    fn default() -> Self {
        Self {
            placement: TabPlacement::default(),
            rows: TabRows::default(),
            pinned_row: false,
            show_icon: true,
            show_extension: true,
            show_directory: true,
            mark_modified: false,
            full_path_on_hover: true,
            close_button: CloseButton::default(),
            sort_alphabetically: false,
            open_at_end: true,
            limit: 30,
            over_limit: OverLimit::default(),
            activate_on_close: ActivateOnClose::default(),
            preview: false,
            keep_pinned_left: true,
            declaration_in_same_tab: false,
        }
    }
}

pub(crate) fn settings(session: &Session) -> EditorTabSettings {
    session.settings().get_or(SETTINGS, EditorTabSettings::default())
}

pub fn follow_policy(session: &mut Session) {
    if session.state::<Workbench>().is_none() {
        return;
    }
    let policy = settings(session);
    session.state_mut::<Workbench>().groups.follow_policy(&policy);
}

pub(crate) fn keep_to_the_limit(session: &mut Session) {
    let group = session.state::<Workbench>().map(|workbench| workbench.groups.focused());
    let Some(group) = group else { return };
    while let Some(index) = session.state::<Workbench>().and_then(|workbench| {
        workbench.groups.to_close_for_limit(group, |tab| {
            workbench.documents.iter().any(|document| document.path == tab && document.is_dirty())
        })
    }) {
        if crate::closing::close_tab(session, Some(group), Some(index)).is_err() {
            break;
        }
    }
}

pub(crate) fn tab_to_replace(session: &Session, target: &Path) -> Option<(GroupId, PathBuf)> {
    if !settings(session).declaration_in_same_tab {
        return None;
    }
    let workbench = session.state::<Workbench>()?;
    let group = workbench.groups.focused_group();
    let path = group.active_path()?.to_owned();
    let clean = !workbench.documents.iter().any(|document| document.path == path && document.is_dirty());
    (clean && path != target && !group.is_pinned(&path) && !group.has_tab(target)).then_some((group.id, path))
}

pub(crate) fn replace_left_tab(session: &mut Session, left: &(GroupId, PathBuf), target: &Path) {
    let workbench = session.state_mut::<Workbench>();
    let (group, old) = left;
    if workbench.groups.replace_tab(*group, old, target) {
        workbench.groups.close_path(*group, old);
        if !workbench.groups.contains(old) {
            workbench.forget_document(old);
        }
        workbench.sync_groups();
    }
}

pub(crate) fn open_preview(session: &mut Session, path: &Path, group: Option<GroupId>) -> Result<(), CommandError> {
    let spelled = session.state_mut::<Workbench>().resolve(path);
    let shown = session.state::<Workbench>().is_some_and(|workbench| workbench.groups.contains(&spelled));
    if shown || !settings(session).preview {
        return crate::open_file_in(session, path, false, group);
    }
    let workbench = session.state_mut::<Workbench>();
    workbench.settle_previews();
    let target = group.unwrap_or(workbench.groups.main_focused());
    crate::open_bringing(session, path, false, Some(target))?;
    let workbench = session.state_mut::<Workbench>();
    let spelled = workbench.groups.group(target).and_then(EditorGroup::active_path).map(Path::to_owned);
    if let Some(old) = spelled.and_then(|spelled| workbench.groups.mark_preview(target, &spelled)) {
        workbench.groups.close_path(target, &old);
        if !workbench.groups.contains(&old) {
            workbench.forget_document(&old);
        }
        workbench.sync_groups();
    }
    keep_to_the_limit(session);
    Ok(())
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(PinTab)?;
    registry.register(MoveTabInRow)?;
    registry.register(MoveProjectTab)?;
    registry.register(KeepTabOpen)?;
    registry.register(NextTab)?;
    registry.register(PreviousTab)?;
    registry.register(ReopenClosed)?;
    Ok(())
}

/// An editor tab that was closed, kept so that Reopen Closed Tab brings it back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClosedTab {
    /// The file the tab showed.
    pub path: PathBuf,
    /// The editor group it was in.
    pub group: GroupId,
    /// Its place in the group's row of tabs, from 0.
    pub index: usize,
    /// Whether it was pinned.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl Workbench {
    pub fn closed_tabs(&self) -> &[ClosedTab] {
        &self.closed_tabs
    }

    pub fn restore_closed_tabs(&mut self, tabs: Vec<ClosedTab>) {
        let skip = tabs.len().saturating_sub(CLOSED_KEPT);
        self.closed_tabs = tabs.into_iter().skip(skip).collect();
    }

    pub(crate) fn note_closing(&mut self, group: GroupId, index: usize) {
        let Some(tabs) = self.groups.group(group) else { return };
        let Some(path) = tabs.tabs.get(index).cloned() else { return };
        let pinned = tabs.is_pinned(&path);
        self.closed_tabs.retain(|closed| closed.path != path);
        self.closed_tabs.push(ClosedTab { path, group, index, pinned });
        if self.closed_tabs.len() > CLOSED_KEPT {
            self.closed_tabs.remove(0);
        }
    }
}

/// Which tab to pin or unpin.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct PinParams {
    /// The tab index in its group; the group's active tab when absent.
    pub index: Option<usize>,
    /// The editor group (`editor.split.list`); the focused one when absent.
    pub group: Option<GroupId>,
    /// `true` pins, `false` unpins; absent toggles.
    pub pinned: Option<bool>,
}

/// Which tab to move along its row, and where.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct MoveInRowParams {
    /// The tab index in its group; the group's active tab when absent.
    pub index: Option<usize>,
    /// The editor group (`editor.split.list`); the focused one when absent.
    pub group: Option<GroupId>,
    /// The index the tab takes in the row, from 0.
    pub to: usize,
}

/// Which preview tab to keep open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct KeepOpenParams {
    /// The tab index in its group; the group's active tab when absent.
    pub index: Option<usize>,
    /// The editor group (`editor.split.list`); the focused one when absent.
    pub group: Option<GroupId>,
}

pub struct KeepTabOpen;

impl Command for KeepTabOpen {
    type Params = KeepOpenParams;
    type Output = EditorGroup;
    const ID: &'static str = "editor.tab.keepOpen";
    const TITLE: &'static str = "Keep Tab Open";
    const DESCRIPTION: &'static str = "Makes a preview tab an ordinary one, so that the next previewed file gets a tab of \
         its own; an edit or a double click on the tab does the same. Answers with the group.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| {
            let group = workbench.groups().focused_group();
            group.active_path().is_some_and(|path| group.is_preview(path))
        })
    }

    fn run(&self, session: &mut Session, params: KeepOpenParams) -> Result<EditorGroup, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let group = params.group.unwrap_or(workbench.groups.focused());
        let tabs = workbench.groups.group(group).ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let path = params
            .index
            .or(tabs.active)
            .and_then(|index| tabs.tabs.get(index))
            .cloned()
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        workbench.groups.keep_open(group, &path);
        workbench.groups.group(group).cloned().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
    }
}

pub struct PinTab;

impl Command for PinTab {
    type Params = PinParams;
    type Output = EditorGroup;
    const ID: &'static str = "editor.tab.pin";
    const TITLE: &'static str = "Pin Tab";
    const DESCRIPTION: &'static str = "Pins or unpins an editor tab (toggles without `pinned`). Pinned tabs show a pin, \
         stay at the start of the row, survive Close Other Tabs, Close Tabs to the Left and Right and Close All but \
         Pinned, and are kept with the project. Answers with the group.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| workbench.groups().focused_group().active.is_some())
    }

    fn run(&self, session: &mut Session, params: PinParams) -> Result<EditorGroup, CommandError> {
        follow_policy(session);
        let workbench = session.state_mut::<Workbench>();
        let group = params.group.unwrap_or(workbench.groups.focused());
        let index = params.index.or_else(|| workbench.groups.group(group).and_then(|tabs| tabs.active));
        index
            .and_then(|index| workbench.groups.pin(group, index, params.pinned))
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        workbench.sync_groups();
        workbench.groups.group(group).cloned().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
    }
}

pub struct MoveTabInRow;

impl Command for MoveTabInRow {
    type Params = MoveInRowParams;
    type Output = EditorGroup;
    const ID: &'static str = "editor.tab.move";
    const TITLE: &'static str = "Move Tab";
    const DESCRIPTION: &'static str = "Moves an editor tab to another place in its row, as dragging it along the row \
         does. An unpinned tab never goes before a pinned one, and a pinned tab stays among the pinned ones; the place \
         is moved to the nearest allowed one. Refused while tabs are sorted alphabetically. To move a tab to another \
         group, use editor.split.moveTab. Answers with the group.";

    fn run(&self, session: &mut Session, params: MoveInRowParams) -> Result<EditorGroup, CommandError> {
        follow_policy(session);
        let workbench = session.state_mut::<Workbench>();
        let group = params.group.unwrap_or(workbench.groups.focused());
        let tabs = workbench.groups.group(group).ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        if tabs.is_sorted() {
            return Err(CommandError::new(t!("workbench-error-tabs-sorted")));
        }
        let index = params.index.or(tabs.active).ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        workbench
            .groups
            .move_in_row(group, index, params.to)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        workbench.sync_groups();
        workbench.groups.group(group).cloned().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
    }
}

#[derive(Debug, Default)]
pub struct ProjectTabMove(Option<usize>);

impl ProjectTabMove {
    pub fn take(&mut self) -> Option<usize> {
        self.0.take()
    }

    pub fn asked(&self) -> bool {
        self.0.is_some()
    }
}

/// Where the project's tab goes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct ProjectTabParams {
    /// The place in the main window's row of project tabs, from 0.
    pub to: usize,
}

pub struct MoveProjectTab;

impl Command for MoveProjectTab {
    type Params = ProjectTabParams;
    type Output = ();
    const ID: &'static str = "project.tab.move";
    const TITLE: &'static str = "Move Project Tab";
    const DESCRIPTION: &'static str = "Moves this project's tab to another place in the main window's row of project \
         tabs, as dragging it does; list_windows lists the projects in that order. Projects in windows of their own \
         have no tab and stay as they are.";

    fn run(&self, session: &mut Session, params: ProjectTabParams) -> Result<(), CommandError> {
        session
            .state::<Workbench>()
            .and_then(Workbench::project)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        session.state_mut::<ProjectTabMove>().0 = Some(params.to);
        Ok(())
    }
}

fn cycle(session: &mut Session, forward: bool) -> Result<Option<PathBuf>, CommandError> {
    let origin = history::caret(session);
    let workbench = session.state_mut::<Workbench>();
    let group = workbench.groups.focused();
    let tabs = workbench.groups.group(group).ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let count = tabs.tabs.len();
    let Some(active) = tabs.active.filter(|_| count > 0) else {
        return Err(CommandError::new(t!("workbench-error-no-document")));
    };
    let next = if forward { (active + 1) % count } else { (active + count - 1) % count };
    if next != active && workbench.groups.select(group, next) {
        workbench.sync_groups();
        workbench.shown += 1;
        workbench.touch_recent();
    }
    let shown = workbench.groups.group(group).and_then(EditorGroup::active_path).map(Path::to_owned);
    history::left_file(session, origin);
    Ok(shown)
}

fn several_tabs(session: &Session) -> bool {
    session.state::<Workbench>().is_some_and(|workbench| workbench.groups().focused_group().tabs.len() > 1)
}

pub struct NextTab;

impl Command for NextTab {
    type Params = NoParams;
    type Output = Option<PathBuf>;
    const ID: &'static str = "editor.tab.next";
    const TITLE: &'static str = "Select Next Tab";
    const DESCRIPTION: &'static str = "Makes the tab right of the active one active in the focused editor group, from the \
         last back to the first. Answers with the file now shown.";

    fn is_available(&self, session: &Session) -> bool {
        several_tabs(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Option<PathBuf>, CommandError> {
        cycle(session, true)
    }
}

pub struct PreviousTab;

impl Command for PreviousTab {
    type Params = NoParams;
    type Output = Option<PathBuf>;
    const ID: &'static str = "editor.tab.previous";
    const TITLE: &'static str = "Select Previous Tab";
    const DESCRIPTION: &'static str = "Makes the tab left of the active one active in the focused editor group, from the \
         first round to the last. Answers with the file now shown.";

    fn is_available(&self, session: &Session) -> bool {
        several_tabs(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Option<PathBuf>, CommandError> {
        cycle(session, false)
    }
}

fn place(workbench: &Workbench, group: GroupId, path: &Path) -> Option<usize> {
    workbench.groups.group(group).and_then(|tabs| tabs.tabs.iter().position(|tab| tab == path))
}

pub struct ReopenClosed;

impl Command for ReopenClosed {
    type Params = NoParams;
    type Output = ClosedTab;
    const ID: &'static str = "file.document.reopenClosed";
    const TITLE: &'static str = "Reopen Closed Tab";
    const DESCRIPTION: &'static str = "Reopens the file of the editor tab closed last, in its group (the main one when that \
         group is gone) at its place in the row, pinned again if it was. The closed tabs are kept per project, the \
         latest 30; a file that no longer opens is skipped. Answers with the tab reopened.";

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().is_some_and(|workbench| !workbench.closed_tabs.is_empty())
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ClosedTab, CommandError> {
        let origin = history::caret(session);
        loop {
            let Some(closed) = session.state_mut::<Workbench>().closed_tabs.pop() else {
                return Err(CommandError::new(t!("workbench-error-no-closed-tab")));
            };
            let group = session
                .state::<Workbench>()
                .and_then(|workbench| {
                    let groups = workbench.groups();
                    groups.group(closed.group).map(|_| closed.group)
                })
                .unwrap_or_else(|| {
                    session.state::<Workbench>().map_or(1, |workbench| workbench.groups().main_focused())
                });
            if crate::open_file_in(session, &closed.path, false, Some(group)).is_err() {
                continue;
            }
            let workbench = session.state_mut::<Workbench>();
            if let Some(at) = place(workbench, group, &closed.path) {
                let at = if closed.pinned {
                    workbench.groups.pin(group, at, Some(true)).map(|_| place(workbench, group, &closed.path))
                } else {
                    Some(Some(at))
                };
                if let Some(Some(at)) = at {
                    workbench.groups.reorder(group, at, closed.index);
                }
            }
            workbench.sync_groups();
            history::left_file(session, origin);
            return Ok(ClosedTab { group, ..closed });
        }
    }
}

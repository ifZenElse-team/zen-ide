use std::collections::{BTreeSet, HashMap};

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::ChangeInfo;
use zen_workbench::vcs::branches::BranchList;
use zen_workbench::vcs::commit::ChangePart;
use zen_workbench::vcs::compare::{FileAction, FileTarget, RightPanel};
use zen_workbench::vcs::conflict::ConflictList;
use zen_workbench::vcs::drag::{DropAction, DropMenuInfo};
use zen_workbench::vcs::graph_search::{FocusKind, GraphFocus, SearchState};
use zen_workbench::vcs::history::HistoryList;
use zen_workbench::vcs::index::IndexParams;
use zen_workbench::vcs::log::CommitDetailsInfo;
use zen_workbench::vcs::workspace::{
    CONFLICT_TAB, CommitAction, DIFF_TAB, GRAPH, GitWorkspace, GraphCommit, GraphPage, PromptKind, REBASE_TAB,
    RefAction, RefKind, RefTarget, StashAction, StashInfo, StashList, WORKSPACE as GIT, open_workspace,
};
use zen_workbench::workspaces::Workspaces;

mod avatars;
mod columns;
mod compare;
pub(super) mod conflicts;
mod look;
pub(super) mod rebase;
mod search;
mod worktrees;
use columns::Columns;
use look::{Tint, TreeRow};
use search::{FocusButton, Found};

use super::front::FrontKind;
use super::glyphs::{
    glyph_branch, glyph_check, glyph_cloud, glyph_database, glyph_fetch, glyph_folder, glyph_forward, glyph_label,
    glyph_list, glyph_minus, glyph_pencil, glyph_plus, glyph_pop, glyph_pull, glyph_push, glyph_redo, glyph_search,
    glyph_stash, glyph_submodule, glyph_trash, glyph_tree, glyph_undo, glyph_worktree,
};
use super::vcs::{format_date, status_color};
use super::{IdePalette, IdeView, Side, hint, hover};
use crate::theme;
use zen_workbench::vcs::workspace_columns::GraphColumn;
use zen_workbench::vcs::workspace_pull::PullMode;
use zen_workbench::vcs::workspace_view::{DETAILS_WIDTH, FilesView, GraphFit, WorkspaceView};
use zen_workbench::vcs::worktree::{
    ProjectWindowRequest, SubmoduleAction, SubmoduleInfo, WorktreeAction, WorktreeInfo, WorktreeList, WorktreePrompt,
};

const ROW: f32 = 28.0;
const LANE: f32 = zen_workbench::vcs::workspace_view::LANE_WIDTH;
const NODE: f32 = zen_workbench::vcs::workspace_view::NODE_RADIUS;
const MERGE_NODE: f32 = 5.0;
const CHUNK: usize = 200;
const REFS: f32 = 180.0;
const NARROW_REFS: f32 = 130.0;
const MESSAGE: f32 = 220.0;
const AUTHOR: f32 = 140.0;
const DATE: f32 = 128.0;
const SHA: f32 = 70.0;
const TOOLBAR: f32 = 58.0;
const BANNER: f32 = 42.0;
const HEADER: f32 = 26.0;

use zen_workbench::vcs::workspace_view::PanelSection as Section;

#[derive(Default)]
pub(super) struct GitWorkspaceView {
    pub(super) committing: bool,
    submodules_revision: u64,
    seen_raised: u64,
    diff_tab: bool,
    seen_revealed: u64,
    chunks: HashMap<usize, Vec<GraphCommit>>,
    graph_revision: Option<u64>,
    total: usize,
    lanes: usize,
    computing: bool,
    loading: bool,
    error: Option<String>,
    branches: BranchList,
    stashes: Vec<StashInfo>,
    history: HistoryList,
    refs_read_for: Option<(u64, Option<String>, Option<u64>)>,
    collapsed: BTreeSet<Section>,
    details: Option<CommitDetailsInfo>,
    changes: Vec<ChangeInfo>,
    changes_read_for: Option<u64>,
    reveal: Option<usize>,
    visible: std::ops::Range<usize>,
    #[cfg(test)]
    pub(super) panels: Option<(Rect, Option<Rect>)>,
    menu_for: Option<String>,
    menu_at: Option<egui::Pos2>,
    right_click: Option<(String, egui::Pos2)>,
    menu_info: Option<MenuInfo>,
    menu_keyboard: bool,
    ref_menu_for: Option<Vec<RefTarget>>,
    ref_menu_at: Option<egui::Pos2>,
    ref_right_click: Option<egui::Pos2>,
    ref_menu_keyboard: bool,
    stash_menu_for: Option<String>,
    stash_menu_at: Option<egui::Pos2>,
    stash_right_click: Option<egui::Pos2>,
    stash_menu_keyboard: bool,
    drag: RefDrag,
    drop_menu_for: Option<DropMenuInfo>,
    drop_menu_at: Option<egui::Pos2>,
    drop_menu_keyboard: bool,
    stash_message: String,
    summary: String,
    description: String,
    amend: bool,
    pending_diff: Option<(std::path::PathBuf, ChangePart, f64)>,
    dialog: Option<Dialog>,
    search_text: String,
    pub(super) search_focus: bool,
    search_matcher: Option<search::Matcher>,
    filter_text: String,
    focus: GraphFocus,
    collapsed_folders: BTreeSet<String>,
    all_files: Option<(String, Vec<String>)>,
    picked: Vec<String>,
    picked_wip: bool,
    compared: Option<compare::Compared>,
    file_history: Option<compare::History>,
    file_menu_for: Option<FileTarget>,
    file_menu_at: Option<egui::Pos2>,
    file_right_click: Option<egui::Pos2>,
    file_menu_keyboard: bool,
    conflicts: ConflictList,
    conflicts_read_for: Option<u64>,
    worktrees: Vec<WorktreeInfo>,
    worktrees_revision: Option<u64>,
    worktrees_counted: f64,
    submodules: Vec<SubmoduleInfo>,
    worktree_menu_for: Option<std::path::PathBuf>,
    worktree_menu_at: Option<egui::Pos2>,
    worktree_right_click: Option<egui::Pos2>,
    worktree_menu_keyboard: bool,
    submodule_menu_for: Option<String>,
    submodule_menu_at: Option<egui::Pos2>,
    submodule_right_click: Option<egui::Pos2>,
    submodule_menu_keyboard: bool,
    columns: columns::ColumnsView,
    avatars: avatars::AvatarView,
    graph_change: u64,
}

#[derive(Clone, Debug, Default)]
struct RefDrag {
    source: Option<RefTarget>,
    over: Option<RefTarget>,
    dropped_at: Option<egui::Pos2>,
    dropped_on: Option<RefTarget>,
}

impl GitWorkspaceView {
    pub(super) fn history_changed(&mut self) {
        self.refs_read_for = None;
    }

    pub(super) fn conflicts_changed(&mut self) {
        self.conflicts_read_for = None;
        self.changes_read_for = None;
        self.refs_read_for = None;
    }

    pub(super) fn show_conflicts(&mut self, message: String) {
        self.dialog = Some(Dialog::Conflicts { message });
        self.conflicts_changed();
    }

    pub(super) fn operation_title(&self) -> Option<String> {
        self.conflicts.operation.as_ref().map(|operation| operation.title.clone())
    }
}

impl RefDrag {
    fn track(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        source: Option<&RefTarget>,
        target: Option<&RefTarget>,
    ) -> bool {
        if response.drag_started()
            && let Some(source) = source
        {
            self.source = Some(source.clone());
            self.over = None;
        }
        let Some((dragged, target)) = self.source.as_ref().zip(target) else {
            return target.is_some() && self.dropped_on.as_ref() == target;
        };
        if dragged == target || !droppable(dragged, target) || !ui.rect_contains_pointer(response.rect) {
            return false;
        }
        self.over = Some(target.clone());
        true
    }
}

fn droppable(source: &RefTarget, target: &RefTarget) -> bool {
    source.kind != RefKind::Head
        && target.kind != RefKind::Head
        && (target.kind == RefKind::Local || source.kind == RefKind::Local)
}

#[derive(Clone, Debug, Default)]
struct MenuInfo {
    on_current: bool,
    detached_here: bool,
}

enum Dialog {
    CreateTag {
        commit: DialogCommit,
        name: String,
        message: String,
        focused: bool,
        error: Option<String>,
    },
    Checkout {
        commit: DialogCommit,
        error: Option<String>,
    },
    ResetHard {
        commit: DialogCommit,
        error: Option<String>,
    },
    Conflicts {
        message: String,
    },
    RenameBranch {
        branch: String,
        name: String,
        focused: bool,
        error: Option<String>,
    },
    SetUpstream {
        branch: String,
        upstream: Option<String>,
        error: Option<String>,
    },
    DeleteRemoteBranch {
        branch: String,
        error: Option<String>,
    },
    DropStash {
        stash: DialogStash,
        error: Option<String>,
    },
    RenameStash {
        stash: DialogStash,
        message: String,
        focused: bool,
        error: Option<String>,
    },
    Discard {
        text: String,
        params: IndexParams,
        error: Option<String>,
    },
    CheckoutAndDrop {
        action: DropAction,
        source: RefTarget,
        target: RefTarget,
        error: Option<String>,
    },
    CreateWorktree {
        branch: RefTarget,
        folder: String,
        folder_edited: bool,
        new_branch: String,
        open: bool,
        focused: bool,
        error: Option<String>,
    },
    RemoveWorktree {
        worktree: WorktreeInfo,
        delete_branch: bool,
        error: Option<String>,
    },
}

struct DialogStash {
    commit: String,
    info: Option<StashInfo>,
}

impl DialogStash {
    fn label(&self) -> String {
        match &self.info {
            Some(info) => t!("vcs-workspace-stash-label", name = info.name.as_str(), message = info.message.as_str()),
            None => self.commit.chars().take(7).collect(),
        }
    }
}

struct DialogCommit {
    id: String,
    subject: Option<String>,
}

impl DialogCommit {
    fn new(id: String) -> Self {
        DialogCommit { id, subject: None }
    }

    fn label(&self) -> String {
        let short: String = self.id.chars().take(7).collect();
        match &self.subject {
            Some(subject) if !subject.is_empty() => {
                t!("vcs-workspace-commit-label", commit = short.as_str(), subject = subject.as_str())
            }
            _ => short,
        }
    }
}

impl Dialog {
    fn name(&self) -> &'static str {
        match self {
            Dialog::CreateTag { .. } => "createTag",
            Dialog::Checkout { .. } => "checkoutCommit",
            Dialog::ResetHard { .. } => "resetHard",
            Dialog::Conflicts { .. } => "gitConflicts",
            Dialog::RenameBranch { .. } => "renameBranch",
            Dialog::SetUpstream { .. } => "setUpstream",
            Dialog::DeleteRemoteBranch { .. } => "deleteRemoteBranch",
            Dialog::DropStash { .. } => "dropStash",
            Dialog::RenameStash { .. } => "renameStash",
            Dialog::Discard { .. } => "discardChanges",
            Dialog::CheckoutAndDrop { action: DropAction::Merge, .. } => "checkoutAndMerge",
            Dialog::CheckoutAndDrop { .. } => "checkoutAndRebase",
            Dialog::CreateWorktree { .. } => zen_workbench::vcs::worktree::CREATE_DIALOG,
            Dialog::RemoveWorktree { .. } => zen_workbench::vcs::worktree::REMOVE_DIALOG,
        }
    }
}

enum RefMenuOpen {
    Ref(RefTarget),
    Labels(String),
}

enum Ask {
    Select(String),
    Pick(String),
    SelectWip,
    PickWip,
    DiffBetween { base: String, id: Option<String>, path: std::path::PathBuf },
    FileMenu(Option<(std::path::PathBuf, Option<String>)>),
    File(std::path::PathBuf, FileAction),
    Checkout(String),
    Diff { id: Option<String>, path: std::path::PathBuf, part: Option<ChangePart> },
    Index { command: &'static str, params: serde_json::Value },
    CommitStaged { push: bool },
    Amend(bool),
    Action(&'static str),
    Menu(Option<String>),
    Commit(String, CommitAction),
    RefMenu(Option<RefMenuOpen>),
    Ref(RefTarget, RefAction),
    StashMenu(Option<String>),
    Stash(Option<String>, StashAction),
    DropMenu(Option<(RefTarget, RefTarget)>),
    Drop(RefTarget, RefTarget, DropAction),
    StashChanges { staged: bool, message: Option<String> },
    Search(serde_json::Value),
    Filter(String),
    Focus(&'static str, serde_json::Value),
    Command(&'static str, serde_json::Value),
    ActionWith(&'static str, serde_json::Value),
    WorktreeMenu(Option<std::path::PathBuf>),
    Worktree(std::path::PathBuf, WorktreeAction),
    SubmoduleMenu(Option<String>),
    Submodule(String, SubmoduleAction),
}

impl IdeView {
    pub(super) fn git_in_front(&self, session: &Session) -> bool {
        self.git_tab_in_front(session, GRAPH) && session.state::<GitWorkspace>().is_some_and(GitWorkspace::is_open)
    }

    pub(super) fn git_tab_in_front(&self, session: &Session, kind: &str) -> bool {
        zen_workbench::workspaces::in_front(session)
            .is_some_and(|(info, tab)| info.id == GIT && tab.is_some_and(|tab| tab.kind == kind))
    }

    pub(super) fn put_git_tab(session: &mut Session, id: &str, title: String) {
        let tab = zen_workbench::workspaces::InnerTab {
            id: id.to_owned(),
            kind: id.to_owned(),
            title,
            path: None,
            closable: true,
            data: serde_json::Value::Null,
            group: 0,
        };
        session.state_mut::<Workspaces>().put_tab(GIT, tab);
    }

    fn has_git_tab(session: &Session, id: &str) -> bool {
        session.state::<Workspaces>().is_some_and(|workspaces| workspaces.tabs(GIT).iter().any(|tab| tab.id == id))
    }

    pub(super) fn git_inner_tab(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        tab: &zen_workbench::workspaces::InnerTab,
    ) {
        match tab.kind.as_str() {
            DIFF_TAB if self.diff.is_some() => self.diff_body(ui, registry, session),
            REBASE_TAB => self.as_git(|view| view.rebase_editor(ui, registry, session)),
            CONFLICT_TAB => self.as_git(|view| view.conflict_editor(ui, registry, session)),
            GRAPH => self.as_git(|view| view.git_workspace(ui, registry, session)),
            _ => {}
        }
    }

    pub(super) fn git_workspace_frame(&mut self, ctx: &egui::Context, session: &mut Session) {
        if let Some(text) = session.state_mut::<zen_workbench::vcs::ClipboardRequest>().take() {
            ctx.copy_text(text);
        }
        if let Some(path) = session.state_mut::<ProjectWindowRequest>().take() {
            self.requests.push(super::app::Request::OpenWindow(path));
        }
        if let Some(prompt) = session.state_mut::<GitWorkspace>().take_worktree_prompt() {
            self.git_workspace.dialog = Some(match prompt {
                WorktreePrompt::Create { branch } => {
                    let remote = branch.kind == RefKind::Remote;
                    let new_branch = if remote { worktrees::local_name(&branch.name, true) } else { String::new() };
                    let root = self.vcs.info.root.clone().unwrap_or_default();
                    let folder = worktrees::offered_folder(&root, &worktrees::local_name(&branch.name, remote));
                    Dialog::CreateWorktree {
                        branch,
                        folder,
                        folder_edited: false,
                        new_branch,
                        open: true,
                        focused: false,
                        error: None,
                    }
                }
                WorktreePrompt::Remove { worktree } => {
                    Dialog::RemoveWorktree { worktree, delete_branch: false, error: None }
                }
            });
        }
        if let Some(message) = session.state_mut::<GitWorkspace>().take_conflicts() {
            self.git_workspace.dialog = Some(Dialog::Conflicts { message });
        }
        if let Some(params) = session.state_mut::<GitWorkspace>().take_discard() {
            let root = self.vcs.info.root.clone().unwrap_or_default();
            let text = discard_text(&params, &root, &self.git_workspace.changes);
            self.git_workspace.dialog = Some(Dialog::Discard { text, params, error: None });
        }
        let Some(prompt) = session.state_mut::<GitWorkspace>().take_prompt() else {
            return self.git_workspace_tab(session);
        };
        let id = prompt.id;
        match prompt.kind {
            PromptKind::CreateBranch => self.open_new_branch(Some(id)),
            PromptKind::CreateTag => {
                let commit = DialogCommit::new(id);
                self.git_workspace.dialog = Some(Dialog::CreateTag {
                    commit,
                    name: String::new(),
                    message: String::new(),
                    focused: false,
                    error: None,
                });
            }
            PromptKind::Checkout => {
                self.git_workspace.dialog = Some(Dialog::Checkout { commit: DialogCommit::new(id), error: None });
            }
            PromptKind::ResetHard => {
                self.git_workspace.dialog = Some(Dialog::ResetHard { commit: DialogCommit::new(id), error: None });
            }
            PromptKind::RenameBranch => {
                let branch = prompt.branch.unwrap_or_default();
                let name = branch.clone();
                self.git_workspace.dialog = Some(Dialog::RenameBranch { branch, name, focused: false, error: None });
            }
            PromptKind::SetUpstream => {
                let branch = prompt.branch.unwrap_or_default();
                let upstream = default_upstream(&self.git_workspace.branches, &branch);
                self.git_workspace.dialog = Some(Dialog::SetUpstream { branch, upstream, error: None });
            }
            PromptKind::DeleteRemoteBranch => {
                let branch = prompt.branch.unwrap_or_default();
                self.git_workspace.dialog = Some(Dialog::DeleteRemoteBranch { branch, error: None });
            }
            PromptKind::DropStash => {
                let stash = DialogStash { commit: id, info: None };
                self.git_workspace.dialog = Some(Dialog::DropStash { stash, error: None });
            }
            PromptKind::RenameStash => {
                let stash = DialogStash { commit: id, info: None };
                let message = String::new();
                self.git_workspace.dialog = Some(Dialog::RenameStash { stash, message, focused: false, error: None });
            }
            PromptKind::CheckoutAndMerge | PromptKind::CheckoutAndRebase => {
                let action =
                    if prompt.kind == PromptKind::CheckoutAndMerge { DropAction::Merge } else { DropAction::Rebase };
                if let Some((source, target)) = prompt.drop {
                    self.git_workspace.dialog = Some(Dialog::CheckoutAndDrop { action, source, target, error: None });
                }
            }
        }
        self.git_workspace_tab(session);
    }

    pub(super) fn open_workspace_dialogs(&self) -> Option<&'static str> {
        self.git_workspace.dialog.as_ref().map(Dialog::name)
    }

    fn git_workspace_tab(&mut self, session: &mut Session) {
        let Some(workspace) = session.state::<GitWorkspace>() else { return };
        let (open, raised) = (workspace.is_open(), workspace.raised());
        if open && raised != self.git_workspace.seen_raised {
            self.git_workspace.seen_raised = raised;
            self.close_text_dialogs();
        }
        let kept = session.state::<Workspaces>().is_some_and(|workspaces| workspaces.is_open(GIT));
        if kept && !open && self.vcs.info.root.is_some() && open_workspace(session).is_err() {
            zen_workbench::workspaces::close(session, GIT);
        }
        if let Some(view) = session.state_mut::<GitWorkspace>().take_diff() {
            let from_workspace = zen_workbench::workspaces::expanded(session).as_deref() == Some(GIT);
            self.show_diff(session, view);
            if from_workspace {
                self.drop_front(FrontKind::Diff);
                self.diff_from_workspace = true;
                let title = self.diff.as_ref().map(super::diff_view::DiffTab::title).unwrap_or_default();
                Self::put_git_tab(session, DIFF_TAB, title);
                self.git_workspace.diff_tab = true;
                zen_workbench::workspaces::bring(session, GIT, DIFF_TAB);
            }
        }
        let wanted = self.diff.is_some() && self.diff_from_workspace;
        let present = Self::has_git_tab(session, DIFF_TAB);
        if wanted && !present && self.git_workspace.diff_tab {
            self.diff = None;
            self.diff_from_workspace = false;
        } else if wanted {
            let title = self.diff.as_ref().map(super::diff_view::DiffTab::title).unwrap_or_default();
            Self::put_git_tab(session, DIFF_TAB, title);
        } else if present {
            session.state_mut::<Workspaces>().remove_tab(GIT, DIFF_TAB);
        }
        self.git_workspace.diff_tab = self.diff.is_some() && self.diff_from_workspace;
    }

    pub(super) fn git_workspace(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        self.caret = None;
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        if self.vcs.info.root.is_none() {
            ui.painter().text(
                full.center(),
                Align2::CENTER_CENTER,
                if self.safe_mode { t!("ide-vcs-safe-mode") } else { t!("vcs-commit-no-repository") },
                theme::regular(13.0),
                p.text_secondary,
            );
            return;
        }
        self.read_workspace(registry, session);
        let view = &mut self.git_workspace;
        view.drag.over = None;
        view.drag.dropped_on = view.drop_menu_for.as_ref().map(|menu| menu.target.clone());
        view.columns.hover = None;
        let mut asks = Vec::new();
        let toolbar = Rect::from_min_size(full.min, vec2(full.width(), TOOLBAR));
        let pull_open = session.state::<GitWorkspace>().is_some_and(GitWorkspace::pull_menu);
        let pull_mode = session.state::<GitWorkspace>().map(GitWorkspace::pull_mode).unwrap_or_default();
        self.workspace_toolbar(ui, toolbar, &p, pull_open, pull_mode, &mut asks);
        let mut top = toolbar.bottom();
        let list = self.git_workspace.conflicts.clone();
        if list.operation.is_some() || !list.files.is_empty() {
            let banner = Rect::from_min_size(pos2(full.left(), top), vec2(full.width(), BANNER));
            operation_banner(ui, banner, &p, &list, &mut asks);
            top = banner.bottom();
        }
        let body = Rect::from_min_max(pos2(full.left(), top), full.max);
        let left_width = (body.width() * 0.19).clamp(190.0, 260.0);
        let left = Rect::from_min_max(body.min, pos2(body.left() + left_width, body.bottom()));
        let middle = Rect::from_min_max(pos2(left.right() + 1.0, body.top()), body.max);
        ui.painter().hline(full.x_range(), toolbar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        ui.painter().vline(left.right(), body.y_range(), Stroke::new(1.0, p.frame));
        let focused_before = ui.memory(|memory| memory.focused());
        let details_open = session.state::<GitWorkspace>().is_some_and(GitWorkspace::details_open);
        let selected = session.state::<GitWorkspace>().and_then(|w| w.selected_commit().map(str::to_owned));
        let wip = session.state::<GitWorkspace>().is_some_and(GitWorkspace::wip_selected);
        let search = session.state::<GitWorkspace>().map(|w| w.search().clone()).unwrap_or_default();
        let filter = session.state::<GitWorkspace>().map(|w| w.filter().to_owned()).unwrap_or_default();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(left).id_salt("gw-left"));
        self.workspace_sidebar(&mut child, &p, selected.as_deref(), &filter, &mut asks);
        let middle = if search.is_open() {
            let bar = Rect::from_min_size(middle.min, vec2(middle.width(), search::BAR));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar).id_salt("gw-search"));
            self.workspace_search_bar(&mut child, bar, &p, &search, &mut asks);
            Rect::from_min_max(pos2(middle.left(), bar.bottom()), middle.max)
        } else {
            middle
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(middle).id_salt("gw-graph"));
        self.workspace_graph(&mut child, registry, session, &p, selected.as_deref(), wip, &search, &mut asks);
        let dragged = session.state::<GitWorkspace>().and_then(|workspace| workspace.view().columns.details_width);
        let (right, shade) = details_panel(body, middle, dragged);
        #[cfg(test)]
        {
            self.git_workspace.panels = Some((left, details_open.then_some(right)));
        }
        if details_open {
            egui::Area::new(ui.scope_id().with("gw-details-panel"))
                .order(egui::Order::Middle)
                .fixed_pos(right.min)
                .constrain(false)
                .show(ui.ctx(), |area| {
                    area.set_clip_rect(shade);
                    let shadow = egui::Shadow { offset: [-6, 0], blur: 18, spread: 0, color: p.popup_shadow };
                    egui::Frame::new().fill(p.editor_background).shadow(shadow).show(area, |frame| {
                        frame.set_min_size(right.size());
                        frame.set_max_size(right.size());
                        frame.set_clip_rect(right);
                        frame.interact(right, frame.scope_id().with("gw-details-back"), Sense::click());
                        let mut child = frame.new_child(egui::UiBuilder::new().max_rect(right).id_salt("gw-details"));
                        match session.state::<GitWorkspace>().and_then(GitWorkspace::panel).cloned() {
                            Some(RightPanel::Compare { from, to }) => {
                                let points = (from.as_str(), to.as_deref());
                                self.workspace_compare(&mut child, registry, session, &p, points, &mut asks);
                            }
                            Some(RightPanel::FileHistory { path }) => {
                                self.workspace_file_history(&mut child, registry, session, &p, &path, &mut asks);
                            }
                            None if wip => self.workspace_wip(&mut child, &p, &mut asks),
                            None => {
                                let id = selected.as_deref();
                                self.workspace_details(&mut child, registry, session, &p, id, &mut asks);
                            }
                        }
                        if let Some(width) = details_edge(frame, &p, right) {
                            asks.push(Ask::Command("vcs.workspace.view", json!({ "detailsWidth": width.round() })));
                        }
                        if close_button(frame, &p, right) {
                            asks.push(Ask::Command("vcs.workspace.details", json!({ "open": false })));
                        }
                    });
                });
        }
        let view = &mut self.git_workspace;
        if view.file_menu_for.is_some() && view.file_menu_at.is_none() {
            view.file_menu_at = Some(pos2(right.left() + 24.0, right.top() + 40.0));
        }
        compare::file_menu(ui, view, &mut asks);
        let view = &mut self.git_workspace;
        if view.ref_menu_for.is_some() && view.ref_menu_at.is_none() {
            view.ref_menu_at = Some(pos2(left.left() + 24.0, left.top() + 36.0));
        }
        ref_menu(ui, view, &self.vcs.info, &mut asks);
        if view.stash_menu_for.is_some() && view.stash_menu_at.is_none() {
            view.stash_menu_at = Some(pos2(left.left() + 24.0, left.top() + 36.0));
        }
        stash_menu(ui, view, &mut asks);
        for (menu_for, menu_at) in [
            (view.worktree_menu_for.is_some(), &mut view.worktree_menu_at),
            (view.submodule_menu_for.is_some(), &mut view.submodule_menu_at),
        ] {
            if menu_for && menu_at.is_none() {
                *menu_at = Some(pos2(left.left() + 24.0, left.top() + 36.0));
            }
        }
        worktrees::worktree_menu(ui, view, &mut asks);
        worktrees::submodule_menu(ui, view, &mut asks);
        finish_drag(ui, &p, &mut view.drag, &mut asks);
        if view.drop_menu_for.is_some() && view.drop_menu_at.is_none() {
            view.drop_menu_at = Some(pos2(left.left() + 24.0, left.top() + 36.0));
        }
        drop_menu(ui, view, &mut asks);
        let menus = session.state::<GitWorkspace>().is_some_and(|workspace| {
            workspace.menu().is_some()
                || workspace.ref_menu().is_some()
                || workspace.stash_menu().is_some()
                || workspace.drop_menu().is_some()
                || workspace.file_menu().is_some()
                || workspace.pull_menu()
                || workspace.columns_menu()
                || workspace.search().is_open()
        });
        if details_open
            && !menus
            && view.dialog.is_none()
            && view.drag.source.is_none()
            && focused_before.is_none()
            && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            asks.push(Ask::Command("vcs.workspace.details", json!({ "open": false })));
        }
        let refs_changed = asks.iter().any(|ask| {
            matches!(
                ask,
                Ask::Ref(..)
                    | Ask::Checkout(_)
                    | Ask::Stash(..)
                    | Ask::StashChanges { .. }
                    | Ask::Drop(..)
                    | Ask::Worktree(..)
                    | Ask::Submodule(..)
            ) || matches!(ask, Ask::Action(action) if action.starts_with("vcs.history."))
                || matches!(ask, Ask::Command(command, _) if command.starts_with("vcs.rebase.")
                    || command.starts_with("vcs.operation.")
                    || command.starts_with("vcs.conflict."))
        });
        for ask in asks {
            match ask {
                Ask::Select(revision) => {
                    self.dispatch(ui, registry, session, "vcs.workspace.select", json!({ "revision": revision }));
                }
                Ask::Pick(revision) => {
                    let params = json!({ "revision": revision, "toggle": true });
                    self.dispatch(ui, registry, session, "vcs.workspace.select", params);
                }
                Ask::SelectWip => self.dispatch(ui, registry, session, "vcs.workspace.select", json!({ "wip": true })),
                Ask::PickWip => {
                    let params = json!({ "wip": true, "toggle": true });
                    self.dispatch(ui, registry, session, "vcs.workspace.select", params);
                }
                Ask::DiffBetween { base, id, path } => {
                    let params = json!({ "base": base, "id": id, "path": path });
                    self.dispatch(ui, registry, session, "vcs.workspace.openDiff", params);
                }
                Ask::FileMenu(file) => {
                    let params = file.map_or_else(|| json!({}), |(path, id)| json!({ "path": path, "id": id }));
                    self.dispatch(ui, registry, session, "vcs.workspace.fileMenu", params);
                }
                Ask::File(path, action) => {
                    let params = json!({ "path": path, "action": action });
                    self.dispatch(ui, registry, session, "vcs.workspace.fileAction", params);
                }
                Ask::Checkout(name) => {
                    self.dispatch(ui, registry, session, "vcs.branch.checkout", json!({ "name": name }));
                }
                Ask::Diff { id, path, part } => {
                    let params = json!({ "id": id, "path": path, "part": part });
                    self.dispatch(ui, registry, session, "vcs.workspace.openDiff", params);
                }
                Ask::Index { command, params } => {
                    self.dispatch(ui, registry, session, command, params);
                    self.git_workspace.changes_read_for = None;
                    let _ = registry.dispatch(session, "vcs.status.refresh", json!({}));
                }
                Ask::CommitStaged { push } => self.commit_staged(ui, registry, session, push),
                Ask::Amend(on) => {
                    let view = &mut self.git_workspace;
                    view.amend = on;
                    if on && view.summary.trim().is_empty() && view.description.trim().is_empty() {
                        let last = registry.dispatch(session, "vcs.commit.lastMessage", json!({})).ok();
                        if let Some(message) = last.and_then(|last| last["message"].as_str().map(str::to_owned)) {
                            (view.summary, view.description) = message_fields(&message);
                        }
                    }
                }
                Ask::Action(action) => self.run_action(ui, registry, session, action, json!({})),
                Ask::Menu(revision) => {
                    self.dispatch(ui, registry, session, "vcs.workspace.menu", json!({ "revision": revision }));
                }
                Ask::Commit(revision, action) => {
                    let params = json!({ "revision": revision, "action": action });
                    self.dispatch(ui, registry, session, "vcs.workspace.commitAction", params);
                }
                Ask::RefMenu(open) => {
                    let params = match open {
                        Some(RefMenuOpen::Ref(target)) => json!({ "name": target.name, "kind": target.kind }),
                        Some(RefMenuOpen::Labels(id)) => json!({ "revision": id }),
                        None => json!({}),
                    };
                    self.dispatch(ui, registry, session, "vcs.workspace.refMenu", params);
                }
                Ask::Ref(target, action) => {
                    let params = json!({ "name": target.name, "kind": target.kind, "action": action });
                    self.dispatch(ui, registry, session, "vcs.workspace.refAction", params);
                }
                Ask::StashMenu(commit) => {
                    let params = commit.map_or_else(|| json!({}), |commit| json!({ "commit": commit }));
                    self.dispatch(ui, registry, session, "vcs.workspace.stashMenu", params);
                }
                Ask::Stash(commit, action) => {
                    let params = json!({ "commit": commit, "action": action });
                    self.dispatch(ui, registry, session, "vcs.workspace.stashAction", params);
                }
                Ask::DropMenu(drop) => {
                    let params = drop
                        .map_or_else(|| json!({}), |(source, target)| json!({ "source": source, "target": target }));
                    self.dispatch(ui, registry, session, "vcs.workspace.dropMenu", params);
                }
                Ask::Drop(source, target, action) => {
                    let params = json!({ "source": source, "target": target, "action": action });
                    self.dispatch(ui, registry, session, "vcs.workspace.dropAction", params);
                }
                Ask::Search(params) => self.dispatch(ui, registry, session, "vcs.graph.search", params),
                Ask::Filter(text) => {
                    self.dispatch(ui, registry, session, "vcs.workspace.filter", json!({ "text": text }));
                }
                Ask::Focus(command, params) | Ask::Command(command, params) => {
                    self.dispatch(ui, registry, session, command, params);
                }
                Ask::ActionWith(action, params) => self.run_action(ui, registry, session, action, params),
                Ask::WorktreeMenu(path) => {
                    let params = path.map_or_else(|| json!({}), |path| json!({ "path": path }));
                    self.dispatch(ui, registry, session, "vcs.workspace.worktreeMenu", params);
                }
                Ask::Worktree(path, action) => {
                    let params = json!({ "path": path, "action": action });
                    self.dispatch(ui, registry, session, "vcs.workspace.worktreeAction", params);
                }
                Ask::SubmoduleMenu(path) => {
                    let params = path.map_or_else(|| json!({}), |path| json!({ "path": path }));
                    self.dispatch(ui, registry, session, "vcs.workspace.submoduleMenu", params);
                }
                Ask::Submodule(path, action) => {
                    let params = json!({ "path": path, "action": action });
                    self.dispatch(ui, registry, session, "vcs.workspace.submoduleAction", params);
                }
                Ask::StashChanges { staged, message } => {
                    let from_panel = message.is_some();
                    let params = json!({ "message": message, "stagedOnly": staged });
                    match registry.dispatch(session, "vcs.stash.create", params) {
                        Ok(_) if from_panel => self.git_workspace.stash_message.clear(),
                        Ok(_) => {}
                        Err(error) => self.report(ui, error),
                    }
                }
            }
        }
        if refs_changed {
            self.git_workspace.refs_read_for = None;
            self.git_workspace.conflicts_read_for = None;
            self.git_workspace.changes_read_for = None;
        }
        let view = &mut self.git_workspace;
        let hovered = view.columns.hover.clone().filter(|_| view.drag.source.is_none());
        columns::follow_hover(&mut view.columns, registry, session, hovered, view.graph_change);
        self.git_workspace.avatars.follow(ui.ctx(), registry, session);
        self.git_workspace_frame(ui.ctx(), session);
    }

    fn read_workspace(&mut self, registry: &Registry, session: &mut Session) {
        let view = &mut self.git_workspace;
        if let Some(page) = registry
            .dispatch(session, "vcs.graph.page", json!({ "limit": 1 }))
            .ok()
            .and_then(|page| serde_json::from_value::<GraphPage>(page).ok())
        {
            if view.graph_revision == Some(page.rows_revision) && page.total != view.total {
                let last = view.total.saturating_sub(1) / CHUNK;
                view.chunks.retain(|&chunk, _| chunk < last);
            }
            view.graph_change = page.revision;
            if view.graph_revision != Some(page.rows_revision) {
                view.chunks.clear();
                view.graph_revision = Some(page.rows_revision);
                view.focus = registry
                    .dispatch(session, "vcs.graph.focus", json!({}))
                    .ok()
                    .and_then(|info| serde_json::from_value(info).ok())
                    .unwrap_or_default();
            }
            let search = session.state::<GitWorkspace>().map(GitWorkspace::search);
            if search.is_some_and(|search| search.is_open() && search.revision() != Some(page.revision)) {
                let _ = registry.dispatch(session, "vcs.graph.search", json!({}));
            }
            view.total = page.total;
            view.lanes = page.lanes;
            view.computing = page.computing;
            view.loading = page.loading;
            view.error = page.error;
        }
        let key = (self.vcs.info.revision, self.vcs.info.commit.clone(), view.graph_revision);
        if view.refs_read_for.as_ref() != Some(&key) {
            view.branches = registry
                .dispatch(session, "vcs.branch.list", json!({}))
                .ok()
                .and_then(|list| serde_json::from_value(list).ok())
                .unwrap_or_default();
            view.stashes = registry
                .dispatch(session, "vcs.stash.list", json!({}))
                .ok()
                .and_then(|list| serde_json::from_value::<StashList>(list).ok())
                .map(|list| list.stashes)
                .unwrap_or_default();
            view.history = registry
                .dispatch(session, "vcs.history.list", json!({}))
                .ok()
                .and_then(|list| serde_json::from_value(list).ok())
                .unwrap_or_default();
            let _ = registry.dispatch(session, "vcs.submodule.list", json!({ "cached": true, "refresh": true }));
            view.refs_read_for = Some(key);
        }
        if let Some(list) = registry
            .dispatch(session, "vcs.submodule.list", json!({ "cached": true }))
            .ok()
            .and_then(|list| serde_json::from_value::<zen_workbench::vcs::worktree::SubmoduleList>(list).ok())
            && view.submodules_revision != list.revision
        {
            view.submodules = list.submodules;
            view.submodules_revision = list.revision;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |elapsed| elapsed.as_secs_f64());
        let recount = now - view.worktrees_counted >= worktrees::RECOUNT;
        if recount {
            view.worktrees_counted = now;
        }
        if let Some(list) = registry
            .dispatch(session, "vcs.worktree.list", json!({ "refresh": recount }))
            .ok()
            .and_then(|list| serde_json::from_value::<WorktreeList>(list).ok())
            && view.worktrees_revision != Some(list.revision)
        {
            view.worktrees = list.worktrees;
            view.worktrees_revision = Some(list.revision);
        }
        view.picked = session.state::<GitWorkspace>().map(|w| w.picked().to_vec()).unwrap_or_default();
        view.picked_wip = session.state::<GitWorkspace>().is_some_and(GitWorkspace::picked_wip);
        let file_menu = session.state::<GitWorkspace>().and_then(|w| w.file_menu().cloned());
        if file_menu != view.file_menu_for {
            view.file_menu_at = if file_menu.is_some() { view.file_right_click.take() } else { None };
            view.file_right_click = None;
            view.file_menu_keyboard = file_menu.is_some() && view.file_menu_at.is_none();
            view.file_menu_for = file_menu;
        }
        view.collapsed = session.state::<GitWorkspace>().map(|w| w.open_sections().clone()).unwrap_or_default();
        if view.conflicts_read_for != Some(self.vcs.info.revision) {
            view.conflicts = registry
                .dispatch(session, "vcs.conflict.list", json!({}))
                .ok()
                .and_then(|list| serde_json::from_value::<ConflictList>(list).ok())
                .unwrap_or_default();
            view.conflicts_read_for = Some(self.vcs.info.revision);
        }
        if view.changes_read_for != Some(self.vcs.info.revision) {
            view.changes = registry
                .dispatch(session, "vcs.status.list", json!({}))
                .ok()
                .and_then(|report| serde_json::from_value::<zen_workbench::vcs::StatusReport>(report).ok())
                .map(|report| report.changes)
                .unwrap_or_default();
            view.changes_read_for = Some(self.vcs.info.revision);
        }
        let revealed = session.state::<GitWorkspace>().map_or(0, GitWorkspace::revealed);
        if revealed != view.seen_revealed {
            view.seen_revealed = revealed;
            let selected = session.state::<GitWorkspace>().and_then(|w| w.selected_commit().map(str::to_owned));
            let wip = session.state::<GitWorkspace>().is_some_and(GitWorkspace::wip_selected);
            view.reveal = if wip {
                Some(0)
            } else {
                selected
                    .and_then(|id| registry.dispatch(session, "vcs.graph.find", json!({ "revision": id })).ok())
                    .and_then(|found| found["index"].as_u64())
                    .map(|index| index as usize + usize::from(!view.changes.is_empty()))
            };
        }
        let menu = session.state::<GitWorkspace>().and_then(|w| w.menu().map(str::to_owned));
        if menu != view.menu_for {
            view.menu_at = match (&menu, view.right_click.take()) {
                (Some(id), Some((clicked, at))) if *id == clicked => Some(at),
                _ => None,
            };
            view.menu_keyboard = menu.is_some() && view.menu_at.is_none();
            view.menu_info = menu.as_ref().map(|id| {
                let details = registry
                    .dispatch(session, "vcs.commit.details", json!({ "id": id }))
                    .ok()
                    .and_then(|details| serde_json::from_value::<CommitDetailsInfo>(details).ok());
                let head = self.vcs.info.commit.as_deref();
                let at_head = head == Some(id.as_str());
                match &self.vcs.info.branch {
                    Some(branch) => MenuInfo {
                        on_current: at_head
                            || details.is_some_and(|details| {
                                details.contained_in.iter().any(|label| label.kind == "local" && label.name == *branch)
                            }),
                        detached_here: false,
                    },
                    None => MenuInfo { on_current: at_head, detached_here: at_head },
                }
            });
            view.menu_for = menu;
        }
        let ref_menu = session.state::<GitWorkspace>().and_then(|w| w.ref_menu().map(<[RefTarget]>::to_vec));
        if ref_menu != view.ref_menu_for {
            view.ref_menu_at = if ref_menu.is_some() { view.ref_right_click.take() } else { None };
            view.ref_right_click = None;
            view.ref_menu_keyboard = ref_menu.is_some() && view.ref_menu_at.is_none();
            view.ref_menu_for = ref_menu;
        }
        let worktree_menu = session.state::<GitWorkspace>().and_then(|w| w.worktree_menu().map(ToOwned::to_owned));
        if worktree_menu != view.worktree_menu_for {
            view.worktree_menu_at = if worktree_menu.is_some() { view.worktree_right_click.take() } else { None };
            view.worktree_right_click = None;
            view.worktree_menu_keyboard = worktree_menu.is_some() && view.worktree_menu_at.is_none();
            view.worktree_menu_for = worktree_menu;
        }
        let submodule_menu = session.state::<GitWorkspace>().and_then(|w| w.submodule_menu().map(str::to_owned));
        if submodule_menu != view.submodule_menu_for {
            view.submodule_menu_at = if submodule_menu.is_some() { view.submodule_right_click.take() } else { None };
            view.submodule_right_click = None;
            view.submodule_menu_keyboard = submodule_menu.is_some() && view.submodule_menu_at.is_none();
            view.submodule_menu_for = submodule_menu;
        }
        let stash_menu = session.state::<GitWorkspace>().and_then(|w| w.stash_menu().map(str::to_owned));
        if stash_menu != view.stash_menu_for {
            view.stash_menu_at = if stash_menu.is_some() { view.stash_right_click.take() } else { None };
            view.stash_right_click = None;
            view.stash_menu_keyboard = stash_menu.is_some() && view.stash_menu_at.is_none();
            view.stash_menu_for = stash_menu;
        }
        let drop_menu = session.state::<GitWorkspace>().and_then(|w| w.drop_menu().cloned());
        if drop_menu != view.drop_menu_for {
            view.drop_menu_at = if drop_menu.is_some() { view.drag.dropped_at.take() } else { None };
            view.drag.dropped_at = None;
            view.drop_menu_keyboard = drop_menu.is_some() && view.drop_menu_at.is_none();
            view.drop_menu_for = drop_menu;
        }
    }

    fn workspace_toolbar(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        p: &IdePalette,
        pull_open: bool,
        pull_mode: PullMode,
        asks: &mut Vec<Ask>,
    ) {
        let (changes, stashes) = (!self.git_workspace.changes.is_empty(), !self.git_workspace.stashes.is_empty());
        let history = self.git_workspace.history.clone();
        let buttons = toolbar_buttons(changes, stashes, &history);
        let caption_font = theme::regular(11.5);
        let measure = ui.painter().clone();
        let caption_width = |text: &str| measure.layout_no_wrap(text.to_owned(), caption_font.clone(), p.text).size().x;
        let captions: Vec<f32> = buttons.iter().map(|button| caption_width(&t!(button.label))).collect();
        let search_caption = caption_width(&t!("vcs-workspace-search"));
        let gaps: f32 = buttons.iter().map(|button| button.gap).sum();
        let row = |compact: bool| buttons_width(&captions, compact) * buttons.len() as f32 + gaps;
        let compact = rect.left() + 24.0 + row(false) + tool_width(search_caption, false) + 24.0 > rect.right();
        let right_edge = rect.right() - 8.0;
        let search_rect = Rect::from_min_max(
            pos2(right_edge - tool_width(search_caption, compact), rect.top() + 4.0),
            pos2(right_edge, rect.bottom() - 4.0),
        );
        let search_button = ToolButton {
            glyph: glyph_search,
            label: "vcs-workspace-search",
            action: "view.find.open",
            enabled: true,
            gap: 0.0,
        };
        if tool_button(ui, p, search_rect, &search_button, compact, &history, pull_mode).clicked() {
            asks.push(Ask::Action("view.find.open"));
        }
        let width = buttons_width(&captions, compact);
        let mut x = rect.left() + 8.0;
        for button in &buttons {
            x += button.gap;
            let rect = Rect::from_min_max(pos2(x, rect.top() + 4.0), pos2(x + width, rect.bottom() - 4.0));
            if button.gap > 0.0 {
                let line = rect.left() - button.gap / 2.0;
                ui.painter().vline(line, rect.y_range().shrink(6.0), Stroke::new(1.0, p.frame));
            }
            x = rect.right();
            let response = tool_button(ui, p, rect, button, compact, &history, pull_mode);
            if button.action == "vcs.workspace.pull" {
                let arrow = pull_arrow(rect);
                let menu = ui.interact(arrow, ui.scope_id().with("gw-pull-menu"), Sense::click());
                if menu.hovered() {
                    ui.painter().rect_filled(arrow, 4, p.hover);
                }
                if !pull_open {
                    hint(ui, &menu, &t!("vcs-workspace-pull-options"), None, Side::Below);
                }
                crate::icons::chevron_down(ui.painter(), arrow.shrink2(vec2(2.0, 4.0)), p.text_secondary);
                if menu.clicked() {
                    asks.push(Ask::Command("vcs.workspace.pullMenu", json!({ "open": !pull_open })));
                } else if pull_open {
                    pull_menu(ui, p, pos2(rect.left(), rect.bottom() + 2.0), pull_mode, asks);
                }
                if response.clicked() && !menu.hovered() {
                    asks.push(Ask::Command("vcs.workspace.pull", json!({})));
                }
                continue;
            }
            if response.clicked() {
                asks.push(match button.action {
                    "vcs.stash.create" => Ask::StashChanges { staged: false, message: None },
                    "vcs.stash.pop" => Ask::Stash(None, StashAction::Pop),
                    action => Ask::Action(action),
                });
            }
        }
    }

    fn workspace_sidebar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        selected: Option<&str>,
        filter: &str,
        asks: &mut Vec<Ask>,
    ) {
        let view = &mut self.git_workspace;
        let shows = zen_workbench::vcs::graph_search::PanelFilter::new(filter);
        let viewing = view.branches.local.iter().filter(|branch| shows.shows(&branch.name)).count()
            + view.branches.remote.iter().filter(|branch| shows.shows(&branch.name)).count()
            + view.branches.tags.iter().filter(|tag| shows.shows(&tag.name)).count()
            + view.stashes.iter().filter(|stash| shows.shows(&stash.name) || shows.shows(&stash.message)).count()
            + worktrees::shown_worktrees(view, &shows).len()
            + worktrees::shown_submodules(view, &shows).len();
        viewing_row(ui, p, viewing);
        search::filter_field(ui, p, view, filter, asks);
        if !view.focus.is_empty() {
            search::focus_summary(ui, p, &view.focus, asks);
        }
        let filtering = shows.is_active();
        let focus = view.focus.clone();
        let sections_before = view.collapsed.clone();
        egui::ScrollArea::vertical().id_salt("gw-sidebar").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let local: Vec<&zen_workbench::vcs::BranchInfo> =
                view.branches.local.iter().filter(|branch| shows.shows(&branch.name)).collect();
            if !(filtering && local.is_empty())
                && section_header(
                    ui,
                    p,
                    &mut view.collapsed,
                    filtering,
                    Section::Local,
                    &t!("vcs-workspace-local"),
                    local.len(),
                )
            {
                let names: Vec<&str> = local.iter().map(|branch| branch.name.as_str()).collect();
                for row in look::tree_rows(&names, "local", &view.collapsed_folders, filtering) {
                    let (index, depth, short) = match row {
                        TreeRow::Folder { key, name, depth, open, count } => {
                            folder_row(
                                ui,
                                p,
                                &mut view.collapsed_folders,
                                (&key, &name),
                                depth as f32 * 14.0,
                                open,
                                count,
                            );
                            continue;
                        }
                        TreeRow::Leaf { index, depth, name } => (index, depth, name),
                    };
                    let branch = local[index];
                    let mut badge = String::new();
                    if branch.ahead > 0 {
                        badge.push_str(&format!("↑{}", branch.ahead));
                    }
                    if branch.behind > 0 {
                        badge.push_str(&format!(" ↓{}", branch.behind));
                    }
                    let row = RefRow {
                        name: &short,
                        kind: RefKind::Local,
                        current: branch.current,
                        selected: selected == Some(branch.commit.as_str()),
                        badge: badge.trim(),
                        indent: depth as f32 * 14.0,
                        focus: search::RowFocus::of(&focus, &branch.name, RefKind::Local),
                    };
                    let target = RefTarget { name: branch.name.clone(), kind: RefKind::Local };
                    let (response, button) = ref_row(ui, p, &row, &mut view.drag, &target);
                    if response.hovered() {
                        view.columns.hover = Some(columns::Hovered::Branch(target.clone()));
                    }
                    place_drop_menu(&view.drop_menu_for, &mut view.drop_menu_at, &target, &response.rect);
                    if let Some(button) = button {
                        asks.push(search::focus_ask(button, &branch.name, FocusKind::Local, row.focus));
                        continue;
                    }
                    hint(ui, &response, &branch.name, branch.upstream.as_deref(), Side::Right);
                    if response.double_clicked() {
                        asks.push(Ask::Checkout(branch.name.clone()));
                    } else if response.clicked() {
                        asks.push(Ask::Select(branch.name.clone()));
                    }
                    ref_row_menu(
                        &response,
                        target,
                        &view.ref_menu_for,
                        &mut view.ref_menu_at,
                        &mut view.ref_right_click,
                        asks,
                    );
                }
            }
            let remote: Vec<&zen_workbench::vcs::BranchInfo> =
                view.branches.remote.iter().filter(|branch| shows.shows(&branch.name)).collect();
            if !(filtering && remote.is_empty())
                && section_header(
                    ui,
                    p,
                    &mut view.collapsed,
                    filtering,
                    Section::Remote,
                    &t!("vcs-workspace-remote"),
                    remote.len(),
                )
            {
                let mut remotes: Vec<&str> = Vec::new();
                for branch in &remote {
                    let name = branch.name.split_once('/').map_or("", |(remote, _)| remote);
                    if !remotes.contains(&name) {
                        remotes.push(name);
                    }
                }
                let mut rows = Vec::new();
                for remote_name in remotes {
                    rows.push((remote_name, None));
                    let branches: Vec<&zen_workbench::vcs::BranchInfo> = remote
                        .iter()
                        .copied()
                        .filter(|branch| branch.name.split_once('/').map_or("", |(remote, _)| remote) == remote_name)
                        .collect();
                    let names: Vec<&str> = branches
                        .iter()
                        .map(|branch| branch.name.split_once('/').map_or(branch.name.as_str(), |(_, name)| name))
                        .collect();
                    let scope = format!("remote:{remote_name}");
                    for row in look::tree_rows(&names, &scope, &view.collapsed_folders, filtering) {
                        let branch = match &row {
                            TreeRow::Leaf { index, .. } => Some(branches[*index]),
                            TreeRow::Folder { .. } => None,
                        };
                        rows.push((remote_name, Some((row, branch))));
                    }
                }
                for (remote_name, row) in rows {
                    let Some((row, branch)) = row else {
                        let row_focus = search::RowFocus::of_remote(&focus, remote_name);
                        if let Some(button) = search::remote_row(ui, p, remote_name, row_focus) {
                            asks.push(search::focus_ask(button, remote_name, FocusKind::WholeRemote, row_focus));
                        }
                        continue;
                    };
                    let (depth, short, branch) = match (row, branch) {
                        (TreeRow::Folder { key, name, depth, open, count }, _) => {
                            let indent = 14.0 + depth as f32 * 14.0;
                            folder_row(ui, p, &mut view.collapsed_folders, (&key, &name), indent, open, count);
                            continue;
                        }
                        (TreeRow::Leaf { depth, name, .. }, Some(branch)) => (depth, name, branch),
                        (TreeRow::Leaf { .. }, None) => continue,
                    };
                    let row = RefRow {
                        name: &short,
                        kind: RefKind::Remote,
                        current: false,
                        selected: selected == Some(branch.commit.as_str()),
                        badge: "",
                        indent: 14.0 + depth as f32 * 14.0,
                        focus: search::RowFocus::of(&focus, &branch.name, RefKind::Remote),
                    };
                    let target = RefTarget { name: branch.name.clone(), kind: RefKind::Remote };
                    let (response, button) = ref_row(ui, p, &row, &mut view.drag, &target);
                    if response.hovered() {
                        view.columns.hover = Some(columns::Hovered::Branch(target.clone()));
                    }
                    place_drop_menu(&view.drop_menu_for, &mut view.drop_menu_at, &target, &response.rect);
                    if let Some(button) = button {
                        asks.push(search::focus_ask(button, &branch.name, FocusKind::Remote, row.focus));
                        continue;
                    }
                    if response.double_clicked() {
                        asks.push(Ask::Checkout(branch.name.clone()));
                    } else if response.clicked() {
                        asks.push(Ask::Select(branch.name.clone()));
                    }
                    ref_row_menu(
                        &response,
                        target,
                        &view.ref_menu_for,
                        &mut view.ref_menu_at,
                        &mut view.ref_right_click,
                        asks,
                    );
                }
            }
            let tags: Vec<&zen_workbench::vcs::TagInfo> =
                view.branches.tags.iter().filter(|tag| shows.shows(&tag.name)).collect();
            if !(filtering && tags.is_empty())
                && section_header(
                    ui,
                    p,
                    &mut view.collapsed,
                    filtering,
                    Section::Tags,
                    &t!("vcs-workspace-tags"),
                    tags.len(),
                )
            {
                for tag in tags {
                    let row = RefRow {
                        name: &tag.name,
                        kind: RefKind::Tag,
                        current: false,
                        selected: selected == Some(tag.commit.as_str()),
                        badge: "",
                        indent: 0.0,
                        focus: search::RowFocus::default(),
                    };
                    let target = RefTarget { name: tag.name.clone(), kind: RefKind::Tag };
                    let (response, _) = ref_row(ui, p, &row, &mut view.drag, &target);
                    if response.hovered() {
                        view.columns.hover = Some(columns::Hovered::Branch(target.clone()));
                    }
                    place_drop_menu(&view.drop_menu_for, &mut view.drop_menu_at, &target, &response.rect);
                    if response.clicked() {
                        asks.push(Ask::Select(tag.name.clone()));
                    }
                    ref_row_menu(
                        &response,
                        target,
                        &view.ref_menu_for,
                        &mut view.ref_menu_at,
                        &mut view.ref_right_click,
                        asks,
                    );
                }
            }
            let stashes: Vec<&StashInfo> =
                view.stashes.iter().filter(|stash| shows.shows(&stash.name) || shows.shows(&stash.message)).collect();
            if !(filtering && stashes.is_empty())
                && section_header(
                    ui,
                    p,
                    &mut view.collapsed,
                    filtering,
                    Section::Stashes,
                    &t!("vcs-workspace-stashes"),
                    stashes.len(),
                )
            {
                for stash in stashes {
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
                    let is_selected = selected == Some(stash.commit.as_str());
                    if is_selected {
                        ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.hover);
                    }
                    glyph_stash(
                        ui.painter(),
                        Rect::from_center_size(pos2(row.left() + 26.0, row.center().y), Vec2::splat(12.0)),
                        p.text_secondary,
                    );
                    ui.painter().with_clip_rect(row.shrink2(vec2(6.0, 0.0))).text(
                        pos2(row.left() + 40.0, row.center().y),
                        Align2::LEFT_CENTER,
                        &stash.message,
                        theme::regular(13.0),
                        p.text,
                    );
                    hint(ui, &response, &stash.name, Some(&stash.message), Side::Right);
                    if response.clicked() {
                        asks.push(Ask::Select(stash.commit.clone()));
                    }
                    if view.stash_menu_at.is_none() && view.stash_menu_for.as_deref() == Some(stash.commit.as_str()) {
                        view.stash_menu_at = Some(pos2(row.left() + 32.0, row.bottom()));
                    }
                    if response.secondary_clicked() {
                        view.stash_right_click = Some(response.interact_pointer_pos().unwrap_or(row.center()));
                        asks.push(Ask::StashMenu(Some(stash.commit.clone())));
                    }
                }
            }
            let worktree_rows: Vec<WorktreeInfo> =
                worktrees::shown_worktrees(view, &shows).into_iter().cloned().collect();
            if !(filtering && worktree_rows.is_empty())
                && section_header(
                    ui,
                    p,
                    &mut view.collapsed,
                    filtering,
                    Section::Worktrees,
                    &t!("vcs-workspace-worktrees"),
                    worktree_rows.len(),
                )
            {
                for worktree in &worktree_rows {
                    worktrees::worktree_row(ui, p, view, worktree, selected, asks);
                }
            }
            let submodule_rows: Vec<SubmoduleInfo> =
                worktrees::shown_submodules(view, &shows).into_iter().cloned().collect();
            if !(filtering && submodule_rows.is_empty())
                && section_header(
                    ui,
                    p,
                    &mut view.collapsed,
                    filtering,
                    Section::Submodules,
                    &t!("vcs-workspace-submodules"),
                    submodule_rows.len(),
                )
            {
                for submodule in &submodule_rows {
                    worktrees::submodule_row(ui, p, view, submodule, asks);
                }
            }
            if filtering
                && [&view.branches.local, &view.branches.remote]
                    .iter()
                    .all(|list| list.iter().all(|b| !shows.shows(&b.name)))
                && view.branches.tags.iter().all(|tag| !shows.shows(&tag.name))
                && view.stashes.iter().all(|stash| !shows.shows(&stash.name) && !shows.shows(&stash.message))
                && worktree_rows.is_empty()
                && submodule_rows.is_empty()
            {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(t!("vcs-workspace-filter-nothing")).color(p.text_secondary));
                });
            }
            ui.add_space(10.0);
        });
        for section in sections_before.symmetric_difference(&view.collapsed) {
            let open = view.collapsed.contains(section);
            asks.push(Ask::Command("vcs.workspace.section", json!({ "section": section, "open": open })));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn workspace_graph(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
        selected: Option<&str>,
        wip_selected: bool,
        search: &SearchState,
        asks: &mut Vec<Ask>,
    ) {
        let width = ui.available_width() - 10.0;
        let matcher = self.git_workspace.matcher(search);
        let workspace = session.state::<GitWorkspace>();
        let settings = workspace.map(|workspace| workspace.view().columns.clone()).unwrap_or_default();
        let hover = workspace.and_then(GitWorkspace::hover).cloned();
        let scroll = workspace.map(GitWorkspace::graph_scroll).unwrap_or_default();
        let dragged = settings.widths.get(&GraphColumn::Graph).copied();
        let fit = GraphFit::new(dragged, self.git_workspace.lanes, scroll);
        let (header, _) = ui.allocate_exact_size(vec2(width, HEADER), Sense::hover());
        let columns = Columns::new(header, &settings, fit);
        columns::header(ui, p, header, &columns, &mut self.git_workspace.columns, asks);

        let view = &mut self.git_workspace;
        if view.total == 0 {
            let text = match (&view.error, view.computing) {
                (Some(error), _) => error.clone(),
                (None, true) => t!("vcs-log-loading"),
                (None, false) => t!("vcs-log-empty"),
            };
            ui.add_space(16.0);
            ui.label(egui::RichText::new(text).color(p.text_secondary));
            return;
        }
        let wip_rows = usize::from(!view.changes.is_empty());
        let rows = view.total + wip_rows;
        let scroll = &mut ui.spacing_mut().scroll;
        (scroll.floating, scroll.bar_width, scroll.bar_inner_margin) = (false, 8.0, 2.0);
        (scroll.dormant_handle_opacity, scroll.active_handle_opacity) = (0.5, 0.7);
        let mut area = egui::ScrollArea::vertical()
            .id_salt("gw-rows")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible);
        if let Some(row) = view.reveal.take()
            && !(row > view.visible.start && row + 1 < view.visible.end)
        {
            let height = ui.available_height();
            let offset = (row as f32 * ROW - height / 2.0 + ROW).max(0.0);
            area = area.vertical_scroll_offset(offset);
        }
        let head = self.vcs.info.commit.clone();
        let changes = view.changes.len();
        let mut moved: Option<isize> = None;
        let rows_rect = ui.available_rect_before_wrap();
        self.free_scroll.push(rows_rect);
        ui.spacing_mut().item_spacing.y = 0.0;
        area.show_rows(ui, ROW, rows, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            view.visible = range.clone();
            let first = range.start.saturating_sub(wip_rows);
            let last = range.end.saturating_sub(wip_rows).min(view.total);
            for chunk in first / CHUNK..=last.saturating_sub(1) / CHUNK {
                if view.chunks.contains_key(&chunk) {
                    continue;
                }
                let page = registry
                    .dispatch(session, "vcs.graph.page", json!({ "skip": chunk * CHUNK, "limit": CHUNK }))
                    .ok()
                    .and_then(|page| serde_json::from_value::<GraphPage>(page).ok());
                if let Some(page) = page {
                    view.chunks.insert(chunk, page.commits);
                }
            }
            let top = view.chunks.get(&0).map(|rows| wip_head(rows, head.as_deref())).unwrap_or_default();
            for index in range {
                let commit = index
                    .checked_sub(wip_rows)
                    .and_then(|number| view.chunks.get(&(number / CHUNK)).and_then(|rows| rows.get(number % CHUNK)));
                let labels = commit
                    .filter(|commit| commit.stash.is_none() && commit.worktree.is_none())
                    .map(commit_labels)
                    .unwrap_or_default();
                let sense = if labels.is_empty() { Sense::click() } else { Sense::click_and_drag() };
                let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), sense);
                let columns = Columns::new(rect, &settings, fit);
                let dimmed = |row: usize| hover.as_ref().and_then(|hover| hover.highlights(row)) == Some(false);
                if index < wip_rows {
                    let marked = wip_selected || view.picked_wip;
                    draw_wip_row(ui, p, &columns, rect, &response, marked, changes, top.head);
                    if dimmed(top.head.map_or(usize::MAX, |head| head.index)) {
                        columns::dim(ui.painter(), p, rect);
                    }
                    if response.clicked() {
                        asks.push(if ui.input(|input| input.modifiers.command) {
                            Ask::PickWip
                        } else {
                            Ask::SelectWip
                        });
                    }
                    continue;
                }
                let number = index - wip_rows;
                let Some(commit) = commit else { continue };
                if let Some(path) = &commit.worktree {
                    let worktree = view.worktrees.iter().find(|worktree| &worktree.path == path);
                    let changes = worktree.and_then(|worktree| worktree.changes);
                    let is_selected = view.worktree_menu_for.as_ref() == Some(path);
                    worktrees::draw_worktree_wip_row(ui, p, &columns, rect, &response, commit, changes, is_selected);
                    if dimmed(commit.index) {
                        columns::dim(ui.painter(), p, rect);
                    }
                    if let Some(worktree) = worktree {
                        hint(
                            ui,
                            &response,
                            &path.display().to_string(),
                            Some(&worktrees::worktree_detail(worktree)),
                            Side::Right,
                        );
                    }
                    if response.double_clicked() {
                        asks.push(Ask::Worktree(path.clone(), WorktreeAction::Open));
                    } else if response.clicked()
                        && let Some(base) = commit.parents.first()
                    {
                        asks.push(Ask::Select(base.clone()));
                    }
                    if response.secondary_clicked() {
                        view.worktree_right_click = Some(response.interact_pointer_pos().unwrap_or(rect.center()));
                        asks.push(Ask::WorktreeMenu(Some(path.clone())));
                    }
                    if view.worktree_menu_at.is_none() && view.worktree_menu_for.as_ref() == Some(path) {
                        view.worktree_menu_at = Some(pos2(columns.message.0 + 10.0, rect.bottom()));
                    }
                    continue;
                }
                let dropping = label_drag(ui, &mut view.drag, &response, columns.refs, &labels);
                let is_selected = selected == Some(commit.id.as_str());
                let found = match search.current_row() {
                    Some(current) if current == commit.index => Found::Current,
                    _ if search.is_match(commit.index) => Found::Match,
                    _ => Found::No,
                };
                let matcher = matcher.as_ref().filter(|_| found != Found::No);
                let is_selected = is_selected || view.picked.contains(&commit.id);
                let picture = if commit.stash.is_none() { view.avatars.picture(&commit.email) } else { None };
                let marks = (is_selected, found, matcher);
                draw_commit_row(ui, p, &columns, rect, &response, commit, marks, picture.as_ref());
                if response.hovered()
                    && let Some(at) = response.hover_pos()
                {
                    let pill = look::pills(&commit.refs).into_iter().next();
                    view.columns.hover = Some(match pill {
                        Some(pill) if at.x >= columns.refs.0 && at.x < columns.refs.1 && commit.stash.is_none() => {
                            columns::Hovered::Branch(RefTarget { name: pill.name, kind: pill.kind })
                        }
                        _ => columns::Hovered::Commit(commit.id.clone()),
                    });
                }
                let nearest = hover
                    .as_ref()
                    .filter(|hover| hover.info.revision.as_deref() == Some(commit.id.as_str()))
                    .and_then(|hover| hover.info.nearest.as_ref());
                if let Some(branch) = nearest.filter(|_| commit.refs.is_empty() && commit.stash.is_none()) {
                    let painter = ui
                        .painter()
                        .with_clip_rect(Rect::from_x_y_ranges(columns.refs.0..=columns.refs.1 - 4.0, rect.y_range()));
                    let color = lane_color(p, commit.color);
                    columns::draw_nearest(&painter, p, columns.refs, rect.center().y, branch, color);
                }
                if dimmed(commit.index) {
                    columns::dim(ui.painter(), p, rect);
                }
                if dropping {
                    let labels = Rect::from_x_y_ranges(columns.refs.0 + 4.0..=columns.refs.1 - 4.0, rect.y_range());
                    drop_highlight(&ui.painter().with_clip_rect(rect), p, labels.shrink2(vec2(0.0, 2.0)));
                }
                if let Some(target) = drop_label(&labels) {
                    let under = Rect::from_x_y_ranges(columns.refs.0 + 8.0..=columns.refs.1, rect.y_range());
                    place_drop_menu(&view.drop_menu_for, &mut view.drop_menu_at, target, &under);
                }
                if wip_rows > 0
                    && number < top.above
                    && let Some(head) = top.head
                    && columns.lines().is_some()
                {
                    let x = columns.node_x(head.lane);
                    let painter = ui.painter().with_clip_rect(rect);
                    dashed(&painter, pos2(x, rect.top()), pos2(x, rect.bottom()), lane_color(p, head.color));
                }
                if response.clicked() {
                    if ui.input(|input| input.modifiers.command) && commit.stash.is_none() {
                        asks.push(Ask::Pick(commit.id.clone()));
                    } else {
                        asks.push(Ask::Select(commit.id.clone()));
                    }
                }
                if commit.stash.is_some() {
                    if response.secondary_clicked() {
                        view.stash_right_click = Some(response.interact_pointer_pos().unwrap_or(rect.center()));
                        asks.push(Ask::StashMenu(Some(commit.id.clone())));
                    }
                    if view.stash_menu_at.is_none() && view.stash_menu_for.as_deref() == Some(commit.id.as_str()) {
                        view.stash_menu_at = Some(pos2(columns.message.0 + 10.0, rect.bottom()));
                    }
                    continue;
                }
                if response.secondary_clicked() {
                    let at = response.interact_pointer_pos().unwrap_or(rect.center());
                    if !labels.is_empty() && at.x >= columns.refs.0 && at.x < columns.refs.1 {
                        view.ref_right_click = Some(at);
                        asks.push(Ask::RefMenu(Some(RefMenuOpen::Labels(commit.id.clone()))));
                    } else {
                        view.right_click = Some((commit.id.clone(), at));
                        asks.push(Ask::Menu(Some(commit.id.clone())));
                    }
                }
                if view.menu_for.as_deref() == Some(commit.id.as_str()) && view.menu_at.is_none() {
                    view.menu_at = Some(pos2(columns.message.0 + 10.0, rect.bottom()));
                }
                if view.ref_menu_at.is_none() && !labels.is_empty() && view.ref_menu_for.as_ref() == Some(&labels) {
                    view.ref_menu_at = Some(pos2(columns.refs.0 + 8.0, rect.bottom()));
                }
                if response.double_clicked()
                    && let Some(branch) = commit.refs.iter().find(|label| label.kind == RefKind::Local)
                {
                    asks.push(Ask::Checkout(branch.name.clone()));
                }
            }
        });
        let lanes_columns = Columns::new(rows_rect, &settings, fit);
        columns::graph_border(ui, p, rows_rect, &lanes_columns, asks);
        columns::graph_scroll(ui, p, rows_rect, &lanes_columns, asks);
        let below = rows_rect.top() + rows as f32 * ROW;
        if below < rows_rect.bottom() - 4.0 {
            let empty = Rect::from_min_max(pos2(rows_rect.left(), below), rows_rect.max);
            if ui.interact(empty, ui.scope_id().with("gw-graph-empty"), Sense::click()).clicked() {
                asks.push(Ask::Command("vcs.workspace.details", json!({ "open": false })));
            }
        }
        if view.loading {
            let painter = ui.painter_at(rows_rect);
            let galley = painter.layout_no_wrap(
                t!("vcs-workspace-reading", count = view.total),
                theme::regular(12.0),
                p.text_secondary,
            );
            let plate = Rect::from_center_size(
                pos2(rows_rect.center().x, rows_rect.bottom() - 22.0),
                galley.size() + vec2(24.0, 12.0),
            );
            painter.rect(plate, 6, p.popup, Stroke::new(1.0, p.popup_border), egui::StrokeKind::Inside);
            painter.galley(plate.center() - galley.size() / 2.0, galley, p.text_secondary);
        }
        if view.menu_for.is_none()
            && view.ref_menu_for.is_none()
            && view.stash_menu_for.is_none()
            && view.drop_menu_for.is_none()
            && view.worktree_menu_for.is_none()
            && view.submodule_menu_for.is_none()
            && view.file_menu_for.is_none()
            && view.dialog.is_none()
            && ui.memory(|memory| memory.focused().is_none())
        {
            ui.input(|input| {
                if input.key_pressed(egui::Key::ArrowDown) {
                    moved = Some(1);
                } else if input.key_pressed(egui::Key::ArrowUp) {
                    moved = Some(-1);
                }
            });
        }
        if let Some(step) = moved {
            let current = if wip_selected {
                Some(0)
            } else {
                selected.and_then(|id| {
                    view.chunks.values().flatten().find(|commit| commit.id == id).map(|commit| commit.index + wip_rows)
                })
            };
            let next = current.map_or(0, |row| (row as isize + step).clamp(0, rows as isize - 1) as usize);
            if next < wip_rows {
                asks.push(Ask::SelectWip);
            } else if let Some(commit) =
                view.chunks.get(&((next - wip_rows) / CHUNK)).and_then(|rows| rows.get((next - wip_rows) % CHUNK))
            {
                asks.push(Ask::Select(commit.id.clone()));
            }
        }
        commit_menu(ui, view, &self.vcs.info, asks);
        if session.state::<GitWorkspace>().is_some_and(GitWorkspace::columns_menu) {
            columns::columns_menu(ui, p, &settings, &mut view.columns, asks);
        }
    }

    fn workspace_details(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
        selected: Option<&str>,
        asks: &mut Vec<Ask>,
    ) {
        let view = &mut self.git_workspace;
        let Some(id) = selected else {
            if !view.changes.is_empty() && changes_banner(ui, p, view.changes.len()) {
                asks.push(Ask::SelectWip);
            }
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(t!("vcs-workspace-select-hint")).color(p.text_secondary));
            });
            return;
        };
        if view.details.as_ref().is_none_or(|details| details.id != id) {
            view.details = registry
                .dispatch(session, "vcs.commit.details", json!({ "id": id }))
                .ok()
                .and_then(|details| serde_json::from_value(details).ok());
        }
        let Some(details) = view.details.clone() else { return };
        let stash = view.stashes.iter().find(|stash| stash.commit == details.id).cloned();
        let (kind, name, parents, parents_label) = match &stash {
            Some(stash) => (
                t!("vcs-workspace-stash-kind"),
                stash.name.clone(),
                vec![stash.base.clone()],
                t!("vcs-workspace-stash-base"),
            ),
            None => (
                t!("vcs-workspace-commit"),
                details.id.chars().take(7).collect(),
                details.parents.clone(),
                t!("vcs-workspace-parents", count = details.parents.len()),
            ),
        };
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let settings = session.state::<GitWorkspace>().map(|workspace| workspace.view().clone()).unwrap_or_default();
        if settings.all_files && view.all_files.as_ref().is_none_or(|(of, _)| *of != details.id) {
            let files = registry
                .dispatch(session, "vcs.commit.files", json!({ "id": details.id }))
                .ok()
                .and_then(|list| {
                    serde_json::from_value::<zen_workbench::vcs::workspace_view::CommitFileList>(list).ok()
                })
                .map(|list| list.files)
                .unwrap_or_default();
            view.all_files = Some((details.id.clone(), files));
        }
        let changes = view.changes.len();
        let picture = view.avatars.picture(&details.email);
        egui::ScrollArea::vertical().id_salt("gw-details-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if changes > 0 && changes_banner(ui, p, changes) {
                asks.push(Ask::SelectWip);
            }
            let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
            let label = ui.painter().layout_no_wrap(format!("{kind}:"), theme::regular(13.0), p.text_secondary);
            let value = ui.painter().layout_no_wrap(name.clone(), FontId::monospace(13.0), p.text);
            let start = row.center().x - (label.size().x + 6.0 + value.size().x) / 2.0;
            let label_width = label.size().x;
            ui.painter().galley(pos2(start, row.center().y - label.size().y / 2.0), label, p.text_secondary);
            ui.painter().galley(pos2(start + label_width + 6.0, row.center().y - value.size().y / 2.0), value, p.text);
            ui.painter().hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, p.frame));
            hint(ui, &response, &details.id, Some(&t!("vcs-workspace-copy-id")), Side::Below);
            if response.clicked() {
                asks.push(Ask::Command("vcs.commit.copy", json!({ "revision": details.id })));
            }
            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 4 }).show(ui, |ui| {
                egui::Frame::new()
                    .stroke(Stroke::new(1.0, p.frame))
                    .corner_radius(6)
                    .inner_margin(egui::Margin { left: 14, right: 14, top: 10, bottom: 12 })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&details.subject).font(theme::regular(17.0)).color(p.text),
                            )
                            .wrap(),
                        );
                        if !details.body.is_empty() {
                            ui.add_space(8.0);
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(details.body.trim_end())
                                        .font(theme::regular(13.0))
                                        .color(p.text),
                                )
                                .wrap(),
                            );
                        }
                    });
                ui.add_space(14.0);
                let wide = ui.available_width() >= 330.0;
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (badge, avatar) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                    let face = (badge.center(), 20.0);
                    avatars::draw_face(ui.painter(), p, face, &details.author, 15.0, picture.as_ref());
                    hint(ui, &avatar, &details.author, Some(&details.email), Side::Below);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(egui::RichText::new(&details.author).font(theme::regular(14.5)).color(p.text));
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 5.0;
                            ui.label(
                                egui::RichText::new(t!("vcs-workspace-authored"))
                                    .font(theme::regular(12.5))
                                    .color(p.text_secondary),
                            );
                            ui.label(
                                egui::RichText::new(format_date(details.time + i64::from(details.offset)))
                                    .font(theme::regular(12.5))
                                    .color(p.text),
                            );
                        });
                        if !wide {
                            ui.horizontal_wrapped(|ui| parent_links(ui, p, &parents_label, &parents, asks, false));
                        }
                    });
                    if wide {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            parent_links(ui, p, &parents_label, &parents, asks, true);
                        });
                    }
                });
            });
            ui.add_space(10.0);
            let files = &details.files;
            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 4, bottom: 4 }).show(ui, |ui| {
                file_counts(ui, p, files);
                ui.add_space(8.0);
                if let Some(ask) = files_toolbar(ui, p, settings.clone(), true) {
                    asks.push(ask);
                }
            });
            ui.add_space(4.0);
            let all = settings.all_files.then(|| view.all_files.as_ref().map(|(_, all)| all.as_slice())).flatten();
            let rows = commit_file_rows(files, all);
            let collapsed = &mut view.collapsed_folders;
            let menu =
                view.file_menu_for.as_ref().map(|target| target.path.as_path()).zip(Some(&mut view.file_menu_at));
            match draw_commit_files(ui, p, &rows, (settings.files, &root), collapsed, menu) {
                Some(FilePick::Open(path)) => asks.push(Ask::Diff { id: Some(details.id.clone()), path, part: None }),
                Some(FilePick::Menu(path, at)) => {
                    view.file_right_click = Some(at);
                    asks.push(Ask::FileMenu(Some((path, Some(details.id.clone())))));
                }
                None => {}
            }
            ui.add_space(10.0);
        });
    }

    fn workspace_wip(&mut self, ui: &mut Ui, p: &IdePalette, asks: &mut Vec<Ask>) {
        let branch = self.vcs.info.label.clone();
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let view = &mut self.git_workspace;
        let now = ui.input(|input| input.time);
        let delay = ui.ctx().options(|options| options.input_options.max_double_click_delay);
        if let Some((path, part, at)) = view.pending_diff.clone() {
            if now - at > delay {
                view.pending_diff = None;
                asks.push(Ask::Diff { id: None, path, part: Some(part) });
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(delay - (now - at) + 0.01));
            }
        }
        let changes = view.changes.clone();
        let changes: Vec<ChangeInfo> = changes.into_iter().filter(|change| change.status != "conflicted").collect();
        let unstaged: Vec<&ChangeInfo> = changes.iter().filter(|change| change.unstaged_status.is_some()).collect();
        let staged: Vec<&ChangeInfo> = changes.iter().filter(|change| change.staged_status.is_some()).collect();
        egui::ScrollArea::vertical().id_salt("gw-wip-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 14, bottom: 10 }).show(ui, |ui| {
                ui.label(egui::RichText::new(t!("vcs-workspace-wip-title")).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t!("vcs-workspace-wip-on", branch = branch.as_str()))
                        .font(theme::regular(12.5))
                        .color(p.text_secondary),
                );
            });
            if view.conflicts.operation.is_some() || !view.conflicts.files.is_empty() {
                let list = view.conflicts.clone();
                egui::Frame::new()
                    .inner_margin(egui::Margin { left: 12, right: 12, top: 0, bottom: 12 })
                    .show(ui, |ui| wip_operation_state(ui, p, &list, &root, asks));
            }
            for part in [ChangePart::Unstaged, ChangePart::Staged] {
                let files = if part == ChangePart::Unstaged { &unstaged } else { &staged };
                ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, p.frame));
                ui.add_space(4.0);
                wip_list_header(ui, p, part, files.len(), asks);
                if files.is_empty() {
                    let empty = if part == ChangePart::Unstaged {
                        t!("vcs-workspace-no-unstaged")
                    } else {
                        t!("vcs-workspace-no-staged")
                    };
                    egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 2, bottom: 6 }).show(
                        ui,
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(empty).font(theme::regular(12.5)).color(p.text_secondary),
                                )
                                .wrap(),
                            );
                        },
                    );
                }
                for change in files {
                    let status = match part {
                        ChangePart::Unstaged => change.unstaged_status.as_deref(),
                        ChangePart::Staged => change.staged_status.as_deref(),
                    };
                    let row = wip_file_row(ui, p, change, status.unwrap_or("modified"), part);
                    let paths = json!({ "paths": [change.path] });
                    match row {
                        WipRow::Button(WipButton::Stage) => {
                            asks.push(Ask::Index { command: "vcs.index.stage", params: paths });
                        }
                        WipRow::Button(WipButton::Unstage) => {
                            asks.push(Ask::Index { command: "vcs.index.unstage", params: paths });
                        }
                        WipRow::Button(WipButton::Discard) => {
                            asks.push(Ask::Index { command: "vcs.workspace.discard", params: paths });
                        }
                        WipRow::DoubleClicked => {
                            view.pending_diff = None;
                            let command =
                                if part == ChangePart::Unstaged { "vcs.index.stage" } else { "vcs.index.unstage" };
                            asks.push(Ask::Index { command, params: paths });
                        }
                        WipRow::Clicked => view.pending_diff = Some((change.path.clone(), part, now)),
                        WipRow::Menu(at) => {
                            view.file_right_click = Some(at);
                            asks.push(Ask::FileMenu(Some((change.path.clone(), None))));
                        }
                        WipRow::None => {}
                    }
                }
                ui.add_space(8.0);
            }
            ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, p.frame));
            egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 6 }).show(ui, |ui| {
                wip_commit_form(ui, p, view, staged.len(), asks);
            });
            ui.add_space(6.0);
            ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, p.frame));
            egui::Frame::new().inner_margin(egui::Margin { left: 16, right: 16, top: 12, bottom: 12 }).show(ui, |ui| {
                wip_stash_form(ui, p, view, !staged.is_empty(), asks);
            });
        });
    }
}

fn wip_operation_state(ui: &mut Ui, p: &IdePalette, list: &ConflictList, root: &std::path::Path, asks: &mut Vec<Ask>) {
    let conflicts = &list.files;
    let color = if conflicts.is_empty() { p.accent } else { p.vcs_conflict };
    egui::Frame::new()
        .fill(look::mix(p.editor_background, color, 0.1))
        .stroke(Stroke::new(1.0, look::mix(p.editor_background, color, 0.55)))
        .corner_radius(8)
        .inner_margin(egui::Margin { left: 12, right: 12, top: 10, bottom: 12 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            let caps = |ui: &mut Ui, text: String, color: Color32| {
                let galley = ui.painter().layout_job(look::small_caps(&text, 10.5, color));
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y), Sense::hover());
                ui.painter().galley(rect.min, galley, color);
            };
            let title_color = if conflicts.is_empty() { p.text_secondary } else { p.vcs_conflict };
            match &list.operation {
                Some(operation) => {
                    caps(ui, t!("vcs-operation-in-progress"), title_color);
                    ui.add_space(6.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&operation.title).font(theme::semibold(13.0)).color(p.text),
                        )
                        .wrap(),
                    );
                }
                None => caps(ui, t!("vcs-conflict-files-title"), title_color),
            }
            ui.add_space(10.0);
            if conflicts.is_empty() {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t!("vcs-operation-resolved")).font(theme::regular(12.5)).color(p.text),
                    )
                    .wrap(),
                );
            } else {
                caps(ui, t!("vcs-rebase-conflicts", count = conflicts.len()), p.text_secondary);
                ui.add_space(4.0);
                for file in conflicts {
                    let row = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 24.0));
                    let hovered = ui.rect_contains_pointer(row);
                    let room = if hovered { 2.0 * 54.0 + 8.0 } else { 0.0 };
                    let response = file_row_clipped(ui, p, &file.relative, "conflicted", room);
                    let mut took = false;
                    if hovered {
                        let mut right = row.right() - 4.0;
                        for (take, label) in
                            [("theirs", t!("vcs-conflict-theirs-short")), ("ours", t!("vcs-conflict-ours-short"))]
                        {
                            let galley = ui.painter().layout_no_wrap(label, theme::regular(11.5), p.text);
                            let rect = Rect::from_min_max(
                                pos2(right - galley.size().x - 14.0, row.center().y - 10.0),
                                pos2(right, row.center().y + 10.0),
                            );
                            let pill = ui.interact(
                                rect,
                                ui.scope_id().with(("gw-conflict-take", &file.relative, take)),
                                Sense::click(),
                            );
                            ui.painter().rect_filled(
                                rect,
                                5,
                                if pill.hovered() { p.hover } else { p.editor_background },
                            );
                            ui.painter().rect_stroke(
                                rect,
                                5,
                                Stroke::new(1.0, p.button_border),
                                egui::StrokeKind::Inside,
                            );
                            ui.painter().galley(rect.center() - galley.size() / 2.0, galley, p.text);
                            let help = if take == "ours" {
                                t!("vcs-conflict-accept-ours-help")
                            } else {
                                t!("vcs-conflict-accept-theirs-help")
                            };
                            hint(ui, &pill, &help, None, Side::Below);
                            if pill.clicked() {
                                took = true;
                                asks.push(Ask::Command(
                                    "vcs.conflict.resolve",
                                    json!({ "path": file.relative, "take": take }),
                                ));
                            }
                            right = rect.left() - 4.0;
                        }
                    }
                    let response =
                        response.on_hover_text(t!("vcs-conflict-resolve-file", file = file.relative.as_str()));
                    if response.clicked() && !took {
                        let path = file.relative.split('/').fold(root.to_path_buf(), |path, part| path.join(part));
                        asks.push(Ask::Command("vcs.conflict.open", json!({ "path": path })));
                    }
                }
                ui.add_space(6.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t!("vcs-conflict-files-help"))
                            .font(theme::regular(12.0))
                            .color(p.text_secondary),
                    )
                    .wrap(),
                );
            }
            if list.operation.is_none() {
                return;
            }
            ui.add_space(12.0);
            let half = (ui.available_width() - 8.0) / 2.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let can_continue = conflicts.is_empty();
                let label = egui::RichText::new(t!("vcs-rebase-continue"))
                    .font(theme::regular(13.0))
                    .color(if can_continue { p.accent_text } else { p.text_secondary });
                let fill = if can_continue { p.accent } else { p.hover };
                let response = ui
                    .add_enabled(
                        can_continue,
                        egui::Button::new(label).fill(fill).corner_radius(6).min_size(vec2(half, 30.0)),
                    )
                    .on_disabled_hover_text(t!("vcs-conflict-files-help"));
                if response.clicked() {
                    asks.push(Ask::Command("vcs.operation.continue", json!({})));
                }
                let label = egui::RichText::new(t!("vcs-rebase-abort")).font(theme::regular(13.0)).color(p.text);
                let response = ui
                    .add(
                        egui::Button::new(label)
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, p.button_border))
                            .corner_radius(6)
                            .min_size(vec2(half, 30.0)),
                    )
                    .on_hover_text(t!("vcs-operation-abort-help"));
                if response.clicked() {
                    asks.push(Ask::Command("vcs.operation.abort", json!({})));
                }
            });
        });
}

fn operation_banner(ui: &mut Ui, rect: Rect, p: &IdePalette, list: &ConflictList, asks: &mut Vec<Ask>) {
    let conflicted = !list.files.is_empty();
    let color = if conflicted { p.vcs_conflict } else { p.accent };
    ui.painter().rect_filled(rect, 0, look::mix(p.editor_background, color, 0.12));
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, look::mix(p.editor_background, color, 0.45)),
    );
    let y = rect.center().y;
    let mark = Rect::from_center_size(pos2(rect.left() + 20.0, y), Vec2::splat(18.0));
    ui.painter().circle_filled(mark.center(), 8.0, color);
    ui.painter().text(mark.center(), Align2::CENTER_CENTER, "!", theme::semibold(12.0), p.accent_text);
    let title =
        list.operation.as_ref().map_or_else(|| t!("vcs-conflict-files-title"), |operation| operation.title.clone());
    let state =
        if conflicted { t!("vcs-operation-conflicted", count = list.files.len()) } else { t!("vcs-operation-ready") };
    let mut right = rect.right() - 12.0;
    let mut button = |ui: &mut Ui, label: String, enabled: bool, primary: bool| {
        let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(12.5), p.text);
        let width = galley.size().x + 24.0;
        let area = Rect::from_min_max(pos2(right - width, y - 13.0), pos2(right, y + 13.0));
        right = area.left() - 8.0;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let text = match (enabled, primary) {
            (false, _) => p.text_secondary,
            (true, true) => p.accent_text,
            (true, false) => p.text,
        };
        let mut widget = egui::Button::new(egui::RichText::new(label).font(theme::regular(12.5)).color(text))
            .fill(if primary && enabled { p.accent } else { Color32::TRANSPARENT })
            .corner_radius(6)
            .min_size(area.size());
        if !(primary && enabled) {
            widget = widget.stroke(Stroke::new(1.0, p.button_border));
        }
        child.add_enabled(enabled, widget).clicked()
    };
    if list.operation.is_some() {
        if button(ui, t!("vcs-rebase-abort"), true, false) {
            asks.push(Ask::Command("vcs.operation.abort", json!({})));
        }
        if button(ui, t!("vcs-rebase-continue"), !conflicted, true) {
            asks.push(Ask::Command("vcs.operation.continue", json!({})));
        }
    }
    if conflicted && button(ui, t!("vcs-conflict-resolve"), true, list.operation.is_none()) {
        asks.push(Ask::Command("vcs.conflict.open", json!({})));
    }
    let painter = ui.painter().with_clip_rect(Rect::from_min_max(rect.min, pos2(right - 8.0, rect.bottom())));
    let shown = painter.text(pos2(mark.right() + 10.0, y), Align2::LEFT_CENTER, title, theme::semibold(13.0), p.text);
    painter.text(pos2(shown.right() + 12.0, y), Align2::LEFT_CENTER, state, theme::regular(12.5), p.text_secondary);
}

struct ToolButton {
    glyph: fn(&egui::Painter, Rect, Color32),
    label: &'static str,
    action: &'static str,
    enabled: bool,
    gap: f32,
}

fn buttons_width(captions: &[f32], compact: bool) -> f32 {
    captions.iter().map(|caption| tool_width(*caption, compact)).fold(tool_width(0.0, compact), f32::max)
}

fn pull_arrow(rect: Rect) -> Rect {
    Rect::from_min_max(pos2(rect.right() - 15.0, rect.bottom() - 22.0), pos2(rect.right() - 1.0, rect.bottom() - 2.0))
}

fn toolbar_buttons(changes: bool, stashes: bool, history: &HistoryList) -> [ToolButton; 7] {
    let button = |glyph, label, action, enabled, gap| ToolButton { glyph, label, action, enabled, gap };
    [
        button(
            glyph_undo as fn(&egui::Painter, Rect, Color32),
            "vcs-workspace-undo",
            "vcs.history.undo",
            history.undo.is_some(),
            0.0,
        ),
        button(glyph_redo, "vcs-workspace-redo", "vcs.history.redo", history.redo.is_some(), 0.0),
        button(glyph_pull, "vcs-workspace-pull", "vcs.workspace.pull", true, 6.0),
        button(glyph_push, "vcs-workspace-push", "view.vcs.push", true, 0.0),
        button(glyph_branch, "vcs-workspace-branch", "view.vcs.newBranch", true, 0.0),
        button(glyph_stash, "vcs-workspace-stash", "vcs.stash.create", changes, 0.0),
        button(glyph_pop, "vcs-workspace-pop", "vcs.stash.pop", stashes, 0.0),
    ]
}

fn tool_width(caption: f32, compact: bool) -> f32 {
    if compact { 46.0 } else { (caption + 18.0).max(54.0) }
}

fn history_hint(action: &str, history: &HistoryList) -> (String, Option<String>) {
    let undo = action == "vcs.history.undo";
    let next = if undo { &history.undo } else { &history.redo };
    match next {
        Some(entry) => {
            let key = if undo { "vcs-workspace-undo-named" } else { "vcs-workspace-redo-named" };
            let shortcut = crate::keymap::shortcut(if undo { "editor.edit.undo" } else { "editor.edit.redo" });
            (t!(key, operation = entry.title.as_str()), shortcut)
        }
        None => {
            let key = if undo { "vcs-workspace-undo" } else { "vcs-workspace-redo" };
            let none = if undo { "vcs-workspace-nothing-undo" } else { "vcs-workspace-nothing-redo" };
            (t!(key), Some(t!(none)))
        }
    }
}

fn tool_button(
    ui: &mut Ui,
    p: &IdePalette,
    rect: Rect,
    button: &ToolButton,
    compact: bool,
    history: &HistoryList,
    pull: PullMode,
) -> egui::Response {
    let sense = if button.enabled { Sense::click() } else { Sense::hover() };
    let response = ui.interact(rect, ui.scope_id().with(("gw-tool", button.action)), sense);
    if button.enabled && response.hovered() {
        ui.painter().rect_filled(rect, 6, p.hover);
    }
    let dim = p.text_secondary.gamma_multiply(0.55);
    let (text, icon) = if button.enabled { (p.text_secondary, p.text) } else { (dim, dim) };
    let center = rect.center().x;
    let icon_y = if compact { rect.center().y } else { rect.top() + 33.0 };
    if !compact {
        ui.painter().with_clip_rect(rect).text(
            pos2(center, rect.top() + 11.0),
            Align2::CENTER_CENTER,
            t!(button.label),
            theme::regular(11.5),
            text,
        );
    }
    (button.glyph)(ui.painter(), Rect::from_center_size(pos2(center, icon_y), Vec2::splat(17.0)), icon);
    let (name, second) = if button.action.starts_with("vcs.history.") {
        history_hint(button.action, history)
    } else {
        let (name, shortcut) = button_hint(button, pull);
        (name, if button.enabled { shortcut } else { None })
    };
    hint(ui, &response, &name, second.as_deref(), Side::Below);
    response
}

fn button_hint(button: &ToolButton, pull: PullMode) -> (String, Option<String>) {
    match button.action {
        "vcs.workspace.pull" => (t!(pull.message_id()), crate::keymap::shortcut("view.vcs.update")),
        action => (t!(button.label), crate::keymap::shortcut(action)),
    }
}

fn pull_menu(ui: &Ui, p: &IdePalette, at: egui::Pos2, default: PullMode, asks: &mut Vec<Ask>) {
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("gw-pull-menu-popup"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let widest = PullMode::ALL
        .iter()
        .map(|mode| ui.painter().layout_no_wrap(t!(mode.message_id()), theme::regular(13.0), p.text).size().x)
        .fold(0.0, f32::max);
    let width = (widest + 78.0).max(240.0);
    popup.show(|ui| {
        ui.set_width(width);
        for mode in PullMode::ALL {
            let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
            let mark = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(22.0));
            let chooser = ui.interact(mark, ui.scope_id().with(("gw-pull-default", mode)), Sense::click());
            if response.hovered() && !chooser.hovered() {
                ui.painter().rect_filled(rect, 5, p.selection);
            }
            let glyph = if mode == PullMode::FetchAll { glyph_fetch } else { glyph_pull };
            glyph(
                ui.painter(),
                Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(13.0)),
                p.text_secondary,
            );
            ui.painter().text(
                pos2(rect.left() + 30.0, rect.center().y),
                Align2::LEFT_CENTER,
                t!(mode.message_id()),
                theme::regular(13.0),
                p.text,
            );
            if chooser.hovered() {
                ui.painter().rect_filled(mark, 5, p.hover);
            }
            let center = mark.center();
            if mode == default {
                ui.painter().circle_stroke(center, 5.5, Stroke::new(1.5, p.accent));
                ui.painter().circle_filled(center, 3.0, p.accent);
            } else {
                ui.painter().circle_stroke(center, 5.5, Stroke::new(1.0, p.text_secondary));
            }
            let tip = if mode == default { "vcs-workspace-pull-is-default" } else { "vcs-workspace-pull-set-default" };
            hint(ui, &chooser, &t!(tip), None, Side::Right);
            if chooser.clicked() {
                asks.push(Ask::Command("vcs.workspace.pullDefault", json!({ "mode": mode })));
            } else if response.clicked() {
                asks.push(Ask::Command("vcs.workspace.pull", json!({ "mode": mode })));
                ui.close();
            }
        }
    });
    if !open {
        asks.push(Ask::Command("vcs.workspace.pullMenu", json!({ "open": false })));
    }
}

const SUMMARY: usize = 72;

fn commit_message(summary: &str, description: &str) -> String {
    let (summary, description) = (summary.trim(), description.trim());
    if description.is_empty() { summary.to_owned() } else { format!("{summary}\n\n{description}") }
}

fn message_fields(message: &str) -> (String, String) {
    let (summary, rest) = message.split_once('\n').unwrap_or((message, ""));
    (summary.trim().to_owned(), rest.trim().to_owned())
}

fn commit_label(staged: usize, amend: bool) -> String {
    if amend {
        t!("vcs-workspace-amend-commit")
    } else if staged == 0 {
        t!("vcs-workspace-commit-nothing")
    } else {
        t!("vcs-workspace-commit-files", count = staged)
    }
}

fn framed_field(
    ui: &mut Ui,
    p: &IdePalette,
    id: egui::Id,
    add: impl FnOnce(&mut Ui) -> egui::Response,
) -> egui::Response {
    let focused = ui.memory(|memory| memory.has_focus(id));
    egui::Frame::new()
        .fill(p.text_field)
        .stroke(Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { p.accent } else { p.text_field_border }))
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, add)
        .inner
}

fn wip_commit_form(ui: &mut Ui, p: &IdePalette, view: &mut GitWorkspaceView, staged: usize, asks: &mut Vec<Ask>) {
    ui.label(egui::RichText::new(t!("vcs-workspace-commit-title")).font(theme::semibold(12.0)).color(p.text));
    ui.add_space(6.0);
    let count = view.summary.trim().chars().count();
    let counter = t!("vcs-workspace-summary-count", count = count, limit = SUMMARY);
    let over = count > SUMMARY;
    let counter_color = if over { p.warning_text } else { p.text_secondary };
    let summary_id = ui.scope_id().with("gw-commit-summary");
    let mut commit_key = false;
    let summary = framed_field(ui, p, summary_id, |ui| {
        ui.horizontal(|ui| {
            let galley = ui.painter().layout_no_wrap(counter.clone(), theme::regular(11.5), counter_color);
            let room = (ui.available_width() - galley.size().x - 8.0).max(40.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut view.summary)
                    .id(summary_id)
                    .frame(egui::Frame::NONE)
                    .hint_text(t!("vcs-workspace-summary"))
                    .desired_width(room),
            );
            let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x, field.rect.height()), Sense::hover());
            ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, counter_color);
            field
        })
        .inner
    });
    if over {
        summary.clone().on_hover_text(t!("vcs-workspace-summary-long", limit = SUMMARY));
    }
    if summary.has_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
        commit_key = true;
    }
    ui.add_space(8.0);
    let description_id = ui.scope_id().with("gw-commit-description");
    let description = framed_field(ui, p, description_id, |ui| {
        ui.add(
            egui::TextEdit::multiline(&mut view.description)
                .id(description_id)
                .frame(egui::Frame::NONE)
                .hint_text(t!("vcs-workspace-description"))
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        )
    });
    if description.has_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter)) {
        commit_key = true;
    }
    ui.add_space(8.0);
    let mut amend = view.amend;
    if ui.checkbox(&mut amend, egui::RichText::new(t!("vcs-workspace-amend")).font(theme::regular(13.0))).changed() {
        asks.push(Ask::Amend(amend));
    }
    ui.add_space(10.0);
    let can_commit = !view.summary.trim().is_empty() && (staged > 0 || view.amend);
    let width = ui.available_width();
    let commit = ui.add_enabled(
        can_commit,
        egui::Button::new(
            egui::RichText::new(commit_label(staged, view.amend)).font(theme::regular(13.0)).color(if can_commit {
                p.accent_text
            } else {
                p.text_secondary
            }),
        )
        .fill(if can_commit { p.accent } else { p.hover })
        .corner_radius(6)
        .min_size(vec2(width, 30.0)),
    );
    if commit.clicked() || (commit_key && can_commit) {
        asks.push(Ask::CommitStaged { push: false });
    }
    ui.add_space(8.0);
    let push = ui.add_enabled(
        can_commit,
        egui::Button::new(
            egui::RichText::new(t!("vcs-workspace-commit-push")).font(theme::regular(13.0)).color(if can_commit {
                p.text
            } else {
                p.text_secondary
            }),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::new(1.0, p.button_border))
        .corner_radius(6)
        .min_size(vec2(width, 30.0)),
    );
    if push.clicked() {
        asks.push(Ask::CommitStaged { push: true });
    }
}

fn wip_stash_form(ui: &mut Ui, p: &IdePalette, view: &mut GitWorkspaceView, staged: bool, asks: &mut Vec<Ask>) {
    ui.label(egui::RichText::new(t!("vcs-workspace-stash-title")).font(theme::semibold(12.0)).color(p.text));
    ui.add_space(6.0);
    let id = ui.scope_id().with("gw-stash-message");
    let mut entered = false;
    framed_field(ui, p, id, |ui| {
        let field = ui.add(
            egui::TextEdit::singleline(&mut view.stash_message)
                .id(id)
                .frame(egui::Frame::NONE)
                .hint_text(t!("vcs-workspace-stash-message"))
                .desired_width(f32::INFINITY),
        );
        entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        field
    });
    ui.add_space(8.0);
    let message = Some(view.stash_message.trim().to_owned());
    let labels = [t!("vcs-workspace-stash-all"), t!("vcs-workspace-stash-staged")];
    let half = (ui.available_width() - 8.0) / 2.0;
    let fits = labels
        .iter()
        .all(|label| ui.painter().layout_no_wrap(label.clone(), theme::regular(13.0), p.text).size().x + 20.0 <= half);
    let width = if fits { half } else { ui.available_width() };
    let button = |ui: &mut Ui, label: String, enabled: bool| {
        let color = if enabled { p.text } else { p.text_secondary };
        ui.add_enabled(
            enabled,
            egui::Button::new(egui::RichText::new(label).font(theme::regular(13.0)).color(color))
                .min_size(vec2(width, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border)),
        )
    };
    let [all, staged_only] = labels;
    let mut buttons = |ui: &mut Ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if button(ui, all.clone(), true).clicked() || entered {
            asks.push(Ask::StashChanges { staged: false, message: message.clone() });
        }
        if button(ui, staged_only.clone(), staged).clicked() {
            asks.push(Ask::StashChanges { staged: true, message: message.clone() });
        }
    };
    if fits {
        ui.horizontal(|ui| buttons(ui));
    } else {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Center), |ui| buttons(ui));
    }
}

fn wip_list_header(ui: &mut Ui, p: &IdePalette, part: ChangePart, count: usize, asks: &mut Vec<Ask>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let title = match part {
        ChangePart::Unstaged => t!("vcs-workspace-unstaged", count = count),
        ChangePart::Staged => t!("vcs-workspace-staged", count = count),
    };
    let title = ui.painter().layout_no_wrap(title, theme::semibold(12.0), p.text_secondary);
    let (all, all_command) = match part {
        ChangePart::Unstaged => (t!("vcs-workspace-stage-all"), "vcs.index.stage"),
        ChangePart::Staged => (t!("vcs-workspace-unstage-all"), "vcs.index.unstage"),
    };
    let words = ui.painter().layout_no_wrap(all.clone(), theme::regular(12.0), p.link);
    let discard = part == ChangePart::Unstaged;
    let buttons = words.size().x + 16.0 + if discard { 28.0 } else { 0.0 };
    let as_words = header_fits(title.size().x, buttons, rect.width() - 32.0);
    let painter = ui.painter().with_clip_rect(rect);
    painter.galley(pos2(rect.left() + 16.0, rect.center().y - title.size().y / 2.0), title, p.text_secondary);
    if count == 0 {
        return;
    }
    let mut right = rect.right() - 12.0;
    if discard {
        let button = Rect::from_center_size(pos2(right - 12.0, rect.center().y), Vec2::splat(24.0));
        let response = ui.interact(button, ui.scope_id().with("gw-discard-all"), Sense::click());
        hover(ui, &response, p);
        hint(ui, &response, &t!("vcs-workspace-discard-all"), None, Side::Below);
        glyph_trash(ui.painter(), button.shrink(6.0), p.text_secondary);
        if response.clicked() {
            asks.push(Ask::Index { command: "vcs.workspace.discard", params: json!({ "all": true }) });
        }
        right = button.left() - 4.0;
    }
    let response = if as_words {
        let button =
            Rect::from_min_max(pos2(right - words.size().x - 12.0, rect.top() + 3.0), pos2(right, rect.bottom() - 3.0));
        let response = ui.interact(button, ui.scope_id().with(("gw-all", all_command)), Sense::click());
        hover(ui, &response, p);
        ui.painter().galley(pos2(button.left() + 6.0, button.center().y - words.size().y / 2.0), words, p.link);
        response
    } else {
        let button = Rect::from_center_size(pos2(right - 12.0, rect.center().y), Vec2::splat(24.0));
        let response = ui.interact(button, ui.scope_id().with(("gw-all", all_command)), Sense::click());
        hover(ui, &response, p);
        hint(ui, &response, &all, None, Side::Below);
        stage_glyph(ui.painter(), button.shrink(6.0), part, p.text_secondary);
        response
    };
    if response.clicked() {
        asks.push(Ask::Index { command: all_command, params: json!({ "all": true }) });
    }
}

fn discard_text(params: &IndexParams, root: &std::path::Path, changes: &[ChangeInfo]) -> String {
    let name = |path: &std::path::Path| path.strip_prefix(root).unwrap_or(path).display().to_string();
    let partial = !params.hunks.is_empty() || !params.lines.is_empty() || !params.removed_lines.is_empty();
    let unversioned = |path: &std::path::Path| {
        changes.iter().any(|change| {
            (change.path == path || change.relative == name(path))
                && change.unstaged_status.as_deref() == Some("unversioned")
        })
    };
    match params.paths.as_slice() {
        _ if params.all => t!("vcs-discard-all-text"),
        [path] if partial => t!("vcs-discard-lines-text", file = name(path)),
        [path] if unversioned(path) => t!("vcs-discard-new-file-text", file = name(path)),
        [path] => t!("vcs-discard-file-text", file = name(path)),
        paths => t!("vcs-discard-files-text", count = paths.len()),
    }
}

fn header_fits(title: f32, buttons: f32, room: f32) -> bool {
    title + 12.0 + buttons <= room
}

fn stage_glyph(painter: &egui::Painter, rect: Rect, part: ChangePart, color: Color32) {
    match part {
        ChangePart::Unstaged => glyph_plus(painter, rect, color),
        ChangePart::Staged => glyph_minus(painter, rect, color),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WipButton {
    Stage,
    Unstage,
    Discard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WipRow {
    None,
    Clicked,
    DoubleClicked,
    Button(WipButton),
    Menu(egui::Pos2),
}

fn wip_file_row(ui: &mut Ui, p: &IdePalette, change: &ChangeInfo, status: &str, part: ChangePart) -> WipRow {
    let id = ui.scope_id().with(("gw-wip-row", part == ChangePart::Staged, &change.path));
    let row = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), 24.0));
    let hovered = ui.rect_contains_pointer(row);
    let buttons: &[WipButton] = match part {
        ChangePart::Unstaged => &[WipButton::Stage, WipButton::Discard],
        ChangePart::Staged => &[WipButton::Unstage],
    };
    let room = if hovered { buttons.len() as f32 * 26.0 + 8.0 } else { 0.0 };
    let response = file_row_clipped(ui, p, &change.relative, status, room).on_hover_text(&change.relative);
    let mut clicked = WipRow::None;
    if hovered {
        let mut right = row.right() - 10.0;
        for (index, button) in buttons.iter().enumerate() {
            let rect = Rect::from_center_size(pos2(right - 12.0, row.center().y), Vec2::splat(22.0));
            let response = ui.interact(rect, id.with(index), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            let (name, glyph): (String, fn(&egui::Painter, Rect, Color32)) = match button {
                WipButton::Stage => (t!("vcs-workspace-stage"), glyph_plus),
                WipButton::Unstage => (t!("vcs-workspace-unstage"), glyph_minus),
                WipButton::Discard => (t!("vcs-workspace-discard"), glyph_trash),
            };
            hint(ui, &response, &name, None, Side::Below);
            glyph(ui.painter(), rect.shrink(5.0), p.text_secondary);
            if response.clicked() {
                clicked = WipRow::Button(*button);
            }
            right = rect.left() - 4.0;
        }
    }
    match clicked {
        WipRow::None if response.double_clicked() => WipRow::DoubleClicked,
        WipRow::None if response.clicked() => WipRow::Clicked,
        WipRow::None if response.secondary_clicked() => {
            WipRow::Menu(response.interact_pointer_pos().unwrap_or(response.rect.center()))
        }
        other => other,
    }
}

fn commit_menu(ui: &Ui, view: &mut GitWorkspaceView, info: &zen_workbench::vcs::RepositoryInfo, asks: &mut Vec<Ask>) {
    let (Some(id), Some(at)) = (view.menu_for.clone(), view.menu_at) else { return };
    let offers = view.menu_info.clone().unwrap_or_default();
    let mut focus_first = view.menu_keyboard;
    let mut focused = false;
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("gw-commit-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    popup.show(|ui| {
        ui.set_min_width(250.0);
        let mut item = |ui: &mut Ui, title: String, enabled: bool, action: CommitAction| {
            let response = ui.add_enabled(enabled, egui::Button::new(title));
            if focus_first && !ui.is_sizing_pass() {
                focus_first = false;
                focused = true;
                response.request_focus();
            }
            if response.clicked() {
                asks.push(Ask::Commit(id.clone(), action));
                ui.close();
            }
        };
        item(ui, t!("vcs-workspace-menu-create-branch"), true, CommitAction::CreateBranch);
        item(ui, t!("vcs-workspace-menu-create-tag"), true, CommitAction::CreateTag);
        item(ui, t!("vcs-workspace-menu-checkout"), !offers.detached_here, CommitAction::Checkout);
        ui.separator();
        let picked = view.picked.len();
        if picked > 1 && view.picked.contains(&id) {
            let title = t!("vcs-workspace-menu-cherry-pick-picked", count = picked);
            item(ui, title, true, CommitAction::CherryPickPicked);
        } else {
            item(ui, t!("vcs-workspace-menu-cherry-pick"), !offers.on_current, CommitAction::CherryPick);
        }
        if view.picked.len() + usize::from(view.picked_wip) == 2 && view.picked.contains(&id) {
            item(ui, t!("vcs-workspace-menu-compare"), true, CommitAction::Compare);
        }
        item(ui, t!("vcs-workspace-menu-revert"), offers.on_current, CommitAction::Revert);
        let from_here = offers.on_current && info.commit.as_deref() != Some(id.as_str());
        item(ui, t!("vcs-workspace-menu-interactive-rebase"), from_here, CommitAction::InteractiveRebase);
        let branch = info.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
        ui.menu_button(t!("vcs-workspace-menu-reset", branch = branch.as_str()), |ui| {
            ui.set_min_width(260.0);
            item(ui, t!("vcs-workspace-menu-reset-soft"), true, CommitAction::ResetSoft);
            item(ui, t!("vcs-workspace-menu-reset-mixed"), true, CommitAction::ResetMixed);
            item(ui, t!("vcs-workspace-menu-reset-hard"), true, CommitAction::ResetHard);
        });
        ui.separator();
        item(ui, t!("vcs-workspace-menu-copy-sha"), true, CommitAction::CopySha);
        item(ui, t!("vcs-workspace-menu-copy-message"), true, CommitAction::CopyMessage);
        ui.separator();
        item(ui, t!("keymap-view-vcs-createPatch"), true, CommitAction::CreatePatch);
    });
    if focused {
        view.menu_keyboard = false;
    }
    if !open {
        asks.push(Ask::Menu(None));
    }
}

fn commit_labels(commit: &GraphCommit) -> Vec<RefTarget> {
    commit
        .refs
        .iter()
        .filter(|label| label.kind != RefKind::Head)
        .map(|label| RefTarget { name: label.name.clone(), kind: label.kind })
        .collect()
}

fn ref_row_menu(
    response: &egui::Response,
    target: RefTarget,
    menu_for: &Option<Vec<RefTarget>>,
    menu_at: &mut Option<egui::Pos2>,
    right_click: &mut Option<egui::Pos2>,
    asks: &mut Vec<Ask>,
) {
    if menu_at.is_none() && menu_for.as_deref().is_some_and(|refs| refs.len() == 1 && refs[0] == target) {
        *menu_at = Some(pos2(response.rect.left() + 32.0, response.rect.bottom()));
    }
    if response.secondary_clicked() {
        *right_click = Some(response.interact_pointer_pos().unwrap_or(response.rect.center()));
        asks.push(Ask::RefMenu(Some(RefMenuOpen::Ref(target))));
    }
}

fn default_upstream(branches: &BranchList, branch: &str) -> Option<String> {
    let local = branches.local.iter().find(|local| local.name == branch);
    if let Some(upstream) = local.and_then(|local| local.upstream.clone()) {
        return Some(upstream);
    }
    let same = |remote: &&zen_workbench::vcs::BranchInfo| {
        remote.name.split_once('/').is_some_and(|(_, short)| short == branch)
    };
    let origin = format!("origin/{branch}");
    branches
        .remote
        .iter()
        .find(|remote| remote.name == origin)
        .or_else(|| branches.remote.iter().find(same))
        .map(|remote| remote.name.clone())
}

fn ref_menu(ui: &Ui, view: &mut GitWorkspaceView, info: &zen_workbench::vcs::RepositoryInfo, asks: &mut Vec<Ask>) {
    let (Some(refs), Some(at)) = (view.ref_menu_for.clone(), view.ref_menu_at) else { return };
    let branches = view.branches.clone();
    let focus = view.focus.clone();
    let mut focus_asks = Vec::new();
    let focus_first = std::cell::Cell::new(view.ref_menu_keyboard);
    let focused = std::cell::Cell::new(false);
    let take_focus = |ui: &Ui, response: &egui::Response| {
        if focus_first.get() && !ui.is_sizing_pass() {
            focus_first.set(false);
            focused.set(true);
            response.request_focus();
        }
    };
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("gw-ref-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    popup.show(|ui| {
        ui.set_min_width(250.0);
        let mut item = |ui: &mut Ui, title: String, enabled: bool, target: &RefTarget, action: RefAction| {
            let response = ui.add_enabled(enabled, egui::Button::new(title));
            if enabled {
                take_focus(ui, &response);
            }
            if response.clicked() {
                asks.push(Ask::Ref(target.clone(), action));
                ui.close();
            }
        };
        if let [target] = refs.as_slice() {
            ref_items(ui, target, &branches, info, &mut |ui, title, enabled, action| {
                item(ui, title, enabled, target, action);
            });
            search::focus_items(ui, target, &focus, &mut focus_asks);
        } else {
            for target in &refs {
                let title = match target.kind {
                    RefKind::Remote => t!("vcs-workspace-ref-remote", name = target.name.as_str()),
                    RefKind::Tag => t!("vcs-workspace-ref-tag", name = target.name.as_str()),
                    RefKind::Local | RefKind::Head => t!("vcs-workspace-ref-local", name = target.name.as_str()),
                };
                let submenu = ui.menu_button(title, |ui| {
                    ui.set_min_width(250.0);
                    ref_items(ui, target, &branches, info, &mut |ui, title, enabled, action| {
                        item(ui, title, enabled, target, action);
                    });
                    search::focus_items(ui, target, &focus, &mut focus_asks);
                });
                take_focus(ui, &submenu.response);
            }
        }
    });
    asks.extend(focus_asks);
    if focused.get() {
        view.ref_menu_keyboard = false;
    }
    if !open {
        asks.push(Ask::RefMenu(None));
    }
}

fn stash_menu(ui: &Ui, view: &mut GitWorkspaceView, asks: &mut Vec<Ask>) {
    let (Some(commit), Some(at)) = (view.stash_menu_for.clone(), view.stash_menu_at) else { return };
    let mut focus_first = view.stash_menu_keyboard;
    let mut focused = false;
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("gw-stash-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    popup.show(|ui| {
        ui.set_min_width(220.0);
        let mut item = |ui: &mut Ui, title: String, action: StashAction| {
            let response = ui.button(title);
            if focus_first && !ui.is_sizing_pass() {
                focus_first = false;
                focused = true;
                response.request_focus();
            }
            if response.clicked() {
                asks.push(Ask::Stash(Some(commit.clone()), action));
                ui.close();
            }
        };
        item(ui, t!("vcs-workspace-stash-apply"), StashAction::Apply);
        item(ui, t!("vcs-workspace-stash-pop"), StashAction::Pop);
        ui.separator();
        item(ui, t!("vcs-workspace-stash-rename"), StashAction::Rename);
        item(ui, t!("vcs-workspace-stash-drop"), StashAction::Drop);
    });
    if focused {
        view.stash_menu_keyboard = false;
    }
    if !open {
        asks.push(Ask::StashMenu(None));
    }
}

fn drop_label(labels: &[RefTarget]) -> Option<&RefTarget> {
    [RefKind::Local, RefKind::Remote, RefKind::Tag]
        .into_iter()
        .find_map(|kind| labels.iter().find(|label| label.kind == kind))
}

fn label_drag(ui: &Ui, drag: &mut RefDrag, response: &egui::Response, refs: (f32, f32), labels: &[RefTarget]) -> bool {
    let on_labels = ui.input(|input| input.pointer.press_origin()).is_some_and(|at| at.x < refs.1);
    let source = labels.first().filter(|_| on_labels);
    drag.track(ui, response, source, drop_label(labels))
}

fn drop_highlight(painter: &egui::Painter, p: &IdePalette, rect: Rect) {
    painter.rect_filled(rect, 5, p.accent.gamma_multiply(0.16));
    painter.rect_stroke(rect, 5, Stroke::new(1.5, p.accent), egui::StrokeKind::Inside);
}

fn place_drop_menu(menu: &Option<DropMenuInfo>, at: &mut Option<egui::Pos2>, target: &RefTarget, row: &Rect) {
    if at.is_none() && menu.as_ref().is_some_and(|menu| menu.target == *target) {
        *at = Some(pos2(row.left() + 32.0, row.bottom()));
    }
}

fn finish_drag(ui: &mut Ui, p: &IdePalette, drag: &mut RefDrag, asks: &mut Vec<Ask>) {
    let Some(source) = drag.source.clone() else { return };
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        *drag = RefDrag::default();
        return;
    }
    let (released, down, at) =
        ui.input(|input| (input.pointer.any_released(), input.pointer.any_down(), input.pointer.latest_pos()));
    if released || !down {
        if let Some(target) = drag.over.take() {
            drag.dropped_at = at;
            asks.push(Ask::DropMenu(Some((source, target))));
        }
        drag.source = None;
        return;
    }
    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    let Some(at) = at else { return };
    let layer = egui::LayerId::new(egui::Order::Tooltip, ui.scope_id().with("gw-drag"));
    let painter = ui.ctx().layer_painter(layer);
    let font = theme::regular(12.0);
    let galley = painter.layout_no_wrap(source.name.clone(), font, p.text);
    let pill = Rect::from_min_size(at + vec2(14.0, 8.0), vec2(galley.size().x + 32.0, 22.0));
    painter.rect_filled(pill.translate(vec2(0.0, 2.0)), 6, p.popup_shadow);
    painter.rect_filled(pill, 6, p.popup);
    let border = if drag.over.is_some() { p.accent } else { p.popup_border };
    painter.rect_stroke(pill, 6, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    let icon = Rect::from_center_size(pos2(pill.left() + 12.0, pill.center().y), Vec2::splat(11.0));
    match source.kind {
        RefKind::Tag => glyph_label(&painter, icon, p.text, p.popup),
        RefKind::Remote => glyph_database(&painter, icon, p.text),
        RefKind::Local | RefKind::Head => glyph_branch(&painter, icon, p.text),
    }
    painter.galley(pos2(pill.left() + 23.0, pill.center().y - galley.size().y / 2.0), galley, p.text);
}

fn drop_menu(ui: &Ui, view: &mut GitWorkspaceView, asks: &mut Vec<Ask>) {
    let (Some(menu), Some(at)) = (view.drop_menu_for.clone(), view.drop_menu_at) else { return };
    let mut focus_first = view.drop_menu_keyboard;
    let mut focused = false;
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("gw-drop-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let (source, target) = (menu.source.name.as_str(), menu.target.name.as_str());
    popup.show(|ui| {
        ui.set_min_width(250.0);
        for item in &menu.items {
            let asks_first = item.checkout.is_some();
            let (title, why) = match item.action {
                DropAction::FastForward => {
                    (t!("vcs-drop-fast-forward", source = source, target = target), String::new())
                }
                DropAction::Merge => (
                    t!(
                        if asks_first { "vcs-drop-merge-checkout" } else { "vcs-drop-merge" },
                        source = source,
                        target = target
                    ),
                    t!("vcs-drop-merge-nothing", source = source, target = target),
                ),
                DropAction::Rebase => (
                    t!(
                        if asks_first { "vcs-drop-rebase-checkout" } else { "vcs-drop-rebase" },
                        source = source,
                        target = target
                    ),
                    t!("vcs-drop-rebase-nothing", source = source, target = target),
                ),
                DropAction::InteractiveRebase => (
                    t!("vcs-drop-interactive-rebase", source = source, target = target),
                    t!("vcs-drop-rebase-nothing", source = source, target = target),
                ),
            };
            if item.action == DropAction::InteractiveRebase {
                ui.separator();
            }
            let response = ui.add_enabled(item.enabled, egui::Button::new(title)).on_disabled_hover_text(why);
            if item.enabled && focus_first && !ui.is_sizing_pass() {
                focus_first = false;
                focused = true;
                response.request_focus();
            }
            if response.clicked() {
                asks.push(Ask::Drop(menu.source.clone(), menu.target.clone(), item.action));
                ui.close();
            }
        }
    });
    if focused {
        view.drop_menu_keyboard = false;
    }
    if !open {
        asks.push(Ask::DropMenu(None));
    }
}

fn ref_items(
    ui: &mut Ui,
    target: &RefTarget,
    branches: &BranchList,
    info: &zen_workbench::vcs::RepositoryInfo,
    item: &mut dyn FnMut(&mut Ui, String, bool, RefAction),
) {
    let name = target.name.as_str();
    let current = info.branch.as_deref();
    let head = current.unwrap_or("HEAD");
    let is_current = target.kind == RefKind::Local && current == Some(name);
    let merge = t!("vcs-workspace-ref-merge", name = name, current = head);
    let rebase = t!("vcs-workspace-ref-rebase", name = name, current = head);
    let interactive = t!("vcs-workspace-ref-interactive-rebase", name = name, current = head);
    let fast_forward =
        |branch: Option<&zen_workbench::vcs::BranchInfo>, upstream: Option<&str>| match (branch, upstream) {
            (Some(branch), Some(upstream)) => (
                t!("vcs-workspace-ref-fast-forward", name = branch.name.as_str(), target = upstream),
                branch.behind > 0 && branch.ahead == 0,
            ),
            _ => (t!("vcs-workspace-ref-fast-forward-upstream"), false),
        };
    match target.kind {
        RefKind::Local => {
            let branch = branches.local.iter().find(|branch| branch.name == name);
            item(ui, t!("vcs-workspace-ref-checkout", name = name), !is_current, RefAction::Checkout);
            ui.separator();
            item(ui, merge, !is_current, RefAction::Merge);
            item(ui, rebase, !is_current && current.is_some(), RefAction::Rebase);
            item(ui, interactive, !is_current && current.is_some(), RefAction::InteractiveRebase);
            let (title, enabled) = fast_forward(branch, branch.and_then(|branch| branch.upstream.as_deref()));
            item(ui, title, enabled, RefAction::FastForward);
            ui.separator();
            item(ui, t!("vcs-workspace-ref-push", name = name), true, RefAction::Push);
            item(ui, t!("vcs-workspace-ref-set-upstream"), true, RefAction::SetUpstream);
            ui.separator();
            item(ui, t!("vcs-workspace-ref-create-worktree"), true, RefAction::CreateWorktree);
            ui.separator();
            item(ui, t!("vcs-workspace-ref-rename", name = name), true, RefAction::Rename);
            item(ui, t!("vcs-workspace-ref-delete", name = name), !is_current, RefAction::Delete);
        }
        RefKind::Remote => {
            let tracking = branches.tracking(name);
            item(ui, t!("vcs-workspace-ref-checkout", name = name), true, RefAction::Checkout);
            ui.separator();
            item(ui, merge, true, RefAction::Merge);
            item(ui, rebase, current.is_some(), RefAction::Rebase);
            item(ui, interactive, current.is_some(), RefAction::InteractiveRebase);
            let (title, enabled) = fast_forward(tracking, Some(name));
            item(ui, title, enabled, RefAction::FastForward);
            ui.separator();
            item(ui, t!("vcs-workspace-ref-create-worktree"), true, RefAction::CreateWorktree);
            ui.separator();
            item(ui, t!("vcs-workspace-ref-delete-remote", name = name), true, RefAction::Delete);
        }
        RefKind::Tag => {
            item(ui, t!("vcs-workspace-ref-checkout-tag", name = name), true, RefAction::Checkout);
            ui.separator();
            item(ui, merge, true, RefAction::Merge);
            item(ui, rebase, current.is_some(), RefAction::Rebase);
            item(ui, interactive, current.is_some(), RefAction::InteractiveRebase);
            ui.separator();
            item(ui, t!("vcs-workspace-ref-push-tag", name = name), true, RefAction::Push);
            item(ui, t!("vcs-workspace-ref-delete-tag", name = name), true, RefAction::DeleteTag);
        }
        RefKind::Head => {}
    }
}

impl IdeView {
    fn commit_staged(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, push: bool) {
        let view = &self.git_workspace;
        if view.committing {
            return;
        }
        let params = json!({
            "message": commit_message(&view.summary, &view.description),
            "staged": true,
            "amend": view.amend,
            "push": push,
        });
        let create = |session: &mut Session| registry.dispatch(session, "vcs.commit.create", params);
        match zen_workbench::vcs::off_the_frame(session, create) {
            Ok(_) => self.git_workspace.committing = true,
            Err(error) => self.report(ui, error),
        }
    }

    pub(in crate::ide) fn staged_commit_over(&mut self, succeeded: bool) {
        let view = &mut self.git_workspace;
        view.committing = false;
        view.changes_read_for = None;
        if succeeded {
            view.summary.clear();
            view.description.clear();
            view.amend = false;
        }
    }

    pub(super) fn workspace_dialogs(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let remotes: Vec<String> =
            self.git_workspace.branches.remote.iter().map(|branch| branch.name.clone()).collect();
        let Some(dialog) = &mut self.git_workspace.dialog else { return };
        let p = self.palette;
        if let Dialog::CreateTag { commit, .. } | Dialog::Checkout { commit, .. } | Dialog::ResetHard { commit, .. } =
            dialog
            && commit.subject.is_none()
        {
            let details = registry.dispatch(session, "vcs.commit.details", json!({ "id": commit.id })).ok();
            commit.subject =
                Some(details.and_then(|details| details["subject"].as_str().map(str::to_owned)).unwrap_or_default());
        }
        if let Dialog::DropStash { stash, .. } | Dialog::RenameStash { stash, .. } = &*dialog
            && stash.info.is_none()
        {
            let commit = stash.commit.clone();
            let list = registry.dispatch(session, "vcs.stash.list", json!({})).ok();
            let list = list.and_then(|list| serde_json::from_value::<StashList>(list).ok()).unwrap_or_default();
            let info = list.stashes.into_iter().find(|info| info.commit == commit).unwrap_or_else(|| StashInfo {
                name: commit.chars().take(7).collect(),
                commit: commit.clone(),
                ..StashInfo::default()
            });
            match dialog {
                Dialog::RenameStash { stash, message, .. } => {
                    *message = zen_vcs::stash_text(&info.message).to_owned();
                    stash.info = Some(info);
                }
                Dialog::DropStash { stash, .. } => stash.info = Some(info),
                _ => {}
            }
        }
        let branch = self.vcs.info.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
        let (mut confirm, mut cancel) = (false, false);
        let enter_confirms = !matches!(
            dialog,
            Dialog::CreateTag { .. }
                | Dialog::RenameBranch { .. }
                | Dialog::SetUpstream { .. }
                | Dialog::RenameStash { .. }
                | Dialog::CreateWorktree { .. }
        );
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let modal = egui::Modal::new(ui.scope_id().with(("gw-dialog", dialog.name())))
            .frame(super::dialog_frame(&p))
            .show(ui.ctx(), |ui| {
                ui.set_width(440.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let title = |ui: &mut Ui, text: String| {
                    ui.label(egui::RichText::new(text).font(theme::semibold(15.0)).color(p.text));
                    ui.add_space(12.0);
                };
                let text = |ui: &mut Ui, text: String| {
                    ui.add(egui::Label::new(egui::RichText::new(text).font(theme::regular(13.0)).color(p.text)).wrap());
                };
                let failure = |ui: &mut Ui, error: &Option<String>| {
                    if let Some(error) = error {
                        ui.add_space(8.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(error).font(theme::regular(12.0)).color(p.vcs_conflict),
                            )
                            .wrap(),
                        );
                    }
                };
                let (ok, cancel_label) = match dialog {
                    Dialog::CreateTag { commit, name, message, focused, error } => {
                        title(ui, t!("vcs-create-tag-title"));
                        ui.label(
                            egui::RichText::new(t!("vcs-create-tag-at", commit = commit.label()))
                                .font(theme::regular(12.5))
                                .color(p.text_secondary),
                        );
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new(t!("vcs-create-tag-name")).font(theme::regular(13.0)).color(p.text),
                        );
                        ui.add_space(6.0);
                        let field = ui.add(
                            egui::TextEdit::singleline(name)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if !std::mem::replace(focused, true) {
                            field.request_focus();
                        }
                        if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                            confirm = true;
                        }
                        ui.add_space(12.0);
                        ui.label(
                            egui::RichText::new(t!("vcs-create-tag-message")).font(theme::regular(13.0)).color(p.text),
                        );
                        ui.add_space(6.0);
                        ui.add(
                            egui::TextEdit::multiline(message)
                                .desired_width(f32::INFINITY)
                                .desired_rows(3)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        failure(ui, error);
                        (t!("vcs-create-tag-create"), Some(t!("settings-cancel")))
                    }
                    Dialog::Checkout { commit, error } => {
                        title(ui, t!("vcs-checkout-commit-title"));
                        text(ui, t!("vcs-checkout-commit-text", commit = commit.label()));
                        failure(ui, error);
                        (t!("vcs-checkout-commit-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::ResetHard { commit, error } => {
                        title(ui, t!("vcs-reset-hard-title"));
                        text(ui, t!("vcs-reset-hard-text", branch = branch.as_str(), commit = commit.label()));
                        failure(ui, error);
                        (t!("vcs-reset-hard-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::Conflicts { message } => {
                        title(ui, t!("vcs-conflicts-title"));
                        text(ui, message.clone());
                        (t!("vcs-conflict-resolve"), Some(t!("vcs-conflict-later")))
                    }
                    Dialog::RenameBranch { branch, name, focused, error } => {
                        title(ui, t!("vcs-rename-branch-title"));
                        ui.label(
                            egui::RichText::new(t!("vcs-rename-branch-text", branch = branch.as_str()))
                                .font(theme::regular(13.0))
                                .color(p.text),
                        );
                        ui.add_space(6.0);
                        let field = ui.add(
                            egui::TextEdit::singleline(name)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if !std::mem::replace(focused, true) {
                            field.request_focus();
                            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                                let all = egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(name.chars().count()),
                                );
                                state.cursor.set_char_range(Some(all));
                                state.store(ui.ctx(), field.id);
                            }
                        }
                        if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                            confirm = true;
                        }
                        failure(ui, error);
                        (t!("vcs-rename-branch-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::SetUpstream { branch, upstream, error } => {
                        title(ui, t!("vcs-set-upstream-title"));
                        text(ui, t!("vcs-set-upstream-text", branch = branch.as_str()));
                        ui.add_space(14.0);
                        ui.label(
                            egui::RichText::new(t!("vcs-set-upstream-remote")).font(theme::regular(13.0)).color(p.text),
                        );
                        ui.add_space(6.0);
                        let none = t!("vcs-set-upstream-none");
                        let shown = upstream.clone().unwrap_or_else(|| none.clone());
                        let combo = ui.scope(|ui| {
                            ui.spacing_mut().interact_size.y = 30.0;
                            ui.spacing_mut().button_padding.x = 8.0;
                            egui::ComboBox::from_id_salt("gw-set-upstream")
                                .selected_text(shown)
                                .width(ui.available_width())
                                .height(320.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(upstream, None, none.clone());
                                    for remote in &remotes {
                                        ui.selectable_value(upstream, Some(remote.clone()), remote.as_str());
                                    }
                                })
                        });
                        ui.painter().rect_stroke(
                            combo.inner.response.rect,
                            4,
                            Stroke::new(1.0, p.button_border),
                            egui::StrokeKind::Inside,
                        );
                        failure(ui, error);
                        (t!("vcs-set-upstream-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::DeleteRemoteBranch { branch, error } => {
                        title(ui, t!("vcs-delete-remote-title"));
                        text(ui, t!("vcs-delete-remote-text", branch = branch.as_str()));
                        failure(ui, error);
                        (t!("vcs-delete-remote-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::DropStash { stash, error } => {
                        title(ui, t!("vcs-drop-stash-title"));
                        text(ui, t!("vcs-drop-stash-text", stash = stash.label()));
                        failure(ui, error);
                        (t!("vcs-drop-stash-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::Discard { text: says, error, .. } => {
                        title(ui, t!("vcs-discard-title"));
                        text(ui, says.clone());
                        failure(ui, error);
                        (t!("vcs-discard-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::CheckoutAndDrop { action, source, target, error } => {
                        let (source, target) = (source.name.as_str(), target.name.as_str());
                        let merge = *action == DropAction::Merge;
                        title(ui, t!(if merge { "vcs-checkout-merge-title" } else { "vcs-checkout-rebase-title" }));
                        let says = if merge { "vcs-checkout-merge-text" } else { "vcs-checkout-rebase-text" };
                        text(ui, t!(says, source = source, target = target));
                        failure(ui, error);
                        let ok = if merge { "vcs-checkout-merge-ok" } else { "vcs-checkout-rebase-ok" };
                        (t!(ok), Some(t!("settings-cancel")))
                    }
                    Dialog::RenameStash { stash, message, focused, error } => {
                        title(ui, t!("vcs-rename-stash-title"));
                        text(ui, t!("vcs-rename-stash-text", stash = stash.label()));
                        ui.add_space(10.0);
                        let field = ui.add(
                            egui::TextEdit::singleline(message)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if stash.info.is_some() && !std::mem::replace(focused, true) {
                            field.request_focus();
                            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), field.id) {
                                let all = egui::text::CCursorRange::two(
                                    egui::text::CCursor::new(0),
                                    egui::text::CCursor::new(message.chars().count()),
                                );
                                state.cursor.set_char_range(Some(all));
                                state.store(ui.ctx(), field.id);
                            }
                        }
                        if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                            confirm = true;
                        }
                        failure(ui, error);
                        (t!("vcs-rename-stash-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::CreateWorktree { branch, folder, folder_edited, new_branch, open, focused, error } => {
                        title(ui, t!("vcs-create-worktree-title"));
                        text(ui, t!("vcs-create-worktree-text", branch = branch.name.as_str()));
                        ui.add_space(14.0);
                        let label = |ui: &mut Ui, text: String| {
                            ui.label(egui::RichText::new(text).font(theme::regular(13.0)).color(p.text));
                            ui.add_space(6.0);
                        };
                        label(ui, t!("vcs-create-worktree-new-branch"));
                        let named = ui.add(
                            egui::TextEdit::singleline(new_branch)
                                .hint_text(t!("vcs-create-worktree-new-branch-hint", branch = branch.name.as_str()))
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if !std::mem::replace(focused, true) {
                            named.request_focus();
                        }
                        if named.changed() && !*folder_edited {
                            let local = worktrees::local_name(&branch.name, branch.kind == RefKind::Remote);
                            let name = if new_branch.trim().is_empty() { local } else { new_branch.trim().to_owned() };
                            *folder = worktrees::offered_folder(&root, &name);
                        }
                        ui.add_space(12.0);
                        label(ui, t!("vcs-create-worktree-folder"));
                        let typed = ui.add(
                            egui::TextEdit::singleline(folder)
                                .desired_width(f32::INFINITY)
                                .margin(egui::Margin::symmetric(8, 6)),
                        );
                        if typed.changed() {
                            *folder_edited = true;
                        }
                        if (named.lost_focus() || typed.lost_focus())
                            && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        {
                            confirm = true;
                        }
                        ui.add_space(10.0);
                        ui.checkbox(open, t!("vcs-create-worktree-open"));
                        failure(ui, error);
                        (t!("vcs-create-worktree-ok"), Some(t!("settings-cancel")))
                    }
                    Dialog::RemoveWorktree { worktree, delete_branch, error } => {
                        title(ui, t!("vcs-remove-worktree-title"));
                        text(ui, worktrees::remove_text(worktree));
                        if let Some(branch) = &worktree.branch {
                            ui.add_space(12.0);
                            ui.checkbox(delete_branch, t!("vcs-remove-worktree-branch", branch = branch.as_str()));
                        }
                        failure(ui, error);
                        (t!("vcs-remove-worktree-ok"), Some(t!("settings-cancel")))
                    }
                };
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if super::vcs::primary_button(ui, &p, &ok).clicked() {
                        confirm = true;
                    }
                    if let Some(label) = cancel_label
                        && super::vcs::secondary_button(ui, &p, &label).clicked()
                    {
                        cancel = true;
                    }
                });
            });
        if enter_confirms && !cancel && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
            confirm = true;
        }
        let closed = cancel || (!confirm && modal.should_close());
        if closed {
            self.git_workspace.dialog = None;
            return;
        }
        if !confirm {
            return;
        }
        let (command, params, select) = match dialog {
            Dialog::CreateTag { commit, name, message, .. } => {
                let message = (!message.trim().is_empty()).then(|| message.clone());
                ("vcs.tag.create", json!({ "name": name.trim(), "revision": commit.id, "message": message }), None)
            }
            Dialog::Checkout { commit, .. } => ("vcs.revision.checkout", json!({ "revision": commit.id }), None),
            Dialog::ResetHard { commit, .. } => {
                ("vcs.branch.reset", json!({ "revision": commit.id, "mode": "hard" }), Some(commit.id.clone()))
            }
            Dialog::Conflicts { .. } => {
                self.git_workspace.dialog = None;
                self.dispatch(ui, registry, session, "vcs.conflict.open", json!({}));
                self.git_workspace.conflicts_changed();
                return;
            }
            Dialog::RenameBranch { branch, name, .. } => {
                ("vcs.branch.rename", json!({ "name": branch, "newName": name.trim() }), None)
            }
            Dialog::SetUpstream { branch, upstream, .. } => {
                ("vcs.branch.setUpstream", json!({ "name": branch, "upstream": upstream }), None)
            }
            Dialog::DeleteRemoteBranch { branch, .. } => ("vcs.branch.deleteRemote", json!({ "name": branch }), None),
            Dialog::DropStash { stash, .. } => {
                ("vcs.stash.drop", json!({ "commit": stash.commit }), Some("HEAD".into()))
            }
            Dialog::RenameStash { stash, message, .. } => {
                ("vcs.stash.rename", json!({ "commit": stash.commit, "message": message.trim() }), None)
            }
            Dialog::Discard { params, .. } => ("vcs.changes.discard", json!(params), None),
            Dialog::CreateWorktree { branch, folder, new_branch, open, .. } => {
                ("vcs.worktree.create", worktrees::create_params(&branch.name, folder, new_branch, *open), None)
            }
            Dialog::RemoveWorktree { worktree, delete_branch, .. } => {
                ("vcs.worktree.remove", worktrees::remove_params(worktree, *delete_branch), None)
            }
            Dialog::CheckoutAndDrop { action, source, target, .. } => (
                "vcs.workspace.dropAction",
                json!({ "source": source, "target": target, "action": action, "confirmed": true }),
                None,
            ),
        };
        let discarded = matches!(dialog, Dialog::Discard { .. });
        let answer = zen_workbench::vcs::off_the_frame(session, |session| registry.dispatch(session, command, params));
        match answer {
            Ok(_) => {
                self.git_workspace.dialog = None;
                self.git_workspace.refs_read_for = None;
                self.git_workspace.worktrees_revision = None;
                if discarded {
                    self.git_workspace.changes_read_for = None;
                    let _ = registry.dispatch(session, "vcs.status.refresh", json!({}));
                    self.refresh_diff(registry, session);
                }
                if let Some(revision) = select {
                    self.dispatch(ui, registry, session, "vcs.workspace.select", json!({ "revision": revision }));
                }
            }
            Err(failed) => {
                let message = match failed {
                    zen_core::DispatchError::Failed { error, .. } => error.message,
                    other => other.to_string(),
                };
                if let Some(
                    Dialog::CreateTag { error, .. }
                    | Dialog::Checkout { error, .. }
                    | Dialog::ResetHard { error, .. }
                    | Dialog::RenameBranch { error, .. }
                    | Dialog::SetUpstream { error, .. }
                    | Dialog::DeleteRemoteBranch { error, .. }
                    | Dialog::DropStash { error, .. }
                    | Dialog::RenameStash { error, .. }
                    | Dialog::Discard { error, .. }
                    | Dialog::CheckoutAndDrop { error, .. }
                    | Dialog::CreateWorktree { error, .. }
                    | Dialog::RemoveWorktree { error, .. },
                ) = &mut self.git_workspace.dialog
                {
                    *error = Some(message);
                }
            }
        }
    }
}

fn lane_color(p: &IdePalette, index: usize) -> Color32 {
    p.graph[index % p.graph.len()]
}

#[allow(clippy::too_many_arguments)]
fn draw_commit_row(
    ui: &Ui,
    p: &IdePalette,
    columns: &Columns,
    rect: Rect,
    response: &egui::Response,
    commit: &GraphCommit,
    (selected, found, matcher): (bool, Found, Option<&zen_vcs::filter::TextMatcher>),
    picture: Option<&egui::TextureHandle>,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let color = lane_color(p, commit.color);
    let middle = rect.center().y;
    let node = pos2(columns.node_x(commit.lane), middle);
    let (band_left, band_right) = columns.band(node.x, rect);
    let band = Rect::from_min_max(pos2(band_left, rect.top() + 2.0), pos2(band_right, rect.bottom() - 2.0));
    let fill = match (selected, found, response.hovered()) {
        (true, ..) => look::row_tint(p, color, Tint::Selected),
        (false, Found::Match | Found::Current, _) => search::row_tint(p),
        (false, Found::No, true) => look::row_tint(p, color, Tint::Hovered),
        (false, Found::No, false) => look::row_tint(p, color, Tint::Row),
    };
    painter.rect_filled(band, 0, fill);
    painter.rect_filled(Rect::from_min_size(pos2(columns.message.0, band.top()), vec2(2.0, band.height())), 0, color);
    if found == Found::Current {
        painter.rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0, p.search_mark);
    }
    let pills = look::pills(&commit.refs);
    if let Some(first) = pills.first() {
        let refs = painter.with_clip_rect(Rect::from_x_y_ranges(columns.refs.0..=columns.refs.1 - 4.0, rect.y_range()));
        let end = draw_pill(&refs, p, first, pills.len() - 1, columns.refs, middle, color);
        let line = if columns.refs.1 == columns.graph.0 && end < node.x - NODE {
            Some((end, node.x - NODE))
        } else if columns.graph.1 == columns.refs.0 && node.x + NODE < columns.refs.0 + 8.0 {
            Some((node.x + NODE, columns.refs.0 + 8.0))
        } else {
            None
        };
        if let Some((from, to)) = line {
            painter.line_segment([pos2(from, middle), pos2(to, middle)], Stroke::new(1.0, color));
        }
        let over_labels = |pos: egui::Pos2| pos.x >= columns.refs.0 && pos.x < columns.refs.1;
        if response.hovered() && ui.input(|input| input.pointer.hover_pos()).is_some_and(over_labels) {
            let names: Vec<&str> = commit.refs.iter().map(|label| label.name.as_str()).collect();
            hint(ui, response, &names.join(", "), None, Side::Right);
        }
    }
    draw_edges(&painter, p, columns, commit, rect);
    let graph = painter.with_clip_rect(Rect::from_x_y_ranges(columns.graph.0..=columns.graph.1, rect.y_range()));
    if commit.stash.is_some() {
        let square = Rect::from_center_size(node, Vec2::splat(NODE * 2.0 - 2.0));
        graph.rect_filled(square, 4, fill);
        graph.rect_stroke(square, 4, Stroke::new(look::LINE, color), egui::StrokeKind::Inside);
        glyph_stash(&graph, Rect::from_center_size(node, Vec2::splat(10.0)), color);
    } else if commit.parents.len() > 1 {
        graph.circle_filled(node, MERGE_NODE, color);
    } else {
        avatars::draw_face(&graph, p, (node, NODE - 1.0), &commit.author, 8.5, picture);
        graph.circle_stroke(node, NODE - 1.0, Stroke::new(look::LINE, color));
    }
    let text = if commit.stash.is_some() { p.text_secondary } else { p.text };
    let mark = |part: &str| matcher.and_then(|matcher| matcher.find(part));
    let highlight = p.search;
    let message = search::found_cell(
        &painter,
        columns.message,
        middle,
        &commit.subject,
        (theme::regular(13.0), text),
        mark(&commit.subject),
        highlight,
    );
    if let Some(author) = columns.author {
        let font = (theme::regular(12.5), p.text_secondary);
        search::found_cell(&painter, author, middle, &commit.author, font, mark(&commit.author), highlight);
    }
    if let Some(date) = columns.date {
        let text = format_date(commit.time + i64::from(commit.offset));
        cell(&painter, date, middle, &text, theme::regular(12.5), p.text_secondary);
    }
    if let Some(sha) = columns.sha {
        let found = matcher.filter(|matcher| matcher.matches_id(&commit.id)).map(|matcher| {
            matcher.find(&commit.short_id).filter(|range| range.start == 0).unwrap_or(0..commit.short_id.len())
        });
        let font = (FontId::monospace(12.0), p.text_secondary);
        search::found_cell(&painter, sha, middle, &commit.short_id, font, found, highlight);
    }
    let over_message = response.hover_pos().is_some_and(|pos| pos.x >= columns.message.0 && pos.x < columns.message.1);
    if message && over_message {
        response.clone().on_hover_text(&commit.subject);
    }
}

fn details_panel(body: Rect, middle: Rect, dragged: Option<f32>) -> (Rect, Rect) {
    let own = (body.width() * 0.27).clamp(270.0, 380.0);
    let width = dragged
        .unwrap_or(own)
        .min(body.width() * 0.7)
        .max(DETAILS_WIDTH.0.min(middle.width()))
        .min(middle.width())
        .max(0.0);
    let panel = Rect::from_min_max(pos2(body.right() - width, middle.top()), body.max);
    let shade = Rect::from_min_max(pos2((panel.left() - 24.0).max(middle.left()), middle.top()), body.max);
    (panel, shade)
}

fn details_edge(ui: &mut Ui, p: &IdePalette, panel: Rect) -> Option<f32> {
    let handle = Rect::from_x_y_ranges(panel.left()..=panel.left() + 6.0, panel.y_range());
    let edge = ui.interact(handle, ui.scope_id().with("gw-details-edge"), Sense::drag());
    let active = edge.hovered() || edge.dragged();
    if active {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    let line = Stroke::new(if active { 2.0 } else { 1.0 }, if active { p.accent } else { p.frame });
    ui.painter().vline(panel.left() + 0.5, panel.y_range(), line);
    let at = edge.interact_pointer_pos().filter(|_| edge.dragged())?;
    Some(panel.right() - at.x)
}

fn close_button(ui: &mut Ui, p: &IdePalette, panel: Rect) -> bool {
    let rect = Rect::from_min_size(pos2(panel.right() - 30.0, panel.top() + 8.0), Vec2::splat(22.0));
    let response = ui.interact(rect, ui.scope_id().with("gw-details-close"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let color = if response.hovered() { p.text } else { p.text_secondary };
    super::find_bar::glyph_close(ui.painter(), rect.shrink(6.0), color);
    hint(ui, &response, &t!("vcs-workspace-details-close"), Some("⎋"), Side::Below);
    response.clicked()
}

fn draw_edges(painter: &egui::Painter, p: &IdePalette, columns: &Columns, commit: &GraphCommit, rect: Rect) {
    let edges = commit.up.iter().map(|edge| (edge, false)).chain(commit.down.iter().map(|edge| (edge, true)));
    for (edge, lower) in edges {
        let Some((from, to, (left, right))) = columns.edge(edge.from, edge.to, lower, rect) else { continue };
        let graph = painter.with_clip_rect(Rect::from_x_y_ranges(left..=right, rect.y_range()));
        look::draw_elbow(&graph, from, to, lower, lane_color(p, edge.color));
    }
}

fn draw_pill(
    painter: &egui::Painter,
    p: &IdePalette,
    pill: &look::Pill,
    more: usize,
    refs: (f32, f32),
    middle: f32,
    lane: Color32,
) -> f32 {
    const ICON: f32 = 12.0;
    let fill = look::pill_fill(p, lane, pill.current);
    let text = look::pill_text(p, pill.current);
    let more_text = (more > 0).then(|| format!("+{more}"));
    let more_galley = more_text.map(|more| painter.layout_no_wrap(more, theme::regular(11.5), p.text));
    let more_width = more_galley.as_ref().map_or(0.0, |galley| galley.size().x + 14.0);
    let mut marks: Vec<fn(&egui::Painter, Rect, Color32)> =
        [(pill.local, crate::icons::laptop as fn(&egui::Painter, Rect, Color32)), (pill.remote, glyph_cloud)]
            .into_iter()
            .filter_map(|(shown, glyph)| shown.then_some(glyph))
            .collect();
    let mut tag = pill.kind == RefKind::Tag;
    let check_width = if pill.current { ICON + 4.0 } else { 0.0 };
    let left = refs.0 + 8.0;
    let max_width = (refs.1 - 8.0 - left - more_width).max(24.0);
    let mut marks_width = (marks.len() + usize::from(tag)) as f32 * (ICON + 4.0);
    if max_width - 16.0 - marks_width - check_width < 44.0 {
        marks.clear();
        tag = false;
        marks_width = 0.0;
    }
    let name_room = (max_width - 16.0 - marks_width - check_width).max(8.0);
    let font = if pill.current { theme::semibold(12.5) } else { theme::regular(12.5) };
    let galley = painter.layout_job(super::vcs::truncated(&pill.name, font, text, name_room));
    let width = (8.0 + check_width + galley.size().x + marks_width + 8.0).min(max_width);
    let rect = Rect::from_min_size(pos2(left, middle - 11.0), vec2(width, 22.0));
    painter.rect_filled(rect, 4, fill);
    let mut x = rect.left() + 8.0;
    if pill.current {
        glyph_check(painter, Rect::from_center_size(pos2(x + ICON / 2.0, middle), Vec2::splat(ICON - 1.0)), text);
        x += ICON + 4.0;
    }
    let size = galley.size();
    painter.with_clip_rect(rect.shrink2(vec2(4.0, 0.0))).galley(pos2(x, middle - size.y / 2.0), galley, text);
    x += size.x + 4.0;
    for mark in marks {
        mark(painter, Rect::from_center_size(pos2(x + ICON / 2.0, middle), Vec2::splat(ICON + 1.0)), text);
        x += ICON + 4.0;
    }
    if tag {
        glyph_label(painter, Rect::from_center_size(pos2(x + ICON / 2.0, middle), Vec2::splat(ICON - 1.0)), text, fill);
    }
    let mut end = rect.right();
    if let Some(galley) = more_galley {
        let badge = Rect::from_min_size(pos2(rect.right() + 4.0, middle - 11.0), vec2(galley.size().x + 10.0, 22.0));
        painter.rect_filled(badge, 4, look::mix(p.editor_background, lane, 0.32));
        painter.galley(badge.center() - galley.size() / 2.0, galley, p.text);
        end = badge.right();
    }
    end
}

fn cell(painter: &egui::Painter, range: (f32, f32), middle: f32, text: &str, font: FontId, color: Color32) -> bool {
    let width = (range.1 - range.0 - 18.0).max(0.0);
    let galley = painter.layout_job(super::vcs::truncated(text, font, color, width));
    let elided = galley.elided;
    painter.galley(pos2(range.0 + 10.0, middle - galley.size().y / 2.0), galley, color);
    elided
}

#[derive(Default)]
struct WipHead<'a> {
    head: Option<&'a GraphCommit>,
    above: usize,
}

fn wip_head<'a>(rows: &'a [GraphCommit], head: Option<&str>) -> WipHead<'a> {
    let above = rows.iter().take_while(|row| row.stash.is_some() || row.worktree.is_some()).count();
    match rows.get(above) {
        Some(row) if head == Some(row.id.as_str()) => WipHead { head: Some(row), above },
        _ => WipHead::default(),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_wip_row(
    ui: &Ui,
    p: &IdePalette,
    columns: &Columns,
    rect: Rect,
    response: &egui::Response,
    selected: bool,
    changes: usize,
    head_row: Option<&GraphCommit>,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let middle = rect.center().y;
    let lane = head_row.map_or(0, |commit| commit.lane);
    let color = head_row.map_or(p.text_secondary, |commit| lane_color(p, commit.color));
    let node = pos2(columns.node_x(lane), middle);
    let tint = if selected {
        Tint::Selected
    } else if response.hovered() {
        Tint::Hovered
    } else {
        Tint::Row
    };
    let band = Rect::from_min_max(
        pos2(columns.band(node.x, rect).0, rect.top() + 2.0),
        pos2(columns.band(node.x, rect).1, rect.bottom() - 2.0),
    );
    painter.rect_filled(band, 0, look::row_tint(p, color, tint));
    painter.rect_filled(Rect::from_min_size(pos2(columns.message.0, band.top()), vec2(2.0, band.height())), 0, color);
    if head_row.is_some() {
        dashed(&painter, node + vec2(0.0, NODE), pos2(node.x, rect.bottom()), color);
    }
    dashed_ring(&painter, node, color);
    let label = painter.text(
        pos2(columns.message.0 + 10.0, middle),
        Align2::LEFT_CENTER,
        t!("vcs-workspace-wip"),
        theme::semibold(13.0),
        p.text_secondary,
    );
    let count = t!("vcs-workspace-wip-count", count = changes);
    let galley = painter.layout_no_wrap(count, theme::regular(11.5), p.text);
    let badge = Rect::from_min_size(pos2(label.right() + 10.0, middle - 9.0), vec2(galley.size().x + 14.0, 18.0));
    painter.rect_filled(badge, 9, p.vcs_modified.gamma_multiply(0.3));
    painter.galley(badge.center() - galley.size() / 2.0, galley, p.text);
}

fn dashed_ring(painter: &egui::Painter, node: egui::Pos2, color: Color32) {
    for dash in 0..12 {
        let start = dash as f32 / 12.0 * std::f32::consts::TAU;
        let points: Vec<egui::Pos2> = (0..=3)
            .map(|step| {
                let angle = start + step as f32 / 3.0 * std::f32::consts::TAU / 24.0;
                node + vec2(angle.cos(), angle.sin()) * (NODE - 1.0)
            })
            .collect();
        painter.add(egui::Shape::line(points, Stroke::new(look::LINE, color)));
    }
}

fn dashed(painter: &egui::Painter, from: egui::Pos2, to: egui::Pos2, color: Color32) {
    let mut y = from.y;
    while y < to.y {
        let end = (y + 3.0).min(to.y);
        painter.line_segment([pos2(from.x, y), pos2(from.x, end)], Stroke::new(look::LINE, color));
        y += 6.0;
    }
}

fn draw_avatar(painter: &egui::Painter, p: &IdePalette, center: egui::Pos2, radius: f32, name: &str, size: f32) {
    let hash = name.bytes().fold(0usize, |hash, byte| hash.wrapping_mul(31).wrapping_add(byte as usize));
    painter.circle_filled(center, radius, lane_color(p, hash));
    painter.text(center, Align2::CENTER_CENTER, initials(name), theme::semibold(size), p.project_text);
}

pub(super) fn initials(name: &str) -> String {
    let letters: String = name
        .split(|c: char| c.is_whitespace() || c == '.' || c == '-' || c == '_')
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    if letters.is_empty() { "?".to_owned() } else { letters }
}

struct RefRow<'a> {
    name: &'a str,
    kind: RefKind,
    current: bool,
    selected: bool,
    badge: &'a str,
    indent: f32,
    focus: search::RowFocus,
}

fn ref_row(
    ui: &mut Ui,
    p: &IdePalette,
    row: &RefRow<'_>,
    drag: &mut RefDrag,
    target: &RefTarget,
) -> (egui::Response, Option<FocusButton>) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click_and_drag());
    let inner = rect.shrink2(vec2(6.0, 0.0));
    let dropping = drag.track(ui, &response, Some(target), Some(target));
    let hovered = drag.source.is_none() && ui.rect_contains_pointer(rect);
    if dropping {
        drop_highlight(ui.painter(), p, inner);
    } else if row.selected {
        ui.painter().rect_filled(inner, 5, p.selection);
    } else if hovered {
        ui.painter().rect_filled(inner, 5, p.hover);
    }
    let (mut right, button) = if row.kind == RefKind::Tag {
        (inner.right() - 6.0, None)
    } else {
        search::focus_buttons(
            ui,
            p,
            inner,
            row.focus,
            hovered,
            ui.scope_id().with(("gw-focus", row.kind == RefKind::Remote, target.name.as_str())),
        )
    };
    let text = if row.focus.out() { search::out_text(p) } else { p.text };
    let icon = Rect::from_center_size(pos2(rect.left() + row.indent + 26.0, rect.center().y), Vec2::splat(12.0));
    match (row.kind, row.current) {
        (_, true) => glyph_label(ui.painter(), icon, p.vcs_current_branch, p.island),
        (RefKind::Tag, _) => glyph_label(ui.painter(), icon, p.text_secondary, p.island),
        _ => glyph_branch(ui.painter(), icon, p.text_secondary),
    }
    if !row.badge.is_empty() {
        let badge = ui.painter().text(
            pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            row.badge,
            theme::regular(11.5),
            p.text_secondary,
        );
        right = badge.left() - 6.0;
    }
    let font = if row.current { theme::semibold(13.0) } else { theme::regular(13.0) };
    ui.painter().with_clip_rect(Rect::from_min_max(inner.min, pos2(right, inner.bottom()))).text(
        pos2(icon.right() + 8.0, rect.center().y),
        Align2::LEFT_CENTER,
        row.name,
        font,
        text,
    );
    (response, button)
}

fn viewing_row(ui: &mut Ui, p: &IdePalette, count: usize) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
    let label = ui.painter().text(
        pos2(rect.left() + 14.0, rect.bottom() - 10.0),
        Align2::LEFT_CENTER,
        t!("vcs-workspace-viewing"),
        theme::regular(13.0),
        p.text_secondary,
    );
    ui.painter().text(
        pos2(label.right() + 6.0, rect.bottom() - 10.0),
        Align2::LEFT_CENTER,
        count.to_string(),
        theme::semibold(13.0),
        p.link,
    );
}

fn section_header(
    ui: &mut Ui,
    p: &IdePalette,
    collapsed: &mut BTreeSet<Section>,
    filtering: bool,
    section: Section,
    title: &str,
    count: usize,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    ui.painter().hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, p.frame));
    if response.hovered() {
        ui.painter().rect_filled(rect.shrink2(vec2(0.0, 1.0)), 0, p.hover);
    }
    let open = filtering || collapsed.contains(&section);
    let chevron = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(10.0));
    if open {
        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
    } else {
        crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 34.0, rect.center().y), Vec2::splat(15.0));
    match section {
        Section::Local => crate::icons::laptop(ui.painter(), icon, p.text_secondary),
        Section::Remote => glyph_cloud(ui.painter(), icon, p.text_secondary),
        Section::Tags => glyph_label(ui.painter(), icon.shrink(1.5), p.text_secondary, p.island),
        Section::Stashes => glyph_stash(ui.painter(), icon.shrink(1.0), p.text_secondary),
        Section::Worktrees => glyph_worktree(ui.painter(), icon.shrink(0.5), p.text_secondary),
        Section::Submodules => glyph_submodule(ui.painter(), icon.shrink(1.0), p.text_secondary),
    }
    let galley = ui.painter().layout_no_wrap(count.to_string(), theme::semibold(12.5), p.link);
    let count_left = rect.right() - 14.0 - galley.size().x;
    ui.painter().galley(pos2(count_left, rect.center().y - galley.size().y / 2.0), galley, p.link);
    let (galley, shortened) =
        look::small_caps_fitted(ui.painter(), title, 12.0, p.text, count_left - 6.0 - (rect.left() + 50.0));
    ui.painter().galley(pos2(rect.left() + 50.0, rect.center().y - galley.size().y / 2.0), galley, p.text);
    if shortened {
        hint(ui, &response, &title.to_uppercase(), None, Side::Right);
    }
    if response.clicked() && !filtering && !collapsed.remove(&section) {
        collapsed.insert(section);
    }
    open
}

fn folder_row(
    ui: &mut Ui,
    p: &IdePalette,
    collapsed: &mut BTreeSet<String>,
    (key, name): (&str, &str),
    indent: f32,
    open: bool,
    count: usize,
) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let inner = rect.shrink2(vec2(6.0, 0.0));
    if response.hovered() {
        ui.painter().rect_filled(inner, 5, p.hover);
    }
    let chevron = Rect::from_center_size(pos2(rect.left() + indent + 13.0, rect.center().y), Vec2::splat(9.0));
    if open {
        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
    } else {
        crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + indent + 27.0, rect.center().y), Vec2::splat(13.0));
    glyph_folder(ui.painter(), icon, p.text_secondary);
    let mut right = inner.right() - 6.0;
    if !open {
        let badge = ui.painter().text(
            pos2(right, rect.center().y),
            Align2::RIGHT_CENTER,
            count.to_string(),
            theme::regular(11.5),
            p.text_secondary,
        );
        right = badge.left() - 6.0;
    }
    ui.painter().with_clip_rect(Rect::from_min_max(inner.min, pos2(right, inner.bottom()))).text(
        pos2(icon.right() + 7.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        theme::regular(13.0),
        p.text,
    );
    if response.clicked() && !collapsed.remove(key) {
        collapsed.insert(key.to_owned());
    }
}

fn file_status(status: &str) -> zen_vcs::FileStatus {
    match status {
        "added" => zen_vcs::FileStatus::Added,
        "deleted" => zen_vcs::FileStatus::Deleted,
        "renamed" => zen_vcs::FileStatus::Renamed,
        "unversioned" => zen_vcs::FileStatus::Unversioned,
        "conflicted" => zen_vcs::FileStatus::Conflicted,
        _ => zen_vcs::FileStatus::Modified,
    }
}

fn status_mark(painter: &egui::Painter, p: &IdePalette, rect: Rect, status: zen_vcs::FileStatus) {
    let color = status_color(p, status);
    match status {
        zen_vcs::FileStatus::Added | zen_vcs::FileStatus::Unversioned => glyph_plus(painter, rect.shrink(1.0), color),
        zen_vcs::FileStatus::Deleted => glyph_minus(painter, rect.shrink(1.0), color),
        zen_vcs::FileStatus::Renamed => glyph_forward(painter, rect.shrink(1.0), color),
        zen_vcs::FileStatus::Conflicted => {
            painter.text(rect.center(), Align2::CENTER_CENTER, "!", theme::semibold(13.0), color);
        }
        _ => glyph_pencil(painter, rect, color),
    }
}

fn file_line(
    ui: &mut Ui,
    p: &IdePalette,
    relative: &str,
    status: Option<&str>,
    (indent, folder, room): (f32, bool, f32),
) -> egui::Response {
    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    if ui.rect_contains_pointer(row) {
        ui.painter().rect_filled(row.shrink2(vec2(8.0, 0.0)), 5, p.hover);
    }
    let left = row.left() + indent;
    if let Some(status) = status {
        let mark = Rect::from_center_size(pos2(left + 24.0, row.center().y), Vec2::splat(13.0));
        status_mark(ui.painter(), p, mark, file_status(status));
    }
    let (path, name) = look::split_path(relative);
    let text = Rect::from_min_max(row.min, pos2((row.right() - room).max(row.left()), row.bottom()));
    let painter = ui.painter().with_clip_rect(text.shrink2(vec2(8.0, 0.0)).intersect(ui.clip_rect()));
    let (dim, bright) = if status.is_some() {
        (p.text_secondary, p.text)
    } else {
        (p.text_secondary.gamma_multiply(0.7), p.text_secondary)
    };
    let mut x = left + 38.0;
    if folder && !path.is_empty() {
        x = painter.text(pos2(x, row.center().y), Align2::LEFT_CENTER, path, theme::regular(13.0), dim).right();
    }
    painter.text(pos2(x, row.center().y), Align2::LEFT_CENTER, name, theme::regular(13.0), bright);
    if status.is_some() { response.on_hover_cursor(egui::CursorIcon::PointingHand) } else { response }
}

fn file_row_clipped(ui: &mut Ui, p: &IdePalette, relative: &str, status: &str, room: f32) -> egui::Response {
    file_line(ui, p, relative, Some(status), (0.0, true, room))
}

fn changes_banner(ui: &mut Ui, p: &IdePalette, count: usize) -> bool {
    const CLOSE_ROOM: f32 = 34.0;
    let width = ui.available_width();
    let galley = ui.painter().layout_no_wrap(t!("vcs-workspace-view-changes"), theme::regular(12.5), p.text);
    let room = (width - galley.size().x - 20.0 - 12.0 - 24.0 - CLOSE_ROOM).max(40.0);
    let text = t!("vcs-workspace-banner-changes", count = count);
    let font = theme::regular(13.5);
    let mut words = ui.painter().layout(text.clone(), font.clone(), p.text, room);
    let stacked = words.rows.len() > 2;
    if stacked {
        words = ui.painter().layout(text, font, p.text, (width - 28.0 - CLOSE_ROOM).max(40.0));
    }
    let height = if stacked { words.size().y + 58.0 } else { (words.size().y + 24.0).max(44.0) };
    let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    ui.painter().rect_filled(rect, 0, p.banner);
    let text_y = if stacked { rect.top() + 12.0 } else { rect.center().y - words.size().y / 2.0 };
    let words_bottom = text_y + words.size().y;
    ui.painter().galley(pos2(rect.left() + 14.0, text_y), words, p.text);
    let button_min = if stacked {
        pos2(rect.left() + 14.0, words_bottom + 8.0)
    } else {
        pos2(rect.right() - CLOSE_ROOM - galley.size().x - 20.0, rect.center().y - 13.0)
    };
    let button = Rect::from_min_size(button_min, vec2(galley.size().x + 20.0, 26.0));
    let response = ui.interact(button, ui.scope_id().with("gw-view-changes"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(button, 4, p.hover);
    }
    ui.painter().rect_stroke(button, 4, Stroke::new(1.0, p.text_secondary), egui::StrokeKind::Inside);
    ui.painter().galley(button.center() - galley.size() / 2.0, galley, p.text);
    response.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn parent_links(ui: &mut Ui, p: &IdePalette, label: &str, parents: &[String], asks: &mut Vec<Ask>, reversed: bool) {
    if parents.is_empty() {
        return;
    }
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut parts: Vec<Option<(&String, String)>> = vec![None];
    for (index, parent) in parents.iter().enumerate() {
        let short: String = parent.chars().take(7).collect();
        let text = if index + 1 < parents.len() { format!("{short},") } else { short };
        parts.push(Some((parent, text)));
    }
    if reversed {
        parts.reverse();
    }
    for part in parts {
        let Some((parent, text)) = part else {
            ui.label(egui::RichText::new(label).font(theme::regular(12.5)).color(p.text_secondary));
            continue;
        };
        let link = ui.add(
            egui::Label::new(egui::RichText::new(text).font(FontId::monospace(12.5)).color(p.link))
                .sense(Sense::click()),
        );
        if link.clicked() {
            asks.push(Ask::Select(parent.clone()));
        }
        link.on_hover_cursor(egui::CursorIcon::PointingHand);
    }
}

fn file_counts(ui: &mut Ui, p: &IdePalette, files: &[zen_workbench::vcs::log::CommitFile]) {
    let count = |status: &str| files.iter().filter(|file| file.status == status).count();
    let kinds = [
        ("modified", "vcs-workspace-count-modified"),
        ("added", "vcs-workspace-count-added"),
        ("deleted", "vcs-workspace-count-deleted"),
        ("renamed", "vcs-workspace-count-renamed"),
    ];
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let mut any = false;
        for (status, key) in kinds {
            let count = count(status);
            if count == 0 {
                continue;
            }
            if any {
                ui.add_space(10.0);
            }
            any = true;
            let (mark, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            status_mark(ui.painter(), p, mark.shrink(0.5), file_status(status));
            ui.label(egui::RichText::new(t!(key, count = count)).font(theme::regular(13.5)).color(p.text));
        }
        if !any {
            ui.label(
                egui::RichText::new(t!("vcs-workspace-no-files")).font(theme::regular(13.0)).color(p.text_secondary),
            );
        }
    });
}

fn files_toolbar(ui: &mut Ui, p: &IdePalette, settings: WorkspaceView, all_files: bool) -> Option<Ask> {
    let mut ask = None;
    let font = theme::regular(12.5);
    let segments = [
        (FilesView::Path, "vcs-workspace-files-path", glyph_list as fn(&egui::Painter, Rect, Color32), "path"),
        (FilesView::Tree, "vcs-workspace-files-tree", glyph_tree, "tree"),
    ];
    let widths: Vec<f32> = segments
        .iter()
        .map(|(_, key, ..)| ui.painter().layout_no_wrap(t!(key), font.clone(), p.text).size().x + 38.0)
        .collect();
    let check = ui.painter().layout_no_wrap(t!("vcs-workspace-all-files"), font.clone(), p.text);
    let check_width = if all_files { check.size().x + 26.0 } else { 0.0 };
    let toggle_width: f32 = widths.iter().sum();
    let one_line = toggle_width + 16.0 + check_width <= ui.available_width();
    let (row, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), if one_line { 28.0 } else { 60.0 }), Sense::hover());
    let mut x = row.left();
    for (index, ((view, key, glyph, name), width)) in segments.into_iter().zip(widths).enumerate() {
        let rect = Rect::from_min_size(pos2(x, row.top()), vec2(width, 28.0));
        let response = ui.interact(rect, ui.scope_id().with(("gw-files-view", index)), Sense::click());
        let chosen = settings.files == view;
        look::segment(ui, p, rect, index == 0, chosen, response.hovered());
        let color = if chosen { p.text } else { p.text_secondary };
        glyph(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(12.0)),
            color,
        );
        ui.painter().text(pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, t!(key), font.clone(), color);
        if response.clicked() && !chosen {
            ask = Some(Ask::Command("vcs.workspace.view", json!({ "files": name })));
        }
        x = rect.right();
    }
    if !all_files {
        return ask;
    }
    let check_rect = if one_line {
        Rect::from_min_size(pos2(row.right() - check_width, row.top()), vec2(check_width, 28.0))
    } else {
        Rect::from_min_size(pos2(row.left(), row.top() + 32.0), vec2(check_width, 28.0))
    };
    let response = ui.interact(check_rect, ui.scope_id().with("gw-all-files"), Sense::click());
    let square = Rect::from_center_size(pos2(check_rect.left() + 8.0, check_rect.center().y), Vec2::splat(14.0));
    if settings.all_files {
        ui.painter().rect_filled(square, 3, p.accent);
        glyph_check(ui.painter(), square.shrink(2.5), p.accent_text);
    } else {
        let border = if response.hovered() { p.text_secondary } else { p.text_field_border };
        ui.painter().rect_stroke(square, 3, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    }
    ui.painter().galley(pos2(square.right() + 8.0, check_rect.center().y - check.size().y / 2.0), check, p.text);
    if response.clicked() {
        ask = Some(Ask::Command("vcs.workspace.view", json!({ "allFiles": !settings.all_files })));
    }
    ask
}

#[derive(Clone, Debug, PartialEq)]
struct FileEntry<'a> {
    relative: &'a str,
    changed: Option<(&'a str, &'a std::path::Path)>,
}

fn commit_file_rows<'a>(
    files: &'a [zen_workbench::vcs::log::CommitFile],
    all: Option<&'a [String]>,
) -> Vec<FileEntry<'a>> {
    let changed = |file: &'a zen_workbench::vcs::log::CommitFile| FileEntry {
        relative: &file.relative,
        changed: Some((file.status.as_str(), file.path.as_path())),
    };
    let Some(all) = all else { return files.iter().map(changed).collect() };
    let by_path: HashMap<&str, &zen_workbench::vcs::log::CommitFile> =
        files.iter().map(|file| (file.relative.as_str(), file)).collect();
    let mut rows: Vec<FileEntry<'a>> = all
        .iter()
        .map(|path| match by_path.get(path.as_str()) {
            Some(file) => changed(file),
            None => FileEntry { relative: path, changed: None },
        })
        .collect();
    rows.extend(files.iter().filter(|file| file.status == "deleted").map(changed));
    rows.sort_by(|a, b| a.relative.cmp(b.relative));
    rows.dedup_by(|a, b| a.relative == b.relative);
    rows
}

enum FilePick {
    Open(std::path::PathBuf),
    Menu(std::path::PathBuf, egui::Pos2),
}

fn draw_commit_files(
    ui: &mut Ui,
    p: &IdePalette,
    rows: &[FileEntry<'_>],
    (view, root): (FilesView, &std::path::Path),
    collapsed: &mut BTreeSet<String>,
    mut menu: Option<(&std::path::Path, &mut Option<egui::Pos2>)>,
) -> Option<FilePick> {
    let mut clicked = None;
    let mut line = |ui: &mut Ui, entry: &FileEntry<'_>, indent: f32, folder: bool| {
        let response = file_line(ui, p, entry.relative, entry.changed.map(|(status, _)| status), (indent, folder, 0.0));
        let response = response.on_hover_text(entry.relative);
        let path = match entry.changed {
            Some((_, path)) if !path.as_os_str().is_empty() => path.to_path_buf(),
            _ => root.join(entry.relative),
        };
        if let Some((menu_for, at)) = menu.as_mut()
            && at.is_none()
            && *menu_for == path
        {
            **at = Some(pos2(response.rect.left() + 38.0, response.rect.bottom()));
        }
        if response.clicked() && entry.changed.is_some() {
            clicked = Some(FilePick::Open(path));
        } else if response.secondary_clicked() {
            clicked = Some(FilePick::Menu(path, response.interact_pointer_pos().unwrap_or(response.rect.center())));
        }
    };
    match view {
        FilesView::Path => {
            for entry in rows {
                line(ui, entry, 0.0, true);
            }
        }
        FilesView::Tree => {
            let names: Vec<&str> = rows.iter().map(|entry| entry.relative).collect();
            for row in look::tree_rows(&names, "files", collapsed, false) {
                match row {
                    TreeRow::Folder { key, name, depth, open, count } => {
                        folder_row(ui, p, collapsed, (&key, &name), 4.0 + depth as f32 * 14.0, open, count);
                    }
                    TreeRow::Leaf { index, depth, .. } => line(ui, &rows[index], depth as f32 * 14.0, false),
                }
            }
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use zen_workbench::vcs::BranchInfo;

    use super::*;

    #[test]
    fn every_toolbar_button_is_as_wide_and_pulls_menu_fits_inside_pull() {
        let captions = [20.0, 31.0, 44.0, 28.0];
        assert_eq!(buttons_width(&captions, false), tool_width(44.0, false));
        assert_eq!(buttons_width(&[], false), tool_width(0.0, false));
        for compact in [false, true] {
            let width = buttons_width(&captions, compact);
            let button = Rect::from_min_size(pos2(100.0, 4.0), vec2(width, TOOLBAR - 8.0));
            let arrow = pull_arrow(button);
            assert!(button.contains_rect(arrow), "{compact}: the ▾ is inside Pull");
            let icon = Rect::from_center_size(pos2(button.center().x, button.bottom() - 12.0), Vec2::splat(17.0));
            assert!(arrow.left() >= icon.right() - 2.0, "{compact}: the ▾ keeps clear of the icon");
        }
    }

    #[test]
    fn undo_and_redo_name_the_operation_in_their_hints() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let entry =
            |title: &str| zen_workbench::vcs::history::HistoryEntry { title: title.into(), ..Default::default() };
        let history = HistoryList { undo: Some(entry("Commit 'Fix login'")), ..Default::default() };
        let (name, shortcut) = history_hint("vcs.history.undo", &history);
        assert_eq!(name, "Undo Commit 'Fix login'");
        assert_eq!(shortcut, crate::keymap::shortcut("editor.edit.undo"));
        assert_eq!(
            history_hint("vcs.history.redo", &history),
            ("Redo".to_owned(), Some("No Git operation to redo".to_owned()))
        );
        let buttons = toolbar_buttons(true, true, &history);
        assert!(buttons[0].enabled, "something to undo");
        assert!(!buttons[1].enabled, "nothing to redo");
        assert!(!toolbar_buttons(true, true, &HistoryList::default())[0].enabled);

        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
        for (language, title) in [("en", "Commit 'Fix login'"), ("ru", "Коммит «Fix login»")] {
            zen_i18n::set_language(Some(language));
            let history = HistoryList { undo: Some(entry(title)), redo: None, ..Default::default() };
            let mut screen = crate::offscreen::Offscreen::new(vec2(420.0, 110.0), 2.0);
            let palette = IdePalette::zen_dark();
            palette.apply(screen.context());
            let buttons = toolbar_buttons(true, true, &history);
            let mut draw = |ui: &mut Ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.editor_background);
                let mut x = 12.0;
                for button in &buttons[..2] {
                    let caption = ui.painter().layout_no_wrap(t!(button.label), theme::regular(11.5), palette.text);
                    let width = tool_width(caption.size().x, false);
                    let rect = Rect::from_min_size(pos2(x, 4.0), vec2(width, TOOLBAR - 8.0));
                    tool_button(ui, &palette, rect, button, false, &history, PullMode::default());
                    x = rect.right();
                }
            };
            screen.render(&mut draw);
            let hover = vec![egui::Event::PointerMoved(pos2(40.0, 30.0))];
            let image = screen.frame(hover, &mut draw);
            let file = std::path::Path::new(&dir).join(format!("undo-hint-{language}.png"));
            crate::offscreen::write_png(&file, &image).unwrap();
        }
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn the_details_panel_never_covers_the_left_panel() {
        let tab = |width: f32| {
            let body = Rect::from_min_size(pos2(0.0, 58.0), vec2(width, 600.0));
            let left = (width * 0.19).clamp(190.0, 260.0);
            (body, Rect::from_min_max(pos2(left + 1.0, body.top()), body.max))
        };
        let (body, middle) = tab(1400.0);
        let (panel, _) = details_panel(body, middle, None);
        assert_eq!(panel.width(), 1400.0 * 0.27, "its own share of a wide tab");
        assert_eq!(details_panel(body, middle, Some(900.0)).0.width(), 900.0, "as wide as dragged");
        for width in [400.0, 520.0, 640.0, 760.0, 900.0, 1200.0] {
            let (body, middle) = tab(width);
            for dragged in [None, Some(280.0), Some(600.0), Some(1600.0)] {
                let (panel, shade) = details_panel(body, middle, dragged);
                assert!(panel.left() >= middle.left(), "{width} {dragged:?}: {panel:?} covers the left panel");
                assert!(shade.left() >= middle.left(), "{width} {dragged:?}: its shadow falls on the left panel");
                assert_eq!(panel.right(), body.right());
                assert!(shade.contains_rect(panel));
                assert!(panel.width() >= DETAILS_WIDTH.0.min(middle.width()), "{width} {dragged:?}");
            }
        }
    }

    #[test]
    fn initials_take_the_first_letters_of_two_words() {
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("grace"), "G");
        assert_eq!(initials("jean-luc picard"), "JL");
        assert_eq!(initials(""), "?");
        assert_eq!(initials("Анна Каренина"), "АК");
    }

    fn menu_frames(view: &mut GitWorkspaceView, keys: &[Option<egui::Key>]) -> Vec<(String, CommitAction)> {
        let ctx = egui::Context::default();
        let info = zen_workbench::vcs::RepositoryInfo { branch: Some("main".into()), ..Default::default() };
        let mut ran = Vec::new();
        for key in keys {
            let events = key
                .map(|key| {
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                })
                .unwrap_or_default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                let mut asks = Vec::new();
                commit_menu(ui, view, &info, &mut asks);
                ran.extend(asks.into_iter().filter_map(|ask| match ask {
                    Ask::Commit(id, action) => Some((id, action)),
                    _ => None,
                }));
            });
            output.textures_delta.clear();
        }
        ran
    }

    #[test]
    fn the_commit_menu_works_from_the_keyboard_when_a_command_opened_it() {
        let mut view = GitWorkspaceView {
            menu_for: Some("abc".into()),
            menu_at: Some(pos2(100.0, 100.0)),
            menu_keyboard: true,
            menu_info: Some(MenuInfo { on_current: true, detached_here: false }),
            ..GitWorkspaceView::default()
        };
        let down = Some(egui::Key::ArrowDown);
        let ran = menu_frames(&mut view, &[None, None, None, down, None, down, None, Some(egui::Key::Enter), None]);
        assert_eq!(ran, [("abc".to_owned(), CommitAction::Checkout)]);
        assert!(!view.menu_keyboard, "the keyboard was given once");
    }

    fn branch(name: &str, current: bool, upstream: Option<&str>, ahead: usize, behind: usize) -> BranchInfo {
        BranchInfo {
            name: name.to_owned(),
            current,
            upstream: upstream.map(str::to_owned),
            ahead,
            behind,
            ..BranchInfo::default()
        }
    }

    fn ref_menu_frames(view: &mut GitWorkspaceView, keys: &[Option<egui::Key>]) -> Vec<(RefTarget, RefAction)> {
        let ctx = egui::Context::default();
        let info = zen_workbench::vcs::RepositoryInfo { branch: Some("main".into()), ..Default::default() };
        let mut ran = Vec::new();
        for key in keys {
            let events = key
                .map(|key| {
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                })
                .unwrap_or_default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                let mut asks = Vec::new();
                ref_menu(ui, view, &info, &mut asks);
                ran.extend(asks.into_iter().filter_map(|ask| match ask {
                    Ask::Ref(target, action) => Some((target, action)),
                    _ => None,
                }));
            });
            output.textures_delta.clear();
        }
        ran
    }

    #[test]
    fn the_branch_menu_offers_what_applies_and_works_from_the_keyboard() {
        let branches = BranchList {
            current: Some("main".into()),
            local: vec![
                branch("main", true, Some("origin/main"), 0, 0),
                branch("release", false, Some("origin/main"), 0, 2),
            ],
            remote: vec![branch("origin/main", false, None, 0, 0), branch("origin/release", false, None, 0, 0)],
            ..BranchList::default()
        };
        let main = RefTarget { name: "main".into(), kind: RefKind::Local };
        let mut view = GitWorkspaceView {
            ref_menu_for: Some(vec![main.clone()]),
            ref_menu_at: Some(pos2(100.0, 100.0)),
            ref_menu_keyboard: true,
            branches,
            ..GitWorkspaceView::default()
        };
        let ran = ref_menu_frames(&mut view, &[None, None, None, Some(egui::Key::Enter), None]);
        assert_eq!(ran, [(main, RefAction::Push)]);
        assert!(!view.ref_menu_keyboard, "the keyboard was given once");

        let remote = RefTarget { name: "origin/main".into(), kind: RefKind::Remote };
        view.ref_menu_for = Some(vec![remote.clone()]);
        view.ref_menu_keyboard = true;
        let down = Some(egui::Key::ArrowDown);
        let keys = [None, None, None, down, None, down, None, down, None, down, None, Some(egui::Key::Enter), None];
        assert_eq!(ref_menu_frames(&mut view, &keys), [(remote.clone(), RefAction::FastForward)]);
        view.ref_menu_keyboard = true;
        let keys = [None, None, None, down, None, down, None, down, None, Some(egui::Key::Enter), None];
        assert_eq!(ref_menu_frames(&mut view, &keys), [(remote, RefAction::InteractiveRebase)]);
        assert_eq!(view.branches.tracking("origin/main").map(|branch| branch.name.as_str()), Some("release"));

        assert_eq!(default_upstream(&view.branches, "release").as_deref(), Some("origin/main"));
        view.branches.local.push(branch("topic", false, None, 0, 0));
        assert_eq!(default_upstream(&view.branches, "topic"), None);
        view.branches.remote.push(branch("upstream/topic", false, None, 0, 0));
        assert_eq!(default_upstream(&view.branches, "topic").as_deref(), Some("upstream/topic"));
    }

    #[test]
    fn the_stash_menu_works_from_the_keyboard_when_a_command_opened_it() {
        let ctx = egui::Context::default();
        let mut view = GitWorkspaceView {
            stash_menu_for: Some("5ta5h".into()),
            stash_menu_at: Some(pos2(100.0, 100.0)),
            stash_menu_keyboard: true,
            ..GitWorkspaceView::default()
        };
        let mut ran = Vec::new();
        let down = Some(egui::Key::ArrowDown);
        for key in [None, None, None, down, None, down, None, Some(egui::Key::Enter), None] {
            let events = key
                .map(|key| {
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                })
                .unwrap_or_default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                let mut asks = Vec::new();
                stash_menu(ui, &mut view, &mut asks);
                ran.extend(asks.into_iter().filter_map(|ask| match ask {
                    Ask::Stash(commit, action) => Some((commit, action)),
                    _ => None,
                }));
            });
            output.textures_delta.clear();
        }
        assert_eq!(ran, [(Some("5ta5h".to_owned()), StashAction::Rename)]);
        assert!(!view.stash_menu_keyboard, "the keyboard was given once");
    }

    fn row_answers(part: ChangePart, clicks: &[Vec2], quick: bool) -> Vec<WipRow> {
        zen_i18n::register(crate::toolbox::LOCALES);
        let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let ctx = egui::Context::default();
        let change = ChangeInfo {
            path: "/r/src/a.go".into(),
            relative: "src/a.go".into(),
            status: "modified".into(),
            unstaged_status: Some("modified".into()),
            ..ChangeInfo::default()
        };
        let mut origin = egui::Pos2::ZERO;
        let mut width = 0.0;
        let mut time = 0.0;
        let frame = |events: Vec<egui::Event>, origin: &mut egui::Pos2, width: &mut f32, time: f64| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 200.0))),
                time: Some(time),
                events,
                ..Default::default()
            };
            let mut answer = WipRow::None;
            let mut output = ctx.run_ui(input, |ui| {
                *origin = ui.cursor().min;
                *width = ui.available_width();
                answer = wip_file_row(ui, &palette, &change, "modified", part);
            });
            output.textures_delta.clear();
            answer
        };
        frame(Vec::new(), &mut origin, &mut width, time);
        let mut answers = Vec::new();
        for click in clicks {
            let at = pos2(origin.x + width + click.x, origin.y + click.y);
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            time += if quick { 0.05 } else { 0.5 };
            frame(vec![egui::Event::PointerMoved(at)], &mut origin, &mut width, time);
            time += 0.01;
            answers.push(frame(vec![button(true), button(false)], &mut origin, &mut width, time));
        }
        answers
    }

    #[test]
    fn a_file_row_stages_on_its_button_or_a_double_click_and_shows_the_diff_on_a_click() {
        let name = vec2(-250.0, 12.0);
        let (stage, discard) = (vec2(-22.0, 12.0), vec2(-48.0, 12.0));
        assert_eq!(row_answers(ChangePart::Unstaged, &[name], false), [WipRow::Clicked]);
        assert_eq!(
            row_answers(ChangePart::Unstaged, &[name, name], true),
            [WipRow::Clicked, WipRow::DoubleClicked],
            "the second click of a double click"
        );
        assert_eq!(
            row_answers(ChangePart::Unstaged, &[stage, discard], false),
            [WipRow::Button(WipButton::Stage), WipRow::Button(WipButton::Discard)]
        );
        assert_eq!(row_answers(ChangePart::Staged, &[stage], false), [WipRow::Button(WipButton::Unstage)]);
    }

    #[test]
    fn the_commit_form_joins_and_splits_messages() {
        assert_eq!(commit_message(" Fix the build ", ""), "Fix the build");
        assert_eq!(commit_message("Fix the build", "  Details.\nMore.  "), "Fix the build\n\nDetails.\nMore.");
        assert_eq!(message_fields("Fix\n\nDetails.\nMore."), ("Fix".to_owned(), "Details.\nMore.".to_owned()));
        assert_eq!(message_fields("Only a summary"), ("Only a summary".to_owned(), String::new()));
        assert!(header_fits(100.0, 80.0, 200.0));
        assert!(!header_fits(130.0, 80.0, 200.0), "the buttons become icons");
    }

    #[test]
    fn the_discard_dialog_names_what_goes() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let root = std::path::Path::new("/r");
        let params = |paths: &[&str], hunks: Vec<usize>, all: bool| IndexParams {
            paths: paths.iter().map(std::path::PathBuf::from).collect(),
            hunks,
            all,
            ..IndexParams::default()
        };
        let new = ChangeInfo {
            path: "/r/new.go".into(),
            relative: "new.go".into(),
            status: "unversioned".into(),
            unstaged_status: Some("unversioned".into()),
            ..ChangeInfo::default()
        };
        let changes = [new];
        let one = discard_text(&params(&["/r/src/a.go"], Vec::new(), false), root, &changes);
        assert!(one.contains("src/a.go") && !one.contains("/r/"), "{one}");
        let lines = discard_text(&params(&["/r/src/a.go"], vec![0], false), root, &changes);
        assert_ne!(lines, one, "some lines are not the whole file");
        let deleted = discard_text(&params(&["new.go"], Vec::new(), false), root, &changes);
        assert!(deleted.contains("new.go") && deleted != one.replace("src/a.go", "new.go"), "an unversioned file goes");
        assert!(discard_text(&params(&["/r/a", "/r/b"], Vec::new(), false), root, &changes).contains('2'));
        let all = discard_text(&params(&[], Vec::new(), true), root, &changes);
        assert!(!all.is_empty() && all != one);
    }

    fn target(name: &str, kind: RefKind) -> RefTarget {
        RefTarget { name: name.to_owned(), kind }
    }

    enum Do {
        Press(f32),
        Move(f32),
        Release(f32),
        Key(egui::Key),
        Nothing,
    }

    #[allow(clippy::type_complexity)]
    fn drag_frames(
        rows: &[RefTarget],
        actions: &[Do],
    ) -> (Vec<Option<(RefTarget, RefTarget)>>, Vec<Option<RefTarget>>, RefDrag) {
        zen_i18n::register(crate::toolbox::LOCALES);
        let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let ctx = egui::Context::default();
        let mut drag = RefDrag::default();
        let (mut drops, mut over) = (Vec::new(), Vec::new());
        let at = |row: f32| pos2(100.0, 12.0 + row * 24.0);
        let pointer = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let mut time = 0.0;
        for action in std::iter::once(&Do::Nothing).chain(actions) {
            let events = match action {
                Do::Press(row) => vec![egui::Event::PointerMoved(at(*row)), pointer(at(*row), true)],
                Do::Move(row) => vec![egui::Event::PointerMoved(at(*row))],
                Do::Release(row) => vec![egui::Event::PointerMoved(at(*row)), pointer(at(*row), false)],
                Do::Key(key) => vec![egui::Event::Key {
                    key: *key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                Do::Nothing => Vec::new(),
            };
            time += 0.1;
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 400.0))),
                time: Some(time),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                drag.over = None;
                let mut asks = Vec::new();
                let panel = Rect::from_min_size(pos2(0.0, 0.0), vec2(360.0, 400.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(panel));
                child.spacing_mut().item_spacing.y = 0.0;
                for row in rows {
                    let shown = RefRow {
                        name: &row.name,
                        kind: row.kind,
                        current: false,
                        selected: false,
                        badge: "",
                        indent: 0.0,
                        focus: search::RowFocus::default(),
                    };
                    ref_row(&mut child, &palette, &shown, &mut drag, row);
                }
                over.push(drag.over.clone());
                finish_drag(ui, &palette, &mut drag, &mut asks);
                drops.extend(asks.into_iter().filter_map(|ask| match ask {
                    Ask::DropMenu(drop) => Some(drop),
                    _ => None,
                }));
            });
            output.textures_delta.clear();
        }
        (drops, over, drag)
    }

    #[test]
    fn a_branch_dragged_onto_another_in_the_left_panel_asks_for_the_drop_menu() {
        let main = target("main", RefKind::Local);
        let feature = target("feature", RefKind::Local);
        let remote = target("origin/main", RefKind::Remote);
        let tag = target("v1", RefKind::Tag);
        let rows = [main.clone(), feature.clone(), remote.clone(), tag.clone()];

        let (drops, over, drag) =
            drag_frames(&rows, &[Do::Press(0.0), Do::Move(0.4), Do::Move(0.8), Do::Move(1.0), Do::Release(1.0)]);
        assert_eq!(drops, [Some((main.clone(), feature.clone()))]);
        assert_eq!(over.last().unwrap().as_ref(), Some(&feature), "highlighted as it is dropped");
        assert!(over[..3].iter().all(Option::is_none), "not over a branch yet: {over:?}");
        assert_eq!(drag.dropped_at, Some(pos2(100.0, 36.0)), "the menu opens where it was dropped");
        assert!(drag.source.is_none(), "the drag is over");

        let (drops, _, _) = drag_frames(&rows, &[Do::Press(0.0), Do::Move(0.4), Do::Move(0.2), Do::Release(0.2)]);
        assert!(drops.is_empty());
        let (drops, _, _) = drag_frames(&rows, &[Do::Press(2.0), Do::Move(2.6), Do::Move(3.0), Do::Release(3.0)]);
        assert!(drops.is_empty(), "a remote branch on a tag offers nothing");
        let (drops, _, _) = drag_frames(&rows, &[Do::Press(1.0), Do::Move(1.6), Do::Move(9.0), Do::Release(9.0)]);
        assert!(drops.is_empty(), "let go below the rows");

        let (drops, _, _) = drag_frames(&rows, &[Do::Press(3.0), Do::Move(2.0), Do::Move(1.0), Do::Release(1.0)]);
        assert_eq!(drops, [Some((tag, feature.clone()))]);
        let (drops, _, _) = drag_frames(&rows, &[Do::Press(1.0), Do::Move(1.6), Do::Move(2.0), Do::Release(2.0)]);
        assert_eq!(drops, [Some((feature, remote))]);

        let actions =
            [Do::Press(0.0), Do::Move(0.6), Do::Move(1.0), Do::Key(egui::Key::Escape), Do::Move(1.1), Do::Release(1.1)];
        let (drops, over, drag) = drag_frames(&rows, &actions);
        assert!(drops.is_empty());
        assert!(over[3].is_some(), "over feature before Escape");
        assert!(over[5..].iter().all(Option::is_none), "{over:?}");
        assert!(drag.source.is_none());
    }

    #[test]
    fn a_label_is_dragged_from_the_labels_column_onto_a_rows_branch() {
        let labels = vec![target("v2", RefKind::Tag), target("origin/x", RefKind::Remote), target("x", RefKind::Local)];
        assert_eq!(drop_label(&labels), Some(&labels[2]), "a drop goes to the local branch");
        assert_eq!(drop_label(&labels[..2]), Some(&labels[1]));
        assert_eq!(drop_label(&labels[..1]), Some(&labels[0]));
        assert_eq!(drop_label(&[]), None);

        let rows = [vec![target("main", RefKind::Local)], labels.clone()];
        let run = |press: egui::Pos2, to: egui::Pos2| {
            let ctx = egui::Context::default();
            let mut drag = RefDrag::default();
            let mut found = Vec::new();
            let pointer = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            let middle = pos2((press.x + to.x) / 2.0, (press.y + to.y) / 2.0);
            let frames = [
                vec![],
                vec![egui::Event::PointerMoved(press), pointer(press, true)],
                vec![egui::Event::PointerMoved(middle)],
                vec![egui::Event::PointerMoved(to)],
            ];
            for (index, events) in frames.into_iter().enumerate() {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 200.0))),
                    time: Some(index as f64 * 0.1),
                    events,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    drag.over = None;
                    for labels in &rows {
                        let (rect, response) = ui.allocate_exact_size(vec2(600.0, ROW), Sense::click_and_drag());
                        label_drag(ui, &mut drag, &response, (rect.left(), rect.left() + 180.0), labels);
                    }
                });
                output.textures_delta.clear();
                found = vec![drag.source.clone(), drag.over.clone()];
            }
            found
        };
        let dropped = run(pos2(60.0, 14.0), pos2(300.0, 42.0));
        assert_eq!(dropped, [Some(target("main", RefKind::Local)), Some(target("x", RefKind::Local))]);
        let dropped = run(pos2(60.0, 42.0), pos2(60.0, 14.0));
        assert_eq!(dropped, [Some(target("v2", RefKind::Tag)), Some(target("main", RefKind::Local))]);
        assert_eq!(run(pos2(300.0, 14.0), pos2(300.0, 42.0)), [None, None]);
    }

    #[test]
    fn the_drop_menu_offers_its_items_and_works_from_the_keyboard() {
        use zen_workbench::vcs::drag::DropItem;
        zen_i18n::register(crate::toolbox::LOCALES);
        let item = |action, enabled, checkout: Option<&str>| DropItem {
            action,
            enabled,
            checkout: checkout.map(str::to_owned),
        };
        let menu = DropMenuInfo {
            source: target("feature", RefKind::Local),
            target: target("main", RefKind::Local),
            items: vec![
                item(DropAction::Merge, false, None),
                item(DropAction::Rebase, true, Some("feature")),
                item(DropAction::InteractiveRebase, true, Some("feature")),
            ],
        };
        let mut view = GitWorkspaceView {
            drop_menu_for: Some(menu.clone()),
            drop_menu_at: Some(pos2(100.0, 100.0)),
            drop_menu_keyboard: true,
            ..GitWorkspaceView::default()
        };
        let ctx = egui::Context::default();
        let frames = |view: &mut GitWorkspaceView, keys: &[Option<egui::Key>]| {
            let mut ran = Vec::new();
            for key in keys {
                let events = key
                    .map(|key| {
                        vec![egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed: true,
                            repeat: false,
                            modifiers: egui::Modifiers::NONE,
                        }]
                    })
                    .unwrap_or_default();
                let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                    let mut asks = Vec::new();
                    drop_menu(ui, view, &mut asks);
                    ran.extend(asks.into_iter().filter_map(|ask| match ask {
                        Ask::Drop(source, target, action) => Some(Some((source.name, target.name, action))),
                        Ask::DropMenu(None) => Some(None),
                        _ => None,
                    }));
                });
                output.textures_delta.clear();
            }
            ran
        };
        let ran = frames(&mut view, &[None, None, None, Some(egui::Key::Enter), None]);
        assert_eq!(ran, [Some(("feature".to_owned(), "main".to_owned(), DropAction::Rebase)), None]);
        assert!(!view.drop_menu_keyboard, "the keyboard was given once");
        view.drop_menu_keyboard = true;
        let ran = frames(&mut view, &[None, None, Some(egui::Key::Escape), None]);
        assert_eq!(ran, [None]);
    }

    #[test]
    fn the_wip_line_goes_past_the_stashes_on_head() {
        let row = |id: &str, stash: Option<usize>, lane: usize| GraphCommit {
            id: id.to_owned(),
            stash,
            lane,
            ..GraphCommit::default()
        };
        let rows = [row("s0", Some(0), 1), row("s1", Some(1), 2), row("head", None, 0), row("old", None, 0)];
        let top = wip_head(&rows, Some("head"));
        assert_eq!((top.head.map(|head| head.id.as_str()), top.above), (Some("head"), 2));
        let top = wip_head(&rows[2..], Some("head"));
        assert_eq!((top.head.is_some(), top.above), (true, 0), "no stash above HEAD");
        let top = wip_head(&rows, Some("old"));
        assert!(top.head.is_none(), "HEAD is not the newest commit: the WIP row stands alone");
    }

    #[test]
    fn a_filter_opens_the_folded_sections_without_forgetting_their_folds() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let p = IdePalette::zen_dark();
        let frame = |collapsed: &mut BTreeSet<Section>, filtering: bool, events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 200.0))),
                events,
                ..Default::default()
            };
            let mut open = false;
            let mut output = ctx.run_ui(input, |ui| {
                open = section_header(ui, &p, collapsed, filtering, Section::Local, "Local", 1);
            });
            output.textures_delta.clear();
            open
        };
        let click = |collapsed: &mut BTreeSet<Section>, filtering: bool| {
            let at = pos2(120.0, 20.0);
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            frame(collapsed, filtering, vec![egui::Event::PointerMoved(at), button(true)]);
            frame(collapsed, filtering, vec![button(false)]);
            frame(collapsed, filtering, Vec::new())
        };
        let mut collapsed = BTreeSet::new();
        assert!(!frame(&mut collapsed, false, Vec::new()), "folded without a filter");
        assert!(frame(&mut collapsed, true, Vec::new()), "a filter opens a folded section");
        assert!(collapsed.is_empty(), "the filter does not change what is remembered");
        assert!(click(&mut collapsed, true));
        assert!(collapsed.is_empty(), "{collapsed:?}");
        assert!(!frame(&mut collapsed, false, Vec::new()));
        assert!(click(&mut collapsed, false), "a click opens it");
        assert_eq!(collapsed, BTreeSet::from([Section::Local]));
    }

    #[test]
    fn section_titles_fit_the_narrowest_left_panel_in_every_language() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let languages = [include_str!("../../locales/en/ui.ftl"), include_str!("../../locales/ru/ui.ftl")];
        let keys = [
            "vcs-workspace-local",
            "vcs-workspace-remote",
            "vcs-workspace-tags",
            "vcs-workspace-stashes",
            "vcs-workspace-worktrees",
            "vcs-workspace-submodules",
        ];
        let room = 190.0 - 50.0 - 14.0 - 16.0 - 6.0;
        let mut seen = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            for source in languages {
                for key in keys {
                    let title = source
                        .lines()
                        .find_map(|line| line.strip_prefix(key)?.trim_start().strip_prefix('='))
                        .unwrap_or_else(|| panic!("{key}"))
                        .trim();
                    let (galley, shortened) = look::small_caps_fitted(ui.painter(), title, 12.0, Color32::WHITE, room);
                    seen.push((title.to_owned(), shortened, galley.size().x));
                }
            }
            let (galley, shortened) =
                look::small_caps_fitted(ui.painter(), "Рабочие деревья проекта", 12.0, Color32::WHITE, room);
            let last = galley.rows.iter().flat_map(|row| row.glyphs.iter()).last().map(|glyph| glyph.chr);
            seen.push((String::new(), !(shortened && last == Some('…') && galley.size().x <= room), 0.0));
        });
        output.textures_delta.clear();
        let cut: Vec<_> = seen.iter().filter(|(_, shortened, _)| *shortened).collect();
        assert!(cut.is_empty(), "titles cut in {room} points: {cut:?}");
    }

    #[test]
    fn a_long_message_ends_in_an_ellipsis_inside_its_column() {
        let ctx = egui::Context::default();
        let mut seen = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            let long = "Merge branch 'feature/short-description' into main, with every review fix";
            for text in [long, "fix: keep it short"] {
                let galley =
                    painter.layout_job(super::super::vcs::truncated(text, theme::regular(13.0), Color32::WHITE, 182.0));
                let elided = cell(painter, (300.0, 500.0), 14.0, text, theme::regular(13.0), Color32::WHITE);
                let ends =
                    galley.rows.iter().flat_map(|row| row.glyphs.iter()).last().map(|glyph| glyph.chr) == Some('…');
                seen.push((elided, ends, galley.size().x <= 182.0));
            }
        });
        output.textures_delta.clear();
        assert_eq!(seen[0], (true, true, true), "a long message is shortened with \"…\" to its column");
        assert_eq!(seen[1], (false, false, true), "a short one is drawn whole");
    }

    #[test]
    fn the_commit_menu_offers_compare_when_two_points_are_picked() {
        let menu = |picked: &[&str], picked_wip: bool| GitWorkspaceView {
            menu_for: Some("abc".into()),
            menu_at: Some(pos2(100.0, 100.0)),
            menu_keyboard: true,
            menu_info: Some(MenuInfo { on_current: true, detached_here: false }),
            picked: picked.iter().map(|id| (*id).to_owned()).collect(),
            picked_wip,
            ..GitWorkspaceView::default()
        };
        let down = Some(egui::Key::ArrowDown);
        let keys = [None, None, None, down, None, down, None, down, None, down, None, Some(egui::Key::Enter), None];
        let ran = menu_frames(&mut menu(&["abc", "def"], false), &keys);
        assert_eq!(ran, [("abc".to_owned(), CommitAction::Compare)]);
        let keys = [None, None, None, down, None, down, None, down, None, Some(egui::Key::Enter), None];
        let ran = menu_frames(&mut menu(&["abc"], true), &keys);
        assert_eq!(ran, [("abc".to_owned(), CommitAction::Compare)]);
        let keys = [None, None, None, down, None, down, None, down, None, down, None, Some(egui::Key::Enter), None];
        let ran = menu_frames(&mut menu(&["abc", "def", "123"], false), &keys);
        assert_eq!(ran, [("abc".to_owned(), CommitAction::Revert)]);
    }

    #[test]
    fn a_right_click_on_a_file_of_the_panel_asks_for_its_menu() {
        let ctx = egui::Context::default();
        let files = vec![zen_workbench::vcs::log::CommitFile {
            path: "/repo/src/main.go".into(),
            relative: "src/main.go".into(),
            status: "modified".into(),
            old_path: None,
        }];
        let rows = commit_file_rows(&files, None);
        let mut picks = Vec::new();
        let mut collapsed = BTreeSet::new();
        let at = pos2(120.0, 13.0);
        for (index, events) in [
            vec![],
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ]
        .into_iter()
        .enumerate()
        {
            let input = egui::RawInput {
                events,
                time: Some(index as f64 * 0.05),
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 300.0))),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let root = std::path::Path::new("/repo");
                let p = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
                picks.push(draw_commit_files(ui, &p, &rows, (FilesView::Path, root), &mut collapsed, None));
            });
            output.textures_delta.clear();
        }
        let picks: Vec<String> = picks
            .into_iter()
            .flatten()
            .map(|pick| match pick {
                FilePick::Open(path) => format!("open {}", path.display()),
                FilePick::Menu(path, _) => format!("menu {}", path.display()),
            })
            .collect();
        assert_eq!(picks, ["menu /repo/src/main.go", "open /repo/src/main.go"]);
    }

    #[test]
    fn a_diagonal_swipe_over_the_graph_scrolls_the_rows_and_the_lanes_at_once() {
        #[derive(Default)]
        struct Scrolled {
            rows: f32,
            lanes: f32,
            graph: Option<Rect>,
        }
        let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 500.0), 1.0);
        let palette = IdePalette::zen_dark();
        let settings = zen_workbench::vcs::workspace_columns::ColumnSettings::default();
        let draw = |screen: &mut crate::offscreen::Offscreen, scrolled: &mut Scrolled, events: Vec<egui::Event>| {
            let fit = GraphFit::new(Some(120.0), 12, scrolled.lanes);
            assert!(fit.max_scroll > 0.0, "the lanes scroll sideways");
            let mut asks = Vec::new();
            screen.step(events, |ui| {
                let rows_rect = ui.available_rect_before_wrap();
                let area = egui::ScrollArea::vertical().id_salt("gw-rows").auto_shrink([false, false]);
                let output = area.show_rows(ui, ROW, 500, |ui, range| {
                    for _ in range {
                        ui.allocate_exact_size(vec2(rows_rect.width(), ROW), Sense::click());
                    }
                });
                scrolled.rows = output.state.clamped_offset().y;
                let columns = Columns::new(rows_rect, &settings, fit);
                scrolled.graph = Some(Rect::from_x_y_ranges(columns.graph.0..=columns.graph.1, rows_rect.y_range()));
                columns::graph_scroll(ui, &palette, rows_rect, &columns, &mut asks);
            });
            for ask in asks {
                if let Ask::Command("vcs.workspace.view", params) = ask {
                    scrolled.lanes = params["graphScroll"].as_f64().unwrap() as f32;
                }
            }
        };
        let wheel = |delta: Vec2, modifiers: egui::Modifiers| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers,
        };
        let mut scrolled = Scrolled::default();
        draw(&mut screen, &mut scrolled, Vec::new());
        let over = scrolled.graph.unwrap().center();
        draw(&mut screen, &mut scrolled, vec![egui::Event::PointerMoved(over)]);
        let swipe = wheel(vec2(-6.0, -5.0), egui::Modifiers::NONE);
        draw(&mut screen, &mut scrolled, vec![egui::Event::PointerMoved(over), swipe]);
        for _ in 0..5 {
            draw(&mut screen, &mut scrolled, Vec::new());
        }
        assert!(scrolled.rows > 0.0, "the rows moved down the history: {}", scrolled.rows);
        assert!(scrolled.lanes > 0.0, "the lanes moved sideways in the same gesture: {}", scrolled.lanes);
        let (rows, lanes) = (scrolled.rows, scrolled.lanes);
        let shifted = wheel(vec2(0.0, -2.0), egui::Modifiers::SHIFT);
        draw(&mut screen, &mut scrolled, vec![egui::Event::PointerMoved(over), shifted]);
        for _ in 0..5 {
            draw(&mut screen, &mut scrolled, Vec::new());
        }
        assert!(scrolled.lanes > lanes, "⇧ and the wheel scroll sideways: {}", scrolled.lanes);
        assert!((scrolled.rows - rows).abs() < 0.5, "the rows stay: {}", scrolled.rows);
    }
}

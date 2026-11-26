use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};

use super::log::CommitParams;
use super::workspace::{GitWorkspace, WorkspaceState, read, state};
use super::workspace_columns::{ColumnSettings, GraphColumn};
use super::{message, repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(SetView)?;
    registry.register(DetailsPanel)?;
    registry.register(CommitFiles)?;
    registry.register(PullMenu)?;
    registry.register(FoldSection)?;
    Ok(())
}

/// A section of the Git workspace's left panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PanelSection {
    /// LOCAL: the local branches.
    Local,
    /// REMOTE: the remote branches, by remote.
    Remote,
    /// TAGS.
    Tags,
    /// STASHES.
    Stashes,
    /// WORKTREES.
    Worktrees,
    /// SUBMODULES.
    Submodules,
}

/// A section to fold or unfold.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SectionParams {
    /// The section.
    pub section: PanelSection,
    /// Unfold it (`true`) or fold it (`false`); without it, the other way round from now, as a
    /// click on its title does.
    #[serde(default)]
    pub open: Option<bool>,
}

pub struct FoldSection;

impl Command for FoldSection {
    type Params = SectionParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.section";
    const TITLE: &'static str = "Fold Git Workspace Section";
    const DESCRIPTION: &'static str = "Unfolds (`open: true`) or folds (`open: false`) a section of the Git workspace's left \
         panel, as a click on its title does (without `open`, it turns it the other way round): local, \
         remote, tags, stashes, worktrees or submodules. Every section starts folded. Answers with the \
         tab's state, `openSections` the sections unfolded.";

    fn run(&self, session: &mut Session, params: SectionParams) -> Result<WorkspaceState, CommandError> {
        let sections = &mut session.state_mut::<GitWorkspace>().open_sections;
        let open = params.open.unwrap_or(!sections.contains(&params.section));
        if open {
            sections.insert(params.section);
        } else {
            sections.remove(&params.section);
        }
        Ok(state(session))
    }
}

pub const REFS_WIDTH: (f32, f32) = (60.0, 480.0);
pub const GRAPH_WIDTH: (f32, f32) = (40.0, 800.0);
pub const DETAILS_WIDTH: (f32, f32) = (280.0, 1600.0);
pub const LANE_WIDTH: f32 = 20.0;
pub const LANES_MARGIN: f32 = 16.0;
pub const NODE_RADIUS: f32 = 10.0;
pub const GATHERED_WIDTH: f32 = 24.0;
pub const GRAPH_AUTO_WIDTH: (f32, f32) = (60.0, 360.0);

/// How the graph fits its column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GraphStage {
    /// Every lane shows.
    #[default]
    Lanes,
    /// The column is narrower than the lanes: the lanes that fit show, and the nodes of the others
    /// gather into one line of dots at the column's right, each in its lane's colour.
    Gathered,
    /// The narrowest: only the line of dots, every node in it; the column's title is an icon.
    Dots,
}

/// How the graph fits its column, worked out from the column's width and the lanes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct GraphFit {
    /// The column's width in points: dragged, or following the lanes.
    pub width: f32,
    /// Whether the width was dragged (`graphWidth`); otherwise it follows the lanes.
    pub dragged: bool,
    /// How many lanes the widest row uses.
    pub lanes: usize,
    /// The first lane that shows: lanes scrolled out on the left come before it.
    pub first_lane: usize,
    /// How many lanes show from `firstLane`; the nodes of the others are in the line of dots.
    pub shown_lanes: usize,
    /// The stage this makes.
    pub stage: GraphStage,
    /// How far the lanes are scrolled sideways, in points (only while they are gathered: the
    /// column then has a horizontal scroll bar).
    pub scroll: f32,
    /// How far they can be scrolled sideways; 0 while every lane shows or only the dots do.
    pub max_scroll: f32,
}

impl GraphFit {
    pub fn new(dragged: Option<f32>, lanes: usize, scroll: f32) -> GraphFit {
        let natural = lanes_width(lanes);
        let width = dragged.unwrap_or_else(|| natural.clamp(GRAPH_AUTO_WIDTH.0, GRAPH_AUTO_WIDTH.1));
        let fit = GraphFit {
            width,
            dragged: dragged.is_some(),
            lanes,
            first_lane: 0,
            shown_lanes: lanes,
            stage: GraphStage::Lanes,
            scroll: 0.0,
            max_scroll: 0.0,
        };
        if width + 0.5 >= natural {
            return fit;
        }
        let room = width - GATHERED_WIDTH - LANES_MARGIN - NODE_RADIUS;
        let fitting = if room < 0.0 { 0 } else { (room / LANE_WIDTH).floor() as usize + 1 };
        if fitting < 2 {
            return GraphFit { first_lane: 0, shown_lanes: 0, stage: GraphStage::Dots, ..fit };
        }
        let max_scroll = (lanes.saturating_sub(fitting)) as f32 * LANE_WIDTH;
        let scroll = if scroll.is_finite() { scroll.clamp(0.0, max_scroll) } else { 0.0 };
        let first_lane = ((scroll - LANES_MARGIN + NODE_RADIUS) / LANE_WIDTH).ceil().max(0.0) as usize;
        let last_lane = (((room + scroll) / LANE_WIDTH).floor() as usize).min(lanes - 1);
        let shown_lanes = (last_lane + 1).saturating_sub(first_lane);
        GraphFit { first_lane, shown_lanes, stage: GraphStage::Gathered, scroll, max_scroll, ..fit }
    }

    pub fn shows(&self, lane: usize) -> bool {
        lane >= self.first_lane && lane < self.first_lane + self.shown_lanes
    }

    pub fn gathered_width(shown: usize) -> f32 {
        LANES_MARGIN + (shown.max(1) - 1) as f32 * LANE_WIDTH + NODE_RADIUS + GATHERED_WIDTH
    }
}

pub fn lanes_width(lanes: usize) -> f32 {
    lanes.max(1) as f32 * LANE_WIDTH + LANES_MARGIN
}

pub(super) fn graph_fit(session: &mut Session) -> GraphFit {
    let lanes = read(&vcs(session).graph.slot().graph()).lanes();
    let workspace = session.state_mut::<GitWorkspace>();
    let dragged = workspace.view.columns.widths.get(&GraphColumn::Graph).copied();
    GraphFit::new(dragged, lanes, workspace.graph_scroll)
}

/// How the right panel lists a commit's files.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FilesView {
    /// One row per file with its whole path, the folders dimmed.
    #[default]
    Path,
    /// The folders as a tree, the files under them.
    Tree,
}

/// How the workspace shows things.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceView {
    /// The graph's columns: their order, which are shown, their widths (vcs.workspaceColumns.get).
    pub columns: ColumnSettings,
    /// How a commit's files are listed.
    pub files: FilesView,
    /// Whether every file of the commit is listed, not only the changed ones.
    pub all_files: bool,
}

/// What to change in the view; what is left out stays as it is.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewParams {
    /// The width of the branch and tag column (60 to 480 points).
    pub refs_width: Option<f32>,
    /// The width of the graph column (40 to 800 points): narrower than its lanes, the nodes of
    /// the lanes that do not fit gather into one line of dots; at the narrowest only that line
    /// shows.
    pub graph_width: Option<f32>,
    /// Fits the graph column to a stage, after `graphWidth`: `lanes` makes it follow its lanes
    /// again (every lane shows), `gathered` narrows it to half of its lanes with the others' nodes
    /// in the line of dots, `dots` narrows it to only the line of dots.
    pub graph_stage: Option<GraphStage>,
    /// Scrolls the lanes sideways to this many points from the left, as the graph column's
    /// horizontal scroll bar, a sideways swipe or ⇧ and the wheel do while lanes do not fit (kept
    /// within `maxScroll` of `graph`).
    pub graph_scroll: Option<f32>,
    /// The width of the details panel floating over the graph, as dragging its left edge sets it
    /// (280 points at least; the window keeps it within 70 % of the tab). Kept per repository with
    /// the columns (`detailsWidth` of `columns`).
    pub details_width: Option<f32>,
    /// Give every column its own width again (before `refsWidth` and `graphWidth` apply), as
    /// `resetWidths` of vcs.workspaceColumns.set does.
    pub reset_columns: bool,
    /// List a commit's files by `path` or as a `tree`.
    pub files: Option<FilesView>,
    /// List every file of the commit (true) or only the changed ones (false).
    pub all_files: Option<bool>,
}

/// The view, with how the graph fits its column.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewState {
    /// The view.
    #[serde(flatten)]
    pub view: WorkspaceView,
    /// How the graph fits its column: its width, the lanes shown and the stage.
    pub graph: GraphFit,
}

pub struct SetView;

impl Command for SetView {
    type Params = ViewParams;
    type Output = ViewState;
    const ID: &'static str = "vcs.workspace.view";
    const TITLE: &'static str = "Git Workspace View";
    const DESCRIPTION: &'static str = "Sets how the Git workspace shows things, as dragging the column borders and the right \
         panel's Path/Tree and View all files do: `refsWidth` and `graphWidth` narrow or widen the \
         branch and graph columns (`resetColumns` gives every column its own width back; \
         vcs.workspaceColumns.set arranges all the columns); a graph column narrower than its lanes \
         gathers the nodes of the lanes that do not fit into one line of dots coloured by their \
         lane, and at the narrowest shows only that line (`graphStage` `lanes`, `gathered` or `dots` \
         fits it to a stage; `graph` in the answer says the stage, the width and the lanes shown; \
         `graphScroll` scrolls lanes that do not fit sideways, as the column's horizontal scroll bar \
         does); `detailsWidth` widens or narrows the floating details panel as dragging its left edge \
         does (`columns.detailsWidth`, kept per repository); `files` lists a commit's files by `path` or as a `tree`, `allFiles` lists every file of the \
         commit instead of the changed ones (vcs.commit.files reads them). Leave everything out to \
         read the view. Answers with the view.";

    fn run(&self, session: &mut Session, params: ViewParams) -> Result<ViewState, CommandError> {
        let finite = |width: Option<f32>, name: &str| match width {
            Some(width) if !width.is_finite() => Err(CommandError::new(format!("{name} must be a number of points"))),
            _ => Ok(width),
        };
        let refs = finite(params.refs_width, "refsWidth")?;
        let graph = finite(params.graph_width, "graphWidth")?;
        let view = &mut session.state_mut::<GitWorkspace>().view;
        let widths = &mut view.columns.widths;
        if params.reset_columns {
            widths.clear();
        }
        if let Some(width) = refs {
            widths.insert(GraphColumn::Refs, width.clamp(REFS_WIDTH.0, REFS_WIDTH.1));
        }
        if let Some(width) = graph {
            widths.insert(GraphColumn::Graph, width.clamp(GRAPH_WIDTH.0, GRAPH_WIDTH.1));
        }
        if let Some(files) = params.files {
            view.files = files;
        }
        if let Some(all) = params.all_files {
            view.all_files = all;
        }
        if let Some(stage) = params.graph_stage {
            let lanes = read(&vcs(session).graph.slot().graph()).lanes();
            let widths = &mut session.state_mut::<GitWorkspace>().view.columns.widths;
            match stage {
                GraphStage::Lanes => {
                    widths.remove(&GraphColumn::Graph);
                }
                GraphStage::Gathered if lanes < 3 => {
                    return Err(CommandError::new(format!(
                        "a graph of {lanes} lanes has too few to gather: it shows its lanes or only its dots"
                    )));
                }
                GraphStage::Gathered => {
                    let width = GraphFit::gathered_width(lanes.div_ceil(2)).clamp(GRAPH_WIDTH.0, GRAPH_WIDTH.1);
                    widths.insert(GraphColumn::Graph, width);
                }
                GraphStage::Dots => {
                    widths.insert(GraphColumn::Graph, GRAPH_WIDTH.0);
                }
            }
        }
        if let Some(width) = finite(params.details_width, "detailsWidth")? {
            session.state_mut::<GitWorkspace>().view.columns.details_width =
                Some(width.clamp(DETAILS_WIDTH.0, DETAILS_WIDTH.1));
        }
        if let Some(scroll) = finite(params.graph_scroll, "graphScroll")? {
            session.state_mut::<GitWorkspace>().graph_scroll = scroll;
        }
        let fit = graph_fit(session);
        let workspace = session.state_mut::<GitWorkspace>();
        if fit.lanes > 0 {
            workspace.graph_scroll = fit.scroll;
        }
        Ok(ViewState { view: workspace.view.clone(), graph: fit })
    }
}

/// Whether the details panel shows.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DetailsParams {
    /// Open it (`true`) or close it (`false`); without it, the other way round from now.
    pub open: Option<bool>,
}

pub struct DetailsPanel;

impl Command for DetailsPanel {
    type Params = DetailsParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.details";
    const TITLE: &'static str = "Git Workspace Details Panel";
    const DESCRIPTION: &'static str = "Opens (`open: true`) or closes (`open: false`) the Git workspace's details panel, which \
         floats over the graph from the right and shows the selected commit, the work in progress, a \
         comparison or a file's history; without `open`, turns it the other way round. Selecting a \
         commit or the work in progress opens it; its ×, Escape and a click on the graph's empty space \
         close it, and the selection stays. It opens only when there is something to show. Answers \
         with the tab's state, `details` true while it is open.";

    fn run(&self, session: &mut Session, params: DetailsParams) -> Result<WorkspaceState, CommandError> {
        let workspace = session.state_mut::<GitWorkspace>();
        let open = params.open.unwrap_or(!workspace.details_open());
        if open && !workspace.has_details() {
            return Err(CommandError::new(
                "the details panel has nothing to show: select a commit or the work in progress first",
            ));
        }
        workspace.details = open;
        Ok(state(session))
    }
}

/// Whether to open or close a menu.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MenuOpenParams {
    /// Open it (the default) or close it.
    pub open: bool,
}

impl Default for MenuOpenParams {
    fn default() -> Self {
        MenuOpenParams { open: true }
    }
}

pub struct PullMenu;

impl Command for PullMenu {
    type Params = MenuOpenParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.pullMenu";
    const TITLE: &'static str = "Pull Menu";
    const DESCRIPTION: &'static str = "Opens (or with `open: false` closes) the menu of the ▾ beside Pull on the Git workspace's \
         toolbar, as a click on it does. Its items are vcs.workspace.pull with each `mode`: Fetch All, \
         Pull (fast-forward if possible), Pull (fast-forward only) and Pull (rebase); the one Pull \
         itself runs is marked, and vcs.workspace.pullDefault chooses it. Answers with the tab's \
         state, `pullMenu` true while it is open.";

    fn run(&self, session: &mut Session, params: MenuOpenParams) -> Result<WorkspaceState, CommandError> {
        repository(session)?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        workspace.pull_menu = params.open;
        Ok(state(session))
    }
}

/// Every file of a commit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitFileList {
    /// The commit's full id.
    pub id: String,
    /// Every file of its tree, by path relative to the repository's root, sorted.
    pub files: Vec<String>,
}

pub struct CommitFiles;

impl Command for CommitFiles {
    type Params = CommitParams;
    type Output = CommitFileList;
    const ID: &'static str = "vcs.commit.files";
    const TITLE: &'static str = "Commit Files";
    const DESCRIPTION: &'static str = "Every file of a commit's tree (not only the files it changed, which vcs.commit.details lists), by \
         path relative to the repository's root, sorted: what the Git workspace's View all files \
         shows.";

    fn run(&self, session: &mut Session, params: CommitParams) -> Result<CommitFileList, CommandError> {
        let repository = repository(session)?;
        let id = repository.resolve(&params.id).map_err(|error| CommandError::new(message(&error)))?;
        let files = repository.commit_files(&id).map_err(|error| CommandError::new(message(&error)))?;
        Ok(CommitFileList { id, files })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    fn session(dir: &std::path::Path) -> (Registry, Session) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir })).unwrap();
        (registry, session)
    }

    #[test]
    fn the_view_keeps_its_column_widths_within_bounds_and_its_file_list() {
        let dir = test_repository();
        let (registry, mut session) = session(dir.path());
        let mut view = |params: serde_json::Value| {
            serde_json::from_value::<WorkspaceView>(
                registry.dispatch(&mut session, "vcs.workspace.view", params).unwrap(),
            )
            .unwrap()
        };
        assert_eq!(view(json!({})), WorkspaceView::default());
        let set = view(json!({ "refsWidth": 10.0, "graphWidth": 150.0, "files": "tree", "allFiles": true }));
        let width = |view: &WorkspaceView, column| view.columns.widths.get(&column).copied();
        assert_eq!(
            (width(&set, GraphColumn::Refs), width(&set, GraphColumn::Graph), set.files, set.all_files),
            (Some(REFS_WIDTH.0), Some(150.0), FilesView::Tree, true)
        );
        assert_eq!(width(&view(json!({ "refsWidth": 9000 })), GraphColumn::Refs), Some(REFS_WIDTH.1));
        let reset = view(json!({ "resetColumns": true }));
        assert_eq!((reset.columns.widths.len(), reset.files, reset.all_files), (0, FilesView::Tree, true));
        assert_eq!(width(&view(json!({ "resetColumns": true, "graphWidth": 90 })), GraphColumn::Graph), Some(90.0));
        assert!(registry.dispatch(&mut session, "vcs.workspace.view", json!({ "files": "grid" })).is_err());
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["view"]["columns"]["widths"]["graph"], json!(90.0));
        assert_eq!(state["view"]["files"], json!("tree"));
    }

    #[test]
    fn a_column_narrower_than_its_lanes_gathers_their_nodes_and_at_the_narrowest_shows_only_dots() {
        let lanes = GraphFit::new(None, 8, 0.0);
        assert_eq!((lanes.width, lanes.stage, lanes.shown_lanes, lanes.dragged), (176.0, GraphStage::Lanes, 8, false));
        assert_eq!(GraphFit::new(Some(200.0), 8, 0.0).stage, GraphStage::Lanes, "wider than the lanes");
        let gathered = GraphFit::new(Some(120.0), 8, 0.0);
        assert_eq!((gathered.stage, gathered.first_lane, gathered.shown_lanes), (GraphStage::Gathered, 0, 4));
        assert!(gathered.shows(3) && !gathered.shows(4));
        assert_eq!(gathered.max_scroll, 4.0 * LANE_WIDTH);
        let scrolled = GraphFit::new(Some(120.0), 8, 40.0);
        assert_eq!((scrolled.first_lane, scrolled.shown_lanes), (2, 4));
        assert!(!scrolled.shows(1) && scrolled.shows(5) && !scrolled.shows(6));
        let end = GraphFit::new(Some(120.0), 8, 9000.0);
        assert_eq!((end.scroll, end.first_lane + end.shown_lanes), (end.max_scroll, 8), "the last lane shows");
        assert_eq!(GraphFit::new(Some(120.0), 8, f32::NAN).scroll, 0.0);
        for width in [GRAPH_WIDTH.0, 60.0, 69.0] {
            let dots = GraphFit::new(Some(width), 8, 30.0);
            assert_eq!((dots.stage, dots.shown_lanes, dots.max_scroll, dots.scroll), (GraphStage::Dots, 0, 0.0, 0.0));
        }
        assert_eq!(GraphFit::new(Some(70.0), 8, 0.0).stage, GraphStage::Gathered);
        let many = GraphFit::new(None, 40, 0.0);
        assert_eq!((many.width, many.stage), (GRAPH_AUTO_WIDTH.1, GraphStage::Gathered));
        assert_eq!(GraphFit::new(None, 1, 0.0).width, GRAPH_AUTO_WIDTH.0);
        assert_eq!(GraphFit::new(Some(GraphFit::gathered_width(3)), 8, 0.0).shown_lanes, 3);
    }

    fn four_lanes() -> tempfile::TempDir {
        let dir = test_repository();
        for name in ["b1", "b2", "b3"] {
            git(dir.path(), &["checkout", "-q", "-b", name, "main"]);
            std::fs::write(dir.path().join(format!("{name}.txt")), name).unwrap();
            git(dir.path(), &["add", "."]);
            git(dir.path(), &["commit", "-q", "-m", name]);
        }
        git(dir.path(), &["checkout", "-q", "main"]);
        std::fs::write(dir.path().join("m.txt"), "m").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "m"]);
        dir
    }

    #[test]
    fn the_graph_is_fitted_to_a_stage_and_scrolled_sideways_through_the_view() {
        let dir = four_lanes();
        let (registry, mut session) = session(dir.path());
        let page = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        assert_eq!(page["lanes"], json!(4));
        let mut view = |params: serde_json::Value| {
            serde_json::from_value::<ViewState>(registry.dispatch(&mut session, "vcs.workspace.view", params).unwrap())
                .unwrap()
        };
        assert_eq!(view(json!({})).graph.stage, GraphStage::Lanes);
        let gathered = view(json!({ "graphStage": "gathered" }));
        assert_eq!((gathered.graph.stage, gathered.graph.shown_lanes), (GraphStage::Gathered, 2));
        assert_eq!(gathered.view.columns.widths[&GraphColumn::Graph], GraphFit::gathered_width(2));
        let scrolled = view(json!({ "graphScroll": 20 }));
        assert_eq!((scrolled.graph.scroll, scrolled.graph.first_lane), (20.0, 1));
        assert_eq!(view(json!({ "graphScroll": 5000 })).graph.scroll, scrolled.graph.max_scroll);
        let dots = view(json!({ "graphStage": "dots" }));
        assert_eq!((dots.graph.stage, dots.graph.width), (GraphStage::Dots, GRAPH_WIDTH.0));
        let lanes = view(json!({ "graphStage": "lanes" }));
        assert_eq!((lanes.graph.stage, lanes.graph.dragged), (GraphStage::Lanes, false));
        assert_eq!(view(json!({ "graphWidth": 75 })).graph.stage, GraphStage::Gathered);
        assert_eq!(view(json!({ "graphWidth": 500 })).graph.stage, GraphStage::Lanes);
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["graph"]["stage"], json!("lanes"));
        assert!(registry.dispatch(&mut session, "vcs.workspace.view", json!({ "graphStage": "folded" })).is_err());
        let single = test_repository();
        let (registry, mut session) = self::session(single.path());
        registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        assert!(registry.dispatch(&mut session, "vcs.workspace.view", json!({ "graphStage": "gathered" })).is_err());
    }

    #[test]
    fn the_details_panel_opens_on_a_selection_and_closes_keeping_it() {
        let dir = test_repository();
        let (registry, mut session) = session(dir.path());
        registry.dispatch(&mut session, "vcs.workspace.open", json!({})).unwrap();
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["details"], json!(false), "nothing to show at first");
        assert!(registry.dispatch(&mut session, "vcs.workspace.details", json!({ "open": true })).is_err());
        registry.dispatch(&mut session, "vcs.workspace.select", json!({ "revision": "HEAD" })).unwrap();
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["details"], json!(true), "a selection opens it");
        let closed = registry.dispatch(&mut session, "vcs.workspace.details", json!({ "open": false })).unwrap();
        assert_eq!(closed["details"], json!(false));
        assert!(closed["selected"].is_string(), "the selection stays");
        let toggled = registry.dispatch(&mut session, "vcs.workspace.details", json!({})).unwrap();
        assert_eq!(toggled["details"], json!(true));
        registry.dispatch(&mut session, "vcs.workspace.details", json!({ "open": false })).unwrap();
        let wip = registry.dispatch(&mut session, "vcs.workspace.select", json!({ "wip": true }));
        assert!(wip.is_ok());
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!((state["details"].as_bool(), state["wip"].as_bool()), (Some(true), Some(true)));
        let mut width = |width: f32| {
            let view = registry.dispatch(&mut session, "vcs.workspace.view", json!({ "detailsWidth": width })).unwrap();
            view["columns"]["detailsWidth"].as_f64()
        };
        assert_eq!(width(100.0), Some(f64::from(DETAILS_WIDTH.0)));
        assert_eq!(width(9000.0), Some(f64::from(DETAILS_WIDTH.1)));
        assert_eq!(width(420.0), Some(420.0));
        let columns = registry.dispatch(&mut session, "vcs.workspaceColumns.get", json!({})).unwrap();
        assert_eq!(columns["detailsWidth"], json!(420.0));
    }

    #[test]
    fn the_pull_menu_opens_and_closes_and_gives_way_to_another_menu() {
        let dir = test_repository();
        let (registry, mut session) = session(dir.path());
        let opened = registry.dispatch(&mut session, "vcs.workspace.pullMenu", json!({})).unwrap();
        assert_eq!(opened["pullMenu"], json!(true));
        let closed = registry.dispatch(&mut session, "vcs.workspace.pullMenu", json!({ "open": false })).unwrap();
        assert_eq!(closed["pullMenu"], json!(false));
        registry.dispatch(&mut session, "vcs.workspace.pullMenu", json!({ "open": true })).unwrap();
        let other = registry.dispatch(&mut session, "vcs.workspace.menu", json!({ "revision": "HEAD" })).unwrap();
        assert_eq!(other["pullMenu"], json!(false));
        assert!(other["menu"].is_string());
    }

    #[test]
    fn every_file_of_a_commit_is_read_not_only_the_changed_ones() {
        let dir = test_repository();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.go"), "package main\n").unwrap();
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "Second"]);
        let (registry, mut session) = session(dir.path());
        let list: CommitFileList = serde_json::from_value(
            registry.dispatch(&mut session, "vcs.commit.files", json!({ "id": "HEAD" })).unwrap(),
        )
        .unwrap();
        assert_eq!(list.files, ["a.txt", "src/main.go"]);
        assert_eq!(list.id, git(dir.path(), &["rev-parse", "HEAD"]).trim());
        let first: CommitFileList = serde_json::from_value(
            registry.dispatch(&mut session, "vcs.commit.files", json!({ "id": "HEAD~1" })).unwrap(),
        )
        .unwrap();
        assert_eq!(first.files, ["a.txt"]);
        assert!(registry.dispatch(&mut session, "vcs.commit.files", json!({ "id": "nowhere" })).is_err());
    }
}

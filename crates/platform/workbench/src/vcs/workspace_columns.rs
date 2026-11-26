use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

use super::workspace::{GitWorkspace, RefTarget, WorkspaceState, fresh_graph, read, state};
use super::workspace_view::MenuOpenParams;
use super::{message, repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetColumns)?;
    registry.register(SetColumns)?;
    registry.register(ColumnsMenu)?;
    registry.register(HoverCommand)?;
    Ok(())
}

/// A column of the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum GraphColumn {
    /// BRANCH / TAG: the labels of the branches and tags at each commit.
    Refs,
    /// GRAPH: the lanes and the nodes.
    Graph,
    /// COMMIT MESSAGE: the subject; it takes the room the others leave.
    Message,
    /// AUTHOR.
    Author,
    /// DATE / TIME.
    Date,
    /// SHA: the short id.
    Sha,
}

impl GraphColumn {
    pub const ALL: [GraphColumn; 6] = [
        GraphColumn::Refs,
        GraphColumn::Graph,
        GraphColumn::Message,
        GraphColumn::Author,
        GraphColumn::Date,
        GraphColumn::Sha,
    ];

    pub fn optional(self) -> bool {
        matches!(self, GraphColumn::Author | GraphColumn::Date | GraphColumn::Sha)
    }

    pub fn bounds(self) -> Option<(f32, f32)> {
        match self {
            GraphColumn::Refs => Some(super::workspace_view::REFS_WIDTH),
            GraphColumn::Graph => Some(super::workspace_view::GRAPH_WIDTH),
            GraphColumn::Message => None,
            GraphColumn::Author => Some((60.0, 400.0)),
            GraphColumn::Date => Some((70.0, 300.0)),
            GraphColumn::Sha => Some((50.0, 200.0)),
        }
    }

    pub fn default_width(self) -> Option<f32> {
        match self {
            GraphColumn::Author => Some(140.0),
            GraphColumn::Date => Some(128.0),
            GraphColumn::Sha => Some(70.0),
            GraphColumn::Refs | GraphColumn::Graph | GraphColumn::Message => None,
        }
    }
}

/// How the graph's columns are arranged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ColumnSettings {
    /// The columns from left to right, each once.
    pub order: Vec<GraphColumn>,
    /// The columns switched off: some of `author`, `date` and `sha`.
    pub hidden: Vec<GraphColumn>,
    /// The widths dragged, in points; a column left out has its own width (the branch and graph
    /// columns follow the tab and the lanes).
    pub widths: BTreeMap<GraphColumn, f32>,
    /// Whether hovering a branch highlights the commits of its history and dims the others.
    pub highlight_on_hover: bool,
    /// The width of the details panel floating over the graph, as its left edge was dragged, in
    /// points; `None` while it has its own share of the tab.
    pub details_width: Option<f32>,
}

impl Default for ColumnSettings {
    fn default() -> Self {
        ColumnSettings {
            order: GraphColumn::ALL.to_vec(),
            hidden: Vec::new(),
            widths: BTreeMap::new(),
            highlight_on_hover: true,
            details_width: None,
        }
    }
}

impl Eq for ColumnSettings {}

impl ColumnSettings {
    pub fn normalized(mut self) -> Self {
        let mut order = Vec::with_capacity(GraphColumn::ALL.len());
        for column in self.order.into_iter().chain(GraphColumn::ALL) {
            if !order.contains(&column) {
                order.push(column);
            }
        }
        self.order = order;
        self.hidden.retain(|column| column.optional());
        self.hidden.sort();
        self.hidden.dedup();
        self.widths = self
            .widths
            .into_iter()
            .filter_map(|(column, width)| {
                let (low, high) = column.bounds()?;
                width.is_finite().then(|| (column, width.clamp(low, high)))
            })
            .collect();
        self.details_width = self
            .details_width
            .filter(|width| width.is_finite())
            .map(|width| width.clamp(super::workspace_view::DETAILS_WIDTH.0, super::workspace_view::DETAILS_WIDTH.1));
        self
    }

    pub fn shown(&self, column: GraphColumn) -> bool {
        !self.hidden.contains(&column)
    }

    pub fn width(&self, column: GraphColumn) -> Option<f32> {
        self.widths.get(&column).copied().or_else(|| column.default_width())
    }

    pub fn is_default(&self) -> bool {
        *self == ColumnSettings::default()
    }
}

pub struct GetColumns;

impl Command for GetColumns {
    type Params = NoParams;
    type Output = ColumnSettings;
    const ID: &'static str = "vcs.workspaceColumns.get";
    const TITLE: &'static str = "Git Workspace Columns";
    const DESCRIPTION: &'static str = "How the Git workspace's graph arranges its columns (refs, graph, message, author, date, \
         sha): `order` from left to right, `hidden` the ones switched off (author, date and sha can \
         be), `widths` the ones dragged (in points), and `highlightOnHover`, whether hovering a branch \
         highlights its commits and dims the others. The window keeps them per repository.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ColumnSettings, CommandError> {
        Ok(session.state_mut::<GitWorkspace>().view.columns.clone())
    }
}

/// What to change in the columns; what is left out stays as it is.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ColumnsParams {
    /// Every setting at once (as vcs.workspaceColumns.get gives them), before the rest applies.
    pub set: Option<ColumnSettings>,
    /// Back to the columns as they start: every column, in the usual order, with its own width.
    pub reset: bool,
    /// The columns from left to right; those left out follow, in the order they had.
    pub order: Option<Vec<GraphColumn>>,
    /// Columns to switch on.
    pub show: Vec<GraphColumn>,
    /// Columns to switch off: `author`, `date` or `sha`.
    pub hide: Vec<GraphColumn>,
    /// Widths to set, in points, by column (not `message`, which takes the room left); each kept
    /// within its bounds.
    pub widths: BTreeMap<GraphColumn, f32>,
    /// Give every column its own width again (before `widths` apply).
    pub reset_widths: bool,
    /// Whether hovering a branch highlights its commits and dims the others.
    pub highlight_on_hover: Option<bool>,
}

pub struct SetColumns;

impl Command for SetColumns {
    type Params = ColumnsParams;
    type Output = ColumnSettings;
    const ID: &'static str = "vcs.workspaceColumns.set";
    const TITLE: &'static str = "Arrange Git Workspace Columns";
    const DESCRIPTION: &'static str = "Arranges the Git workspace graph's columns, as dragging their titles and borders and the \
         titles' menu do: `order` puts them in an order (refs, graph, message, author, date, sha; \
         those left out follow), `show` and `hide` switch author, date and sha on and off, `widths` \
         sets widths in points (`resetWidths` gives every column its own width again), \
         `highlightOnHover` turns on or off the highlighting of a hovered branch's commits; `reset` \
         goes back to the start and `set` replaces everything. Answers with the columns; the window \
         keeps them per repository.";

    fn run(&self, session: &mut Session, params: ColumnsParams) -> Result<ColumnSettings, CommandError> {
        if let Some(column) = params.hide.iter().find(|column| !column.optional()) {
            return Err(CommandError::new(format!(
                "only author, date and sha can be switched off, not {}",
                serde_json::to_string(column).unwrap_or_default()
            )));
        }
        if params.widths.contains_key(&GraphColumn::Message) {
            return Err(CommandError::new("the message column takes the room the others leave: it has no width"));
        }
        if params.widths.values().any(|width| !width.is_finite()) {
            return Err(CommandError::new("a width must be a number of points"));
        }
        let columns = &mut session.state_mut::<GitWorkspace>().view.columns;
        let mut next = if params.reset { ColumnSettings::default() } else { columns.clone() };
        if let Some(set) = params.set {
            next = set;
        }
        if let Some(order) = params.order {
            let rest: Vec<GraphColumn> = next.order.iter().copied().filter(|column| !order.contains(column)).collect();
            next.order = order.into_iter().chain(rest).collect();
        }
        next.hidden.retain(|column| !params.show.contains(column));
        next.hidden.extend(params.hide);
        if params.reset_widths {
            next.widths.clear();
        }
        next.widths.extend(params.widths);
        if let Some(on) = params.highlight_on_hover {
            next.highlight_on_hover = on;
        }
        *columns = next.normalized();
        Ok(columns.clone())
    }
}

pub struct ColumnsMenu;

impl Command for ColumnsMenu {
    type Params = MenuOpenParams;
    type Output = WorkspaceState;
    const ID: &'static str = "vcs.workspace.columnsMenu";
    const TITLE: &'static str = "Git Workspace Columns Menu";
    const DESCRIPTION: &'static str = "Opens (or with `open: false` closes) the menu of the Git workspace graph's column titles, \
         as a right click on them does: Author, Date and SHA on or off, Highlight Branch on Hover, \
         Reset Columns, each vcs.workspaceColumns.set. Answers with the tab's state, `columnsMenu` \
         true while it is open.";

    fn run(&self, session: &mut Session, params: MenuOpenParams) -> Result<WorkspaceState, CommandError> {
        repository(session)?;
        let workspace = session.state_mut::<GitWorkspace>();
        workspace.close_menu();
        workspace.columns_menu = params.open;
        Ok(state(session))
    }
}

/// What the pointer is over in the graph: a branch or a tag, or a commit.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HoverParams {
    /// A branch or a tag under the pointer: its history is highlighted.
    pub branch: Option<RefTarget>,
    /// A commit under the pointer (its id, a branch, `HEAD~2`...): the branch nearest to it is
    /// shown. Leave both out when the pointer leaves.
    pub revision: Option<String>,
}

/// What hovering shows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct HoverInfo {
    /// The branch or tag hovered.
    pub branch: Option<RefTarget>,
    /// The full id of the commit hovered.
    pub revision: Option<String>,
    /// How many rows show a commit of the hovered branch's history.
    pub highlighted: usize,
    /// Whether the other rows are dimmed (`highlightOnHover` of vcs.workspaceColumns.get).
    pub dims: bool,
    /// The branch nearest to the hovered commit: of the branches that have it in their history,
    /// the one the fewest commits above it (a local branch before a remote one).
    pub nearest: Option<RefTarget>,
    /// The graph's revision (`revision` of vcs.graph.page) it was worked out for.
    pub graph_revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Hover {
    pub info: HoverInfo,
    rows: Vec<bool>,
}

impl Hover {
    pub fn highlights(&self, row: usize) -> Option<bool> {
        (self.info.dims && self.info.branch.is_some()).then(|| self.rows.get(row).copied().unwrap_or(false))
    }
}

pub struct HoverCommand;

impl Command for HoverCommand {
    type Params = HoverParams;
    type Output = Option<HoverInfo>;
    const ID: &'static str = "vcs.workspace.hover";
    const TITLE: &'static str = "Hover in Git Workspace";
    const DESCRIPTION: &'static str = "Says what the pointer is over in the Git workspace's graph, as moving it does: a `branch` \
         (a label or a row of the left panel: its history's rows are highlighted and, with \
         `highlightOnHover`, the others dimmed), or a commit (`revision`: the branch nearest to it is \
         shown by its row); leave both out when the pointer leaves. Answers with what it shows: \
         `highlighted` rows, `nearest` branch; null when nothing is hovered.";

    fn run(&self, session: &mut Session, params: HoverParams) -> Result<Option<HoverInfo>, CommandError> {
        if params.branch.is_none() && params.revision.is_none() {
            session.state_mut::<GitWorkspace>().hover = None;
            return Ok(None);
        }
        let repository = repository(session)?;
        fresh_graph(session, false)?;
        let dims = session.state_mut::<GitWorkspace>().view.columns.highlight_on_hover;
        let (graph, revision) = {
            let slot = vcs(session).graph.slot();
            (slot.graph(), slot.revision)
        };
        if let Some(hover) = &session.state_mut::<GitWorkspace>().hover
            && hover.info.graph_revision == revision
            && hover.info.branch == params.branch
            && hover.info.dims == dims
            && hover.info.revision == params.revision
        {
            return Ok(Some(hover.info.clone()));
        }
        let graph = read(&graph);
        let rows = match &params.branch {
            Some(branch) => {
                let tip = graph.tip_of(&branch.name).ok_or_else(|| {
                    CommandError::new(format!("there is no branch or tag `{}` in the graph", branch.name))
                })?;
                graph.rows_in_history_of(&tip)
            }
            None => Vec::new(),
        };
        let (id, nearest) = match &params.revision {
            Some(revision) => {
                let id = if revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    revision.clone()
                } else {
                    repository.resolve(revision).map_err(|error| CommandError::new(message(&error)))?
                };
                let nearest = graph.nearest_branch(&id);
                (Some(id), nearest)
            }
            None => (None, None),
        };
        drop(graph);
        let info = HoverInfo {
            branch: params.branch,
            revision: id,
            highlighted: rows.iter().filter(|&&row| row).count(),
            dims,
            nearest,
            graph_revision: revision,
        };
        session.state_mut::<GitWorkspace>().hover = Some(Hover { info: info.clone(), rows });
        Ok(Some(info))
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

    fn columns(registry: &Registry, session: &mut Session, params: serde_json::Value) -> ColumnSettings {
        serde_json::from_value(registry.dispatch(session, "vcs.workspaceColumns.set", params).unwrap()).unwrap()
    }

    #[test]
    fn columns_are_reordered_switched_and_resized_within_bounds() {
        let dir = test_repository();
        let (registry, mut session) = session(dir.path());
        let get = registry.dispatch(&mut session, "vcs.workspaceColumns.get", json!({})).unwrap();
        assert_eq!(serde_json::from_value::<ColumnSettings>(get).unwrap(), ColumnSettings::default());
        use GraphColumn::*;
        let set = columns(&registry, &mut session, json!({ "order": ["sha", "graph"] }));
        assert_eq!(set.order, [Sha, Graph, Refs, Message, Author, Date]);
        let set = columns(
            &registry,
            &mut session,
            json!({ "hide": ["author", "sha"], "widths": { "date": 1000, "refs": 90 } }),
        );
        assert_eq!(set.hidden, [Author, Sha]);
        assert_eq!((set.widths[&Date], set.widths[&Refs]), (300.0, 90.0), "kept within bounds");
        assert!(!set.shown(Author) && set.shown(Date) && set.shown(Message));
        assert_eq!(set.width(Author), Some(140.0), "its own width while not dragged");
        let set = columns(&registry, &mut session, json!({ "show": ["sha"], "highlightOnHover": false }));
        assert_eq!(set.hidden, [Author]);
        assert!(!set.highlight_on_hover);
        let set = columns(&registry, &mut session, json!({ "resetWidths": true, "widths": { "sha": 10 } }));
        assert_eq!(set.widths.into_iter().collect::<Vec<_>>(), [(Sha, 50.0)]);
        for wrong in
            [json!({ "hide": ["graph"] }), json!({ "widths": { "message": 300 } }), json!({ "order": ["nowhere"] })]
        {
            assert!(registry.dispatch(&mut session, "vcs.workspaceColumns.set", wrong.clone()).is_err(), "{wrong}");
        }
        let state = registry.dispatch(&mut session, "vcs.workspace.state", json!({})).unwrap();
        assert_eq!(state["view"]["columns"]["hidden"], json!(["author"]));
        let saved = ColumnSettings { order: vec![Message], hidden: vec![Graph, Date], ..ColumnSettings::default() };
        let set = columns(&registry, &mut session, json!({ "set": saved }));
        assert_eq!((set.order[0], set.order.len(), set.hidden.clone()), (Message, 6, vec![Date]), "made whole");
        assert_eq!(columns(&registry, &mut session, json!({ "reset": true })), ColumnSettings::default());
    }

    #[test]
    fn a_width_that_is_not_a_number_is_dropped_from_what_was_saved() {
        let widths = BTreeMap::from([(GraphColumn::Author, f32::NAN)]);
        let settings = ColumnSettings { widths, ..ColumnSettings::default() }.normalized();
        assert!(settings.widths.is_empty());
    }

    #[test]
    fn widths_dragged_through_the_view_are_the_columns_widths() {
        let dir = test_repository();
        let (registry, mut session) = session(dir.path());
        registry.dispatch(&mut session, "vcs.workspace.view", json!({ "refsWidth": 100, "graphWidth": 9000 })).unwrap();
        let get: ColumnSettings =
            serde_json::from_value(registry.dispatch(&mut session, "vcs.workspaceColumns.get", json!({})).unwrap())
                .unwrap();
        assert_eq!(get.widths[&GraphColumn::Refs], 100.0);
        assert_eq!(get.widths[&GraphColumn::Graph], super::super::workspace_view::GRAPH_WIDTH.1);
    }

    #[test]
    fn the_columns_menu_opens_and_closes_and_gives_way_to_another_menu() {
        let dir = test_repository();
        let (registry, mut session) = session(dir.path());
        let opened = registry.dispatch(&mut session, "vcs.workspace.columnsMenu", json!({})).unwrap();
        assert_eq!(opened["columnsMenu"], json!(true));
        let other = registry.dispatch(&mut session, "vcs.workspace.pullMenu", json!({})).unwrap();
        assert_eq!((other["columnsMenu"].as_bool(), other["pullMenu"].as_bool()), (Some(false), Some(true)));
        registry.dispatch(&mut session, "vcs.workspace.columnsMenu", json!({})).unwrap();
        let closed = registry.dispatch(&mut session, "vcs.workspace.columnsMenu", json!({ "open": false })).unwrap();
        assert_eq!(closed["columnsMenu"], json!(false));
    }

    fn branchy() -> tempfile::TempDir {
        let dir = test_repository();
        let root = dir.path();
        let commit = |name: &str| {
            std::fs::write(root.join(format!("{name}.txt")), name).unwrap();
            git(root, &["add", "."]);
            git(root, &["commit", "-q", "-m", name]);
        };
        git(root, &["checkout", "-q", "-b", "feature"]);
        commit("c");
        git(root, &["checkout", "-q", "main"]);
        commit("b");
        git(root, &["branch", "topic"]);
        git(root, &["merge", "-q", "--no-edit", "feature"]);
        git(root, &["checkout", "-q", "feature"]);
        commit("d");
        std::fs::write(root.join("d.txt"), "changed").unwrap();
        git(root, &["stash", "-q"]);
        git(root, &["checkout", "-q", "main"]);
        dir
    }

    fn hover(registry: &Registry, session: &mut Session, params: serde_json::Value) -> Option<HoverInfo> {
        serde_json::from_value(registry.dispatch(session, "vcs.workspace.hover", params).unwrap()).unwrap()
    }

    #[test]
    fn hovering_a_branch_highlights_its_history_and_dims_the_rest() {
        let dir = branchy();
        let root = dir.path();
        let (registry, mut session) = session(root);
        let page = registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        let rows: Vec<(String, Option<u64>)> = page["commits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| (row["subject"].as_str().unwrap().to_owned(), row["stash"].as_u64()))
            .collect();
        let row_of = |subject: &str| rows.iter().position(|(text, stash)| text == subject && stash.is_none()).unwrap();
        let feature =
            hover(&registry, &mut session, json!({ "branch": { "name": "feature", "kind": "local" } })).unwrap();
        assert_eq!(feature.highlighted, 4, "{rows:?}");
        assert!(feature.dims);
        let workspace = session.state::<GitWorkspace>().unwrap();
        let shown = workspace.hover().unwrap();
        assert_eq!(shown.highlights(row_of("d")), Some(true));
        assert_eq!(shown.highlights(row_of("c")), Some(true));
        assert_eq!(shown.highlights(row_of("b")), Some(false), "b is main's, not feature's");
        let main = hover(&registry, &mut session, json!({ "branch": { "name": "main", "kind": "local" } })).unwrap();
        assert_eq!(main.highlighted, 4, "the merge, b, c and the first commit");
        columns(&registry, &mut session, json!({ "highlightOnHover": false }));
        let quiet = hover(&registry, &mut session, json!({ "branch": { "name": "main", "kind": "local" } })).unwrap();
        assert!(!quiet.dims);
        assert_eq!(session.state::<GitWorkspace>().unwrap().hover().unwrap().highlights(0), None);
        assert!(
            registry
                .dispatch(
                    &mut session,
                    "vcs.workspace.hover",
                    json!({ "branch": { "name": "nowhere", "kind": "local" } })
                )
                .is_err()
        );
        assert_eq!(hover(&registry, &mut session, json!({})), None);
        assert!(session.state::<GitWorkspace>().unwrap().hover().is_none());
    }

    #[test]
    fn the_branch_nearest_to_a_hovered_commit_is_found() {
        let dir = branchy();
        let root = dir.path();
        let (registry, mut session) = session(root);
        registry.dispatch(&mut session, "vcs.graph.page", json!({ "wait": true })).unwrap();
        let nearest = |session: &mut Session, revision: &str| {
            hover(&registry, session, json!({ "revision": revision })).unwrap().nearest.map(|branch| branch.name)
        };
        assert_eq!(nearest(&mut session, "feature~1").as_deref(), Some("feature"));
        assert_eq!(nearest(&mut session, "main~1").as_deref(), Some("topic"), "b is topic's tip");
        assert_eq!(nearest(&mut session, "main").as_deref(), Some("main"));
        let stash = git(root, &["rev-parse", "stash@{0}"]).trim().to_owned();
        assert_eq!(nearest(&mut session, &stash).as_deref(), Some("feature"), "a stash: its commit's branch");
        let full = git(root, &["rev-parse", "main~1"]).trim().to_owned();
        let info = hover(&registry, &mut session, json!({ "revision": full })).unwrap();
        assert_eq!(info.revision.as_deref(), Some(full.as_str()));
        assert_eq!(info.highlighted, 0, "a commit highlights nothing");
    }
}

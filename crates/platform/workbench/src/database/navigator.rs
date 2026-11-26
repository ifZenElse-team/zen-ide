use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::engine::ConnectionState;
use zen_db::model::NodeKind;
use zen_db::registry::Entry;
use zen_i18n::t;

use super::connections::{Connect, ConnectParams};
use super::sources::{Database, connection_types, files, find};
use super::structure::{self, Structure, tree_options};
use super::{WORKSPACE, project_key};
use crate::Workbench;
use crate::workspaces::Workspaces;

const ROWS_ANSWERED: usize = 200;

const ENTRIES_KEPT: Duration = Duration::from_secs(1);

const FILTER_VISITS: usize = 20_000;

/// Which objects the navigator shows: its view presets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum View {
    /// Tables, views and their columns.
    Simple,
    /// Every folder and object.
    #[default]
    Advanced,
    /// The connection's own choice; today the same as Advanced.
    Custom,
}

/// How the navigator shows its tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewOptions {
    /// The view preset.
    pub view: View,
    /// Whether system schemas and objects are shown.
    pub show_system_objects: bool,
    /// Whether a connection's host and port are shown after its name.
    pub show_host: bool,
    /// Whether comments are shown after the names.
    pub show_descriptions: bool,
    /// Whether tables show their size as a bar.
    pub show_statistics: bool,
    /// Whether only connected connections are shown.
    pub connected_only: bool,
    /// Whether the navigator selects the connection of the console in front.
    pub link_with_editor: bool,
}

impl Default for ViewOptions {
    fn default() -> Self {
        Self {
            view: View::Advanced,
            show_system_objects: false,
            show_host: true,
            show_descriptions: false,
            show_statistics: true,
            connected_only: false,
            link_with_editor: false,
        }
    }
}

/// A row of the navigator: a folder of connections, a connection, or a node of a connection's tree.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RowKey {
    /// The connection, for a connection or one of its nodes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The folder's path (`Work/Shop`), for a folder of connections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The node's path in the connection's tree; empty for the connection itself.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<String>,
}

impl RowKey {
    pub fn connection(id: &str) -> Self {
        Self { connection: Some(id.to_owned()), folder: None, path: Vec::new() }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    expanded: BTreeSet<RowKey>,
    collapsed_folders: BTreeSet<String>,
    selected: Option<RowKey>,
    options: ViewOptions,
}

/// A visible row of the navigator, as drawn and as `db.navigator.state` answers it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// Which row it is.
    pub key: RowKey,
    /// Its depth, 0 at the top.
    pub depth: usize,
    /// `connectionFolder`, `connection`, `loading`, `message`, or a node's kind (`catalog`, `schema`, `folder`, `table`, `view`, `column`, `index`, `foreignKey`, `function`, …).
    pub kind: String,
    /// The text shown.
    pub label: String,
    /// The text after it in grey: a connection's host and port, a column's type.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Its comment, when descriptions are shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Whether it can be expanded.
    pub has_children: bool,
    /// Whether it is expanded.
    pub expanded: bool,
    /// Why reading its children failed; the row is then collapsed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether it is selected.
    pub selected: bool,
    /// Whether it is a system object.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub system: bool,
    /// Its size in bytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Its size against the largest of its siblings, from 0 to 1, for the size bar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_ratio: Option<f32>,
    /// The server's estimate of its rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<i64>,
    /// A connection's state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<ConnectionState>,
    /// Whether a connection is read-only.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    /// A connection's type colour by name (`green`, `red`, …), when its type has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// A connection's driver.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
}

#[derive(Default)]
pub struct Navigator {
    saved: Saved,
    loaded: bool,
    dirty: bool,
    filter: String,
    revision: u64,
    entries: Vec<Entry>,
    folders: Vec<String>,
    colors: HashMap<String, String>,
    read: Option<Instant>,
}

impl Navigator {
    pub fn show_system(&self) -> bool {
        self.saved.options.show_system_objects
    }

    pub fn simple(&self) -> bool {
        self.saved.options.view == View::Simple
    }

    pub fn options(&self) -> &ViewOptions {
        &self.saved.options
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn selected(&self) -> Option<&RowKey> {
        self.saved.selected.as_ref()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    fn changed(&mut self) {
        self.revision += 1;
        self.dirty = true;
    }

    pub fn invalidate(&mut self) {
        self.read = None;
        self.revision += 1;
    }
}

fn state_file(session: &Session) -> Option<PathBuf> {
    let data = session.state::<Workspaces>()?.data_dir()?;
    let project = session.state::<Workbench>()?.project()?;
    Some(data.join("databases").join(project_key(project)).join("navigator.json"))
}

pub fn navigator(session: &mut Session) -> &mut Navigator {
    let loaded = session.state::<Navigator>().is_some_and(|navigator| navigator.loaded);
    if !loaded {
        let saved = state_file(session)
            .and_then(|file| zen_db::registry::read_json::<Saved>(&file).ok().flatten())
            .unwrap_or_default();
        let navigator = session.state_mut::<Navigator>();
        navigator.saved = saved;
        navigator.loaded = true;
        navigator.revision += 1;
    }
    let stale = session
        .state::<Navigator>()
        .is_none_or(|navigator| navigator.read.is_none_or(|read| read.elapsed() >= ENTRIES_KEPT));
    if stale {
        let registry = files(session);
        let entries = registry.entries().unwrap_or_default();
        let mut folders: Vec<String> =
            registry.folders().unwrap_or_default().into_iter().map(|(_, name)| name).collect();
        folders.sort();
        folders.dedup();
        let (types, _) = connection_types(session);
        let colors = entries
            .iter()
            .map(|entry| (entry.source.id.clone(), types.resolve(&entry.source.connection_type).color))
            .filter(|(_, color)| !color.is_empty())
            .collect();
        let navigator = session.state_mut::<Navigator>();
        if navigator.entries != entries || navigator.folders != folders {
            navigator.revision += 1;
        }
        navigator.entries = entries;
        navigator.folders = folders;
        navigator.colors = colors;
        navigator.read = Some(Instant::now());
    }
    session.state_mut::<Navigator>()
}

fn kind_name(kind: &NodeKind) -> String {
    match kind {
        NodeKind::Catalog => "catalog",
        NodeKind::Schema => "schema",
        NodeKind::Folder => "folder",
        NodeKind::Table => "table",
        NodeKind::View => "view",
        NodeKind::Column => "column",
        NodeKind::Index => "index",
        NodeKind::KeyPrefix => "keyPrefix",
        NodeKind::Key => "key",
        NodeKind::Collection => "collection",
        NodeKind::SearchIndex => "searchIndex",
        NodeKind::Other(name) => name.as_str(),
    }
    .to_owned()
}

fn folder_label(id: &str) -> Option<String> {
    Some(match id {
        "tables" => t!("database-folder-tables"),
        "foreignTables" => t!("database-folder-foreign-tables"),
        "views" => t!("database-folder-views"),
        "materializedViews" => t!("database-folder-materialized-views"),
        "functions" => t!("database-folder-functions"),
        "procedures" => t!("database-folder-procedures"),
        "sequences" => t!("database-folder-sequences"),
        "types" => t!("database-folder-types"),
        "events" => t!("database-folder-events"),
        "packages" => t!("database-folder-packages"),
        "columns" => t!("database-folder-columns"),
        "constraints" => t!("database-folder-constraints"),
        "foreignKeys" => t!("database-folder-foreign-keys"),
        "indexes" => t!("database-folder-indexes"),
        "triggers" => t!("database-folder-triggers"),
        "references" => t!("database-folder-references"),
        "policies" => t!("database-folder-policies"),
        "partitions" => t!("database-folder-partitions"),
        "dictionaries" => t!("database-folder-dictionaries"),
        "aggregates" => t!("database-folder-aggregates"),
        "#extensions" => t!("database-folder-extensions"),
        "#eventTriggers" => t!("database-folder-event-triggers"),
        "#tablespaces" => t!("database-folder-tablespaces"),
        "#roles" => t!("database-folder-roles"),
        _ => return None,
    })
}

fn host(entry: &Entry) -> Option<String> {
    let source = &entry.source;
    match (&source.host, source.port) {
        (Some(host), Some(port)) => Some(format!("{host}:{port}")),
        (Some(host), None) => Some(host.clone()),
        (None, _) => source.database.clone(),
    }
}

struct Walk<'a> {
    filter: String,
    options: ViewOptions,
    expanded: &'a BTreeSet<RowKey>,
    selected: Option<RowKey>,
    collapse: Vec<RowKey>,
    visits: usize,
}

fn matches(filter: &str, label: &str) -> bool {
    filter.is_empty() || label.to_lowercase().contains(filter)
}

fn ratios(rows: &mut [Row], from: usize, depth: usize) {
    let max = rows[from..].iter().filter(|row| row.depth == depth).filter_map(|row| row.size).max().unwrap_or(0);
    if max == 0 {
        return;
    }
    for row in rows[from..].iter_mut().filter(|row| row.depth == depth) {
        row.size_ratio = row.size.map(|size| size as f32 / max as f32);
    }
}

fn catalogs_shown(session: &mut Session, entry: &Entry) -> bool {
    session
        .state_mut::<Database>()
        .drivers()
        .iter()
        .find(|driver| driver.id() == entry.source.driver)
        .is_none_or(|driver| driver.capabilities().catalogs)
}

fn node_rows(
    session: &mut Session,
    walk: &mut Walk<'_>,
    entry: &Entry,
    path: &[String],
    depth: usize,
    out: &mut Vec<Row>,
) -> bool {
    let options = tree_options(entry, walk.options.show_system_objects, walk.options.view == View::Simple);
    let filtering = !walk.filter.is_empty();
    let known = structure::children_of(session, entry, path, &options, !filtering);
    let id = entry.source.id.clone();
    if let Some(error) = known.error {
        if let Some(parent) =
            out.last_mut().filter(|row| row.key.connection.as_deref() == Some(&id) && row.key.path == path)
        {
            parent.error = Some(error);
            parent.expanded = false;
        }
        walk.collapse.push(RowKey { connection: Some(id), folder: None, path: path.to_vec() });
        return false;
    }
    let Some(nodes) = known.nodes else {
        if filtering {
            return false;
        }
        let (kind, label) = if known.loading {
            ("loading", t!("database-navigator-loading"))
        } else {
            ("message", t!("database-navigator-not-read"))
        };
        out.push(Row {
            key: RowKey { connection: Some(id), folder: None, path: [path, &[format!("#{kind}")]].concat() },
            depth,
            kind: kind.to_owned(),
            label,
            ..Row::default()
        });
        return false;
    };
    if let [catalog] = nodes.as_slice()
        && path.is_empty()
        && catalog.kind == NodeKind::Catalog
        && !catalogs_shown(session, entry)
    {
        return node_rows(session, walk, entry, &catalog.path, depth, out);
    }
    let start = out.len();
    let mut any = false;
    for node in nodes {
        walk.visits += 1;
        if walk.visits > FILTER_VISITS && filtering {
            break;
        }
        let key = RowKey { connection: Some(id.clone()), folder: None, path: node.path.clone() };
        let kind = kind_name(&node.kind);
        let label = if node.kind == NodeKind::Folder {
            node.path.last().and_then(|id| folder_label(id)).unwrap_or_else(|| node.label.clone())
        } else {
            node.label.clone()
        };
        let own = matches(&walk.filter, &label);
        let row = Row {
            selected: walk.selected.as_ref() == Some(&key),
            expanded: false,
            key: key.clone(),
            depth,
            kind,
            label,
            detail: node.detail.clone(),
            comment: node.comment.clone().filter(|_| walk.options.show_descriptions),
            has_children: node.has_children,
            system: node.system,
            size: node.size.filter(|_| walk.options.show_statistics),
            rows: node.rows,
            ..Row::default()
        };
        let at = out.len();
        out.push(row);
        let open = walk.expanded.contains(&key) || filtering;
        let mut below = false;
        if node.has_children && open {
            below = node_rows(session, walk, entry, &node.path, depth + 1, out);
            out[at].expanded = out.len() > at + 1 || walk.expanded.contains(&key);
        }
        if filtering && !own && !below {
            out.truncate(at);
            continue;
        }
        if filtering && !below {
            out.truncate(at + 1);
            out[at].expanded = false;
        }
        any = true;
    }
    if walk.options.show_statistics {
        ratios(out, start, depth);
    }
    any
}

pub fn rows(session: &mut Session) -> Vec<Row> {
    let navigator = navigator(session);
    let saved = navigator.saved.clone();
    let filter = navigator.filter.to_lowercase();
    let entries = navigator.entries.clone();
    let folders = navigator.folders.clone();
    let colors = navigator.colors.clone();
    let engine = session.state::<Database>().and_then(Database::started).cloned();
    let mut walk = Walk {
        filter: filter.clone(),
        options: saved.options.clone(),
        expanded: &saved.expanded,
        selected: saved.selected.clone(),
        collapse: Vec::new(),
        visits: 0,
    };
    let mut out = Vec::new();
    let mut groups: Vec<(Option<String>, Vec<&Entry>)> = Vec::new();
    for folder in &folders {
        groups.push((Some(folder.clone()), Vec::new()));
    }
    groups.push((None, Vec::new()));
    for entry in &entries {
        let at = groups.iter().position(|(folder, _)| *folder == entry.source.folder).unwrap_or(groups.len() - 1);
        groups[at].1.push(entry);
    }
    for (folder, members) in groups {
        let mut depth = 0;
        let folder_at = out.len();
        if let Some(folder) = &folder {
            let key = RowKey { connection: None, folder: Some(folder.clone()), path: Vec::new() };
            out.push(Row {
                selected: walk.selected.as_ref() == Some(&key),
                key,
                depth: 0,
                kind: "connectionFolder".to_owned(),
                label: folder.clone(),
                has_children: true,
                expanded: !saved.collapsed_folders.contains(folder),
                ..Row::default()
            });
            depth = 1;
            if saved.collapsed_folders.contains(folder) && filter.is_empty() {
                continue;
            }
        }
        let mut shown = 0;
        for entry in members {
            let id = entry.source.id.clone();
            let state = engine.as_ref().map_or(ConnectionState::Disconnected, |engine| engine.status(&id).state);
            if saved.options.connected_only && !matches!(state, ConnectionState::Connected | ConnectionState::Closed) {
                continue;
            }
            let key = RowKey::connection(&id);
            let error = engine
                .as_ref()
                .and_then(|engine| engine.status(&id).error)
                .map(|error| super::connections::describe(&error));
            let at = out.len();
            out.push(Row {
                selected: walk.selected.as_ref() == Some(&key),
                key: key.clone(),
                depth,
                kind: "connection".to_owned(),
                label: entry.source.name.clone(),
                detail: host(entry).filter(|_| saved.options.show_host),
                has_children: true,
                expanded: false,
                error: error.filter(|_| state == ConnectionState::Failed),
                state: Some(state),
                read_only: entry.source.read_only,
                color: colors.get(&id).cloned(),
                driver: Some(entry.source.driver.clone()),
                ..Row::default()
            });
            let own = matches(&filter, &entry.source.name);
            let open = saved.expanded.contains(&key) || !filter.is_empty();
            let mut below = false;
            if open {
                below = node_rows(session, &mut walk, entry, &[], depth + 1, &mut out);
                out[at].expanded = out.len() > at + 1;
            }
            if !filter.is_empty() && !own && !below {
                out.truncate(at);
                continue;
            }
            shown += 1;
        }
        if folder.is_some()
            && !filter.is_empty()
            && shown == 0
            && !matches(&filter, folder.as_deref().unwrap_or_default())
        {
            out.truncate(folder_at);
        }
    }
    let collapse = std::mem::take(&mut walk.collapse);
    if !collapse.is_empty() {
        let navigator = session.state_mut::<Navigator>();
        for key in collapse {
            if navigator.saved.expanded.remove(&key) {
                navigator.dirty = true;
            }
        }
    }
    out
}

pub fn step(session: &mut Session) {
    let Some(navigator) = session.state::<Navigator>() else { return };
    let (dirty, link) = (navigator.dirty, navigator.saved.options.link_with_editor);
    if link {
        let console = session
            .state::<Workspaces>()
            .and_then(|workspaces| workspaces.active_tab(WORKSPACE))
            .and_then(|tab| tab.path.as_ref())
            .and_then(|path| path.parent()?.file_name().map(|name| name.to_string_lossy().into_owned()));
        if let Some(connection) = console {
            let navigator = session.state_mut::<Navigator>();
            let key = RowKey::connection(&connection);
            let known = navigator.entries.iter().any(|entry| entry.source.id == connection);
            let inside = navigator
                .saved
                .selected
                .as_ref()
                .is_some_and(|selected| selected.connection.as_deref() == Some(&connection));
            if known && !inside {
                navigator.saved.selected = Some(key);
                navigator.changed();
            }
        }
    }
    if dirty && let Some(file) = state_file(session) {
        let navigator = session.state_mut::<Navigator>();
        navigator.dirty = false;
        let saved = navigator.saved.clone();
        let _ = zen_db::registry::write_json(&file, &saved);
    }
}

/// A row of the navigator, named by its connection and path or its folder.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RowParams {
    /// The connection, for a connection (with an empty `path`) or one of its nodes.
    pub connection: Option<String>,
    /// A folder of connections, by its path.
    pub folder: Option<String>,
    /// The node's path in the connection's tree, as `db.navigator.state` or `db.schema.children` give it.
    pub path: Vec<String>,
}

impl RowParams {
    fn key(&self, session: &mut Session) -> Result<RowKey, CommandError> {
        if self.connection.is_none() && self.folder.is_none() {
            return navigator(session)
                .saved
                .selected
                .clone()
                .ok_or_else(|| CommandError::new(t!("database-navigator-nothing-selected")));
        }
        Ok(RowKey { connection: self.connection.clone(), folder: self.folder.clone(), path: self.path.clone() })
    }
}

/// What part of the rows to answer.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// The first row to answer.
    pub offset: usize,
    /// The most rows to answer; 200 when absent.
    pub limit: Option<usize>,
}

/// The navigator as it is shown.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StateAnswer {
    /// The visible rows from `offset`, at most `limit`.
    pub rows: Vec<Row>,
    /// How many rows are visible.
    pub total: usize,
    /// Whether rows were left out because of `limit`.
    pub truncated: bool,
    /// The filter typed.
    pub filter: String,
    /// The view options.
    pub options: ViewOptions,
    /// The selected row.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<RowKey>,
}

fn answer(session: &mut Session, params: &StateParams) -> StateAnswer {
    let rows = rows(session);
    let navigator = navigator(session);
    let total = rows.len();
    let limit = params.limit.unwrap_or(ROWS_ANSWERED);
    StateAnswer {
        truncated: total > params.offset + limit,
        rows: rows.into_iter().skip(params.offset).take(limit).collect(),
        total,
        filter: navigator.filter.clone(),
        options: navigator.saved.options.clone(),
        selected: navigator.saved.selected.clone(),
    }
}

pub struct NavigatorState;

impl Command for NavigatorState {
    type Params = StateParams;
    type Output = StateAnswer;
    const ID: &'static str = "db.navigator.state";
    const TITLE: &'static str = "Database Navigator State";
    const DESCRIPTION: &'static str = "Answers the Database Navigator's visible rows in order (from `offset`, at most `limit`, 200): folders of connections, connections (state, host:port, type colour, read-only, error) and the expanded nodes of their trees (databases, schemas, folders per object kind, objects, columns, keys, indexes) with depth, kind, label, detail, expanded, error, size and size ratio; a `loading` row stands for children being read. Also the filter, the view options and the selection. Reads what expanded nodes need in the background; never connects.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<StateAnswer, CommandError> {
        Ok(answer(session, &params))
    }
}

/// What an expand did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpandAnswer {
    /// The row expanded.
    pub key: RowKey,
    /// The connect it started, when the connection was disconnected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connecting: Option<u64>,
}

pub struct NavigatorExpand;

impl Command for NavigatorExpand {
    type Params = RowParams;
    type Output = ExpandAnswer;
    const ID: &'static str = "db.navigator.expand";
    const TITLE: &'static str = "Expand in Database Navigator";
    const DESCRIPTION: &'static str = "Expands a row of the Database Navigator: a folder (`folder`), a connection (`connection`, empty `path`) or a node (`connection` and `path`); the selected row when none is named. Expanding a disconnected connection connects it, as the person's action (code `passwordNeeded` when it has no saved password); children not known yet are read in the background (a `loading` row in db.navigator.state).";

    fn run(&self, session: &mut Session, params: RowParams) -> Result<ExpandAnswer, CommandError> {
        let key = params.key(session)?;
        if let Some(folder) = &key.folder {
            let navigator = navigator(session);
            navigator.saved.collapsed_folders.remove(folder);
            navigator.changed();
            return Ok(ExpandAnswer { key, connecting: None });
        }
        let id = key.connection.clone().unwrap_or_default();
        let entry = find(session, &id)?;
        session.state_mut::<Structure>().clear_error(&id, &key.path);
        let state = session.state::<Database>().and_then(Database::started).map(|engine| engine.status(&id).state);
        let mut connecting = None;
        if key.path.is_empty() && matches!(state, None | Some(ConnectionState::Disconnected | ConnectionState::Failed))
        {
            let started = Connect.run(session, ConnectParams { connection: id.clone(), password: None })?;
            connecting = Some(started.operation);
        }
        structure::ensure(session, &entry, false);
        let navigator = navigator(session);
        navigator.saved.expanded.insert(key.clone());
        navigator.changed();
        Ok(ExpandAnswer { key, connecting })
    }
}

/// Which rows to collapse.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CollapseParams {
    /// The row to collapse, with its descendants.
    #[serde(flatten)]
    pub row: RowParams,
    /// Collapse every row (Collapse All).
    pub all: bool,
}

/// The rows collapsed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Collapsed {
    /// How many expanded rows were collapsed.
    pub collapsed: usize,
}

pub struct NavigatorCollapse;

impl Command for NavigatorCollapse {
    type Params = CollapseParams;
    type Output = Collapsed;
    const ID: &'static str = "db.navigator.collapse";
    const TITLE: &'static str = "Collapse in Database Navigator";
    const DESCRIPTION: &'static str = "Collapses a row of the Database Navigator and its expanded descendants (the selected row when none is named), or with `all` every row (Collapse All). Nothing is disconnected.";

    fn run(&self, session: &mut Session, params: CollapseParams) -> Result<Collapsed, CommandError> {
        let key = if params.all { None } else { Some(params.row.key(session)?) };
        let navigator = navigator(session);
        let before = navigator.saved.expanded.len();
        match &key {
            None => navigator.saved.expanded.clear(),
            Some(RowKey { folder: Some(folder), .. }) => {
                navigator.saved.collapsed_folders.insert(folder.clone());
            }
            Some(key) => navigator
                .saved
                .expanded
                .retain(|open| !(open.connection == key.connection && open.path.starts_with(&key.path))),
        }
        let collapsed = before - navigator.saved.expanded.len();
        navigator.changed();
        Ok(Collapsed { collapsed })
    }
}

pub struct NavigatorSelect;

impl Command for NavigatorSelect {
    type Params = RowParams;
    type Output = RowKey;
    const ID: &'static str = "db.navigator.select";
    const TITLE: &'static str = "Select in Database Navigator";
    const DESCRIPTION: &'static str = "Selects a row of the Database Navigator by `connection` and `path`, or `folder`; its ancestors are expanded so that it shows. Answers the row's key.";

    fn run(&self, session: &mut Session, params: RowParams) -> Result<RowKey, CommandError> {
        let key = params.key(session)?;
        let navigator = navigator(session);
        for length in 0..key.path.len() {
            if key.connection.is_some() {
                navigator.saved.expanded.insert(RowKey {
                    connection: key.connection.clone(),
                    folder: None,
                    path: key.path[..length].to_vec(),
                });
            }
        }
        navigator.saved.selected = Some(key.clone());
        navigator.changed();
        Ok(key)
    }
}

pub struct NavigatorRefresh;

impl Command for NavigatorRefresh {
    type Params = RowParams;
    type Output = structure::RefreshAnswer;
    const ID: &'static str = "db.navigator.refresh";
    const TITLE: &'static str = "Refresh";
    const DESCRIPTION: &'static str = "Refresh (F5, ⌘R in the navigator): reads the selected row's structure again, or the named one's, in the background, keeping every expanded row expanded. On a connection or database the fingerprints decide which schemas changed; a schema or an object below is read again whatever they say. A folder of connections refreshes its connected connections. Never connects.";

    fn run(&self, session: &mut Session, params: RowParams) -> Result<structure::RefreshAnswer, CommandError> {
        let key = params.key(session)?;
        let ids: Vec<String> = match (&key.connection, &key.folder) {
            (Some(id), _) => vec![id.clone()],
            (None, Some(folder)) => navigator(session)
                .entries
                .iter()
                .filter(|entry| entry.source.folder.as_ref() == Some(folder))
                .map(|entry| entry.source.id.clone())
                .collect(),
            (None, None) => Vec::new(),
        };
        let mut first = None;
        for id in ids {
            let answer = structure::SchemaRefresh.run(
                session,
                structure::RefreshParams {
                    connection: id,
                    path: if key.folder.is_some() { Vec::new() } else { key.path.clone() },
                    force: false,
                },
            )?;
            first = first.or(answer.operation);
        }
        navigator(session).revision += 1;
        Ok(structure::RefreshAnswer { operation: first, started: first.is_some() })
    }
}

/// The navigator's filter.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterParams {
    /// The text to filter by (part of a name, any case); empty shows everything.
    pub text: Option<String>,
    /// Show only connected connections.
    pub connected_only: Option<bool>,
}

pub struct NavigatorFilter;

impl Command for NavigatorFilter {
    type Params = FilterParams;
    type Output = StateAnswer;
    const ID: &'static str = "db.navigator.filter";
    const TITLE: &'static str = "Filter Database Navigator";
    const DESCRIPTION: &'static str = "Filters the Database Navigator: `text` keeps the rows whose names contain it (any case) among what is already known, with their ancestors, expanding them so that the matches show; `connectedOnly` hides disconnected connections. Answers the first 200 rows.";

    fn run(&self, session: &mut Session, params: FilterParams) -> Result<StateAnswer, CommandError> {
        let navigator = navigator(session);
        if let Some(text) = params.text {
            navigator.filter = text;
            navigator.revision += 1;
        }
        if let Some(connected) = params.connected_only {
            navigator.saved.options.connected_only = connected;
            navigator.changed();
        }
        Ok(answer(session, &StateParams::default()))
    }
}

/// View options to change; absent ones stay.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OptionsParams {
    /// The view preset: `simple`, `advanced` or `custom`.
    pub view: Option<View>,
    /// Show system schemas and objects.
    pub show_system_objects: Option<bool>,
    /// Show a connection's host and port.
    pub show_host: Option<bool>,
    /// Show comments after the names.
    pub show_descriptions: Option<bool>,
    /// Show tables' sizes as bars.
    pub show_statistics: Option<bool>,
    /// Select the connection of the console in front.
    pub link_with_editor: Option<bool>,
}

pub struct NavigatorOptions;

impl Command for NavigatorOptions {
    type Params = OptionsParams;
    type Output = ViewOptions;
    const ID: &'static str = "db.navigator.options";
    const TITLE: &'static str = "Database Navigator Options";
    const DESCRIPTION: &'static str = "Changes the Database Navigator's view options, kept per project: `view` (`simple`: tables, views and columns; `advanced`, the default; `custom`), `showSystemObjects` (off), `showHost` (on), `showDescriptions` (off), `showStatistics` (size bars, on), `linkWithEditor` (off). Answers the options in effect.";

    fn run(&self, session: &mut Session, params: OptionsParams) -> Result<ViewOptions, CommandError> {
        let navigator = navigator(session);
        let options = &mut navigator.saved.options;
        if let Some(view) = params.view {
            options.view = view;
        }
        if let Some(value) = params.show_system_objects {
            options.show_system_objects = value;
        }
        if let Some(value) = params.show_host {
            options.show_host = value;
        }
        if let Some(value) = params.show_descriptions {
            options.show_descriptions = value;
        }
        if let Some(value) = params.show_statistics {
            options.show_statistics = value;
        }
        if let Some(value) = params.link_with_editor {
            options.link_with_editor = value;
        }
        let options = options.clone();
        navigator.changed();
        Ok(options)
    }
}

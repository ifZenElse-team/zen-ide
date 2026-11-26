use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::diagram::{Colors, ErDiagram, Layout, Metrics, layout, mermaid, of_schema, of_table, svg};
use zen_db::registry::Entry;
use zen_db::schema::{IntrospectRequest, LARGE_SCHEMA, Level};
use zen_i18n::t;

use super::WORKSPACE;
use super::objects::{self, ObjectOpen, ObjectTab, OpenParams, Page};
use super::sources::{Database, find};
use super::structure::{self, Structure};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};

pub const KIND: &str = "diagram";

const ZOOMS: [u32; 15] = [10, 25, 33, 50, 67, 75, 90, 100, 110, 125, 150, 175, 200, 300, 400];
const DETAILS_ASKED: usize = 500;
const LISTED: usize = 100;
const MOST_LISTED: usize = 1000;
const TEXT_LIMIT: usize = 20_000;
const MOST_PIXELS: f32 = 64_000_000.0;

/// A diagram's zoom and scroll position, kept with its tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct View {
    /// The zoom in percent, 100 for actual size.
    pub zoom: u32,
    /// The diagram's point shown at the canvas's left edge.
    pub x: i32,
    /// The diagram's point shown at the canvas's top edge.
    pub y: i32,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 100, x: -24, y: -24 }
    }
}

impl View {
    pub fn is_default(&self) -> bool {
        *self == View::default()
    }
}

/// What a schema's diagram tab keeps in `workspace.json`: never data values or secrets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DiagramTab {
    /// The connection's id.
    pub connection: String,
    /// The schema's path, `[catalog, schema]`.
    pub path: Vec<String>,
    /// The zoom and scroll position.
    pub view: View,
}

pub trait Renderer: Send + Sync {
    fn colors(&self) -> Colors;
    fn png(&self, diagram: &ErDiagram, layout: &Layout, scale: f32, path: &Path) -> Result<(), String>;
}

#[derive(Debug)]
pub struct Built {
    pub diagram: Arc<ErDiagram>,
    pub layout: Layout,
}

type Slot = Arc<Mutex<Option<Result<Arc<Built>, String>>>>;

struct Job {
    diagram: Arc<ErDiagram>,
    slot: Slot,
    cancel: Arc<AtomicBool>,
    started: Instant,
    shown: Option<Arc<Built>>,
    asked: bool,
    revision: u64,
}

struct ExportJob {
    format: Format,
    path: PathBuf,
    slot: Arc<Mutex<Option<Result<u64, String>>>>,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Diagrams {
    jobs: HashMap<String, Job>,
    selected: HashMap<String, String>,
    exports: HashMap<String, ExportJob>,
    renderer: Option<Arc<dyn Renderer>>,
}

impl Diagrams {
    pub fn set_renderer(&mut self, renderer: Arc<dyn Renderer>) {
        self.renderer = Some(renderer);
    }

    pub fn shown(&mut self, tab: &str) -> Option<Arc<Built>> {
        let job = self.jobs.get_mut(tab)?;
        take(job);
        job.shown.clone()
    }

    pub fn computing(&self, tab: &str) -> bool {
        self.jobs.get(tab).is_some_and(|job| job.slot.lock().is_ok_and(|slot| slot.is_none()))
    }

    pub fn selected(&self, tab: &str) -> Option<&str> {
        self.selected.get(tab).map(String::as_str)
    }
}

fn take(job: &mut Job) -> Option<String> {
    let done = job.slot.lock().ok()?.clone()?;
    match done {
        Ok(built) => {
            job.shown = Some(built);
            None
        }
        Err(error) => Some(error),
    }
}

/// The format of an export.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    /// A PNG image of the diagram in the theme's colours.
    Png,
    /// An SVG document in the theme's colours.
    Svg,
    /// Mermaid `erDiagram` text.
    Mermaid,
}

struct Source {
    tab: String,
    connection: String,
    path: Vec<String>,
    view: View,
    object: bool,
}

fn tab_of(session: &Session, tab: Option<&str>) -> Result<InnerTab, CommandError> {
    let workspaces = session.state::<Workspaces>();
    let found = match tab {
        Some(id) => workspaces.and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|known| known.id == id)),
        None => workspaces.and_then(|workspaces| workspaces.active_tab(WORKSPACE)),
    };
    found
        .filter(|known| known.kind == KIND || known.kind == objects::KIND)
        .cloned()
        .ok_or_else(|| CommandError::with_code("noDiagram", t!("database-diagram-error-no-tab")))
}

fn source(session: &Session, tab: Option<&str>) -> Result<Source, CommandError> {
    let tab = tab_of(session, tab)?;
    if tab.kind == KIND {
        let data: DiagramTab = serde_json::from_value(tab.data.clone()).unwrap_or_default();
        return Ok(Source {
            tab: tab.id,
            connection: data.connection,
            path: data.path,
            view: data.view,
            object: false,
        });
    }
    let data: ObjectTab = serde_json::from_value(tab.data.clone()).unwrap_or_default();
    if !objects::pages(&kind_of(&data.path)).contains(&Page::Diagram) {
        return Err(CommandError::with_code("noDiagram", t!("database-diagram-error-no-tab")));
    }
    Ok(Source { tab: tab.id, connection: data.connection, path: data.path, view: data.diagram, object: true })
}

fn kind_of(path: &[String]) -> String {
    match path.get(2).map(String::as_str) {
        Some("views") => "view".to_owned(),
        Some("materializedViews") => "materializedView".to_owned(),
        Some("foreignTables") => "foreignTable".to_owned(),
        Some("tables") => "table".to_owned(),
        _ => String::new(),
    }
}

fn build(session: &mut Session, entry: &Entry, path: &[String]) -> Option<ErDiagram> {
    structure::ensure(session, entry, false);
    let snapshot = session.state::<Structure>()?.snapshot(&entry.source.id)?;
    match path {
        [catalog, schema] => of_schema(snapshot, catalog, schema),
        [_, _, _, _] => of_table(snapshot, path),
        _ => None,
    }
}

fn ask_details(session: &mut Session, entry: &Entry, path: &[String], diagram: &ErDiagram) -> bool {
    let level = session
        .state::<Structure>()
        .and_then(|structure| structure.snapshot(&entry.source.id))
        .and_then(|snapshot| snapshot.schema(&path[0], path.get(1)?))
        .map(|schema| schema.level);
    let request = if level == Some(Level::NotRead) {
        IntrospectRequest { schemas: vec![path[1].clone()], large_schema: LARGE_SCHEMA, ..IntrospectRequest::default() }
    } else {
        let objects: Vec<(String, String)> = diagram
            .entities
            .iter()
            .filter(|entity| !entity.detailed)
            .take(DETAILS_ASKED)
            .map(|entity| (entity.schema.clone(), entity.name.clone()))
            .collect();
        if objects.is_empty() {
            return false;
        }
        IntrospectRequest { objects, large_schema: LARGE_SCHEMA, system: true, ..IntrospectRequest::default() }
    };
    matches!(structure::refresh(session, entry, request, path.to_vec()), Ok(Some(_)))
}

fn spawn(diagram: Arc<ErDiagram>, waker: Option<zen_db::Waker>) -> (Slot, Arc<AtomicBool>) {
    let slot: Slot = Arc::new(Mutex::new(None));
    let cancel = Arc::new(AtomicBool::new(false));
    let (into, stop) = (Arc::clone(&slot), Arc::clone(&cancel));
    let started = std::thread::Builder::new().name("zen-db-diagram".to_owned()).spawn(move || {
        let laid = layout(&diagram, &Metrics::default(), &stop);
        let done = match laid {
            Some(layout) => Ok(Arc::new(Built { diagram, layout })),
            None => Err(t!("database-diagram-cancelled")),
        };
        if let Ok(mut slot) = into.lock() {
            *slot = Some(done);
        }
        if let Some(waker) = waker {
            waker();
        }
    });
    if let Err(error) = started
        && let Ok(mut slot) = slot.lock()
    {
        *slot = Some(Err(error.to_string()));
    }
    (slot, cancel)
}

pub fn refresh(session: &mut Session, tab: &str) -> Result<(), CommandError> {
    update(session, tab, false)
}

fn update(session: &mut Session, tab: &str, again: bool) -> Result<(), CommandError> {
    let source = source(session, Some(tab))?;
    let revision = session.state::<Structure>().map_or(0, Structure::revision);
    let retry = |job: &Job| again && job.cancel.load(Ordering::Relaxed);
    let fresh = session
        .state::<Diagrams>()
        .and_then(|diagrams| diagrams.jobs.get(&source.tab))
        .is_some_and(|job| job.revision == revision && !retry(job));
    if fresh {
        return Ok(());
    }
    let entry = find(session, &source.connection)?;
    let Some(diagram) = build(session, &entry, &source.path) else {
        if let Some(job) = session.state_mut::<Diagrams>().jobs.remove(&source.tab) {
            job.cancel.store(true, Ordering::Relaxed);
        }
        return Ok(());
    };
    let revision = session.state::<Structure>().map_or(0, Structure::revision);
    let known = session.state_mut::<Diagrams>().jobs.get_mut(&source.tab);
    let (same, asked) = match known {
        Some(job) if *job.diagram == diagram && !retry(job) => {
            job.revision = revision;
            (true, job.asked)
        }
        _ => (false, false),
    };
    let diagram = if same { None } else { Some(Arc::new(diagram)) };
    if let Some(diagram) = &diagram {
        let waker = session.state::<Database>().and_then(Database::waker);
        let (slot, cancel) = spawn(Arc::clone(diagram), waker);
        let diagrams = session.state_mut::<Diagrams>();
        let shown = diagrams.jobs.get_mut(&source.tab).and_then(|job| {
            job.cancel.store(true, Ordering::Relaxed);
            take(job);
            job.shown.take()
        });
        let job = Job { diagram: Arc::clone(diagram), slot, cancel, started: Instant::now(), shown, asked, revision };
        diagrams.jobs.insert(source.tab.clone(), job);
    }
    let current = session
        .state::<Diagrams>()
        .and_then(|diagrams| diagrams.jobs.get(&source.tab))
        .map(|job| Arc::clone(&job.diagram));
    if let Some(current) = current
        && current.partial()
        && !asked
        && ask_details(session, &entry, &source.path, &current)
        && let Some(job) = session.state_mut::<Diagrams>().jobs.get_mut(&source.tab)
    {
        job.asked = true;
    }
    Ok(())
}

fn set_view(session: &mut Session, source: &Source, view: View) -> Result<(), CommandError> {
    let tab = tab_of(session, Some(&source.tab))?;
    let data = if source.object {
        let mut data: ObjectTab = serde_json::from_value(tab.data).unwrap_or_default();
        data.diagram = view;
        serde_json::to_value(data)
    } else {
        let mut data: DiagramTab = serde_json::from_value(tab.data).unwrap_or_default();
        data.view = view;
        serde_json::to_value(data)
    };
    session.state_mut::<Workspaces>().set_kind_data(WORKSPACE, &source.tab, data.unwrap_or_default());
    Ok(())
}

pub fn view_of(session: &Session, tab: &str) -> Option<View> {
    source(session, Some(tab)).ok().map(|source| source.view)
}

/// What to open a diagram of.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenDiagramParams {
    /// The connection's id.
    pub connection: String,
    /// `[catalog, schema]` for a schema's diagram, `[catalog, schema, folder, name]` for a table's or view's.
    pub path: Vec<String>,
}

/// A diagram opened.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDiagram {
    /// The tab showing it.
    pub tab: InnerTab,
    /// `schema` or `table`.
    pub of: String,
}

pub struct OpenDiagram;

impl Command for OpenDiagram {
    type Params = OpenDiagramParams;
    type Output = OpenedDiagram;
    const ID: &'static str = "db.diagram.open";
    const TITLE: &'static str = "View Database Diagram";
    const DESCRIPTION: &'static str = "Opens the ER diagram of a schema (`path` `[catalog, schema]`: a tab of kind `diagram`, id `diagram:<connection>:<catalog>/<schema>`, with its tables, views and the tables of other schemas they reference) or of a table or view (`[catalog, schema, folder, name]`: its object tab on the Diagram page, with the tables it references and that reference it), and expands the Database workspace. Read from the connection's snapshot, offline, and laid out in the background; never connects. Follow it with db.diagram.state.";

    fn run(&self, session: &mut Session, params: OpenDiagramParams) -> Result<OpenedDiagram, CommandError> {
        let entry = find(session, &params.connection)?;
        if params.path.len() == 4 {
            let opened = ObjectOpen.run(
                session,
                OpenParams { connection: params.connection, path: params.path, page: Some(Page::Diagram) },
            )?;
            update(session, &opened.tab.id, true)?;
            return Ok(OpenedDiagram { tab: opened.tab, of: "table".to_owned() });
        }
        let [catalog, schema] = params.path.as_slice() else {
            return Err(CommandError::with_code("noDiagram", t!("database-diagram-error-path")));
        };
        structure::ensure(session, &entry, true);
        let known = session
            .state::<Structure>()
            .and_then(|structure| structure.snapshot(&entry.source.id))
            .is_some_and(|snapshot| snapshot.schema(catalog, schema).is_some());
        if !known {
            return Err(CommandError::with_code(
                "unknownObject",
                t!("database-error-unknown-object", path = params.path.join("/"), connection = params.connection),
            ));
        }
        let id = format!("{KIND}:{}", objects::tab_id(&entry.source.id, &params.path).trim_start_matches("object:"));
        let existing = session
            .state::<Workspaces>()
            .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|tab| tab.id == id).cloned());
        let data = existing.and_then(|tab| serde_json::from_value::<DiagramTab>(tab.data).ok()).unwrap_or_else(|| {
            DiagramTab { connection: entry.source.id.clone(), path: params.path.clone(), view: View::default() }
        });
        let open = OpenTabParams {
            workspace: WORKSPACE.to_owned(),
            id: Some(id.clone()),
            kind: Some(KIND.to_owned()),
            title: Some(t!("database-diagram-tab-title", schema = schema.as_str())),
            closable: Some(true),
            data: Some(serde_json::to_value(&data).unwrap_or_default()),
            path: None,
            group: None,
        };
        let tab = OpenTab.run(session, open)?;
        update(session, &id, true)?;
        Ok(OpenedDiagram { tab, of: "schema".to_owned() })
    }
}

/// Which diagram and how much of it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DiagramStateParams {
    /// The tab: a diagram tab or an object tab with a Diagram page; the active tab of the Database workspace when absent.
    pub tab: Option<String>,
    /// The first entity listed.
    pub offset: Option<usize>,
    /// How many entities and relations to list; 100 when absent, at most 1000.
    pub limit: Option<usize>,
    /// Whether to list each entity's columns; true when absent.
    pub columns: Option<bool>,
    /// How long to wait for the layout, in milliseconds (at most 10 000).
    pub wait_ms: Option<u64>,
}

/// A column as agents read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ColumnInfo {
    /// Its name.
    pub name: String,
    /// Its type.
    #[serde(rename = "type")]
    pub type_name: String,
    /// The keys it is part of: `primary`, `foreign`, `unique`.
    pub keys: Vec<String>,
    /// Whether it may be NULL.
    pub nullable: bool,
}

/// An entity as agents read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntityInfo {
    /// The name shown: qualified with its schema for another schema's table.
    pub name: String,
    /// Its schema.
    pub schema: String,
    /// What it is: `table`, `view`, `materializedView` or `foreignTable`.
    pub kind: String,
    /// Its path, for db.object.open.
    pub path: Vec<String>,
    /// Whether its columns and keys are known.
    pub detailed: bool,
    /// Its box `[x, y, width, height]` at 100 %, once laid out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<[f32; 4]>,
    /// How many columns it has.
    pub column_count: usize,
    /// Its columns, when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ColumnInfo>>,
}

/// A relation as agents read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelationInfo {
    /// The foreign key's name.
    pub name: String,
    /// The entity holding it.
    pub from: String,
    /// Its columns.
    pub from_columns: Vec<String>,
    /// The entity it references.
    pub to: String,
    /// The referenced columns.
    pub to_columns: Vec<String>,
    /// `many-to-one`, or `one-to-one` when its columns are unique.
    pub cardinality: String,
    /// Whether a row may reference nothing.
    pub optional: bool,
}

/// An export as agents follow it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportInfo {
    /// The format.
    pub format: Format,
    /// The file written.
    pub path: PathBuf,
    /// `running`, `done`, `failed` or `cancelled`.
    pub status: String,
    /// The file's size in bytes, once written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A diagram as views and agents read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DiagramState {
    /// The tab.
    pub tab: String,
    /// The connection's id.
    pub connection: String,
    /// What the diagram is of.
    pub path: Vec<String>,
    /// `computing`, `ready`, `failed`, `cancelled` (db.diagram.cancel; db.diagram.open lays it out again) or `missing` (the snapshot does not have it, or was not read yet).
    pub status: String,
    /// Why laying it out failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// How long the layout in flight has run, in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    /// Whether some entity's columns are not known yet: they are read when the connection is connected.
    pub partial: bool,
    /// The entities listed.
    pub entities: Vec<EntityInfo>,
    /// How many entities it has.
    pub total: usize,
    /// The relations listed.
    pub relations: Vec<RelationInfo>,
    /// How many relations it has.
    pub relation_total: usize,
    /// Whether entities or relations were left out.
    pub truncated: bool,
    /// The diagram's size `[width, height]` at 100 %, once laid out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 2]>,
    /// The zoom and scroll position.
    pub view: View,
    /// The entity selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<String>,
    /// The latest export.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export: Option<ExportInfo>,
}

fn wait_for(session: &Session, tab: &str, wait: Option<u64>) {
    let Some(wait) = wait.filter(|wait| *wait > 0) else { return };
    let until = Instant::now() + Duration::from_millis(wait.min(10_000));
    while Instant::now() < until {
        let busy = session.state::<Diagrams>().is_some_and(|diagrams| {
            diagrams.computing(tab)
                || diagrams.exports.get(tab).is_some_and(|export| export.slot.lock().is_ok_and(|slot| slot.is_none()))
        });
        if !busy {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn export_info(export: &ExportJob) -> ExportInfo {
    let done = export.slot.lock().ok().and_then(|slot| slot.clone());
    let (status, bytes, error) = match done {
        None => ("running", None, None),
        Some(Ok(bytes)) => ("done", Some(bytes), None),
        Some(Err(error)) if export.cancel.load(Ordering::Relaxed) => ("cancelled", None, Some(error)),
        Some(Err(error)) => ("failed", None, Some(error)),
    };
    ExportInfo { format: export.format, path: export.path.clone(), status: status.to_owned(), bytes, error }
}

fn keys(column: &zen_db::diagram::Attribute) -> Vec<String> {
    [(column.primary, "primary"), (column.foreign, "foreign"), (column.unique, "unique")]
        .into_iter()
        .filter(|(on, _)| *on)
        .map(|(_, key)| key.to_owned())
        .collect()
}

fn state_of(session: &mut Session, params: &DiagramStateParams) -> Result<DiagramState, CommandError> {
    let source = source(session, params.tab.as_deref())?;
    refresh(session, &source.tab)?;
    wait_for(session, &source.tab, params.wait_ms);
    let diagrams = session.state_mut::<Diagrams>();
    let selected = diagrams.selected(&source.tab).map(str::to_owned);
    let export = diagrams.exports.get(&source.tab).map(export_info);
    let mut state = DiagramState {
        tab: source.tab.clone(),
        connection: source.connection,
        path: source.path,
        status: "missing".to_owned(),
        error: None,
        elapsed_ms: None,
        partial: false,
        entities: Vec::new(),
        total: 0,
        relations: Vec::new(),
        relation_total: 0,
        truncated: false,
        size: None,
        view: source.view,
        selected,
        export,
    };
    let Some(job) = diagrams.jobs.get_mut(&source.tab) else { return Ok(state) };
    let failed = take(job);
    let computing = job.slot.lock().is_ok_and(|slot| slot.is_none());
    state.status = match (&failed, computing) {
        (_, true) => "computing",
        (Some(_), false) if job.cancel.load(Ordering::Relaxed) => "cancelled",
        (Some(_), false) => "failed",
        (None, false) => "ready",
    }
    .to_owned();
    state.error = failed;
    state.elapsed_ms = computing.then(|| job.started.elapsed().as_millis() as u64);
    let diagram = Arc::clone(&job.diagram);
    let built = job.shown.clone().filter(|built| Arc::ptr_eq(&built.diagram, &diagram) || *built.diagram == *diagram);
    let limit = params.limit.unwrap_or(LISTED).min(MOST_LISTED);
    let offset = params.offset.unwrap_or(0);
    let with_columns = params.columns.unwrap_or(true);
    state.partial = diagram.partial();
    state.total = diagram.entities.len();
    state.relation_total = diagram.relations.len();
    state.size = built.as_ref().map(|built| [built.layout.width, built.layout.height]);
    state.entities = diagram
        .entities
        .iter()
        .enumerate()
        .skip(offset)
        .take(limit)
        .map(|(at, entity)| EntityInfo {
            name: entity.label(),
            schema: entity.schema.clone(),
            kind: entity.kind.clone(),
            path: entity.path.clone(),
            detailed: entity.detailed,
            bounds: built.as_ref().map(|built| {
                let bounds = built.layout.boxes[at];
                [bounds.x, bounds.y, bounds.width, bounds.height]
            }),
            column_count: entity.columns.len(),
            columns: with_columns.then(|| {
                entity
                    .columns
                    .iter()
                    .map(|column| ColumnInfo {
                        name: column.name.clone(),
                        type_name: column.type_name.clone(),
                        keys: keys(column),
                        nullable: column.nullable,
                    })
                    .collect()
            }),
        })
        .collect();
    state.relations = diagram
        .relations
        .iter()
        .take(limit)
        .map(|relation| RelationInfo {
            name: relation.name.clone(),
            from: diagram.entities[relation.from].label(),
            from_columns: relation.from_columns.clone(),
            to: diagram.entities[relation.to].label(),
            to_columns: relation.to_columns.clone(),
            cardinality: if relation.unique { "one-to-one" } else { "many-to-one" }.to_owned(),
            optional: relation.optional,
        })
        .collect();
    state.truncated = offset + state.entities.len() < state.total || state.relations.len() < state.relation_total;
    Ok(state)
}

pub struct DiagramStateCommand;

impl Command for DiagramStateCommand {
    type Params = DiagramStateParams;
    type Output = DiagramState;
    const ID: &'static str = "db.diagram.state";
    const TITLE: &'static str = "Database Diagram";
    const DESCRIPTION: &'static str = "The ER diagram of a tab (`tab`: a `diagram` tab or an object tab with a Diagram page; the active one when absent): `status` (`computing`, `ready`, `failed`, `missing`), its entities (name, schema, kind, path for db.object.open, box at 100 %, columns with their keys unless `columns: false`) from `offset`, at most `limit` (100, ≤ 1000) entities and relations, `total`, `relationTotal`, `truncated`, `partial` when some columns are not read yet, the diagram's size, the view (zoom in percent, scroll position), the selected entity and the latest export. `waitMs` waits for the layout or the export. Offline; never data values or secrets.";

    fn run(&self, session: &mut Session, params: DiagramStateParams) -> Result<DiagramState, CommandError> {
        state_of(session, &params)
    }
}

/// How to zoom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ZoomAction {
    /// The next larger step.
    In,
    /// The next smaller step.
    Out,
    /// 100 %.
    Reset,
    /// The whole diagram in the canvas.
    Fit,
}

/// A zoom change.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ZoomParams {
    /// The tab; the active one when absent.
    #[serde(default)]
    pub tab: Option<String>,
    /// `in`, `out`, `reset` (100 %) or `fit`.
    pub action: ZoomAction,
    /// The canvas's width in points; zooming keeps its centre in place, and `fit` fills it. 1200 when absent.
    #[serde(default)]
    pub width: Option<f32>,
    /// The canvas's height in points; 800 when absent.
    #[serde(default)]
    pub height: Option<f32>,
}

fn zoomed(view: View, zoom: u32, width: f32, height: f32) -> View {
    let (old, new) = (view.zoom.max(1) as f32 / 100.0, zoom as f32 / 100.0);
    let center = (view.x as f32 + width / 2.0 / old, view.y as f32 + height / 2.0 / old);
    View { zoom, x: (center.0 - width / 2.0 / new).round() as i32, y: (center.1 - height / 2.0 / new).round() as i32 }
}

pub fn fitted(size: [f32; 2], width: f32, height: f32) -> View {
    let margin = 24.0;
    let scale = ((width - 2.0 * margin) / size[0].max(1.0)).min((height - 2.0 * margin) / size[1].max(1.0));
    let zoom = (scale * 100.0).floor().clamp(ZOOMS[0] as f32, 100.0) as u32;
    let factor = zoom as f32 / 100.0;
    View { zoom, x: (size[0] / 2.0 - width / 2.0 / factor).round() as i32, y: (-margin / factor).round() as i32 }
}

pub struct ZoomDiagram;

impl Command for ZoomDiagram {
    type Params = ZoomParams;
    type Output = View;
    const ID: &'static str = "db.diagram.zoom";
    const TITLE: &'static str = "Zoom Database Diagram";
    const DESCRIPTION: &'static str = "Zooms a tab's ER diagram (`tab`, else the active one): `in` and `out` by steps from 10 % to 400 %, `reset` to 100 %, `fit` to show the whole diagram (at most 100 %). `width` and `height` are the canvas's size in points (1200 × 800 when absent): zooming keeps its centre in place. Answers the view, kept with the tab.";

    fn run(&self, session: &mut Session, params: ZoomParams) -> Result<View, CommandError> {
        let source = source(session, params.tab.as_deref())?;
        let (width, height) = (params.width.unwrap_or(1200.0).max(50.0), params.height.unwrap_or(800.0).max(50.0));
        let current = source.view;
        let view = match params.action {
            ZoomAction::In => {
                let next = ZOOMS.iter().copied().find(|zoom| *zoom > current.zoom).unwrap_or(ZOOMS[ZOOMS.len() - 1]);
                zoomed(current, next, width, height)
            }
            ZoomAction::Out => {
                let next = ZOOMS.iter().rev().copied().find(|zoom| *zoom < current.zoom).unwrap_or(ZOOMS[0]);
                zoomed(current, next, width, height)
            }
            ZoomAction::Reset => zoomed(current, 100, width, height),
            ZoomAction::Fit => {
                refresh(session, &source.tab)?;
                let size = session
                    .state_mut::<Diagrams>()
                    .shown(&source.tab)
                    .map(|built| [built.layout.width, built.layout.height])
                    .ok_or_else(|| CommandError::with_code("notReady", t!("database-diagram-error-not-ready")))?;
                fitted(size, width, height)
            }
        };
        set_view(session, &source, view)?;
        Ok(view)
    }
}

/// A view to show.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewParams {
    /// The tab; the active one when absent.
    pub tab: Option<String>,
    /// The zoom in percent (10 to 400); unchanged when absent.
    pub zoom: Option<u32>,
    /// The diagram's point at the canvas's left edge; unchanged when absent.
    pub x: Option<i32>,
    /// The diagram's point at the canvas's top edge; unchanged when absent.
    pub y: Option<i32>,
    /// Moves the view right by this many points of the diagram (left when negative).
    pub dx: Option<i32>,
    /// Moves the view down by this many points of the diagram (up when negative).
    pub dy: Option<i32>,
}

pub struct SetDiagramView;

impl Command for SetDiagramView {
    type Params = ViewParams;
    type Output = View;
    const ID: &'static str = "db.diagram.view";
    const TITLE: &'static str = "Scroll Database Diagram";
    const DESCRIPTION: &'static str = "Sets a tab's ER diagram view (`tab`, else the active one): `zoom` in percent (10 to 400), the diagram point at the canvas's top-left corner (`x`, `y`), or moves it by `dx`, `dy` (panning). Answers the view, kept with the tab.";

    fn run(&self, session: &mut Session, params: ViewParams) -> Result<View, CommandError> {
        let source = source(session, params.tab.as_deref())?;
        let mut view = source.view;
        if let Some(zoom) = params.zoom {
            view.zoom = zoom.clamp(ZOOMS[0], ZOOMS[ZOOMS.len() - 1]);
        }
        view.x = params.x.unwrap_or(view.x).saturating_add(params.dx.unwrap_or(0));
        view.y = params.y.unwrap_or(view.y).saturating_add(params.dy.unwrap_or(0));
        set_view(session, &source, view)?;
        Ok(view)
    }
}

/// An entity to select.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SelectParams {
    /// The tab; the active one when absent.
    #[serde(default)]
    pub tab: Option<String>,
    /// The entity: its name as db.diagram.state lists it, or `schema.name`; none clears the selection.
    #[serde(default)]
    pub entity: Option<String>,
    /// Whether to open its object tab too.
    #[serde(default)]
    pub open: bool,
}

/// An entity selected.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Selected {
    /// The entity selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity: Option<String>,
    /// Its path.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
    /// Its object tab, when opened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opened: Option<InnerTab>,
}

pub struct SelectEntity;

impl Command for SelectEntity {
    type Params = SelectParams;
    type Output = Selected;
    const ID: &'static str = "db.diagram.select";
    const TITLE: &'static str = "Select Diagram Entity";
    const DESCRIPTION: &'static str = "Selects an entity on a tab's ER diagram (`tab`, else the active one) by its name as db.diagram.state lists it, or `schema.name`; without `entity` clears the selection. `open: true` also opens its object tab (db.object.open), as a double click does. Code `unknownEntity` when the diagram has no such entity.";

    fn run(&self, session: &mut Session, params: SelectParams) -> Result<Selected, CommandError> {
        let source = source(session, params.tab.as_deref())?;
        refresh(session, &source.tab)?;
        let Some(name) = params.entity else {
            session.state_mut::<Diagrams>().selected.remove(&source.tab);
            return Ok(Selected { entity: None, path: None, opened: None });
        };
        let found = session.state::<Diagrams>().and_then(|diagrams| diagrams.jobs.get(&source.tab)).and_then(|job| {
            let at = job.diagram.find(&name)?;
            let entity = &job.diagram.entities[at];
            Some((entity.label(), entity.path.clone()))
        });
        let (label, path) = found.ok_or_else(|| {
            CommandError::with_code("unknownEntity", t!("database-diagram-error-entity", name = name.as_str()))
        })?;
        session.state_mut::<Diagrams>().selected.insert(source.tab.clone(), label.clone());
        let opened = if params.open {
            let params = OpenParams { connection: source.connection, path: path.clone(), page: None };
            Some(ObjectOpen.run(session, params)?.tab)
        } else {
            None
        };
        Ok(Selected { entity: Some(label), path: Some(path), opened })
    }
}

/// An export to make.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    /// The tab; the active one when absent.
    #[serde(default)]
    pub tab: Option<String>,
    /// `png`, `svg` or `mermaid`.
    pub format: Format,
    /// The file to write; needed for PNG. Without it SVG and Mermaid text are answered.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Whether to replace an existing file.
    #[serde(default)]
    pub overwrite: bool,
    /// Pixels per point of a PNG image, 1 to 4; 2 when absent (lowered for a very large diagram).
    #[serde(default)]
    pub scale: Option<f32>,
    /// The most characters of text answered; 20 000 when absent.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// An export made or started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    /// The format.
    pub format: Format,
    /// The text, when no file was asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Whether the text was cut at `limit`.
    pub truncated: bool,
    /// The file being written in the background; follow it with db.diagram.state's `export`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

fn renderer(session: &Session) -> Result<Arc<dyn Renderer>, CommandError> {
    session
        .state::<Diagrams>()
        .and_then(|diagrams| diagrams.renderer.clone())
        .ok_or_else(|| CommandError::with_code("noRenderer", t!("database-diagram-error-renderer")))
}

fn write_text(path: &Path, text: &str, cancel: &AtomicBool) -> Result<u64, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err(t!("database-diagram-cancelled"));
    }
    let partial = path.with_extension("part");
    std::fs::write(&partial, text).map_err(|error| error.to_string())?;
    std::fs::rename(&partial, path).map_err(|error| error.to_string())?;
    Ok(text.len() as u64)
}

pub struct ExportDiagram;

impl Command for ExportDiagram {
    type Params = ExportParams;
    type Output = Exported;
    const ID: &'static str = "db.diagram.export";
    const TITLE: &'static str = "Export Database Diagram";
    const DESCRIPTION: &'static str = "Exports a tab's ER diagram (`tab`, else the active one) as `png` (the canvas in the theme's colours, `scale` pixels per point, 2 by default), `svg` (in the theme's colours) or `mermaid` (`erDiagram` text with columns, keys and cardinalities). With `path` the file is written in the background (code `exists` unless `overwrite`; follow db.diagram.state's `export`); without it SVG and Mermaid text are answered, cut at `limit` characters (20 000) with `truncated`. PNG and SVG wait for the layout (code `notReady`).";

    fn run(&self, session: &mut Session, params: ExportParams) -> Result<Exported, CommandError> {
        let source = source(session, params.tab.as_deref())?;
        refresh(session, &source.tab)?;
        let (diagram, built) = {
            let diagrams = session.state_mut::<Diagrams>();
            let shown = diagrams.shown(&source.tab);
            let job = diagrams
                .jobs
                .get(&source.tab)
                .ok_or_else(|| CommandError::with_code("notReady", t!("database-diagram-error-not-ready")))?;
            let current = shown.filter(|built| *built.diagram == *job.diagram);
            (Arc::clone(&job.diagram), current)
        };
        if params.format != Format::Mermaid && built.is_none() {
            return Err(CommandError::with_code("notReady", t!("database-diagram-error-not-ready")));
        }
        let colors = match params.format {
            Format::Mermaid => None,
            _ => Some(renderer(session)?),
        };
        let Some(path) = params.path else {
            let text = match params.format {
                Format::Mermaid => mermaid(&diagram),
                Format::Svg => {
                    let built = built
                        .as_ref()
                        .ok_or_else(|| CommandError::with_code("notReady", t!("database-diagram-error-not-ready")))?;
                    svg(&diagram, &built.layout, &colors.as_ref().map(|renderer| renderer.colors()).unwrap_or_default())
                }
                Format::Png => {
                    return Err(CommandError::with_code("pathNeeded", t!("database-diagram-error-path-needed")));
                }
            };
            let limit = params.limit.unwrap_or(TEXT_LIMIT);
            let truncated = text.chars().count() > limit;
            let text = if truncated { text.chars().take(limit).collect() } else { text };
            return Ok(Exported { format: params.format, text: Some(text), truncated, path: None });
        };
        if path.is_relative() {
            return Err(CommandError::with_code("invalidPath", t!("database-diagram-error-relative")));
        }
        super::guard_agent_file(session, &path)?;
        if path.exists() && !params.overwrite {
            return Err(CommandError::with_code(
                "exists",
                t!("database-diagram-error-exists", path = path.display().to_string()),
            ));
        }
        let slot: Arc<Mutex<Option<Result<u64, String>>>> = Arc::new(Mutex::new(None));
        let cancel = Arc::new(AtomicBool::new(false));
        let waker = session.state::<Database>().and_then(Database::waker);
        let (into, file, format, stop) = (Arc::clone(&slot), path.clone(), params.format, Arc::clone(&cancel));
        let scale = params.scale.unwrap_or(2.0).clamp(1.0, 4.0);
        let thread = std::thread::Builder::new().name("zen-db-diagram-export".to_owned()).spawn(move || {
            let done = match (format, built) {
                (Format::Mermaid, _) => write_text(&file, &mermaid(&diagram), &stop),
                (Format::Svg, Some(built)) => {
                    let colors = colors.as_ref().map(|renderer| renderer.colors()).unwrap_or_default();
                    write_text(&file, &svg(&diagram, &built.layout, &colors), &stop)
                }
                (Format::Png, Some(built)) => {
                    let area = (built.layout.width + 32.0) * (built.layout.height + 32.0);
                    let scale = scale.min((MOST_PIXELS / area.max(1.0)).sqrt()).max(0.1);
                    colors
                        .filter(|_| !stop.load(Ordering::Relaxed))
                        .ok_or_else(|| t!("database-diagram-cancelled"))
                        .and_then(|renderer| renderer.png(&diagram, &built.layout, scale, &file))
                        .and_then(|()| {
                            std::fs::metadata(&file).map(|meta| meta.len()).map_err(|error| error.to_string())
                        })
                }
                _ => Err(t!("database-diagram-error-not-ready")),
            };
            if let Ok(mut slot) = into.lock() {
                *slot = Some(done);
            }
            if let Some(waker) = waker {
                waker();
            }
        });
        thread.map_err(|error| CommandError::new(error.to_string()))?;
        let export = ExportJob { format: params.format, path: path.clone(), slot, cancel };
        if let Some(previous) = session.state_mut::<Diagrams>().exports.insert(source.tab, export) {
            previous.cancel.store(true, Ordering::Relaxed);
        }
        Ok(Exported { format: params.format, text: None, truncated: false, path: Some(path) })
    }
}

/// Which diagram's work to cancel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CancelParams {
    /// The tab; the active one when absent.
    pub tab: Option<String>,
}

/// What was cancelled.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Whether a layout or an export in flight was stopped.
    pub cancelled: bool,
}

pub struct CancelDiagram;

impl Command for CancelDiagram {
    type Params = CancelParams;
    type Output = Cancelled;
    const ID: &'static str = "db.diagram.cancel";
    const TITLE: &'static str = "Cancel Database Diagram Layout";
    const DESCRIPTION: &'static str = "Stops laying out a tab's ER diagram (`tab`, else the active one) and its export in flight (a file not yet written is not written); the diagram shown before stays, and db.diagram.open or the next structure change lays it out again. Answers whether something was running.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<Cancelled, CommandError> {
        let source = source(session, params.tab.as_deref())?;
        let diagrams = session.state_mut::<Diagrams>();
        let mut running = diagrams.computing(&source.tab);
        if let Some(job) = diagrams.jobs.get(&source.tab) {
            job.cancel.store(true, Ordering::Relaxed);
        }
        if let Some(export) = diagrams.exports.get(&source.tab)
            && export.slot.lock().is_ok_and(|slot| slot.is_none())
        {
            export.cancel.store(true, Ordering::Relaxed);
            running = true;
        }
        Ok(Cancelled { cancelled: running })
    }
}

#[cfg(test)]
mod tests;

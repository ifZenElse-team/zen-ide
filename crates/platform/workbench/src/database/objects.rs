use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{ConnectionState, OperationStatus, QueryOptions, QueryStatement};
use zen_db::registry::Entry;
use zen_db::schema::{
    ConstraintKind, DdlOptions, EVENT_TRIGGERS, EXTENSIONS, Found, Object, ObjectKind, ROLES, SCHEMA_INDEXES, Schema,
    Snapshot, TABLESPACES, find_object, qualified, qualified_for, quote_for, references,
};
use zen_i18n::t;

use super::WORKSPACE;
use super::connections::describe;
use super::consoles::{engine, ensure_connected};
use super::diagrams::View;
use super::sources::{Database, connection_types, find};
use super::structure::{PathParams, Structure, details, ensure, open_own_tree};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};

pub const KIND: &str = "object";

const LISTED: usize = 200;

/// A page of an object editor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Page {
    /// The rows of a table or view.
    #[default]
    Data,
    /// The header and the property sections.
    Properties,
    /// The generated DDL.
    Ddl,
    /// The ER diagram of the object and its neighbours.
    Diagram,
}

/// What an object tab keeps in `workspace.json`: never data values or secrets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ObjectTab {
    /// The connection's id.
    pub connection: String,
    /// The object's path in the connection's tree.
    pub path: Vec<String>,
    /// The page shown.
    pub page: Page,
    /// The property section shown, when one was chosen.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// The DDL page's toggles.
    pub ddl: DdlOptions,
    /// The Diagram page's zoom and scroll position.
    #[serde(skip_serializing_if = "View::is_default")]
    pub diagram: View,
}

#[derive(Clone, Debug)]
struct Read {
    query: u64,
    result: Option<u64>,
    previous: Option<u64>,
    error: Option<String>,
    statement: String,
}

#[derive(Default)]
pub struct Objects {
    reads: HashMap<String, Read>,
}

impl Objects {
    pub fn tab_of(&self, result: u64) -> Option<&str> {
        self.reads
            .iter()
            .find(|(_, read)| read.result == Some(result) || (read.result.is_none() && read.previous == Some(result)))
            .map(|(tab, _)| tab.as_str())
    }

    pub fn shown(&self, tab: &str) -> Option<u64> {
        self.reads.get(tab).and_then(|read| read.result.or(read.previous))
    }

    pub fn reading(&self, tab: &str) -> Option<u64> {
        self.reads.get(tab).filter(|read| read.result.is_none() && read.error.is_none()).map(|read| read.query)
    }

    pub fn failed(&self, tab: &str) -> Option<&str> {
        self.reads.get(tab).and_then(|read| read.error.as_deref())
    }

    pub fn rereading(&self, result: u64) -> bool {
        self.reads.values().any(|read| read.result.is_none() && read.error.is_none() && read.previous == Some(result))
    }
}

fn escape(segment: &str) -> String {
    segment.replace('%', "%25").replace('/', "%2F")
}

pub fn tab_id(connection: &str, path: &[String]) -> String {
    let path: Vec<String> = path.iter().map(|segment| escape(segment)).collect();
    format!("{KIND}:{}:{}", escape(connection), path.join("/"))
}

pub(super) fn kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Table => "table",
        ObjectKind::ForeignTable => "foreignTable",
        ObjectKind::View => "view",
        ObjectKind::MaterializedView => "materializedView",
        ObjectKind::Function => "function",
        ObjectKind::Procedure => "procedure",
        ObjectKind::Sequence => "sequence",
        ObjectKind::Type => "type",
        ObjectKind::Event => "event",
        ObjectKind::Package => "package",
    }
}

pub(super) fn has_data(kind: &str) -> bool {
    matches!(kind, "table" | "foreignTable" | "view" | "materializedView")
}

pub fn pages(kind: &str) -> Vec<Page> {
    if has_data(kind) {
        vec![Page::Properties, Page::Data, Page::Ddl, Page::Diagram]
    } else if ITEMS.contains(&kind) {
        vec![Page::Properties]
    } else {
        vec![Page::Properties, Page::Ddl]
    }
}

pub fn sections(kind: &str) -> &'static [&'static str] {
    match kind {
        "table" => &[
            "columns",
            "constraints",
            "foreignKeys",
            "indexes",
            "triggers",
            "references",
            "dependencies",
            "rules",
            "policies",
            "partitions",
            "statistics",
            "permissions",
        ],
        "foreignTable" => &["columns", "constraints", "triggers", "statistics", "permissions"],
        "view" => &["columns", "source", "dependencies", "rules", "triggers", "permissions"],
        "materializedView" => &["columns", "source", "indexes", "dependencies", "statistics", "permissions"],
        "function" | "procedure" => &["source", "permissions"],
        "event" => &["source"],
        "sequence" => &["settings", "permissions"],
        "type" => &["settings", "columns"],
        "schema" => &["objects"],
        "index" | "extension" | "eventTrigger" | "tablespace" | "role" => &["settings"],
        _ => &[],
    }
}

pub fn applies(driver: &str, part: &str) -> bool {
    let missing: &[&str] = match driver {
        "sqlite" | "duckdb" => {
            &["owner", "tablespace", "policies", "partitions", "permissions", "dependencies", "rules"]
        }
        "mysql" | "mariadb" => &["owner", "policies", "dependencies", "rules"],
        "clickhouse" => &["tablespace", "policies", "permissions", "dependencies", "rules"],
        _ => &[],
    };
    !missing.contains(&part)
}

fn section_title(section: &str) -> String {
    match section {
        "columns" => t!("database-object-section-columns"),
        "constraints" => t!("database-object-section-constraints"),
        "foreignKeys" => t!("database-object-section-foreign-keys"),
        "indexes" => t!("database-object-section-indexes"),
        "triggers" => t!("database-object-section-triggers"),
        "references" => t!("database-object-section-references"),
        "policies" => t!("database-object-section-policies"),
        "partitions" => t!("database-object-section-partitions"),
        "statistics" => t!("database-object-section-statistics"),
        "permissions" => t!("database-object-section-permissions"),
        "source" => t!("database-object-section-source"),
        "settings" => t!("database-object-section-settings"),
        "objects" => t!("database-object-section-objects"),
        "dependencies" => t!("database-object-section-dependencies"),
        "rules" => t!("database-object-section-rules"),
        other => other.to_owned(),
    }
}

const ITEMS: [&str; 5] = ["index", "extension", "eventTrigger", "tablespace", "role"];

struct Item {
    kind: &'static str,
    name: String,
    qualified: String,
    owner: Option<String>,
    comment: Option<String>,
    settings: Vec<Vec<String>>,
}

enum Target<'a> {
    Object(Found<'a>),
    Schema(&'a Schema),
    Item(Item),
}

impl Target<'_> {
    fn kind(&self) -> &'static str {
        match self {
            Target::Object(found) => kind_name(found.object.kind),
            Target::Schema(_) => "schema",
            Target::Item(item) => item.kind,
        }
    }

    fn name(&self) -> String {
        match self {
            Target::Object(found) => found.object.name.clone(),
            Target::Schema(schema) => schema.name.clone(),
            Target::Item(item) => item.name.clone(),
        }
    }

    fn qualified(&self, driver: &str) -> String {
        match self {
            Target::Object(found) => qualified_for(driver, &found.schema.name, &found.object.name),
            Target::Schema(schema) => quote_for(driver, &schema.name),
            Target::Item(item) => item.qualified.clone(),
        }
    }
}

fn target<'a>(snapshot: &'a Snapshot, path: &'a [String]) -> Option<Target<'a>> {
    match path {
        [catalog, folder, name] if folder.starts_with('#') => database_item(snapshot, catalog, folder, name),
        [catalog, schema] => snapshot.schema(catalog, schema).map(Target::Schema),
        [catalog, schema, folder, name] if folder == SCHEMA_INDEXES => index_item(snapshot, catalog, schema, name),
        [_, _, _, _] => find_object(snapshot, path).map(Target::Object),
        _ => None,
    }
}

fn flag_text(flag: bool) -> String {
    if flag { t!("database-object-yes") } else { t!("database-object-no") }
}

fn index_item<'a>(snapshot: &'a Snapshot, catalog: &str, schema: &str, name: &str) -> Option<Target<'a>> {
    let schema = snapshot.schema(catalog, schema)?;
    let (table, index) = schema
        .objects
        .iter()
        .find_map(|object| object.indexes.iter().find(|index| index.name == name).map(|index| (object, index)))?;
    let settings = vec![
        property(&t!("database-object-column-table"), qualified(&schema.name, &table.name)),
        property(&t!("database-object-column-columns"), index.columns.join(", ")),
        property(&t!("database-object-column-unique"), flag_text(index.unique)),
        property(&t!("database-object-primary"), flag_text(index.primary)),
        property(&t!("database-object-column-method"), index.method.clone()),
        property(&t!("database-object-column-size"), index.size.map(size_text).unwrap_or_default()),
        property(&t!("database-object-column-constraint"), index.constraint.clone().unwrap_or_default()),
        property(&t!("database-object-column-definition"), index.definition.clone()),
    ];
    Some(Target::Item(Item {
        kind: "index",
        name: index.name.clone(),
        qualified: qualified_for(&snapshot.driver, &schema.name, &index.name),
        owner: table.owner.clone(),
        comment: index.comment.clone(),
        settings,
    }))
}

fn database_item<'a>(snapshot: &'a Snapshot, catalog: &str, folder: &str, name: &str) -> Option<Target<'a>> {
    let catalog = snapshot.catalog(catalog)?;
    let item = |kind: &'static str, owner: Option<String>, comment: Option<String>, settings: Vec<Vec<String>>| {
        Target::Item(Item {
            kind,
            name: name.to_owned(),
            qualified: quote_for(&snapshot.driver, name),
            owner,
            comment,
            settings,
        })
    };
    match folder {
        EXTENSIONS => {
            let known = catalog.extensions.iter().find(|known| known.name == name)?;
            Some(item(
                "extension",
                None,
                known.comment.clone(),
                vec![
                    property(&t!("database-object-version"), known.version.clone()),
                    property(&t!("database-object-column-schema"), known.schema.clone()),
                    property(&t!("database-object-relocatable"), flag_text(known.relocatable)),
                ],
            ))
        }
        EVENT_TRIGGERS => {
            let known = catalog.event_triggers.iter().find(|known| known.name == name)?;
            Some(item(
                "eventTrigger",
                known.owner.clone(),
                known.comment.clone(),
                vec![
                    property(&t!("database-object-event"), known.event.clone()),
                    property(&t!("database-object-column-function"), known.function.clone()),
                    property(&t!("database-object-column-enabled"), known.enabled.clone()),
                    property(&t!("database-object-tags"), known.tags.join(", ")),
                ],
            ))
        }
        TABLESPACES => {
            let known = catalog.tablespaces.iter().find(|known| known.name == name)?;
            Some(item(
                "tablespace",
                known.owner.clone(),
                known.comment.clone(),
                vec![
                    property(&t!("database-object-location"), known.location.clone()),
                    property(&t!("database-object-column-size"), known.size.map(size_text).unwrap_or_default()),
                    property(&t!("database-object-options"), known.options.join(", ")),
                ],
            ))
        }
        ROLES => {
            let known = catalog.roles.iter().find(|known| known.name == name)?;
            let limit = if known.connection_limit < 0 {
                t!("database-object-no-limit")
            } else {
                known.connection_limit.to_string()
            };
            Some(item(
                "role",
                None,
                known.comment.clone(),
                vec![
                    property(&t!("database-object-login"), flag_text(known.login)),
                    property(&t!("database-object-superuser"), flag_text(known.superuser)),
                    property(&t!("database-object-create-database"), flag_text(known.create_database)),
                    property(&t!("database-object-create-role"), flag_text(known.create_role)),
                    property(&t!("database-object-replication"), flag_text(known.replication)),
                    property(&t!("database-object-bypass-row-security"), flag_text(known.bypass_row_security)),
                    property(&t!("database-object-connection-limit"), limit),
                    property(&t!("database-object-valid-until"), known.valid_until.clone().unwrap_or_default()),
                    property(&t!("database-object-member-of"), known.member_of.join(", ")),
                ],
            ))
        }
        _ => None,
    }
}

fn unknown(connection: &str, path: &[String]) -> CommandError {
    CommandError::with_code(
        "unknownObject",
        t!("database-error-unknown-object", path = path.join("/"), connection = connection),
    )
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn snapshot_ref<'a>(session: &'a mut Session, entry: &Entry, wait: bool) -> Option<&'a Snapshot> {
    ensure(session, entry, wait);
    session.state::<Structure>().and_then(|structure| structure.snapshot(&entry.source.id))
}

fn tab_data(tab: &InnerTab) -> Option<ObjectTab> {
    (tab.kind == KIND).then(|| serde_json::from_value(tab.data.clone()).ok()).flatten()
}

fn no_object_tab(session: &Session) -> CommandError {
    let message =
        super::for_caller(session, t!("database-error-no-object-tab"), t!("database-error-no-object-tab-agent"));
    CommandError::with_code("noObjectTab", message)
}

pub(super) fn object_tab(session: &Session, tab: Option<&str>) -> Result<(InnerTab, ObjectTab), CommandError> {
    let workspaces = session.state::<Workspaces>();
    let found = match tab {
        Some(id) => workspaces.and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|known| known.id == id)),
        None => workspaces.and_then(|workspaces| workspaces.active_tab(WORKSPACE)),
    };
    let found = found.filter(|known| known.kind == KIND).cloned();
    let data = found.as_ref().and_then(tab_data);
    match (found, data) {
        (Some(found), Some(data)) => Ok((found, data)),
        _ => Err(no_object_tab(session)),
    }
}

fn keep(session: &mut Session, tab: &InnerTab, data: &ObjectTab) -> Result<InnerTab, CommandError> {
    let data = serde_json::to_value(data).unwrap_or_default();
    session.state_mut::<Workspaces>().set_kind_data(WORKSPACE, &tab.id, data);
    session
        .state::<Workspaces>()
        .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|known| known.id == tab.id).cloned())
        .ok_or_else(|| no_object_tab(session))
}

pub fn select_statement(driver: &str, found: &Found<'_>) -> String {
    format!("SELECT * FROM {}", table_source(driver, found))
}

pub fn table_source(driver: &str, found: &Found<'_>) -> String {
    qualified_for(driver, &found.schema.name, &found.object.name)
}

fn connected(session: &Session, connection: &str) -> bool {
    session.state::<Database>().and_then(Database::started).is_some_and(|engine| {
        matches!(
            engine.status(connection).state,
            ConnectionState::Connected | ConnectionState::Closed | ConnectionState::Connecting
        )
    })
}

/// What reading a tab's data started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataRead {
    /// The tab.
    pub tab: String,
    /// The connection.
    pub connection: String,
    /// The statement it reads with.
    pub statement: String,
    /// The query reading it, for `db.query.state`; absent when nothing started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// Whether the connection was connected first.
    pub connecting: bool,
}

pub(super) fn start_read(
    session: &mut Session,
    tab: &str,
    data: &ObjectTab,
    connect: bool,
) -> Result<DataRead, CommandError> {
    refuse_pending_changes(session, tab)?;
    let entry = find(session, &data.connection)?;
    let dialect = super::filters::dialect_of(session, &data.connection).unwrap_or_default();
    let (name, columns) = {
        let snapshot = snapshot_ref(session, &entry, true).ok_or_else(|| unknown(&data.connection, &data.path))?;
        let found = find_object(snapshot, &data.path).ok_or_else(|| unknown(&data.connection, &data.path))?;
        if !has_data(kind_name(found.object.kind)) {
            return Err(CommandError::with_code(
                "noData",
                t!("database-error-object-no-data", name = found.object.name.as_str()),
            ));
        }
        let name = table_source(&snapshot.driver, &found);
        let columns: Vec<String> = found.object.columns.iter().map(|column| column.name.clone()).collect();
        (name, columns)
    };
    let statement = super::filters::table_statement(session, tab, (&name, None), &columns, dialect)?;
    let nothing = DataRead {
        tab: tab.to_owned(),
        connection: data.connection.clone(),
        statement: statement.clone(),
        query: None,
        connecting: false,
    };
    if !connect && !connected(session, &data.connection) {
        return Ok(nothing);
    }
    let connecting = ensure_connected(session, &data.connection)?;
    let engine = engine(session);
    let source = if super::filters::filtered(session, tab) {
        zen_db::history::Source::Filter
    } else {
        zen_db::history::Source::Data
    };
    let entry = find(session, &data.connection)?;
    super::safety::agent_reads(session, &entry, &statement)?;
    let provenance = super::history::provenance(session, source, data.path.last().cloned());
    let options = QueryOptions {
        stop_on_error: true,
        rollback_on_error: false,
        timeout: None,
        schema: None,
        provenance,
        read_only: false,
        notes: Vec::new(),
    };
    let query_statement = QueryStatement {
        text: statement.clone(),
        start: 0,
        end: statement.chars().count(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let query = engine
        .run_query(&data.connection, ContextKind::Main, vec![query_statement], options)
        .map_err(|error| failed(&error))?;
    let objects = session.state_mut::<Objects>();
    let previous = objects.reads.remove(tab).and_then(|read| {
        if read.result.is_none() && read.error.is_none() {
            engine.cancel_query(read.query);
            let started = engine.query(read.query).and_then(|query| query.statements.first()?.result);
            if let Some(started) = started {
                engine.results().remove(started);
            }
        }
        if let (Some(_), Some(older)) = (read.result, read.previous) {
            engine.results().remove(older);
        }
        read.result.or(read.previous)
    });
    objects
        .reads
        .insert(tab.to_owned(), Read { query, result: None, previous, error: None, statement: statement.clone() });
    Ok(DataRead { query: Some(query), connecting, ..nothing })
}

pub(super) fn reread_table(session: &mut Session, connection: &str, path: &[String]) {
    let id = tab_id(connection, path);
    if session.state::<Objects>().is_none_or(|objects| !objects.reads.contains_key(&id)) {
        return;
    }
    if let Ok((tab, data)) = object_tab(session, Some(&id)) {
        let _ = start_read(session, &tab.id, &data, false);
    }
}

fn refuse_pending_changes(session: &Session, tab: &str) -> Result<(), CommandError> {
    let shown: Vec<u64> = session.state::<Objects>().and_then(|objects| objects.shown(tab)).into_iter().collect();
    super::edits::refuse_staged(session, &shown, (ReadData::ID, serde_json::json!({ "tab": tab })))
}

pub fn results_of(session: &Session, tab: &str) -> Vec<u64> {
    session.state::<Objects>().and_then(|objects| objects.shown(tab)).into_iter().collect()
}

pub fn step(session: &mut Session) {
    let open: Vec<String> = session
        .state::<Workspaces>()
        .map(|workspaces| workspaces.tabs(WORKSPACE).iter().map(|tab| tab.id.clone()).collect())
        .unwrap_or_default();
    let engine = session.state::<Database>().and_then(Database::started).cloned();
    if session.state::<Objects>().is_none_or(|objects| objects.reads.is_empty()) {
        return;
    }
    let objects = session.state_mut::<Objects>();
    let closed: Vec<String> = objects.reads.keys().filter(|tab| !open.contains(tab)).cloned().collect();
    for tab in closed {
        if let (Some(read), Some(engine)) = (objects.reads.remove(&tab), &engine) {
            for result in [read.result, read.previous].into_iter().flatten() {
                engine.results().remove(result);
            }
        }
    }
    let Some(engine) = engine else { return };
    for read in objects.reads.values_mut().filter(|read| read.result.is_none() && read.error.is_none()) {
        let Some(query) = engine.query(read.query) else {
            read.error = Some(t!("database-error-cancelled"));
            continue;
        };
        let statement = query.statements.first();
        if let Some(result) =
            statement.and_then(|statement| statement.result).filter(|result| columns_known(&engine, *result))
        {
            read.result = Some(result);
            if let Some(previous) = read.previous.take() {
                engine.results().remove(previous);
            }
        } else if query.status != OperationStatus::Running {
            let error = statement.and_then(|statement| statement.error.as_ref()).or(query.error.as_ref());
            read.error = Some(super::connections::unread(query.status, error));
        }
    }
}

pub(super) fn columns_known(engine: &zen_db::engine::Engine, result: u64) -> bool {
    engine.results().get(result).is_some_and(|set| !set.columns().is_empty())
}

pub fn reread(session: &mut Session, result: u64) -> Option<Result<DataRead, CommandError>> {
    let tab = session.state::<Objects>()?.tab_of(result)?.to_owned();
    Some(object_tab(session, Some(&tab)).and_then(|(tab, data)| start_read(session, &tab.id, &data, true)))
}

pub(super) fn read_tab(session: &mut Session, tab: &str) -> Result<DataRead, CommandError> {
    let (tab, data) = object_tab(session, Some(tab))?;
    start_read(session, &tab.id, &data, true)
}

pub fn data_view(session: &Session, result: u64) -> Option<(String, ObjectTab)> {
    let tab = session.state::<Objects>()?.tab_of(result)?.to_owned();
    let (found, data) = object_tab(session, Some(&tab)).ok()?;
    Some((found.id, data))
}

pub fn object_of(session: &mut Session, data: &ObjectTab) -> Option<(String, Object)> {
    let entry = find(session, &data.connection).ok()?;
    let snapshot = snapshot_ref(session, &entry, false)?;
    let found = find_object(snapshot, &data.path)?;
    Some((found.schema.name.clone(), found.object.clone()))
}

/// Where to open an object.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenParams {
    /// The connection's id.
    pub connection: String,
    /// The object's path: `[db, schema, folder, object]`, or `[db, schema]` for a schema.
    pub path: Vec<String>,
    /// The page to show; Data for tables and views, Properties for the rest, when absent.
    pub page: Option<Page>,
}

/// An object tab opened.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenedObject {
    /// The tab.
    pub tab: InnerTab,
    /// What the object is: `table`, `view`, `materializedView`, `foreignTable`, `function`, `procedure`, `sequence`, `type`, `event` or `schema`; for a driver's own tree the node's kind (`key`, `table`…).
    pub kind: String,
    /// The page shown.
    pub page: Page,
    /// The query reading its data, when the Data page is shown and the connection is connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// The console's file, when a node of a driver's own tree opened in a console.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<std::path::PathBuf>,
    /// The text that console was opened with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

pub struct ObjectOpen;

impl Command for ObjectOpen {
    type Params = OpenParams;
    type Output = OpenedObject;
    const ID: &'static str = "db.object.open";
    const TITLE: &'static str = "Open Database Object";
    const DESCRIPTION: &'static str = "Opens the database object at `path` of connection `connection` (`[db, schema, folder, object]` as db.schema.children gives them, or `[db, schema]` for a schema) as a tab of the Database workspace, or brings its tab to front, and expands the workspace. In a driver's own tree (Redis, ClickHouse) a key opens its value tab (kind `key`, db.key.open) and a table or view a new console with the driver's `SELECT` (answering `path` and `text`). A table, view, materialised view or foreign table opens on its Data page, the rest on Properties (`page`: `data`, `properties`, `ddl`, `diagram` chooses). On Data, the rows are read in the background from the table ordered by its key, in the connection's Main context, when the connection is connected; never connects (db.object.readData does). The tab is kept across restarts without data.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<OpenedObject, CommandError> {
        let entry = find(session, &params.connection)?;
        let own = PathParams { connection: params.connection.clone(), path: params.path.clone(), limit: None };
        if let Some(opened) = open_own_tree(session, &entry, &own)? {
            let (path, text) = opened.console.unzip();
            return Ok(OpenedObject { tab: opened.tab, kind: opened.kind, page: Page::Data, query: None, path, text });
        }
        let (kind, title) = {
            let snapshot =
                snapshot_ref(session, &entry, true).ok_or_else(|| unknown(&params.connection, &params.path))?;
            let found = target(snapshot, &params.path).ok_or_else(|| unknown(&params.connection, &params.path))?;
            (found.kind(), found.name())
        };
        let offered = pages(kind);
        let id = tab_id(&entry.source.id, &params.path);
        let existing = session
            .state::<Workspaces>()
            .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|tab| tab.id == id))
            .and_then(tab_data);
        let mut data = existing.unwrap_or_else(|| ObjectTab {
            connection: entry.source.id.clone(),
            path: params.path.clone(),
            page: if has_data(kind) { Page::Data } else { Page::Properties },
            ..ObjectTab::default()
        });
        if let Some(page) = params.page {
            if !offered.contains(&page) {
                return Err(CommandError::with_code(
                    "invalidPage",
                    t!("database-error-object-page", page = format!("{page:?}").to_lowercase()),
                ));
            }
            data.page = page;
        }
        let open = OpenTabParams {
            workspace: WORKSPACE.to_owned(),
            group: None,
            id: Some(id.clone()),
            kind: Some(KIND.to_owned()),
            title: Some(title),
            closable: Some(true),
            data: Some(serde_json::to_value(&data).unwrap_or_default()),
            path: None,
        };
        let tab = OpenTab.run(session, open)?;
        let unread = session.state::<Objects>().is_none_or(|objects| !objects.reads.contains_key(&id));
        let query =
            if data.page == Page::Data && unread { start_read(session, &id, &data, false)?.query } else { None };
        Ok(OpenedObject { tab, kind: kind.to_owned(), page: data.page, query, path: None, text: None })
    }
}

/// Which object tab.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TabParams {
    /// The tab's id (`object:<connection>:<path>`); the active tab of the Database workspace when absent.
    pub tab: Option<String>,
}

/// Where a tab's Data page is.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataState {
    /// The statement the rows are read with.
    pub statement: String,
    /// The query reading them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// The result holding them, for `db.result.page`, `db.result.value` and the rest.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<u64>,
    /// Whether they are being read: the query has not answered, or rows are still arriving.
    pub reading: bool,
    /// Why reading them failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// An object tab as views and agents read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ObjectState {
    /// The tab's id.
    pub tab: String,
    /// The connection's id.
    pub connection: String,
    /// The connection's name.
    pub connection_name: String,
    /// The connection type's colour, a theme colour name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The connection's state: `disconnected`, `connecting`, `connected`, `closed` or `failed`.
    pub state: ConnectionState,
    /// The object's path.
    pub path: Vec<String>,
    /// What the object is; absent when the snapshot no longer has it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Its name.
    pub name: String,
    /// Its name qualified and quoted as SQL needs it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualified: Option<String>,
    /// The page shown.
    pub page: Page,
    /// The pages it has.
    pub pages: Vec<Page>,
    /// The property section shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub section: Option<String>,
    /// The DDL page's toggles.
    pub ddl: DdlOptions,
    /// The Data page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DataState>,
}

fn state_of(session: &mut Session, tab: &InnerTab, data: &ObjectTab) -> ObjectState {
    step(session);
    let entry = find(session, &data.connection).ok();
    let (types, _) = connection_types(session);
    let color = entry
        .as_ref()
        .map(|entry| types.resolve(&entry.source.connection_type).color.clone())
        .filter(|color| color != "none");
    let state = session
        .state::<Database>()
        .and_then(Database::started)
        .map_or(ConnectionState::Disconnected, |engine| engine.status(&data.connection).state);
    let known = entry.as_ref().and_then(|entry| {
        let snapshot = snapshot_ref(session, entry, false)?;
        let found = target(snapshot, &data.path)?;
        let statement = match &found {
            Target::Object(found) if has_data(kind_name(found.object.kind)) => {
                Some(select_statement(&snapshot.driver, found))
            }
            _ => None,
        };
        Some((found.kind(), found.qualified(&snapshot.driver), statement))
    });
    let read = session.state::<Objects>().and_then(|objects| objects.reads.get(&tab.id)).cloned();
    let rows_coming = read.as_ref().and_then(|read| read.result).is_some_and(|result| {
        session
            .state::<Database>()
            .and_then(Database::started)
            .and_then(|engine| engine.results().get(result))
            .is_some_and(|set| set.info().state == zen_db::results::ReadState::Reading)
    });
    let data_state = known.as_ref().and_then(|(_, _, statement)| statement.clone()).map(|statement| match read {
        Some(read) => DataState {
            statement: read.statement,
            query: Some(read.query),
            result: read.result.or(read.previous),
            reading: (read.result.is_none() && read.error.is_none()) || rows_coming,
            error: read.error,
        },
        None => DataState { statement, ..DataState::default() },
    });
    let kind = known.as_ref().map(|(kind, _, _)| (*kind).to_owned());
    ObjectState {
        tab: tab.id.clone(),
        connection: data.connection.clone(),
        connection_name: entry.map_or_else(|| data.connection.clone(), |entry| entry.source.name),
        color,
        state,
        path: data.path.clone(),
        pages: kind.as_deref().map(pages).unwrap_or_default(),
        kind,
        name: tab.title.clone(),
        qualified: known.map(|(_, qualified, _)| qualified),
        page: data.page,
        section: data.section.clone(),
        ddl: data.ddl.clone(),
        data: data_state,
    }
}

pub struct ObjectStateCommand;

impl Command for ObjectStateCommand {
    type Params = TabParams;
    type Output = ObjectState;
    const ID: &'static str = "db.object.state";
    const TITLE: &'static str = "Database Object Tab";
    const DESCRIPTION: &'static str = "An object tab of the Database workspace (`tab`, else the active one): its connection with its state and colour, the object's path, kind and qualified name, the page shown and the pages it has, the property section, the DDL toggles, and for tables and views the Data page: the statement, the query, the `result` (read rows with db.result.page and values with db.result.value), whether it is reading, and the error. Never data values.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<ObjectState, CommandError> {
        let (tab, data) = object_tab(session, params.tab.as_deref())?;
        Ok(state_of(session, &tab, &data))
    }
}

/// A page to show.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PageParams {
    /// The tab's id; the active tab of the Database workspace when absent.
    #[serde(default)]
    pub tab: Option<String>,
    /// `data`, `properties`, `ddl` or `diagram`.
    pub page: Page,
}

pub struct ShowPage;

impl Command for ShowPage {
    type Params = PageParams;
    type Output = ObjectState;
    const ID: &'static str = "db.object.showPage";
    const TITLE: &'static str = "Show Object Page";
    const DESCRIPTION: &'static str = "Shows a page of an object tab (`tab`, else the active one): `data`, `properties`, `ddl` or `diagram`, among those its kind has (refused with code `invalidPage` otherwise). Showing Data for the first time reads the rows when the connection is connected; never connects. Answers the tab's state.";

    fn run(&self, session: &mut Session, params: PageParams) -> Result<ObjectState, CommandError> {
        let (tab, mut data) = object_tab(session, params.tab.as_deref())?;
        let state = state_of(session, &tab, &data);
        if !state.pages.contains(&params.page) {
            return Err(CommandError::with_code(
                "invalidPage",
                t!("database-error-object-page", page = format!("{:?}", params.page).to_lowercase()),
            ));
        }
        data.page = params.page;
        let tab = keep(session, &tab, &data)?;
        let unread = session.state::<Objects>().is_none_or(|objects| !objects.reads.contains_key(&tab.id));
        if data.page == Page::Data && unread {
            start_read(session, &tab.id, &data, false)?;
        }
        Ok(state_of(session, &tab, &data))
    }
}

/// A property section to show.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SectionParams {
    /// The tab's id; the active tab of the Database workspace when absent.
    #[serde(default)]
    pub tab: Option<String>,
    /// The section: `columns`, `constraints`, `foreignKeys`, `indexes`, `triggers`, `references`, `policies`, `partitions`, `statistics`, `permissions`, `source`, `settings` or `objects`, among those of the object's kind.
    pub section: String,
}

pub struct ShowSection;

impl Command for ShowSection {
    type Params = SectionParams;
    type Output = ObjectState;
    const ID: &'static str = "db.object.showSection";
    const TITLE: &'static str = "Show Object Properties Section";
    const DESCRIPTION: &'static str = "Shows the Properties page of an object tab (`tab`, else the active one) on `section`, one of the sections its kind has (db.object.properties lists them; refused with code `invalidSection` otherwise). Answers the tab's state.";

    fn run(&self, session: &mut Session, params: SectionParams) -> Result<ObjectState, CommandError> {
        let (tab, mut data) = object_tab(session, params.tab.as_deref())?;
        let state = state_of(session, &tab, &data);
        let offered = state.kind.as_deref().map(sections).unwrap_or_default();
        if !offered.contains(&params.section.as_str()) {
            return Err(CommandError::with_code(
                "invalidSection",
                t!("database-error-object-section", section = params.section.as_str(), sections = offered.join(", ")),
            ));
        }
        data.page = Page::Properties;
        data.section = Some(params.section);
        let tab = keep(session, &tab, &data)?;
        Ok(state_of(session, &tab, &data))
    }
}

/// The DDL toggles to set.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DdlOptionsParams {
    /// The tab's id; the active tab of the Database workspace when absent.
    pub tab: Option<String>,
    /// A table's indexes, triggers and policies.
    pub nested: Option<bool>,
    /// GRANT statements.
    pub permissions: Option<bool>,
    /// Foreign keys as ALTER TABLE after the table.
    pub separate_foreign_keys: Option<bool>,
    /// Primary and unique keys as ALTER TABLE after the table.
    pub separate_constraint_indexes: Option<bool>,
    /// COMMENT ON statements.
    pub comments: Option<bool>,
    /// A partitioned table's partitions.
    pub partitions: Option<bool>,
}

/// The DDL page of a tab.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DdlPage {
    /// The tab's id.
    pub tab: String,
    /// The toggles in effect.
    pub options: DdlOptions,
}

pub struct SetDdlOptions;

impl Command for SetDdlOptions {
    type Params = DdlOptionsParams;
    type Output = DdlPage;
    const ID: &'static str = "db.object.ddlOptions";
    const TITLE: &'static str = "Object DDL Options";
    const DESCRIPTION: &'static str = "Sets the toggles of an object tab's DDL page (`tab`, else the active one), kept with the tab: `nested`, `permissions`, `separateForeignKeys`, `separateConstraintIndexes`, `comments`, `partitions`; absent ones stay. Read the DDL itself with db.object.ddl and these toggles.";

    fn run(&self, session: &mut Session, params: DdlOptionsParams) -> Result<DdlPage, CommandError> {
        let (tab, mut data) = object_tab(session, params.tab.as_deref())?;
        let options = &mut data.ddl;
        for (wanted, field) in [
            (params.nested, &mut options.nested),
            (params.permissions, &mut options.permissions),
            (params.separate_foreign_keys, &mut options.separate_foreign_keys),
            (params.separate_constraint_indexes, &mut options.separate_constraint_indexes),
            (params.comments, &mut options.comments),
            (params.partitions, &mut options.partitions),
        ] {
            if let Some(wanted) = wanted {
                *field = wanted;
            }
        }
        let tab = keep(session, &tab, &data)?;
        Ok(DdlPage { tab: tab.id, options: data.ddl })
    }
}

pub struct ReadData;

impl Command for ReadData {
    type Params = TabParams;
    type Output = DataRead;
    const ID: &'static str = "db.object.readData";
    const TITLE: &'static str = "Read Object Data";
    const DESCRIPTION: &'static str = "Reads the rows of an object tab's table or view (`tab`, else the active one) again, in the background, in the connection's Main context: every column, ordered by the primary key (or a unique key of non-null columns) when there is one; the first page shows at once and reading goes on as in consoles. This is the person's action: a disconnected connection connects first with its stored password (code `passwordNeeded` otherwise). The previous rows of the tab are freed. Answers the query at once; db.object.state then gives the result.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<DataRead, CommandError> {
        let (tab, mut data) = object_tab(session, params.tab.as_deref())?;
        if data.page != Page::Data {
            data.page = Page::Data;
            keep(session, &tab, &data)?;
        }
        start_read(session, &tab.id, &data, true)
    }
}

/// A label and its value in an object's header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// The label.
    pub label: String,
    /// The value; empty when there is none.
    pub value: String,
}

/// A property section as listed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SectionInfo {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// How many rows it has, for sections that are lists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// What to read of an object's properties.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PropertiesParams {
    /// An object tab's id; with neither `tab` nor `connection`, the active tab of the Database workspace.
    pub tab: Option<String>,
    /// A connection's id, with `path`, to read an object without a tab.
    pub connection: Option<String>,
    /// The object's path, with `connection`.
    pub path: Option<Vec<String>>,
    /// The section to answer; the tab's, else the first, when absent.
    pub section: Option<String>,
    /// The most rows to answer; 200 when absent.
    pub limit: Option<usize>,
}

/// An object's Properties page: its header, its sections and one section's rows.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PropertiesAnswer {
    /// The connection's id.
    pub connection: String,
    /// The object's path.
    pub path: Vec<String>,
    /// What the object is.
    pub kind: String,
    /// The header: name, kind, comment, owner, tablespace and what the kind adds.
    pub header: Vec<Field>,
    /// The sections its kind has.
    pub sections: Vec<SectionInfo>,
    /// The section answered.
    pub section: String,
    /// The section's column titles, for sections that are lists.
    pub columns: Vec<String>,
    /// Its rows, at most `limit`.
    pub rows: Vec<Vec<String>>,
    /// The text of a section that is text (a view's query, a routine's source).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A line said above the rows (a partitioned table's key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// How many rows the section has.
    pub total: usize,
    /// Whether rows were left out because of `limit`.
    pub truncated: bool,
    /// Whether the object's details were not read yet; they are read when the connection is connected.
    pub loading: bool,
}

pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 || value >= 100.0 || value.fract() == 0.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn yes(flag: bool) -> String {
    if flag { t!("database-object-yes") } else { String::new() }
}

fn privilege(letter: char) -> Option<&'static str> {
    Some(match letter {
        'r' => "SELECT",
        'a' => "INSERT",
        'w' => "UPDATE",
        'd' => "DELETE",
        'D' => "TRUNCATE",
        'x' => "REFERENCES",
        't' => "TRIGGER",
        'X' => "EXECUTE",
        'U' => "USAGE",
        'C' => "CREATE",
        'c' => "CONNECT",
        'T' => "TEMPORARY",
        'm' => "MAINTAIN",
        _ => return None,
    })
}

fn permission_rows(acl: &[String]) -> Vec<Vec<String>> {
    acl.iter()
        .filter_map(|item| {
            let (grantee, rest) = item.split_once('=')?;
            let (privileges, grantor) = rest.split_once('/').unwrap_or((rest, ""));
            let mut names = Vec::new();
            let mut letters = privileges.chars().peekable();
            while let Some(letter) = letters.next() {
                let grantable = letters.peek() == Some(&'*');
                if grantable {
                    letters.next();
                }
                if let Some(name) = privilege(letter) {
                    names.push(if grantable { format!("{name}*") } else { name.to_owned() });
                }
            }
            let grantee = if grantee.is_empty() { "PUBLIC".to_owned() } else { grantee.trim_matches('"').to_owned() };
            Some(vec![grantee, names.join(", "), grantor.trim_matches('"').to_owned()])
        })
        .collect()
}

fn column_rows(object: &Object) -> Vec<Vec<String>> {
    object
        .columns
        .iter()
        .map(|column| {
            let default = match (&column.identity, &column.generated, &column.default) {
                (Some(identity), _, _) => t!("database-object-identity", how = identity.as_str()),
                (_, Some(expression), _) => t!("database-object-generated", expression = expression.as_str()),
                (_, _, Some(default)) => default.clone(),
                _ => String::new(),
            };
            vec![
                column.name.clone(),
                column.type_name.clone(),
                yes(!column.nullable),
                default,
                column.collation.clone().unwrap_or_default(),
                column.comment.clone().unwrap_or_default(),
            ]
        })
        .collect()
}

fn titles(keys: &[&str]) -> Vec<String> {
    keys.iter()
        .map(|key| match *key {
            "position" => "#".to_owned(),
            "name" => t!("database-object-column-name"),
            "type" => t!("database-object-column-type"),
            "notNull" => t!("database-object-column-not-null"),
            "default" => t!("database-object-column-default"),
            "collation" => t!("database-object-column-collation"),
            "comment" => t!("database-object-column-comment"),
            "kind" => t!("database-object-column-kind"),
            "columns" => t!("database-object-column-columns"),
            "definition" => t!("database-object-column-definition"),
            "references" => t!("database-object-column-references"),
            "onUpdate" => t!("database-object-column-on-update"),
            "onDelete" => t!("database-object-column-on-delete"),
            "unique" => t!("database-object-column-unique"),
            "method" => t!("database-object-column-method"),
            "size" => t!("database-object-column-size"),
            "function" => t!("database-object-column-function"),
            "enabled" => t!("database-object-column-enabled"),
            "schema" => t!("database-object-column-schema"),
            "table" => t!("database-object-column-table"),
            "constraint" => t!("database-object-column-constraint"),
            "command" => t!("database-object-column-command"),
            "permissive" => t!("database-object-column-permissive"),
            "roles" => t!("database-object-column-roles"),
            "using" => t!("database-object-column-using"),
            "check" => t!("database-object-column-check"),
            "partition" => t!("database-object-column-partition"),
            "bound" => t!("database-object-column-bound"),
            "property" => t!("database-object-column-property"),
            "value" => t!("database-object-column-value"),
            "grantee" => t!("database-object-column-grantee"),
            "privileges" => t!("database-object-column-privileges"),
            "grantor" => t!("database-object-column-grantor"),
            "rows" => t!("database-object-column-rows"),
            "direction" => t!("database-object-column-direction"),
            "event" => t!("database-object-event"),
            "instead" => t!("database-object-column-instead"),
            other => other.to_owned(),
        })
        .collect()
}

fn constraint_kind(kind: ConstraintKind) -> String {
    match kind {
        ConstraintKind::PrimaryKey => t!("database-object-primary-key"),
        ConstraintKind::Unique => t!("database-object-unique-key"),
        ConstraintKind::Check => t!("database-object-check"),
        ConstraintKind::Exclusion => t!("database-object-exclusion"),
        ConstraintKind::ForeignKey => t!("database-object-foreign-key"),
    }
}

struct Section {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    text: Option<String>,
    note: Option<String>,
}

impl Section {
    fn table(keys: &[&str], rows: Vec<Vec<String>>) -> Self {
        Self { columns: titles(keys), rows, text: None, note: None }
    }

    fn text(text: Option<String>) -> Self {
        Self { columns: Vec::new(), rows: Vec::new(), text: Some(text.unwrap_or_default()), note: None }
    }
}

fn property(label: &str, value: impl Into<String>) -> Vec<String> {
    vec![label.to_owned(), value.into()]
}

fn object_section(found: &Found<'_>, section: &str) -> Section {
    let object = found.object;
    match section {
        "columns" => {
            Section::table(&["name", "type", "notNull", "default", "collation", "comment"], column_rows(object))
        }
        "constraints" => Section::table(
            &["name", "kind", "columns", "definition"],
            object
                .constraints
                .iter()
                .filter(|constraint| constraint.kind != ConstraintKind::ForeignKey)
                .map(|constraint| {
                    vec![
                        constraint.name.clone(),
                        constraint_kind(constraint.kind),
                        constraint.columns.join(", "),
                        constraint.definition.clone(),
                    ]
                })
                .collect(),
        ),
        "foreignKeys" => Section::table(
            &["name", "columns", "references", "onUpdate", "onDelete"],
            object
                .constraints
                .iter()
                .filter(|constraint| constraint.kind == ConstraintKind::ForeignKey)
                .map(|constraint| {
                    let reference = constraint.references.clone().unwrap_or_default();
                    vec![
                        constraint.name.clone(),
                        constraint.columns.join(", "),
                        format!(
                            "{} ({})",
                            qualified(&reference.schema, &reference.table),
                            reference.columns.join(", ")
                        ),
                        reference.on_update,
                        reference.on_delete,
                    ]
                })
                .collect(),
        ),
        "indexes" => Section::table(
            &["name", "columns", "unique", "method", "size", "definition"],
            object
                .indexes
                .iter()
                .map(|index| {
                    vec![
                        index.name.clone(),
                        index.columns.join(", "),
                        yes(index.unique),
                        index.method.clone(),
                        index.size.map(size_text).unwrap_or_default(),
                        index.definition.clone(),
                    ]
                })
                .collect(),
        ),
        "triggers" => Section::table(
            &["name", "function", "enabled", "definition"],
            object
                .triggers
                .iter()
                .map(|trigger| {
                    vec![
                        trigger.name.clone(),
                        trigger.function.clone(),
                        yes(trigger.enabled),
                        trigger.definition.clone(),
                    ]
                })
                .collect(),
        ),
        "dependencies" => Section::table(
            &["direction", "schema", "name", "kind"],
            object
                .dependencies
                .iter()
                .map(|dependency| {
                    let direction = if dependency.direction == "usedBy" {
                        t!("database-object-used-by")
                    } else {
                        t!("database-object-uses")
                    };
                    vec![direction, dependency.schema.clone(), dependency.name.clone(), dependency.kind.clone()]
                })
                .collect(),
        ),
        "rules" => Section::table(
            &["name", "event", "instead", "definition"],
            object
                .rules
                .iter()
                .map(|rule| {
                    vec![rule.name.clone(), rule.event.to_uppercase(), yes(rule.instead), rule.definition.clone()]
                })
                .collect(),
        ),
        "references" => Section::table(
            &["schema", "table", "constraint"],
            references(found.catalog, &found.schema.name, &object.name)
                .into_iter()
                .map(|(schema, table, constraint)| vec![schema, table, constraint])
                .collect(),
        ),
        "policies" => Section::table(
            &["name", "command", "permissive", "roles", "using", "check"],
            object
                .policies
                .iter()
                .map(|policy| {
                    vec![
                        policy.name.clone(),
                        policy.command.clone(),
                        yes(policy.permissive),
                        policy.roles.join(", "),
                        policy.using.clone().unwrap_or_default(),
                        policy.check.clone().unwrap_or_default(),
                    ]
                })
                .collect(),
        ),
        "partitions" => {
            let parent = qualified(&found.schema.name, &object.name);
            let rows = found
                .catalog
                .schemas
                .iter()
                .flat_map(|schema| schema.objects.iter().map(move |other| (schema, other)))
                .filter(|(_, other)| {
                    other.partitioning.as_ref().and_then(|partitioning| partitioning.parent.as_deref())
                        == Some(parent.as_str())
                })
                .map(|(schema, other)| {
                    vec![
                        qualified(&schema.name, &other.name),
                        other
                            .partitioning
                            .as_ref()
                            .and_then(|partitioning| partitioning.bound.clone())
                            .unwrap_or_default(),
                    ]
                })
                .collect();
            let mut section = Section::table(&["partition", "bound"], rows);
            section.note =
                object.partitioning.as_ref().and_then(|partitioning| match (&partitioning.key, &partitioning.parent) {
                    (Some(key), _) => Some(t!("database-object-partitioned-by", key = key.as_str())),
                    (None, Some(parent)) => Some(t!(
                        "database-object-partition-of",
                        parent = parent.as_str(),
                        bound = partitioning.bound.clone().unwrap_or_default()
                    )),
                    _ => None,
                });
            section
        }
        "statistics" => {
            let mut rows = Vec::new();
            if let Some(estimate) = object.rows {
                rows.push(property(&t!("database-object-row-estimate"), estimate.to_string()));
            }
            if let Some(size) = object.size {
                rows.push(property(&t!("database-object-total-size"), size_text(size)));
            }
            let indexes: u64 = object.indexes.iter().filter_map(|index| index.size).sum();
            if !object.indexes.is_empty() {
                rows.push(property(&t!("database-object-index-count"), object.indexes.len().to_string()));
            }
            if indexes > 0 {
                rows.push(property(&t!("database-object-index-size"), size_text(indexes)));
            }
            rows.push(property(&t!("database-object-column-count"), object.columns.len().to_string()));
            rows.push(property(&t!("database-object-row-security"), yes(object.row_security)));
            Section::table(&["property", "value"], rows)
        }
        "permissions" => Section::table(&["grantee", "privileges", "grantor"], permission_rows(&object.acl)),
        "source" => Section::text(
            object
                .definition
                .clone()
                .or_else(|| object.routine.as_ref().and_then(|routine| routine.definition.clone())),
        ),
        "settings" => {
            let mut rows = Vec::new();
            if let Some(sequence) = &object.sequence {
                rows.push(property(&t!("database-object-data-type"), sequence.data_type.clone()));
                rows.push(property(&t!("database-object-start"), sequence.start.clone()));
                rows.push(property(&t!("database-object-increment"), sequence.increment.clone()));
                rows.push(property(&t!("database-object-minimum"), sequence.min.clone()));
                rows.push(property(&t!("database-object-maximum"), sequence.max.clone()));
                rows.push(property(&t!("database-object-cache"), sequence.cache.clone()));
                rows.push(property(&t!("database-object-cycle"), yes(sequence.cycle)));
                rows.push(property(&t!("database-object-owned-by"), sequence.owned_by.clone().unwrap_or_default()));
            }
            if let Some(info) = &object.type_info {
                rows.push(property(&t!("database-object-column-kind"), info.kind.clone()));
                if !info.labels.is_empty() {
                    rows.push(property(&t!("database-object-labels"), info.labels.join(", ")));
                }
                if let Some(base) = &info.base {
                    rows.push(property(&t!("database-object-base-type"), base.clone()));
                }
                if info.kind == "domain" {
                    rows.push(property(&t!("database-object-column-not-null"), yes(info.not_null)));
                    rows.push(property(
                        &t!("database-object-column-default"),
                        info.default.clone().unwrap_or_default(),
                    ));
                    rows.push(property(&t!("database-object-checks"), info.checks.join("; ")));
                }
            }
            Section::table(&["property", "value"], rows)
        }
        _ => Section::table(&[], Vec::new()),
    }
}

fn schema_section(schema: &Schema) -> Section {
    let rows = schema
        .objects
        .iter()
        .map(|object| {
            vec![
                object.key(),
                kind_name(object.kind).to_owned(),
                object.rows.map(|rows| rows.to_string()).unwrap_or_default(),
                object.size.map(size_text).unwrap_or_default(),
                object.comment.clone().unwrap_or_default(),
            ]
        })
        .collect();
    Section::table(&["name", "kind", "rows", "size", "comment"], rows)
}

fn header(found: &Target<'_>, driver: &str) -> Vec<Field> {
    let field = |label: String, value: Option<String>| Field { label, value: value.unwrap_or_default() };
    match found {
        Target::Schema(schema) => vec![
            field(t!("database-object-column-name"), Some(schema.name.clone())),
            field(t!("database-object-column-comment"), schema.comment.clone()),
            field(t!("database-object-owner"), schema.owner.clone()),
        ],
        Target::Item(item) => vec![
            field(t!("database-object-column-name"), Some(item.name.clone())),
            field(t!("database-object-column-comment"), item.comment.clone()),
            field(t!("database-object-owner"), item.owner.clone()),
        ],
        Target::Object(found) => {
            let object = found.object;
            let mut fields = vec![
                field(t!("database-object-column-name"), Some(object.name.clone())),
                field(t!("database-object-column-comment"), object.comment.clone()),
            ];
            if applies(driver, "owner") {
                fields.push(field(t!("database-object-owner"), object.owner.clone()));
            }
            if matches!(object.kind, ObjectKind::Table | ObjectKind::MaterializedView) && applies(driver, "tablespace")
            {
                fields.push(field(
                    t!("database-object-tablespace"),
                    Some(object.tablespace.clone().unwrap_or_else(|| t!("database-object-default-tablespace"))),
                ));
            }
            if let Some(routine) = &object.routine {
                fields.push(field(t!("database-object-arguments"), Some(routine.arguments.clone())));
                fields.push(field(t!("database-object-returns"), routine.result.clone()));
                fields.push(field(t!("database-object-language"), Some(routine.language.clone())));
            }
            fields
        }
    }
}

fn count_of(found: &Target<'_>, section: &str) -> Option<usize> {
    let Target::Object(found) = found else {
        return match found {
            Target::Schema(schema) if section == "objects" => Some(schema.objects.len()),
            _ => None,
        };
    };
    let object = found.object;
    Some(match section {
        "columns" => object.columns.len(),
        "constraints" => object.constraints.iter().filter(|known| known.kind != ConstraintKind::ForeignKey).count(),
        "foreignKeys" => object.constraints.iter().filter(|known| known.kind == ConstraintKind::ForeignKey).count(),
        "indexes" => object.indexes.len(),
        "triggers" => object.triggers.len(),
        "references" => references(found.catalog, &found.schema.name, &object.name).len(),
        "policies" => object.policies.len(),
        "permissions" => object.acl.len(),
        "dependencies" => object.dependencies.len(),
        "rules" => object.rules.len(),
        _ => {
            let content = object_section(found, section);
            return content.text.is_none().then_some(content.rows.len());
        }
    })
}

pub fn properties(
    snapshot: &Snapshot,
    connection: &str,
    path: &[String],
    section: Option<&str>,
    limit: usize,
) -> Option<PropertiesAnswer> {
    let found = target(snapshot, path)?;
    let kind = found.kind();
    let offered: Vec<&str> =
        sections(kind).iter().copied().filter(|section| applies(&snapshot.driver, section)).collect();
    let section = section.filter(|wanted| offered.contains(wanted)).or(offered.first().copied()).unwrap_or_default();
    let content = match &found {
        Target::Object(object) => object_section(object, section),
        Target::Schema(schema) => schema_section(schema),
        Target::Item(item) => Section::table(&["property", "value"], item.settings.clone()),
    };
    let total = content.rows.len();
    let loading = match &found {
        Target::Object(object) => !object.object.detailed,
        Target::Schema(_) | Target::Item(_) => false,
    };
    Some(PropertiesAnswer {
        connection: connection.to_owned(),
        path: path.to_vec(),
        kind: kind.to_owned(),
        header: header(&found, &snapshot.driver),
        sections: offered
            .iter()
            .map(|id| SectionInfo { id: (*id).to_owned(), title: section_title(id), count: count_of(&found, id) })
            .collect(),
        section: section.to_owned(),
        columns: content.columns,
        rows: content.rows.into_iter().take(limit).collect(),
        text: content.text,
        note: content.note,
        total,
        truncated: total > limit,
        loading,
    })
}

pub struct ObjectProperties;

impl Command for ObjectProperties {
    type Params = PropertiesParams;
    type Output = PropertiesAnswer;
    const ID: &'static str = "db.object.properties";
    const TITLE: &'static str = "Database Object Properties";
    const DESCRIPTION: &'static str = "The Properties page of an object, as its tab shows it (`tab`, the active object tab, or `connection` and `path`): the header (name, comment, owner and tablespace where the driver has them; a routine's arguments, result and language), the sections its kind has on that driver (SQLite has no owner, tablespace, policies, partitions or permissions) with their counts, and one section (`section`, else the tab's or the first) as column titles and rows of text, at most `limit` (200) with `total` and `truncated`, or as `text` (a view's query, a routine's source). From the snapshot, offline; `loading` when the details were not read yet. Never data values.";

    fn run(&self, session: &mut Session, params: PropertiesParams) -> Result<PropertiesAnswer, CommandError> {
        let (connection, path, kept) = match (&params.connection, &params.path) {
            (Some(connection), Some(path)) => (connection.clone(), path.clone(), None),
            _ => {
                let (_, data) = object_tab(session, params.tab.as_deref())?;
                (data.connection, data.path, data.section)
            }
        };
        let entry = find(session, &connection)?;
        let section = params.section.or(kept);
        let limit = params.limit.unwrap_or(LISTED);
        let answer = {
            let snapshot = snapshot_ref(session, &entry, true).ok_or_else(|| unknown(&connection, &path))?;
            properties(snapshot, &connection, &path, section.as_deref(), limit)
                .ok_or_else(|| unknown(&connection, &path))?
        };
        let busy = session.state::<Structure>().is_some_and(|structure| structure.busy(&connection));
        if answer.loading && !busy {
            details(session, &entry, &path)?;
        }
        Ok(answer)
    }
}

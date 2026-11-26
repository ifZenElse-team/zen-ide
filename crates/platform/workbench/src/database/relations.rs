use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::dialect::{SqlDialect, table_id};
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::edit::{self, Staged, source_table};
use zen_db::engine::{ConnectionState, OperationStatus, QueryOptions, QueryStatement};
use zen_db::filter::Sort;
use zen_db::registry::Entry;
use zen_db::relations::{self, Link};
use zen_db::results::ResultSet;
use zen_db::schema::{Object, Snapshot, find_object, qualified};
use zen_db::value::Value;
use zen_i18n::t;

use super::WORKSPACE;
use super::connections::describe;
use super::consoles::{self, engine};
use super::edits::{self, EditState};
use super::filters::{self, result_or_active};
use super::objects::{self, ObjectTab, Page};
use super::sources::{Database, find};
use super::structure::{Structure, details, ensure};
use crate::workspaces::{SelectTab, TabParams, Workspaces};

const LISTED: usize = 50;
const LOOKUP_ROWS: usize = 200;
const LOOKUP_COLUMNS: usize = 8;
const SEARCHED: usize = 6;
const CELL: usize = 200;
const KEPT: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Location {
    tab: String,
    title: String,
    object: Option<ObjectTab>,
    filter: Option<(String, Vec<Sort>)>,
}

struct Lookup {
    source: u64,
    row: usize,
    link: Link,
    columns: Vec<usize>,
    query: u64,
    search: String,
    limit: usize,
}

#[derive(Default)]
pub struct Relations {
    back: Vec<Location>,
    forward: Vec<Location>,
    lookup: Option<Lookup>,
}

struct Followed {
    link: Link,
    columns: Vec<usize>,
    path: Option<Vec<String>>,
}

struct Known {
    connection: String,
    dialect: SqlDialect,
    table: String,
    links: Vec<Followed>,
    references: Vec<Followed>,
}

enum Resolved {
    Known(Box<Known>),
    Reading,
    None,
}

fn connected(session: &Session, connection: &str) -> bool {
    session.state::<Database>().and_then(Database::started).is_some_and(|engine| {
        matches!(engine.status(connection).state, ConnectionState::Connected | ConnectionState::Closed)
    })
}

fn snapshot<'a>(session: &'a Session, entry: &Entry) -> Option<&'a Snapshot> {
    session.state::<Structure>().and_then(|structure| structure.snapshot(&entry.source.id))
}

fn by_id(snapshot: &Snapshot, id: u32, numbered: bool) -> Option<Vec<String>> {
    snapshot.catalogs.iter().find_map(|catalog| {
        catalog.schemas.iter().find_map(|schema| {
            let matches =
                |object: &Object| if numbered { object.oid == id } else { table_id(&schema.name, &object.name) == id };
            schema.objects.iter().find(|object| matches(object)).map(|object| {
                vec![catalog.name.clone(), schema.name.clone(), object.kind.folder().to_owned(), object.key()]
            })
        })
    })
}

fn path_of(snapshot: &Snapshot, catalog: &str, schema: &str, table: &str) -> Option<Vec<String>> {
    let (catalog, schema, object) = relations::find_table(snapshot, catalog, schema, table)?;
    Some(vec![catalog.name.clone(), schema.name.clone(), object.kind.folder().to_owned(), object.key()])
}

fn indices(map: &[Option<String>], names: &[String]) -> Option<Vec<usize>> {
    names.iter().map(|name| map.iter().position(|known| known.as_deref() == Some(name.as_str()))).collect()
}

fn resolve(session: &mut Session, result: &ResultSet) -> Resolved {
    let connection = result.connection().to_owned();
    let Some(dialect) = filters::dialect_of(session, &connection) else { return Resolved::None };
    let Ok(entry) = find(session, &connection) else { return Resolved::None };
    let columns = result.columns();
    ensure(session, &entry, false);
    let view = objects::data_view(session, result.id());
    let pending = session
        .state::<Structure>()
        .is_some_and(|structure| structure.loading(&entry.source.id) || structure.busy(&entry.source.id));
    let path = {
        let Some(snapshot) = snapshot(session, &entry) else {
            return if pending { Resolved::Reading } else { Resolved::None };
        };
        match &view {
            Some((_, data)) => Some(data.path.clone()),
            None => source_table(&columns).ok().and_then(|id| by_id(snapshot, id, dialect == SqlDialect::Postgres)),
        }
    };
    let Some(path) = path else {
        return if pending { Resolved::Reading } else { Resolved::None };
    };
    let detailed = {
        let Some(snapshot) = snapshot(session, &entry) else { return Resolved::None };
        let Some(found) = find_object(snapshot, &path) else { return Resolved::None };
        found.object.detailed
    };
    if !detailed {
        let busy = session.state::<Structure>().is_some_and(|structure| structure.busy(&entry.source.id));
        if !busy && connected(session, &entry.source.id) {
            let _ = details(session, &entry, &path);
        }
        return Resolved::Reading;
    }
    let Some(snapshot) = snapshot(session, &entry) else { return Resolved::None };
    let Some(found) = find_object(snapshot, &path) else { return Resolved::None };
    let (catalog, schema, object) = (&found.catalog.name, &found.schema.name, found.object);
    let id = if dialect == SqlDialect::Postgres { object.oid } else { table_id(schema, &object.name) };
    let map = relations::table_columns(&columns, object, id, view.is_some());
    let links = relations::outgoing(snapshot, catalog, schema, object)
        .into_iter()
        .filter_map(|link| {
            let columns = indices(&map, &link.columns)?;
            let path = path_of(snapshot, catalog, &link.target_schema, &link.target_table);
            Some(Followed { link, columns, path })
        })
        .collect();
    let references = relations::incoming(snapshot, catalog, schema, &object.name)
        .into_iter()
        .filter_map(|link| {
            let columns = indices(&map, &link.target_columns)?;
            let path = path_of(snapshot, catalog, &link.schema, &link.table);
            Some(Followed { link, columns, path })
        })
        .collect();
    Resolved::Known(Box::new(Known { connection, dialect, table: qualified(schema, &object.name), links, references }))
}

fn known(session: &mut Session, result: &ResultSet) -> Result<Known, CommandError> {
    if result.columns().is_empty() {
        return Err(CommandError::with_code("columnsPending", t!("database-error-columns-pending")));
    }
    match resolve(session, result) {
        Resolved::Known(known) => Ok(*known),
        Resolved::Reading => Err(CommandError::with_code("structurePending", t!("database-error-relations-pending"))),
        Resolved::None => Err(CommandError::with_code("noLink", t!("database-error-no-relations"))),
    }
}

/// A foreign key of a result's table as views and agents read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KeyLink {
    /// The constraint's name.
    pub name: String,
    /// The result's columns it is made of, from 0.
    pub columns: Vec<usize>,
    /// Their names in the table.
    pub names: Vec<String>,
    /// The other table, `schema.table`: the referenced one for a link, the referencing one for a reference.
    pub table: String,
    /// The other table's columns it pairs with.
    pub other_columns: Vec<String>,
    /// The other table's path for db.object.open; absent when the snapshot does not know it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<Vec<String>>,
}

/// The foreign keys of a result's table.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RelationsAnswer {
    /// The result.
    pub id: u64,
    /// The table its rows come from, `schema.table`, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// Its foreign keys whose columns are all in the result: their cells link to the referenced row.
    pub links: Vec<KeyLink>,
    /// Other tables' foreign keys that point at it, whose referenced columns are all in the result.
    pub references: Vec<KeyLink>,
    /// Whether the table's structure is being read; ask again.
    pub reading: bool,
    /// Whether links or references were left out because of `limit`.
    pub truncated: bool,
}

fn key_link(followed: &Followed, outgoing: bool) -> KeyLink {
    let link = &followed.link;
    let (names, table, other) = if outgoing {
        (&link.columns, qualified(&link.target_schema, &link.target_table), &link.target_columns)
    } else {
        (&link.target_columns, qualified(&link.schema, &link.table), &link.columns)
    };
    KeyLink {
        name: link.name.clone(),
        columns: followed.columns.clone(),
        names: names.clone(),
        table,
        other_columns: other.clone(),
        path: followed.path.clone(),
    }
}

/// Which result.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RelationsParams {
    /// The result's id; the result the Database workspace shows in front when absent.
    pub id: Option<u64>,
    /// The most links and references, each; 50 when absent.
    pub limit: Option<usize>,
}

pub struct ResultRelations;

impl Command for ResultRelations {
    type Params = RelationsParams;
    type Output = RelationsAnswer;
    const ID: &'static str = "db.result.relations";
    const TITLE: &'static str = "Result Foreign Keys";
    const DESCRIPTION: &'static str = "The foreign keys of the table a result's rows come from (`id`, else the one shown in front), as the snapshot knows them: `links`, its own keys whose columns are all in the result (their cells open the referenced row with db.result.openReferenced), and `references`, other tables' keys that point at it (db.result.openReferencing), each with the result's columns, their names, the other table and its columns and path; at most `limit` (50) each (`truncated`). `reading` while the table's structure is read: ask again. Empty for a result of several tables, of no table, or of a driver without SQL (Redis). Never data values.";

    fn run(&self, session: &mut Session, params: RelationsParams) -> Result<RelationsAnswer, CommandError> {
        let result = result_or_active(session, params.id)?;
        let limit = params.limit.unwrap_or(LISTED);
        let mut answer = RelationsAnswer { id: result.id(), ..RelationsAnswer::default() };
        if result.columns().is_empty() {
            answer.reading = true;
            return Ok(answer);
        }
        match resolve(session, &result) {
            Resolved::Known(known) => {
                answer.table = Some(known.table.clone());
                answer.truncated = known.links.len() > limit || known.references.len() > limit;
                answer.links = known.links.iter().take(limit).map(|followed| key_link(followed, true)).collect();
                answer.references =
                    known.references.iter().take(limit).map(|followed| key_link(followed, false)).collect();
            }
            Resolved::Reading => answer.reading = true,
            Resolved::None => {}
        }
        Ok(answer)
    }
}

fn no_row(result: &ResultSet, row: usize) -> CommandError {
    CommandError::with_code("noRow", t!("database-error-no-row", row = row, rows = result.len()))
}

fn location_of_tab(session: &Session, tab: &str) -> Option<Location> {
    let found = session.state::<Workspaces>()?.tabs(WORKSPACE).iter().find(|known| known.id == tab)?.clone();
    let object =
        (found.kind == objects::KIND).then(|| serde_json::from_value::<ObjectTab>(found.data.clone()).ok()).flatten();
    let filter = object.as_ref().map(|_| filters::current(session, &found.id));
    Some(Location { tab: found.id, title: found.title, object, filter })
}

fn location_of_result(session: &Session, result: u64) -> Option<Location> {
    if let Some(tab) = session.state::<objects::Objects>().and_then(|objects| objects.tab_of(result)) {
        return location_of_tab(session, tab);
    }
    let path = session.state::<consoles::Consoles>()?.console_of(result)?.to_owned();
    let tab = session
        .state::<Workspaces>()?
        .tabs(WORKSPACE)
        .iter()
        .find(|known| known.path.as_deref() == Some(path.as_path()))?
        .id
        .clone();
    location_of_tab(session, &tab)
}

fn location_in_front(session: &Session) -> Option<Location> {
    let tab = session.state::<Workspaces>()?.active_tab(WORKSPACE)?.id.clone();
    location_of_tab(session, &tab)
}

fn remember(session: &mut Session, from: Option<Location>) {
    let Some(from) = from else { return };
    let relations = session.state_mut::<Relations>();
    if relations.back.last() != Some(&from) {
        relations.back.push(from);
    }
    if relations.back.len() > KEPT {
        relations.back.remove(0);
    }
    relations.forward.clear();
}

fn table_key(connection: &str, path: &[String]) -> String {
    match path {
        [_, schema, _, name, ..] => format!("{connection}|{}", qualified(schema, name)),
        _ => format!("{connection}|*"),
    }
}

fn open_filtered(
    session: &mut Session,
    (connection, path): (&str, &[String]),
    (condition, sorts): (String, Vec<Sort>),
    retry: (&str, serde_json::Value),
) -> Result<(String, Option<u64>), CommandError> {
    let tab = objects::tab_id(connection, path);
    edits::refuse_staged(session, &objects::results_of(session, &tab), retry)?;
    filters::set_object_filter(session, (&tab, &table_key(connection, path)), (condition, sorts), true)?;
    let opened = objects::ObjectOpen.run(
        session,
        objects::OpenParams { connection: connection.to_owned(), path: path.to_vec(), page: Some(Page::Data) },
    )?;
    let query = match opened.query {
        Some(query) => Some(query),
        None => {
            let data: ObjectTab = serde_json::from_value(opened.tab.data.clone()).unwrap_or_default();
            objects::start_read(session, &opened.tab.id, &data, true)?.query
        }
    };
    Ok((opened.tab.id, query))
}

/// Where following a key went.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FollowAnswer {
    /// The object tab that shows the rows.
    pub tab: String,
    /// The connection.
    pub connection: String,
    /// The table's path.
    pub path: Vec<String>,
    /// The filter its rows are read with.
    pub condition: String,
    /// The query reading them, for db.query.state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// Whether db.data.back has a tab to go to.
    pub can_go_back: bool,
    /// Whether db.data.forward has one.
    pub can_go_forward: bool,
}

fn answer(
    session: &Session,
    (tab, connection, path): (String, String, Vec<String>),
    condition: String,
    query: Option<u64>,
) -> FollowAnswer {
    let relations = session.state::<Relations>();
    FollowAnswer {
        tab,
        connection,
        path,
        condition,
        query,
        can_go_back: relations.is_some_and(|relations| !relations.back.is_empty()),
        can_go_forward: relations.is_some_and(|relations| !relations.forward.is_empty()),
    }
}

fn follow(
    session: &mut Session,
    result: &Arc<ResultSet>,
    (known, followed): (&Known, &Followed),
    (row, outgoing): (usize, bool),
    retry: (&str, serde_json::Value),
) -> Result<FollowAnswer, CommandError> {
    if row >= result.len() {
        return Err(no_row(result, row));
    }
    let names = if outgoing { &followed.link.target_columns } else { &followed.link.columns };
    let values: Vec<Value> = followed.columns.iter().map(|column| result.get(row, *column)).collect();
    let pairs: Vec<(&str, &Value)> = names.iter().map(String::as_str).zip(values.iter()).collect();
    let condition = relations::key_condition(&pairs, known.dialect)
        .ok_or_else(|| CommandError::with_code("nullKey", t!("database-error-null-key")))?;
    let path = followed.path.clone().ok_or_else(|| {
        CommandError::with_code(
            "unknownObject",
            t!("database-error-unknown-object", path = known.table.as_str(), connection = known.connection.as_str()),
        )
    })?;
    let from = location_of_result(session, result.id());
    let (tab, query) = open_filtered(session, (&known.connection, &path), (condition.clone(), Vec::new()), retry)?;
    remember(session, from.filter(|from| from.tab != tab));
    Ok(answer(session, (tab, known.connection.clone(), path), condition, query))
}

/// A cell whose foreign key to follow.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ReferencedParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The row, from 0.
    pub row: usize,
    /// A column of the key, from 0; the key named `key`, or the table's only key, when absent.
    pub column: Option<usize>,
    /// The foreign key's constraint name.
    pub key: Option<String>,
}

pub struct OpenReferenced;

impl Command for OpenReferenced {
    type Params = ReferencedParams;
    type Output = FollowAnswer;
    const ID: &'static str = "db.result.openReferenced";
    const TITLE: &'static str = "Open Referenced Row";
    const DESCRIPTION: &'static str = "Follows a foreign key of row `row` of a result (`id`, else the one shown): the key that has `column` (from 0), else the key named `key`, else the table's only key. Opens the referenced table's Data page as an object tab filtered by the key's values (`a = 1 AND b = 'x'`, written in the connection's SQL dialect) and reads its rows in the background in the Main context; the tab shown before is kept for db.data.back. Codes `noLink` (no such key in the result), `nullKey` (a value of the key is NULL), `noRow`, `structurePending` (the table's structure is being read: ask again), `pendingChanges` (the referenced tab has staged changes).";

    fn run(&self, session: &mut Session, params: ReferencedParams) -> Result<FollowAnswer, CommandError> {
        let result = result_or_active(session, params.id)?;
        let known = known(session, &result)?;
        let followed = known
            .links
            .iter()
            .find(|followed| match (params.column, &params.key) {
                (Some(column), _) => followed.columns.contains(&column),
                (None, Some(key)) => followed.link.name == *key,
                (None, None) => known.links.len() == 1,
            })
            .ok_or_else(|| CommandError::with_code("noLink", t!("database-error-no-link")))?;
        let retry = (
            Self::ID,
            serde_json::json!({ "id": result.id(), "row": params.row, "column": params.column, "key": params.key }),
        );
        follow(session, &result, (&known, followed), (params.row, true), retry)
    }
}

/// A row whose references to open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ReferencingParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The row, from 0.
    pub row: usize,
    /// The referencing table, `schema.table` as db.result.relations names it.
    pub table: String,
    /// Its foreign key's constraint name; its only key that points here when absent.
    pub key: Option<String>,
}

pub struct OpenReferencing;

impl Command for OpenReferencing {
    type Params = ReferencingParams;
    type Output = FollowAnswer;
    const ID: &'static str = "db.result.openReferencing";
    const TITLE: &'static str = "Open References";
    const DESCRIPTION: &'static str = "Opens the rows of `table` (a referencing table from db.result.relations `references`) whose foreign key `key` points at row `row` of a result (`id`, else the one shown): its Data page as an object tab filtered by the key's columns equal to the row's values, in the connection's SQL dialect, read in the background in the Main context; the tab shown before is kept for db.data.back. Codes `noLink`, `nullKey`, `noRow`, `structurePending`, `pendingChanges`.";

    fn run(&self, session: &mut Session, params: ReferencingParams) -> Result<FollowAnswer, CommandError> {
        let result = result_or_active(session, params.id)?;
        let known = known(session, &result)?;
        let candidates: Vec<&Followed> = known
            .references
            .iter()
            .filter(|followed| qualified(&followed.link.schema, &followed.link.table) == params.table)
            .collect();
        let followed = match &params.key {
            Some(key) => candidates.iter().find(|followed| followed.link.name == *key).copied(),
            None if candidates.len() == 1 => candidates.first().copied(),
            None => None,
        }
        .ok_or_else(|| CommandError::with_code("noLink", t!("database-error-no-link")))?;
        let retry = (
            Self::ID,
            serde_json::json!({ "id": result.id(), "row": params.row, "table": params.table, "key": params.key }),
        );
        follow(session, &result, (&known, followed), (params.row, false), retry)
    }
}

/// Back and Forward through the data tabs visited by following keys.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NavigationState {
    /// Whether Back has a tab to go to.
    pub can_go_back: bool,
    /// Whether Forward has one.
    pub can_go_forward: bool,
    /// The titles of the tabs Back goes to, nearest first, at most 20.
    pub back: Vec<String>,
    /// The titles of the tabs Forward goes to, nearest first, at most 20.
    pub forward: Vec<String>,
    /// The tab shown after Back or Forward.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    /// The query reading its rows again when its filter changed back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
}

fn navigation(session: &Session, tab: Option<String>, query: Option<u64>) -> NavigationState {
    let titles = |list: &[Location]| -> Vec<String> {
        list.iter().rev().take(20).map(|location| location.title.clone()).collect()
    };
    let (back, forward) = session
        .state::<Relations>()
        .map(|relations| (titles(&relations.back), titles(&relations.forward)))
        .unwrap_or_default();
    NavigationState { can_go_back: !back.is_empty(), can_go_forward: !forward.is_empty(), back, forward, tab, query }
}

fn go(session: &mut Session, location: &Location) -> Result<(String, Option<u64>), CommandError> {
    let open = session
        .state::<Workspaces>()
        .is_some_and(|workspaces| workspaces.tabs(WORKSPACE).iter().any(|known| known.id == location.tab));
    match (&location.object, &location.filter) {
        (Some(object), Some(filter)) => {
            let unchanged = open && filters::current(session, &location.tab) == *filter;
            if unchanged {
                SelectTab.run(session, TabParams { workspace: WORKSPACE.to_owned(), tab: location.tab.clone() })?;
                return Ok((location.tab.clone(), None));
            }
            let retry = (DataBack::ID, serde_json::json!({}));
            open_filtered(session, (&object.connection, &object.path), filter.clone(), retry)
        }
        _ if open => {
            SelectTab.run(session, TabParams { workspace: WORKSPACE.to_owned(), tab: location.tab.clone() })?;
            Ok((location.tab.clone(), None))
        }
        _ => Err(CommandError::with_code("tabClosed", t!("database-error-tab-closed", tab = location.title.as_str()))),
    }
}

fn travel(session: &mut Session, back: bool) -> Result<NavigationState, CommandError> {
    let target = {
        let relations = session.state_mut::<Relations>();
        if back { relations.back.pop() } else { relations.forward.pop() }
    };
    let Some(target) = target else { return Ok(navigation(session, None, None)) };
    let here = location_in_front(session);
    match go(session, &target) {
        Ok((tab, query)) => {
            if let Some(here) = here.filter(|here| here.tab != tab || here.filter != target.filter) {
                let relations = session.state_mut::<Relations>();
                if back { relations.forward.push(here) } else { relations.back.push(here) }
            }
            Ok(navigation(session, Some(tab), query))
        }
        Err(error) if error.code.as_deref() == Some("tabClosed") => Err(error),
        Err(error) => {
            let relations = session.state_mut::<Relations>();
            if back {
                relations.back.push(target)
            } else {
                relations.forward.push(target)
            }
            Err(error)
        }
    }
}

/// No parameters.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NoParams {}

pub struct DataBack;

impl Command for DataBack {
    type Params = NoParams;
    type Output = NavigationState;
    const ID: &'static str = "db.data.back";
    const TITLE: &'static str = "Back";
    const DESCRIPTION: &'static str = "Goes back to the data tab shown before the last db.result.openReferenced or db.result.openReferencing (or Forward): brings it to front, and reads an object tab's rows again with the filter it had when it was left (a closed object tab opens again). Answers Back and Forward; code `tabClosed` when that tab was a console that has been closed (it is dropped), `pendingChanges` when reading again would drop staged changes.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<NavigationState, CommandError> {
        travel(session, true)
    }
}

pub struct DataForward;

impl Command for DataForward {
    type Params = NoParams;
    type Output = NavigationState;
    const ID: &'static str = "db.data.forward";
    const TITLE: &'static str = "Forward";
    const DESCRIPTION: &'static str =
        "Goes forward to the data tab db.data.back left, with the filter it had. Answers Back and Forward.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<NavigationState, CommandError> {
        travel(session, false)
    }
}

pub struct DataNavigation;

impl Command for DataNavigation {
    type Params = NoParams;
    type Output = NavigationState;
    const ID: &'static str = "db.data.navigation";
    const TITLE: &'static str = "Data Navigation";
    const DESCRIPTION: &'static str = "Whether db.data.back and db.data.forward have a data tab to go to, with the titles of those tabs, nearest first (at most 20 each).";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<NavigationState, CommandError> {
        Ok(navigation(session, None, None))
    }
}

/// A lookup's rows as views and agents read them.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LookupState {
    /// Whether a lookup is open.
    pub open: bool,
    /// The result being edited.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// Its row.
    pub row: usize,
    /// The result's columns the chosen key is written to, from 0.
    pub columns: Vec<usize>,
    /// The referenced table, `schema.table`.
    pub table: String,
    /// The referenced columns, the key.
    pub key: Vec<String>,
    /// The text searched for.
    pub search: String,
    /// The columns shown: the key first, at most eight.
    pub headers: Vec<String>,
    /// The rows read: each one's values as text (absent for NULL), cut to 200 characters.
    pub rows: Vec<Vec<Option<String>>>,
    /// `running`, `done`, `failed` or `cancelled`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<OperationStatus>,
    /// The query reading them, for db.query.cancel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether the table has more matching rows than the limit.
    pub truncated: bool,
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn close_lookup(session: &mut Session) -> bool {
    let Some(lookup) = session.state_mut::<Relations>().lookup.take() else { return false };
    if let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() {
        engine.cancel_query(lookup.query);
        if let Some(result) = engine.query(lookup.query).and_then(|query| query.statements.first()?.result) {
            engine.results().remove(result);
        }
    }
    true
}

fn lookup_rows(session: &Session) -> (Option<Arc<ResultSet>>, Option<zen_db::engine::QueryState>) {
    let Some(lookup) = session.state::<Relations>().and_then(|relations| relations.lookup.as_ref()) else {
        return (None, None);
    };
    let Some(engine) = session.state::<Database>().and_then(Database::started) else { return (None, None) };
    let query = engine.query(lookup.query);
    let result =
        query.as_ref().and_then(|query| query.statements.first()?.result).and_then(|id| engine.results().get(id));
    (result, query)
}

fn lookup_state(session: &Session) -> LookupState {
    let Some(lookup) = session.state::<Relations>().and_then(|relations| relations.lookup.as_ref()) else {
        return LookupState::default();
    };
    let (result, query) = lookup_rows(session);
    let mut state = LookupState {
        open: true,
        id: Some(lookup.source),
        row: lookup.row,
        columns: lookup.columns.clone(),
        table: qualified(&lookup.link.target_schema, &lookup.link.target_table),
        key: lookup.link.target_columns.clone(),
        search: lookup.search.clone(),
        query: Some(lookup.query),
        status: query.as_ref().map(|query| query.status),
        ..LookupState::default()
    };
    if let Some(query) = &query {
        let error = query.statements.first().and_then(|statement| statement.error.as_ref()).or(query.error.as_ref());
        state.error = error.map(describe);
    }
    let Some(result) = result else { return state };
    let columns = result.columns();
    let mut shown: Vec<usize> = lookup
        .link
        .target_columns
        .iter()
        .filter_map(|name| columns.iter().position(|column| column.name == *name))
        .collect();
    for at in 0..columns.len() {
        if shown.len() >= LOOKUP_COLUMNS {
            break;
        }
        if !shown.contains(&at) {
            shown.push(at);
        }
    }
    state.headers = shown.iter().map(|at| columns[*at].name.clone()).collect();
    let rows = result.page(0, lookup.limit + 1);
    state.truncated = rows.len() > lookup.limit;
    state.rows = rows
        .into_iter()
        .take(lookup.limit)
        .map(|values| {
            shown
                .iter()
                .map(|at| values.get(*at).and_then(Value::display).map(|text| text.chars().take(CELL).collect()))
                .collect()
        })
        .collect();
    state
}

/// A key cell to choose a value for.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct LookupParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The row, from 0.
    pub row: usize,
    /// A column of a foreign key, from 0.
    pub column: usize,
    /// Text the rows must contain in their key or text columns.
    pub search: String,
    /// The most rows; 200 when absent, at most 200.
    pub limit: Option<usize>,
}

pub struct LookupStart;

impl Command for LookupStart {
    type Params = LookupParams;
    type Output = LookupState;
    const ID: &'static str = "db.lookup.start";
    const TITLE: &'static str = "Choose Key Value";
    const DESCRIPTION: &'static str = "Opens the lookup for cell (`row`, `column`) of an editable result (`id`, else the one shown) whose column is part of a foreign key: reads at most `limit` (200, at most 200) rows of the referenced table in the background in the Main context, ordered by its key, with `search` (when given) contained in the key's or the text columns (LIKE in the connection's dialect). A lookup open before is closed. Read the rows with db.lookup.state, write one into the staged edit with db.lookup.choose, close with db.lookup.close. Codes `noLink`, `readOnly`, `readOnlyColumn`, `noRow`, `structurePending`.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<LookupState, CommandError> {
        let result = result_or_active(session, params.id)?;
        let known = known(session, &result)?;
        let followed = known
            .links
            .iter()
            .find(|followed| followed.columns.contains(&params.column))
            .ok_or_else(|| CommandError::with_code("noLink", t!("database-error-no-link")))?;
        let editing = edits::editable(session, &result)?;
        for column in &followed.columns {
            if editing.editing.columns.get(*column).is_none_or(|known| !known.editable) {
                let name = result.columns().get(*column).map(|known| known.name.clone()).unwrap_or_default();
                return Err(CommandError::with_code(
                    "readOnlyColumn",
                    t!("database-error-read-only-column", column = name),
                ));
            }
        }
        if params.row >= result.len() {
            return Err(no_row(&result, params.row));
        }
        let entry = find(session, &known.connection)?;
        let (order, searched) = {
            let snapshot = snapshot(session, &entry);
            let target = snapshot.and_then(|snapshot| {
                let catalog = followed.path.as_ref()?.first()?.clone();
                relations::find_table(snapshot, &catalog, &followed.link.target_schema, &followed.link.target_table)
            });
            let order = followed.link.target_columns.clone();
            let mut searched = followed.link.target_columns.clone();
            if let Some((_, _, object)) = target {
                for column in &object.columns {
                    let kind = column.type_name.to_ascii_lowercase();
                    let array = kind.ends_with("[]") || kind.starts_with('_');
                    let textual =
                        !array && ["char", "text", "string", "name", "enum"].iter().any(|word| kind.contains(word));
                    if searched.len() < SEARCHED && textual && !searched.contains(&column.name) {
                        searched.push(column.name.clone());
                    }
                }
            }
            (order, searched)
        };
        let dialect = known.dialect;
        let limit = params.limit.unwrap_or(LOOKUP_ROWS).clamp(1, LOOKUP_ROWS);
        let order: Vec<String> = order.iter().map(|column| dialect.order(&dialect.quote(column), false)).collect();
        let condition = relations::search_condition(&searched, &params.search, dialect);
        let sql = relations::lookup_statement(
            dialect,
            &dialect.qualified(&followed.link.target_schema, &followed.link.target_table),
            condition.as_deref(),
            Some(&order.join(", ")),
            limit + 1,
        );
        super::safety::agent_reads(session, &entry, &sql)?;
        close_lookup(session);
        let provenance = super::history::provenance(
            session,
            zen_db::history::Source::Data,
            Some(followed.link.target_table.clone()),
        );
        let options = QueryOptions {
            stop_on_error: true,
            rollback_on_error: false,
            timeout: None,
            schema: None,
            provenance,
            read_only: false,
            notes: Vec::new(),
        };
        let statement = QueryStatement {
            text: sql.clone(),
            start: 0,
            end: sql.chars().count(),
            client: false,
            params: Vec::new(),
            kept: None,
        };
        let query = engine(session)
            .run_query(&known.connection, ContextKind::Main, vec![statement], options)
            .map_err(|error| failed(&error))?;
        session.state_mut::<Relations>().lookup = Some(Lookup {
            source: result.id(),
            row: params.row,
            link: followed.link.clone(),
            columns: followed.columns.clone(),
            query,
            search: params.search,
            limit,
        });
        Ok(lookup_state(session))
    }
}

pub struct LookupStateCommand;

impl Command for LookupStateCommand {
    type Params = NoParams;
    type Output = LookupState;
    const ID: &'static str = "db.lookup.state";
    const TITLE: &'static str = "Key Lookup";
    const DESCRIPTION: &'static str = "The open lookup of db.lookup.start: the cell it is for, the referenced table and key, the search, the columns shown (the key first, at most eight), the rows read so far as text (cut to 200 characters, at most the limit, `truncated` when there are more), the query's status and error. `open: false` when none is open.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<LookupState, CommandError> {
        Ok(lookup_state(session))
    }
}

/// A lookup row to choose.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ChooseParams {
    /// The row of the lookup, from 0.
    pub row: usize,
}

pub struct LookupChoose;

impl Command for LookupChoose {
    type Params = ChooseParams;
    type Output = EditState;
    const ID: &'static str = "db.lookup.choose";
    const TITLE: &'static str = "Choose Key";
    const DESCRIPTION: &'static str = "Stages the key of lookup row `row` (from 0, of db.lookup.state) into the cells of the foreign key's columns of the edited row, every column of a composite key, and closes the lookup. Nothing is written until db.result.save. Codes `noLookup`, `noRow`, `readOnly`, `invalidValue`. Answers the editing state.";

    fn run(&self, session: &mut Session, params: ChooseParams) -> Result<EditState, CommandError> {
        let (rows, _) = lookup_rows(session);
        let Some((source, row, link, columns)) = session
            .state::<Relations>()
            .and_then(|relations| relations.lookup.as_ref())
            .map(|lookup| (lookup.source, lookup.row, lookup.link.clone(), lookup.columns.clone()))
        else {
            return Err(CommandError::with_code("noLookup", t!("database-error-no-lookup")));
        };
        let rows =
            rows.ok_or_else(|| CommandError::with_code("noRow", t!("database-error-lookup-row", row = params.row)))?;
        if params.row >= rows.len() {
            return Err(CommandError::with_code("noRow", t!("database-error-lookup-row", row = params.row)));
        }
        let names: Vec<String> = rows.columns().into_iter().map(|column| column.name).collect();
        let result = result_or_active(session, Some(source))?;
        let editing = edits::editable(session, &result)?;
        let mut staged = Vec::new();
        for (target, column) in link.target_columns.iter().zip(&columns) {
            let at = names
                .iter()
                .position(|name| name == target)
                .ok_or_else(|| CommandError::with_code("noRow", t!("database-error-lookup-row", row = params.row)))?;
            let value = rows.get(params.row, at);
            let type_name =
                editing.editing.columns.get(*column).map(|known| known.type_name.clone()).unwrap_or_default();
            let value = match value.display() {
                Some(text) => edit::parse_input(&text, &type_name).unwrap_or(value),
                None => Value::Null,
            };
            staged.push((*column, value));
        }
        for (column, value) in staged {
            if !result.stage(row, column, Staged::Value(value)) {
                return Err(no_row(&result, row));
            }
        }
        close_lookup(session);
        Ok(edits::state_of(session, &result))
    }
}

/// Whether a lookup was open.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LookupClosed {
    /// Whether one was open and is now closed.
    pub closed: bool,
}

pub struct LookupClose;

impl Command for LookupClose {
    type Params = NoParams;
    type Output = LookupClosed;
    const ID: &'static str = "db.lookup.close";
    const TITLE: &'static str = "Close Key Lookup";
    const DESCRIPTION: &'static str =
        "Closes the open lookup: cancels its query if it still reads and frees its rows; nothing is staged.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<LookupClosed, CommandError> {
        Ok(LookupClosed { closed: close_lookup(session) })
    }
}

#[cfg(test)]
mod tests;

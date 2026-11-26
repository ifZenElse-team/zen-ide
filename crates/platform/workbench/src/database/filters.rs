use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{DbError, ErrorKind};
use zen_db::engine::{OperationStatus, QueryOptions, QueryStatement};
use zen_db::filter::{self, Composition, FilterError, Operator, Sort, Source};
use zen_db::results::{ReadState, ResultSet};
use zen_db::value::Value;
use zen_i18n::t;

use super::WORKSPACE;
use super::connections::describe;
use super::consoles::{self, engine};
use super::objects::{self, ObjectTab};
use super::sources::{Database, find as find_entry};
use crate::workspaces::Workspaces;

const HISTORY: usize = 20;
const LISTED: usize = 50;
const FOUND: usize = 200;
const FIND_CELLS: usize = 2_000_000;
const DISTINCT_ROWS: usize = 100_000;

/// A filter the person saved for a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SavedFilter {
    /// Its name.
    pub name: String,
    /// The condition.
    pub condition: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct TableFilters {
    history: Vec<String>,
    saved: Vec<SavedFilter>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct FiltersFile {
    version: u32,
    tables: BTreeMap<String, TableFilters>,
}

impl Default for FiltersFile {
    fn default() -> Self {
        Self { version: 1, tables: BTreeMap::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Step {
    condition: String,
    sorts: Vec<Sort>,
}

#[derive(Clone, Debug)]
struct Pending {
    query: u64,
    old: u64,
}

#[derive(Clone, Debug, Default)]
struct View {
    base: Option<String>,
    condition: String,
    sorts: Vec<Sort>,
    applied: Option<String>,
    composition: Option<Composition>,
    back: Vec<Step>,
    forward: Vec<Step>,
    pending: Option<Pending>,
    error: Option<String>,
    client: bool,
}

#[derive(Default)]
pub struct Filters {
    views: HashMap<String, View>,
    file: Option<FiltersFile>,
}

enum Target {
    Object { tab: String, data: ObjectTab },
    Console { path: PathBuf },
    Plain,
}

struct Found {
    result: Arc<ResultSet>,
    target: Target,
    key: String,
    table: String,
    dialect: Option<SqlDialect>,
}

impl Found {
    fn sql(&self) -> Result<SqlDialect, CommandError> {
        self.dialect.ok_or_else(|| filter_error(FilterError::NotAQuery))
    }
}

pub(super) fn dialect_of(session: &mut Session, connection: &str) -> Option<SqlDialect> {
    let entry = find_entry(session, connection).ok()?;
    let drivers = session.state_mut::<Database>().drivers();
    let driver = drivers.iter().find(|driver| driver.id() == entry.source.driver)?;
    SqlDialect::of_language(&driver.capabilities().language)
}

fn file_path(session: &Session) -> Option<PathBuf> {
    session.state::<Workspaces>().and_then(Workspaces::data_dir).map(|data| data.join("databases").join("filters.json"))
}

fn stored(session: &mut Session) -> FiltersFile {
    if let Some(file) = session.state::<Filters>().and_then(|filters| filters.file.clone()) {
        return file;
    }
    let read = file_path(session)
        .and_then(|file| zen_db::registry::read_json::<FiltersFile>(&file).ok().flatten())
        .unwrap_or_default();
    session.state_mut::<Filters>().file = Some(read.clone());
    read
}

fn store(session: &mut Session, file: FiltersFile) -> Result<(), CommandError> {
    if let Some(path) = file_path(session) {
        zen_db::registry::write_json(&path, &file)
            .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
    }
    session.state_mut::<Filters>().file = Some(file);
    Ok(())
}

fn unknown_result(id: u64) -> CommandError {
    CommandError::with_code("unknownResult", t!("database-error-unknown-result", id = id))
}

pub fn active_result(session: &Session) -> Option<u64> {
    let tab = session.state::<Workspaces>()?.active_tab(WORKSPACE)?;
    if tab.kind == objects::KIND {
        return session.state::<objects::Objects>()?.shown(&tab.id);
    }
    consoles::shown_result(session, tab.path.as_deref()?)
}

pub(super) fn result_or_active(session: &Session, id: Option<u64>) -> Result<Arc<ResultSet>, CommandError> {
    let id = id
        .or_else(|| active_result(session))
        .ok_or_else(|| CommandError::with_code("noResult", t!("database-error-no-active-result")))?;
    session
        .state::<Database>()
        .and_then(Database::started)
        .and_then(|engine| engine.results().get(id))
        .ok_or_else(|| unknown_result(id))
        .and_then(|set| super::safety::agent_sees(session, set.connection()).map(|()| set))
}

fn found(session: &mut Session, id: Option<u64>) -> Result<Found, CommandError> {
    consoles::step(session);
    objects::step(session);
    let result = result_or_active(session, id)?;
    refuse_columns_unknown(&result)?;
    let connection = result.connection().to_owned();
    let dialect = dialect_of(session, &connection);
    if let Some((tab, data)) = objects::data_view(session, result.id()) {
        let table = objects::object_of(session, &data)
            .map(|(schema, object)| zen_db::schema::qualified(&schema, &object.name))
            .unwrap_or_else(|| data.path.last().cloned().unwrap_or_default());
        return Ok(Found {
            result,
            key: tab.clone(),
            table: format!("{connection}|{table}"),
            target: Target::Object { tab, data },
            dialect,
        });
    }
    let table = format!("{connection}|{}", result.table().unwrap_or("*"));
    let key = format!("result:{}", result.id());
    let target = match session.state::<consoles::Consoles>().and_then(|consoles| consoles.console_of(result.id())) {
        Some(path) => Target::Console { path: path.to_owned() },
        None => Target::Plain,
    };
    Ok(Found { result, target, key, table, dialect })
}

fn view(session: &Session, key: &str) -> View {
    session.state::<Filters>().and_then(|filters| filters.views.get(key).cloned()).unwrap_or_default()
}

pub(super) fn filtered(session: &Session, key: &str) -> bool {
    let kept = view(session, key);
    !kept.condition.trim().is_empty() || !kept.sorts.is_empty()
}

fn keep_view(session: &mut Session, key: &str, view: View) {
    session.state_mut::<Filters>().views.insert(key.to_owned(), view);
}

fn filter_error(error: FilterError) -> CommandError {
    let message = match error {
        FilterError::Semicolon => t!("database-error-filter-semicolon"),
        FilterError::Parentheses => t!("database-error-filter-parentheses"),
        FilterError::Unterminated => t!("database-error-filter-unterminated"),
        FilterError::NotAQuery => t!("database-error-filter-not-query"),
    };
    CommandError::with_code("invalidFilter", message).with_data(serde_json::json!({ "error": error }))
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

pub(super) fn table_statement(
    session: &mut Session,
    tab: &str,
    (name, order): (&str, Option<&str>),
    columns: &[String],
    dialect: SqlDialect,
) -> Result<String, CommandError> {
    let mut kept = view(session, tab);
    let condition = (!kept.condition.trim().is_empty()).then_some(kept.condition.as_str());
    let (sql, composition) = filter::compose(Source::Table { name, order }, condition, &kept.sorts, columns, dialect)
        .map_err(filter_error)?;
    kept.client = false;
    kept.applied = Some(sql.clone());
    kept.composition = Some(composition);
    kept.error = None;
    keep_view(session, tab, kept);
    Ok(sql)
}

pub(super) fn current(session: &Session, key: &str) -> (String, Vec<Sort>) {
    let kept = view(session, key);
    (kept.condition, kept.sorts)
}

pub(super) fn set_object_filter(
    session: &mut Session,
    (tab, table): (&str, &str),
    (condition, sorts): (String, Vec<Sort>),
    record: bool,
) -> Result<(), CommandError> {
    let mut kept = view(session, tab);
    let step = Step { condition: kept.condition.clone(), sorts: kept.sorts.clone() };
    let next = Step { condition: condition.trim().to_owned(), sorts };
    if let Some(pending) = kept.pending.take() {
        engine(session).cancel_query(pending.query);
    }
    if step != next && record {
        kept.back.push(step);
        kept.forward.clear();
    }
    kept.condition = next.condition.clone();
    kept.sorts = next.sorts;
    kept.client = false;
    keep_view(session, tab, kept);
    remember(session, table, &next.condition)
}

fn names(result: &ResultSet) -> Vec<String> {
    result.columns().into_iter().map(|column| column.name).collect()
}

fn remember(session: &mut Session, table: &str, condition: &str) -> Result<(), CommandError> {
    let condition = condition.trim();
    if condition.is_empty() {
        return Ok(());
    }
    let mut file = stored(session);
    let history = &mut file.tables.entry(table.to_owned()).or_default().history;
    history.retain(|known| known != condition);
    history.insert(0, condition.to_owned());
    history.truncate(HISTORY);
    store(session, file)
}

fn refuse_columns_unknown(result: &ResultSet) -> Result<(), CommandError> {
    if result.columns().is_empty() {
        return Err(CommandError::with_code("columnsPending", t!("database-error-columns-pending")));
    }
    Ok(())
}

pub(super) fn refuse_while_reading(session: &mut Session, result: &ResultSet) -> Result<(), CommandError> {
    consoles::step(session);
    objects::step(session);
    refuse_columns_unknown(result)?;
    let id = result.id();
    let filtering = session.state::<Filters>().is_some_and(|filters| {
        filters.views.values().any(|view| view.pending.as_ref().is_some_and(|pending| pending.old == id))
    });
    if filtering || session.state::<objects::Objects>().is_some_and(|objects| objects.rereading(id)) {
        return Err(CommandError::with_code("reading", t!("database-error-reading")));
    }
    Ok(())
}

fn refuse_pending_changes(result: &ResultSet) -> Result<(), CommandError> {
    if result.pending().any() {
        return Err(CommandError::with_code("pendingChanges", t!("database-error-filter-pending-changes")));
    }
    Ok(())
}

fn apply(
    session: &mut Session,
    found: &Found,
    (condition, sorts): (String, Vec<Sort>),
    record: bool,
) -> Result<FilterState, CommandError> {
    refuse_pending_changes(&found.result)?;
    let mut kept = view(session, &found.key);
    let step = Step { condition: kept.condition.clone(), sorts: kept.sorts.clone() };
    let next = Step { condition: condition.trim().to_owned(), sorts: sorts.clone() };
    if let Some(pending) = kept.pending.take() {
        engine(session).cancel_query(pending.query);
    }
    let changed = step != next;
    match &found.target {
        Target::Object { tab, data } => {
            let previous = kept.clone();
            kept.condition = next.condition.clone();
            kept.sorts = sorts;
            kept.client = false;
            keep_view(session, tab, kept.clone());
            if let Err(error) = objects::start_read(session, tab, data, true) {
                keep_view(session, tab, previous);
                return Err(error);
            }
            kept = view(session, tab);
        }
        Target::Console { .. } | Target::Plain => {
            let base = kept.base.clone().unwrap_or_else(|| found.result.statement_text().to_owned());
            let entry = super::sources::find(session, found.result.connection())?;
            if !engine(session).driver(&entry.source.driver).is_some_and(|driver| driver.repeatable(&base)) {
                return Err(filter_error(FilterError::NotAQuery));
            }
            let condition_text = (!next.condition.is_empty()).then_some(next.condition.as_str());
            let (sql, composition) =
                filter::compose(Source::Query(&base), condition_text, &sorts, &names(&found.result), found.sql()?)
                    .map_err(filter_error)?;
            let engine = engine(session);
            let schema = match &found.target {
                Target::Console { path } => consoles::schema_of(session, path),
                _ => None,
            };
            let place = match &found.target {
                Target::Console { path } => path.file_name().map(|name| name.to_string_lossy().into_owned()),
                _ => None,
            };
            super::safety::agent_reads(session, &entry, &sql)?;
            let provenance = super::history::provenance(session, zen_db::history::Source::Filter, place);
            let options = QueryOptions {
                stop_on_error: true,
                rollback_on_error: false,
                timeout: None,
                schema,
                provenance,
                read_only: false,
                notes: Vec::new(),
            };
            let params = super::variables::positional(found.result.statement_params(), found.sql()?);
            let statement = QueryStatement {
                text: sql.clone(),
                start: 0,
                end: sql.chars().count(),
                client: false,
                params,
                kept: None,
            };
            let query = engine
                .run_query(found.result.connection(), found.result.context().clone(), vec![statement], options)
                .map_err(|error| failed(&error))?;
            kept.base = Some(base);
            kept.condition = next.condition.clone();
            kept.sorts = sorts;
            kept.client = false;
            kept.applied = Some(sql);
            kept.composition = Some(composition);
            kept.error = None;
            kept.pending = Some(Pending { query, old: found.result.id() });
        }
    }
    if changed && record {
        kept.back.push(step);
        kept.forward.clear();
    }
    keep_view(session, &found.key, kept);
    remember(session, &found.table, &next.condition)?;
    Ok(state_of(session, found, LISTED))
}

pub(super) fn filter_tab<T>(
    session: &mut Session,
    (tab, table): (&str, &str),
    condition: &str,
    read: impl FnOnce(&mut Session) -> Result<T, CommandError>,
) -> Result<T, CommandError> {
    let previous = view(session, tab);
    if let Some(pending) = &previous.pending {
        engine(session).cancel_query(pending.query);
    }
    let step = Step { condition: previous.condition.clone(), sorts: previous.sorts.clone() };
    let mut kept = previous.clone();
    kept.condition = condition.trim().to_owned();
    kept.client = false;
    kept.pending = None;
    let changed = kept.condition != step.condition;
    if changed {
        kept.back.push(step);
        kept.forward.clear();
    }
    keep_view(session, tab, kept);
    match read(session) {
        Ok(answer) => {
            remember(session, table, condition)?;
            Ok(answer)
        }
        Err(error) => {
            keep_view(session, tab, previous);
            Err(error)
        }
    }
}

pub fn step(session: &mut Session) {
    let waiting: Vec<(String, Pending)> = session
        .state::<Filters>()
        .map(|filters| {
            filters.views.iter().filter_map(|(key, view)| Some((key.clone(), view.pending.clone()?))).collect()
        })
        .unwrap_or_default();
    if waiting.is_empty() {
        return;
    }
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else { return };
    for (key, pending) in waiting {
        let Some(query) = engine.query(pending.query) else {
            if let Some(view) = session.state_mut::<Filters>().views.get_mut(&key) {
                view.pending = None;
            }
            continue;
        };
        let statement = query.statements.first();
        let new = statement.and_then(|statement| statement.result).filter(|new| objects::columns_known(&engine, *new));
        if new.is_none() && query.status == OperationStatus::Running {
            continue;
        }
        let mut kept = session.state_mut::<Filters>().views.remove(&key).unwrap_or_default();
        kept.pending = None;
        match new {
            Some(new) => {
                let swapped = consoles::with_tab(session, pending.old, |tabs, active| {
                    if let Some(tab) = tabs.iter_mut().find(|tab| tab.result == pending.old) {
                        tab.result = new;
                    }
                    if *active == Some(consoles::PanelTab::Result(pending.old)) {
                        *active = Some(consoles::PanelTab::Result(new));
                    }
                });
                if swapped.is_some() {
                    engine.results().remove(pending.old);
                }
                session.state_mut::<Filters>().views.insert(format!("result:{new}"), kept);
            }
            None => {
                let error = statement.and_then(|statement| statement.error.as_ref()).or(query.error.as_ref());
                kept.error = Some(super::connections::unread(query.status, error));
                session.state_mut::<Filters>().views.insert(key, kept);
            }
        }
    }
}

/// A data view's filter as views and agents read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilterState {
    /// The result shown.
    pub id: u64,
    /// The condition in the filter box.
    pub condition: String,
    /// The sort, first column first.
    pub sorts: Vec<Sort>,
    /// Whether the sort is done in memory over every row loaded rather than by the server.
    pub client_sort: bool,
    /// The statement the rows were read with.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied: Option<String>,
    /// How the condition was put into it: `table`, `merged`, `wrapped` or `unchanged`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub composition: Option<Composition>,
    /// Whether the rows can be read again with a condition; when not, only finding and sorting loaded rows work.
    pub filterable: bool,
    /// The query reading the filtered rows, while it runs (cancel it with db.query.cancel).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// Why the last filter or read of the rows failed; the rows shown are those read before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether Back has a filter to go to.
    pub can_go_back: bool,
    /// Whether Forward has one.
    pub can_go_forward: bool,
    /// The table's conditions used last, newest first, at most `limit`.
    pub history: Vec<String>,
    /// The table's saved filters.
    pub saved: Vec<SavedFilter>,
    /// The columns whose conditions are in the filter, from 0.
    pub filtered_columns: Vec<usize>,
}

fn state_of(session: &mut Session, found: &Found, limit: usize) -> FilterState {
    let kept = view(session, &found.key);
    let (reading, failed) = match &found.target {
        Target::Object { tab, .. } => session
            .state::<objects::Objects>()
            .map_or((None, None), |objects| (objects.reading(tab), objects.failed(tab).map(str::to_owned))),
        _ => (None, None),
    };
    let file = stored(session);
    let table = file.tables.get(&found.table).cloned().unwrap_or_default();
    let filterable = match &found.target {
        Target::Object { .. } => true,
        _ => filter::filterable(kept.base.as_deref().unwrap_or(found.result.statement_text())),
    };
    let names = names(&found.result);
    FilterState {
        id: found.result.id(),
        filtered_columns: filter::columns_named(&kept.condition, &names, found.dialect.unwrap_or_default()),
        condition: kept.condition,
        sorts: kept.sorts,
        client_sort: kept.client,
        applied: kept.applied.or_else(|| Some(found.result.statement_text().to_owned())),
        composition: kept.composition,
        filterable,
        query: kept.pending.map(|pending| pending.query).or(reading),
        error: kept.error.or(failed),
        can_go_back: !kept.back.is_empty(),
        can_go_forward: !kept.forward.is_empty(),
        history: table.history.into_iter().take(limit).collect(),
        saved: table.saved,
    }
}

/// A result's data view.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ViewParams {
    /// The result's id; the result the Database workspace shows in front when absent.
    pub id: Option<u64>,
    /// The most history entries to answer; 50 when absent.
    pub limit: Option<usize>,
}

pub struct FilterStateCommand;

impl Command for FilterStateCommand {
    type Params = ViewParams;
    type Output = FilterState;
    const ID: &'static str = "db.result.filterState";
    const TITLE: &'static str = "Result Filter";
    const DESCRIPTION: &'static str = "The WHERE filter of a result's data view (`id`, else the one shown in front): the condition, the sort, whether it is sorted in memory, the statement the rows were read with (`applied`) and how the condition went into it (`table`: the table's generated SELECT; `merged` into a plain SELECT; `wrapped` around another query as a subselect), whether the view can be filtered, the query reading new rows (cancel with db.query.cancel), the last error, Back and Forward, the table's history (newest first, at most `limit`, 50) and saved filters, and the columns the condition names. Never data values.";

    fn run(&self, session: &mut Session, params: ViewParams) -> Result<FilterState, CommandError> {
        let found = found(session, params.id)?;
        Ok(state_of(session, &found, params.limit.unwrap_or(LISTED)))
    }
}

/// A condition to filter by.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FilterParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The condition: an SQL expression as after `WHERE` (`total > 10 AND name LIKE 'A%'`); empty clears the filter.
    pub condition: String,
}

pub struct ApplyFilter;

impl Command for ApplyFilter {
    type Params = FilterParams;
    type Output = FilterState;
    const ID: &'static str = "db.result.filter";
    const TITLE: &'static str = "Apply Filter";
    const DESCRIPTION: &'static str = "Filters a result's data view (`id`, else the one shown) by `condition`, an SQL expression as after WHERE; empty clears it. The condition must be one balanced expression (no `;`, matching parentheses and quotes; refused with code `invalidFilter`). An object tab's table is read again with the condition in its generated SELECT; a console's plain SELECT gets it merged into its WHERE, any other query is wrapped as a subselect; a statement that does not only read cannot be filtered. The rows are read again in the background in the same session and replace the shown ones when they come (`query` while reading); staged data changes must be saved or cancelled first (code `pendingChanges`). The condition goes into the table's history (20 kept, in the data folder). Answers the filter state.";

    fn run(&self, session: &mut Session, params: FilterParams) -> Result<FilterState, CommandError> {
        let found = found(session, params.id)?;
        let kept = view(session, &found.key);
        apply(session, &found, (params.condition, kept.sorts), true)
    }
}

/// How to sort by a column.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    /// Smallest first.
    Asc,
    /// Largest first.
    Desc,
    /// Not by this column.
    None,
}

/// A column to sort by.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SortParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The column, from 0.
    pub column: usize,
    /// The direction; the next of ascending, descending and none after the column's current one when absent, as a header click cycles.
    pub direction: Option<Direction>,
    /// Add the column to the sort (⇧-click) instead of sorting by it alone.
    pub add: bool,
}

pub struct SortColumn;

impl Command for SortColumn {
    type Params = SortParams;
    type Output = FilterState;
    const ID: &'static str = "db.result.sort";
    const TITLE: &'static str = "Sort by Column";
    const DESCRIPTION: &'static str = "Sorts a result's data view (`id`, else the one shown) by `column` (from 0): `direction` `asc`, `desc` or `none`, else the next one after its current direction as a header click cycles; `add` adds it to the sort (⇧-click) instead of replacing it. When every row is loaded the rows are sorted in memory (`clientSort`), NULL largest as the server sorts; otherwise the rows are read again in the background with ORDER BY (by name, or by position when two columns share a name). A result that cannot be read again and is not fully loaded cannot be sorted (code `cannotSort`).";

    fn run(&self, session: &mut Session, params: SortParams) -> Result<FilterState, CommandError> {
        let found = found(session, params.id)?;
        let columns = found.result.columns();
        let name = columns.get(params.column).map(|column| column.name.clone()).ok_or_else(|| {
            CommandError::with_code("noColumn", t!("database-error-no-column", column = params.column))
        })?;
        let mut kept = view(session, &found.key);
        let current = kept.sorts.iter().find(|sort| sort.column == params.column).map(|sort| sort.descending);
        let direction = params.direction.unwrap_or(match current {
            None => Direction::Asc,
            Some(false) => Direction::Desc,
            Some(true) => Direction::None,
        });
        let mut sorts: Vec<Sort> = if params.add { kept.sorts.clone() } else { Vec::new() };
        sorts.retain(|sort| sort.column != params.column);
        if direction != Direction::None {
            let sort = Sort { column: params.column, name, descending: direction == Direction::Desc };
            match kept.sorts.iter().position(|known| known.column == params.column).filter(|_| params.add) {
                Some(at) => sorts.insert(at.min(sorts.len()), sort),
                None => sorts.push(sort),
            }
        }
        let info = found.result.info();
        let loaded = info.state == ReadState::Done && !info.more;
        let filterable = matches!(found.target, Target::Object { .. })
            || filter::filterable(kept.base.as_deref().unwrap_or(found.result.statement_text()));
        if loaded || !filterable {
            if !loaded {
                return Err(CommandError::with_code("cannotSort", t!("database-error-cannot-sort")));
            }
            let keys: Vec<(usize, bool)> = sorts.iter().map(|sort| (sort.column, sort.descending)).collect();
            if !found.result.sort_rows(&keys) {
                return Err(CommandError::with_code("cannotSort", t!("database-error-cannot-sort")));
            }
            kept.sorts = sorts;
            kept.client = true;
            keep_view(session, &found.key, kept);
            return Ok(state_of(session, &found, LISTED));
        }
        let condition = kept.condition.clone();
        apply(session, &found, (condition, sorts), true)
    }
}

/// A quick filter from a cell.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ByValueParams {
    /// The result's id; the result shown in front when absent.
    #[serde(default)]
    pub id: Option<u64>,
    /// The column, from 0.
    pub column: usize,
    /// The row whose value is used, from 0; also the first of `rows` for `in`.
    #[serde(default)]
    pub row: Option<usize>,
    /// More rows whose values `in` takes (a selection).
    #[serde(default)]
    pub rows: Vec<usize>,
    /// `equal`, `notEqual`, `greater`, `less`, `contains`, `isNull`, `isNotNull` or `in`.
    pub operator: Operator,
    /// A value typed or pasted instead of the cell's (a number for numeric columns when it reads as one, else text).
    #[serde(default)]
    pub value: Option<String>,
}

pub struct FilterByValue;

impl Command for FilterByValue {
    type Params = ByValueParams;
    type Output = FilterState;
    const ID: &'static str = "db.result.filterByValue";
    const TITLE: &'static str = "Filter by Value";
    const DESCRIPTION: &'static str = "Adds a condition on `column` to a data view's filter with AND and reads the rows again: `equal`, `notEqual`, `greater`, `less` the value of cell (`row`, `column`), or `value` given (typed or from the clipboard); `contains` (LIKE '%value%' ESCAPE '!' on its text, % and _ escaped); `isNull`, `isNotNull`; `in` the values of `row` and `rows` (NULL among them adds OR IS NULL). Names and values are written in the connection's SQL dialect (quotes, literals, casts to text). Answers the filter state.";

    fn run(&self, session: &mut Session, params: ByValueParams) -> Result<FilterState, CommandError> {
        let found = found(session, params.id)?;
        let columns = found.result.columns();
        let column = columns.get(params.column).cloned().ok_or_else(|| {
            CommandError::with_code("noColumn", t!("database-error-no-column", column = params.column))
        })?;
        let numeric = matches!(
            found.result.get(params.row.unwrap_or(0), params.column),
            Value::Integer(_) | Value::Float(_) | Value::Decimal(_)
        );
        let values: Vec<Value> = match (&params.value, params.operator) {
            (Some(text), _) => vec![filter::typed_value(text, numeric)],
            (None, Operator::In) => params
                .row
                .into_iter()
                .chain(params.rows.iter().copied())
                .map(|row| found.result.get(row, params.column))
                .collect(),
            (None, Operator::IsNull | Operator::IsNotNull) => Vec::new(),
            (None, _) => {
                let row =
                    params.row.ok_or_else(|| CommandError::with_code("noRow", t!("database-error-filter-no-row")))?;
                vec![found.result.get(row, params.column)]
            }
        };
        let dialect = found.sql()?;
        let added = filter::quick(&column.name, params.operator, &values, dialect);
        let kept = view(session, &found.key);
        let condition = filter::and(&kept.condition, &added, dialect);
        apply(session, &found, (condition, kept.sorts), true)
    }
}

/// What filter to reset.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResetParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// Only this column's conditions (from 0); the whole filter and sort when absent.
    pub column: Option<usize>,
}

pub struct ResetFilter;

impl Command for ResetFilter {
    type Params = ResetParams;
    type Output = FilterState;
    const ID: &'static str = "db.result.resetFilter";
    const TITLE: &'static str = "Reset Filter";
    const DESCRIPTION: &'static str = "Clears a data view's filter and sort and reads the rows again (`id`, else the one shown), only when its statement just reads (else code `invalidFilter`); with `column`, removes only the top-level AND parts of the condition that name that column (and its sort). Answers the filter state.";

    fn run(&self, session: &mut Session, params: ResetParams) -> Result<FilterState, CommandError> {
        let found = found(session, params.id)?;
        let kept = view(session, &found.key);
        match params.column {
            None => apply(session, &found, (String::new(), Vec::new()), true),
            Some(column) => {
                let name = found.result.columns().get(column).map(|column| column.name.clone()).ok_or_else(|| {
                    CommandError::with_code("noColumn", t!("database-error-no-column", column = column))
                })?;
                let condition = filter::without_column(&kept.condition, &name, found.sql()?);
                let sorts = kept.sorts.into_iter().filter(|sort| sort.column != column).collect();
                apply(session, &found, (condition, sorts), true)
            }
        }
    }
}

fn travel(session: &mut Session, id: Option<u64>, back: bool) -> Result<FilterState, CommandError> {
    let found = found(session, id)?;
    let mut kept = view(session, &found.key);
    let target = if back { kept.back.pop() } else { kept.forward.pop() };
    let Some(target) = target else { return Ok(state_of(session, &found, LISTED)) };
    let current = Step { condition: kept.condition.clone(), sorts: kept.sorts.clone() };
    if back {
        kept.forward.push(current);
    } else {
        kept.back.push(current);
    }
    keep_view(session, &found.key, kept);
    apply(session, &found, (target.condition, target.sorts), false)
}

pub struct FilterBack;

impl Command for FilterBack {
    type Params = ViewParams;
    type Output = FilterState;
    const ID: &'static str = "db.result.filterBack";
    const TITLE: &'static str = "Previous Filter";
    const DESCRIPTION: &'static str = "Goes back to the filter and sort a data view had before (`id`, else the one shown) and reads the rows again; Forward returns. Answers the filter state.";

    fn run(&self, session: &mut Session, params: ViewParams) -> Result<FilterState, CommandError> {
        travel(session, params.id, true)
    }
}

pub struct FilterForward;

impl Command for FilterForward {
    type Params = ViewParams;
    type Output = FilterState;
    const ID: &'static str = "db.result.filterForward";
    const TITLE: &'static str = "Next Filter";
    const DESCRIPTION: &'static str = "Goes forward to the filter a data view had before Back (`id`, else the one shown) and reads the rows again. Answers the filter state.";

    fn run(&self, session: &mut Session, params: ViewParams) -> Result<FilterState, CommandError> {
        travel(session, params.id, false)
    }
}

/// Text to complete in the filter box.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CompleteParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The filter box's text.
    pub text: String,
    /// The caret, a character offset into `text`; its end when absent.
    pub caret: Option<usize>,
    /// The most completions; 50 when absent.
    pub limit: Option<usize>,
}

/// Completions of the word at the caret.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompleteAnswer {
    /// The first character of the word they replace.
    pub start: usize,
    /// The character after it.
    pub end: usize,
    /// The completions: columns first (quoted as SQL needs), then keywords.
    pub items: Vec<filter::Completion>,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct CompleteFilter;

impl Command for CompleteFilter {
    type Params = CompleteParams;
    type Output = CompleteAnswer;
    const ID: &'static str = "db.result.completeFilter";
    const TITLE: &'static str = "Complete Filter";
    const DESCRIPTION: &'static str = "Completions of the word before `caret` in the filter box text `text` of a data view (`id`, else the one shown): its columns with their types, quoted as SQL needs, then the keywords AND, OR, NOT, IS, NULL, BETWEEN, LIKE, ILIKE, IN, TRUE, FALSE, EXISTS; at most `limit` (50), with the character range they replace.";

    fn run(&self, session: &mut Session, params: CompleteParams) -> Result<CompleteAnswer, CommandError> {
        let result = result_or_active(session, params.id)?;
        let dialect = dialect_of(session, result.connection()).unwrap_or_default();
        let columns: Vec<(String, String)> =
            result.columns().into_iter().map(|column| (column.name, column.type_name)).collect();
        let caret = params.caret.unwrap_or_else(|| params.text.chars().count());
        let (start, end, mut items) = filter::complete(&params.text, caret, &columns, dialect);
        let limit = params.limit.unwrap_or(LISTED);
        let truncated = items.len() > limit;
        items.truncate(limit);
        Ok(CompleteAnswer { start, end, items, truncated })
    }
}

/// A column to list the values of.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DistinctParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The column, from 0.
    pub column: usize,
    /// The most values; 50 when absent, at most 1000.
    pub limit: Option<usize>,
}

/// A value of a column with how often it occurs.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DistinctValue {
    /// The value as text, cut to 200 characters; absent for NULL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A row that has it, for db.result.filterByValue.
    pub row: usize,
    /// How many of the rows loaded have it.
    pub count: usize,
}

/// The values of a column.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DistinctAnswer {
    /// The values, the most frequent first.
    pub values: Vec<DistinctValue>,
    /// Whether values were left out because of `limit`.
    pub truncated: bool,
    /// Whether rows remain on the server, so that only the rows loaded were counted.
    pub partial: bool,
}

pub struct DistinctValues;

impl Command for DistinctValues {
    type Params = DistinctParams;
    type Output = DistinctAnswer;
    const ID: &'static str = "db.result.distinctValues";
    const TITLE: &'static str = "Column Values";
    const DESCRIPTION: &'static str = "The distinct values of `column` in the rows a data view has loaded (`id`, else the one shown; at most 100 000 rows looked at), the most frequent first, each with a row that has it and its count; at most `limit` (50, at most 1000). `partial` when rows remain on the server. Filter by some of them with db.result.filterByValue `in` and their rows.";

    fn run(&self, session: &mut Session, params: DistinctParams) -> Result<DistinctAnswer, CommandError> {
        let result = result_or_active(session, params.id)?;
        refuse_columns_unknown(&result)?;
        if params.column >= result.columns().len() {
            return Err(CommandError::with_code("noColumn", t!("database-error-no-column", column = params.column)));
        }
        let rows = result.len().min(DISTINCT_ROWS);
        let mut counted: HashMap<Option<String>, (usize, usize)> = HashMap::new();
        for (row, values) in result.page(0, rows).into_iter().enumerate() {
            let text = values.get(params.column).and_then(Value::display);
            counted.entry(text).or_insert((row, 0)).1 += 1;
        }
        let mut values: Vec<DistinctValue> = counted
            .into_iter()
            .map(|(text, (row, count))| DistinctValue {
                text: text.map(|text| text.chars().take(200).collect()),
                row,
                count,
            })
            .collect();
        values.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.row.cmp(&b.row)));
        let limit = params.limit.unwrap_or(LISTED).min(1000);
        let truncated = values.len() > limit;
        values.truncate(limit);
        Ok(DistinctAnswer { values, truncated, partial: result.info().more || result.len() > DISTINCT_ROWS })
    }
}

/// Text to find.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FindParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The text.
    pub text: String,
    /// Match upper and lower case exactly.
    pub match_case: bool,
    /// The most cells; 200 when absent.
    pub limit: Option<usize>,
}

/// Cells found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FindAnswer {
    /// The cells: row and column, from 0, in reading order.
    pub cells: Vec<(usize, usize)>,
    /// Whether more cells may match: the limit or the look-up cap was reached.
    pub truncated: bool,
}

pub struct FindInResult;

impl Command for FindInResult {
    type Params = FindParams;
    type Output = FindAnswer;
    const ID: &'static str = "db.result.find";
    const TITLE: &'static str = "Find in Results";
    const DESCRIPTION: &'static str = "Finds `text` in the cells a data view has loaded (`id`, else the one shown), in memory without asking the server: the cells whose text contains it (ignoring case unless `matchCase`) as row and column, at most `limit` (200), looking at no more than two million cells (`truncated`). Never the values themselves.";

    fn run(&self, session: &mut Session, params: FindParams) -> Result<FindAnswer, CommandError> {
        let result = result_or_active(session, params.id)?;
        if params.text.is_empty() {
            return Ok(FindAnswer { cells: Vec::new(), truncated: false });
        }
        let (cells, truncated) =
            result.find(&params.text, params.match_case, params.limit.unwrap_or(FOUND).max(1), FIND_CELLS);
        Ok(FindAnswer { cells, truncated })
    }
}

/// A filter to save.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveFilterParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// Its name; a filter of the same name is replaced.
    pub name: String,
    /// The condition; the data view's current one when absent.
    pub condition: Option<String>,
}

pub struct SaveFilter;

impl Command for SaveFilter {
    type Params = SaveFilterParams;
    type Output = FilterState;
    const ID: &'static str = "db.filter.save";
    const TITLE: &'static str = "Save Filter";
    const DESCRIPTION: &'static str = "Saves a condition (`condition`, else the data view's current one) under `name` for the table of a data view (`id`, else the one shown), in the data folder; a filter of the same name is replaced. Apply it with db.result.filter. Answers the filter state with the saved filters.";

    fn run(&self, session: &mut Session, params: SaveFilterParams) -> Result<FilterState, CommandError> {
        let found = found(session, params.id)?;
        let name = params.name.trim().to_owned();
        if name.is_empty() {
            return Err(CommandError::new(t!("database-error-no-name")));
        }
        let condition = params.condition.unwrap_or_else(|| view(session, &found.key).condition).trim().to_owned();
        filter::check(&condition, found.sql()?).map_err(filter_error)?;
        if condition.is_empty() {
            return Err(CommandError::with_code("noCondition", t!("database-error-filter-empty")));
        }
        let mut file = stored(session);
        let saved = &mut file.tables.entry(found.table.clone()).or_default().saved;
        saved.retain(|known| known.name != name);
        saved.push(SavedFilter { name, condition });
        store(session, file)?;
        Ok(state_of(session, &found, LISTED))
    }
}

/// A saved filter to delete.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DeleteFilterParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The filter's name.
    pub name: String,
}

pub struct DeleteFilter;

impl Command for DeleteFilter {
    type Params = DeleteFilterParams;
    type Output = FilterState;
    const ID: &'static str = "db.filter.delete";
    const TITLE: &'static str = "Delete Saved Filter";
    const DESCRIPTION: &'static str = "Deletes the saved filter `name` of the table of a data view (`id`, else the one shown); nothing else changes. Answers the filter state.";

    fn run(&self, session: &mut Session, params: DeleteFilterParams) -> Result<FilterState, CommandError> {
        let found = found(session, params.id)?;
        let mut file = stored(session);
        if let Some(table) = file.tables.get_mut(&found.table) {
            table.saved.retain(|known| known.name != params.name);
        }
        store(session, file)?;
        Ok(state_of(session, &found, LISTED))
    }
}

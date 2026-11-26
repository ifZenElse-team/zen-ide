use std::path::PathBuf;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{ConnectionState, OperationStatus, QueryOptions, QueryState, QueryStatement};
use zen_db::results::{ResultInfo, ResultSet};
use zen_db::sql::Dialect;
use zen_db::value::Value;
use zen_i18n::t;

use super::connections::{describe, request};
use super::consoles::{self, PanelTab, ResultTab, ResultView, engine, resolve, settings, with_tab};
use super::safety::{Confirmation, Run};
use super::sources::{Database, find};

const MOST_WAIT_MS: u64 = 60_000;
const ROWS_ANSWERED: usize = 50;
const CELL_CHARS: usize = 1000;

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        ErrorKind::Unsupported if error.code.as_deref() == Some(zen_db::results::NOT_REPEATABLE) => {
            zen_db::results::NOT_REPEATABLE
        }
        ErrorKind::Unsupported => "unsupported",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn result_of(session: &Session, id: u64) -> Result<std::sync::Arc<ResultSet>, CommandError> {
    session
        .state::<Database>()
        .and_then(Database::started)
        .and_then(|engine| engine.results().get(id))
        .ok_or_else(|| CommandError::with_code("unknownResult", t!("database-error-unknown-result", id = id)))
        .and_then(|set| super::safety::agent_sees(session, set.connection()).map(|()| set))
}

fn cut_value(value: Value, cut: &mut bool) -> Value {
    let shorten = |text: String, cut: &mut bool| {
        if text.chars().count() > CELL_CHARS {
            *cut = true;
            text.chars().take(CELL_CHARS).collect()
        } else {
            text
        }
    };
    match value {
        Value::Text(text) => Value::Text(shorten(text, cut)),
        Value::Json(text) => Value::Json(shorten(text, cut)),
        Value::Bytes(bytes) if bytes.len() > CELL_CHARS / 2 => {
            *cut = true;
            Value::Bytes(bytes[..CELL_CHARS / 2].to_vec())
        }
        Value::Array(items) if items.len() > 100 => {
            *cut = true;
            Value::Array(items.into_iter().take(100).collect())
        }
        other => other,
    }
}

/// Rows of a result as agents read them.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// The result without its rows.
    #[serde(flatten)]
    pub info: ResultInfo,
    /// The first row answered.
    pub offset: usize,
    /// The rows, each a list of values (`{"kind": "integer", "value": 1}`); long texts cut to 1000 characters.
    pub page: Vec<Vec<Value>>,
    /// Whether rows were left out after this page: kept ones, or rows still on the server (`more`).
    pub truncated: bool,
    /// Whether some values were cut.
    pub cells_cut: bool,
}

fn page_of(result: &ResultSet, offset: usize, limit: usize) -> Page {
    let info = result.info();
    let mut cells_cut = false;
    let page: Vec<Vec<Value>> = result
        .page(offset, limit)
        .into_iter()
        .map(|row| row.into_iter().map(|value| cut_value(value, &mut cells_cut)).collect())
        .collect();
    let truncated = (offset + page.len()) < info.rows as usize || info.more;
    Page { info, offset, page, truncated, cells_cut }
}

/// What `db.query.run` runs.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunParams {
    /// The connection's id.
    pub connection: String,
    /// The SQL: one statement or a script, split as Execute Script splits it.
    pub sql: String,
    /// The schema to run in.
    pub schema: Option<String>,
    /// How long to wait for the end, in milliseconds (at most 60 000); answers at once when absent.
    pub wait_ms: Option<u64>,
    /// The most rows of each result to answer; 50 when absent.
    pub limit: Option<usize>,
    /// The session to run in, by name; `agent` when absent. The same name keeps its transaction and settings between calls.
    pub session: Option<String>,
    /// Go on after an error instead of stopping (and rolling back).
    pub continue_on_error: bool,
    /// The `token` of a `confirmExecute` refusal, where the connection type lets agents confirm.
    pub confirmed: Option<String>,
    /// Values of bind parameters and `${name}` variables.
    #[serde(flatten)]
    pub given: super::variables::Given,
}

/// A query with the first rows of its results.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QueryAnswer {
    /// The query: its statements with their status, times and errors, its output.
    #[serde(flatten)]
    pub query: QueryState,
    /// Its results with their first rows.
    pub results: Vec<Page>,
    /// The first error, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Whether output messages were left out (at most 50 are answered).
    pub messages_truncated: bool,
}

fn answer_query(session: &Session, mut query: QueryState, limit: usize) -> QueryAnswer {
    let engine = session.state::<Database>().and_then(Database::started).cloned();
    let results = query
        .statements
        .iter()
        .filter_map(|statement| statement.result)
        .filter_map(|id| engine.as_ref().and_then(|engine| engine.results().get(id)))
        .map(|result| page_of(&result, 0, limit))
        .collect();
    let message =
        query.statements.iter().find_map(|statement| statement.error.as_ref()).or(query.error.as_ref()).map(describe);
    let messages_truncated = query.messages.len() > ROWS_ANSWERED;
    query.messages.truncate(ROWS_ANSWERED);
    QueryAnswer { query, results, message, messages_truncated }
}

fn wait_for(session: &Session, query: u64, wait_ms: Option<u64>) -> Option<QueryState> {
    let engine = session.state::<Database>().and_then(Database::started)?.clone();
    let deadline = Instant::now() + Duration::from_millis(wait_ms.unwrap_or(0).min(MOST_WAIT_MS));
    loop {
        let state = engine.query(query)?;
        if state.status != OperationStatus::Running || Instant::now() >= deadline {
            return Some(state);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub struct RunQuery;

impl Command for RunQuery {
    type Params = RunParams;
    type Output = QueryAnswer;
    const ID: &'static str = "db.query.run";
    const TITLE: &'static str = "Run SQL";
    const DESCRIPTION: &'static str = "Runs `sql` (one statement or a script, split as Execute Script splits it; `@set name = value` and `@include file.sql` lines and `${name}` variables applied) on connection `connection` in the background, in a session of its own named `session` (`agent`), which keeps its transaction between calls; connects first when needed with the stored password (refused with code `passwordNeeded` otherwise: call db.connection.connect with `password`). An error stops the script and rolls back unless `continueOnError`. Waits up to `waitMs` (60 000 at most) and answers the query: each statement's status, times, rows changed and error (with the server's position), the output (NOTICE, RAISE), and each result with its first `limit` (50) rows; long values are cut. Read more with db.result.page. Safety: the connection type says what agents may run (`agents`: `full`, `readOnly` — Production's default —, `none`). Where agents only read, the session is read-only on the server and a change is refused with code `agentReadOnly` and `handOff` (db.console.new with the SQL, for a person to review and run); `none` refuses everything (`agentNotAllowed`); a read-only connection refuses changes (`readOnly`). Where the type confirms SQL execution, and for DELETE or UPDATE without WHERE, DROP and TRUNCATE on every type, the call is refused with code `confirmExecute`, the statements and their risks: with `full` access pass the refusal's `token` as `confirmed` to run them. Every refusal is kept in the query history, marked as an agent's. Placeholders (`:name`; `$1` on PostgreSQL; `?` elsewhere) are bound as real parameters, with values from `parameters` (`{\"id\": 5, \"name\": \"x\", \"since\": {\"type\": \"text\", \"value\": \"2026-01-01\"}, \"gone\": null}`); a missing value or variable is refused with code `parametersNeeded` listing them.";

    fn run(&self, session: &mut Session, params: RunParams) -> Result<QueryAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        let engine = engine(session);
        let dialect = engine
            .driver(&entry.source.driver)
            .map_or(Dialect::Standard, |driver| Dialect::of_language(&driver.capabilities().language));
        let text = params.sql;
        let statements: Vec<QueryStatement> = zen_db::sql::split(&text, dialect)
            .iter()
            .map(|statement| QueryStatement {
                text: statement.text(&text).to_owned(),
                start: text[..statement.start].chars().count(),
                end: text[..statement.end].chars().count(),
                client: statement.client,
                params: Vec::new(),
                kept: None,
            })
            .collect();
        if statements.is_empty() {
            return Err(CommandError::with_code("noStatement", t!("database-error-no-statement")));
        }
        let asking = serde_json::json!({
            "connection": params.connection,
            "sql": text,
            "schema": params.schema,
            "waitMs": params.wait_ms,
            "limit": params.limit,
            "session": params.session,
            "continueOnError": params.continue_on_error,
        });
        let prepared = super::variables::prepare(
            session,
            None,
            &params.connection,
            statements,
            (&params.given, (Self::ID, asking)),
        )?;
        let stop = !params.continue_on_error;
        let name = params.session.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| "agent".to_owned());
        let context = ContextKind::Console(name.clone());
        let mut provenance = super::history::provenance(session, zen_db::history::Source::Query, Some(name.clone()));
        let confirmation = Confirmation { confirmed: params.confirmed.clone(), skip: Vec::new() };
        let mut retry = serde_json::json!({
            "connection": params.connection,
            "sql": text,
            "schema": params.schema,
            "waitMs": params.wait_ms,
            "limit": params.limit,
            "session": name,
            "continueOnError": params.continue_on_error,
        });
        params.given.add_to(&mut retry);
        let run = Run {
            entry: &entry,
            context: &context,
            provenance: &provenance,
            confirmation: &confirmation,
            retry: (Self::ID, retry),
        };
        let guarded = super::safety::guard(session, &run, prepared.statements)?;
        provenance.confirmed = guarded.confirmed;
        if matches!(engine.status(&params.connection).state, ConnectionState::Disconnected | ConnectionState::Failed) {
            let request = request(session, &entry, None);
            engine.connect(request).map_err(|error| failed(&error))?;
        }
        let options = QueryOptions {
            stop_on_error: stop,
            rollback_on_error: stop,
            timeout: None,
            schema: params.schema,
            provenance,
            read_only: guarded.read_only,
            notes: prepared.notes,
        };
        let query = engine
            .run_query(&params.connection, context, guarded.statements, options)
            .map_err(|error| failed(&error))?;
        let state = wait_for(session, query, params.wait_ms)
            .ok_or_else(|| CommandError::new(t!("database-error-unknown-query", id = query)))?;
        Ok(answer_query(session, state, params.limit.unwrap_or(ROWS_ANSWERED)))
    }
}

/// Which query.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct QueryParams {
    /// The query's id.
    pub query: u64,
    /// How long to wait for its end, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
    /// The most rows of each result to answer; 50 when absent.
    pub limit: Option<usize>,
}

pub struct QueryStateCommand;

impl Command for QueryStateCommand {
    type Params = QueryParams;
    type Output = QueryAnswer;
    const ID: &'static str = "db.query.state";
    const TITLE: &'static str = "Query State";
    const DESCRIPTION: &'static str = "A query run by db.query.run or a console (db.console.execute…), waiting up to `waitMs` for its end: its statements (status, times, rows changed, result, error with the server's position), its output and each result with its first `limit` (50) rows. The last 100 queries are kept.";

    fn run(&self, session: &mut Session, params: QueryParams) -> Result<QueryAnswer, CommandError> {
        let state = wait_for(session, params.query, params.wait_ms).ok_or_else(|| {
            CommandError::with_code("unknownQuery", t!("database-error-unknown-query", id = params.query))
        })?;
        super::safety::agent_sees(session, &state.connection)?;
        consoles::step(session);
        Ok(answer_query(session, state, params.limit.unwrap_or(ROWS_ANSWERED)))
    }
}

/// The query to cancel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CancelParams {
    /// The query's id.
    pub query: u64,
}

/// Whether a query was asked to stop.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Whether it was running or had a portal open.
    pub cancelled: bool,
}

pub struct CancelQuery;

impl Command for CancelQuery {
    type Params = CancelParams;
    type Output = Cancelled;
    const ID: &'static str = "db.query.cancel";
    const TITLE: &'static str = "Cancel SQL";
    const DESCRIPTION: &'static str = "Cancels a query: its running statement stops on the server too (within a second; db.query.state then says cancelled), the rest of its script is not run, and an open portal of its results is closed.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<Cancelled, CommandError> {
        let cancelled = session
            .state::<Database>()
            .and_then(Database::started)
            .is_some_and(|engine| engine.cancel_query(params.query));
        Ok(Cancelled { cancelled })
    }
}

/// Which results to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// Only the result tabs of this console; with neither `path` nor `query`, every result of the window.
    pub path: Option<PathBuf>,
    /// Only the results of this query.
    pub query: Option<u64>,
    /// The most results to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// A result as listed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListedResult {
    /// The result without its rows.
    #[serde(flatten)]
    pub info: ResultInfo,
    /// Its tab's title in its console.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Whether its tab is pinned.
    pub pinned: bool,
    /// The console it belongs to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console: Option<PathBuf>,
}

/// Results.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAnswer {
    /// The results, oldest first.
    pub results: Vec<ListedResult>,
    /// The memory results take, in bytes.
    pub bytes: usize,
    /// The window's budget for results, in bytes.
    pub budget: usize,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct ListResults;

impl Command for ListResults {
    type Params = ListParams;
    type Output = ListAnswer;
    const ID: &'static str = "db.results.list";
    const TITLE: &'static str = "List Results";
    const DESCRIPTION: &'static str = "Lists results without their rows: a console's result tabs (`path`), a query's results (`query`), or every result of the window. Each has its id, columns with their types, rows kept, state (reading, open, done, stopped and why), `more` (rows left on the server: `200+`), the total once counted, execution and fetch milliseconds, memory, and its tab's title and pin. Also the memory all results take against the budget. Read rows with db.result.page.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ListAnswer, CommandError> {
        consoles::step(session);
        let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else {
            return Ok(ListAnswer {
                results: Vec::new(),
                bytes: 0,
                budget: zen_db::results::DEFAULT_BUDGET,
                total: 0,
                truncated: false,
            });
        };
        let path = params.path.map(|path| resolve(session, Some(path))).transpose()?;
        let tabs_of = |session: &Session, id: u64| -> (Option<ResultTab>, Option<PathBuf>) {
            let consoles = session.state::<consoles::Consoles>();
            let console = consoles.and_then(|consoles| consoles.console_of(id)).map(PathBuf::from);
            let tab = consoles.and_then(|consoles| {
                console.as_ref().and_then(|path| consoles.tabs(path).iter().find(|tab| tab.result == id).cloned())
            });
            (tab, console)
        };
        let mut listed: Vec<ListedResult> = Vec::new();
        for result in engine.results().all() {
            let info = result.info();
            if params.query.is_some_and(|query| query != info.query) {
                continue;
            }
            let (tab, console) = tabs_of(session, info.id);
            if let Some(path) = &path
                && console.as_ref() != Some(path)
            {
                continue;
            }
            if path.is_some() && tab.is_none() {
                continue;
            }
            listed.push(ListedResult {
                title: tab.as_ref().map(|tab| tab.title.clone()),
                pinned: tab.is_some_and(|tab| tab.pinned),
                console,
                info,
            });
        }
        if let Some(path) = &path {
            let order: Vec<u64> = session
                .state::<consoles::Consoles>()
                .map(|consoles| consoles.tabs(path).iter().map(|tab| tab.result).collect())
                .unwrap_or_default();
            listed.sort_by_key(|result| order.iter().position(|id| *id == result.info.id));
        }
        let total = listed.len();
        let limit = params.limit.unwrap_or(ROWS_ANSWERED);
        listed.truncate(limit);
        let budget = engine.results().budget();
        Ok(ListAnswer {
            results: listed,
            bytes: budget.used(),
            budget: budget.limit(),
            total,
            truncated: total > limit,
        })
    }
}

/// Which rows of a result.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PageParams {
    /// The result's id.
    pub id: u64,
    /// The first row, from 0.
    pub offset: usize,
    /// The most rows; 50 when absent, at most 1000.
    pub limit: Option<usize>,
}

pub struct ResultPage;

impl Command for ResultPage {
    type Params = PageParams;
    type Output = Page;
    const ID: &'static str = "db.result.page";
    const TITLE: &'static str = "Result Rows";
    const DESCRIPTION: &'static str = "Rows `offset`…`offset + limit` (50, at most 1000) of a result kept in memory, each a list of values in the one value model of every database (null, bool, integer, decimal, float, text, bytes, temporal with its type, interval, uuid, json, array, map, document, other); long values cut to 1000 characters (`cellsCut`). `truncated` says whether rows follow, kept or still on the server (`more`: db.result.fetchNext reads on).";

    fn run(&self, session: &mut Session, params: PageParams) -> Result<Page, CommandError> {
        let result = result_of(session, params.id)?;
        Ok(page_of(&result, params.offset, params.limit.unwrap_or(ROWS_ANSWERED).min(1000)))
    }
}

/// How many more rows.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FetchParams {
    /// The result's id.
    pub id: u64,
    /// How many more rows; the page size (200) when absent.
    pub rows: Option<usize>,
}

/// A result reading on.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Fetching {
    /// The result now.
    #[serde(flatten)]
    pub info: ResultInfo,
    /// The operation that reads its statement again, when its portal was closed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
}

pub struct FetchNext;

impl Command for FetchNext {
    type Params = FetchParams;
    type Output = Fetching;
    const ID: &'static str = "db.result.fetchNext";
    const TITLE: &'static str = "Fetch Next Page";
    const DESCRIPTION: &'static str = "Reads `rows` more rows (the page size, 200) of a result in the background: from its open portal, or, when the portal was closed (the row cap outside your transaction), by reading its statement again past the rows kept; a statement that may change data is never read again (code `notRepeatable`: run it again in a console). Scrolling to the end of the grid does the same.";

    fn run(&self, session: &mut Session, params: FetchParams) -> Result<Fetching, CommandError> {
        let rows = params.rows.unwrap_or_else(|| settings(session).paging.page);
        let engine = engine(session);
        let operation = engine.fetch_more(params.id, Some(rows)).map_err(|error| failed(&error))?;
        Ok(Fetching { info: result_of(session, params.id)?.info(), operation })
    }
}

/// Fetch All, confirmed.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FetchAllParams {
    /// The result's id.
    pub id: u64,
    /// Confirms reading every row; without it the call is refused with code `confirmFetchAll`.
    pub confirmed: bool,
}

pub struct FetchAll;

impl Command for FetchAll {
    type Params = FetchAllParams;
    type Output = Fetching;
    const ID: &'static str = "db.result.fetchAll";
    const TITLE: &'static str = "Fetch All Rows";
    const DESCRIPTION: &'static str = "Reads every remaining row of a result in the background, within the window's memory budget for results (past it the reading stops and says so). Asks first: without `confirmed: true` it is refused with code `confirmFetchAll` and the rows kept so far.";

    fn run(&self, session: &mut Session, params: FetchAllParams) -> Result<Fetching, CommandError> {
        let result = result_of(session, params.id)?;
        if !params.confirmed {
            let info = result.info();
            return Err(CommandError::with_code(
                "confirmFetchAll",
                t!("database-error-confirm-fetch-all", rows = info.rows),
            )
            .with_data(serde_json::json!({ "rows": info.rows, "more": info.more })));
        }
        let engine = engine(session);
        let operation = engine.fetch_more(params.id, None).map_err(|error| failed(&error))?;
        Ok(Fetching { info: result.info(), operation })
    }
}

/// A result.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResultParams {
    /// The result's id.
    pub id: u64,
}

pub struct CountRows;

impl Command for CountRows {
    type Params = ResultParams;
    type Output = Fetching;
    const ID: &'static str = "db.result.count";
    const TITLE: &'static str = "Calculate Total Row Count";
    const DESCRIPTION: &'static str = "Counts every row of a result's query on the server (`SELECT count(*)` around it) in the background, in the result's session (its open portal is closed first); the count shows as the result's `total`. For queries that only read.";

    fn run(&self, session: &mut Session, params: ResultParams) -> Result<Fetching, CommandError> {
        let engine = engine(session);
        let operation = engine.count_rows(params.id).map_err(|error| failed(&error))?;
        Ok(Fetching { info: result_of(session, params.id)?.info(), operation: Some(operation) })
    }
}

/// Pin or unpin.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PinParams {
    /// The result's id.
    pub id: u64,
    /// Pin (`true`, the default) or unpin.
    pub pinned: Option<bool>,
}

/// A result tab.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabAnswer {
    /// The tab.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<ResultTab>,
}

fn no_tab(id: u64) -> CommandError {
    CommandError::with_code("unknownResult", t!("database-error-unknown-result", id = id))
}

pub struct PinResult;

impl Command for PinResult {
    type Params = PinParams;
    type Output = TabAnswer;
    const ID: &'static str = "db.result.pin";
    const TITLE: &'static str = "Pin Result Tab";
    const DESCRIPTION: &'static str = "Pins a console's result tab (`pinned: false` unpins it): a new run leaves pinned tabs and replaces the others.";

    fn run(&self, session: &mut Session, params: PinParams) -> Result<TabAnswer, CommandError> {
        let pinned = params.pinned.unwrap_or(true);
        let tab = with_tab(session, params.id, |tabs, _| {
            let tab = tabs.iter_mut().find(|tab| tab.result == params.id)?;
            tab.pinned = pinned;
            Some(tab.clone())
        })
        .flatten()
        .ok_or_else(|| no_tab(params.id))?;
        Ok(TabAnswer { tab: Some(tab) })
    }
}

/// A new title.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RenameParams {
    /// The result's id.
    pub id: u64,
    /// The new title.
    pub title: String,
}

pub struct RenameResult;

impl Command for RenameResult {
    type Params = RenameParams;
    type Output = TabAnswer;
    const ID: &'static str = "db.result.rename";
    const TITLE: &'static str = "Rename Result Tab";
    const DESCRIPTION: &'static str = "Renames a console's result tab.";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<TabAnswer, CommandError> {
        let title = params.title.trim().to_owned();
        if title.is_empty() {
            return Err(CommandError::new(t!("database-error-no-name")));
        }
        let tab = with_tab(session, params.id, |tabs, _| {
            let tab = tabs.iter_mut().find(|tab| tab.result == params.id)?;
            tab.title = title;
            Some(tab.clone())
        })
        .flatten()
        .ok_or_else(|| no_tab(params.id))?;
        Ok(TabAnswer { tab: Some(tab) })
    }
}

pub struct CloseResult;

impl Command for CloseResult {
    type Params = ResultParams;
    type Output = TabAnswer;
    const ID: &'static str = "db.result.close";
    const TITLE: &'static str = "Close Result Tab";
    const DESCRIPTION: &'static str = "Closes a result: its tab goes, its open portal closes, and its rows' memory goes back to the budget. A result with staged data changes is refused with code `pendingChanges` (save or cancel them first).";

    fn run(&self, session: &mut Session, params: ResultParams) -> Result<TabAnswer, CommandError> {
        super::edits::refuse_staged(session, &[params.id], (Self::ID, serde_json::json!({ "id": params.id })))?;
        let closed = session
            .state::<Database>()
            .and_then(Database::started)
            .is_some_and(|engine| engine.results().remove(params.id));
        let tabbed = with_tab(session, params.id, |tabs, active| {
            let at = tabs.iter().position(|tab| tab.result == params.id);
            if let Some(at) = at {
                tabs.remove(at);
                if *active == Some(PanelTab::Result(params.id)) {
                    *active = tabs.get(at.min(tabs.len().saturating_sub(1))).map(|tab| PanelTab::Result(tab.result));
                }
            }
        });
        if !closed && tabbed.is_none() {
            return Err(no_tab(params.id));
        }
        Ok(TabAnswer { tab: None })
    }
}

/// How results are shown.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewParams {
    /// `grid`, `text`, `record` or `json` (documents of a document database).
    pub view: ResultView,
}

/// The view results show in.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ViewAnswer {
    /// The view.
    pub view: ResultView,
}

pub struct SetView;

impl Command for SetView {
    type Params = ViewParams;
    type Output = ViewAnswer;
    const ID: &'static str = "db.result.view";
    const TITLE: &'static str = "Result View";
    const DESCRIPTION: &'static str = "Shows results as a grid, as aligned text, one row at a time as a form (`record`, with a filter of its columns) or, for a document database such as MongoDB, as documents in the shell's JSON (`json`; other results then show as a grid); the choice is remembered for every result.";

    fn run(&self, session: &mut Session, params: ViewParams) -> Result<ViewAnswer, CommandError> {
        let registry_params = consoles::SettingsParams { view: Some(params.view), ..Default::default() };
        consoles::UpdateSettings.run(session, registry_params)?;
        Ok(ViewAnswer { view: params.view })
    }
}

/// A result read again: in its console, or in its object tab.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum Refreshed {
    /// A console's run.
    Console(consoles::RunStarted),
    /// An object tab's read.
    Object(super::objects::DataRead),
}

pub struct RefreshResult;

/// The result to read again, with the confirmation of a `confirmExecute` refusal.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RefreshParams {
    /// The result's id.
    pub id: u64,
    /// The confirmation of a `confirmExecute` refusal: its `token`.
    #[serde(flatten)]
    pub confirmation: Confirmation,
}

impl Command for RefreshResult {
    type Params = RefreshParams;
    type Output = Refreshed;
    const ID: &'static str = "db.result.refresh";
    const TITLE: &'static str = "Refresh Result";
    const DESCRIPTION: &'static str = "Runs the statement of a result again: a console's in the console's session, where the new result replaces the console's unpinned tabs (a pinned result gets a new tab beside it); an object tab's Data in the Main context, as db.object.readData. Answers the query id at once. A console's statement asks again as running it does (code `confirmExecute`, then `confirmed: token`).";

    fn run(&self, session: &mut Session, params: RefreshParams) -> Result<Refreshed, CommandError> {
        if let Some(read) = super::objects::reread(session, params.id) {
            return read.map(Refreshed::Object);
        }
        consoles::rerun(session, params.id, &params.confirmation).map(Refreshed::Console)
    }
}

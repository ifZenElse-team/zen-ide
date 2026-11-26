use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{ExplainRun, OperationStatus, QueryStatement};
use zen_db::explain::{Detail, ExplainOption, ExplainOptions, Measure, Plan, PlanNode};
use zen_db::history::Source;
use zen_i18n::t;

use super::WORKSPACE;
use super::connections::describe;
use super::consoles::{
    connection_of, console_path, context_of, engine, ensure_connected, is_console, schema_of, statement_text,
};
use super::safety::{Confirmation, Run};
use super::sources::{Database, find};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};

pub const KIND: &str = "plan";

const NODES_ANSWERED: usize = 100;

const MOST_NODES: usize = 1000;

const RAW_ANSWERED: usize = 20_000;

const STATEMENT_ANSWERED: usize = 2000;

const STATEMENT_KEPT: usize = 4000;

const DETAILS_ANSWERED: usize = 100;

/// Where a Plan tab's explain is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PlanStatus {
    /// Not explained in this window yet (a tab brought back after a restart).
    #[default]
    NotRun,
    /// The server is explaining it.
    Running,
    /// The plan is here.
    Done,
    /// It failed; `error` says why.
    Failed,
    /// It was cancelled.
    Cancelled,
}

/// What a Plan tab shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PlanView {
    /// The plan as a tree of nodes.
    #[default]
    Tree,
    /// The plan as the server sent it.
    Raw,
}

#[derive(Clone, Debug, Default)]
pub struct PlanTab {
    pub connection: String,
    pub console: Option<PathBuf>,
    pub statement: String,
    pub options: ExplainOptions,
    pub supported: Vec<ExplainOption>,
    pub ignored: Vec<ExplainOption>,
    pub executes: bool,
    pub rolled_back: bool,
    pub inlined: Vec<String>,
    pub operation: Option<u64>,
    pub status: PlanStatus,
    pub plan: Option<Arc<Plan>>,
    pub error: Option<DbError>,
    pub selected: Option<usize>,
    pub view: PlanView,
}

#[derive(Default)]
pub struct Plans {
    tabs: HashMap<String, PlanTab>,
}

impl Plans {
    pub fn get(&self, tab: &str) -> Option<&PlanTab> {
        self.tabs.get(tab)
    }

    pub fn set(&mut self, tab: &str, state: PlanTab) {
        self.tabs.insert(tab.to_owned(), state);
    }
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        ErrorKind::Unsupported => "unsupported",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn cut(text: &str, most: usize) -> (String, bool) {
    match text.char_indices().nth(most) {
        Some((at, _)) => (text[..at].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

fn plan_tab(session: &Session, id: &str) -> Option<InnerTab> {
    session.state::<Workspaces>()?.tabs(WORKSPACE).iter().find(|tab| tab.id == id && tab.kind == KIND).cloned()
}

fn restored(inner: &InnerTab) -> PlanTab {
    PlanTab {
        connection: inner.data["connection"].as_str().unwrap_or_default().to_owned(),
        console: inner.data["path"].as_str().map(PathBuf::from),
        statement: inner.data["statement"].as_str().unwrap_or_default().to_owned(),
        options: serde_json::from_value(inner.data["options"].clone()).unwrap_or_default(),
        ..PlanTab::default()
    }
}

fn active_plan(session: &Session) -> Option<String> {
    let tab = session.state::<Workspaces>()?.active_tab(WORKSPACE)?;
    (tab.kind == KIND).then(|| tab.id.clone())
}

fn tab_id(session: &Session, tab: Option<String>) -> Result<String, CommandError> {
    tab.or_else(|| active_plan(session))
        .filter(|id| plan_tab(session, id).is_some())
        .ok_or_else(|| CommandError::with_code("noPlan", t!("database-error-no-plan")))
}

pub fn step(session: &mut Session) {
    if session.state::<Plans>().is_none_or(|plans| plans.tabs.is_empty()) {
        return;
    }
    let open: Vec<String> = session
        .state::<Workspaces>()
        .map(|workspaces| workspaces.tabs(WORKSPACE).iter().map(|tab| tab.id.clone()).collect())
        .unwrap_or_default();
    let engine = session.state::<Database>().and_then(Database::started).cloned();
    let plans = session.state_mut::<Plans>();
    plans.tabs.retain(|id, _| open.contains(id));
    let Some(engine) = engine else { return };
    for state in plans.tabs.values_mut().filter(|state| state.status == PlanStatus::Running) {
        let Some(operation) = state.operation else { continue };
        let Some(ended) = engine.operation(operation) else {
            state.status = PlanStatus::Cancelled;
            state.error = Some(DbError::new(ErrorKind::Cancelled, ""));
            continue;
        };
        match ended.status {
            OperationStatus::Running => {}
            OperationStatus::Done => {
                state.plan = engine.take_plan(operation).map(Arc::new);
                state.status = if state.plan.is_some() { PlanStatus::Done } else { PlanStatus::Failed };
                state.selected = state.plan.as_ref().and_then(|plan| hottest(plan));
            }
            OperationStatus::Cancelled => {
                state.status = PlanStatus::Cancelled;
                state.error = ended.error;
            }
            OperationStatus::Failed => {
                state.status = PlanStatus::Failed;
                state.error = ended.error;
            }
        }
    }
}

fn hottest(plan: &Plan) -> Option<usize> {
    if plan.measure == Measure::None {
        return plan.nodes.first().map(|node| node.id);
    }
    plan.nodes.iter().max_by(|a, b| a.share.total_cmp(&b.share)).map(|node| node.id)
}

/// How to explain.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExplainParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// A Plan tab whose statement to explain again; the Plan tab in front when neither `path` nor `statement` is given.
    pub tab: Option<String>,
    /// The statement to explain instead of the selection or the statement under the caret.
    pub statement: Option<String>,
    /// Run the statement and report real times and rows (off by default): it executes, inside a transaction that is rolled back; asks like running it where the connection type confirms changes.
    pub analyze: Option<bool>,
    /// More detail: output columns, qualified names (PostgreSQL).
    pub verbose: Option<bool>,
    /// The planner's costs (PostgreSQL; on by default).
    pub costs: Option<bool>,
    /// Buffer usage (PostgreSQL).
    pub buffers: Option<bool>,
    /// The settings that changed the plan (PostgreSQL).
    pub settings: Option<bool>,
    /// The confirmation of a `confirmExecute` refusal of an ANALYZE: its `token`.
    #[serde(flatten)]
    pub confirmation: Confirmation,
    /// Values of bind parameters and variables, as db.console.execute takes them.
    #[serde(flatten)]
    pub given: super::variables::Given,
}

/// An explain started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExplainStarted {
    /// The Plan tab it opens in, for `db.plan.state`.
    pub tab: String,
    /// Its operation, for `db.operation.state` and `db.connection.cancel`.
    pub operation: u64,
    /// The connection it runs on.
    pub connection: String,
    /// The statement sent, cut to 2000 characters.
    pub statement: String,
    /// Whether the statement runs (ANALYZE).
    pub executes: bool,
    /// Whether it runs inside a transaction that is rolled back.
    pub rolled_back: bool,
    /// The options asked for that the driver does not have.
    pub ignored: Vec<ExplainOption>,
    /// The values written into the statement as quoted literals because the driver has no bind parameters, as `:name = literal`, secret-looking ones masked.
    pub inlined: Vec<String>,
    /// Whether the connection was connected first.
    pub connecting: bool,
}

struct Explained {
    tab: Option<String>,
    console: Option<PathBuf>,
    connection: Option<String>,
    statement: String,
    options: ExplainOptions,
}

fn source_of(session: &mut Session, params: &ExplainParams) -> Result<Explained, CommandError> {
    let tab = params
        .tab
        .clone()
        .or_else(|| (params.path.is_none() && params.statement.is_none()).then(|| active_plan(session)).flatten());
    if let Some(tab) = tab {
        let inner =
            plan_tab(session, &tab).ok_or_else(|| CommandError::with_code("noPlan", t!("database-error-no-plan")))?;
        let kept =
            session.state::<Plans>().and_then(|plans| plans.get(&tab)).cloned().unwrap_or_else(|| restored(&inner));
        let connection = Some(kept.connection).filter(|connection| !connection.is_empty());
        let statement = params.statement.clone().unwrap_or(kept.statement);
        return Ok(Explained { tab: Some(tab), console: kept.console, connection, statement, options: kept.options });
    }
    let console = console_path(session, params.path.clone())?;
    let statement = match &params.statement {
        Some(statement) => statement.clone(),
        None => statement_text(session, &console)?.unwrap_or_default(),
    };
    Ok(Explained { tab: None, connection: None, console: Some(console), statement, options: ExplainOptions::default() })
}

fn connection_for(session: &mut Session, source: &Explained) -> Result<String, CommandError> {
    if let Some(connection) = &source.connection {
        return Ok(connection.clone());
    }
    let console = source.console.as_deref();
    match console.and_then(|console| connection_of(session, console)) {
        Some(connection) => Ok(connection),
        None if console.is_some_and(|console| !is_console(session, console)) => {
            Err(super::scripts::not_bound(session, console.unwrap_or(Path::new(""))))
        }
        None => Err(CommandError::with_code("noConnection", super::no_console_connection(session))),
    }
}

fn title_of(console: Option<&Path>, connection: &str) -> String {
    let name = console
        .and_then(Path::file_stem)
        .map_or_else(|| connection.to_owned(), |stem| stem.to_string_lossy().into_owned());
    t!("database-plan-title", name = name)
}

fn explain(session: &mut Session, params: ExplainParams) -> Result<ExplainStarted, CommandError> {
    let source = source_of(session, &params)?;
    if source.statement.trim().is_empty() {
        return Err(CommandError::with_code("noStatement", t!("database-error-no-statement")));
    }
    let connection = connection_for(session, &source)?;
    let mut options = source.options.clone();
    for (value, field) in [
        (params.analyze, &mut options.analyze),
        (params.verbose, &mut options.verbose),
        (params.costs, &mut options.costs),
        (params.buffers, &mut options.buffers),
        (params.settings, &mut options.settings),
    ] {
        if let Some(value) = value {
            *field = value;
        }
    }
    let entry = find(session, &connection)?;
    let engine = engine(session);
    let driver =
        engine.driver(&entry.source.driver).filter(|driver| driver.capabilities().explain).ok_or_else(|| {
            CommandError::with_code(
                "unsupported",
                t!("database-error-explain-unsupported", name = entry.source.name.as_str()),
            )
        })?;
    let mut retry = serde_json::json!({
        "statement": source.statement,
        "analyze": options.analyze,
        "verbose": options.verbose,
        "costs": options.costs,
        "buffers": options.buffers,
        "settings": options.settings,
    });
    match (&source.tab, &source.console) {
        (Some(tab), _) => retry["tab"] = serde_json::json!(tab),
        (None, Some(console)) => retry["path"] = serde_json::json!(console),
        (None, None) => {}
    }
    let asking = (Explain::ID, retry.clone());
    params.given.add_to(&mut retry);
    let unbound = QueryStatement {
        text: source.statement.clone(),
        start: 0,
        end: source.statement.chars().count(),
        client: false,
        params: Vec::new(),
        kept: None,
    };
    let console = source.console.as_deref();
    let prepared = super::variables::prepare(session, console, &connection, vec![unbound], (&params.given, asking))?;
    let inlined = prepared.inlined;
    let bound = prepared
        .statements
        .into_iter()
        .next()
        .ok_or_else(|| CommandError::with_code("noStatement", t!("database-error-no-statement")))?;
    let dialect = zen_db::sql::Dialect::of_language(&driver.capabilities().language);
    if zen_db::sql::split(&bound.text, dialect).len() > 1 {
        return Err(CommandError::with_code("oneStatement", t!("database-error-explain-one-statement")));
    }
    let request = driver.explain(&bound.text, &options).map_err(|error| failed(&error))?;
    let body = zen_db::explain::body(&bound.text);
    if request.executes {
        let effect = driver.assess(body).effect;
        if !driver.capabilities().transactions && effect != zen_db::safety::Effect::Read {
            return Err(CommandError::with_code("analyzeChanges", t!("database-error-explain-analyze-changes")));
        }
        if dialect == zen_db::sql::Dialect::MySql && effect != zen_db::safety::Effect::Read {
            return Err(CommandError::with_code("analyzeChanges", t!("database-error-explain-analyze-changes")));
        }
    }
    let judged = format!("{} {body}", if request.executes { "EXPLAIN ANALYZE" } else { "EXPLAIN" });
    let context = source.console.as_deref().map_or(ContextKind::Main, context_of);
    let place = source.console.as_deref().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned());
    let mut provenance = super::history::provenance(session, Source::Statement, place);
    let run = Run {
        entry: &entry,
        context: &context,
        provenance: &provenance,
        confirmation: &params.confirmation,
        retry: (Explain::ID, retry),
    };
    let guarded_text = if request.executes { body.to_owned() } else { judged.clone() };
    let checked = QueryStatement {
        text: guarded_text.clone(),
        start: 0,
        end: guarded_text.chars().count(),
        client: false,
        params: bound.params.clone(),
        kept: None,
    };
    let guarded = super::safety::guard(session, &run, vec![checked])?;
    provenance.confirmed = guarded.confirmed;
    let connecting = ensure_connected(session, &connection)?;
    let rolled_back = request.executes && driver.capabilities().transactions;
    let (executes, ignored) = (request.executes, request.ignored.clone());
    let (sent, _) = cut(&request.text, STATEMENT_ANSWERED);
    let run = ExplainRun {
        request,
        judged,
        rollback: rolled_back,
        schema: source.console.as_deref().and_then(|console| schema_of(session, console)),
        read_only: guarded.read_only,
        timeout: None,
        provenance,
        params: bound.params,
    };
    let operation = engine.explain(&connection, context.clone(), run).map_err(|error| failed(&error))?;
    let id = source.tab.clone().unwrap_or_else(|| match &source.console {
        Some(_) => format!("{KIND}:{context}"),
        None => format!("{KIND}:{connection}"),
    });
    let (kept_statement, _) = cut(&source.statement, STATEMENT_KEPT);
    let open = OpenTabParams {
        workspace: WORKSPACE.to_owned(),
        group: None,
        id: Some(id.clone()),
        kind: Some(KIND.to_owned()),
        title: Some(title_of(source.console.as_deref(), &entry.source.name)),
        closable: Some(true),
        data: Some(serde_json::json!({
            "connection": connection,
            "path": source.console,
            "statement": kept_statement,
            "options": options,
        })),
        path: None,
    };
    OpenTab.run(session, open)?;
    let previous =
        session.state::<Plans>().and_then(|plans| plans.get(&id)).map(|state| state.view).unwrap_or_default();
    let state = PlanTab {
        connection: connection.clone(),
        console: source.console.clone(),
        statement: source.statement.clone(),
        options,
        supported: driver.explain_options(),
        ignored: ignored.clone(),
        executes,
        rolled_back,
        inlined: inlined.clone(),
        operation: Some(operation),
        status: PlanStatus::Running,
        plan: None,
        error: None,
        selected: None,
        view: previous,
    };
    session.state_mut::<Plans>().set(&id, state);
    Ok(ExplainStarted {
        tab: id,
        operation,
        connection,
        statement: sent,
        executes,
        rolled_back,
        ignored,
        inlined,
        connecting,
    })
}

pub struct Explain;

impl Command for Explain {
    type Params = ExplainParams;
    type Output = ExplainStarted;
    const ID: &'static str = "db.console.explain";
    const TITLE: &'static str = "Explain Plan";
    const DESCRIPTION: &'static str = "Explain Plan (⌃⇧E): explains the selection or the statement under the caret of a console or `.sql` file (the active one; or `statement`; or the statement of a Plan tab `tab`, the one in front when nothing else is given) on its connection (connected first when needed), in the console's own session, in the background, and opens the plan in a Plan tab of the Database workspace. PostgreSQL takes `analyze`, `verbose`, `costs` (on by default), `buffers` and `settings` (EXPLAIN (FORMAT JSON, …)); MySQL and MariaDB EXPLAIN FORMAT=JSON and `analyze`; SQLite EXPLAIN QUERY PLAN; other drivers refuse with code `unsupported`; options a driver lacks are listed in `ignored`. Placeholders (`:name`; `$1` on PostgreSQL; `?` elsewhere) and `${name}` variables take the same step as db.console.execute: without `parameters` a statement with placeholders is refused with code `parametersNeeded`, its placeholders (each with its `inferred` type) and `retry`; values are bound on the server (PostgreSQL's extended protocol, MySQL's and MariaDB's prepared statements), and where a driver cannot bind (SQLite) written as quoted literals, listed in `inlined`. `analyze` RUNS the statement, inside a transaction (or a savepoint in an open one) that is rolled back; it is checked as running it would be: refused on read-only connections and for agents that may only read, and refused with code `confirmExecute` and a `token` where the connection type confirms changes or the statement is risky (run again with `confirmed: token`). Answers the tab and operation at once: read the plan with db.plan.state; cancel with db.connection.cancel { operation }.";

    fn run(&self, session: &mut Session, params: ExplainParams) -> Result<ExplainStarted, CommandError> {
        explain(session, params)
    }
}

/// Which plan to read and how much of it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanStateParams {
    /// The Plan tab; the one in front when absent.
    pub tab: Option<String>,
    /// How many nodes to skip, in tree order.
    pub offset: usize,
    /// The most nodes to answer; 100 when absent, at most 1000.
    pub limit: Option<usize>,
    /// Answer every node's details, not only the chosen node's.
    pub details: bool,
    /// Answer the plan as the server sent it.
    pub raw: bool,
    /// The most characters of the raw plan; 20000 when absent.
    pub raw_limit: Option<usize>,
}

/// A Plan tab as agents and the view read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanAnswer {
    /// The tab's id.
    pub tab: String,
    /// The connection's id.
    pub connection: String,
    /// The console or script it was explained from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The statement explained, cut to 2000 characters.
    pub statement: String,
    /// Whether the statement was cut.
    pub statement_cut: bool,
    /// Where the explain is.
    pub status: PlanStatus,
    /// The explain's operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
    /// Why it failed, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The error's code, when it has one (`readOnly`, a SQLSTATE).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// The options it was explained with.
    pub options: ExplainOptions,
    /// The options the driver has.
    pub supported: Vec<ExplainOption>,
    /// The options asked for that the driver does not have.
    pub ignored: Vec<ExplainOption>,
    /// Whether the statement ran (ANALYZE).
    pub executes: bool,
    /// Whether it ran inside a transaction that was rolled back.
    pub rolled_back: bool,
    /// The values written into the statement as quoted literals because the driver has no bind parameters.
    pub inlined: Vec<String>,
    /// What the nodes' shares are of: `time`, `cost` or `none`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measure: Option<Measure>,
    /// The estimated cost of the whole plan.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_cost: Option<f64>,
    /// Milliseconds the whole plan took (ANALYZE).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_time_ms: Option<f64>,
    /// Milliseconds of planning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planning_ms: Option<f64>,
    /// Milliseconds of execution.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_ms: Option<f64>,
    /// The nodes from `offset`, at most `limit`, in tree order; details only with `details`.
    pub nodes: Vec<PlanNode>,
    /// How many nodes the plan has.
    pub total: usize,
    /// Whether nodes were left out after these.
    pub truncated: bool,
    /// The ids of the hot nodes, the hottest first, at most 20.
    pub hot: Vec<usize>,
    /// The node chosen for the details pane, with its details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected: Option<PlanNode>,
    /// What the server said about the plan as a whole, at most 100.
    pub plan_details: Vec<Detail>,
    /// The plan as the server sent it, with `raw`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    /// Whether the raw plan was cut.
    pub raw_truncated: bool,
    /// What the tab shows.
    pub view: PlanView,
}

fn answer(session: &Session, id: &str, params: &PlanStateParams) -> Result<PlanAnswer, CommandError> {
    let inner = plan_tab(session, id).ok_or_else(|| CommandError::with_code("noPlan", t!("database-error-no-plan")))?;
    let state = session.state::<Plans>().and_then(|plans| plans.get(id)).cloned().unwrap_or_else(|| restored(&inner));
    let (statement, statement_cut) = cut(&state.statement, STATEMENT_ANSWERED);
    let plan = state.plan.as_deref();
    let total = plan.map_or(0, |plan| plan.nodes.len());
    let limit = params.limit.unwrap_or(NODES_ANSWERED).min(MOST_NODES);
    let nodes: Vec<PlanNode> = plan
        .map(|plan| {
            plan.nodes
                .iter()
                .skip(params.offset)
                .take(limit)
                .map(|node| {
                    let mut node = node.clone();
                    if !params.details {
                        node.details.clear();
                    }
                    node
                })
                .collect()
        })
        .unwrap_or_default();
    let mut hot: Vec<&PlanNode> =
        plan.map(|plan| plan.nodes.iter().filter(|node| node.hot).collect()).unwrap_or_default();
    hot.sort_by(|a, b| b.share.total_cmp(&a.share));
    let (raw, raw_truncated) = match plan.filter(|_| params.raw) {
        Some(plan) => {
            let (text, cut_off) = cut(&plan.raw, params.raw_limit.unwrap_or(RAW_ANSWERED));
            (Some(text), cut_off)
        }
        None => (None, false),
    };
    Ok(PlanAnswer {
        tab: id.to_owned(),
        connection: state.connection.clone(),
        path: state.console.clone(),
        statement,
        statement_cut,
        status: state.status,
        operation: state.operation,
        error: state.error.as_ref().map(describe),
        error_code: state.error.as_ref().and_then(|error| error.code.clone()),
        options: state.options.clone(),
        supported: state.supported.clone(),
        ignored: state.ignored.clone(),
        executes: state.executes,
        rolled_back: state.rolled_back,
        inlined: state.inlined.clone(),
        measure: plan.map(|plan| plan.measure),
        total_cost: plan.and_then(|plan| plan.total_cost),
        total_time_ms: plan.and_then(|plan| plan.total_time_ms),
        planning_ms: plan.and_then(|plan| plan.planning_ms),
        execution_ms: plan.and_then(|plan| plan.execution_ms),
        truncated: params.offset.saturating_add(nodes.len()) < total,
        nodes,
        total,
        hot: hot.iter().take(20).map(|node| node.id).collect(),
        selected: state.selected.and_then(|selected| plan.and_then(|plan| plan.nodes.get(selected)).cloned()),
        plan_details: plan
            .map(|plan| plan.details.iter().take(DETAILS_ANSWERED).cloned().collect())
            .unwrap_or_default(),
        raw,
        raw_truncated,
        view: state.view,
    })
}

pub struct PlanStateCommand;

impl Command for PlanStateCommand {
    type Params = PlanStateParams;
    type Output = PlanAnswer;
    const ID: &'static str = "db.plan.state";
    const TITLE: &'static str = "Plan State";
    const DESCRIPTION: &'static str = "A Plan tab (`tab`, the one in front): its statement, status (`running`, `done`, `failed`, `cancelled`, `notRun` after a restart), the options used, supported and ignored, whether it ran (ANALYZE) and was rolled back, the totals (cost, time, planning and execution), and its nodes in tree order from `offset` (at most `limit`, 100 by default, with `total` and `truncated`): operation, entity, condition, startup cost, cost, self cost, estimated and actual rows, loops, time, self time, `share` of the plan's time (analyzed) or cost and `hot` (a share of 20% or more); `hot` lists the hottest nodes; `selected` is the node in the details pane with every property. `details` answers every node's properties, `raw` the plan as the server sent it (cut to `rawLimit`, 20000 characters). Connects nowhere.";

    fn run(&self, session: &mut Session, params: PlanStateParams) -> Result<PlanAnswer, CommandError> {
        step(session);
        let id = tab_id(session, params.tab.clone())?;
        answer(session, &id, &params)
    }
}

/// The node to choose.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SelectNodeParams {
    /// The Plan tab; the one in front when absent.
    pub tab: Option<String>,
    /// The node's id, from db.plan.state.
    pub node: usize,
}

pub struct SelectNode;

impl Command for SelectNode {
    type Params = SelectNodeParams;
    type Output = PlanNode;
    const ID: &'static str = "db.plan.select";
    const TITLE: &'static str = "Select Plan Node";
    const DESCRIPTION: &'static str = "Chooses a node of a Plan tab (`tab`, the one in front) for its details pane; answers the node with every property the server reported.";

    fn run(&self, session: &mut Session, params: SelectNodeParams) -> Result<PlanNode, CommandError> {
        step(session);
        let id = tab_id(session, params.tab)?;
        let node = session
            .state::<Plans>()
            .and_then(|plans| plans.get(&id))
            .and_then(|state| state.plan.as_ref())
            .and_then(|plan| plan.nodes.get(params.node))
            .cloned()
            .ok_or_else(|| CommandError::with_code("noNode", t!("database-error-no-plan-node", node = params.node)))?;
        if let Some(state) = session.state_mut::<Plans>().tabs.get_mut(&id) {
            state.selected = Some(params.node);
        }
        Ok(node)
    }
}

/// What a Plan tab shows.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanViewParams {
    /// The Plan tab; the one in front when absent.
    pub tab: Option<String>,
    /// `tree` or `raw`.
    pub view: PlanView,
}

/// What a Plan tab shows, answered.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanViewAnswer {
    /// The tab.
    pub tab: String,
    /// What it shows.
    pub view: PlanView,
}

pub struct SetPlanView;

impl Command for SetPlanView {
    type Params = PlanViewParams;
    type Output = PlanViewAnswer;
    const ID: &'static str = "db.plan.view";
    const TITLE: &'static str = "Show Plan As";
    const DESCRIPTION: &'static str =
        "Shows a Plan tab (`tab`, the one in front) as a `tree` of nodes or as the `raw` text the server sent.";

    fn run(&self, session: &mut Session, params: PlanViewParams) -> Result<PlanViewAnswer, CommandError> {
        let id = tab_id(session, params.tab)?;
        let inner =
            plan_tab(session, &id).ok_or_else(|| CommandError::with_code("noPlan", t!("database-error-no-plan")))?;
        let plans = session.state_mut::<Plans>();
        match plans.tabs.get_mut(&id) {
            Some(state) => state.view = params.view,
            None => plans.set(&id, PlanTab { view: params.view, ..restored(&inner) }),
        }
        Ok(PlanViewAnswer { tab: id, view: params.view })
    }
}

#[cfg(test)]
mod tests;

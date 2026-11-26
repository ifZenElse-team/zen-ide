use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Session};
use zen_db::engine::{Message, OperationStatus, QueryState, StatementStatus};
use zen_db::history::Source;
use zen_i18n::t;

use super::WORKSPACE;
use super::connections::describe;
use super::consoles::{self, Checked, Consoles, Scope, Start};
use super::safety::{self, CONFIRM_EXECUTE, Confirmation};
use super::sources::{Database, find};
use super::variables::{self, Given, PARAMETERS_NEEDED, Prepared};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams};

pub const MOST_TARGETS: usize = 20;

const MOST_LISTED: usize = 10;

const MOST_WAIT_MS: u64 = 60_000;

/// Where a connection of a run on several connections is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TargetStatus {
    /// Waiting for its turn.
    Waiting,
    /// Running now.
    Running,
    /// Every statement ran.
    Done,
    /// A statement failed, or the connection could not run the script.
    Failed,
    /// Stopped by Cancel.
    Cancelled,
    /// Not run: an earlier connection failed and the run stops at the first error.
    Skipped,
}

struct Slot {
    target: safety::Target,
    checked: Option<Checked>,
    notes: Vec<Message>,
    inlined: Vec<String>,
    statements: usize,
    confirmed: bool,
    read_only: bool,
    status: TargetStatus,
    query: Option<u64>,
    error: Option<String>,
}

struct Run {
    id: u64,
    stop_on_error: bool,
    replace: bool,
    cancelled: bool,
    slots: Vec<Slot>,
}

impl Run {
    fn finish(&mut self, status: TargetStatus) {
        for slot in self.slots.iter_mut().filter(|slot| slot.status == TargetStatus::Waiting) {
            slot.status = status;
            slot.checked = None;
        }
    }

    fn running(&self) -> bool {
        self.slots.iter().any(|slot| matches!(slot.status, TargetStatus::Waiting | TargetStatus::Running))
    }
}

#[derive(Default)]
pub struct SeveralRuns {
    runs: HashMap<PathBuf, Run>,
    next: u64,
}

/// A connection of a run on several connections, with its outcome so far.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetAnswer {
    /// The connection, its type and its safety.
    #[serde(flatten)]
    pub target: safety::Target,
    /// Where it is.
    pub status: TargetStatus,
    /// The query that runs the script there, once started: read it with db.query.state.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// The statements it runs.
    pub statements: usize,
    /// Those that ran.
    pub executed: usize,
    /// Rows read into its results.
    pub rows: u64,
    /// Rows changed.
    pub updated: u64,
    /// Milliseconds it ran.
    pub elapsed_ms: u64,
    /// Why it failed, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Its result tabs in the console, in order: read them with db.result.page.
    pub results: Vec<u64>,
    /// Whether a confirmation let its statements through.
    pub confirmed: bool,
    /// Whether it runs in a read-only session (an agent where agents only read).
    pub read_only: bool,
    /// The values written into its statements as quoted literals because its driver has no bind parameters, as `:name = literal`, secret-looking ones masked.
    pub inlined: Vec<String>,
}

/// A run of a console's script on several connections.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeveralRun {
    /// The run's id.
    pub run: u64,
    /// The console or `.sql` file.
    pub path: PathBuf,
    /// Whether a connection still runs or waits.
    pub running: bool,
    /// Whether the run stops at the first connection that fails.
    pub stop_on_error: bool,
    /// The connections in the order they run.
    pub targets: Vec<TargetAnswer>,
}

/// What a run on several connections started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SeveralStarted {
    /// The run.
    #[serde(flatten)]
    pub run: SeveralRun,
    /// The console's tab in the Database workspace, where the results show.
    pub tab: InnerTab,
}

pub fn connection_name(session: &Session, path: &Path, query: u64) -> Option<String> {
    let run = session.state::<SeveralRuns>()?.runs.get(path)?;
    run.slots.iter().find(|slot| slot.query == Some(query)).map(|slot| slot.target.name.clone())
}

pub fn cancel(session: &mut Session, path: &Path) -> bool {
    let Some(run) = session.state_mut::<SeveralRuns>().runs.get_mut(path) else { return false };
    if !run.running() {
        return false;
    }
    run.cancelled = true;
    run.finish(TargetStatus::Cancelled);
    true
}

fn summed(query: &QueryState) -> (usize, u64, u64, Option<String>) {
    let executed = query
        .statements
        .iter()
        .filter(|statement| matches!(statement.status, StatementStatus::Done | StatementStatus::Failed))
        .count();
    let rows = query.statements.iter().filter_map(|statement| statement.rows).sum();
    let failed = query.statements.iter().find(|statement| statement.status == StatementStatus::Failed);
    let error = failed.and_then(|statement| statement.error.as_ref()).or(query.error.as_ref()).map(describe);
    (executed, rows, query.updated, error)
}

pub fn answer(session: &Session, path: &Path) -> Option<SeveralRun> {
    let run = session.state::<SeveralRuns>()?.runs.get(path)?;
    let engine = session.state::<Database>().and_then(Database::started);
    let tabs = session.state::<Consoles>().map(|consoles| consoles.tabs(path)).unwrap_or_default();
    let targets = run
        .slots
        .iter()
        .map(|slot| {
            let state = slot.query.and_then(|query| engine.and_then(|engine| engine.query(query)));
            let (executed, rows, updated, error) = state.as_ref().map_or((0, 0, 0, None), summed);
            TargetAnswer {
                target: slot.target.clone(),
                status: slot.status,
                query: slot.query,
                statements: slot.statements,
                executed,
                rows,
                updated,
                elapsed_ms: state.as_ref().map_or(0, |state| state.elapsed_ms),
                error: slot.error.clone().or(error.filter(|_| slot.status == TargetStatus::Failed)),
                results: tabs.iter().filter(|tab| Some(tab.query) == slot.query).map(|tab| tab.result).collect(),
                confirmed: slot.confirmed,
                read_only: slot.read_only,
                inlined: slot.inlined.clone(),
            }
        })
        .collect();
    Some(SeveralRun {
        run: run.id,
        path: path.to_owned(),
        running: run.running(),
        stop_on_error: run.stop_on_error,
        targets,
    })
}

fn ended(session: &mut Session, path: &Path, query: u64) -> Option<TargetStatus> {
    let engine = session.state::<Database>().and_then(Database::started).cloned()?;
    let Some(state) = engine.query(query) else { return Some(TargetStatus::Failed) };
    if state.status == OperationStatus::Running {
        return None;
    }
    consoles::refresh(session, path);
    let failed = state.status == OperationStatus::Failed
        || state.statements.iter().any(|statement| statement.status == StatementStatus::Failed);
    Some(match state.status {
        OperationStatus::Cancelled => TargetStatus::Cancelled,
        _ if failed => TargetStatus::Failed,
        _ => TargetStatus::Done,
    })
}

fn console_busy(session: &Session, path: &Path) -> bool {
    let engine = session.state::<Database>().and_then(Database::started);
    let running = session.state::<Consoles>().and_then(|consoles| consoles.running(path));
    running
        .and_then(|query| engine.and_then(|engine| engine.query(query)))
        .is_some_and(|state| state.status == OperationStatus::Running)
}

fn advance(session: &mut Session, path: &Path) {
    loop {
        let Some(run) = session.state::<SeveralRuns>().and_then(|runs| runs.runs.get(path)) else { return };
        if let Some((index, query)) = run
            .slots
            .iter()
            .enumerate()
            .find(|(_, slot)| slot.status == TargetStatus::Running)
            .map(|(index, slot)| (index, slot.query))
        {
            let Some(status) = query.map_or(Some(TargetStatus::Failed), |query| ended(session, path, query)) else {
                return;
            };
            let Some(run) = session.state_mut::<SeveralRuns>().runs.get_mut(path) else { return };
            run.slots[index].status = status;
            match status {
                TargetStatus::Cancelled => {
                    run.cancelled = true;
                    run.finish(TargetStatus::Cancelled);
                    return;
                }
                TargetStatus::Failed if run.stop_on_error => {
                    run.finish(TargetStatus::Skipped);
                    return;
                }
                _ => {}
            }
            continue;
        }
        if run.cancelled {
            if let Some(run) = session.state_mut::<SeveralRuns>().runs.get_mut(path) {
                run.finish(TargetStatus::Cancelled);
            }
            return;
        }
        let Some(index) = run.slots.iter().position(|slot| slot.status == TargetStatus::Waiting) else { return };
        if console_busy(session, path) {
            return;
        }
        let first = run.slots.iter().all(|slot| slot.query.is_none());
        let (replace, stop_on_error) = (run.replace && first, run.stop_on_error);
        let Some((checked, notes)) = session.state_mut::<SeveralRuns>().runs.get_mut(path).and_then(|run| {
            let slot = &mut run.slots[index];
            slot.checked.take().map(|checked| (checked, std::mem::take(&mut slot.notes)))
        }) else {
            return;
        };
        let own = consoles::connection_of(session, path).as_deref() == Some(checked.connection.as_str());
        let schema = if own { consoles::schema_of(session, path) } else { None };
        let start = Start { new_tabs: !replace, stop_on_error, schema, own: false, notes };
        let started = consoles::begin(session, path, checked, start);
        if first {
            consoles::show_connections(session, path);
        }
        let Some(run) = session.state_mut::<SeveralRuns>().runs.get_mut(path) else { return };
        match started {
            Ok(started) => {
                run.slots[index].status = TargetStatus::Running;
                run.slots[index].query = Some(started.query);
                return;
            }
            Err(error) => {
                run.slots[index].status = TargetStatus::Failed;
                run.slots[index].error = Some(error.message);
                if run.stop_on_error {
                    run.finish(TargetStatus::Skipped);
                    return;
                }
            }
        }
    }
}

pub fn step(session: &mut Session) {
    let paths: Vec<PathBuf> = session
        .state::<SeveralRuns>()
        .map(|runs| runs.runs.iter().filter(|(_, run)| run.running()).map(|(path, _)| path.clone()).collect())
        .unwrap_or_default();
    for path in paths {
        advance(session, &path);
    }
}

/// How to run a console's script on several connections.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunOnParams {
    /// The console or `.sql` file: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
    /// The connections to run on, in this order: ids from db.dataSource.list, at most 20.
    pub connections: Vec<String>,
    /// What to run: `script` (the default: every statement, or those of the selection), `statement` or `fromCaret`.
    pub scope: Option<Scope>,
    /// A character offset to run at instead of the caret and selection.
    pub offset: Option<usize>,
    /// Stop at the first error: the failing connection's script stops and rolls back, and the connections after it are skipped; `true` when absent.
    pub stop_on_error: Option<bool>,
    /// Keep the console's result tabs there are.
    pub new_tab: bool,
    /// The `token` of this run's `confirmExecute` refusal: runs it on every connection it listed, unchanged since.
    pub confirmed: Option<String>,
    /// Values of bind parameters and variables, one set for every connection, as db.console.execute takes them.
    #[serde(flatten)]
    pub given: Given,
}

pub struct RunOnConnections;

struct Asked {
    index: usize,
    token: String,
    data: Value,
}

fn code_of(error: &CommandError) -> &str {
    error.code.as_deref().unwrap_or("databaseError")
}

fn combined(connections: &[String], tokens: &[Option<String>]) -> String {
    let mut hasher = DefaultHasher::new();
    for (connection, token) in connections.iter().zip(tokens) {
        connection.hash(&mut hasher);
        token.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn listed(data: &Value) -> Value {
    let statements: Vec<Value> =
        data["statements"].as_array().map(|all| all.iter().take(MOST_LISTED).cloned().collect()).unwrap_or_default();
    let asked = data["asked"].as_u64().or(data["total"].as_u64()).unwrap_or(statements.len() as u64);
    json!({ "statements": statements, "asked": asked, "truncated": asked > statements.len() as u64 })
}

fn merged(refusals: Vec<CommandError>) -> CommandError {
    let mut parameters: Vec<Value> = Vec::new();
    let mut variables: Vec<Value> = Vec::new();
    let mut inlined = false;
    let mut retry = Value::Null;
    for refusal in refusals {
        let data = Value::Object(refusal.data.unwrap_or_default());
        for (all, listed) in [(&mut parameters, &data["parameters"]), (&mut variables, &data["variables"])] {
            for asked in listed.as_array().into_iter().flatten() {
                if !all.iter().any(|known| known["name"] == asked["name"]) {
                    all.push(asked.clone());
                }
            }
        }
        inlined |= data["inlined"] == true;
        if retry.is_null() {
            retry = data["retry"].clone();
        }
    }
    let count = parameters.len() + variables.len();
    CommandError::with_code(PARAMETERS_NEEDED, t!("database-error-parameters-needed", count = count)).with_data(json!({
        "parameters": parameters,
        "variables": variables,
        "inlined": inlined,
        "retry": retry,
    }))
}

fn names(targets: &[safety::Target]) -> String {
    targets.iter().map(|target| format!("{} ({})", target.name, target.type_name)).collect::<Vec<_>>().join(", ")
}

impl Command for RunOnConnections {
    type Params = RunOnParams;
    type Output = SeveralStarted;
    const ID: &'static str = "db.script.runOnConnections";
    const TITLE: &'static str = "Execute on Several Connections";
    const DESCRIPTION: &'static str = "Runs a console's or `.sql` file's statements (the active one; `scope` `script` by default, `statement` or `fromCaret`) on each connection of `connections` (ids from db.dataSource.list, at most 20) in turn, split in each one's dialect, in the console's own session there; the file opens as a tab of the Database workspace, whose results panel shows each connection's outcome (`{\"kind\": \"connections\"}`) and result tabs named after their connection. `stopOnError` (default true) stops a connection's script at its first error with a rollback and skips the connections after it; false runs everything. Placeholders (`:name`; `$1` on PostgreSQL; `?` elsewhere) and `${name}` variables take the same step as db.console.execute, with one set of values for every connection: without `parameters` the run is refused with code `parametersNeeded` listing the placeholders and variables of every connection once, and `retry`; each connection binds them on its server or, where its driver cannot (SQLite), writes them as quoted literals, listed in its target's `inlined`. Before anything runs every connection is checked by its own type (ADR-0172): if any refuses (`readOnly`, `agentReadOnly` with `handOff`, `agentNotAllowed`) the whole run is refused with that code and `targets` naming each connection's refusal; if any asks, it is refused with code `confirmExecute`, a `token` and `targets` listing every connection with its type and the statements that ask there: run again with `confirmed: token`. Connects each connection when its turn comes, never before. Answers at once; read progress with db.script.connectionsState (`waitMs`), stop with db.operation.cancel.";

    fn run(&self, session: &mut Session, params: RunOnParams) -> Result<SeveralStarted, CommandError> {
        let path = super::scripts::script_path(session, params.path.clone())?;
        let mut connections: Vec<String> = Vec::new();
        for id in params.connections.iter().map(|id| id.trim()).filter(|id| !id.is_empty()) {
            if !connections.iter().any(|known| known == id) {
                connections.push(id.to_owned());
            }
        }
        if connections.is_empty() {
            return Err(CommandError::with_code("noTargets", t!("database-error-several-none")));
        }
        if connections.len() > MOST_TARGETS {
            return Err(CommandError::with_code(
                "tooManyTargets",
                t!("database-error-several-too-many", most = MOST_TARGETS, count = connections.len()),
            ));
        }
        let entries = connections.iter().map(|id| find(session, id)).collect::<Result<Vec<_>, _>>()?;
        if session.state::<SeveralRuns>().and_then(|runs| runs.runs.get(&path)).is_some_and(Run::running) {
            return Err(CommandError::with_code("running", t!("database-error-several-running")));
        }
        let scope = params.scope.unwrap_or(Scope::Script);
        let stop_on_error = params.stop_on_error.unwrap_or(true);
        let mut retry = json!({
            "path": path,
            "connections": connections,
            "scope": scope,
            "offset": params.offset,
            "stopOnError": stop_on_error,
            "newTab": params.new_tab,
        });
        let asking = retry.clone();
        params.given.add_to(&mut retry);
        if !params.new_tab {
            consoles::refuse_replacing(session, &path, (Self::ID, retry.clone()))?;
        }
        let targets: Vec<safety::Target> = entries.iter().map(|entry| safety::target(session, entry)).collect();
        let mut prepared: Vec<Prepared> = Vec::new();
        let mut needed: Vec<CommandError> = Vec::new();
        for connection in &connections {
            let dialect = consoles::dialect_of(session, Some(connection));
            let statements = consoles::pick(session, &path, (scope, params.offset), dialect)?;
            let asked = (Self::ID, asking.clone());
            match variables::prepare(session, Some(&path), connection, statements, (&params.given, asked)) {
                Ok(ready) => prepared.push(ready),
                Err(error) if error.code.as_deref() == Some(PARAMETERS_NEEDED) => {
                    needed.push(error);
                    prepared.push(Prepared::default());
                }
                Err(error) => return Err(error),
            }
        }
        if !needed.is_empty() {
            return Err(merged(needed));
        }
        let mut picked = Vec::new();
        let mut ready: Vec<Option<Checked>> = Vec::new();
        let mut asked: Vec<Asked> = Vec::new();
        let mut refused: Vec<(usize, CommandError)> = Vec::new();
        for (index, connection) in connections.iter().enumerate() {
            let statements = std::mem::take(&mut prepared[index].statements);
            picked.push(statements.clone());
            let checked = consoles::check(
                session,
                &path,
                (connection.clone(), statements),
                Source::Connections,
                (&Confirmation::default(), (Self::ID, retry.clone())),
            );
            match checked {
                Ok(checked) => ready.push(Some(checked)),
                Err(error) if error.code.as_deref() == Some(CONFIRM_EXECUTE) => {
                    let data = Value::Object(error.data.clone().unwrap_or_default());
                    let token = data["token"].as_str().unwrap_or_default().to_owned();
                    asked.push(Asked { index, token, data });
                    ready.push(None);
                }
                Err(error) => {
                    refused.push((index, error));
                    ready.push(None);
                }
            }
        }
        if let Some((_, first)) = refused.first() {
            let code = code_of(first).to_owned();
            let listed: Vec<Value> = refused
                .iter()
                .map(|(index, error)| {
                    let data = Value::Object(error.data.clone().unwrap_or_default());
                    let mut entry = json!({
                        "target": targets[*index],
                        "code": code_of(error),
                        "message": error.message,
                    });
                    if let Some(hand_off) = data.get("handOff") {
                        entry["handOff"] = hand_off.clone();
                    }
                    entry
                })
                .collect();
            let refusing: Vec<safety::Target> = refused.iter().map(|(index, _)| targets[*index].clone()).collect();
            let message = t!(
                "database-error-several-refused",
                count = refused.len(),
                names = names(&refusing),
                reason = first.message.as_str()
            );
            return Err(CommandError::with_code(&code, message)
                .with_data(json!({ "targets": listed, "total": connections.len() })));
        }
        let mut confirmed = vec![false; connections.len()];
        if !asked.is_empty() {
            let tokens: Vec<Option<String>> = (0..connections.len())
                .map(|index| asked.iter().find(|ask| ask.index == index).map(|ask| ask.token.clone()))
                .collect();
            let token = combined(&connections, &tokens);
            if params.confirmed.as_deref() != Some(token.as_str()) {
                let dangerous = asked.iter().any(|ask| ask.data["dangerous"] == true);
                let listed: Vec<Value> = targets
                    .iter()
                    .enumerate()
                    .map(|(index, target)| {
                        let mut entry = json!({
                            "target": target,
                            "asks": false,
                            "statements": [],
                            "asked": 0,
                            "total": picked[index].len(),
                            "dangerous": false,
                            "truncated": false,
                        });
                        if let Some(ask) = asked.iter().find(|ask| ask.index == index) {
                            let shown = listed(&ask.data);
                            entry["asks"] = json!(true);
                            entry["statements"] = shown["statements"].clone();
                            entry["asked"] = shown["asked"].clone();
                            entry["truncated"] = shown["truncated"].clone();
                            entry["dangerous"] = ask.data["dangerous"].clone();
                        }
                        entry
                    })
                    .collect();
                let asking: Vec<safety::Target> = asked.iter().map(|ask| targets[ask.index].clone()).collect();
                let message = t!(
                    "database-error-several-confirm",
                    count = connections.len(),
                    asking = asked.len(),
                    names = names(&asking)
                );
                let mut again = retry.clone();
                again["confirmed"] = json!(token);
                return Err(CommandError::with_code(CONFIRM_EXECUTE, message).with_data(json!({
                    "token": token,
                    "targets": listed,
                    "asking": asked.len(),
                    "total": connections.len(),
                    "dangerous": dangerous,
                    "retry": { "command": Self::ID, "params": again },
                })));
            }
            for ask in &asked {
                let confirmation = Confirmation { confirmed: Some(ask.token.clone()), skip: Vec::new() };
                let checked = consoles::check(
                    session,
                    &path,
                    (connections[ask.index].clone(), picked[ask.index].clone()),
                    Source::Connections,
                    (&confirmation, (Self::ID, retry.clone())),
                )?;
                ready[ask.index] = Some(checked);
                confirmed[ask.index] = true;
            }
        }
        let open =
            OpenTabParams { workspace: WORKSPACE.to_owned(), path: Some(path.clone()), ..OpenTabParams::default() };
        let tab = OpenTab.run(session, open)?;
        let slots = targets
            .into_iter()
            .zip(ready)
            .zip(confirmed)
            .zip(prepared)
            .map(|(((target, checked), confirmed), prepared)| Slot {
                statements: checked.as_ref().map_or(0, |checked| checked.statements.len()),
                read_only: checked.as_ref().is_some_and(|checked| checked.read_only),
                notes: prepared.notes,
                inlined: prepared.inlined,
                confirmed,
                checked,
                target,
                status: TargetStatus::Waiting,
                query: None,
                error: None,
            })
            .collect();
        let runs = session.state_mut::<SeveralRuns>();
        runs.next += 1;
        let run = Run { id: runs.next, stop_on_error, replace: !params.new_tab, cancelled: false, slots };
        runs.runs.insert(path.clone(), run);
        advance(session, &path);
        let run = answer(session, &path).ok_or_else(|| CommandError::new(t!("database-error-several-none")))?;
        Ok(SeveralStarted { run, tab })
    }
}

/// Which run to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// The console or `.sql` file: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
    /// How long to wait for the run's end, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
}

pub struct ConnectionsState;

impl Command for ConnectionsState {
    type Params = StateParams;
    type Output = SeveralRun;
    const ID: &'static str = "db.script.connectionsState";
    const TITLE: &'static str = "Several Connections Run State";
    const DESCRIPTION: &'static str = "The last run of a console or `.sql` file (the active one) on several connections (db.script.runOnConnections), waiting up to `waitMs` (60 000 at most) for its end: each connection in order with its type, status (`waiting`, `running`, `done`, `failed`, `cancelled`, `skipped`), query id, statements run, rows read and changed, time, error in words and its result ids (read rows with db.result.page). Never rows or secrets. Refused with code `noRun` when the console has none.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<SeveralRun, CommandError> {
        let path = super::scripts::script_path(session, params.path)?;
        let deadline = Instant::now() + Duration::from_millis(params.wait_ms.unwrap_or(0).min(MOST_WAIT_MS));
        loop {
            consoles::step(session);
            let running = session.state::<SeveralRuns>().and_then(|runs| runs.runs.get(&path)).map(Run::running);
            match running {
                None => return Err(CommandError::with_code("noRun", t!("database-error-several-no-run"))),
                Some(true) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                Some(_) => break,
            }
        }
        answer(session, &path).ok_or_else(|| CommandError::with_code("noRun", t!("database-error-several-no-run")))
    }
}

#[cfg(test)]
mod tests;

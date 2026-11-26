use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{
    ConnectionState, Engine, Message, OperationStatus, QueryOptions, QueryState, QueryStatement, StatementStatus,
};
use zen_db::history::Source;
use zen_db::results::Paging;
use zen_db::sql::{BlankLines, Dialect, Script, Statement};
use zen_i18n::t;

use super::connections::{describe, request};
use super::safety::{Confirmation, Run};
use super::sources::{Database, connection_types, files, find};
use crate::workspaces::Workspaces;
use crate::{CharEdit, Workbench};

pub const MOST_TABS: usize = 20;

const KEPT_QUERIES: usize = 50;

/// The Database settings of the IDE: how statements are split and how results are read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DbSettings {
    /// When a blank line ends a statement for Execute Statement: `always` (the default), `smart` or `never`.
    pub blank_lines: BlankLines,
    /// How results are read page by page.
    pub paging: Paging,
    /// The memory all results of a window may take, in MiB.
    pub budget_mb: usize,
    /// Whether a script stops at its first error, rolling back the open transaction.
    pub stop_on_error: bool,
    /// The view results open in: `grid`, `text` or `record`; the last one chosen.
    pub view: ResultView,
    /// Whether the value panel shows beside the grid.
    pub value_panel: bool,
    /// The format the value panel shows values in when they offer it; each value's preferred one when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value_format: Option<zen_db::present::ValueFormat>,
}

impl Default for DbSettings {
    fn default() -> Self {
        Self {
            blank_lines: BlankLines::Always,
            paging: Paging::default(),
            budget_mb: zen_db::results::DEFAULT_BUDGET / (1024 * 1024),
            stop_on_error: true,
            view: ResultView::Grid,
            value_panel: true,
            value_format: None,
        }
    }
}

/// How a result is shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ResultView {
    /// Rows and columns.
    #[default]
    Grid,
    /// Rows as aligned plain text.
    Text,
    /// One row as a form.
    Record,
    /// Documents in the shell's JSON, for document databases (MongoDB); other results show as a grid.
    Json,
}

/// What the results panel of a console shows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind", content = "result")]
pub enum PanelTab {
    /// A result, by id.
    Result(u64),
    /// The server's messages (`NOTICE`, `RAISE`, `PRINT`).
    Output,
    /// The execution log: every statement run, its time and its outcome.
    Log,
    /// The console's variables and remembered parameter values.
    Variables,
    /// The connections of a run on several connections, each with its outcome.
    Connections,
}

/// A result tab of a console.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResultTab {
    /// The result's id.
    pub result: u64,
    /// Its title, numbered within its run: the table read (`orders`, then `orders 2`), or `Result N`.
    pub title: String,
    /// Whether it is pinned: a new run leaves it.
    pub pinned: bool,
    /// The query that made it.
    pub query: u64,
    /// The statement's index in that query.
    pub statement: usize,
    /// The connection that made it, when the console ran on several connections.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The table its title names, for numbering the tabs of one run.
    #[serde(skip)]
    named: Option<String>,
}

#[derive(Clone, Debug)]
struct Mark {
    start: usize,
    end: usize,
    text: String,
    tooltip: Option<String>,
    error: bool,
}

#[derive(Clone, Debug, Default)]
struct Marks {
    version: u64,
    problems: Vec<Mark>,
    hints: Vec<Mark>,
    ran: Option<(usize, usize)>,
}

impl Marks {
    fn moved(&self, edits: &[CharEdit]) -> Self {
        let mut moved = self.clone();
        if !edits.is_empty() {
            moved.ran = None;
        }
        for edit in edits {
            let keep =
                |mark: &Mark| mark.end <= edit.range.start || mark.start >= edit.range.end.max(edit.range.start + 1);
            moved.problems.retain(keep);
            moved.hints.retain(|mark| mark.end < edit.range.start || mark.start >= edit.range.end);
            for mark in moved.problems.iter_mut().chain(moved.hints.iter_mut()) {
                mark.start = edit.map(mark.start);
                mark.end = edit.map(mark.end);
            }
        }
        moved
    }
}

type CaretStatement = (u64, usize, usize, Option<(usize, usize)>);

#[derive(Clone, Debug, Default)]
struct Console {
    connection: Option<String>,
    schema: Option<String>,
    tabs: Vec<ResultTab>,
    active: Option<PanelTab>,
    queries: Vec<u64>,
    running: Option<u64>,
    absorbed: HashSet<u64>,
    shown: bool,
    maximized: bool,
    marks: Marks,
    caret: Option<CaretStatement>,
}

struct Split {
    version: u64,
    dialect: Dialect,
    script: Script,
}

#[derive(Default)]
pub struct Consoles {
    consoles: HashMap<PathBuf, Console>,
    splits: HashMap<PathBuf, Split>,
    settings: Option<DbSettings>,
    schemas: HashMap<String, (u64, Option<Vec<String>>)>,
}

impl Consoles {
    pub fn tabs(&self, path: &Path) -> &[ResultTab] {
        self.consoles.get(path).map_or(&[], |console| console.tabs.as_slice())
    }

    pub fn running(&self, path: &Path) -> Option<u64> {
        self.consoles.get(path).and_then(|console| console.running)
    }

    pub fn console_of(&self, result: u64) -> Option<&Path> {
        self.consoles
            .iter()
            .find(|(_, console)| console.tabs.iter().any(|tab| tab.result == result))
            .map(|(path, _)| path.as_path())
    }
}

fn settings_file(session: &Session) -> Option<PathBuf> {
    session
        .state::<Workspaces>()
        .and_then(Workspaces::data_dir)
        .map(|data| data.join("databases").join("settings.json"))
}

pub fn settings(session: &mut Session) -> DbSettings {
    if let Some(settings) = session.state::<Consoles>().and_then(|consoles| consoles.settings.clone()) {
        return settings;
    }
    let read = settings_file(session)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str::<DbSettings>(&text).ok())
        .unwrap_or_default();
    session.state_mut::<Consoles>().settings = Some(read.clone());
    read
}

pub fn engine(session: &mut Session) -> Engine {
    let settings = settings(session);
    let engine = session.state_mut::<Database>().engine();
    super::history::attach(session, &engine);
    engine.results().set_paging(settings.paging.clone());
    engine.results().budget().set_limit(settings.budget_mb.max(1).saturating_mul(1024 * 1024));
    engine
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

fn char_of(text: &str, byte: usize) -> usize {
    let byte = byte.min(text.len());
    text.as_bytes()[..byte].iter().filter(|byte| (**byte as i8) >= -0x40).count()
}

fn byte_of(text: &str, chars: usize) -> usize {
    if chars == 0 {
        return 0;
    }
    let mut seen = 0;
    for (at, byte) in text.as_bytes().iter().enumerate() {
        if (*byte as i8) >= -0x40 {
            if seen == chars {
                return at;
            }
            seen += 1;
        }
    }
    text.len()
}

pub fn context_of(path: &Path) -> ContextKind {
    let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let folder = path.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned());
    ContextKind::Console(match folder {
        Some(folder) => format!("{folder}/{stem}"),
        None => stem,
    })
}

pub(super) fn known_connections(session: &Session) -> Vec<String> {
    files(session)
        .entries()
        .map(|entries| entries.into_iter().map(|entry| entry.source.id).collect())
        .unwrap_or_default()
}

fn tab_data(session: &Session, path: &Path) -> serde_json::Value {
    session
        .state::<Workspaces>()
        .and_then(|workspaces| {
            workspaces
                .tabs(super::WORKSPACE)
                .iter()
                .find(|tab| tab.path.as_deref() == Some(path))
                .map(|tab| tab.data.clone())
        })
        .unwrap_or(serde_json::Value::Null)
}

pub(super) fn is_console(session: &Session, path: &Path) -> bool {
    session.state::<Workspaces>().is_some_and(|workspaces| workspaces.is_console(path))
}

pub fn connection_of(session: &Session, path: &Path) -> Option<String> {
    if super::injected::hosts(path) {
        return super::injected::binding(session, path).map(|(binding, _)| binding.connection);
    }
    let known = known_connections(session);
    if !is_console(session, path) {
        return super::scripts::bound(session, path).map(|binding| binding.connection).filter(|id| known.contains(id));
    }
    let chosen = session
        .state::<Consoles>()
        .and_then(|consoles| consoles.consoles.get(path))
        .and_then(|console| console.connection.clone());
    let data = tab_data(session, path);
    let kept = data["connection"].as_str().map(str::to_owned);
    if let Some(id) = chosen.or(kept).filter(|id| known.contains(id)) {
        return Some(id);
    }
    if data["removed"].is_string() {
        return None;
    }
    let folder = path.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned());
    match folder {
        Some(folder) if known.contains(&folder) => return Some(folder),
        Some(folder) if folder != super::LOCAL_CONNECTION => return None,
        _ => {}
    }
    (known.len() == 1).then(|| known[0].clone())
}

pub(super) fn tab_label(session: &Session, path: &Path) -> Option<String> {
    let connection = connection_of(session, path)?;
    let entries = files(session).entries().ok()?;
    let name = entries.into_iter().find(|entry| entry.source.id == connection).map(|entry| entry.source.name);
    Some(name.unwrap_or(connection))
}

pub fn schema_of(session: &Session, path: &Path) -> Option<String> {
    if super::injected::hosts(path) {
        return super::injected::binding(session, path).and_then(|(binding, _)| binding.schema);
    }
    if !is_console(session, path) {
        return super::scripts::bound(session, path).and_then(|binding| binding.schema);
    }
    session
        .state::<Consoles>()
        .and_then(|consoles| consoles.consoles.get(path))
        .and_then(|console| console.schema.clone())
        .or_else(|| tab_data(session, path)["schema"].as_str().map(str::to_owned))
}

pub(super) fn dialect_of(session: &mut Session, connection: Option<&str>) -> Dialect {
    let Some(id) = connection else { return Dialect::Standard };
    let Ok(entry) = find(session, id) else { return Dialect::Standard };
    let engine = session.state_mut::<Database>().engine();
    engine
        .driver(&entry.source.driver)
        .map_or(Dialect::Standard, |driver| Dialect::of_language(&driver.capabilities().language))
}

pub(super) fn console_path(session: &Session, path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    let path = match path {
        Some(path) => path,
        None => session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .map(|document| document.path().to_owned())
            .ok_or_else(|| CommandError::with_code("noConsole", t!("database-error-no-console")))?,
    };
    let is_console = session.state::<Workspaces>().is_some_and(|workspaces| workspaces.is_console(&path));
    let is_sql = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("sql"));
    if !is_console && !is_sql && !super::injected::hosts(&path) {
        return Err(CommandError::with_code("noConsole", t!("database-error-no-console")));
    }
    super::guard_agent_file(session, &path)?;
    Ok(path)
}

struct Snapshot {
    text: String,
    selection: (usize, usize),
}

fn snapshot(session: &Session, path: &Path) -> Result<Snapshot, CommandError> {
    let document = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path));
    match document {
        Some(document) => {
            let primary = document.selections().primary();
            Ok(Snapshot { text: document.text().to_owned(), selection: (primary.range().start, primary.range().end) })
        }
        None => {
            let text = std::fs::read_to_string(path).map_err(|error| {
                CommandError::new(t!(
                    "database-error-read-console",
                    path = crate::display(path),
                    error = error.to_string()
                ))
            })?;
            Ok(Snapshot { text, selection: (0, 0) })
        }
    }
}

struct Shot {
    selection: (usize, usize),
}

fn with_split<T>(
    session: &mut Session,
    path: &Path,
    dialect: Dialect,
    use_it: impl FnOnce(&Script, &str, &Shot) -> T,
) -> Result<T, CommandError> {
    let mut consoles = std::mem::take(session.state_mut::<Consoles>());
    let outcome = (|| {
        let document = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path));
        let Some(document) = document else {
            consoles.splits.remove(path);
            let owned = snapshot(session, path)?;
            let script = Script::new(&owned.text, dialect);
            return Ok(use_it(&script, &owned.text, &Shot { selection: owned.selection }));
        };
        let primary = document.selections().primary();
        let previous = consoles.splits.get(path).filter(|split| split.dialect == dialect).map(|split| split.version);
        let changes = previous.and_then(|version| document.changes_since(version));
        let (text, version, selection) =
            (document.text(), document.version(), (primary.range().start, primary.range().end));
        let fresh = match (consoles.splits.get_mut(path), changes) {
            (Some(split), Some(changes)) if split.dialect == dialect => {
                if let Some(change) = zen_syntax::TextChange::merged(&changes) {
                    split.script.edit(text, change.start, change.old_end, change.new_end);
                }
                split.version = version;
                false
            }
            (Some(split), _) if split.dialect == dialect && split.version == version => false,
            _ => true,
        };
        if fresh {
            consoles.splits.insert(path.to_owned(), Split { version, dialect, script: Script::new(text, dialect) });
        }
        let split = &consoles.splits[path];
        Ok(use_it(&split.script, text, &Shot { selection }))
    })();
    *session.state_mut::<Consoles>() = consoles;
    outcome
}

pub(super) fn statement_around(
    session: &mut Session,
    path: &Path,
    dialect: Dialect,
    caret: usize,
) -> Option<(usize, String)> {
    with_split(session, path, dialect, |script, text, _| {
        let caret = caret.min(text.len());
        match script.statement_at(text, caret, BlankLines::Smart) {
            Some(statement)
                if statement.start <= caret
                    && (statement.delimiter_end == statement.end || caret < statement.delimiter_end) =>
            {
                let end = statement.end.max(caret).min(text.len());
                (statement.start, text[statement.start..end].to_owned())
            }
            _ => (caret, String::new()),
        }
    })
    .ok()
}

/// A statement as the commands answer it, in characters of the console's text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementAnswer {
    /// Its first character.
    pub start: usize,
    /// The character after its last, before its delimiter.
    pub end: usize,
    /// The line it starts on, one-based.
    pub line: usize,
    /// Its text, cut to 2000 characters.
    pub text: String,
    /// Whether the text was cut.
    pub cut: bool,
    /// Whether it is a command of the client (a psql meta-command, `DELIMITER`), which is not sent.
    pub client: bool,
}

fn answer(text: &str, statement: &Statement) -> StatementAnswer {
    let body = statement.text(text);
    let shown: String = body.chars().take(2000).collect();
    let cut = shown.len() < body.len();
    StatementAnswer {
        start: char_of(text, statement.start),
        end: char_of(text, statement.end),
        line: text[..statement.start].matches('\n').count() + 1,
        text: shown,
        cut,
        client: statement.client,
    }
}

/// What `db.script.split` splits.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SplitParams {
    /// The text to split; the console's text when absent.
    pub text: Option<String>,
    /// The console or `.sql` file to split when no text is given; the active one when absent.
    pub path: Option<PathBuf>,
    /// The dialect: `postgres`, `mysql`, `sqlServer`, `standard` or `lines`; the console's connection's when absent.
    pub dialect: Option<Dialect>,
    /// The most statements to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// The statements of a script.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SplitAnswer {
    /// The dialect used.
    pub dialect: Dialect,
    /// The statements, at most `limit`.
    pub statements: Vec<StatementAnswer>,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct SplitScript;

impl Command for SplitScript {
    type Params = SplitParams;
    type Output = SplitAnswer;
    const ID: &'static str = "db.script.split";
    const TITLE: &'static str = "Split Script into Statements";
    const DESCRIPTION: &'static str = "Splits SQL into its statements as Execute Script does: on delimiters (`;`, MySQL `DELIMITER`, SQL Server `GO`), never inside quotes, comments, dollar-quoted bodies, brackets or BEGIN … END blocks; psql meta-commands are client commands. Splits `text`, or the console or `.sql` file at `path` (the active one). Answers each statement's character range, line and text (cut to 2000 characters), at most `limit` (50) with `total` and `truncated`.";

    fn run(&self, session: &mut Session, params: SplitParams) -> Result<SplitAnswer, CommandError> {
        let limit = params.limit.unwrap_or(50);
        let (dialect, found) = match params.text {
            Some(text) => {
                let dialect = params.dialect.unwrap_or_default();
                let statements = zen_db::sql::split(&text, dialect)
                    .iter()
                    .map(|statement| answer(&text, statement))
                    .collect::<Vec<_>>();
                (dialect, statements)
            }
            None => {
                let path = console_path(session, params.path)?;
                let connection = connection_of(session, &path);
                let dialect = params.dialect.unwrap_or_else(|| dialect_of(session, connection.as_deref()));
                let found = with_split(session, &path, dialect, |script, text, _| {
                    script.statements(text).iter().map(|statement| answer(text, statement)).collect::<Vec<_>>()
                })?;
                (dialect, found)
            }
        };
        let total = found.len();
        Ok(SplitAnswer {
            dialect,
            statements: found.into_iter().take(limit).collect(),
            total,
            truncated: total > limit,
        })
    }
}

/// Which console and where.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ConsoleParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
}

/// Where to find the statement.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StatementParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// A character offset instead of the caret.
    pub offset: Option<usize>,
    /// When blank lines end a statement; the setting when absent.
    pub blank_lines: Option<BlankLines>,
}

/// The statement Execute Statement runs.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatementAt {
    /// The statement, or the selection when one is made; absent when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement: Option<StatementAnswer>,
    /// Whether it is the selection.
    pub selection: bool,
}

fn statement_at(
    session: &mut Session,
    path: &Path,
    offset: Option<usize>,
    mode: Option<BlankLines>,
) -> Result<StatementAt, CommandError> {
    let connection = connection_of(session, path);
    let dialect = dialect_of(session, connection.as_deref());
    if super::injected::hosts(path) {
        let selection =
            offset.is_none() && snapshot(session, path).is_ok_and(|shot| shot.selection.0 != shot.selection.1);
        let found = super::injected::statements(session, path, (Scope::Statement, offset), dialect)?;
        let text = snapshot(session, path)?.text;
        let statement = found.into_iter().next().map(|statement| {
            let shown: String = statement.text.chars().take(2000).collect();
            StatementAnswer {
                start: statement.start,
                end: statement.end,
                line: text.chars().take(statement.start).filter(|c| *c == '\n').count() + 1,
                cut: shown.len() < statement.text.len(),
                text: shown,
                client: statement.client,
            }
        });
        return Ok(StatementAt { statement, selection });
    }
    let mode = mode.unwrap_or_else(|| settings(session).blank_lines);
    with_split(session, path, dialect, |script, text, shot| {
        let (from, to) = shot.selection;
        if offset.is_none() && from != to {
            let (start, end) = (byte_of(text, from), byte_of(text, to));
            let statement = Statement { start, end, delimiter_end: end, client: false };
            return StatementAt { statement: Some(answer(text, &statement)), selection: true };
        }
        let caret = byte_of(text, offset.unwrap_or(to));
        let statement = script.statement_at(text, caret, mode).map(|statement| answer(text, &statement));
        StatementAt { statement, selection: false }
    })
}

pub(super) fn statement_text(session: &mut Session, path: &Path) -> Result<Option<String>, CommandError> {
    let connection = connection_of(session, path);
    let dialect = dialect_of(session, connection.as_deref());
    if super::injected::hosts(path) {
        let statements = super::injected::statements(session, path, (Scope::Statement, None), dialect)?;
        return Ok(statements.into_iter().next().map(|statement| statement.text));
    }
    let mode = settings(session).blank_lines;
    with_split(session, path, dialect, |script, text, shot| {
        let (from, to) = shot.selection;
        if from != to {
            return Some(text[byte_of(text, from)..byte_of(text, to)].to_owned());
        }
        script.statement_at(text, byte_of(text, to), mode).map(|statement| statement.text(text).to_owned())
    })
}

pub struct StatementUnderCaret;

impl Command for StatementUnderCaret {
    type Params = StatementParams;
    type Output = StatementAt;
    const ID: &'static str = "db.console.statement";
    const TITLE: &'static str = "Statement at Caret";
    const DESCRIPTION: &'static str = "The statement Execute Statement (⌘⏎) would run in a console or `.sql` file (the active one): the selection when one is made, else the statement containing the caret (or `offset`); the caret right after a delimiter belongs to the statement before it, and on an empty line the line before counts. Blank lines end statements as `blankLines` says (the setting, `always` by default). Answers its character range, line and text.";

    fn run(&self, session: &mut Session, params: StatementParams) -> Result<StatementAt, CommandError> {
        let path = console_path(session, params.path)?;
        statement_at(session, &path, params.offset, params.blank_lines)
    }
}

fn step_statement(session: &mut Session, path: Option<PathBuf>, forward: bool) -> Result<StatementAt, CommandError> {
    let path = console_path(session, path)?;
    let connection = connection_of(session, &path);
    let dialect = dialect_of(session, connection.as_deref());
    let mode = settings(session).blank_lines;
    let found = with_split(session, &path, dialect, |script, text, shot| {
        let caret = byte_of(text, shot.selection.1);
        script.neighbour(text, caret, forward, mode).map(|statement| answer(text, &statement))
    })?;
    if let Some(found) = &found
        && let Some(document) =
            session.state_mut::<Workbench>().documents.iter_mut().find(|document| document.path() == path)
    {
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(found.start)));
    }
    Ok(StatementAt { statement: found, selection: false })
}

pub struct NextStatement;

impl Command for NextStatement {
    type Params = ConsoleParams;
    type Output = StatementAt;
    const ID: &'static str = "db.console.nextStatement";
    const TITLE: &'static str = "Next Statement";
    const DESCRIPTION: &'static str = "Moves the caret of a console (the active one) to the start of the statement after the one under it, as Execute Statement splits them; answers that statement, or none at the end.";

    fn run(&self, session: &mut Session, params: ConsoleParams) -> Result<StatementAt, CommandError> {
        step_statement(session, params.path, true)
    }
}

pub struct PreviousStatement;

impl Command for PreviousStatement {
    type Params = ConsoleParams;
    type Output = StatementAt;
    const ID: &'static str = "db.console.previousStatement";
    const TITLE: &'static str = "Previous Statement";
    const DESCRIPTION: &'static str = "Moves the caret of a console (the active one) to the start of the statement before the one under it; answers that statement, or none at the start.";

    fn run(&self, session: &mut Session, params: ConsoleParams) -> Result<StatementAt, CommandError> {
        step_statement(session, params.path, false)
    }
}

/// What a run started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunStarted {
    /// The query's id, for `db.query.state`, `db.query.cancel` and `db.results.list`.
    pub query: u64,
    /// The connection it runs on.
    pub connection: String,
    /// The console or file.
    pub path: PathBuf,
    /// How many statements it runs.
    pub statements: usize,
    /// Whether the connection was connected first.
    pub connecting: bool,
}

/// What a run takes of a console or script.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    /// The selection, else the statement under the caret.
    #[default]
    Statement,
    /// Every statement, or those of the selection.
    Script,
    /// The statement under the caret and every one after it.
    FromCaret,
}

pub fn ensure_connected(session: &mut Session, connection: &str) -> Result<bool, CommandError> {
    let engine = engine(session);
    match engine.status(connection).state {
        ConnectionState::Connected | ConnectionState::Closed | ConnectionState::Connecting => Ok(false),
        ConnectionState::Disconnected | ConnectionState::Failed => {
            let entry = find(session, connection)?;
            let request = request(session, &entry, None);
            engine.connect(request).map_err(|error| failed(&error))?;
            Ok(true)
        }
    }
}

fn file_name(path: &Path) -> Option<String> {
    path.file_name().map(|name| name.to_string_lossy().into_owned())
}

pub(super) fn unbind(session: &mut Session, path: &Path, name: &str) {
    let console = console_mut(session, path);
    console.connection = None;
    console.schema = None;
    console.tabs.clear();
    console.active = None;
    console.running = None;
    let data = serde_json::json!({ "removed": name });
    session.state_mut::<Workspaces>().set_tab_data(super::WORKSPACE, path, data);
}

fn console_mut<'a>(session: &'a mut Session, path: &Path) -> &'a mut Console {
    session.state_mut::<Consoles>().consoles.entry(path.to_owned()).or_default()
}

pub(super) fn refuse_replacing(
    session: &Session,
    path: &Path,
    retry: (&str, serde_json::Value),
) -> Result<(), CommandError> {
    let unpinned: Vec<u64> = session
        .state::<Consoles>()
        .map(|consoles| consoles.tabs(path).iter().filter(|tab| !tab.pinned).map(|tab| tab.result).collect())
        .unwrap_or_default();
    super::edits::refuse_staged(session, &unpinned, retry)
}

pub(super) fn pick_bound(
    session: &mut Session,
    path: &Path,
    (scope, offset): (Scope, Option<usize>),
) -> Result<(String, Vec<QueryStatement>), CommandError> {
    let connection = match connection_of(session, path) {
        Some(connection) => connection,
        None if super::injected::hosts(path) => return Err(super::injected::not_bound(session, path)),
        None if !is_console(session, path) => return Err(super::scripts::not_bound(session, path)),
        None => {
            return Err(CommandError::with_code("noConnection", super::no_console_connection(session)));
        }
    };
    let dialect = dialect_of(session, Some(&connection));
    let statements = if super::injected::hosts(path) {
        super::injected::statements(session, path, (scope, offset), dialect)?
    } else {
        pick(session, path, (scope, offset), dialect)?
    };
    Ok((connection, statements))
}

pub(super) fn pick(
    session: &mut Session,
    path: &Path,
    (scope, offset): (Scope, Option<usize>),
    dialect: Dialect,
) -> Result<Vec<QueryStatement>, CommandError> {
    let settings = settings(session);
    with_split(session, path, dialect, |script, text, shot| {
        let (from, to) = offset.map_or(shot.selection, |at| (at, at));
        let picked: Vec<Statement> = match scope {
            Scope::Statement if from != to => {
                let (start, end) = (byte_of(text, from), byte_of(text, to));
                let client = zen_db::params::is_script_command(&text[start..end]);
                vec![Statement { start, end, delimiter_end: end, client }]
            }
            Scope::Statement => {
                let caret = byte_of(text, to);
                let at = match (offset, text[caret..].chars().next()) {
                    (Some(_), Some(first)) if !first.is_whitespace() => caret + first.len_utf8(),
                    _ => caret,
                };
                script.statement_at(text, at, settings.blank_lines).into_iter().collect()
            }
            Scope::Script if from != to => {
                let (start, end) = (byte_of(text, from), byte_of(text, to));
                zen_db::sql::split(&text[start..end], dialect)
                    .into_iter()
                    .map(|statement| Statement {
                        start: statement.start + start,
                        end: statement.end + start,
                        delimiter_end: statement.delimiter_end + start,
                        client: statement.client,
                    })
                    .collect()
            }
            Scope::Script => script.statements(text),
            Scope::FromCaret => script.statements_from(text, byte_of(text, to)),
        };
        picked
            .iter()
            .filter(|statement| !statement.text(text).trim().is_empty())
            .map(|statement| QueryStatement {
                text: statement.text(text).to_owned(),
                start: char_of(text, statement.start),
                end: char_of(text, statement.end),
                client: statement.client,
                params: Vec::new(),
                kept: None,
            })
            .collect::<Vec<_>>()
    })
}

pub(super) fn start(
    session: &mut Session,
    path: &Path,
    (scope, offset): (Scope, Option<usize>),
    (new_tabs, command): (bool, &str),
    (confirmation, given): (&Confirmation, &super::variables::Given),
) -> Result<RunStarted, CommandError> {
    let mut again = serde_json::json!({ "path": path, "newTab": new_tabs });
    if [super::scripts::RunScript::ID, super::injected::RunInjection::ID].contains(&command) {
        again["scope"] = serde_json::json!(scope);
        again["offset"] = serde_json::json!(offset);
    }
    let asking = (command, again.clone());
    given.add_to(&mut again);
    let retry = (command, again);
    let (connection, statements) = pick_bound(session, path, (scope, offset))?;
    if !new_tabs {
        refuse_replacing(session, path, (command, serde_json::json!({ "path": path })))?;
    }
    let prepared = super::variables::prepare(session, Some(path), &connection, statements, (given, asking))?;
    let source = if scope == Scope::Statement { Source::Statement } else { Source::Script };
    launch(session, path, (connection, prepared), (new_tabs, source), (confirmation, retry))
}

#[derive(Clone, Debug)]
pub(super) struct Checked {
    pub(super) connection: String,
    pub(super) statements: Vec<QueryStatement>,
    pub(super) provenance: zen_db::history::Provenance,
    pub(super) read_only: bool,
}

pub(super) fn check(
    session: &mut Session,
    path: &Path,
    (connection, statements): (String, Vec<QueryStatement>),
    source: Source,
    (confirmation, retry): (&Confirmation, (&str, serde_json::Value)),
) -> Result<Checked, CommandError> {
    if statements.is_empty() {
        return Err(CommandError::with_code("noStatement", t!("database-error-no-statement")));
    }
    let entry = find(session, &connection)?;
    let mut provenance = super::history::provenance(session, source, file_name(path));
    let context = context_of(path);
    let run = Run { entry: &entry, context: &context, provenance: &provenance, confirmation, retry };
    let guarded = super::safety::guard(session, &run, statements)?;
    provenance.confirmed = guarded.confirmed;
    Ok(Checked { connection, statements: guarded.statements, provenance, read_only: guarded.read_only })
}

#[derive(Clone, Debug)]
pub(super) struct Start {
    pub(super) new_tabs: bool,
    pub(super) stop_on_error: bool,
    pub(super) schema: Option<String>,
    pub(super) own: bool,
    pub(super) notes: Vec<Message>,
}

pub(super) fn begin(
    session: &mut Session,
    path: &Path,
    checked: Checked,
    start: Start,
) -> Result<RunStarted, CommandError> {
    let Checked { connection, statements, provenance, read_only } = checked;
    let new_tabs = start.new_tabs;
    let connecting = ensure_connected(session, &connection)?;
    let engine = engine(session);
    let options = QueryOptions {
        stop_on_error: start.stop_on_error,
        rollback_on_error: start.stop_on_error,
        timeout: None,
        schema: start.schema,
        provenance,
        read_only,
        notes: start.notes,
    };
    let count = statements.len();
    let context = context_of(path);
    let query = engine.run_query(&connection, context, statements, options).map_err(|error| failed(&error))?;
    let console = console_mut(session, path);
    if !new_tabs {
        let dropped: Vec<u64> = console.tabs.iter().filter(|tab| !tab.pinned).map(|tab| tab.result).collect();
        console.tabs.retain(|tab| tab.pinned);
        for result in dropped {
            engine.results().remove(result);
        }
    }
    if start.own {
        console.connection = Some(connection.clone());
    }
    console.running = Some(query);
    console.queries.push(query);
    if console.queries.len() > KEPT_QUERIES {
        console.queries.remove(0);
    }
    console.shown = true;
    console.marks = Marks::default();
    console.caret = None;
    if start.own && console.active == Some(PanelTab::Connections) {
        console.active = None;
    }
    if console.tabs.is_empty() && console.active != Some(PanelTab::Connections) {
        console.active = Some(PanelTab::Log);
    }
    Ok(RunStarted { query, connection, path: path.to_owned(), statements: count, connecting })
}

pub(super) fn show_connections(session: &mut Session, path: &Path) {
    let console = console_mut(session, path);
    console.active = Some(PanelTab::Connections);
    console.shown = true;
}

fn launch(
    session: &mut Session,
    path: &Path,
    (connection, prepared): (String, super::variables::Prepared),
    (new_tabs, source): (bool, Source),
    (confirmation, retry): (&Confirmation, (&str, serde_json::Value)),
) -> Result<RunStarted, CommandError> {
    let settings = settings(session);
    let super::variables::Prepared { statements, notes, .. } = prepared;
    let checked = check(session, path, (connection, statements), source, (confirmation, retry))?;
    let schema = schema_of(session, path);
    let start = Start { new_tabs, stop_on_error: settings.stop_on_error, schema, own: true, notes };
    begin(session, path, checked, start)
}

/// How to run a console's statements.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExecuteParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// Keep the result tabs there are and open new ones.
    pub new_tab: bool,
    /// The confirmation of a `confirmExecute` refusal: its `token` and the statements to skip.
    #[serde(flatten)]
    pub confirmation: Confirmation,
    /// Values of bind parameters and variables.
    #[serde(flatten)]
    pub given: super::variables::Given,
}

pub struct ExecuteStatement;

impl Command for ExecuteStatement {
    type Params = ExecuteParams;
    type Output = RunStarted;
    const ID: &'static str = "db.console.execute";
    const TITLE: &'static str = "Execute SQL Statement";
    const DESCRIPTION: &'static str = "Execute Statement (⌘⏎) in a console or `.sql` file (the active one): runs the selection as it is, else the statement under the caret (db.console.statement), on the console's connection (connected first when needed) in the console's own session, in the background. Replaces the unpinned result tabs unless `newTab`. Answers the query id at once: read it with db.query.state and db.results.list. Safety (db.connection.readOnly, the connection type): a read-only connection refuses changes (code `readOnly`); where the type confirms SQL execution, statements that change something, and on every type DELETE or UPDATE without WHERE, DROP and TRUNCATE, are refused with code `confirmExecute`, the statements, their risks and a `token`: run again with `confirmed: token` (and `skip`, indexes to leave out). Agents follow the type's agent access: `agentReadOnly` and `agentNotAllowed` refusals cannot be confirmed.";

    fn run(&self, session: &mut Session, params: ExecuteParams) -> Result<RunStarted, CommandError> {
        let path = console_path(session, params.path)?;
        start(
            session,
            &path,
            (Scope::Statement, None),
            (params.new_tab, Self::ID),
            (&params.confirmation, &params.given),
        )
    }
}

pub struct ExecuteInNewTab;

impl Command for ExecuteInNewTab {
    type Params = ExecuteParams;
    type Output = RunStarted;
    const ID: &'static str = "db.console.executeInNewTab";
    const TITLE: &'static str = "Execute SQL in New Tab";
    const DESCRIPTION: &'static str = "As db.console.execute (the selection or the statement under the caret, with the same confirmations), keeping the result tabs there are: the result opens in a new tab.";

    fn run(&self, session: &mut Session, params: ExecuteParams) -> Result<RunStarted, CommandError> {
        let path = console_path(session, params.path)?;
        start(session, &path, (Scope::Statement, None), (true, Self::ID), (&params.confirmation, &params.given))
    }
}

/// How to run a console's script.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// Keep the result tabs there are.
    pub new_tab: bool,
    /// The confirmation of a `confirmExecute` refusal: its `token` and the statements to skip.
    #[serde(flatten)]
    pub confirmation: Confirmation,
    /// Values of bind parameters and variables.
    #[serde(flatten)]
    pub given: super::variables::Given,
}

pub struct ExecuteScript;

impl Command for ExecuteScript {
    type Params = ScriptParams;
    type Output = RunStarted;
    const ID: &'static str = "db.console.executeScript";
    const TITLE: &'static str = "Execute SQL Script";
    const DESCRIPTION: &'static str = "Execute Script (⌥X, also ⌥⌘⏎): runs every statement of a console or `.sql` file (the active one; the selection when one is made), split on delimiters only, one after another in the console's session; each result set opens a tab (at most 20). An error stops the script and rolls back the open transaction (the setting `stopOnError`). Answers the query id at once. Confirmations and refusals as db.console.execute: one `confirmExecute` refusal lists every statement that asks, `skip` leaves some out.";

    fn run(&self, session: &mut Session, params: ScriptParams) -> Result<RunStarted, CommandError> {
        let path = console_path(session, params.path)?;
        start(session, &path, (Scope::Script, None), (params.new_tab, Self::ID), (&params.confirmation, &params.given))
    }
}

pub struct ExecuteFromCaret;

impl Command for ExecuteFromCaret {
    type Params = ScriptParams;
    type Output = RunStarted;
    const ID: &'static str = "db.console.executeFromCaret";
    const TITLE: &'static str = "Execute SQL Script from Caret";
    const DESCRIPTION: &'static str =
        "As db.console.executeScript, from the statement under the caret to the end of the console.";

    fn run(&self, session: &mut Session, params: ScriptParams) -> Result<RunStarted, CommandError> {
        let path = console_path(session, params.path)?;
        start(
            session,
            &path,
            (Scope::FromCaret, None),
            (params.new_tab, Self::ID),
            (&params.confirmation, &params.given),
        )
    }
}

/// The connection to choose.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ChooseConnectionParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// The connection's id.
    pub connection: String,
    /// Changes pending in the console's transaction on its connection so far: `ask` (default) refuses with code `openTransactions`, `commit` or `rollback` ends them first.
    pub transactions: zen_db::engine::TransactionPolicy,
}

/// A console's connection and schema.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    /// The console.
    pub path: PathBuf,
    /// Its connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// Its schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

fn keep_binding(
    session: &mut Session,
    path: &Path,
    chosen: Option<(String, Option<String>)>,
) -> Result<Binding, CommandError> {
    if !is_console(session, path) {
        let binding = chosen.map(|(connection, schema)| super::scripts::ScriptBinding { connection, schema });
        super::scripts::store(session, path, binding.clone())?;
        return Ok(Binding {
            path: path.to_owned(),
            connection: binding.as_ref().map(|binding| binding.connection.clone()),
            schema: binding.and_then(|binding| binding.schema),
        });
    }
    let connection = connection_of(session, path);
    let schema = schema_of(session, path);
    let data = serde_json::json!({ "connection": connection, "schema": schema });
    session.state_mut::<Workspaces>().set_tab_data(super::WORKSPACE, path, data);
    Ok(Binding { path: path.to_owned(), connection, schema })
}

pub struct ChooseConnection;

impl Command for ChooseConnection {
    type Params = ChooseConnectionParams;
    type Output = Binding;
    const ID: &'static str = "db.console.chooseConnection";
    const TITLE: &'static str = "Choose Connection";
    const DESCRIPTION: &'static str = "Sets the connection a console or `.sql` file (the active one) runs on, kept across restarts (a console's with its tab, a project file's per project in the data folder, as db.script.bind); its schema is forgotten. Without a choice a console runs on the connection its folder is named after, or on the only connection there is; a project file runs only on the connection it is bound to. Leaving a connection closes the console's session there: changes pending in its transaction refuse it with code `openTransactions` unless `transactions` is `commit` or `rollback`. Connects nowhere.";

    fn run(&self, session: &mut Session, params: ChooseConnectionParams) -> Result<Binding, CommandError> {
        let path = console_path(session, params.path)?;
        choose_connection(session, &path, params.connection, params.transactions, Self::ID)
    }
}

pub(super) fn choose_connection(
    session: &mut Session,
    path: &Path,
    connection: String,
    transactions: zen_db::engine::TransactionPolicy,
    command: &str,
) -> Result<Binding, CommandError> {
    find(session, &connection)?;
    let current = connection_of(session, path);
    if current.as_ref().is_some_and(|current| *current != connection) {
        let retry = serde_json::json!({ "path": path, "connection": connection });
        super::transactions::close_console_session(session, path, transactions, Some((command, retry)))?;
    }
    let console = console_mut(session, path);
    if console.connection.as_deref() != Some(connection.as_str()) {
        console.schema = None;
    }
    console.connection = Some(connection.clone());
    if !is_console(session, path) {
        let schema = if current.as_deref() == Some(connection.as_str()) { schema_of(session, path) } else { None };
        return keep_binding(session, path, Some((connection, schema)));
    }
    if console_mut(session, path).schema.is_none() {
        let data = serde_json::json!({ "connection": connection });
        session.state_mut::<Workspaces>().set_tab_data(super::WORKSPACE, path, data);
    }
    keep_binding(session, path, None)
}

pub(super) fn choose_schema(
    session: &mut Session,
    path: &Path,
    schema: Option<String>,
) -> Result<Binding, CommandError> {
    let schema = schema.filter(|schema| !schema.trim().is_empty());
    if !is_console(session, path) {
        let connection = connection_of(session, path).ok_or_else(|| super::scripts::not_bound(session, path))?;
        console_mut(session, path).schema.clone_from(&schema);
        return keep_binding(session, path, Some((connection, schema)));
    }
    console_mut(session, path).schema = schema;
    keep_binding(session, path, None)
}

/// The schema to choose.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ChooseSchemaParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// The schema; none goes back to the connection's default.
    pub schema: Option<String>,
}

pub struct ChooseSchema;

impl Command for ChooseSchema {
    type Params = ChooseSchemaParams;
    type Output = Binding;
    const ID: &'static str = "db.console.chooseSchema";
    const TITLE: &'static str = "Choose Schema";
    const DESCRIPTION: &'static str = "Sets the schema a console's or bound `.sql` file's statements run in (PostgreSQL's search_path), kept across restarts; it is set in the session before its next statement runs. A project file bound to no connection is refused with code `noBinding`. Connects nowhere.";

    fn run(&self, session: &mut Session, params: ChooseSchemaParams) -> Result<Binding, CommandError> {
        let path = console_path(session, params.path)?;
        choose_schema(session, &path, params.schema)
    }
}

/// Which schemas to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SchemasParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// The most schemas to answer; 200 when absent.
    pub limit: Option<usize>,
}

/// The schemas of a console's connection.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchemasAnswer {
    /// The connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The schemas, once read; absent while they are read or when the connection is not connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schemas: Option<Vec<String>>,
    /// Whether they are being read.
    pub reading: bool,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct ListSchemas;

impl Command for ListSchemas {
    type Params = SchemasParams;
    type Output = SchemasAnswer;
    const ID: &'static str = "db.console.schemas";
    const TITLE: &'static str = "List Schemas";
    const DESCRIPTION: &'static str = "The schemas of a console's connection, read in the background through its Metadata session when it is connected (call again to get them); never connects.";

    fn run(&self, session: &mut Session, params: SchemasParams) -> Result<SchemasAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        let limit = params.limit.unwrap_or(200);
        let Some(connection) = connection_of(session, &path) else {
            return Ok(SchemasAnswer { connection: None, schemas: None, reading: false, total: 0, truncated: false });
        };
        let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else {
            return Ok(SchemasAnswer {
                connection: Some(connection),
                schemas: None,
                reading: false,
                total: 0,
                truncated: false,
            });
        };
        if !matches!(engine.status(&connection).state, ConnectionState::Connected | ConnectionState::Closed) {
            return Ok(SchemasAnswer {
                connection: Some(connection),
                schemas: None,
                reading: false,
                total: 0,
                truncated: false,
            });
        }
        let known = session.state::<Consoles>().and_then(|consoles| consoles.schemas.get(&connection).cloned());
        let (op, schemas) = match known {
            Some((op, Some(schemas))) => (op, Some(schemas)),
            Some((op, None)) => match engine.operation(op) {
                Some(state) if state.status == OperationStatus::Running => (op, None),
                Some(state) => {
                    let names: Vec<String> = state
                        .nodes
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|node| node.kind == zen_db::model::NodeKind::Schema)
                        .map(|node| node.label)
                        .collect();
                    (op, Some(names))
                }
                None => (op, Some(Vec::new())),
            },
            None => {
                let entry = find(session, &connection)?;
                let database = entry.source.database.clone().unwrap_or_else(|| "postgres".to_owned());
                let op = engine.children(&connection, vec![database]).map_err(|error| failed(&error))?;
                (op, None)
            }
        };
        session.state_mut::<Consoles>().schemas.insert(connection.clone(), (op, schemas.clone()));
        let total = schemas.as_ref().map_or(0, Vec::len);
        let reading = schemas.is_none();
        Ok(SchemasAnswer {
            connection: Some(connection),
            schemas: schemas.map(|schemas| schemas.into_iter().take(limit).collect()),
            reading,
            total,
            truncated: total > limit,
        })
    }
}

/// The results panel of a console.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PanelAnswer {
    /// Whether it is shown below the console.
    pub shown: bool,
    /// Whether it takes the console's whole height.
    pub maximized: bool,
    /// What it shows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<PanelTab>,
}

fn panel(session: &Session, path: &Path) -> PanelAnswer {
    let console =
        session.state::<Consoles>().and_then(|consoles| consoles.consoles.get(path)).cloned().unwrap_or_default();
    PanelAnswer { shown: console.shown, maximized: console.maximized, active: console.active }
}

pub struct ToggleResults;

impl Command for ToggleResults {
    type Params = ConsoleParams;
    type Output = PanelAnswer;
    const ID: &'static str = "db.console.toggleResults";
    const TITLE: &'static str = "Show/Hide Results Panel";
    const DESCRIPTION: &'static str =
        "Shows or hides the results panel below a console (the active one); a run shows it.";

    fn run(&self, session: &mut Session, params: ConsoleParams) -> Result<PanelAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        let console = console_mut(session, &path);
        console.shown = !console.shown;
        if !console.shown {
            console.maximized = false;
        }
        Ok(panel(session, &path))
    }
}

pub struct MaximizeResults;

impl Command for MaximizeResults {
    type Params = ConsoleParams;
    type Output = PanelAnswer;
    const ID: &'static str = "db.console.maximizeResults";
    const TITLE: &'static str = "Maximize Results Panel";
    const DESCRIPTION: &'static str = "Lets the results panel of a console (the active one) take the console's whole height, or gives the editor its place back.";

    fn run(&self, session: &mut Session, params: ConsoleParams) -> Result<PanelAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        let console = console_mut(session, &path);
        console.maximized = !console.maximized;
        console.shown = true;
        Ok(panel(session, &path))
    }
}

/// What the results panel shows.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PanelTabParams {
    /// The console or `.sql` file; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// A result, `output`, `log`, `variables` or `connections`.
    pub tab: PanelTab,
}

pub struct ShowPanelTab;

impl Command for ShowPanelTab {
    type Params = PanelTabParams;
    type Output = PanelAnswer;
    const ID: &'static str = "db.console.showPanelTab";
    const TITLE: &'static str = "Show Results Tab";
    const DESCRIPTION: &'static str = "Shows one tab of a console's results panel: a result (`{\"kind\": \"result\", \"result\": 5}`), the server's output (`{\"kind\": \"output\"}`), the execution log (`{\"kind\": \"log\"}`), the variables and remembered parameter values (`{\"kind\": \"variables\"}`, read them with db.console.variables) or, after a run on several connections, each connection's outcome (`{\"kind\": \"connections\"}`).";

    fn run(&self, session: &mut Session, params: PanelTabParams) -> Result<PanelAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        let console = console_mut(session, &path);
        if let PanelTab::Result(id) = params.tab
            && !console.tabs.iter().any(|tab| tab.result == id)
        {
            return Err(CommandError::with_code("unknownResult", t!("database-error-unknown-result", id = id)));
        }
        console.active = Some(params.tab);
        console.shown = true;
        Ok(panel(session, &path))
    }
}

pub struct CancelConsole;

/// What was cancelled.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelAnswer {
    /// The query asked to stop, if one was running.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// Whether a run on several connections was stopped: the connections still waiting never run.
    pub several: bool,
}

impl Command for CancelConsole {
    type Params = ConsoleParams;
    type Output = CancelAnswer;
    const ID: &'static str = "db.operation.cancel";
    const TITLE: &'static str = "Cancel Query";
    const DESCRIPTION: &'static str = "Cancels the query a console (the active one) runs (⌘F2): its statement stops on the server too, within a second, and the rest of the script is not run; an open portal is closed, its rows stay. A run on several connections (db.script.runOnConnections) stops too: the connections still waiting never run (`several`).";

    fn run(&self, session: &mut Session, params: ConsoleParams) -> Result<CancelAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        refresh(session, &path);
        let several = super::several::cancel(session, &path);
        let running = session
            .state::<Consoles>()
            .and_then(|consoles| consoles.consoles.get(&path))
            .and_then(|console| console.running.or_else(|| console.queries.last().copied()));
        let Some(query) = running else { return Ok(CancelAnswer { query: None, several }) };
        let engine = engine(session);
        Ok(CancelAnswer { query: engine.cancel_query(query).then_some(query), several })
    }
}

/// A mark the editor draws after a statement: its time and rows, or that it failed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HintAnswer {
    /// The character offset it is drawn at.
    pub offset: usize,
    /// What it says.
    pub label: String,
    /// What it says when the pointer rests on it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tooltip: Option<String>,
    /// Whether the statement failed.
    pub error: bool,
}

/// The last query of a console, summed up for the header line.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct QuerySummary {
    /// The query's id.
    pub query: u64,
    /// Where it is.
    pub status: OperationStatus,
    /// Milliseconds it has run.
    pub elapsed_ms: u64,
    /// Its statements.
    pub statements: usize,
    /// Those that ran.
    pub executed: usize,
    /// Rows changed.
    pub updated: u64,
    /// Rows read into its results.
    pub rows: u64,
    /// Milliseconds until the server answered, summed.
    pub execution_ms: u64,
    /// Milliseconds spent reading rows, summed.
    pub fetch_ms: u64,
    /// Messages of its output.
    pub messages: usize,
    /// The first error, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The line of the statement that failed, one-based.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_line: Option<usize>,
}

/// A console as its view and agents read it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleAnswer {
    /// The console.
    pub path: PathBuf,
    /// Its connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The connection's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_name: Option<String>,
    /// The name of the connection it ran on until that connection was removed, while it has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed_connection: Option<String>,
    /// The connection's state.
    pub state: ConnectionState,
    /// The connection type's colour (`red`, `green`), for the tab's strip.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Its schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The dialect its text is split in.
    pub dialect: Dialect,
    /// The statement under the caret, in characters, for the editor to highlight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement: Option<(usize, usize)>,
    /// The characters of the statements the last run executed, until the text is edited, for the editor to flash.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ran: Option<(usize, usize)>,
    /// The marks after statements.
    pub hints: Vec<HintAnswer>,
    /// Its results panel.
    pub panel: PanelAnswer,
    /// Its result tabs.
    pub tabs: Vec<ResultTab>,
    /// Its last query.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<QuerySummary>,
    /// Whether a query runs.
    pub running: bool,
    /// Its commit mode and open transaction with its last statements, when it has a connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<zen_db::engine::TransactionState>,
    /// Its connection's safety: type, read-only, confirmations and what agents may run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety: Option<super::safety::Target>,
    /// Its last run on several connections, each connection with its outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connections: Option<super::several::SeveralRun>,
}

fn summary_of(text: Option<&str>, query: &QueryState) -> QuerySummary {
    let failed = query.statements.iter().find(|statement| statement.status == StatementStatus::Failed);
    let error = failed.and_then(|statement| statement.error.as_ref()).or(query.error.as_ref()).map(describe);
    let error_line =
        failed.zip(text).map(|(statement, text)| text.chars().take(statement.start).filter(|c| *c == '\n').count() + 1);
    QuerySummary {
        query: query.id,
        status: query.status,
        elapsed_ms: query.elapsed_ms,
        statements: query.statements.len(),
        executed: query
            .statements
            .iter()
            .filter(|statement| matches!(statement.status, StatementStatus::Done | StatementStatus::Failed))
            .count(),
        updated: query.updated,
        rows: query.statements.iter().filter_map(|statement| statement.rows).sum(),
        execution_ms: query.statements.iter().map(|statement| statement.execution_ms).sum(),
        fetch_ms: query.statements.iter().map(|statement| statement.fetch_ms).sum(),
        messages: query.messages.len(),
        error,
        error_line,
    }
}

fn marks_of(query: &QueryState, version: u64) -> Marks {
    let mut marks = Marks { version, ..Marks::default() };
    let finished = query
        .statements
        .iter()
        .filter(|statement| matches!(statement.status, StatementStatus::Done | StatementStatus::Failed));
    marks.ran = finished.fold(None, |ran, statement| match ran {
        None => Some((statement.start, statement.end)),
        Some((start, end)) => Some((start.min(statement.start), end.max(statement.end))),
    });
    for statement in &query.statements {
        match statement.status {
            StatementStatus::Done => {
                let time = t!("database-console-hint-time", ms = statement.execution_ms + statement.fetch_ms);
                let label = match (statement.rows, statement.affected) {
                    (Some(rows), _) => t!("database-console-hint-rows", time = time.as_str(), rows = rows),
                    (None, Some(changed)) => t!("database-console-hint-updated", time = time.as_str(), rows = changed),
                    (None, None) => time,
                };
                marks.hints.push(Mark {
                    start: statement.end,
                    end: statement.end,
                    text: label,
                    tooltip: None,
                    error: false,
                });
            }
            StatementStatus::Failed => {
                let Some(error) = &statement.error else { continue };
                let message = describe(error);
                let start =
                    error.position.map_or(statement.start, |position| statement.start + position.saturating_sub(1));
                let start = start.min(statement.end);
                marks.problems.push(Mark {
                    start,
                    end: (start + 1).max(start),
                    text: message.clone(),
                    tooltip: None,
                    error: true,
                });
                marks.hints.push(Mark {
                    start: statement.end,
                    end: statement.end,
                    text: t!("database-console-hint-failed"),
                    tooltip: Some(message),
                    error: true,
                });
            }
            _ => {}
        }
    }
    marks
}

fn hint_place(text: &str, end: usize, code: bool) -> usize {
    let rest: Vec<char> = text.chars().skip(end).take_while(|c| *c != '\n').collect();
    if code {
        return end + rest.iter().rposition(|c| !c.is_whitespace()).map_or(0, |last| last + 1);
    }
    let blank = rest.iter().take_while(|c| c.is_whitespace()).count();
    if rest.get(blank) == Some(&';') { end + blank + 1 } else { end }
}

fn word_end(text: &str, start: usize) -> usize {
    let length = text.chars().skip(start).take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '"').count();
    start + length.max(1)
}

pub fn refresh(session: &mut Session, path: &Path) {
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else { return };
    let Some(console) = session.state::<Consoles>().and_then(|consoles| consoles.consoles.get(path)).cloned() else {
        return;
    };
    let Some(running) = console.running else { return };
    let Some(query) = engine.query(running) else {
        console_mut(session, path).running = None;
        return;
    };
    let (text, version) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map_or((None, 0), |document| (Some(document.text().to_owned()), document.version()));
    let mut console = console;
    let mut added = Vec::new();
    let several = super::several::connection_name(session, path, query.id);
    let summary = console.active == Some(PanelTab::Connections);
    for statement in &query.statements {
        let Some(result) = statement.result else { continue };
        if console.tabs.iter().any(|tab| tab.result == result) || console.absorbed.contains(&result) {
            continue;
        }
        if !super::objects::columns_known(&engine, result) {
            continue;
        }
        console.absorbed.insert(result);
        let made = console.tabs.iter().filter(|tab| tab.query == query.id).count();
        if made >= MOST_TABS {
            engine.results().remove(result);
            continue;
        }
        let table = engine.results().get(result).and_then(|set| set.table().map(str::to_owned));
        let number = 1 + console.tabs.iter().filter(|tab| tab.query == query.id && tab.named == table).count();
        let shown = table.as_ref().map(|table| table.replace(['"', '`', '[', ']'], ""));
        let title = match shown {
            Some(table) if number == 1 => table,
            Some(table) => format!("{table} {number}"),
            None => t!("database-result-title", number = number),
        };
        let title = match &several {
            Some(name) => t!("database-result-on-connection", connection = name.as_str(), title = title.as_str()),
            None => title,
        };
        console.tabs.push(ResultTab {
            result,
            title,
            pinned: false,
            query: query.id,
            statement: statement.index,
            connection: several.as_ref().map(|_| query.connection.clone()),
            named: table,
        });
        added.push(result);
    }
    if let Some(first) = added.first()
        && !summary
        && !matches!(console.active, Some(PanelTab::Result(id)) if console.tabs.iter().any(|tab| tab.result == id && tab.query == query.id))
    {
        console.active = Some(PanelTab::Result(*first));
    }
    if query.status != OperationStatus::Running {
        console.running = None;
        let mut marks = marks_of(&query, version);
        if let Some(text) = &text {
            for problem in &mut marks.problems {
                problem.end = word_end(text, problem.start);
            }
            let code = super::injected::hosts(path);
            for hint in &mut marks.hints {
                hint.start = hint_place(text, hint.start, code);
                hint.end = hint.start;
            }
        }
        console.marks = marks;
        if !summary
            && (console.tabs.is_empty()
                || query.statements.iter().any(|statement| statement.status == StatementStatus::Failed))
        {
            let failed = query.statements.iter().any(|statement| statement.status == StatementStatus::Failed);
            if console.tabs.iter().all(|tab| tab.query != query.id) {
                console.active =
                    Some(if failed || query.messages.is_empty() { PanelTab::Log } else { PanelTab::Output });
            }
        }
    }
    session.state_mut::<Consoles>().consoles.insert(path.to_owned(), console);
}

pub fn shown_result(session: &Session, path: &Path) -> Option<u64> {
    let console = session.state::<Consoles>()?.consoles.get(path)?;
    match &console.active {
        Some(PanelTab::Result(id)) if console.tabs.iter().any(|tab| tab.result == *id) => Some(*id),
        Some(PanelTab::Output | PanelTab::Log | PanelTab::Variables) => None,
        _ => console.tabs.first().map(|tab| tab.result),
    }
}

pub fn step(session: &mut Session) {
    super::filters::step(session);
    let running: Vec<PathBuf> = session
        .state::<Consoles>()
        .map(|consoles| {
            consoles
                .consoles
                .iter()
                .filter(|(_, console)| console.running.is_some())
                .map(|(path, _)| path.clone())
                .collect()
        })
        .unwrap_or_default();
    for path in running {
        refresh(session, &path);
    }
    super::several::step(session);
}

fn current_marks(session: &Session, path: &Path) -> Marks {
    let Some(console) = session.state::<Consoles>().and_then(|consoles| consoles.consoles.get(path)) else {
        return Marks::default();
    };
    let marks = &console.marks;
    let document = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path));
    match document {
        Some(document) if document.version() != marks.version => match document.char_edits_since(marks.version) {
            Some(edits) => marks.moved(&edits),
            None => Marks::default(),
        },
        _ => marks.clone(),
    }
}

pub(crate) fn problems(session: &Session, path: &Path) -> Vec<crate::language::Problem> {
    let marks = current_marks(session, path);
    if marks.problems.is_empty() {
        return Vec::new();
    }
    let Some(document) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
    else {
        return Vec::new();
    };
    let text = document.snapshot();
    marks
        .problems
        .iter()
        .filter_map(|mark| {
            let point = text.offset_to_point(mark.start.min(text.len_chars())).ok()?;
            Some(crate::language::Problem {
                start: mark.start,
                end: mark.end.min(text.len_chars()),
                line: u32::try_from(point.line + 1).unwrap_or(u32::MAX),
                column: u32::try_from(point.column + 1).unwrap_or(u32::MAX),
                severity: crate::language::Severity::Error,
                message: mark.text.clone(),
                source: Some("database".to_owned()),
                code: None,
                note: None,
                rows: Vec::new(),
                details: None,
                fixes: Vec::new(),
                deprecated: false,
                unused: false,
            })
        })
        .collect()
}

pub struct ConsoleStateCommand;

impl Command for ConsoleStateCommand {
    type Params = ConsoleParams;
    type Output = ConsoleAnswer;
    const ID: &'static str = "db.console.state";
    const TITLE: &'static str = "Console State";
    const DESCRIPTION: &'static str = "A console (the active one) as its view shows it: its connection (name, state, type colour) and schema, the dialect, the statement under the caret, the marks after statements (time, rows, failures), the results panel (shown, maximized, the tab shown), its result tabs and its last query summed up for the header line (statements, rows changed, rows read, execution and fetch time, messages, the first error). Never rows or secrets: read rows with db.result.page.";

    fn run(&self, session: &mut Session, params: ConsoleParams) -> Result<ConsoleAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        refresh(session, &path);
        super::filters::step(session);
        let connection = connection_of(session, &path);
        let dialect = dialect_of(session, connection.as_deref());
        let mode = settings(session).blank_lines;
        let (version, selection) = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .map_or((0, (0, 0)), |document| {
                let primary = document.selections().primary();
                (document.version(), (primary.range().start, primary.range().end))
            });
        let cached = session
            .state::<Consoles>()
            .and_then(|consoles| consoles.consoles.get(&path))
            .and_then(|console| console.caret)
            .filter(|(at, from, to, _)| (*at, *from, *to) == (version, selection.0, selection.1));
        let statement = match cached {
            Some((_, _, _, statement)) => statement,
            None => {
                let found = if selection.0 != selection.1 {
                    None
                } else if super::injected::hosts(&path) {
                    super::injected::statements(session, &path, (Scope::Statement, None), dialect)
                        .ok()
                        .and_then(|statements| statements.first().map(|statement| (statement.start, statement.end)))
                } else {
                    with_split(session, &path, dialect, |script, text, _| {
                        script
                            .statement_at(text, byte_of(text, selection.1), mode)
                            .map(|statement| (char_of(text, statement.start), char_of(text, statement.end)))
                    })
                    .ok()
                    .flatten()
                };
                console_mut(session, &path).caret = Some((version, selection.0, selection.1, found));
                found
            }
        };
        let marks = current_marks(session, &path);
        let hints = marks
            .hints
            .iter()
            .map(|mark| HintAnswer {
                offset: mark.start,
                label: mark.text.clone(),
                tooltip: mark.tooltip.clone(),
                error: mark.error,
            })
            .collect();
        let console =
            session.state::<Consoles>().and_then(|consoles| consoles.consoles.get(&path)).cloned().unwrap_or_default();
        let engine = session.state::<Database>().and_then(Database::started).cloned();
        let entry = connection.as_deref().and_then(|id| find(session, id).ok());
        let color = entry.as_ref().and_then(|entry| {
            let (types, _) = connection_types(session);
            Some(types.resolve(&entry.source.connection_type).color.clone()).filter(|color| color != "none")
        });
        let state = match (&engine, &connection) {
            (Some(engine), Some(id)) => engine.status(id).state,
            _ => ConnectionState::Disconnected,
        };
        let last = console.queries.last().and_then(|query| engine.as_ref().and_then(|engine| engine.query(*query)));
        let transaction =
            entry.as_ref().map(|entry| super::transactions::state_of(session, entry, &context_of(&path), 10));
        let text = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .map(|document| document.text().to_owned());
        let safety = entry.as_ref().map(|entry| super::safety::target(session, entry));
        let connections = super::several::answer(session, &path);
        let removed_connection =
            connection.is_none().then(|| tab_data(session, &path)["removed"].as_str().map(str::to_owned)).flatten();
        Ok(ConsoleAnswer {
            removed_connection,
            safety,
            connections,
            schema: schema_of(session, &path),
            connection_name: entry.map(|entry| entry.source.name),
            color,
            state,
            dialect,
            statement,
            ran: marks.ran,
            hints,
            panel: PanelAnswer { shown: console.shown, maximized: console.maximized, active: console.active.clone() },
            tabs: console.tabs.clone(),
            running: console.running.is_some(),
            transaction,
            last: last.map(|query| summary_of(text.as_deref(), &query)),
            connection,
            path,
        })
    }
}

/// What `db.settings.update` changes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsParams {
    /// When a blank line ends a statement: `always`, `smart` or `never`.
    pub blank_lines: Option<BlankLines>,
    /// The rows of the first page and of each Fetch Next.
    pub page: Option<usize>,
    /// The rows read before a portal outside the person's transaction is closed.
    pub row_cap: Option<usize>,
    /// Keep portals open outside the person's transaction on connections that are not Production.
    pub keep_open: Option<bool>,
    /// The memory all results may take, in MiB.
    pub budget_mb: Option<usize>,
    /// Whether a script stops at its first error.
    pub stop_on_error: Option<bool>,
    /// The view results open in.
    pub view: Option<ResultView>,
    /// Whether the value panel shows beside the grid.
    pub value_panel: Option<bool>,
    /// The format the value panel prefers (`text`, `json`, `xml`, `hex`, `image`); `null` for each value's own.
    #[serde(deserialize_with = "given")]
    pub value_format: Option<Option<zen_db::present::ValueFormat>>,
}

fn given<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

pub struct GetSettings;

impl Command for GetSettings {
    type Params = zen_core::NoParams;
    type Output = DbSettings;
    const ID: &'static str = "db.settings.get";
    const TITLE: &'static str = "Database Settings";
    const DESCRIPTION: &'static str = "The Database settings: when blank lines end a statement (`blankLines`), paging (`page`, `rowCap`, `keepOpen`), the memory budget for results in MiB, whether scripts stop at the first error, the view results open in, and the value panel: whether it shows (`valuePanel`) and the format it prefers (`valueFormat`).";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<DbSettings, CommandError> {
        Ok(settings(session))
    }
}

pub struct UpdateSettings;

impl Command for UpdateSettings {
    type Params = SettingsParams;
    type Output = DbSettings;
    const ID: &'static str = "db.settings.update";
    const TITLE: &'static str = "Update Database Settings";
    const DESCRIPTION: &'static str = "Changes the Database settings given (absent ones stay) and keeps them in the data folder (`databases/settings.json`): `blankLines`, `page` (1–10 000), `rowCap` (at least `page`), `keepOpen`, `budgetMb` (16–65 536), `stopOnError`, `view`, `valuePanel` and `valueFormat` (`null`: each value's own).";

    fn run(&self, session: &mut Session, params: SettingsParams) -> Result<DbSettings, CommandError> {
        let mut changed = settings(session);
        if let Some(mode) = params.blank_lines {
            changed.blank_lines = mode;
        }
        if let Some(page) = params.page {
            changed.paging.page = page.clamp(1, 10_000);
        }
        if let Some(cap) = params.row_cap {
            changed.paging.row_cap = cap;
        }
        changed.paging.row_cap = changed.paging.row_cap.max(changed.paging.page);
        if let Some(keep) = params.keep_open {
            changed.paging.keep_open = keep;
        }
        if let Some(budget) = params.budget_mb {
            changed.budget_mb = budget.clamp(16, 65_536);
        }
        if let Some(stop) = params.stop_on_error {
            changed.stop_on_error = stop;
        }
        if let Some(view) = params.view {
            changed.view = view;
        }
        if let Some(shown) = params.value_panel {
            changed.value_panel = shown;
        }
        if let Some(format) = params.value_format {
            changed.value_format = format;
        }
        if let Some(file) = settings_file(session) {
            let write = file
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&file, serde_json::to_string_pretty(&changed).unwrap_or_default()));
            write.map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
        }
        session.state_mut::<Consoles>().settings = Some(changed.clone());
        if let Some(engine) = session.state::<Database>().and_then(Database::started) {
            engine.results().set_paging(changed.paging.clone());
            engine.results().budget().set_limit(changed.budget_mb.saturating_mul(1024 * 1024));
        }
        Ok(changed)
    }
}

pub(super) fn with_tab<T>(
    session: &mut Session,
    result: u64,
    change: impl FnOnce(&mut Vec<ResultTab>, &mut Option<PanelTab>) -> T,
) -> Option<T> {
    let consoles = session.state_mut::<Consoles>();
    let console = consoles.consoles.values_mut().find(|console| console.tabs.iter().any(|tab| tab.result == result))?;
    Some(change(&mut console.tabs, &mut console.active))
}

pub(super) fn resolve(session: &Session, path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    console_path(session, path)
}

/// Which output to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OutputParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// The most messages and log entries to answer, the latest; 200 when absent.
    pub limit: Option<usize>,
}

/// A statement of the execution log.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// The query.
    pub query: u64,
    /// The statement's index in it.
    pub index: usize,
    /// Its first line.
    pub summary: String,
    /// Where it is.
    pub status: StatementStatus,
    /// Milliseconds until the server answered.
    pub execution_ms: u64,
    /// Milliseconds reading rows.
    pub fetch_ms: u64,
    /// Rows read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    /// Rows changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// Why it failed, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A message of the output.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputMessage {
    /// The query.
    pub query: u64,
    /// The message.
    #[serde(flatten)]
    pub message: Message,
}

/// A console's output and execution log.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputAnswer {
    /// What the server said, oldest first.
    pub messages: Vec<OutputMessage>,
    /// Every statement run, oldest first.
    pub log: Vec<LogEntry>,
    /// Whether older ones were left out because of `limit`.
    pub truncated: bool,
}

pub struct ConsoleOutput;

impl Command for ConsoleOutput {
    type Params = OutputParams;
    type Output = OutputAnswer;
    const ID: &'static str = "db.console.output";
    const TITLE: &'static str = "Console Output and Execution Log";
    const DESCRIPTION: &'static str = "What a console's queries got (the active one): the server's messages (NOTICE, RAISE, WARNING, PRINT, and the IDE's own such as a rollback after an error) and the execution log, every statement run with its status, times, rows and error; the latest `limit` (200) of each.";

    fn run(&self, session: &mut Session, params: OutputParams) -> Result<OutputAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        refresh(session, &path);
        let limit = params.limit.unwrap_or(200);
        let queries = session
            .state::<Consoles>()
            .and_then(|consoles| consoles.consoles.get(&path))
            .map(|console| console.queries.clone())
            .unwrap_or_default();
        let engine = session.state::<Database>().and_then(Database::started).cloned();
        let mut messages = Vec::new();
        let mut log = Vec::new();
        for query in queries.iter().filter_map(|query| engine.as_ref().and_then(|engine| engine.query(*query))) {
            messages.extend(
                query.messages.iter().map(|message| OutputMessage { query: query.id, message: message.clone() }),
            );
            log.extend(query.statements.iter().filter(|statement| statement.status != StatementStatus::Pending).map(
                |statement| LogEntry {
                    query: query.id,
                    index: statement.index,
                    summary: statement.summary.clone(),
                    status: statement.status,
                    execution_ms: statement.execution_ms,
                    fetch_ms: statement.fetch_ms,
                    rows: statement.rows,
                    affected: statement.affected,
                    error: statement.error.as_ref().map(describe),
                },
            ));
        }
        let truncated = messages.len() > limit || log.len() > limit;
        let messages = messages.split_off(messages.len().saturating_sub(limit));
        let log = log.split_off(log.len().saturating_sub(limit));
        Ok(OutputAnswer { messages, log, truncated })
    }
}

pub fn rerun(session: &mut Session, result: u64, confirmation: &Confirmation) -> Result<RunStarted, CommandError> {
    let unknown = || CommandError::with_code("unknownResult", t!("database-error-unknown-result", id = result));
    let path = session
        .state::<Consoles>()
        .and_then(|consoles| consoles.console_of(result))
        .map(Path::to_owned)
        .ok_or_else(unknown)?;
    let tab = session
        .state::<Consoles>()
        .and_then(|consoles| consoles.tabs(&path).iter().find(|tab| tab.result == result).cloned())
        .ok_or_else(unknown)?;
    let engine = engine(session);
    let set = engine.results().get(result).ok_or_else(unknown)?;
    let (start, end) = engine
        .query(tab.query)
        .and_then(|query| query.statements.get(tab.statement).map(|statement| (statement.start, statement.end)))
        .unwrap_or((0, 0));
    let connection = set.connection().to_owned();
    let sql = super::variables::target(session, &connection).sql;
    let params = super::variables::positional(set.statement_params(), sql);
    let statement =
        QueryStatement { text: set.statement_text().to_owned(), start, end, client: false, params, kept: None };
    let prepared = super::variables::Prepared { statements: vec![statement], ..super::variables::Prepared::default() };
    let retry = ("db.result.refresh", serde_json::json!({ "id": result }));
    if tab.pinned {
        return launch(session, &path, (connection, prepared), (true, Source::Statement), (confirmation, retry));
    }
    refuse_replacing(session, &path, retry.clone())?;
    launch(session, &path, (connection, prepared), (false, Source::Statement), (confirmation, retry))
}

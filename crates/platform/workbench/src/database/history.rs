use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::engine::Engine;
use zen_db::history::{Entry, Journal, Outcome, Provenance, Query, Settings, Source};
use zen_i18n::t;

use super::connections::describe;
use super::consoles::{ExecuteScript, RunStarted, ScriptParams};
use super::sources::files;
use super::{NewConsole, NewConsoleParams, WORKSPACE};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};

pub const TAB: &str = "history";

const LISTED: usize = 50;

const MOST_LISTED: usize = 500;

const TEXT_SHOWN: usize = 500;

const CLEAR_WAIT: Duration = Duration::from_secs(2);

pub fn journal(session: &Session) -> Option<Arc<Journal>> {
    let data = session.state::<Workspaces>().and_then(Workspaces::data_dir)?;
    Some(Journal::shared(&data.join("databases")))
}

pub fn attach(session: &Session, engine: &Engine) {
    if engine.has_history() {
        return;
    }
    if let Some(journal) = journal(session) {
        engine.set_history(Some(journal));
    }
}

pub fn provenance(session: &Session, source: Source, place: Option<String>) -> Provenance {
    Provenance { source, agent: zen_core::macros::by_agent(session), place, confirmed: false }
}

fn needed(session: &Session) -> Result<Arc<Journal>, CommandError> {
    journal(session).ok_or_else(|| CommandError::new(t!("database-error-no-data-folder")))
}

/// A time range of the history, counted back from now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Period {
    /// Since the last local midnight.
    Today,
    /// The last 7 days.
    Week,
    /// The last 30 days.
    Month,
}

impl Period {
    pub fn since(self, now: i64) -> i64 {
        const DAY: i64 = 86_400_000;
        match self {
            Period::Today => zen_db::history::local_midnight(now),
            Period::Week => now - 7 * DAY,
            Period::Month => now - 30 * DAY,
        }
    }
}

/// Which statements `db.history.list` answers.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// Words that must all be in the statement or its error, in any case.
    pub query: Option<String>,
    /// Only this connection's statements.
    pub connection: Option<String>,
    /// Only statements from these sources: `statement`, `script`, `filter`, `data`, `save`, `transaction`, `query`.
    pub sources: Vec<Source>,
    /// Only statements agents asked for (`true`) or people did (`false`).
    pub agent: Option<bool>,
    /// Only statements that ended so: `done`, `failed`, `cancelled`, `refused`.
    pub outcome: Option<Outcome>,
    /// Only statements of `today`, the last `week` or the last `month`.
    pub period: Option<Period>,
    /// Only statements started at or after this time, in milliseconds since 1970.
    pub since: Option<i64>,
    /// Only statements started before this time, in milliseconds since 1970.
    pub until: Option<i64>,
    /// How many matching statements to skip, newest first.
    pub offset: usize,
    /// The most statements to answer; 50 when absent, at most 500.
    pub limit: Option<usize>,
    /// The most characters of each statement to answer; 500 when absent.
    pub text_limit: Option<usize>,
}

/// A statement of the history as answered.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryItem {
    /// Its id, for `db.history.get` and `db.history.openInConsole`.
    pub id: u64,
    /// When it started, in milliseconds since 1970.
    pub at: i64,
    /// When it started, in the computer's time zone: `2026-10-27 14:05:09`.
    pub time: String,
    /// The connection's id.
    pub connection: String,
    /// The connection's name, while it exists.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_name: Option<String>,
    /// The context it ran in: `main`, `console:<name>`.
    pub context: String,
    /// The schema it ran in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The statement, passwords masked, cut to `textLimit` characters.
    pub text: String,
    /// Whether the statement is longer than answered here (`db.history.get` answers it whole) or was cut when kept.
    pub text_cut: bool,
    /// Whether a password in it was masked.
    pub redacted: bool,
    /// Milliseconds it ran.
    pub duration_ms: u64,
    /// The rows it returned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<u64>,
    /// The rows it changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affected: Option<u64>,
    /// How it ended.
    pub outcome: Outcome,
    /// Why it failed, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The server's code of the error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// What made it run.
    pub source: Source,
    /// Whether an agent asked for it.
    pub agent: bool,
    /// Whether it ran after a confirmation its connection type or its risk asked for.
    pub confirmed: bool,
    /// Where it was run from: a console's file name, an object's name, an agent's session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// Its bind parameters: `:name`, or `:name = value` when the history keeps values (secret-looking ones masked).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<String>,
}

fn names(session: &Session) -> HashMap<String, String> {
    files(session)
        .entries()
        .map(|entries| entries.into_iter().map(|entry| (entry.source.id, entry.source.name)).collect())
        .unwrap_or_default()
}

fn item(entry: Entry, names: &HashMap<String, String>, text_limit: usize) -> HistoryItem {
    let (text, shortened) = match entry.text.char_indices().nth(text_limit) {
        Some((end, _)) => (entry.text[..end].to_owned(), true),
        None => (entry.text.clone(), false),
    };
    HistoryItem {
        id: entry.id,
        at: entry.at,
        time: zen_db::history::local_time(entry.at),
        connection_name: names.get(&entry.connection).cloned(),
        connection: entry.connection,
        context: entry.context.to_string(),
        schema: entry.schema,
        text,
        text_cut: shortened || entry.cut,
        redacted: entry.redacted,
        duration_ms: entry.duration_ms,
        rows: entry.rows,
        affected: entry.affected,
        outcome: entry.outcome,
        error: entry.error.as_ref().map(describe),
        code: entry.error.and_then(|error| error.code),
        source: entry.source,
        agent: entry.agent,
        confirmed: entry.confirmed,
        place: entry.place,
        parameters: entry.parameters,
    }
}

/// Statements of the history.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryList {
    /// The statements, newest first.
    pub entries: Vec<HistoryItem>,
    /// How many statements match.
    pub total: usize,
    /// Whether matching statements were left out after these.
    pub truncated: bool,
    /// How many statements the history keeps.
    pub kept: usize,
    /// Whether statements are recorded.
    pub enabled: bool,
    /// Whether the history is still being read from disk.
    pub loading: bool,
    /// Grows with every change of the history.
    pub revision: u64,
    /// Why the history could not be read or written, when that happened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub struct ListHistory;

impl Command for ListHistory {
    type Params = ListParams;
    type Output = HistoryList;
    const ID: &'static str = "db.history.list";
    const TITLE: &'static str = "Query History";
    const DESCRIPTION: &'static str = "Lists the query history, newest first: every statement run from consoles (`statement`, `script`), filters and sorts (`filter`), object tabs' Data pages (`data`), saved data edits (`save`), Commit and Rollback (`transaction`) and db.query.run (`query`), with its time, connection, schema, duration, rows, outcome and error, and `agent` when an agent asked for it. `query` finds statements holding all its words (any case, in the text or the error); `connection`, `sources`, `agent`, `outcome`, `period` (`today`, `week`, `month`), `since` and `until` (milliseconds since 1970) narrow it. Answers at most `limit` (50, ≤ 500) from `offset`, each text cut to `textLimit` (500) characters, with `total` and `truncated`. Passwords written in statements are masked and values of bind parameters are never kept.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<HistoryList, CommandError> {
        let journal = needed(session)?;
        let now = zen_db::history::now();
        let since = match (params.since, params.period.map(|period| period.since(now))) {
            (Some(given), Some(period)) => Some(given.max(period)),
            (given, period) => given.or(period),
        };
        let mut connections: Vec<String> = params.connection.into_iter().collect();
        if let Some(allowed) = agent_connections(session) {
            connections = if connections.is_empty() {
                allowed
            } else {
                connections.into_iter().filter(|connection| allowed.contains(connection)).collect()
            };
            if connections.is_empty() {
                connections.push(String::new());
            }
        }
        let query = Query {
            text: params.query.filter(|text| !text.trim().is_empty()),
            connections,
            sources: params.sources,
            agent: params.agent,
            outcome: params.outcome,
            since,
            until: params.until,
            offset: params.offset,
            limit: params.limit.unwrap_or(LISTED).min(MOST_LISTED),
        };
        let found = journal.search(&query);
        let names = names(session);
        let text_limit = params.text_limit.unwrap_or(TEXT_SHOWN).max(1);
        Ok(HistoryList {
            entries: found.entries.into_iter().map(|entry| item(entry, &names, text_limit)).collect(),
            total: found.total,
            truncated: found.truncated,
            kept: found.kept,
            enabled: journal.settings().enabled,
            loading: !found.loaded,
            revision: found.revision,
            error: journal.error(),
        })
    }
}

/// Which statement of the history.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryParams {
    /// The statement's id, from `db.history.list`.
    pub id: u64,
}

fn entry(session: &Session, id: u64) -> Result<Entry, CommandError> {
    let found = needed(session)?
        .get(id)
        .ok_or_else(|| CommandError::with_code("unknownEntry", t!("database-error-unknown-history", id = id)))?;
    match agent_connections(session) {
        Some(allowed) if !allowed.contains(&found.connection) => {
            Err(CommandError::with_code("unknownEntry", t!("database-error-unknown-history", id = id)))
        }
        _ => Ok(found),
    }
}

fn agent_connections(session: &Session) -> Option<Vec<String>> {
    if !zen_core::macros::by_agent(session) {
        return None;
    }
    let entries = files(session).entries().unwrap_or_default();
    Some(
        entries
            .into_iter()
            .filter(|entry| super::safety::agent_access(session, entry) != zen_db::types::AgentAccess::None)
            .map(|entry| entry.source.id)
            .collect(),
    )
}

pub struct GetHistory;

impl Command for GetHistory {
    type Params = EntryParams;
    type Output = HistoryItem;
    const ID: &'static str = "db.history.get";
    const TITLE: &'static str = "Query History Entry";
    const DESCRIPTION: &'static str = "One statement of the query history by `id`, with its whole text (as kept: passwords masked, at most 64 KiB; `textCut` when it was longer).";

    fn run(&self, session: &mut Session, params: EntryParams) -> Result<HistoryItem, CommandError> {
        let found = entry(session, params.id)?;
        Ok(item(found, &names(session), usize::MAX))
    }
}

/// What `db.history.clear` removes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ClearParams {
    /// Only this connection's statements; all when absent.
    pub connection: Option<String>,
    /// Only statements started before this time, in milliseconds since 1970; all when absent.
    pub before: Option<i64>,
}

/// What the history keeps.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistorySize {
    /// How many statements it keeps.
    pub kept: usize,
    /// The bytes of their text.
    pub bytes: usize,
    /// Its settings.
    pub settings: Settings,
    /// Where it is kept.
    pub file: PathBuf,
}

fn size(journal: &Journal) -> HistorySize {
    let (kept, bytes) = journal.size();
    HistorySize { kept, bytes, settings: journal.settings(), file: journal.folder().join(zen_db::history::JOURNAL) }
}

pub struct ClearHistory;

impl Command for ClearHistory {
    type Params = ClearParams;
    type Output = HistorySize;
    const ID: &'static str = "db.history.clear";
    const TITLE: &'static str = "Clear Query History";
    const DESCRIPTION: &'static str = "Removes statements from the query history: all of them, or only `connection`'s, or only those started before `before` (milliseconds since 1970). The journal is written again without them. Answers what the history keeps after.";

    fn run(&self, session: &mut Session, params: ClearParams) -> Result<HistorySize, CommandError> {
        let journal = needed(session)?;
        journal.clear(params.connection, params.before);
        journal.flush(CLEAR_WAIT);
        Ok(size(&journal))
    }
}

/// What `db.history.settings` changes; absent ones stay.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsParams {
    /// Whether statements are recorded.
    pub enabled: Option<bool>,
    /// Days a statement is kept, 1–3650.
    pub retention_days: Option<u32>,
    /// The most statements kept, 100–100 000.
    pub max_entries: Option<usize>,
    /// Whether values of bind parameters are kept with their names (secret-looking ones masked).
    pub parameter_values: Option<bool>,
}

pub struct HistorySettings;

impl Command for HistorySettings {
    type Params = SettingsParams;
    type Output = HistorySize;
    const ID: &'static str = "db.history.settings";
    const TITLE: &'static str = "Query History Settings";
    const DESCRIPTION: &'static str = "Reads the query history's settings, and changes the ones given: `enabled` (the off switch: nothing is recorded while off; what is kept stays), `retentionDays` (90 by default, 1–3650), `maxEntries` (10 000, 100–100 000) and `parameterValues` (off: statements keep the names of their bind parameters only; on: `:name = value` too, secret-looking names such as `password` or `token` always masked). Kept in `<data folder>/databases/history-settings.json`; older statements are removed in the background. Answers the settings, how many statements are kept and the journal's file.";

    fn run(&self, session: &mut Session, params: SettingsParams) -> Result<HistorySize, CommandError> {
        let journal = needed(session)?;
        if params.enabled.is_some()
            || params.retention_days.is_some()
            || params.max_entries.is_some()
            || params.parameter_values.is_some()
        {
            let mut settings = journal.settings();
            if let Some(enabled) = params.enabled {
                settings.enabled = enabled;
            }
            if let Some(days) = params.retention_days {
                settings.retention_days = days;
            }
            if let Some(most) = params.max_entries {
                settings.max_entries = most;
            }
            if let Some(values) = params.parameter_values {
                settings.parameter_values = values;
            }
            journal.set_settings(settings);
        }
        Ok(size(&journal))
    }
}

/// What the History tab opens with.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenParams {
    /// The search to show.
    pub query: Option<String>,
    /// The connection to show the statements of.
    pub connection: Option<String>,
}

pub struct OpenHistory;

impl Command for OpenHistory {
    type Params = OpenParams;
    type Output = InnerTab;
    const ID: &'static str = "db.history.open";
    const TITLE: &'static str = "Open Query History";
    const DESCRIPTION: &'static str = "Opens the Query History tab of the Database workspace (or brings it to front) and expands the workspace; `query` and `connection` set its search and connection filter. Read what it lists with db.history.list. Connects nowhere.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<InnerTab, CommandError> {
        let existing = session
            .state::<Workspaces>()
            .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|tab| tab.id == TAB).cloned());
        let mut data =
            existing.map(|tab| tab.data).filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
        if let Some(query) = params.query {
            data["query"] = serde_json::Value::String(query);
        }
        if let Some(connection) = params.connection {
            data["connection"] = serde_json::Value::String(connection);
        }
        let open = OpenTabParams {
            workspace: WORKSPACE.to_owned(),
            id: Some(TAB.to_owned()),
            kind: Some(TAB.to_owned()),
            title: Some(t!("database-history-title")),
            data: Some(data),
            ..OpenTabParams::default()
        };
        OpenTab.run(session, open)
    }
}

/// Which statement to open in a console.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ToConsoleParams {
    /// The statement's id, from `db.history.list`.
    pub id: u64,
    /// Run it in the new console at once (Execute Script).
    pub run: bool,
}

/// A statement of the history opened in a console.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenedInConsole {
    /// The new console's file.
    pub path: PathBuf,
    /// The connection it belongs to.
    pub connection: String,
    /// The run started, when `run` was asked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunStarted>,
}

pub struct OpenInConsole;

impl Command for OpenInConsole {
    type Params = ToConsoleParams;
    type Output = OpenedInConsole;
    const ID: &'static str = "db.history.openInConsole";
    const TITLE: &'static str = "Open in Console";
    const DESCRIPTION: &'static str = "Opens the statement `id` of the query history in a new console of its connection (db.console.new) and, with `run`, runs it there at once (Execute Script, connecting first when needed). A statement that was cut when kept, or whose password was masked, is refused for `run` with code `notWhole`; one whose connection is no longer in the project with code `unknownConnection`, before any console is made.";

    fn run(&self, session: &mut Session, params: ToConsoleParams) -> Result<OpenedInConsole, CommandError> {
        let found = entry(session, params.id)?;
        if params.run && (found.cut || found.redacted) {
            return Err(CommandError::with_code("notWhole", t!("database-error-history-not-whole")));
        }
        if params.run {
            super::sources::find(session, &found.connection)?;
        }
        let opened = NewConsole.run(
            session,
            NewConsoleParams { connection: Some(found.connection.clone()), text: Some(found.text.clone()) },
        )?;
        let run = if params.run {
            Some(
                ExecuteScript
                    .run(session, ScriptParams { path: Some(opened.path.clone()), ..ScriptParams::default() })?,
            )
        } else {
            None
        };
        Ok(OpenedInConsole { path: opened.path, connection: opened.connection, run })
    }
}

#[cfg(test)]
mod tests;

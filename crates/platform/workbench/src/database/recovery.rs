use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::driver::ContextKind;
use zen_db::recovery::{ConsoleText, Item, OpenTransaction, Recovery, StagedEdits, dialect_name};
use zen_i18n::t;

use super::sources::{Database, find};
use super::transactions::{context_name, state_of};
use super::{NewConsole, NewConsoleParams, WORKSPACE};
use crate::Workbench;
use crate::notifications::{Action, Group, Kind, NewNotification, post};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};

pub const TAB: &str = "recovery";

const LISTED: usize = 50;

const MOST_LISTED: usize = 500;

const TEXT_SHOWN: usize = 2000;

const LOGGED: usize = 20;

const FINISH_WAIT: Duration = Duration::from_secs(2);

pub struct Store(pub Arc<Recovery>);

pub fn store(session: &Session) -> Option<Arc<Recovery>> {
    if let Some(Store(own)) = session.state::<Store>() {
        return Some(Arc::clone(own));
    }
    let data = session.state::<Workspaces>().and_then(Workspaces::data_dir)?;
    Some(Recovery::shared(&data.join("databases")))
}

struct Tracked {
    token: String,
    written: HashMap<String, u64>,
    offered: bool,
}

impl Default for Tracked {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        Self { token, written: HashMap::new(), offered: false }
    }
}

fn now() -> i64 {
    zen_db::history::now()
}

fn installed(session: &Session) -> bool {
    session.state::<Workspaces>().is_some_and(|workspaces| workspaces.info(WORKSPACE).is_some())
}

pub fn capture(session: &mut Session) -> bool {
    if !installed(session) {
        return false;
    }
    let Some(recovery) = store(session) else { return false };
    let waiting = !session.state::<Tracked>().is_some_and(|tracked| tracked.offered);
    if waiting {
        if !recovery.loaded() {
            return true;
        }
        offer(session, &recovery);
    }
    let mut seen: HashMap<String, u64> = HashMap::new();
    let mut puts: Vec<(String, Item)> = Vec::new();
    let token = session.state_mut::<Tracked>().token.clone();
    let written = session.state::<Tracked>().map(|tracked| tracked.written.clone()).unwrap_or_default();
    let fresh = |key: &str, fingerprint: u64| written.get(key) != Some(&fingerprint);
    for (path, version) in crate::workspaces::dirty_console_versions(session) {
        let key = console_key(&path);
        if fresh(&key, version) {
            let text = session
                .state::<Workbench>()
                .and_then(|workbench| workbench.documents.iter().find(|document| document.path == path))
                .map(|document| document.text().to_owned());
            if let Some(text) = text {
                puts.push((key.clone(), Item::Console(ConsoleText { path: path.clone(), text, at: now() })));
            }
        }
        seen.insert(key, version);
    }
    let results = session
        .state::<Database>()
        .and_then(Database::started)
        .map(|engine| engine.results().all())
        .unwrap_or_default();
    for result in results.into_iter().filter(|result| result.pending().any()) {
        let key = format!("edits:{token}:{}", result.id());
        let revision = result.revision();
        if fresh(&key, revision)
            && let Some(item) = staged(session, &result)
        {
            puts.push((key.clone(), item));
        }
        seen.insert(key, revision);
    }
    let pending = session.state::<Database>().and_then(Database::started).map(|engine| engine.pending());
    for open in pending.unwrap_or_default().into_iter().filter(|state| state.in_transaction) {
        let key = format!("transaction:{token}:{}:{}", open.connection, open.context);
        let fingerprint = (open.statements as u64) << 32 | open.modifying as u64;
        if fresh(&key, fingerprint)
            && let Some(item) = transaction(session, &open.connection, &open.context)
        {
            puts.push((key.clone(), item));
        }
        seen.insert(key, fingerprint);
    }
    let gone: Vec<String> = written.keys().filter(|key| !seen.contains_key(*key)).cloned().collect();
    if puts.is_empty() && gone.is_empty() {
        return false;
    }
    for (key, item) in puts {
        recovery.put(&key, item);
    }
    for key in &gone {
        recovery.remove(key);
    }
    session.state_mut::<Tracked>().written = seen;
    false
}

pub fn finish(session: &mut Session) {
    let Some(recovery) = store(session) else { return };
    let Some(tracked) = session.state::<Tracked>() else { return };
    let ended: Vec<String> = tracked.written.keys().filter(|key| key.starts_with("transaction:")).cloned().collect();
    for key in &ended {
        recovery.remove(key);
    }
    session.state_mut::<Tracked>().written.retain(|key, _| !key.starts_with("transaction:"));
    recovery.flush(FINISH_WAIT);
}

fn console_key(path: &Path) -> String {
    format!("console:{}", path.display())
}

fn staged(session: &mut Session, result: &zen_db::results::ResultSet) -> Option<Item> {
    let entry = find(session, result.connection()).ok()?;
    let known = super::edits::known(session, result);
    if known.editing.table.is_none() || known.editing.key.is_none() {
        return None;
    }
    let place = match result.context() {
        ContextKind::Console(_) => Some(context_name(result.context())),
        _ => known.editing.table.clone(),
    };
    Some(Item::Edits(Box::new(StagedEdits {
        connection: entry.source.id.clone(),
        connection_name: entry.source.name.clone(),
        dialect: dialect_name(known.editing.dialect).to_owned(),
        changes: result.changes(),
        editing: known.editing,
        statement: zen_db::history::redact(result.statement_text()).0,
        place,
        at: now(),
    })))
}

fn transaction(session: &mut Session, connection: &str, context: &ContextKind) -> Option<Item> {
    let entry = find(session, connection).ok()?;
    let state = state_of(session, &entry, context, LOGGED);
    Some(Item::Transaction(OpenTransaction {
        connection: entry.source.id.clone(),
        connection_name: entry.source.name.clone(),
        context: context.clone(),
        statements: state.statements,
        modifying: state.modifying,
        log: state.log.iter().map(|logged| zen_db::history::redact(&logged.summary).0).collect(),
        at: now(),
    }))
}

fn offer(session: &mut Session, recovery: &Recovery) {
    session.state_mut::<Tracked>().offered = true;
    for (id, key, item) in recovery.recovered() {
        let Item::Console(console) = &*item else { continue };
        let version =
            session.state_mut::<Workbench>().documents.iter_mut().find(|document| document.path == console.path).map(
                |document| {
                    if document.text() != console.text {
                        let whole = 0..document.snapshot().len_chars();
                        document.replace_ranges(vec![(whole, console.text.clone())]);
                    }
                    (document.version(), document.is_dirty())
                },
            );
        let Some((version, dirty)) = version else { continue };
        recovery.adopt(id);
        if dirty {
            session.state_mut::<Tracked>().written.insert(key, version);
            post(
                session,
                NewNotification::worded(
                    Group::General,
                    Kind::Info,
                    zen_i18n::Message::new("database-recovery-console-restored").with("name", file_name(&console.path)),
                )
                .titled_by(zen_i18n::Message::new("database-recovery-title")),
            );
        } else {
            recovery.remove(&key);
        }
        session.state_mut::<Workbench>().revision += 1;
    }
    if recovery.announce() {
        let count = recovery.recovered().len();
        post(
            session,
            NewNotification::worded(
                Group::General,
                Kind::Warning,
                zen_i18n::Message::new("database-recovery-found").with_number("count", count as f64),
            )
            .titled_by(zen_i18n::Message::new("database-recovery-title"))
            .action(Action::new(
                "review",
                t!("database-recovery-review"),
                OpenRecovery::ID,
                serde_json::json!({}),
            )),
        );
    }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

/// What a recovered item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum RecoveredKind {
    /// A console's text that was not saved.
    Console,
    /// Data edits that were staged and not saved.
    Edits,
    /// A transaction that was open; the server rolled it back.
    Transaction,
}

/// A recovered item as answered.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredItem {
    /// Its id, for `db.recovery.get`, `.restore` and `.discard`.
    pub id: u64,
    /// What it is.
    pub kind: RecoveredKind,
    /// When it was kept last, in milliseconds since 1970.
    pub at: i64,
    /// When it was kept last, in the computer's time zone.
    pub time: String,
    /// What it is about, in words: a console's file name, a table, a connection's context.
    pub title: String,
    /// The connection's id, for edits and transactions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The connection's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_name: Option<String>,
    /// The console's file, for a console.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// Rows that change, for edits; statements run, for a transaction.
    pub count: usize,
    /// Of a transaction's statements, those that may have changed data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifying: Option<usize>,
}

fn item(id: u64, recovered: &Item) -> RecoveredItem {
    let base = |kind, at| RecoveredItem {
        id,
        kind,
        at,
        time: zen_db::history::local_time(at),
        title: String::new(),
        connection: None,
        connection_name: None,
        path: None,
        count: 0,
        modifying: None,
    };
    match recovered {
        Item::Console(console) => RecoveredItem {
            title: file_name(&console.path),
            path: Some(console.path.clone()),
            count: console.text.lines().count(),
            ..base(RecoveredKind::Console, console.at)
        },
        Item::Edits(edits) => RecoveredItem {
            title: edits.editing.table.clone().unwrap_or_default(),
            connection: Some(edits.connection.clone()),
            connection_name: Some(edits.connection_name.clone()),
            count: edits.changes.rows(),
            ..base(RecoveredKind::Edits, edits.at)
        },
        Item::Transaction(open) => RecoveredItem {
            title: context_name(&open.context),
            connection: Some(open.connection.clone()),
            connection_name: Some(open.connection_name.clone()),
            count: open.statements,
            modifying: Some(open.modifying),
            ..base(RecoveredKind::Transaction, open.at)
        },
    }
}

fn needed(session: &Session) -> Result<Arc<Recovery>, CommandError> {
    store(session).ok_or_else(|| CommandError::new(t!("database-error-no-data-folder")))
}

fn recovered(session: &Session, id: u64) -> Result<(Arc<Recovery>, String, Arc<Item>), CommandError> {
    let recovery = needed(session)?;
    let (key, item) = recovery
        .recovered_item(id)
        .ok_or_else(|| CommandError::with_code("unknownItem", t!("database-error-unknown-recovered", id = id)))?;
    Ok((recovery, key, item))
}

/// Which recovered items `db.recovery.list` answers.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// The most items to answer; 50 when absent, at most 500.
    pub limit: Option<usize>,
}

/// The recovered items.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredList {
    /// The items, newest first.
    pub items: Vec<RecoveredItem>,
    /// How many items there are.
    pub total: usize,
    /// Whether items were left out because of `limit`.
    pub truncated: bool,
    /// Whether the journal an earlier run left is still being read.
    pub loading: bool,
    /// Grows with every change.
    pub revision: u64,
    /// The journal's file.
    pub file: PathBuf,
    /// Why the journal could not be read or written, when that happened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub fn list(recovery: &Recovery, limit: usize) -> RecoveredList {
    let mut items: Vec<RecoveredItem> = recovery.recovered().iter().map(|(id, _, found)| item(*id, found)).collect();
    items.sort_by(|a, b| b.at.cmp(&a.at).then(b.id.cmp(&a.id)));
    let total = items.len();
    items.truncate(limit);
    RecoveredList {
        truncated: total > items.len(),
        items,
        total,
        loading: !recovery.loaded(),
        revision: recovery.revision(),
        file: recovery.file(),
        error: recovery.error(),
    }
}

pub struct ListRecovered;

impl Command for ListRecovered {
    type Params = ListParams;
    type Output = RecoveredList;
    const ID: &'static str = "db.recovery.list";
    const TITLE: &'static str = "Recovered After a Crash";
    const DESCRIPTION: &'static str = "Lists what an earlier run of the IDE kept and left behind when it stopped without closing (a crash, a kill, a power loss), newest first: `console` text that was not saved (put back into its open tab by itself), staged data `edits` that were not saved (with the rows that change), and `transaction`s that were open, which the server rolled back. Answers at most `limit` (50, ≤ 500) with `total` and `truncated`. Nothing is run against a server; see an item with db.recovery.get, bring it back with db.recovery.restore, drop it with db.recovery.discard.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<RecoveredList, CommandError> {
        let recovery = needed(session)?;
        Ok(list(&recovery, params.limit.unwrap_or(LISTED).min(MOST_LISTED)))
    }
}

/// Which recovered item.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemParams {
    /// The item's id, from `db.recovery.list`.
    pub id: u64,
    /// The most characters of its text or SQL to answer; 2000 when absent.
    pub text_limit: Option<usize>,
}

/// A recovered item, whole.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RecoveredDetail {
    /// The item.
    #[serde(flatten)]
    pub item: RecoveredItem,
    /// A console's text; for edits, the SQL that would save them (values as literals), never run by itself.
    pub text: String,
    /// Whether `text` was cut to `textLimit`.
    pub text_cut: bool,
    /// For edits, the statement their rows were read with, passwords masked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub statement: Option<String>,
    /// For a transaction, the first line of its last statements, passwords masked.
    pub log: Vec<String>,
}

pub fn edits_sql(edits: &StagedEdits) -> String {
    zen_db::edit::preview(&zen_db::edit::plan(&edits.editing(), &edits.changes))
}

fn cut(text: &str, limit: usize) -> (String, bool) {
    match text.char_indices().nth(limit) {
        Some((end, _)) => (text[..end].to_owned(), true),
        None => (text.to_owned(), false),
    }
}

pub fn detail(id: u64, found: &Item, text_limit: usize) -> RecoveredDetail {
    let (text, statement, log) = match found {
        Item::Console(console) => (console.text.clone(), None, Vec::new()),
        Item::Edits(edits) => (edits_sql(edits), Some(edits.statement.clone()), Vec::new()),
        Item::Transaction(open) => (String::new(), None, open.log.clone()),
    };
    let (text, text_cut) = cut(&text, text_limit);
    RecoveredDetail { item: item(id, found), text, text_cut, statement, log }
}

pub struct GetRecovered;

impl Command for GetRecovered {
    type Params = ItemParams;
    type Output = RecoveredDetail;
    const ID: &'static str = "db.recovery.get";
    const TITLE: &'static str = "Recovered Item";
    const DESCRIPTION: &'static str = "One recovered item by `id` (db.recovery.list): a console's text, or for staged edits the SQL that would save them in the connection's dialect (an UPDATE finds its row by the old key and checks that each changed cell still holds the value read), or a transaction's last statements (first lines, passwords masked); text cut to `textLimit` (2000) characters. Runs nothing.";

    fn run(&self, session: &mut Session, params: ItemParams) -> Result<RecoveredDetail, CommandError> {
        let (_, _, found) = recovered(session, params.id)?;
        Ok(detail(params.id, &found, params.text_limit.unwrap_or(TEXT_SHOWN).max(1)))
    }
}

/// Which recovered item to bring back.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RestoreParams {
    /// The item's id, from `db.recovery.list`.
    pub id: u64,
}

/// A recovered item brought back.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    /// The console it is in now.
    pub path: PathBuf,
    /// Its tab in the Database workspace.
    pub tab: InnerTab,
}

pub struct RestoreRecovered;

impl Command for RestoreRecovered {
    type Params = RestoreParams;
    type Output = Restored;
    const ID: &'static str = "db.recovery.restore";
    const TITLE: &'static str = "Restore Recovered Item";
    const DESCRIPTION: &'static str = "Brings the recovered item `id` back for review, running nothing: a console opens in the Database workspace with the recovered text as unsaved changes (⌘Z goes back to the text saved); staged edits open as their SQL in a new console of their connection, to read and run by hand. A transaction cannot come back (the server rolled it back): refused with code `rolledBack`. Answers the console and its tab.";

    fn run(&self, session: &mut Session, params: RestoreParams) -> Result<Restored, CommandError> {
        let (recovery, key, found) = recovered(session, params.id)?;
        match &*found {
            Item::Console(console) => {
                let open = OpenTabParams {
                    workspace: WORKSPACE.to_owned(),
                    path: Some(console.path.clone()),
                    ..OpenTabParams::default()
                };
                if !console.path.exists() {
                    if let Some(folder) = console.path.parent() {
                        std::fs::create_dir_all(folder).map_err(|error| {
                            CommandError::new(t!(
                                "workbench-error-create",
                                path = crate::display(folder),
                                error = error.to_string()
                            ))
                        })?;
                    }
                    zen_vfs::create_file(&console.path).map_err(|error| {
                        CommandError::new(t!(
                            "workbench-error-create",
                            path = crate::display(&console.path),
                            error = error.to_string()
                        ))
                    })?;
                }
                let tab = OpenTab.run(session, open)?;
                recovery.adopt(params.id);
                let version = session
                    .state_mut::<Workbench>()
                    .documents
                    .iter_mut()
                    .find(|document| document.path == console.path)
                    .map(|document| {
                        if document.text() != console.text {
                            let whole = 0..document.snapshot().len_chars();
                            document.replace_ranges(vec![(whole, console.text.clone())]);
                        }
                        document.version()
                    });
                if let Some(version) = version {
                    session.state_mut::<Tracked>().written.insert(key, version);
                }
                session.state_mut::<Workbench>().revision += 1;
                Ok(Restored { path: console.path.clone(), tab })
            }
            Item::Edits(edits) => {
                let header = t!(
                    "database-recovery-sql-header",
                    table = edits.editing.table.clone().unwrap_or_default(),
                    connection = edits.connection_name.as_str()
                );
                let text = format!("-- {header}\n{}\n", edits_sql(edits));
                let opened = NewConsole
                    .run(session, NewConsoleParams { connection: Some(edits.connection.clone()), text: Some(text) })?;
                recovery.discard(params.id);
                Ok(Restored { path: opened.path, tab: opened.tab })
            }
            Item::Transaction(_) => {
                Err(CommandError::with_code("rolledBack", t!("database-error-recovered-rolled-back")))
            }
        }
    }
}

/// Which recovered items to drop.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscardParams {
    /// The items' ids, from `db.recovery.list`.
    pub ids: Vec<u64>,
    /// Every recovered item instead of `ids`.
    pub all: bool,
    /// The person agreed to lose the unsaved text and edits among them.
    pub confirmed: bool,
}

pub const CONFIRM_DISCARD: &str = "confirmDiscard";

/// What `db.recovery.discard` dropped.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Discarded {
    /// How many items were dropped.
    pub discarded: usize,
    /// How many recovered items are left.
    pub left: usize,
}

pub struct DiscardRecovered;

impl Command for DiscardRecovered {
    type Params = DiscardParams;
    type Output = Discarded;
    const ID: &'static str = "db.recovery.discard";
    const TITLE: &'static str = "Discard Recovered Items";
    const DESCRIPTION: &'static str = "Drops the recovered items `ids` (or every one with `all: true`; nothing when neither is given) for good: their text and edits are removed from the recovery journal, which is deleted when it keeps nothing. Unsaved console text or data edits among them are lost, so without `confirmed: true` it is refused with code `confirmDiscard`, the `items` it would drop and the `retry` to run with `confirmed`; notices of rolled-back transactions drop at once. Touches no console file and no database.";

    fn run(&self, session: &mut Session, params: DiscardParams) -> Result<Discarded, CommandError> {
        let recovery = needed(session)?;
        let recovered = recovery.recovered();
        let chosen: Vec<&(u64, String, Arc<Item>)> =
            recovered.iter().filter(|(id, _, _)| params.all || params.ids.contains(id)).collect();
        let losing: Vec<serde_json::Value> = chosen
            .iter()
            .filter(|(_, _, item)| !matches!(item.as_ref(), Item::Transaction(_)))
            .map(|(id, key, _)| serde_json::json!({ "id": id, "key": key }))
            .collect();
        if !losing.is_empty() && !params.confirmed {
            let retry = serde_json::json!({ "ids": params.ids, "all": params.all, "confirmed": true });
            return Err(CommandError::with_code(
                CONFIRM_DISCARD,
                t!("database-recovery-confirm-discard", count = losing.len()),
            )
            .with_data(serde_json::json!({ "items": losing, "retry": { "command": Self::ID, "params": retry } })));
        }
        let discarded = chosen.iter().filter(|(id, _, _)| recovery.discard(*id)).count();
        Ok(Discarded { discarded, left: recovery.recovered().len() })
    }
}

pub struct OpenRecovery;

impl Command for OpenRecovery {
    type Params = serde_json::Value;
    type Output = InnerTab;
    const ID: &'static str = "db.recovery.open";
    const TITLE: &'static str = "Open Recovered Items";
    const DESCRIPTION: &'static str = "Opens the Recovered tab of the Database workspace (or brings it to front) and expands the workspace: what an earlier run left unsaved when it stopped without closing, each with Restore and Discard. Read it with db.recovery.list. Connects nowhere.";

    fn run(&self, session: &mut Session, _params: serde_json::Value) -> Result<InnerTab, CommandError> {
        let open = OpenTabParams {
            workspace: WORKSPACE.to_owned(),
            id: Some(TAB.to_owned()),
            kind: Some(TAB.to_owned()),
            title: Some(t!("database-recovery-tab-title")),
            ..OpenTabParams::default()
        };
        OpenTab.run(session, open)
    }
}

#[cfg(test)]
mod tests;

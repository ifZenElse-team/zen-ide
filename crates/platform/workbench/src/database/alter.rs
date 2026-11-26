use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::alter::{self, Change, Note, Reason, Refusal, Script};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{OperationStatus, QueryOptions, QueryStatement};
use zen_db::registry::Entry;
use zen_db::schema::{Catalog, IntrospectRequest, LARGE_SCHEMA, ObjectKind};
use zen_i18n::t;

use super::WORKSPACE;
use super::connections::{describe, request};
use super::consoles::{engine, ensure_connected};
use super::objects::{ObjectTab, OpenParams, object_tab};
use super::safety::{Confirmation, Run};
use super::sources::{Database, find};
use super::structure::{Structure, ensure};
use crate::Workbench;
use crate::workspaces::{InnerTab, Workspaces};

pub const PENDING_STRUCTURE: &str = "pendingStructureChanges";

const LISTED: usize = 200;
const MIGRATIONS: &str = "migrations";

#[derive(Clone, Debug)]
struct Running {
    query: u64,
    changes: Vec<Change>,
    then: Option<Then>,
}

#[derive(Clone, Debug)]
struct Then {
    statements: Vec<QueryStatement>,
    options: QueryOptions,
}

const FOREIGN_KEYS_OFF: &str = "PRAGMA foreign_keys = OFF";

#[derive(Clone, Debug, Default)]
struct Staged {
    connection: String,
    changes: Vec<Change>,
    running: Option<Running>,
    last: Option<Outcome>,
    reopen: Option<Vec<String>>,
}

#[derive(Default)]
pub struct Alterations {
    tabs: HashMap<String, Staged>,
}

impl Alterations {
    pub fn staged(&self, tab: &str) -> bool {
        self.tabs.get(tab).is_some_and(|staged| !staged.changes.is_empty())
    }
}

/// How the last run of a tab's script ended.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// `done`, `failed` or `cancelled`.
    pub status: OperationStatus,
    /// The query that ran it, for db.query.state.
    pub query: u64,
    /// Why it failed, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// What a connection's server can change, for the window to offer only that.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Supports {
    /// Structure can be changed at all.
    pub structure: bool,
    /// Schemas can be created and dropped.
    pub schemas: bool,
    /// Schemas can be renamed.
    pub rename_schemas: bool,
    /// Sequences exist.
    pub sequences: bool,
    /// Functions and procedures can be created.
    pub routines: bool,
    /// Objects and columns keep comments.
    pub comments: bool,
    /// Drops can take what depends on them along (`CASCADE`).
    pub cascade: bool,
    /// Some changes rebuild the table (SQLite).
    pub rebuilds: bool,
}

fn supports(dialect: Option<SqlDialect>) -> Supports {
    let Some(dialect) = dialect.filter(|dialect| dialect.edits()) else { return Supports::default() };
    let sqlite = dialect == SqlDialect::Sqlite;
    Supports {
        structure: true,
        schemas: !sqlite,
        rename_schemas: dialect == SqlDialect::Postgres,
        sequences: matches!(dialect, SqlDialect::Postgres | SqlDialect::MariaDb),
        routines: !sqlite,
        comments: !sqlite,
        cascade: dialect == SqlDialect::Postgres,
        rebuilds: sqlite,
    }
}

/// A staged change as listed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    /// Its index, for db.structure.unstage.
    pub index: usize,
    /// What it does, in words.
    pub summary: String,
    /// The change itself.
    pub change: Change,
}

/// An object tab's staged structure changes.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AlterState {
    /// The tab.
    pub tab: String,
    /// The connection's id.
    pub connection: String,
    /// The staged changes in order, at most `limit`.
    pub changes: Vec<Listed>,
    /// How many are staged.
    pub total: usize,
    /// Whether changes were left out because of `limit`.
    pub truncated: bool,
    /// Whether anything is staged: the tab is marked as changed.
    pub dirty: bool,
    /// The query running the script now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<u64>,
    /// How the last run ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<Outcome>,
    /// Whether the connection is read-only, so nothing can be staged.
    pub read_only: bool,
    /// Whether the connection type asks before statements that change something.
    pub confirm: bool,
    /// What the server can change.
    pub supports: Supports,
    /// The dialect the script is written in, by its driver's id (`postgresql`, `mysql`, `mariadb`, `sqlite`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dialect: Option<String>,
}

fn reason_text(refusal: &Refusal) -> String {
    let subject = refusal.subject.as_str();
    match refusal.reason {
        Reason::Driver => t!("database-alter-reason-driver", subject = subject),
        Reason::Schemas => t!("database-alter-reason-schemas"),
        Reason::RenameSchema => t!("database-alter-reason-rename-schema"),
        Reason::Sequences => t!("database-alter-reason-sequences"),
        Reason::Routines => t!("database-alter-reason-routines"),
        Reason::Comments => t!("database-alter-reason-comments", subject = subject),
        Reason::Rename => t!("database-alter-reason-rename", subject = subject),
        Reason::Cascade => t!("database-alter-reason-cascade", subject = subject),
        Reason::Identity => t!("database-alter-reason-identity", subject = subject),
        Reason::ConstraintKind => t!("database-alter-reason-constraint-kind"),
        Reason::Generated => t!("database-alter-reason-generated", subject = subject),
        Reason::Restate => t!("database-alter-reason-restate", subject = subject),
        Reason::RebuildLoses => t!("database-alter-reason-rebuild-loses", subject = subject),
        Reason::UnknownSchema => t!("database-alter-reason-unknown-schema", subject = subject),
        Reason::UnknownObject => t!("database-alter-reason-unknown-object", subject = subject),
        Reason::UnknownColumn => t!("database-alter-reason-unknown-column", subject = subject),
        Reason::UnknownConstraint => t!("database-alter-reason-unknown-constraint", subject = subject),
        Reason::UnknownIndex => t!("database-alter-reason-unknown-index", subject = subject),
        Reason::Exists => t!("database-alter-reason-exists", subject = subject),
        Reason::Missing => t!("database-alter-reason-missing", subject = subject),
        Reason::NotOneStatement => t!("database-alter-reason-not-one-statement", subject = subject),
        Reason::NotEmpty => t!("database-alter-reason-not-empty", subject = subject),
        Reason::Action => t!("database-alter-reason-action", subject = subject),
        Reason::Nothing => t!("database-alter-reason-nothing", subject = subject),
        Reason::RunFirst => t!("database-alter-reason-run-first", subject = subject),
    }
}

fn note_text(note: &Note) -> String {
    match note {
        Note::Rebuild { table } => t!("database-alter-note-rebuild", table = table.as_str()),
        Note::ImplicitCommit => t!("database-alter-note-implicit-commit"),
    }
}

fn kind_text(kind: ObjectKind) -> String {
    match kind {
        ObjectKind::Table => t!("database-alter-kind-table"),
        ObjectKind::ForeignTable => t!("database-alter-kind-foreign-table"),
        ObjectKind::View => t!("database-alter-kind-view"),
        ObjectKind::MaterializedView => t!("database-alter-kind-materialized-view"),
        ObjectKind::Function => t!("database-alter-kind-function"),
        ObjectKind::Procedure => t!("database-alter-kind-procedure"),
        ObjectKind::Sequence => t!("database-alter-kind-sequence"),
        ObjectKind::Type => t!("database-alter-kind-type"),
        ObjectKind::Event => t!("database-alter-kind-event"),
        ObjectKind::Package => t!("database-alter-kind-package"),
    }
}

fn cascade_text(cascade: bool) -> String {
    if cascade { t!("database-alter-with-cascade") } else { String::new() }
}

pub fn summary(change: &Change) -> String {
    match change {
        Change::CreateSchema { name } => t!("database-alter-create-schema", name = name.as_str()),
        Change::RenameSchema { name, to } => t!("database-alter-rename-schema", name = name.as_str(), to = to.as_str()),
        Change::DropSchema { name, cascade } => {
            t!("database-alter-drop-schema", name = name.as_str(), cascade = cascade_text(*cascade))
        }
        Change::CommentSchema { name, .. } => t!("database-alter-comment-schema", name = name.as_str()),
        Change::CreateTable { name, columns, .. } => {
            t!("database-alter-create-table", name = name.as_str(), count = columns.len())
        }
        Change::CreateView { name, .. } => t!("database-alter-create-view", name = name.as_str()),
        Change::CreateSequence { name, .. } => t!("database-alter-create-sequence", name = name.as_str()),
        Change::CreateRoutine { name, .. } => t!("database-alter-create-routine", name = name.as_str()),
        Change::Rename { kind, name, to, .. } => {
            t!("database-alter-rename", kind = kind_text(*kind), name = name.as_str(), to = to.as_str())
        }
        Change::Drop { kind, name, cascade, .. } => {
            t!("database-alter-drop", kind = kind_text(*kind), name = name.as_str(), cascade = cascade_text(*cascade))
        }
        Change::Comment { kind, name, .. } => {
            t!("database-alter-comment", kind = kind_text(*kind), name = name.as_str())
        }
        Change::AddColumn { table, column, .. } => t!(
            "database-alter-add-column",
            name = column.name.as_str(),
            type = column.type_name.as_str(),
            table = table.as_str()
        ),
        Change::AlterColumn { table, column, .. } => {
            t!("database-alter-alter-column", name = column.as_str(), table = table.as_str())
        }
        Change::DropColumn { table, column, cascade, .. } => t!(
            "database-alter-drop-column",
            name = column.as_str(),
            table = table.as_str(),
            cascade = cascade_text(*cascade)
        ),
        Change::AddConstraint { table, constraint, .. } => t!(
            "database-alter-add-constraint",
            name = constraint.name.clone().unwrap_or_default(),
            columns = constraint.columns.join(", "),
            table = table.as_str()
        ),
        Change::DropConstraint { table, name, cascade, .. } => t!(
            "database-alter-drop-constraint",
            name = name.as_str(),
            table = table.as_str(),
            cascade = cascade_text(*cascade)
        ),
        Change::CreateIndex { table, index, .. } => {
            t!("database-alter-create-index", columns = index.columns.join(", "), table = table.as_str())
        }
        Change::DropIndex { table, name, .. } => {
            t!("database-alter-drop-index", name = name.as_str(), table = table.as_str())
        }
    }
}

fn refused(refusal: &Refusal) -> CommandError {
    CommandError::with_code("unsupported", reason_text(refusal)).with_data(serde_json::json!({ "refusal": refusal }))
}

fn tab_of(session: &Session, tab: Option<&str>) -> Result<(InnerTab, ObjectTab), CommandError> {
    object_tab(session, tab)
}

fn catalog_of(session: &mut Session, entry: &Entry, data: &ObjectTab) -> Result<Catalog, CommandError> {
    ensure(session, entry, true);
    let name = data.path.first().cloned().unwrap_or_default();
    session
        .state::<Structure>()
        .and_then(|structure| structure.snapshot(&entry.source.id))
        .and_then(|snapshot| snapshot.catalog(&name))
        .cloned()
        .ok_or_else(|| {
            CommandError::with_code(
                "unknownObject",
                t!("database-error-unknown-object", path = data.path.join("/"), connection = data.connection.as_str()),
            )
        })
}

fn generate(session: &mut Session, data: &ObjectTab, changes: &[Change]) -> Result<(Script, SqlDialect), CommandError> {
    let entry = find(session, &data.connection)?;
    let dialect = super::filters::dialect_of(session, &data.connection)
        .ok_or_else(|| refused(&Refusal { change: 0, reason: Reason::Driver, subject: entry.source.driver.clone() }))?;
    let catalog = catalog_of(session, &entry, data)?;
    let script = alter::script(dialect, &catalog, changes).map_err(|refusal| refused(&refusal))?;
    Ok((script, dialect))
}

fn staged_of<'a>(session: &'a Session, tab: &str) -> Option<&'a Staged> {
    session.state::<Alterations>()?.tabs.get(tab)
}

fn state_of(session: &mut Session, tab: &InnerTab, data: &ObjectTab, limit: usize) -> AlterState {
    step(session);
    let entry = find(session, &data.connection).ok();
    let read_only = entry.as_ref().is_some_and(|entry| super::safety::read_only(session, entry));
    let confirm = entry.as_ref().is_some_and(|entry| super::safety::type_of(session, entry).confirm_execute);
    let dialect = super::filters::dialect_of(session, &data.connection);
    let staged = staged_of(session, &tab.id).cloned().unwrap_or_default();
    let total = staged.changes.len();
    AlterState {
        tab: tab.id.clone(),
        connection: data.connection.clone(),
        changes: staged
            .changes
            .iter()
            .enumerate()
            .take(limit)
            .map(|(index, change)| Listed { index, summary: summary(change), change: change.clone() })
            .collect(),
        total,
        truncated: total > limit,
        dirty: total > 0,
        running: staged.running.as_ref().map(|running| running.query),
        last: staged.last,
        read_only,
        confirm,
        supports: supports(dialect),
        dialect: dialect.map(|dialect| dialect.driver().to_owned()),
    }
}

fn refuse_running(session: &Session, tab: &str) -> Result<(), CommandError> {
    if staged_of(session, tab).is_some_and(|staged| staged.running.is_some()) {
        return Err(CommandError::with_code("saving", t!("database-alter-error-running")));
    }
    Ok(())
}

fn object_path(data: &ObjectTab, kind: ObjectKind, schema: &str, key: &str) -> Vec<String> {
    let catalog = data.path.first().cloned().unwrap_or_default();
    vec![catalog, schema.to_owned(), kind.folder().to_owned(), key.to_owned()]
}

fn refuse_unread(session: &mut Session, data: &ObjectTab, change: &Change) -> Result<(), CommandError> {
    let Some((kind, name)) = change.inside() else { return Ok(()) };
    let entry = find(session, &data.connection)?;
    let path = object_path(data, kind, change.schema(), name);
    let detailed = session
        .state::<Structure>()
        .and_then(|structure| structure.snapshot(&entry.source.id))
        .and_then(|snapshot| zen_db::schema::find_object(snapshot, &path))
        .is_none_or(|found| found.object.detailed);
    if detailed {
        return Ok(());
    }
    let busy = session.state::<Structure>().is_some_and(|structure| structure.busy(&entry.source.id));
    if !busy {
        super::structure::details(session, &entry, &path)?;
    }
    Err(CommandError::with_code("loading", t!("database-alter-error-loading", name = name)))
}

/// Which object tab.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TabParams {
    /// The object tab's id; the active tab of the Database workspace when absent.
    pub tab: Option<String>,
    /// The most changes to list; 200 when absent.
    pub limit: Option<usize>,
}

/// A change to stage.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StageParams {
    /// The object tab's id; the active tab of the Database workspace when absent.
    #[serde(default)]
    pub tab: Option<String>,
    /// The change: `{"action": "addColumn", "schema", "table", "column": {"name", "type", "notNull", "default", "identity", "comment"}}` and the other actions of db.structure.stage's description.
    pub change: Change,
}

pub struct Stage;

impl Command for Stage {
    type Params = StageParams;
    type Output = AlterState;
    const ID: &'static str = "db.structure.stage";
    const TITLE: &'static str = "Stage Structure Change";
    const DESCRIPTION: &'static str = "Stages a structure change on an object tab (`tab`, else the active one) of the Database workspace; changes accumulate until db.structure.execute and nothing runs before. `change.action` is one of `createSchema` {name}, `renameSchema` {name, to}, `dropSchema` {name, cascade}, `commentSchema` {name, comment}, `createTable` {schema, name, columns: [{name, type, notNull, default, identity, comment}], primaryKey, comment}, `createView` {schema, name, query}, `createSequence` {schema, name, settings: {start, increment, min, max, cycle}}, `createRoutine` {schema, name, definition: the whole CREATE FUNCTION/PROCEDURE statement}, `rename` {schema, kind, name, to}, `drop` {schema, kind, name, cascade}, `comment` {schema, kind, name, comment}, `addColumn` {schema, table, column}, `alterColumn` {schema, table, column, patch: {name, type, notNull, default, comment}; empty default or comment drops it}, `dropColumn` {schema, table, column, cascade}, `addConstraint` {schema, table, constraint: {name, kind: primaryKey|unique|check|foreignKey, columns, check, references: {schema, table, columns, onDelete, onUpdate}}}, `dropConstraint` {schema, table, name, cascade}, `createIndex` {schema, table, index: {name, columns, unique}}, `dropIndex` {schema, table, name}; `kind` is `table`, `view`, `materializedView`, `foreignTable`, `function`, `procedure`, `sequence`, `type` or `event`, a routine's `name` with its arguments as its path has it. Checked at once against what the server can do in its dialect: refused with code `unsupported`, the reason in words and `refusal` (`reason`, `subject`), `readOnly` on a read-only connection, `loading` while the table's details are read. Answers the staged changes.";

    fn run(&self, session: &mut Session, params: StageParams) -> Result<AlterState, CommandError> {
        let (tab, data) = tab_of(session, params.tab.as_deref())?;
        refuse_running(session, &tab.id)?;
        let entry = find(session, &data.connection)?;
        if super::safety::read_only(session, &entry) {
            return Err(CommandError::with_code(
                super::safety::READ_ONLY,
                t!("database-alter-error-read-only", name = entry.source.name.as_str()),
            ));
        }
        refuse_unread(session, &data, &params.change)?;
        let mut changes = staged_of(session, &tab.id).map(|staged| staged.changes.clone()).unwrap_or_default();
        changes.push(params.change);
        generate(session, &data, &changes)?;
        let staged = session.state_mut::<Alterations>().tabs.entry(tab.id.clone()).or_default();
        staged.connection.clone_from(&data.connection);
        staged.changes = changes;
        staged.last = None;
        Ok(state_of(session, &tab, &data, LISTED))
    }
}

/// A staged change to drop.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UnstageParams {
    /// The object tab's id; the active tab of the Database workspace when absent.
    pub tab: Option<String>,
    /// The change's index.
    pub index: usize,
}

pub struct Unstage;

impl Command for Unstage {
    type Params = UnstageParams;
    type Output = AlterState;
    const ID: &'static str = "db.structure.unstage";
    const TITLE: &'static str = "Drop Staged Structure Change";
    const DESCRIPTION: &'static str = "Drops staged change `index` of an object tab (`tab`, else the active one); refused with code `unsupported` when the changes after it no longer make sense without it (a column added by it and changed later). Nothing is sent to the server. Answers the staged changes.";

    fn run(&self, session: &mut Session, params: UnstageParams) -> Result<AlterState, CommandError> {
        let (tab, data) = tab_of(session, params.tab.as_deref())?;
        refuse_running(session, &tab.id)?;
        let mut changes = staged_of(session, &tab.id).map(|staged| staged.changes.clone()).unwrap_or_default();
        if params.index >= changes.len() {
            return Err(CommandError::with_code(
                "noChange",
                t!("database-alter-error-no-change", index = params.index, count = changes.len()),
            ));
        }
        changes.remove(params.index);
        generate(session, &data, &changes)?;
        if let Some(staged) = session.state_mut::<Alterations>().tabs.get_mut(&tab.id) {
            staged.changes = changes;
        }
        Ok(state_of(session, &tab, &data, LISTED))
    }
}

pub struct Discard;

impl Command for Discard {
    type Params = TabParams;
    type Output = AlterState;
    const ID: &'static str = "db.structure.discard";
    const TITLE: &'static str = "Discard Structure Changes";
    const DESCRIPTION: &'static str = "Drops every staged structure change of an object tab (`tab`, else the active one). Nothing is sent to the server. Answers the staged changes, now none.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<AlterState, CommandError> {
        let (tab, data) = tab_of(session, params.tab.as_deref())?;
        refuse_running(session, &tab.id)?;
        if let Some(staged) = session.state_mut::<Alterations>().tabs.get_mut(&tab.id) {
            staged.changes.clear();
            staged.last = None;
        }
        Ok(state_of(session, &tab, &data, params.limit.unwrap_or(LISTED)))
    }
}

pub struct StateCommand;

impl Command for StateCommand {
    type Params = TabParams;
    type Output = AlterState;
    const ID: &'static str = "db.structure.state";
    const TITLE: &'static str = "Staged Structure Changes";
    const DESCRIPTION: &'static str = "The structure changes staged on an object tab (`tab`, else the active one): each with its index, its summary in words and the change, at most `limit` (200) with `total` and `truncated`; the script running and how the last run ended; whether the connection is read-only or asks before changes; and what its server can change (`supports`: schemas, sequences, routines, comments, cascade, rebuilds).";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<AlterState, CommandError> {
        let (tab, data) = tab_of(session, params.tab.as_deref())?;
        Ok(state_of(session, &tab, &data, params.limit.unwrap_or(LISTED)))
    }
}

/// The script Execute runs.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The script as it runs, each statement ending with `;`.
    pub sql: String,
    /// Its statements, at most `limit`.
    pub statements: Vec<String>,
    /// How many statements it has.
    pub total: usize,
    /// Whether statements were left out of `statements` because of `limit`.
    pub truncated: bool,
    /// What to know before running it, in words.
    pub notes: Vec<String>,
    /// Whether it runs in one transaction, all or nothing.
    pub transaction: bool,
    /// Whether the connection type asks before it runs.
    pub confirm: bool,
}

fn in_transaction(session: &mut Session, entry: &Entry) -> bool {
    let request = request(session, entry, None);
    let state = engine(session).transaction(&request, &ContextKind::Main, 0);
    state.auto_commit && !state.in_transaction
}

fn preview_of(
    session: &mut Session,
    data: &ObjectTab,
    changes: &[Change],
    limit: usize,
) -> Result<(Preview, Vec<String>), CommandError> {
    let (script, _) = generate(session, data, changes)?;
    let entry = find(session, &data.connection)?;
    let transaction = script.transactional && in_transaction(session, &entry);
    let all = script.all(transaction);
    let mut notes: Vec<String> = script.notes.iter().map(note_text).collect();
    if all.iter().any(|statement| statement == FOREIGN_KEYS_OFF) {
        notes.push(t!("database-alter-note-foreign-keys"));
    }
    if script.transactional && !transaction {
        notes.push(t!("database-alter-note-manual-commit"));
    }
    let confirm = super::safety::type_of(session, &entry).confirm_execute;
    let preview = Preview {
        sql: alter::text(&all),
        statements: all.iter().take(limit).cloned().collect(),
        total: all.len(),
        truncated: all.len() > limit,
        notes,
        transaction,
        confirm,
    };
    Ok((preview, all))
}

fn staged_changes(session: &Session, tab: &str) -> Vec<Change> {
    staged_of(session, tab).map(|staged| staged.changes.clone()).unwrap_or_default()
}

pub struct PreviewCommand;

impl Command for PreviewCommand {
    type Params = TabParams;
    type Output = Preview;
    const ID: &'static str = "db.structure.preview";
    const TITLE: &'static str = "Preview Structure Changes";
    const DESCRIPTION: &'static str = "The script db.structure.execute runs for the staged structure changes of an object tab (`tab`, else the active one), written in the connection's dialect: `sql` as it runs, `statements` (at most `limit`, 200, with `total` and `truncated`), `notes` (SQLite rebuilding a table, MySQL committing each statement, manual commit leaving it to Commit), whether it runs in one transaction (PostgreSQL and SQLite in auto-commit: `BEGIN` … `COMMIT`) and whether the connection type asks first. Nothing runs.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<Preview, CommandError> {
        let (tab, data) = tab_of(session, params.tab.as_deref())?;
        let changes = staged_changes(session, &tab.id);
        Ok(preview_of(session, &data, &changes, params.limit.unwrap_or(LISTED))?.0)
    }
}

/// Execute, confirmed or not.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExecuteParams {
    /// The object tab's id; the active tab of the Database workspace when absent.
    pub tab: Option<String>,
    /// The `token` of a `confirmExecute` refusal.
    pub confirmed: Option<String>,
    /// Indexes of the refusal's statements to leave out.
    pub skip: Vec<usize>,
}

/// A script started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Executed {
    /// The query running it, for db.query.state; absent when nothing was staged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<u64>,
    /// How many statements it runs.
    pub statements: usize,
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        _ => "databaseError",
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn statements_of(all: &[String]) -> Vec<QueryStatement> {
    let mut at = 0;
    all.iter()
        .map(|text| {
            let length = text.chars().count();
            let statement = QueryStatement {
                text: text.clone(),
                start: at,
                end: at + length,
                client: false,
                params: Vec::new(),
                kept: None,
            };
            at += length + 3;
            statement
        })
        .collect()
}

fn reread(data: &ObjectTab, changes: &[Change]) -> IntrospectRequest {
    let mut request = IntrospectRequest { large_schema: LARGE_SCHEMA, system: true, ..IntrospectRequest::default() };
    if changes.iter().all(|change| change.inside().is_some()) {
        for change in changes {
            if let Some((_, name)) = change.inside() {
                let key = (change.schema().to_owned(), name.to_owned());
                if !request.objects.contains(&key) {
                    request.objects.push(key);
                }
            }
        }
        return request;
    }
    request.force = true;
    for change in changes {
        let mut names = vec![change.schema().to_owned()];
        if let Change::RenameSchema { to, .. } = change {
            names.push(to.clone());
        }
        for name in names {
            if !request.schemas.contains(&name) {
                request.schemas.push(name);
            }
        }
    }
    if request.schemas.is_empty() {
        request.schemas.push(data.path.get(1).cloned().unwrap_or_default());
    }
    request
}

pub struct Execute;

impl Command for Execute {
    type Params = ExecuteParams;
    type Output = Executed;
    const ID: &'static str = "db.structure.execute";
    const TITLE: &'static str = "Execute Structure Changes";
    const DESCRIPTION: &'static str = "Runs the script of db.structure.preview for the staged structure changes of an object tab (`tab`, else the active one) in the background, in the connection's Main context, stopping at the first error (and rolling back when it runs in one transaction); this is the person's action: a disconnected connection connects first with its stored password. Production safety applies (ADR-0172): a read-only connection refuses (`readOnly`), agents follow the connection type (`agentReadOnly` with `handOff`, `agentNotAllowed`), and DROP, ALTER … DROP or a type that confirms SQL execution refuse with code `confirmExecute`, the statements, their risks and a `token`: run again with `confirmed: token`; the script runs whole, so `skip` is refused (`allOrNothing`). A SQLite rebuild reads `PRAGMA foreign_keys` first, switches foreign keys off for it and on again when they were on, and is refused with code `rebuildInTransaction` outside auto-commit or with a transaction open. When it ends well the changes are cleared and only the objects it changed are read again (a new, renamed or dropped object's schema); on an error they stay staged and db.structure.state tells why. Answers the query.";

    fn run(&self, session: &mut Session, params: ExecuteParams) -> Result<Executed, CommandError> {
        let (tab, data) = tab_of(session, params.tab.as_deref())?;
        step(session);
        refuse_running(session, &tab.id)?;
        let changes = staged_changes(session, &tab.id);
        if changes.is_empty() {
            return Ok(Executed { query: None, statements: 0 });
        }
        if !params.skip.is_empty() {
            return Err(CommandError::with_code("allOrNothing", t!("database-alter-error-skip")));
        }
        let (_, all) = preview_of(session, &data, &changes, LISTED)?;
        start(
            session,
            &tab.id,
            &data,
            (changes, all),
            params.confirmed,
            (Self::ID, serde_json::json!({ "tab": tab.id })),
        )
    }
}

fn start(
    session: &mut Session,
    key: &str,
    data: &ObjectTab,
    (changes, all): (Vec<Change>, Vec<String>),
    confirmed: Option<String>,
    retry: (&'static str, serde_json::Value),
) -> Result<Executed, CommandError> {
    let entry = find(session, &data.connection)?;
    let rebuild = all.iter().any(|statement| statement == FOREIGN_KEYS_OFF);
    if rebuild && !in_transaction(session, &entry) {
        return Err(CommandError::with_code("rebuildInTransaction", t!("database-alter-error-rebuild-transaction")));
    }
    let context = ContextKind::Main;
    let place = data.path.last().cloned();
    let mut provenance = super::history::provenance(session, zen_db::history::Source::Structure, place);
    let confirmation = Confirmation { confirmed, skip: Vec::new() };
    let run = Run { entry: &entry, context: &context, provenance: &provenance, confirmation: &confirmation, retry };
    let guarded = super::safety::guard(session, &run, statements_of(&all)).map_err(|mut error| {
        if let Some(data) = error.data.as_mut() {
            data.insert("skippable".to_owned(), serde_json::Value::Bool(false));
        }
        error
    })?;
    provenance.confirmed = guarded.confirmed;
    ensure_connected(session, &data.connection)?;
    let statements = guarded.statements.len();
    let options = QueryOptions {
        stop_on_error: true,
        rollback_on_error: true,
        timeout: None,
        schema: None,
        provenance,
        read_only: guarded.read_only,
        notes: Vec::new(),
    };
    let (first, then) = if rebuild {
        let probe = statements_of(&["PRAGMA foreign_keys".to_owned()]);
        let probe_options = QueryOptions { read_only: true, ..options.clone() };
        ((probe, probe_options), Some(Then { statements: guarded.statements, options }))
    } else {
        ((guarded.statements, options), None)
    };
    let query =
        engine(session).run_query(&data.connection, context, first.0, first.1).map_err(|error| failed(&error))?;
    session.state_mut::<Structure>().expect(&data.connection, reread(data, &changes));
    let staged = session.state_mut::<Alterations>().tabs.entry(key.to_owned()).or_default();
    staged.connection.clone_from(&data.connection);
    staged.running = Some(Running { query, changes, then });
    staged.last = None;
    Ok(Executed { query: Some(query), statements })
}

fn moved(data: &ObjectTab, changes: &[Change]) -> Option<Option<Vec<String>>> {
    let mut path = data.path.clone();
    let mut changed = false;
    for change in changes {
        match (change, path.as_slice()) {
            (Change::RenameSchema { name, to }, [_, schema, ..]) if schema == name => {
                path[1].clone_from(to);
                changed = true;
            }
            (Change::DropSchema { name, .. }, [_, schema, ..]) if schema == name => return Some(None),
            (Change::Rename { schema, kind, name, to }, [_, known, folder, key])
                if known == schema && folder == kind.folder() && key == name =>
            {
                let arguments = key.find('(').map(|open| key[open..].to_owned()).unwrap_or_default();
                path[3] = format!("{to}{arguments}");
                changed = true;
            }
            (Change::Drop { schema, kind, name, .. }, [_, known, folder, key])
                if known == schema && folder == kind.folder() && key == name =>
            {
                return Some(None);
            }
            _ => {}
        }
    }
    changed.then_some(Some(path))
}

pub fn step(session: &mut Session) {
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else { return };
    let running: Vec<(String, String, Running)> = session
        .state::<Alterations>()
        .map(|alterations| {
            alterations
                .tabs
                .iter()
                .filter_map(|(tab, staged)| Some((tab.clone(), staged.connection.clone(), staged.running.clone()?)))
                .collect()
        })
        .unwrap_or_default();
    for (tab, connection, mut run) in running {
        let state = engine.query(run.query);
        if state.as_ref().is_some_and(|state| state.status == OperationStatus::Running) {
            continue;
        }
        if let Some(then) = run.then.take() {
            let read = state.as_ref().filter(|state| state.status == OperationStatus::Done).and_then(|state| {
                let result = state.statements.first()?.result?;
                let set = engine.results().get(result)?;
                let on = set.get(0, 0) == zen_db::value::Value::Integer(1);
                engine.results().remove(result);
                Some(on)
            });
            if let Some(on) = read {
                let mut statements = then.statements;
                if on {
                    let text = "PRAGMA foreign_keys = ON".to_owned();
                    let at = statements.last().map_or(0, |last| last.end + 3);
                    let end = at + text.chars().count();
                    statements.push(QueryStatement {
                        text,
                        start: at,
                        end,
                        client: false,
                        params: Vec::new(),
                        kept: None,
                    });
                }
                if let Ok(query) = engine.run_query(&connection, ContextKind::Main, statements, then.options) {
                    if let Some(staged) = session.state_mut::<Alterations>().tabs.get_mut(&tab) {
                        staged.running = Some(Running { query, changes: run.changes, then: None });
                    }
                    continue;
                }
            }
            session.state_mut::<Structure>().forget_expected(&connection);
            if let Some(staged) = session.state_mut::<Alterations>().tabs.get_mut(&tab) {
                staged.running = None;
                staged.last = Some(Outcome {
                    status: OperationStatus::Failed,
                    query: run.query,
                    message: Some(t!("database-alter-error-foreign-keys")),
                });
            }
            continue;
        }
        session.state_mut::<Structure>().forget_expected(&connection);
        let status = state.as_ref().map_or(OperationStatus::Cancelled, |state| state.status);
        let error = state.as_ref().and_then(|state| {
            state
                .statements
                .iter()
                .find_map(|statement| statement.error.as_ref())
                .or(state.error.as_ref())
                .map(describe)
        });
        let ok = status == OperationStatus::Done && error.is_none();
        let data = session
            .state::<Workspaces>()
            .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|known| known.id == tab).cloned())
            .and_then(|found| serde_json::from_value::<ObjectTab>(found.data).ok());
        let mut close = false;
        if let Some(staged) = session.state_mut::<Alterations>().tabs.get_mut(&tab) {
            staged.running = None;
            staged.last = Some(Outcome {
                status: if ok {
                    OperationStatus::Done
                } else if status == OperationStatus::Done {
                    OperationStatus::Failed
                } else {
                    status
                },
                query: run.query,
                message: error,
            });
            if ok {
                staged.changes.clear();
                match data.as_ref().and_then(|data| moved(data, &run.changes)) {
                    Some(Some(path)) => staged.reopen = Some(path),
                    Some(None) => close = true,
                    None => {}
                }
            }
        }
        if close {
            session.state_mut::<Alterations>().tabs.remove(&tab);
            let _ = crate::workspaces::close_inner_unguarded(session, WORKSPACE, &tab);
        }
        if ok {
            follow(session, &tab, &connection, &run.changes);
        }
    }
    reopen(session);
}

fn follow(session: &mut Session, key: &str, connection: &str, changes: &[Change]) {
    let tabs: Vec<(String, ObjectTab)> = session
        .state::<Workspaces>()
        .map(|workspaces| {
            workspaces
                .tabs(WORKSPACE)
                .iter()
                .filter(|tab| tab.id != key && tab.kind == super::objects::KIND)
                .filter_map(|tab| Some((tab.id.clone(), serde_json::from_value::<ObjectTab>(tab.data.clone()).ok()?)))
                .filter(|(_, data)| data.connection == connection)
                .collect()
        })
        .unwrap_or_default();
    for (id, data) in tabs {
        match moved(&data, changes) {
            Some(None) => {
                session.state_mut::<Alterations>().tabs.remove(&id);
                let _ = crate::workspaces::close_inner_unguarded(session, WORKSPACE, &id);
            }
            Some(Some(path)) => {
                let staged = session.state_mut::<Alterations>().tabs.entry(id).or_default();
                if staged.changes.is_empty() && staged.running.is_none() {
                    connection.clone_into(&mut staged.connection);
                    staged.reopen = Some(path);
                }
            }
            None => {}
        }
    }
}

fn reopen(session: &mut Session) {
    let waiting: Vec<(String, String, Vec<String>)> = session
        .state::<Alterations>()
        .map(|alterations| {
            alterations
                .tabs
                .iter()
                .filter_map(|(tab, staged)| Some((tab.clone(), staged.connection.clone(), staged.reopen.clone()?)))
                .collect()
        })
        .unwrap_or_default();
    for (tab, connection, path) in waiting {
        if session.state::<Structure>().is_some_and(|structure| structure.busy(&connection)) {
            continue;
        }
        let known = session.state::<Structure>().and_then(|structure| structure.snapshot(&connection)).is_some_and(
            |snapshot| match path.as_slice() {
                [catalog, schema] => snapshot.schema(catalog, schema).is_some(),
                _ => zen_db::schema::find_object(snapshot, &path).is_some(),
            },
        );
        if let Some(staged) = session.state_mut::<Alterations>().tabs.get_mut(&tab) {
            staged.reopen = None;
        }
        if !known {
            continue;
        }
        let page = session
            .state::<Workspaces>()
            .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|known| known.id == tab).cloned())
            .and_then(|found| serde_json::from_value::<ObjectTab>(found.data).ok())
            .map(|data| data.page);
        session.state_mut::<Alterations>().tabs.remove(&tab);
        let _ = crate::workspaces::close_inner_unguarded(session, WORKSPACE, &tab);
        let open = OpenParams { connection, path, page };
        let _ = super::objects::ObjectOpen.run(session, open);
    }
}

pub struct ToConsole;

impl Command for ToConsole {
    type Params = TabParams;
    type Output = super::ConsoleOpened;
    const ID: &'static str = "db.structure.toConsole";
    const TITLE: &'static str = "Open Structure Script in Console";
    const DESCRIPTION: &'static str = "Opens the script of db.structure.preview for the staged structure changes of an object tab (`tab`, else the active one) in a new console of its connection, with its notes as comments, to edit and run by hand; the changes stay staged. Nothing runs. Answers the console.";

    fn run(&self, session: &mut Session, params: TabParams) -> Result<super::ConsoleOpened, CommandError> {
        let (tab, data) = tab_of(session, params.tab.as_deref())?;
        let changes = staged_changes(session, &tab.id);
        if changes.is_empty() {
            return Err(CommandError::with_code("nothingStaged", t!("database-alter-error-nothing")));
        }
        let (preview, _) = preview_of(session, &data, &changes, LISTED)?;
        let text = with_notes(&preview);
        super::NewConsole.run(session, super::NewConsoleParams { connection: Some(data.connection), text: Some(text) })
    }
}

fn with_notes(preview: &Preview) -> String {
    let mut text: String = preview.notes.iter().map(|note| format!("-- {note}\n")).collect();
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&preview.sql);
    if preview.sql.contains(FOREIGN_KEYS_OFF) {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str("-- PRAGMA foreign_keys = ON;\n");
    }
    text
}

/// Where to save a migration.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MigrationParams {
    /// The object tab's id; the active tab of the Database workspace when absent.
    pub tab: Option<String>,
    /// The folder, relative to the project; `migrations` when absent.
    pub folder: Option<String>,
    /// What the file is named after the time stamp; the object's name and the first change when absent.
    pub name: Option<String>,
}

/// A migration file written.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSaved {
    /// The new file.
    pub path: PathBuf,
    /// How many statements it holds.
    pub statements: usize,
}

fn slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.ends_with('_') && !slug.is_empty() {
            slug.push('_');
        }
    }
    let slug = slug.trim_end_matches('_').chars().take(60).collect::<String>();
    if slug.is_empty() { "change".to_owned() } else { slug }
}

fn stamp() -> String {
    let seconds =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_secs());
    let (year, month, day, hour, minute, second) = crate::ide_settings::utc(seconds);
    format!("{year:04}{month:02}{day:02}{hour:02}{minute:02}{second:02}")
}

fn inside_project(project: &Path, folder: &str) -> Result<PathBuf, CommandError> {
    let relative = Path::new(folder.trim());
    let plain = relative.components().all(|part| matches!(part, Component::Normal(_)));
    if folder.trim().is_empty() || !plain {
        return Err(CommandError::with_code("invalidFolder", t!("database-alter-error-folder", folder = folder)));
    }
    Ok(project.join(relative))
}

pub struct SaveMigration;

impl Command for SaveMigration {
    type Params = MigrationParams;
    type Output = MigrationSaved;
    const ID: &'static str = "db.structure.saveMigration";
    const TITLE: &'static str = "Save Structure Script as Migration";
    const DESCRIPTION: &'static str = "Writes the script of db.structure.preview for the staged structure changes of an object tab (`tab`, else the active one), with its notes as comments, to a new file `<folder>/<UTC time stamp>_<name>.sql` of the open project (`folder` relative to the project, `migrations` by default; never an existing file, never outside the project). The changes stay staged and nothing runs. Answers the file.";

    fn run(&self, session: &mut Session, params: MigrationParams) -> Result<MigrationSaved, CommandError> {
        let (tab, data) = tab_of(session, params.tab.as_deref())?;
        let changes = staged_changes(session, &tab.id);
        if changes.is_empty() {
            return Err(CommandError::with_code("nothingStaged", t!("database-alter-error-nothing")));
        }
        let project = session
            .state::<Workbench>()
            .and_then(Workbench::project)
            .map(Path::to_path_buf)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        let folder = inside_project(&project, params.folder.as_deref().unwrap_or(MIGRATIONS))?;
        super::guard_agent_file(session, &folder)?;
        let (preview, _) = preview_of(session, &data, &changes, LISTED)?;
        let name = params.name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| {
            let object = data.path.last().cloned().unwrap_or_default();
            format!("{object} {}", changes.first().map(summary).unwrap_or_default())
        });
        let failed = |error: std::io::Error| {
            CommandError::new(t!("workbench-error-create", path = crate::display(&folder), error = error.to_string()))
        };
        std::fs::create_dir_all(&folder).map_err(failed)?;
        let inside = std::fs::canonicalize(&folder)
            .ok()
            .zip(std::fs::canonicalize(&project).ok())
            .is_some_and(|(folder, project)| folder.starts_with(project));
        if !inside {
            let shown = params.folder.as_deref().unwrap_or(MIGRATIONS);
            return Err(CommandError::with_code("invalidFolder", t!("database-alter-error-folder", folder = shown)));
        }
        let stem = format!("{}_{}", stamp(), slug(&name));
        let path = crate::scratch::create_new(&folder, &stem, "sql", &with_notes(&preview)).map_err(failed)?;
        session.state_mut::<Workbench>().revision += 1;
        Ok(MigrationSaved { path, statements: preview.total })
    }
}

mod direct;

pub use direct::*;

#[cfg(test)]
mod tests;

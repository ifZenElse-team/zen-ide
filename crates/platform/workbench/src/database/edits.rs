use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::dialect::{SqlDialect, table_id};
use zen_db::edit::{self, Editing, Generate, InputError, KeyChoice, KeyKind, ReadOnly, Staged, identify, source_table};
use zen_db::engine::{ConnectionState, OperationStatus, SaveReport};
use zen_db::registry::Entry;
use zen_db::results::{Pending, ResultSet};
use zen_db::schema::{Object, Snapshot, qualified};
use zen_db::value::Value;
use zen_i18n::t;

use super::connections::describe;
use super::consoles::engine;
use super::filters::result_or_active;
use super::sources::{Database, find};
use super::structure::{Structure, details, ensure};
use crate::Workbench;

pub const PENDING_CHANGES: &str = "pendingChanges";

pub fn refuse_staged(session: &Session, results: &[u64], retry: (&str, serde_json::Value)) -> Result<(), CommandError> {
    let Some(engine) = session.state::<Database>().and_then(Database::started) else { return Ok(()) };
    let staged: Vec<u64> = results
        .iter()
        .copied()
        .filter(|id| engine.results().get(*id).is_some_and(|result| result.pending().any()))
        .collect();
    if staged.is_empty() {
        return Ok(());
    }
    Err(CommandError::with_code(PENDING_CHANGES, t!("database-question-staged", count = staged.len()))
        .with_data(serde_json::json!({ "results": staged, "retry": { "command": retry.0, "params": retry.1 } })))
}

const PREVIEWED: usize = 200;
const GENERATED: usize = 1000;

#[derive(Default)]
pub struct Edits {
    choices: HashMap<u64, KeyChoice>,
    saving: HashMap<u64, u64>,
    outcomes: HashMap<u64, SaveOutcome>,
}

/// How the last save of a result ended.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    /// `done`, `failed` or `cancelled`.
    pub status: OperationStatus,
    /// What it did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<SaveReport>,
    /// Why it failed, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct KeysFile {
    version: u32,
    keys: BTreeMap<String, Vec<String>>,
}

fn keys_file(session: &Session) -> Option<PathBuf> {
    let project = session.state::<Workbench>()?.project()?;
    Some(project.join(".zen").join("databases").join("virtual-keys.json"))
}

fn saved_keys(session: &Session) -> KeysFile {
    let Some(file) = keys_file(session) else { return KeysFile::default() };
    let read = || Ok::<_, std::convert::Infallible>(zen_db::registry::read_json::<KeysFile>(&file).ok().flatten());
    super::sources::cached(session, std::slice::from_ref(&file), read)
        .unwrap_or_else(|never| match never {})
        .unwrap_or_default()
}

fn find_table(snapshot: &Snapshot, oid: u32, numbered: bool) -> Option<(String, String, Object)> {
    let in_catalog = |catalog: &zen_db::schema::Catalog| {
        catalog.schemas.iter().find_map(|schema| {
            let id = |object: &Object| if numbered { object.oid } else { table_id(&schema.name, &object.name) };
            schema.objects.iter().find(|object| id(object) == oid).map(|object| {
                let mut object = object.clone();
                object.oid = oid;
                (catalog.name.clone(), schema.name.clone(), object)
            })
        })
    };
    let current = snapshot.catalogs.iter().filter(|catalog| catalog.current).find_map(in_catalog);
    if current.is_some() || numbered {
        return current;
    }
    let mut others = snapshot.catalogs.iter().filter(|catalog| !catalog.current).filter_map(in_catalog);
    let found = others.next()?;
    others.next().is_none().then_some(found)
}

pub(super) struct Known {
    pub(super) editing: Editing,
    entry: Option<Entry>,
    table_key: Option<String>,
}

enum EditKind {
    Sql(SqlDialect),
    Documents,
}

fn driver_edits(session: &mut Session, entry: &Entry) -> Result<EditKind, ReadOnly> {
    let drivers = session.state_mut::<Database>().drivers();
    let driver = drivers.iter().find(|driver| driver.id() == entry.source.driver).ok_or(ReadOnly::Driver)?;
    if edit::edits_documents(driver.capabilities()) {
        return Ok(EditKind::Documents);
    }
    edit::edit_dialect(driver.capabilities()).map(EditKind::Sql)
}

fn connected(session: &Session, connection: &str) -> bool {
    session.state::<Database>().and_then(Database::started).is_some_and(|engine| {
        matches!(engine.status(connection).state, ConnectionState::Connected | ConnectionState::Closed)
    })
}

pub(super) fn known(session: &mut Session, result: &ResultSet) -> Known {
    let columns = result.columns();
    let read_only = |reason| Known { editing: Editing::read_only(reason, &columns), entry: None, table_key: None };
    let Ok(entry) = find(session, result.connection()) else { return read_only(ReadOnly::NoTable) };
    let dialect = match driver_edits(session, &entry) {
        Ok(EditKind::Sql(dialect)) => dialect,
        Ok(EditKind::Documents) => {
            let documents = |reason| {
                let mut editing = Editing::read_only(reason, &columns);
                editing.documents = true;
                editing
            };
            if super::safety::read_only(session, &entry) {
                return Known { editing: documents(ReadOnly::Connection), entry: Some(entry), table_key: None };
            }
            let Some(target) = edit::document::collection_of(result.statement_text()) else {
                return Known { editing: documents(ReadOnly::NoTable), entry: Some(entry), table_key: None };
            };
            let editing = edit::document::identify(&columns, &target);
            return Known { editing, entry: Some(entry), table_key: None };
        }
        Err(reason) => return read_only(reason),
    };
    if super::safety::read_only(session, &entry) {
        return Known { entry: Some(entry), ..read_only(ReadOnly::Connection) };
    }
    let oid = match source_table(&columns) {
        Ok(oid) => oid,
        Err(reason) => return Known { entry: Some(entry), ..read_only(reason) },
    };
    ensure(session, &entry, false);
    let found = session
        .state::<Structure>()
        .and_then(|structure| structure.snapshot(&entry.source.id))
        .and_then(|snapshot| find_table(snapshot, oid, dialect == SqlDialect::Postgres));
    let Some((catalog, schema, object)) = found else {
        return Known { entry: Some(entry), ..read_only(ReadOnly::UnknownTable) };
    };
    if !object.detailed {
        let busy = session.state::<Structure>().is_some_and(|structure| structure.busy(&entry.source.id));
        if !busy && connected(session, &entry.source.id) {
            let path = vec![catalog, schema.clone(), object.kind.folder().to_owned(), object.key()];
            let _ = details(session, &entry, &path);
        }
        return Known { entry: Some(entry), ..read_only(ReadOnly::UnknownTable) };
    }
    let table_key = format!("{}|{}", entry.source.id, qualified(&schema, &object.name));
    let choice = session
        .state::<Edits>()
        .and_then(|edits| edits.choices.get(&result.id()).cloned())
        .or_else(|| saved_keys(session).keys.get(&table_key).cloned().map(KeyChoice::Columns));
    let editing = identify(&columns, &schema, &object, choice.as_ref(), dialect);
    Known { editing, entry: Some(entry), table_key: Some(table_key) }
}

pub(super) fn not_editable(editing: &Editing) -> CommandError {
    let reason = editing.reason.unwrap_or(ReadOnly::NoTable);
    CommandError::with_code("readOnly", reason_text(reason)).with_data(serde_json::json!({ "reason": reason }))
}

pub fn reason_text(reason: ReadOnly) -> String {
    match reason {
        ReadOnly::Connection => t!("database-read-only-connection"),
        ReadOnly::Driver => t!("database-read-only-driver"),
        ReadOnly::NoTable => t!("database-read-only-no-table"),
        ReadOnly::SeveralTables => t!("database-read-only-several-tables"),
        ReadOnly::UnknownTable => t!("database-read-only-unknown-table"),
        ReadOnly::NoKey => t!("database-read-only-no-key"),
        ReadOnly::NoRowIdentity => t!("database-read-only-no-row-identity"),
    }
}

pub(super) fn editable(session: &mut Session, result: &ResultSet) -> Result<Known, CommandError> {
    super::filters::refuse_while_reading(session, result)?;
    let found = known(session, result);
    if !found.editing.editable {
        return Err(not_editable(&found.editing));
    }
    refuse_while_saving(session, result)?;
    Ok(found)
}

fn refuse_while_saving(session: &mut Session, result: &ResultSet) -> Result<(), CommandError> {
    settle(session, result.id());
    if session.state::<Edits>().is_some_and(|edits| edits.saving.contains_key(&result.id())) {
        return Err(CommandError::with_code("saving", t!("database-error-saving")));
    }
    Ok(())
}

fn no_cell(result: &ResultSet, row: usize, column: usize) -> CommandError {
    CommandError::with_code(
        "noCell",
        t!("database-error-no-cell", row = row, column = column, rows = result.len(), columns = result.columns().len()),
    )
}

pub(super) fn input_error(error: InputError) -> String {
    match error {
        InputError::Integer => t!("database-error-input-integer"),
        InputError::Number => t!("database-error-input-number"),
        InputError::Boolean => t!("database-error-input-boolean"),
        InputError::Json => t!("database-error-input-json"),
        InputError::Uuid => t!("database-error-input-uuid"),
        InputError::Bytes => t!("database-error-input-bytes"),
        InputError::ObjectId => t!("database-error-input-object-id"),
    }
}

fn settle(session: &mut Session, result: u64) {
    let Some(op) = session.state::<Edits>().and_then(|edits| edits.saving.get(&result).copied()) else { return };
    let Some(state) = session.state::<Database>().and_then(Database::started).and_then(|engine| engine.operation(op))
    else {
        session.state_mut::<Edits>().saving.remove(&result);
        return;
    };
    if state.status == OperationStatus::Running {
        return;
    }
    let message = state.error.as_ref().map(|error| match (&state.saved, error.code.as_deref()) {
        (Some(SaveReport { failed_row: Some(row), kept: true, .. }), code) => {
            let error = if code == Some("conflict") { t!("database-error-save-gone") } else { describe(error) };
            t!("database-error-save-kept", row = row + 1, error = error)
        }
        (Some(report), Some("conflict")) => {
            t!("database-error-save-conflict", row = report.failed_row.map_or(0, |row| row + 1))
        }
        (Some(report), Some("severalRows")) => {
            t!("database-error-save-several", row = report.failed_row.map_or(0, |row| row + 1))
        }
        (Some(SaveReport { failed_row: Some(row), .. }), _) => {
            t!("database-error-save-row", row = row + 1, error = describe(error))
        }
        _ => describe(error),
    });
    let edits = session.state_mut::<Edits>();
    edits.saving.remove(&result);
    edits.outcomes.insert(result, SaveOutcome { status: state.status, report: state.saved, message });
}

/// A result's editing state.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditState {
    /// The result.
    pub id: u64,
    /// Whether staged changes can be saved, why not, the table, the key and each column.
    #[serde(flatten)]
    pub editing: Editing,
    /// Why it is read-only, in words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// What is staged.
    pub pending: Pending,
    /// Whether anything is staged: the tab is marked as changed.
    pub dirty: bool,
    /// Whether Save asks for a confirmation after the preview, as the connection type says (Test, Production).
    pub confirm: bool,
    /// The save running now, for db.operation.state and db.connection.cancel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saving: Option<u64>,
    /// How the last save ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_save: Option<SaveOutcome>,
}

pub(super) fn state_of(session: &mut Session, result: &Arc<ResultSet>) -> EditState {
    settle(session, result.id());
    let found = known(session, result);
    let confirm = found.entry.as_ref().is_some_and(|entry| super::safety::type_of(session, entry).confirm_data_change);
    let pending = result.pending();
    let edits = session.state::<Edits>();
    EditState {
        id: result.id(),
        message: found.editing.reason.map(reason_text),
        editing: found.editing,
        dirty: pending.any(),
        pending,
        confirm,
        saving: edits.and_then(|edits| edits.saving.get(&result.id()).copied()),
        last_save: edits.and_then(|edits| edits.outcomes.get(&result.id()).cloned()),
    }
}

/// Which result.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ResultParams {
    /// The result's id; the result the Database workspace shows in front when absent.
    pub id: Option<u64>,
}

pub struct EditStateCommand;

impl Command for EditStateCommand {
    type Params = ResultParams;
    type Output = EditState;
    const ID: &'static str = "db.result.editState";
    const TITLE: &'static str = "Result Editing";
    const DESCRIPTION: &'static str = "Whether a result (`id`, else the one shown in front) can be edited and why not (`reason`: `connection` read-only, `driver` cannot edit, `noRowIdentity` (ClickHouse: no row identity), `noTable`, `severalTables`, `unknownTable` while its structure is read, `noKey`: choose one with db.result.setRowKey), its table, the key that identifies rows (primary key, unique key of non-null columns, PostgreSQL's ctid, a saved virtual key or every column) and each column's name, type, nullability, default and whether it is editable; what is staged (cells, updated, inserted and deleted rows), whether Save asks for a confirmation (the connection type's), the save running and how the last one ended. Never data values.";

    fn run(&self, session: &mut Session, params: ResultParams) -> Result<EditState, CommandError> {
        let result = result_or_active(session, params.id)?;
        Ok(state_of(session, &result))
    }
}

/// A cell to change.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CellParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The row, from 0; rows after those read are new rows.
    pub row: usize,
    /// The column, from 0.
    pub column: usize,
    /// The new value as typed: checked against the column's type (numbers, booleans, JSON, UUIDs, bytes as `\x` and hex); dates, times and other types are read by the server.
    pub text: Option<String>,
    /// Set the cell to NULL.
    pub null: bool,
    /// Set the cell to the column's default.
    pub default: bool,
}

pub struct SetCell;

impl Command for SetCell {
    type Params = CellParams;
    type Output = EditState;
    const ID: &'static str = "db.result.setCell";
    const TITLE: &'static str = "Edit Cell";
    const DESCRIPTION: &'static str = "Stages a new value of cell (`row`, `column`) of a result (`id`, else the one shown): `text` as typed, checked against the column's type (code `invalidValue` with the reason), `null: true` for NULL or `default: true` for the column's default. Nothing is written until db.result.save; the value read back drops the change. Refused with code `readOnly` and the reason when the result cannot be edited, `readOnlyColumn` for a generated or computed column, `notNull` for NULL in a NOT NULL column. Answers the editing state.";

    fn run(&self, session: &mut Session, params: CellParams) -> Result<EditState, CommandError> {
        let result = result_or_active(session, params.id)?;
        let found = editable(session, &result)?;
        let column = found
            .editing
            .columns
            .get(params.column)
            .filter(|_| params.row < result.len())
            .ok_or_else(|| no_cell(&result, params.row, params.column))?;
        if !column.editable {
            return Err(CommandError::with_code(
                "readOnlyColumn",
                t!("database-error-read-only-column", column = column.name.as_str()),
            ));
        }
        let staged = if params.default {
            Staged::Default
        } else if params.null {
            if !column.nullable {
                return Err(CommandError::with_code(
                    "notNull",
                    t!("database-error-not-null", column = column.name.as_str()),
                ));
            }
            Staged::Value(Value::Null)
        } else {
            let text = params.text.unwrap_or_default();
            let value = edit::parse_cell(&found.editing, &text, &column.type_name)
                .map_err(|error| CommandError::with_code("invalidValue", input_error(error)))?;
            Staged::Value(value)
        };
        if !result.stage(params.row, params.column, staged) {
            return Err(no_cell(&result, params.row, params.column));
        }
        Ok(state_of(session, &result))
    }
}

/// A cell whose change to drop.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RevertCellParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The row, from 0.
    pub row: usize,
    /// The column, from 0.
    pub column: usize,
}

pub struct RevertCell;

impl Command for RevertCell {
    type Params = RevertCellParams;
    type Output = EditState;
    const ID: &'static str = "db.result.revertCell";
    const TITLE: &'static str = "Revert Cell";
    const DESCRIPTION: &'static str = "Drops the staged change of cell (`row`, `column`) of a result (`id`, else the one shown), as Esc does while editing it. Answers the editing state.";

    fn run(&self, session: &mut Session, params: RevertCellParams) -> Result<EditState, CommandError> {
        let result = result_or_active(session, params.id)?;
        refuse_while_saving(session, &result)?;
        result.unstage(params.row, params.column);
        Ok(state_of(session, &result))
    }
}

/// A row to add.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct AddRowParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// A row whose values the new row copies (Duplicate); key and identity columns keep their defaults.
    pub copy_of: Option<usize>,
}

/// A row added.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RowAdded {
    /// The new row, from 0.
    pub row: usize,
    /// The editing state.
    #[serde(flatten)]
    pub state: EditState,
}

fn add_row(session: &mut Session, id: Option<u64>, copy_of: Option<usize>) -> Result<RowAdded, CommandError> {
    let result = result_or_active(session, id)?;
    let found = editable(session, &result)?;
    let key: Vec<usize> = found
        .editing
        .key
        .as_ref()
        .filter(|key| matches!(key.kind, KeyKind::PrimaryKey | KeyKind::UniqueKey))
        .map(|key| key.columns.clone())
        .unwrap_or_default();
    let cells: Vec<(usize, Staged)> = match copy_of {
        Some(row) if row < result.len() => found
            .editing
            .columns
            .iter()
            .filter(|column| {
                column.editable && !key.contains(&column.index) && column.default.as_deref() != Some("identity")
            })
            .map(|column| (column.index, Staged::Value(result.get(row, column.index))))
            .collect(),
        Some(row) => return Err(no_cell(&result, row, 0)),
        None => Vec::new(),
    };
    let row = result.add_row(cells);
    Ok(RowAdded { row, state: state_of(session, &result) })
}

pub struct AddRow;

impl Command for AddRow {
    type Params = AddRowParams;
    type Output = RowAdded;
    const ID: &'static str = "db.result.addRow";
    const TITLE: &'static str = "Add Row";
    const DESCRIPTION: &'static str = "Adds a new row after the rows of a result (`id`, else the one shown), staged until db.result.save; its cells take their columns' defaults until set with db.result.setCell. With `copyOf`, it copies that row's values except key and identity columns (Duplicate). Answers the new row and the editing state.";

    fn run(&self, session: &mut Session, params: AddRowParams) -> Result<RowAdded, CommandError> {
        add_row(session, params.id, params.copy_of)
    }
}

/// A row to duplicate.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DuplicateParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The row to copy, from 0.
    pub row: usize,
}

pub struct DuplicateRow;

impl Command for DuplicateRow {
    type Params = DuplicateParams;
    type Output = RowAdded;
    const ID: &'static str = "db.result.duplicateRow";
    const TITLE: &'static str = "Duplicate Row";
    const DESCRIPTION: &'static str = "Adds a new row that copies row `row` of a result (`id`, else the one shown) except its key and identity columns, staged until db.result.save. Answers the new row and the editing state.";

    fn run(&self, session: &mut Session, params: DuplicateParams) -> Result<RowAdded, CommandError> {
        add_row(session, params.id, Some(params.row))
    }
}

/// Rows to delete.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DeleteRowsParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The rows, from 0.
    pub rows: Vec<usize>,
    /// Take the rows back out of those to delete instead.
    pub restore: bool,
}

pub struct DeleteRows;

impl Command for DeleteRows {
    type Params = DeleteRowsParams;
    type Output = EditState;
    const ID: &'static str = "db.result.deleteRows";
    const TITLE: &'static str = "Delete Rows";
    const DESCRIPTION: &'static str = "Marks `rows` of a result (`id`, else the one shown) to be deleted on db.result.save (they stay shown, struck through); new rows go at once; `restore: true` unmarks them. Answers the editing state.";

    fn run(&self, session: &mut Session, params: DeleteRowsParams) -> Result<EditState, CommandError> {
        let result = result_or_active(session, params.id)?;
        if params.restore {
            refuse_while_saving(session, &result)?;
            result.restore_rows(&params.rows);
        } else {
            editable(session, &result)?;
            result.delete_rows(&params.rows);
        }
        Ok(state_of(session, &result))
    }
}

/// Cells to copy from the row above.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CopyAboveParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The row, from 1.
    pub row: usize,
    /// The columns, from 0; every editable column when empty.
    pub columns: Vec<usize>,
}

pub struct CopyFromAbove;

impl Command for CopyFromAbove {
    type Params = CopyAboveParams;
    type Output = EditState;
    const ID: &'static str = "db.result.copyFromAbove";
    const TITLE: &'static str = "Copy from Row Above";
    const DESCRIPTION: &'static str = "Stages, in row `row` of a result (`id`, else the one shown), the values the row above has in `columns` (every editable column when empty; key columns of a primary or unique key are skipped then). Answers the editing state.";

    fn run(&self, session: &mut Session, params: CopyAboveParams) -> Result<EditState, CommandError> {
        let result = result_or_active(session, params.id)?;
        let found = editable(session, &result)?;
        if params.row == 0 || params.row >= result.len() {
            return Err(no_cell(&result, params.row, 0));
        }
        let key: Vec<usize> = found.editing.key.as_ref().map(|key| key.columns.clone()).unwrap_or_default();
        let columns: Vec<usize> = if params.columns.is_empty() {
            found
                .editing
                .columns
                .iter()
                .filter(|column| column.editable && !key.contains(&column.index))
                .map(|column| column.index)
                .collect()
        } else {
            params
                .columns
                .into_iter()
                .filter(|index| found.editing.columns.get(*index).is_some_and(|column| column.editable))
                .collect()
        };
        for column in columns {
            result.stage(params.row, column, Staged::Value(result.get(params.row - 1, column)));
        }
        Ok(state_of(session, &result))
    }
}

/// How much of the preview to answer.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// The most statements; 200 when absent.
    pub limit: Option<usize>,
}

/// The statements Save will run.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewAnswer {
    /// The statements with their values written as literals, one per line, each ending with `;`.
    pub sql: String,
    /// Each statement with placeholders for its bound parameters, as it is sent.
    pub statements: Vec<edit::Planned>,
    /// How many statements Save runs.
    pub total: usize,
    /// Whether statements were left out because of `limit`.
    pub truncated: bool,
    /// Whether Save asks for a confirmation after the preview.
    pub confirm: bool,
}

fn planned(session: &mut Session, result: &ResultSet) -> Result<(Vec<edit::Planned>, Known), CommandError> {
    let found = editable(session, result)?;
    let planned = edit::plan(&found.editing, &result.changes());
    Ok((planned, found))
}

fn preview_of(planned: &[edit::Planned], limit: usize, confirm: bool) -> PreviewAnswer {
    let shown: Vec<edit::Planned> = planned.iter().take(limit).cloned().collect();
    PreviewAnswer {
        sql: edit::preview(&shown),
        statements: shown,
        total: planned.len(),
        truncated: planned.len() > limit,
        confirm,
    }
}

fn confirms(session: &Session, entry: Option<&Entry>) -> bool {
    entry.is_some_and(|entry| super::safety::type_of(session, entry).confirm_data_change)
}

pub struct PreviewChanges;

impl Command for PreviewChanges {
    type Params = PreviewParams;
    type Output = PreviewAnswer;
    const ID: &'static str = "db.result.previewChanges";
    const TITLE: &'static str = "Preview Changes";
    const DESCRIPTION: &'static str = "The statements db.result.save would run for the staged changes of a result (`id`, else the one shown), in order: deletes, inserts, updates, one per row; an UPDATE sets only the changed columns and finds its row by the old key, checking that the changed cells still hold the values read. `sql` writes the values as literals for reading; `statements` show the placeholders of the real bind parameters (their values are never answered). At most `limit` (200) statements, with `total` and `truncated`.";

    fn run(&self, session: &mut Session, params: PreviewParams) -> Result<PreviewAnswer, CommandError> {
        let result = result_or_active(session, params.id)?;
        let (planned, found) = planned(session, &result)?;
        let confirm = confirms(session, found.entry.as_ref());
        Ok(preview_of(&planned, params.limit.unwrap_or(PREVIEWED), confirm))
    }
}

/// Save, confirmed or not.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveParams {
    /// The result's id; the result shown in front when absent.
    pub id: Option<u64>,
    /// Confirms the save after the preview, where the connection type asks for it.
    pub confirmed: bool,
}

/// A save started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveStarted {
    /// The operation saving, for db.operation.state; absent when nothing was staged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<u64>,
    /// The statements it runs.
    pub statements: usize,
}

pub struct SaveChanges;

impl Command for SaveChanges {
    type Params = SaveParams;
    type Output = SaveStarted;
    const ID: &'static str = "db.result.save";
    const TITLE: &'static str = "Save Data Changes";
    const DESCRIPTION: &'static str = "Saves the staged changes of a result (`id`, else the one shown) in the background, in the result's session: the statements of db.result.previewChanges with real bind parameters, in one transaction that commits; inside an open transaction or in manual commit mode, under a savepoint that is released and left in the transaction for you to commit or roll back. A statement that changes no row (the row was changed or deleted by someone else) stops the save as a conflict; the first error rolls every change back and the changes stay staged. Saved rows are read back from the server. Where the connection type confirms data changes (Test, Production) it is refused with code `confirmDataChange` and the preview until `confirmed: true`. db.result.editState tells how it ended.";

    fn run(&self, session: &mut Session, params: SaveParams) -> Result<SaveStarted, CommandError> {
        let result = result_or_active(session, params.id)?;
        settle(session, result.id());
        if !result.pending().any() {
            return Ok(SaveStarted { operation: None, statements: 0 });
        }
        let (planned, found) = planned(session, &result)?;
        if let Some(entry) = found.entry.as_ref() {
            super::safety::agent_changes(session, entry)?;
        }
        if confirms(session, found.entry.as_ref()) && !params.confirmed {
            let preview = preview_of(&planned, PREVIEWED, true);
            return Err(CommandError::with_code(
                "confirmDataChange",
                t!("database-error-confirm-data-change", count = planned.len()),
            )
            .with_data(serde_json::json!({ "preview": preview.sql, "statements": preview.total })));
        }
        let statements = planned.len();
        let engine = engine(session);
        let provenance = super::history::provenance(session, zen_db::history::Source::Save, None);
        let op = engine.save_edits_from(result.id(), planned, provenance).map_err(|error| {
            CommandError::with_code("databaseError", describe(&error)).with_data(serde_json::json!({ "error": error }))
        })?;
        let edits = session.state_mut::<Edits>();
        edits.saving.insert(result.id(), op);
        edits.outcomes.remove(&result.id());
        Ok(SaveStarted { operation: Some(op), statements })
    }
}

pub struct RevertChanges;

impl Command for RevertChanges {
    type Params = ResultParams;
    type Output = EditState;
    const ID: &'static str = "db.result.revert";
    const TITLE: &'static str = "Cancel Data Changes";
    const DESCRIPTION: &'static str = "Drops every staged change of a result (`id`, else the one shown): edited cells, new rows and rows marked to delete. Nothing is sent to the server. Answers the editing state.";

    fn run(&self, session: &mut Session, params: ResultParams) -> Result<EditState, CommandError> {
        let result = result_or_active(session, params.id)?;
        refuse_while_saving(session, &result)?;
        result.drop_edits();
        session.state_mut::<Edits>().outcomes.remove(&result.id());
        Ok(state_of(session, &result))
    }
}

/// The key to identify rows by.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RowKeyParams {
    /// The result's id; the result shown in front when absent.
    #[serde(default)]
    pub id: Option<u64>,
    /// `{ "kind": "columns", "columns": ["code"] }` for a custom key of the table's columns, or `{ "kind": "allColumns" }` (several rows may change at once).
    pub key: KeyChoice,
    /// Keep a custom key for the table in the project's `.zen/databases/virtual-keys.json`.
    #[serde(default)]
    pub remember: bool,
}

pub struct SetRowKey;

impl Command for SetRowKey {
    type Params = RowKeyParams;
    type Output = EditState;
    const ID: &'static str = "db.result.setRowKey";
    const TITLE: &'static str = "Choose Row Key";
    const DESCRIPTION: &'static str = "For a result whose table has no primary or unique non-null key in it (reason `noKey`), chooses what identifies its rows: a custom key of columns (`{kind: columns, columns}`, kept for the table in the project's `.zen/databases/virtual-keys.json` with `remember`), or every column (`{kind: allColumns}`, for this result only: a save that would change several equal rows is rolled back with code `severalRows`). Answers the editing state.";

    fn run(&self, session: &mut Session, params: RowKeyParams) -> Result<EditState, CommandError> {
        let result = result_or_active(session, params.id)?;
        let found = known(session, &result);
        if let KeyChoice::Columns(columns) = &params.key {
            let names: Vec<&str> = found
                .editing
                .columns
                .iter()
                .filter(|column| column.sourced)
                .map(|column| column.name.as_str())
                .collect();
            if columns.is_empty() || columns.iter().any(|column| !names.contains(&column.as_str())) {
                return Err(CommandError::with_code(
                    "invalidKey",
                    t!("database-error-invalid-key", columns = names.join(", ")),
                ));
            }
            if params.remember {
                let key = found.table_key.clone().ok_or_else(|| not_editable(&found.editing))?;
                let file = keys_file(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
                let mut kept = saved_keys(session);
                kept.version = 1;
                kept.keys.insert(key, columns.clone());
                zen_db::registry::write_json(&file, &kept)
                    .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
            }
        }
        session.state_mut::<Edits>().choices.insert(result.id(), params.key);
        Ok(state_of(session, &result))
    }
}

/// Rows to generate SQL from.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateParams {
    /// The result's id; the result shown in front when absent.
    #[serde(default)]
    pub id: Option<u64>,
    /// `select`, `insert`, `update` or `delete`.
    pub kind: Generate,
    /// The first and last row, both included.
    pub rows: (usize, usize),
}

/// SQL generated from rows.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Generated {
    /// The statements, one per row, values written as literals.
    pub sql: String,
    /// The rows it covers.
    pub rows: usize,
    /// Whether rows were left out: at most 1000.
    pub truncated: bool,
}

pub struct GenerateSql;

impl Command for GenerateSql {
    type Params = GenerateParams;
    type Output = Generated;
    const ID: &'static str = "db.result.generateSql";
    const TITLE: &'static str = "Generate SQL";
    const DESCRIPTION: &'static str = "Writes SQL for rows `rows` ([first, last]) of a result (`id`, else the one shown) as text, nothing run: `select` each row by its key, `insert` it with its editable columns, `update` its non-key columns by its key, or `delete` it by its key (every column when the table has no key); values as SQL literals; at most 1000 rows. Needs the rows' table (code `readOnly` with the reason otherwise). The window copies it; agents get it.";

    fn run(&self, session: &mut Session, params: GenerateParams) -> Result<Generated, CommandError> {
        let result = result_or_active(session, params.id)?;
        let found = known(session, &result);
        if found.editing.table.is_none() {
            return Err(not_editable(&found.editing));
        }
        let (first, last) = params.rows;
        let last = last.min(result.len().saturating_sub(1));
        let count = if result.is_empty() || first > last { 0 } else { last - first + 1 };
        let taken = count.min(GENERATED);
        let rows = result.page(first, taken);
        let sql = edit::generate(&found.editing, params.kind, &rows).unwrap_or_default();
        Ok(Generated { sql, rows: taken, truncated: taken < count })
    }
}

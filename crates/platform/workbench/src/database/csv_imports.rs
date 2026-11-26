use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::csv_import::{
    ColumnMapping, CsvFormat, Kind, LoadSettings, SAMPLE_LIMIT, create_statement, insert_head, sample,
    truncate_statement,
};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{ImportRequest, ImportState, OperationStatus, QueryStatement, RowError};
use zen_db::schema::{Object, ObjectKind, Snapshot, qualified_for};
use zen_i18n::t;

use super::connections::describe;
use super::consoles::{engine, ensure_connected};
use super::safety::{Confirmation, Run, SKIPPED};
use super::sources::find;
use super::structure::Structure;
use crate::notifications::{Group, Kind as NoteKind, NewNotification, post};
use crate::workspaces::Workspaces;

const LISTED: usize = 20;
const SHOWN_TEXT: usize = 200;

/// The import settings kept in `<data folder>/databases/import.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Remembered {
    /// The file's version, 1.
    pub version: u32,
    /// How files are written.
    pub format: CsvFormat,
    /// How rows are loaded.
    pub load: LoadSettings,
    /// The folder the last file was in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
}

impl Default for Remembered {
    fn default() -> Self {
        Self { version: 1, format: CsvFormat::default(), load: LoadSettings::default(), folder: None }
    }
}

#[derive(Default)]
pub struct CsvImports {
    remembered: Option<Remembered>,
    started: Vec<u64>,
    announced: BTreeSet<u64>,
    targets: HashMap<u64, (String, Vec<String>)>,
}

fn file_path(session: &Session) -> Option<PathBuf> {
    session.state::<Workspaces>().and_then(Workspaces::data_dir).map(|data| data.join("databases").join("import.json"))
}

pub fn remembered(session: &mut Session) -> Remembered {
    if let Some(remembered) = session.state::<CsvImports>().and_then(|imports| imports.remembered.clone()) {
        return remembered;
    }
    let read = file_path(session)
        .and_then(|file| zen_db::registry::read_json::<Remembered>(&file).ok().flatten())
        .unwrap_or_default();
    session.state_mut::<CsvImports>().remembered = Some(read.clone());
    read
}

fn remember(session: &mut Session, remembered: Remembered) -> Result<(), CommandError> {
    if session.state::<CsvImports>().and_then(|imports| imports.remembered.as_ref()) == Some(&remembered) {
        return Ok(());
    }
    if let Some(path) = file_path(session) {
        zen_db::registry::write_json(&path, &remembered)
            .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
    }
    session.state_mut::<CsvImports>().remembered = Some(remembered);
    Ok(())
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        _ => error.code.as_deref().unwrap_or("databaseError"),
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

fn format_problem(format: &CsvFormat) -> Result<(), CommandError> {
    match format.problem() {
        Some(problem) => Err(CommandError::with_code(problem, t!("database-error-import-delimiter"))),
        None => Ok(()),
    }
}

fn read_sample(file: &std::path::Path, format: &CsvFormat) -> Result<zen_db::csv_import::Sample, CommandError> {
    if !file.is_file() {
        return Err(CommandError::with_code(
            "noFile",
            t!("database-error-import-no-file", path = crate::display(file)),
        ));
    }
    sample(file, format, SAMPLE_LIMIT).map_err(|error| {
        CommandError::with_code(
            "io",
            t!("database-error-import-read", path = crate::display(file), error = error.to_string()),
        )
    })
}

/// A column of the target table as the mapping step shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TargetColumn {
    /// Its name.
    pub name: String,
    /// Its type as the server writes it.
    #[serde(rename = "type")]
    pub type_name: String,
    /// How fields are converted for it.
    pub kind: Kind,
}

/// The table an import goes into.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// Its path in the connection's tree.
    pub path: Vec<String>,
    /// Its name, quoted and qualified as the import writes it.
    pub qualified: String,
    /// Whether it is in the connection's snapshot.
    pub exists: bool,
    /// Its columns, when they are known.
    pub columns: Vec<TargetColumn>,
    /// Whether its columns are being read.
    pub loading: bool,
}

struct Resolved {
    entry: zen_db::registry::Entry,
    dialect: SqlDialect,
    target: Target,
}

fn table_parts(path: &[String]) -> Result<(String, String), CommandError> {
    match path {
        [_, schema, folder, name] if folder == ObjectKind::Table.folder() && !name.trim().is_empty() => {
            Ok((schema.clone(), name.trim().to_owned()))
        }
        _ => Err(CommandError::with_code("notATable", t!("database-error-import-not-table", path = path.join("/")))),
    }
}

fn snapshot_object(snapshot: Option<&Snapshot>, path: &[String]) -> Option<Object> {
    let snapshot = snapshot?;
    let found = zen_db::schema::find_object(snapshot, path)?;
    Some(found.object.clone())
}

fn resolve(session: &mut Session, connection: &str, path: &[String], load: bool) -> Result<Resolved, CommandError> {
    let entry = find(session, connection)?;
    let (schema, name) = table_parts(path)?;
    let dialect = super::filters::dialect_of(session, connection)
        .ok_or_else(|| CommandError::with_code("unsupported", t!("database-error-import-unsupported")))?;
    super::structure::ensure(session, &entry, true);
    let object = snapshot_object(session.state::<Structure>().and_then(|known| known.snapshot(connection)), path);
    let mut loading = false;
    if load && object.as_ref().is_some_and(|object| !object.detailed) {
        loading = super::structure::details(session, &entry, path)?.is_some();
    }
    let columns = object
        .iter()
        .flat_map(|object| object.columns.iter())
        .filter(|column| column.generated.is_none())
        .map(|column| TargetColumn {
            name: column.name.clone(),
            type_name: column.type_name.clone(),
            kind: Kind::of_type(&column.type_name),
        })
        .collect();
    let target = Target {
        path: path.to_vec(),
        qualified: qualified_for(&entry.source.driver, &schema, &name),
        exists: object.is_some(),
        columns,
        loading,
    };
    Ok(Resolved { entry, dialect, target })
}

fn proposed(
    sample: &zen_db::csv_import::Sample,
    target: &Target,
    create: bool,
    dialect: SqlDialect,
    header: bool,
) -> Vec<ColumnMapping> {
    let names = &sample.columns;
    if create || !target.exists {
        return names
            .iter()
            .enumerate()
            .map(|(at, name)| {
                let kind = sample.kinds.get(at).copied().unwrap_or_default();
                ColumnMapping {
                    source: at,
                    target: name.clone(),
                    kind,
                    sql_type: Some(kind.sql_type(dialect).to_owned()),
                }
            })
            .collect();
    }
    if target.columns.is_empty() {
        if !header {
            return Vec::new();
        }
        return names
            .iter()
            .enumerate()
            .map(|(at, name)| ColumnMapping { source: at, target: name.clone(), kind: Kind::Text, sql_type: None })
            .collect();
    }
    names
        .iter()
        .enumerate()
        .filter_map(|(at, name)| {
            let column = if header {
                target.columns.iter().find(|column| column.name.eq_ignore_ascii_case(name.trim()))
            } else {
                target.columns.get(at)
            }?;
            Some(ColumnMapping { source: at, target: column.name.clone(), kind: column.kind, sql_type: None })
        })
        .collect()
}

/// What `db.csvImport.preview` reads.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewParams {
    /// The CSV file.
    pub file: PathBuf,
    /// How it is written; the remembered format when absent.
    pub format: Option<CsvFormat>,
    /// The most rows answered (20, at most 100); kinds are guessed from the first 100 rows.
    pub limit: Option<usize>,
    /// The connection whose table the rows go into, to propose a mapping.
    pub connection: Option<String>,
    /// The table's path, `[db, schema, "tables", name]`.
    pub table: Option<Vec<String>>,
    /// The table is created by the import.
    pub create: bool,
}

/// A column of the file.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileColumn {
    /// Its name: the header's, else `column1`…
    pub name: String,
    /// What its first rows seem to hold.
    pub kind: Kind,
}

/// The first rows of a file and the mapping proposed.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The file's columns.
    pub columns: Vec<FileColumn>,
    /// The rows shown, each field `null` for `NULL`, text cut to 200 characters.
    pub rows: Vec<Vec<Option<String>>>,
    /// Whether the file has more rows than are shown.
    pub truncated: bool,
    /// The file's size in bytes.
    pub file_bytes: u64,
    /// The format read with.
    pub format: CsvFormat,
    /// The target table, when `connection` and `table` were given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
    /// The mapping proposed: the file's columns by name onto the table's, or every one for a new table with a type from what it holds.
    pub mapping: Vec<ColumnMapping>,
}

pub struct PreviewImport;

impl Command for PreviewImport {
    type Params = PreviewParams;
    type Output = Preview;
    const ID: &'static str = "db.csvImport.preview";
    const TITLE: &'static str = "Preview CSV Import";
    const DESCRIPTION: &'static str = "Reads the first rows of CSV `file` with `format` (the remembered one when absent: `delimiter`, `quote`, `escape?`, `header`, `nullText`, `timestampFormat?` in strftime or Java style) and answers its columns with the kind each seems to hold (text, integer, decimal, boolean, date, timestamp; guessed from 100 rows), at most `limit` rows (20, at most 100; `truncated`), and, with `connection` and `table` (`[db, schema, \"tables\", name]`), the table's columns (`loading` while they are read) and the mapping proposed (`create` for a new table). Never connects.";

    fn run(&self, session: &mut Session, params: PreviewParams) -> Result<Preview, CommandError> {
        let format = params.format.unwrap_or_else(|| remembered(session).format);
        format_problem(&format)?;
        super::guard_agent_file(session, &params.file)?;
        let read = read_sample(&params.file, &format)?;
        let limit = params.limit.unwrap_or(LISTED).clamp(1, SAMPLE_LIMIT);
        let resolved = match (&params.connection, &params.table) {
            (Some(connection), Some(table)) => Some(resolve(session, connection, table, true)?),
            _ => None,
        };
        let mapping = resolved
            .as_ref()
            .map(|resolved| proposed(&read, &resolved.target, params.create, resolved.dialect, format.header))
            .unwrap_or_default();
        let truncated = read.more || read.rows.len() > limit;
        let rows = read
            .rows
            .iter()
            .take(limit)
            .map(|row| {
                row.iter().map(|field| field.as_ref().map(|text| text.chars().take(SHOWN_TEXT).collect())).collect()
            })
            .collect();
        let columns = read
            .columns
            .iter()
            .zip(&read.kinds)
            .map(|(name, kind)| FileColumn { name: name.clone(), kind: *kind })
            .collect();
        Ok(Preview {
            columns,
            rows,
            truncated,
            file_bytes: read.file_bytes,
            format,
            target: resolved.map(|resolved| resolved.target),
            mapping,
        })
    }
}

/// A column of the table and the field it takes, as `db.csvImport.start` is given it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MappingParam {
    /// The field's position in a record, from 0.
    pub source: usize,
    /// The column's name.
    pub target: String,
    /// How the field is converted; from the column's type when it is known, else text, when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// The column's type for a new table; from the kind when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql_type: Option<String>,
}

/// What `db.csvImport.start` imports and how.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StartParams {
    /// The connection's id.
    pub connection: String,
    /// The CSV file.
    pub file: PathBuf,
    /// The table's path, `[db, schema, "tables", name]`.
    pub table: Vec<String>,
    /// Create the table first, with the mapping's columns and types.
    pub create: bool,
    /// How the file is written; the remembered format when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<CsvFormat>,
    /// The columns filled; the proposed mapping when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<MappingParam>>,
    /// How rows are loaded; the remembered settings when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load: Option<LoadSettings>,
    /// The `token` of a `confirmExecute` refusal, to go on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<String>,
    /// Keep the format and load settings for next time; true when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remember: Option<bool>,
}

/// An import started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The import's id, for `db.csvImport.state` and `db.csvImport.cancel`.
    pub id: u64,
    /// The table, quoted and qualified.
    pub table: String,
    /// The statements run before the rows: `CREATE TABLE`, `TRUNCATE TABLE`.
    pub statements: Vec<String>,
    /// The columns filled.
    pub columns: Vec<ColumnMapping>,
    /// Whether the connection was connected first.
    pub connecting: bool,
}

fn mapping_of(
    params: Option<Vec<MappingParam>>,
    proposal: Vec<ColumnMapping>,
    target: &Target,
    dialect: SqlDialect,
    width: usize,
) -> Result<Vec<ColumnMapping>, CommandError> {
    let Some(given) = params else { return Ok(proposal) };
    let mut seen = BTreeSet::new();
    given
        .into_iter()
        .map(|column| {
            if column.source >= width.max(1) || column.target.trim().is_empty() {
                return Err(CommandError::with_code(
                    "badMapping",
                    t!("database-error-import-mapping", column = column.target.as_str()),
                ));
            }
            if !seen.insert(column.target.to_lowercase()) {
                return Err(CommandError::with_code(
                    "badMapping",
                    t!("database-error-import-mapping", column = column.target.as_str()),
                ));
            }
            let known = target.columns.iter().find(|known| known.name.eq_ignore_ascii_case(&column.target));
            let kind = column.kind.or(known.map(|known| known.kind)).unwrap_or_default();
            let sql_type = column.sql_type.filter(|sql_type| !sql_type.trim().is_empty());
            if let Some(sql_type) = sql_type.as_deref().filter(|sql_type| !zen_db::csv_import::plain_sql_type(sql_type))
            {
                return Err(CommandError::with_code(
                    "badSqlType",
                    t!("database-error-import-sql-type", column = column.target.as_str(), sql_type = sql_type),
                ));
            }
            Ok(ColumnMapping {
                source: column.source,
                target: known.map_or_else(|| column.target.trim().to_owned(), |known| known.name.clone()),
                kind,
                sql_type: sql_type.or_else(|| Some(kind.sql_type(dialect).to_owned())),
            })
        })
        .collect()
}

pub struct StartImport;

impl Command for StartImport {
    type Params = StartParams;
    type Output = Started;
    const ID: &'static str = "db.csvImport.start";
    const TITLE: &'static str = "Import from CSV";
    const DESCRIPTION: &'static str = "Imports CSV `file` into `table` (`[db, schema, \"tables\", name]`) of `connection` in the background, the file streamed and never read whole. `create` creates the table first from the mapping (refused with code `exists` when it is known). `format` as db.csvImport.preview's; `columns` `[{ source, target, kind?, sqlType? }]` (the field's position, the column, how it converts: text, integer, decimal, boolean, date, timestamp) or the proposed mapping; `load` `{ batch (1000 rows a statement), commitEvery (0: one transaction for the whole file), truncate (empty the table first, in the same transaction), onError (stop: roll back what is not committed; skip: note the row and go on), serverCopy (PostgreSQL's COPY FROM STDIN) }`. Read-only connections refuse (`readOnly`); a connection type that confirms changes, and `truncate`, ask first (`confirmExecute` with a `token` and `retry`); agents follow the type's agent access. Connects first with the stored password. Answers the import's id at once; follow it with db.csvImport.state.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<Started, CommandError> {
        let known = remembered(session);
        let format = params.format.clone().unwrap_or_else(|| known.format.clone());
        format_problem(&format)?;
        let mut load = params.load.clone().unwrap_or_else(|| known.load.clone());
        load.batch = load.batch.clamp(1, 100_000);
        super::guard_agent_file(session, &params.file)?;
        let read = read_sample(&params.file, &format)?;
        let resolved = resolve(session, &params.connection, &params.table, false)?;
        if params.create && resolved.target.exists {
            return Err(CommandError::with_code(
                "exists",
                t!("database-error-import-table-exists", table = resolved.target.qualified.as_str()),
            ));
        }
        let dialect = resolved.dialect;
        let proposal = proposed(&read, &resolved.target, params.create, dialect, format.header);
        let columns = mapping_of(params.columns.clone(), proposal, &resolved.target, dialect, read.columns.len())?;
        if columns.is_empty() {
            return Err(CommandError::with_code(
                "noColumns",
                t!("database-error-import-no-columns", table = resolved.target.qualified.as_str()),
            ));
        }
        let table = resolved.target.qualified.clone();
        let mut prelude = Vec::new();
        if params.create {
            let defined: Vec<(String, String)> = columns
                .iter()
                .map(|column| {
                    let sql_type = column.sql_type.clone().unwrap_or_else(|| column.kind.sql_type(dialect).to_owned());
                    (column.target.clone(), sql_type)
                })
                .collect();
            prelude.push(create_statement(dialect, &table, &defined));
        } else if load.truncate {
            prelude.push(truncate_statement(dialect, &table));
        }
        let names: Vec<String> = columns.iter().map(|column| column.target.clone()).collect();
        let head = insert_head(dialect, &table, &names);
        let statements: Vec<QueryStatement> = prelude
            .iter()
            .chain(std::iter::once(&head))
            .map(|text| QueryStatement {
                text: text.clone(),
                start: 0,
                end: text.len(),
                client: false,
                params: Vec::new(),
                kept: None,
            })
            .collect();
        let context = ContextKind::Console("import".to_owned());
        let mut provenance = super::history::provenance(session, zen_db::history::Source::Import, None);
        let confirmation = Confirmation { confirmed: params.confirmed.clone(), skip: Vec::new() };
        let mut retry = serde_json::to_value(&params).unwrap_or_default();
        if let Some(object) = retry.as_object_mut() {
            object.remove("confirmed");
        }
        let entry = resolved.entry.clone();
        let run = Run {
            entry: &entry,
            context: &context,
            provenance: &provenance,
            confirmation: &confirmation,
            retry: (Self::ID, retry),
        };
        let guarded = super::safety::guard(session, &run, statements)?;
        provenance.confirmed = guarded.confirmed;
        if !guarded.statements.iter().any(|statement| statement.text == head) {
            return Err(CommandError::with_code(SKIPPED, t!("database-error-all-skipped")));
        }
        let prelude: Vec<String> = guarded
            .statements
            .iter()
            .filter(|statement| statement.text != head)
            .map(|statement| statement.text.clone())
            .collect();
        let connecting = ensure_connected(session, &params.connection)?;
        let request = ImportRequest {
            connection: params.connection.clone(),
            file: params.file.clone(),
            format: format.clone(),
            dialect,
            table: table.clone(),
            name: params.table.last().cloned().unwrap_or_default(),
            columns: columns.clone(),
            prelude: prelude.clone(),
            load: load.clone(),
        };
        let id = engine(session).import_csv(request, provenance).map_err(|error| failed(&error))?;
        let imports = session.state_mut::<CsvImports>();
        imports.started.push(id);
        imports.targets.insert(id, (params.connection.clone(), params.table.clone()));
        if params.remember.unwrap_or(true) {
            let folder = params.file.parent().map(std::path::Path::to_path_buf).or(known.folder);
            remember(session, Remembered { version: 1, format, load, folder })?;
        }
        Ok(Started { id, table, statements: prelude, columns, connecting })
    }
}

pub struct ImportSettings;

impl Command for ImportSettings {
    type Params = zen_core::NoParams;
    type Output = Remembered;
    const ID: &'static str = "db.csvImport.settings";
    const TITLE: &'static str = "CSV Import Settings";
    const DESCRIPTION: &'static str = "The CSV import settings remembered from the last import (`<data folder>/databases/import.json`): the file format, the load settings and the folder of the last file.";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<Remembered, CommandError> {
        Ok(remembered(session))
    }
}

/// Which import to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// The import; the imports of this window, newest first, when absent.
    pub id: Option<u64>,
    /// How long to wait for it to end, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
    /// The most imports listed, and the most row errors each answers; 20 when absent.
    pub limit: Option<usize>,
}

/// Imports as agents read them; never their rows.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportsAnswer {
    /// The imports, newest first, each with at most `limit` row errors (`errorsTotal` counts them all).
    pub imports: Vec<ImportState>,
    /// Whether imports were left out because of `limit`.
    pub truncated: bool,
}

pub struct ImportStateCommand;

impl Command for ImportStateCommand {
    type Params = StateParams;
    type Output = ImportsAnswer;
    const ID: &'static str = "db.csvImport.state";
    const TITLE: &'static str = "CSV Import State";
    const DESCRIPTION: &'static str = "The progress of import `id` (waiting up to `waitMs`, 60 000 at most, for it to end), or the window's imports newest first (at most `limit`, 20): status (running, done, failed, cancelled), bytes of the file read and its size, rows read, inserted, committed and skipped, whether PostgreSQL's COPY was used, and the rows that failed (`errors`: row, line, column, code, the server's detail; at most `limit`, `errorsTotal` counts them). Never values of rows.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<ImportsAnswer, CommandError> {
        let engine = engine(session);
        let limit = params.limit.unwrap_or(LISTED).max(1);
        let cut = |mut state: ImportState| {
            state.errors.truncate(limit);
            state
        };
        if let Some(id) = params.id {
            if let Some(wait) = params.wait_ms {
                engine.wait(id, std::time::Duration::from_millis(wait.min(60_000)));
            }
            let state = engine.import_state(id).ok_or_else(|| {
                CommandError::with_code("unknownImport", t!("database-error-unknown-import", id = id))
            })?;
            return Ok(ImportsAnswer { imports: vec![cut(state)], truncated: false });
        }
        let ids = engine.imports();
        let imports: Vec<ImportState> =
            ids.iter().rev().take(limit).filter_map(|id| engine.import_state(*id)).map(cut).collect();
        Ok(ImportsAnswer { truncated: ids.len() > imports.len(), imports })
    }
}

/// Which import to cancel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CancelParams {
    /// The import.
    pub id: u64,
}

/// Whether an import was cancelled.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Whether it was running and is being cancelled.
    pub cancelled: bool,
}

pub struct CancelImport;

impl Command for CancelImport {
    type Params = CancelParams;
    type Output = Cancelled;
    const ID: &'static str = "db.csvImport.cancel";
    const TITLE: &'static str = "Cancel CSV Import";
    const DESCRIPTION: &'static str = "Cancels import `id`: the statement running stops on the server and what is not committed is rolled back (all of it when the import runs in one transaction). Answers whether it was running.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<Cancelled, CommandError> {
        Ok(Cancelled { cancelled: engine(session).cancel(params.id) })
    }
}

pub fn row_error_text(error: &RowError) -> String {
    let reason = match error.code.as_str() {
        "columns" => t!("database-import-row-columns"),
        "notANumber" => t!("database-import-row-number", column = error.column.clone().unwrap_or_default()),
        "notABoolean" => t!("database-import-row-boolean", column = error.column.clone().unwrap_or_default()),
        "notADate" => t!("database-import-row-date", column = error.column.clone().unwrap_or_default()),
        _ => error.detail.clone(),
    };
    t!("database-import-row", row = error.row, line = error.line, reason = reason)
}

fn announce(session: &mut Session, state: &ImportState) {
    let table = state.table.as_str();
    let errors: Vec<String> = state.errors.iter().take(3).map(row_error_text).collect();
    let (kind, body) = match state.status {
        OperationStatus::Running => return,
        OperationStatus::Cancelled => {
            (NoteKind::Info, t!("database-import-cancelled", table = table, committed = state.rows_committed))
        }
        OperationStatus::Failed => {
            let reason = state.error.as_ref().map(describe).unwrap_or_default();
            (
                NoteKind::Error,
                t!("database-import-failed", table = table, reason = reason, committed = state.rows_committed),
            )
        }
        OperationStatus::Done if state.rows_skipped > 0 => (
            NoteKind::Warning,
            t!(
                "database-import-done-skipped",
                table = table,
                rows = state.rows_inserted,
                skipped = state.rows_skipped,
                errors = errors.join("\n")
            ),
        ),
        OperationStatus::Done => {
            (NoteKind::Info, t!("database-import-done", table = table, rows = state.rows_inserted))
        }
    };
    post(
        session,
        NewNotification::message(Group::General, kind, body).titled_by(zen_i18n::Message::new("database-import-title")),
    );
}

pub fn step(session: &mut Session) {
    let Some(engine) = session.state::<super::Database>().and_then(super::Database::started).cloned() else { return };
    let waiting: Vec<u64> = session
        .state::<CsvImports>()
        .map(|imports| imports.started.iter().copied().filter(|id| !imports.announced.contains(id)).collect())
        .unwrap_or_default();
    for id in waiting {
        let Some(state) = engine.import_state(id) else {
            session.state_mut::<CsvImports>().announced.insert(id);
            continue;
        };
        if state.status == OperationStatus::Running {
            continue;
        }
        let imports = session.state_mut::<CsvImports>();
        imports.announced.insert(id);
        let target = imports.targets.remove(&id);
        announce(session, &state);
        if let Some((connection, path)) = target
            && (state.rows_committed > 0 || state.status == OperationStatus::Done)
        {
            super::objects::reread_table(session, &connection, &path);
        }
    }
}

#[cfg(test)]
mod tests;

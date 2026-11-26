use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{ExportItem, ExportState, ItemStatus, OperationStatus, Output};
use zen_db::export::{Format, PATTERN, Settings, file_name};
use zen_i18n::t;

use super::clipboard::Selection;
use super::connections::describe;
use super::consoles::{self, engine, ensure_connected};
use super::sources::find;
use crate::notifications::{Action, Group, Kind, NewNotification, post};
use crate::workspaces::Workspaces;

const LISTED: usize = 20;

/// The export settings kept in `<data folder>/databases/export.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Remembered {
    /// The file's version, 1.
    pub version: u32,
    /// The format and its settings.
    pub settings: Settings,
    /// The folder files go to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// The file name pattern: `${table}`, `${connection}`, `${timestamp}`, `${date}`, `${index}`.
    pub pattern: String,
    /// Whether the rows go to the clipboard instead of files.
    pub clipboard: bool,
}

impl Default for Remembered {
    fn default() -> Self {
        Self { version: 1, settings: Settings::default(), folder: None, pattern: PATTERN.to_owned(), clipboard: false }
    }
}

#[derive(Default)]
pub struct Exports {
    remembered: Option<Remembered>,
    started: Vec<u64>,
    announced: BTreeSet<u64>,
    clipboard: Option<String>,
}

impl Exports {
    pub fn take_clipboard(&mut self) -> Option<String> {
        self.clipboard.take()
    }
}

fn file_path(session: &Session) -> Option<PathBuf> {
    session.state::<Workspaces>().and_then(Workspaces::data_dir).map(|data| data.join("databases").join("export.json"))
}

pub fn remembered(session: &mut Session) -> Remembered {
    if let Some(remembered) = session.state::<Exports>().and_then(|exports| exports.remembered.clone()) {
        return remembered;
    }
    let read = file_path(session)
        .and_then(|file| zen_db::registry::read_json::<Remembered>(&file).ok().flatten())
        .unwrap_or_default();
    session.state_mut::<Exports>().remembered = Some(read.clone());
    read
}

fn remember(session: &mut Session, remembered: Remembered) -> Result<(), CommandError> {
    if session.state::<Exports>().and_then(|exports| exports.remembered.as_ref()) == Some(&remembered) {
        return Ok(());
    }
    if let Some(path) = file_path(session) {
        zen_db::registry::write_json(&path, &remembered)
            .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
    }
    session.state_mut::<Exports>().remembered = Some(remembered);
    Ok(())
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

fn failed(error: &DbError) -> CommandError {
    let code = match error.kind {
        ErrorKind::PasswordNeeded => "passwordNeeded",
        ErrorKind::NotConnected => "notConnected",
        _ => error.code.as_deref().unwrap_or("databaseError"),
    };
    CommandError::with_code(code, describe(error)).with_data(serde_json::json!({ "error": error }))
}

/// What to export.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Source {
    /// A result's query, read again whole in its own session (its filter and sort kept); the one the workspace shows when `id` is absent.
    Result {
        /// The result.
        #[serde(default)]
        id: Option<u64>,
    },
    /// A query of a console, run in the console's session so that it sees its transaction: `sql`, else the statement under the caret.
    Console {
        /// The console or `.sql` file; the active one when absent.
        #[serde(default)]
        path: Option<PathBuf>,
        /// The query; the statement under the caret (or the selection) when absent.
        #[serde(default)]
        sql: Option<String>,
    },
    /// A query on a connection, in a read-only session of its own.
    Query {
        /// The connection's id.
        connection: String,
        /// The query.
        sql: String,
        /// What it is called in file names; `query` when absent.
        #[serde(default)]
        name: Option<String>,
    },
    /// Tables or views of a connection's tree, each to its own file, each in a read-only session of its own.
    Tables {
        /// The connection's id.
        connection: String,
        /// Their paths in the tree, such as `["shop", "public", "tables", "orders"]`.
        paths: Vec<Vec<String>>,
    },
    /// Cells of a result as they are loaded, staged changes included (at most 100 000 rows), not read again; never Parquet.
    Cells {
        /// The result; the one the workspace shows when absent.
        #[serde(default)]
        id: Option<u64>,
        /// The first and last row, both included; every loaded row when absent.
        #[serde(default)]
        rows: Option<(usize, usize)>,
        /// The first and last column, both included; every column when absent.
        #[serde(default)]
        columns: Option<(usize, usize)>,
    },
}

/// What `db.export.start` exports and how.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartParams {
    /// What to export.
    pub source: Source,
    /// The format and its settings; the remembered ones when absent.
    #[serde(default)]
    pub settings: Option<Settings>,
    /// Copy the rows to the clipboard instead of writing files; the remembered choice when absent.
    #[serde(default)]
    pub clipboard: Option<bool>,
    /// The folder files go to; the remembered one, else the home folder, when absent.
    #[serde(default)]
    pub folder: Option<PathBuf>,
    /// The file name pattern; the remembered one when absent (`${table}_${timestamp}` at first).
    #[serde(default)]
    pub pattern: Option<String>,
    /// The file of a single item, instead of the folder and the pattern.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// Replace files that exist; refused with code `exists` and the paths otherwise.
    #[serde(default)]
    pub overwrite: bool,
    /// Keep the settings, folder and pattern for next time; true when absent.
    #[serde(default)]
    pub remember: Option<bool>,
}

/// An export started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The export's id, for `db.export.state` and `db.export.cancel`; 0 for cells, which `job` follows.
    pub id: u64,
    /// The background copy writing cells to a file, for db.result.copyState.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<u64>,
    /// What each item is and the file it goes to.
    pub items: Vec<StartedItem>,
    /// Whether the rows go to the clipboard.
    pub clipboard: bool,
    /// Whether a connection was connected first.
    pub connecting: bool,
}

/// One item of an export started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartedItem {
    /// What it is.
    pub name: String,
    /// The file it goes to; absent for the clipboard.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

struct Planned {
    connection: String,
    context: Option<ContextKind>,
    statement: String,
    name: String,
    table: String,
}

fn dialect(session: &mut Session, connection: &str) -> SqlDialect {
    super::filters::dialect_of(session, connection).unwrap_or_default()
}

fn driver_of(session: &Session, connection: &str) -> Result<String, CommandError> {
    Ok(find(session, connection)?.source.driver)
}

fn table_planned(session: &Session, connection: &str, path: &[String]) -> Result<Planned, CommandError> {
    let driver = driver_of(session, connection)?;
    let [_, schema, folder, name] = path else {
        return Err(CommandError::with_code("notATable", t!("database-error-export-not-table", path = path.join("/"))));
    };
    let kind = zen_db::schema::ObjectKind::from_folder(folder);
    if !kind.is_some_and(|kind| super::objects::has_data(super::objects::kind_name(kind))) {
        return Err(CommandError::with_code("notATable", t!("database-error-export-not-table", path = path.join("/"))));
    }
    let table = zen_db::schema::qualified_for(&driver, schema, name);
    Ok(Planned {
        connection: connection.to_owned(),
        context: None,
        statement: format!("SELECT * FROM {table}"),
        name: name.clone(),
        table,
    })
}

struct Cells {
    selection: Selection,
    settings: Settings,
    clipboard: bool,
    folder: PathBuf,
    pattern: String,
}

fn start_cells(
    session: &mut Session,
    cells: Cells,
    params: &StartParams,
    known: Remembered,
) -> Result<Started, CommandError> {
    let Cells { selection, settings, clipboard, folder, pattern } = cells;
    if settings.format.binary() {
        return Err(CommandError::with_code("binary", t!("database-error-export-cells-binary")));
    }
    let result = super::filters::result_or_active(session, selection.id)?;
    let name = short_name(result.table().unwrap_or("result"));
    let path = if clipboard {
        None
    } else {
        let path = match &params.path {
            Some(path) => path.clone(),
            None => {
                let connection = find(session, result.connection()).map(|entry| entry.source.name).unwrap_or_default();
                let (date, timestamp) = zen_db::engine::now_stamps();
                let values = [
                    ("table", name.as_str()),
                    ("connection", connection.as_str()),
                    ("timestamp", timestamp.as_str()),
                    ("date", date.as_str()),
                    ("index", "1"),
                ];
                folder.join(file_name(&pattern, &values, settings.format.extension(&settings)))
            }
        };
        if let Some(parent) = path.parent().filter(|parent| !parent.is_dir()) {
            return Err(CommandError::with_code(
                "noFolder",
                t!("database-error-export-no-folder", path = crate::display(parent)),
            ));
        }
        if path.is_dir() {
            return Err(CommandError::with_code("isFolder", t!("database-error-export-is-folder")));
        }
        if path.exists() && !params.overwrite {
            return Err(CommandError::with_code(
                "exists",
                t!("database-error-export-exists", count = 1, path = crate::display(&path)),
            )
            .with_data(serde_json::json!({ "paths": [path] })));
        }
        Some(path)
    };
    let copied = super::clipboard::export_cells(session, &selection, settings.clone(), path.clone())?;
    if params.remember.unwrap_or(true) {
        let kept_folder = if params.path.is_some() || clipboard { known.folder.clone() } else { Some(folder) };
        remember(session, Remembered { version: 1, settings, folder: kept_folder, pattern, clipboard })?;
    }
    Ok(Started { id: 0, job: copied.job, items: vec![StartedItem { name, path }], clipboard, connecting: false })
}

fn short_name(table: &str) -> String {
    table.rsplit('.').next().unwrap_or(table).trim_matches(['"', '`']).to_owned()
}

pub(crate) fn announce_cells(session: &mut Session, rows: usize, written: Result<PathBuf, String>) {
    match written {
        Ok(path) => {
            let body = t!("database-export-done-file", rows = rows, path = crate::display(&path));
            let note = NewNotification::message(Group::General, Kind::Info, body)
                .titled_by(zen_i18n::Message::new("database-export-title"))
                .action(Action::new(
                    "open",
                    t!("database-export-open"),
                    "file.document.open",
                    serde_json::json!({ "path": path }),
                ))
                .action(Action::new(
                    "reveal",
                    t!("database-export-reveal"),
                    "file.fs.reveal",
                    serde_json::json!({ "path": path }),
                ));
            post(session, note);
        }
        Err(reason) => {
            let body = t!("database-export-failed", reasons = reason);
            post(
                session,
                NewNotification::message(Group::General, Kind::Error, body)
                    .titled_by(zen_i18n::Message::new("database-export-title")),
            );
        }
    }
}

/// What an export of `source` reads, for the dialog's subtitle.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    /// What its files are named after: the table, `query` or `result`.
    pub name: String,
    /// The connection's id.
    pub connection: String,
    /// The connection's name.
    pub connection_name: String,
    /// Whether the connection's server can write CSV and TSV itself (PostgreSQL's COPY).
    pub server_copy: bool,
}

fn describe_source(session: &mut Session, source: Source) -> Result<SourceInfo, CommandError> {
    let (connection, name) = match source {
        Source::Cells { id, .. } | Source::Result { id } => {
            let result = super::filters::result_or_active(session, id)?;
            (result.connection().to_owned(), short_name(result.table().unwrap_or("result")))
        }
        other => {
            let planned = plan(session, other)?;
            let first = planned
                .first()
                .ok_or_else(|| CommandError::with_code("nothingToExport", t!("database-error-export-nothing")))?;
            let name = match planned.as_slice() {
                [one] => one.name.clone(),
                _ => String::new(),
            };
            (first.connection.clone(), name)
        }
    };
    let entry = find(session, &connection)?;
    Ok(SourceInfo {
        name,
        connection_name: entry.source.name.clone(),
        server_copy: entry.source.driver == zen_db::postgres::ID,
        connection,
    })
}

fn plan(session: &mut Session, source: Source) -> Result<Vec<Planned>, CommandError> {
    match source {
        Source::Result { id } => {
            let result = super::filters::result_or_active(session, id)?;
            let connection = result.connection().to_owned();
            let table = super::objects::data_view(session, result.id())
                .and_then(|(_, data)| super::objects::object_of(session, &data))
                .map(|(schema, object)| {
                    zen_db::schema::qualified_for(
                        &driver_of(session, &connection).unwrap_or_default(),
                        &schema,
                        &object.name,
                    )
                })
                .or_else(|| result.table().map(str::to_owned));
            let name = table
                .as_deref()
                .map(|table| table.rsplit('.').next().unwrap_or(table).trim_matches(['"', '`']).to_owned())
                .unwrap_or_else(|| "result".to_owned());
            Ok(vec![Planned {
                connection,
                context: Some(result.context().clone()),
                statement: result.statement_text().to_owned(),
                name,
                table: table.unwrap_or_else(|| "result".to_owned()),
            }])
        }
        Source::Console { path, sql } => {
            let path = consoles::console_path(session, path)?;
            let connection = consoles::connection_of(session, &path)
                .ok_or_else(|| CommandError::with_code("noConnection", super::no_console_connection(session)))?;
            let statement = match sql.filter(|sql| !sql.trim().is_empty()) {
                Some(sql) => sql,
                None => consoles::statement_text(session, &path)?
                    .filter(|text| !text.trim().is_empty())
                    .ok_or_else(|| CommandError::with_code("noStatement", t!("database-error-no-statement")))?,
            };
            let table = zen_db::results::table_of(&statement).unwrap_or_else(|| "query".to_owned());
            let name = table.rsplit('.').next().unwrap_or(&table).trim_matches(['"', '`']).to_owned();
            Ok(vec![Planned { connection, context: Some(consoles::context_of(&path)), statement, name, table }])
        }
        Source::Query { connection, sql, name } => {
            find(session, &connection)?;
            if sql.trim().is_empty() {
                return Err(CommandError::with_code("noStatement", t!("database-error-no-statement")));
            }
            let table = zen_db::results::table_of(&sql).unwrap_or_else(|| "query".to_owned());
            let name = name.filter(|name| !name.trim().is_empty()).unwrap_or_else(|| "query".to_owned());
            Ok(vec![Planned { connection, context: None, statement: sql, name, table }])
        }
        Source::Tables { connection, paths } => {
            if paths.is_empty() {
                return Err(CommandError::with_code("nothingToExport", t!("database-error-export-nothing")));
            }
            paths.iter().map(|path| table_planned(session, &connection, path)).collect()
        }
        Source::Cells { .. } => Err(CommandError::with_code("cells", t!("database-error-export-nothing"))),
    }
}

fn unique(path: PathBuf, taken: &mut BTreeSet<PathBuf>) -> PathBuf {
    if taken.insert(path.clone()) {
        return path;
    }
    let stem = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()).unwrap_or_default();
    let extension = path.extension().map(|extension| extension.to_string_lossy().into_owned()).unwrap_or_default();
    let mut number = 2;
    loop {
        let next = path.with_file_name(format!("{stem}_{number}.{extension}"));
        if taken.insert(next.clone()) {
            return next;
        }
        number += 1;
    }
}

pub struct StartExport;

impl Command for StartExport {
    type Params = StartParams;
    type Output = Started;
    const ID: &'static str = "db.export.start";
    const TITLE: &'static str = "Export Data";
    const DESCRIPTION: &'static str = "Exports rows in the background, streamed from the server to files (or the clipboard with `clipboard`), never through the results kept in memory: `source` is `{ kind: result, id? }` (the result's query read again whole in its own session, its filter and sort kept), `{ kind: console, path?, sql? }` (a console's query, the statement under the caret when `sql` is absent, in the console's session), `{ kind: query, connection, sql, name? }` or `{ kind: tables, connection, paths }` (each table to its own file; these two in a read-only session of their own), or `{ kind: cells, id?, rows?, columns? }` (cells of a result as loaded, `rows` and `columns` as [first, last], written from memory in the background: answers `id` 0 and the copy `job` for db.result.copyState; never Parquet). `settings` (the remembered ones when absent) has `format` (csv, tsv, json, sql, markdown, html, xml, parquet; parquet only to files), `delimiter`, `quote`, `quoteAll`, `header`, `nullText`, `lineEnding` (lf, crLf), `escapeFormulas`, `bom`, `json` (array, lines), `table` and `batch` for INSERTs, `serverCopy` (PostgreSQL writes CSV and TSV with COPY), `compression` (snappy, gzip, lz4, none) and `rowGroup` (rows a row group, 1 000 to 1 000 000) for Parquet, typed through Arrow. Files go to `path` (one item) or `folder` with `pattern` (`${table}_${timestamp}`, also `${connection}`, `${date}`, `${index}`); existing files are refused with code `exists` and `paths` unless `overwrite`. A statement that may change data is never run: refused with code `exportChanges`. Connects first when needed with the stored password. Answers the export's id and its files at once; follow it with db.export.state. Settings are remembered unless `remember: false`.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<Started, CommandError> {
        let known = remembered(session);
        let settings = params.settings.clone().unwrap_or_else(|| known.settings.clone());
        if let Some(problem) = settings.problem() {
            return Err(CommandError::with_code(problem, t!("database-error-export-delimiter")));
        }
        let clipboard = params.clipboard.unwrap_or(known.clipboard);
        if clipboard && settings.format.binary() {
            return Err(CommandError::with_code("binary", t!("database-error-export-binary")));
        }
        let folder = params.folder.clone().or_else(|| known.folder.clone()).unwrap_or_else(home);
        super::guard_agent_file(session, &folder)?;
        if let Some(path) = &params.path {
            super::guard_agent_file(session, path)?;
        }
        let pattern = params
            .pattern
            .clone()
            .filter(|pattern| !pattern.trim().is_empty())
            .unwrap_or_else(|| known.pattern.clone());
        if let Source::Cells { id, rows, columns } = params.source {
            let selection = Selection { id, rows, columns, ..Selection::default() };
            let cells = Cells { selection, settings, clipboard, folder, pattern };
            return start_cells(session, cells, &params, known);
        }
        let planned = plan(session, params.source)?;
        for item in &planned {
            let entry = find(session, &item.connection)?;
            super::safety::agent_reads(session, &entry, &item.statement)?;
            if !engine(session)
                .driver(&entry.source.driver)
                .is_some_and(|driver| driver.assess(&item.statement).reads())
            {
                return Err(CommandError::with_code("exportChanges", t!("database-error-export-changes")));
            }
        }
        if params.path.is_some() && planned.len() > 1 {
            return Err(CommandError::with_code("severalItems", t!("database-error-export-one-path")));
        }
        let (date, timestamp) = zen_db::engine::now_stamps();
        let extension = settings.format.extension(&settings);
        let mut taken = BTreeSet::new();
        let mut items = Vec::new();
        for (index, planned) in planned.into_iter().enumerate() {
            let output = if clipboard {
                Output::Text
            } else {
                let path = match &params.path {
                    Some(path) => path.clone(),
                    None => {
                        let connection =
                            find(session, &planned.connection).map(|entry| entry.source.name).unwrap_or_default();
                        let number = (index + 1).to_string();
                        let values = [
                            ("table", planned.name.as_str()),
                            ("connection", connection.as_str()),
                            ("timestamp", timestamp.as_str()),
                            ("date", date.as_str()),
                            ("index", number.as_str()),
                        ];
                        folder.join(file_name(&pattern, &values, extension))
                    }
                };
                Output::File { path: unique(path, &mut taken), overwrite: params.overwrite }
            };
            let dialect = dialect(session, &planned.connection);
            items.push(ExportItem {
                connection: planned.connection,
                context: planned.context,
                statement: planned.statement,
                name: planned.name,
                table: planned.table,
                dialect,
                output,
            });
        }
        let paths: Vec<&Path> = items
            .iter()
            .filter_map(|item| match &item.output {
                Output::File { path, .. } => Some(path.as_path()),
                Output::Text => None,
            })
            .collect();
        if !clipboard {
            if let Some(parent) = paths.iter().filter_map(|path| path.parent()).find(|parent| !parent.is_dir()) {
                return Err(CommandError::with_code(
                    "noFolder",
                    t!("database-error-export-no-folder", path = crate::display(parent)),
                ));
            }
            let existing: Vec<&Path> = paths.iter().copied().filter(|path| path.exists()).collect();
            if !params.overwrite && !existing.is_empty() {
                return Err(CommandError::with_code(
                    "exists",
                    t!("database-error-export-exists", count = existing.len(), path = crate::display(existing[0])),
                )
                .with_data(serde_json::json!({ "paths": existing })));
            }
            if existing.iter().any(|path| path.is_dir()) {
                return Err(CommandError::with_code("isFolder", t!("database-error-export-is-folder")));
            }
        }
        let mut connecting = false;
        let connections: BTreeSet<String> = items.iter().map(|item| item.connection.clone()).collect();
        for connection in &connections {
            connecting |= ensure_connected(session, connection)?;
        }
        let answer_items = items
            .iter()
            .map(|item| StartedItem {
                name: item.name.clone(),
                path: match &item.output {
                    Output::File { path, .. } => Some(path.clone()),
                    Output::Text => None,
                },
            })
            .collect();
        let engine = engine(session);
        let provenance = super::history::provenance(session, zen_db::history::Source::Export, None);
        let id = engine.export_from(items, settings.clone(), provenance).map_err(|error| failed(&error))?;
        session.state_mut::<Exports>().started.push(id);
        if params.remember.unwrap_or(true) {
            let kept_folder = if params.path.is_some() || clipboard { known.folder.clone() } else { Some(folder) };
            remember(session, Remembered { version: 1, settings, folder: kept_folder, pattern, clipboard })?;
        }
        Ok(Started { id, job: None, items: answer_items, clipboard, connecting })
    }
}

/// The settings the export dialog starts with.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportDefaults {
    /// The remembered settings, folder, pattern and destination.
    #[serde(flatten)]
    pub remembered: Remembered,
    /// The folder files go to when none is remembered.
    pub default_folder: PathBuf,
    /// The formats, in order: `{ id, name, extension }`.
    pub formats: Vec<FormatInfo>,
    /// What `source` reads, when it was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceInfo>,
}

/// What `db.export.settings` describes besides the remembered settings.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SettingsParams {
    /// A source as db.export.start takes it, to name it and its connection and tell whether the server can write CSV itself.
    pub source: Option<Source>,
}

/// A format the dialog lists.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormatInfo {
    /// Its id.
    pub id: Format,
    /// Its name.
    pub name: String,
    /// Its files' extension.
    pub extension: String,
}

pub struct ExportSettings;

impl Command for ExportSettings {
    type Params = SettingsParams;
    type Output = ExportDefaults;
    const ID: &'static str = "db.export.settings";
    const TITLE: &'static str = "Export Settings";
    const DESCRIPTION: &'static str = "The export settings remembered from the last export (`<data folder>/databases/export.json`): format and its settings, folder, file name pattern and whether rows go to the clipboard, with the formats and their extensions. With `source` (as db.export.start takes it) also `source`: what its files are named after, its connection and whether the server can write CSV and TSV itself (`serverCopy`, PostgreSQL's COPY); never connects or reads rows.";

    fn run(&self, session: &mut Session, params: SettingsParams) -> Result<ExportDefaults, CommandError> {
        let remembered = remembered(session);
        let formats = Format::ALL
            .iter()
            .map(|format| FormatInfo {
                id: *format,
                name: format.name().to_owned(),
                extension: format.extension(&remembered.settings).to_owned(),
            })
            .collect();
        let source = params.source.map(|source| describe_source(session, source)).transpose()?;
        Ok(ExportDefaults { remembered, default_folder: home(), formats, source })
    }
}

/// Which export to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// The export; the exports of this window, newest first, when absent.
    pub id: Option<u64>,
    /// How long to wait for it to end, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
    /// The most exports to list; 20 when absent.
    pub limit: Option<usize>,
}

/// Exports as agents read them; never their rows.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportsAnswer {
    /// The exports, newest first.
    pub exports: Vec<ExportState>,
    /// Whether exports were left out because of `limit`.
    pub truncated: bool,
}

pub struct ExportStateCommand;

impl Command for ExportStateCommand {
    type Params = StateParams;
    type Output = ExportsAnswer;
    const ID: &'static str = "db.export.state";
    const TITLE: &'static str = "Export State";
    const DESCRIPTION: &'static str = "The progress of export `id` (waiting up to `waitMs`, 60 000 at most, for it to end), or the window's exports newest first (at most `limit`, 20): each with its status (running, done, failed, cancelled), the item being written, rows and bytes written per item and in all, its files, whether PostgreSQL wrote it with COPY (`serverCopy`), and why an item failed. Never rows or values.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<ExportsAnswer, CommandError> {
        let engine = engine(session);
        if let Some(id) = params.id {
            if let Some(wait) = params.wait_ms {
                engine.wait(id, std::time::Duration::from_millis(wait.min(60_000)));
            }
            let state = engine.export_state(id).ok_or_else(|| {
                CommandError::with_code("unknownExport", t!("database-error-unknown-export", id = id))
            })?;
            return Ok(ExportsAnswer { exports: vec![state], truncated: false });
        }
        let ids = engine.exports();
        let limit = params.limit.unwrap_or(LISTED);
        let exports: Vec<ExportState> =
            ids.iter().rev().take(limit).filter_map(|id| engine.export_state(*id)).collect();
        Ok(ExportsAnswer { truncated: ids.len() > exports.len(), exports })
    }
}

/// Which export to cancel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CancelParams {
    /// The export.
    pub id: u64,
}

/// Whether an export was cancelled.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Whether it was running and is being cancelled.
    pub cancelled: bool,
}

pub struct CancelExport;

impl Command for CancelExport {
    type Params = CancelParams;
    type Output = Cancelled;
    const ID: &'static str = "db.export.cancel";
    const TITLE: &'static str = "Cancel Export";
    const DESCRIPTION: &'static str = "Cancels export `id`: the query stops on the server, and the file being written is removed (files are written under a temporary name and moved into place only when complete, so an existing file is never left half replaced). Answers whether it was running.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<Cancelled, CommandError> {
        let engine = engine(session);
        Ok(Cancelled { cancelled: engine.cancel(params.id) })
    }
}

fn announce(session: &mut Session, state: &ExportState) {
    match state.status {
        OperationStatus::Running => {}
        OperationStatus::Cancelled => {
            post(
                session,
                NewNotification::worded(
                    Group::General,
                    Kind::Info,
                    zen_i18n::Message::new("database-export-cancelled"),
                ),
            );
        }
        OperationStatus::Failed => {
            let failures: Vec<String> = state
                .items
                .iter()
                .filter(|item| item.status == ItemStatus::Failed)
                .map(|item| {
                    let reason = item.error.as_ref().map(describe).unwrap_or_default();
                    format!("{}: {reason}", item.name)
                })
                .collect();
            let body = t!("database-export-failed", reasons = failures.join("\n"));
            post(
                session,
                NewNotification::message(Group::General, Kind::Error, body)
                    .titled_by(zen_i18n::Message::new("database-export-title")),
            );
        }
        OperationStatus::Done => {
            let files: Vec<&PathBuf> = state.items.iter().filter_map(|item| item.path.as_ref()).collect();
            if files.is_empty() {
                let text = session
                    .state::<super::Database>()
                    .and_then(super::Database::started)
                    .and_then(|engine| engine.take_export_text(state.id));
                if let Some(text) = text {
                    session.state_mut::<Exports>().clipboard = Some(text);
                }
                let body = t!("database-export-copied", rows = state.rows);
                post(
                    session,
                    NewNotification::message(Group::General, Kind::Info, body)
                        .titled_by(zen_i18n::Message::new("database-export-title")),
                );
                return;
            }
            let body = match files.as_slice() {
                [one] => t!("database-export-done-file", rows = state.rows, path = crate::display(one)),
                many => t!(
                    "database-export-done-files",
                    rows = state.rows,
                    count = many.len(),
                    folder = many[0].parent().map(crate::display).unwrap_or_default()
                ),
            };
            let mut note = NewNotification::message(Group::General, Kind::Info, body)
                .titled_by(zen_i18n::Message::new("database-export-title"));
            if let [one] = files.as_slice() {
                note = note.action(Action::new(
                    "open",
                    t!("database-export-open"),
                    "file.document.open",
                    serde_json::json!({ "path": one }),
                ));
            }
            let reveal = files[0].clone();
            note = note.action(Action::new(
                "reveal",
                t!("database-export-reveal"),
                "file.fs.reveal",
                serde_json::json!({ "path": reveal }),
            ));
            post(session, note);
        }
    }
}

pub fn step(session: &mut Session) {
    let Some(engine) = session.state::<super::Database>().and_then(super::Database::started).cloned() else { return };
    let waiting: Vec<u64> = session
        .state::<Exports>()
        .map(|exports| exports.started.iter().copied().filter(|id| !exports.announced.contains(id)).collect())
        .unwrap_or_default();
    for id in waiting {
        let Some(state) = engine.export_state(id) else {
            session.state_mut::<Exports>().announced.insert(id);
            continue;
        };
        if state.status == OperationStatus::Running {
            continue;
        }
        session.state_mut::<Exports>().announced.insert(id);
        announce(session, &state);
    }
}

use std::collections::BTreeSet;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::csv_import::{LoadSettings, insert_head, truncate_statement};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::engine::{ItemStatus, OperationStatus, QueryStatement, TransferRequest, TransferState, TransferTable};
use zen_db::registry::Entry;
use zen_db::schema::{ConstraintKind, Object, ObjectKind, qualified_for};
use zen_db::transfer::{NewColumn, SqlType, create_table, select_statement, target_type};
use zen_i18n::t;

use super::connections::describe;
use super::consoles::{engine, ensure_connected};
use super::safety::{Confirmation, Run, SKIPPED};
use super::sources::find;
use super::structure::Structure;
use crate::notifications::{Group, Kind as NoteKind, NewNotification, post};
use crate::workspaces::Workspaces;

const LISTED: usize = 20;
const MOST_TABLES: usize = 200;

/// The transfer settings kept in `<data folder>/databases/transfer.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Remembered {
    /// The file's version, 1.
    pub version: u32,
    /// How rows are loaded.
    pub load: LoadSettings,
}

impl Default for Remembered {
    fn default() -> Self {
        Self { version: 1, load: LoadSettings::default() }
    }
}

#[derive(Default)]
pub struct Transfers {
    remembered: Option<Remembered>,
    started: Vec<u64>,
    announced: BTreeSet<u64>,
}

fn file_path(session: &Session) -> Option<PathBuf> {
    session
        .state::<Workspaces>()
        .and_then(Workspaces::data_dir)
        .map(|data| data.join("databases").join("transfer.json"))
}

pub fn remembered(session: &mut Session) -> Remembered {
    if let Some(remembered) = session.state::<Transfers>().and_then(|transfers| transfers.remembered.clone()) {
        return remembered;
    }
    let read = file_path(session)
        .and_then(|file| zen_db::registry::read_json::<Remembered>(&file).ok().flatten())
        .unwrap_or_default();
    session.state_mut::<Transfers>().remembered = Some(read.clone());
    read
}

fn remember(session: &mut Session, remembered: Remembered) -> Result<(), CommandError> {
    if session.state::<Transfers>().and_then(|transfers| transfers.remembered.as_ref()) == Some(&remembered) {
        return Ok(());
    }
    if let Some(path) = file_path(session) {
        zen_db::registry::write_json(&path, &remembered)
            .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
    }
    session.state_mut::<Transfers>().remembered = Some(remembered);
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

fn dialect(session: &mut Session, entry: &Entry) -> Result<SqlDialect, CommandError> {
    super::filters::dialect_of(session, &entry.source.id).ok_or_else(|| {
        CommandError::with_code(
            "unsupported",
            t!("database-error-transfer-unsupported", name = entry.source.name.as_str()),
        )
    })
}

fn table_path(path: &[String]) -> Result<(String, String), CommandError> {
    match path {
        [_, schema, folder, name] if folder == ObjectKind::Table.folder() && !name.trim().is_empty() => {
            Ok((schema.clone(), name.trim().to_owned()))
        }
        _ => Err(CommandError::with_code("notATable", t!("database-error-import-not-table", path = path.join("/")))),
    }
}

fn object(session: &mut Session, entry: &Entry, path: &[String]) -> Option<Object> {
    super::structure::ensure(session, entry, true);
    let snapshot = session.state::<Structure>().and_then(|known| known.snapshot(&entry.source.id))?;
    Some(zen_db::schema::find_object(snapshot, path)?.object.clone())
}

fn schemas(session: &mut Session, entry: &Entry) -> Vec<Vec<String>> {
    super::structure::ensure(session, entry, true);
    let Some(snapshot) = session.state::<Structure>().and_then(|known| known.snapshot(&entry.source.id)) else {
        return Vec::new();
    };
    snapshot
        .catalogs
        .iter()
        .flat_map(|catalog| {
            catalog
                .schemas
                .iter()
                .filter(|schema| !schema.system)
                .map(|schema| vec![catalog.name.clone(), schema.name.clone()])
        })
        .collect()
}

/// A source column and the target column it goes into.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ColumnPlan {
    /// The source column.
    pub source: String,
    /// Its type on the source.
    pub source_type: String,
    /// The target column.
    pub target: String,
    /// The target column's type: the existing column's, or the one a new table gets in the target's dialect.
    #[serde(rename = "type")]
    pub sql_type: String,
    /// Whether the column takes `NULL`s.
    pub nullable: bool,
}

/// A source table and where its rows go.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TablePlan {
    /// The source table's path, `[db, schema, "tables", name]`.
    pub source: Vec<String>,
    /// The target table's path.
    pub target: Vec<String>,
    /// The target table, quoted and qualified.
    pub qualified: String,
    /// Whether the target table is in the target's snapshot.
    pub exists: bool,
    /// Whether the transfer creates it.
    pub create: bool,
    /// The columns copied, source to target; source columns the target lacks are left out.
    pub columns: Vec<ColumnPlan>,
    /// The primary key a new table gets: the source's, when all its columns are copied.
    pub key: Vec<String>,
    /// The server's estimate of the source's rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<i64>,
    /// Whether columns of the source or the target are being read; ask again.
    pub loading: bool,
}

struct Ends {
    source: Entry,
    target: Entry,
    from: SqlDialect,
    to: SqlDialect,
}

fn ends(session: &mut Session, source: &str, target: &str) -> Result<Ends, CommandError> {
    let source = find(session, source)?;
    let target = find(session, target)?;
    let from = dialect(session, &source)?;
    let to = dialect(session, &target)?;
    Ok(Ends { source, target, from, to })
}

fn plan(
    session: &mut Session,
    ends: &Ends,
    source_path: &[String],
    target_path: Vec<String>,
) -> Result<TablePlan, CommandError> {
    let (_, target_name) = table_path(&target_path)?;
    table_path(source_path)?;
    let mut loading = false;
    let found = object(session, &ends.source, source_path).ok_or_else(|| {
        CommandError::with_code("notFound", t!("database-error-transfer-no-table", path = source_path.join("/")))
    })?;
    if !found.detailed {
        loading |= super::structure::details(session, &ends.source, source_path)?.is_some();
    }
    let existing = object(session, &ends.target, &target_path);
    if existing.as_ref().is_some_and(|existing| !existing.detailed) {
        loading |= super::structure::details(session, &ends.target, &target_path)?.is_some();
    }
    let source_columns = found.columns.iter().filter(|column| column.generated.is_none());
    let columns: Vec<ColumnPlan> = match &existing {
        Some(existing) => source_columns
            .filter_map(|column| {
                let into = existing
                    .columns
                    .iter()
                    .filter(|known| known.generated.is_none())
                    .find(|known| known.name.eq_ignore_ascii_case(&column.name))?;
                Some(ColumnPlan {
                    source: column.name.clone(),
                    source_type: column.type_name.clone(),
                    target: into.name.clone(),
                    sql_type: into.type_name.clone(),
                    nullable: into.nullable,
                })
            })
            .collect(),
        None => source_columns
            .map(|column| ColumnPlan {
                source: column.name.clone(),
                source_type: column.type_name.clone(),
                target: column.name.clone(),
                sql_type: target_type(&column.type_name, ends.from, ends.to, column.nullable),
                nullable: column.nullable,
            })
            .collect(),
    };
    let key = found
        .constraints
        .iter()
        .find(|constraint| constraint.kind == ConstraintKind::PrimaryKey)
        .map(|constraint| constraint.columns.clone())
        .filter(|key| key.iter().all(|name| columns.iter().any(|column| column.source == *name)))
        .unwrap_or_default();
    let schema = target_path.get(1).cloned().unwrap_or_default();
    Ok(TablePlan {
        source: source_path.to_vec(),
        qualified: qualified_for(&ends.target.source.driver, &schema, &target_name),
        target: target_path,
        exists: existing.is_some(),
        create: existing.is_none(),
        columns,
        key,
        rows: found.rows,
        loading,
    })
}

fn target_path(schema: &[String], name: &str) -> Result<Vec<String>, CommandError> {
    match schema {
        [db, schema] => Ok(vec![db.clone(), schema.clone(), ObjectKind::Table.folder().to_owned(), name.to_owned()]),
        _ => Err(CommandError::with_code("badSchema", t!("database-error-transfer-schema", path = schema.join("/")))),
    }
}

fn not_itself(
    source_connection: &str,
    target_connection: &str,
    source: &[String],
    target: &[String],
) -> Result<(), CommandError> {
    let same = source_connection == target_connection
        && source.len() == target.len()
        && source.iter().zip(target).all(|(a, b)| a.eq_ignore_ascii_case(b));
    if same {
        return Err(CommandError::with_code(
            "sameTable",
            t!("database-error-transfer-same-table", table = source.last().map_or("", String::as_str)),
        ));
    }
    Ok(())
}

fn not_too_many(count: usize) -> Result<(), CommandError> {
    if count > MOST_TABLES {
        return Err(CommandError::with_code(
            "tooManyTables",
            t!("database-error-transfer-too-many", most = MOST_TABLES),
        ));
    }
    Ok(())
}

fn default_schema(session: &mut Session, ends: &Ends, source: &[String]) -> Option<Vec<String>> {
    let known = schemas(session, &ends.target);
    let wanted = source.get(1)?;
    known.iter().find(|schema| schema[1] == *wanted).or_else(|| known.first()).cloned()
}

/// What `db.transfer.preview` reads.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewParams {
    /// The source connection's id.
    pub source: String,
    /// The source tables' paths, `[db, schema, "tables", name]`.
    pub tables: Vec<Vec<String>>,
    /// The target connection's id.
    pub target: String,
    /// The target schema, `[db, schema]`; the one named as the source's, else the first, when absent.
    pub schema: Option<Vec<String>>,
    /// The most target schemas listed (20).
    pub limit: Option<usize>,
}

/// The plan a transfer would follow.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The source's dialect, as its driver is named.
    pub source_driver: String,
    /// The target's dialect, as its driver is named.
    pub target_driver: String,
    /// The target schema the tables go into.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<Vec<String>>,
    /// The target's schemas known, at most `limit`.
    pub schemas: Vec<Vec<String>>,
    /// Whether schemas were left out because of `limit`.
    pub truncated: bool,
    /// Each table and where it goes.
    pub tables: Vec<TablePlan>,
    /// The load settings remembered from the last transfer.
    pub load: LoadSettings,
}

pub struct PreviewTransfer;

impl Command for PreviewTransfer {
    type Params = PreviewParams;
    type Output = Preview;
    const ID: &'static str = "db.transfer.preview";
    const TITLE: &'static str = "Preview Data Transfer";
    const DESCRIPTION: &'static str = "Plans copying `tables` (`[db, schema, \"tables\", name]`) of connection `source` into schema `schema` (`[db, schema]`; the one named as the source's, else the first) of connection `target`: for each table the target path and whether it exists (from the target's snapshot), the columns copied (into an existing table by name; for a new table every column with its type mapped into the target's dialect, copied as it is between the same dialects), the primary key a new table gets and the source's row estimate; `loading` while columns are read (ask again). Answers the target's schemas (at most `limit`, 20) and the remembered load settings. Never connects, never reads rows.";

    fn run(&self, session: &mut Session, params: PreviewParams) -> Result<Preview, CommandError> {
        let ends = ends(session, &params.source, &params.target)?;
        let limit = params.limit.unwrap_or(LISTED).max(1);
        let schema = match params.schema {
            Some(schema) => Some(schema),
            None => params.tables.first().and_then(|first| default_schema(session, &ends, first)),
        };
        not_too_many(params.tables.len())?;
        let mut tables = Vec::new();
        for source in &params.tables {
            let Some(schema) = &schema else { break };
            let name = source.last().cloned().unwrap_or_default();
            let target = target_path(schema, &name)?;
            not_itself(&params.source, &params.target, source, &target)?;
            tables.push(plan(session, &ends, source, target)?);
        }
        let known = schemas(session, &ends.target);
        Ok(Preview {
            source_driver: ends.source.source.driver.clone(),
            target_driver: ends.target.source.driver.clone(),
            schema,
            truncated: known.len() > limit,
            schemas: known.into_iter().take(limit).collect(),
            tables,
            load: remembered(session).load,
        })
    }
}

/// A source column and the target column it goes into, as `db.transfer.start` is given it.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ColumnParam {
    /// The source column.
    pub source: String,
    /// The target column; the source's name when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// The target column's type for a new table; the mapped type when absent.
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub sql_type: Option<String>,
}

/// One table of `db.transfer.start`.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TableParam {
    /// The source table's path, `[db, schema, "tables", name]`.
    pub source: Vec<String>,
    /// The target table's name; the source's when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Create the target table first; when absent, it is created when the target's snapshot does not have it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub create: Option<bool>,
    /// The columns copied; the plan's when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<ColumnParam>>,
}

/// What `db.transfer.start` copies and how.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StartParams {
    /// The source connection's id.
    pub source: String,
    /// The target connection's id.
    pub target: String,
    /// The target schema, `[db, schema]`.
    pub schema: Vec<String>,
    /// The tables, copied in this order.
    pub tables: Vec<TableParam>,
    /// How rows are loaded; the remembered settings when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load: Option<LoadSettings>,
    /// The `token` of a `confirmExecute` refusal, to go on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confirmed: Option<String>,
    /// Keep the load settings for next time; true when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remember: Option<bool>,
}

/// A table of a transfer started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StartedTable {
    /// What it is: source to target.
    pub name: String,
    /// The target table, quoted and qualified.
    pub table: String,
    /// The statements run before its rows: `CREATE TABLE`, `TRUNCATE TABLE`.
    pub statements: Vec<String>,
    /// The columns copied.
    pub columns: Vec<ColumnPlan>,
}

/// A transfer started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The transfer's id, for `db.transfer.state` and `db.transfer.cancel`.
    pub id: u64,
    /// Its tables in order.
    pub tables: Vec<StartedTable>,
    /// Whether a connection was connected first.
    pub connecting: bool,
}

fn chosen(given: Option<Vec<ColumnParam>>, plan: &TablePlan) -> Result<Vec<ColumnPlan>, CommandError> {
    let Some(given) = given else { return Ok(plan.columns.clone()) };
    let mut seen = BTreeSet::new();
    given
        .into_iter()
        .map(|column| {
            let base = plan.columns.iter().find(|known| known.source.eq_ignore_ascii_case(&column.source));
            let target = column.target.clone().filter(|name| !name.trim().is_empty());
            let bad = || {
                CommandError::with_code(
                    "badMapping",
                    t!("database-error-transfer-mapping", column = column.source.as_str()),
                )
            };
            let base = base.ok_or_else(bad)?;
            let target = target.map_or_else(|| base.target.clone(), |name| name.trim().to_owned());
            if !seen.insert(target.to_lowercase()) {
                return Err(bad());
            }
            let sql_type = column
                .sql_type
                .filter(|sql_type| plan.create && !sql_type.trim().is_empty())
                .map(|sql_type| sql_type.trim().to_owned())
                .unwrap_or_else(|| base.sql_type.clone());
            let depth = sql_type.chars().try_fold(0_i32, |depth, c| match c {
                '(' => Some(depth + 1),
                ')' if depth > 0 => Some(depth - 1),
                ')' => None,
                _ => Some(depth),
            });
            if sql_type.contains([';', '\'', '"', '`']) || sql_type.contains("--") || depth != Some(0) {
                return Err(bad());
            }
            Ok(ColumnPlan { target, sql_type, ..base.clone() })
        })
        .collect()
}

pub struct StartTransfer;

impl Command for StartTransfer {
    type Params = StartParams;
    type Output = Started;
    const ID: &'static str = "db.transfer.start";
    const TITLE: &'static str = "Transfer Data";
    const DESCRIPTION: &'static str = "Copies the rows of `tables` of connection `source` into schema `schema` (`[db, schema]`) of connection `target` (may be the same) in the background, a table at a time, each read page by page in a read-only session and written in batches, never held whole. Each table is `{ source: [db, schema, \"tables\", name], name? (the target table; the source's), create? (create it first with the columns' types mapped into the target's dialect and the source's primary key; by default when the target does not have it), columns? [{ source, target?, type? }] }` (the plan of db.transfer.preview when absent). `load` `{ batch (1000 rows a page and a statement), commitEvery (0: one transaction a table), truncate (empty existing tables first), onError (stop: roll back the table and stop; skip: note the row and go on), serverCopy (PostgreSQL's COPY FROM STDIN) }`. The target's safety applies: read-only connections refuse (`readOnly`), a type that confirms changes and `truncate` ask first (`confirmExecute` with a `token` and `retry`), agents follow each connection's agent access. Refused with `loading` while columns are read (ask again). Connects first with stored passwords. Answers the transfer's id at once; follow it with db.transfer.state.";

    fn run(&self, session: &mut Session, params: StartParams) -> Result<Started, CommandError> {
        let ends = ends(session, &params.source, &params.target)?;
        if params.tables.is_empty() {
            return Err(CommandError::with_code("noTables", t!("database-error-transfer-no-tables")));
        }
        let mut load = params.load.clone().unwrap_or_else(|| remembered(session).load);
        load.batch = load.batch.clamp(1, 100_000);
        let mut tables = Vec::new();
        let mut started = Vec::new();
        let mut loading = false;
        not_too_many(params.tables.len())?;
        for table in &params.tables {
            let name = table.name.clone().filter(|name| !name.trim().is_empty());
            let name = name.or_else(|| table.source.last().cloned()).unwrap_or_default();
            let target = target_path(&params.schema, name.trim())?;
            not_itself(&params.source, &params.target, &table.source, &target)?;
            let mut planned = plan(session, &ends, &table.source, target)?;
            loading |= planned.loading;
            if let Some(create) = table.create {
                if create && planned.exists {
                    return Err(CommandError::with_code(
                        "exists",
                        t!("database-error-import-table-exists", table = planned.qualified.as_str()),
                    ));
                }
                planned.create = create;
            }
            let columns = chosen(table.columns.clone(), &planned)?;
            if columns.is_empty() {
                return Err(CommandError::with_code(
                    "noColumns",
                    t!("database-error-transfer-no-columns", table = planned.qualified.as_str()),
                ));
            }
            let source_schema = table.source.get(1).cloned().unwrap_or_default();
            let source_name = table.source.last().cloned().unwrap_or_default();
            let read = qualified_for(&ends.source.source.driver, &source_schema, &source_name);
            let names: Vec<String> = columns.iter().map(|column| column.source.clone()).collect();
            let select = select_statement(ends.from, &read, &names);
            super::safety::agent_reads(session, &ends.source, &select)?;
            let mut prelude = Vec::new();
            if planned.create {
                let defined: Vec<NewColumn> = columns
                    .iter()
                    .map(|column| NewColumn {
                        name: column.target.clone(),
                        sql_type: column.sql_type.clone(),
                        nullable: column.nullable,
                    })
                    .collect();
                let key: Vec<String> = planned
                    .key
                    .iter()
                    .filter_map(|key| {
                        columns.iter().find(|column| column.source == *key).map(|column| column.target.clone())
                    })
                    .collect();
                let key = if key.len() == planned.key.len() { key } else { Vec::new() };
                prelude.push(create_table(ends.to, &planned.qualified, &defined, &key));
            } else if load.truncate {
                prelude.push(truncate_statement(ends.to, &planned.qualified));
            }
            let shown = format!(
                "{}.{source_name} → {}.{}",
                source_schema,
                params.schema.get(1).cloned().unwrap_or_default(),
                name.trim()
            );
            started.push(StartedTable {
                name: shown.clone(),
                table: planned.qualified.clone(),
                statements: prelude.clone(),
                columns: columns.clone(),
            });
            tables.push(TransferTable {
                name: shown,
                select,
                table: planned.qualified.clone(),
                columns: columns.iter().map(|column| column.target.clone()).collect(),
                types: columns
                    .iter()
                    .map(|column| {
                        let parsed = SqlType::parse(&column.sql_type);
                        if column.sql_type.is_empty() { SqlType::parse(&column.source_type) } else { parsed }
                    })
                    .collect(),
                prelude,
            });
        }
        if loading {
            return Err(CommandError::with_code("loading", t!("database-error-transfer-loading")));
        }
        let mut statements = Vec::new();
        for table in &tables {
            let head = insert_head(ends.to, &table.table, &table.columns);
            for text in table.prelude.iter().chain(std::iter::once(&head)) {
                statements.push(QueryStatement {
                    text: text.clone(),
                    start: 0,
                    end: text.len(),
                    client: false,
                    params: Vec::new(),
                    kept: None,
                });
            }
        }
        let context = ContextKind::Console("transfer".to_owned());
        let mut provenance = super::history::provenance(session, zen_db::history::Source::Transfer, None);
        let confirmation = Confirmation { confirmed: params.confirmed.clone(), skip: Vec::new() };
        let mut retry = serde_json::to_value(&params).unwrap_or_default();
        if let Some(object) = retry.as_object_mut() {
            object.remove("confirmed");
        }
        let run = Run {
            entry: &ends.target,
            context: &context,
            provenance: &provenance,
            confirmation: &confirmation,
            retry: (Self::ID, retry),
        };
        let guarded = super::safety::guard(session, &run, statements)?;
        provenance.confirmed = guarded.confirmed;
        let kept: BTreeSet<&str> = guarded.statements.iter().map(|statement| statement.text.as_str()).collect();
        let mut kept_tables = Vec::new();
        let mut kept_started = Vec::new();
        for (mut table, mut shown) in tables.into_iter().zip(started) {
            if !kept.contains(insert_head(ends.to, &table.table, &table.columns).as_str()) {
                continue;
            }
            table.prelude.retain(|statement| kept.contains(statement.as_str()));
            shown.statements.clone_from(&table.prelude);
            kept_tables.push(table);
            kept_started.push(shown);
        }
        if kept_tables.is_empty() {
            return Err(CommandError::with_code(SKIPPED, t!("database-error-all-skipped")));
        }
        let mut connecting = ensure_connected(session, &params.source)?;
        connecting |= ensure_connected(session, &params.target)?;
        let request = TransferRequest {
            source: params.source.clone(),
            target: params.target.clone(),
            dialect: ends.to,
            tables: kept_tables,
            load: load.clone(),
        };
        let id = engine(session).transfer(request, provenance).map_err(|error| failed(&error))?;
        session.state_mut::<Transfers>().started.push(id);
        if params.remember.unwrap_or(true) {
            remember(session, Remembered { version: 1, load })?;
        }
        Ok(Started { id, tables: kept_started, connecting })
    }
}

/// Which transfer to read.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// The transfer; the transfers of this window, newest first, when absent.
    pub id: Option<u64>,
    /// How long to wait for it to end, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
    /// The most transfers listed, and the most row errors each table answers; 20 when absent.
    pub limit: Option<usize>,
}

/// Transfers as agents read them; never their rows.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TransfersAnswer {
    /// The transfers, newest first, each table with at most `limit` row errors (`errorsTotal` counts them all).
    pub transfers: Vec<TransferState>,
    /// Whether transfers were left out because of `limit`.
    pub truncated: bool,
}

pub struct TransferStateCommand;

impl Command for TransferStateCommand {
    type Params = StateParams;
    type Output = TransfersAnswer;
    const ID: &'static str = "db.transfer.state";
    const TITLE: &'static str = "Data Transfer State";
    const DESCRIPTION: &'static str = "The progress of transfer `id` (waiting up to `waitMs`, 60 000 at most, for it to end), or the window's transfers newest first (at most `limit`, 20): status (running, done, failed, cancelled), the table being copied, and for each table its status, rows read, inserted, committed and skipped, whether PostgreSQL's COPY was used, and the rows that failed (`errors`: row, column, code, the server's detail; at most `limit`, `errorsTotal` counts them). Never values of rows.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<TransfersAnswer, CommandError> {
        let engine = engine(session);
        let limit = params.limit.unwrap_or(LISTED).max(1);
        let cut = |mut state: TransferState| {
            for table in &mut state.tables {
                table.errors.truncate(limit);
            }
            state
        };
        if let Some(id) = params.id {
            if let Some(wait) = params.wait_ms {
                engine.wait(id, std::time::Duration::from_millis(wait.min(60_000)));
            }
            let state = engine.transfer_state(id).ok_or_else(|| {
                CommandError::with_code("unknownTransfer", t!("database-error-unknown-transfer", id = id))
            })?;
            return Ok(TransfersAnswer { transfers: vec![cut(state)], truncated: false });
        }
        let ids = engine.transfers();
        let transfers: Vec<TransferState> =
            ids.iter().rev().take(limit).filter_map(|id| engine.transfer_state(*id)).map(cut).collect();
        Ok(TransfersAnswer { truncated: ids.len() > transfers.len(), transfers })
    }
}

/// Which transfer to cancel.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CancelParams {
    /// The transfer.
    pub id: u64,
}

/// Whether a transfer was cancelled.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// Whether it was running and is being cancelled.
    pub cancelled: bool,
}

pub struct CancelTransfer;

impl Command for CancelTransfer {
    type Params = CancelParams;
    type Output = Cancelled;
    const ID: &'static str = "db.transfer.cancel";
    const TITLE: &'static str = "Cancel Data Transfer";
    const DESCRIPTION: &'static str = "Cancels transfer `id`: reading and writing stop on both servers, what the table being copied has not committed is rolled back, and the tables after it are not started. Answers whether it was running.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<Cancelled, CommandError> {
        Ok(Cancelled { cancelled: engine(session).cancel(params.id) })
    }
}

fn announce(session: &mut Session, state: &TransferState) {
    let done = state.tables.iter().filter(|table| table.status == ItemStatus::Done).count();
    let tables = state.tables.len();
    let (kind, body) = match state.status {
        OperationStatus::Running => return,
        OperationStatus::Cancelled => (
            NoteKind::Info,
            t!("database-transfer-cancelled", done = done, tables = tables, committed = state.rows_committed),
        ),
        OperationStatus::Failed => {
            let failed = state.tables.iter().find(|table| table.status == ItemStatus::Failed);
            let reason = failed.and_then(|table| table.error.as_ref()).map(describe).unwrap_or_default();
            let table = failed.map(|table| table.name.as_str()).unwrap_or_default();
            (
                NoteKind::Error,
                t!("database-transfer-failed", table = table, reason = reason, committed = state.rows_committed),
            )
        }
        OperationStatus::Done if state.rows_skipped > 0 => (
            NoteKind::Warning,
            t!(
                "database-transfer-done-skipped",
                rows = state.rows_inserted,
                tables = tables,
                skipped = state.rows_skipped
            ),
        ),
        OperationStatus::Done => {
            (NoteKind::Info, t!("database-transfer-done", rows = state.rows_inserted, tables = tables))
        }
    };
    post(
        session,
        NewNotification::message(Group::General, kind, body)
            .titled_by(zen_i18n::Message::new("database-transfer-title")),
    );
}

pub fn step(session: &mut Session) {
    let Some(engine) = session.state::<super::Database>().and_then(super::Database::started).cloned() else { return };
    let waiting: Vec<u64> = session
        .state::<Transfers>()
        .map(|transfers| transfers.started.iter().copied().filter(|id| !transfers.announced.contains(id)).collect())
        .unwrap_or_default();
    for id in waiting {
        let Some(state) = engine.transfer_state(id) else {
            session.state_mut::<Transfers>().announced.insert(id);
            continue;
        };
        if state.status == OperationStatus::Running {
            continue;
        }
        session.state_mut::<Transfers>().announced.insert(id);
        announce(session, &state);
    }
}

#[cfg(test)]
mod tests;

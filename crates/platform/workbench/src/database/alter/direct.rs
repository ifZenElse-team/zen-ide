use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::alter::{Change, ColumnPatch};
use zen_db::dialect::SqlDialect;
use zen_db::schema::{Found, ObjectKind, Snapshot};
use zen_i18n::t;

use super::{
    Alterations, Executed, LISTED, ObjectTab, Outcome, Structure, find, kind_text, preview_of, start, supports,
};
use crate::database::objects::{ObjectOpen, OpenParams, Page, SectionParams, ShowSection, size_text};

pub fn navigator_key(connection: &str) -> String {
    format!("navigator:{connection}")
}

impl Alterations {
    pub fn outcome(&self, key: &str) -> Option<&Outcome> {
        self.tabs.get(key).filter(|staged| staged.running.is_none())?.last.as_ref()
    }

    pub fn running(&self, key: &str) -> bool {
        self.tabs.get(key).is_some_and(|staged| staged.running.is_some())
    }
}

/// An object of a connection's tree to delete or empty.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DropParams {
    /// The connection's id.
    pub connection: String,
    /// The object's path as db.navigator.state gives it: `[db, schema]`, `[db, schema, folder, object]`, or a column, index, constraint or foreign key below a table.
    pub path: Vec<String>,
    /// Drop or empty what depends on it too (`CASCADE`), where the server can.
    pub cascade: bool,
    /// The `token` of a `confirmExecute` refusal.
    pub confirmed: Option<String>,
}

/// An object of a connection's tree to rename.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RenameParams {
    /// The connection's id.
    pub connection: String,
    /// The object's path: `[db, schema]`, `[db, schema, folder, object]` or a column of a table.
    pub path: Vec<String>,
    /// Its new name, without a routine's arguments.
    pub to: String,
    /// The `token` of a `confirmExecute` refusal.
    pub confirmed: Option<String>,
}

fn invalid(code: &str, message: String) -> CommandError {
    CommandError::with_code(code, message)
}

fn dialect(session: &mut Session, connection: &str) -> Result<SqlDialect, CommandError> {
    let entry = find(session, connection)?;
    crate::database::filters::dialect_of(session, connection).filter(|dialect| dialect.edits()).ok_or_else(|| {
        invalid("unsupported", t!("database-alter-reason-driver", subject = entry.source.driver.as_str()))
    })
}

fn drop_change(path: &[String], cascade: bool) -> Option<Change> {
    match path {
        [_, schema] => Some(Change::DropSchema { name: schema.clone(), cascade }),
        [_, schema, folder, name] => Some(Change::Drop {
            schema: schema.clone(),
            kind: ObjectKind::from_folder(folder)?,
            name: name.clone(),
            cascade,
        }),
        [_, schema, _, table, folder, name] => match folder.as_str() {
            "columns" => {
                Some(Change::DropColumn { schema: schema.clone(), table: table.clone(), column: name.clone(), cascade })
            }
            "indexes" => Some(Change::DropIndex { schema: schema.clone(), table: table.clone(), name: name.clone() }),
            "constraints" | "foreignKeys" => Some(Change::DropConstraint {
                schema: schema.clone(),
                table: table.clone(),
                name: name.clone(),
                cascade,
            }),
            _ => None,
        },
        _ => None,
    }
}

fn rename_change(path: &[String], to: &str) -> Option<Change> {
    let to = to.to_owned();
    match path {
        [_, schema] => Some(Change::RenameSchema { name: schema.clone(), to }),
        [_, schema, folder, name] => Some(Change::Rename {
            schema: schema.clone(),
            kind: ObjectKind::from_folder(folder)?,
            name: name.clone(),
            to,
        }),
        [_, schema, _, table, folder, name] if folder == "columns" => Some(Change::AlterColumn {
            schema: schema.clone(),
            table: table.clone(),
            column: name.clone(),
            patch: ColumnPatch { name: Some(to), ..ColumnPatch::default() },
        }),
        _ => None,
    }
}

fn rows_text(rows: Option<i64>, size: Option<u64>) -> Option<String> {
    let rows = rows.filter(|rows| *rows >= 0).map(|rows| t!("database-loss-rows", count = rows));
    let size = size.filter(|size| *size > 0).map(size_text);
    match (rows, size) {
        (Some(rows), Some(size)) => Some(format!("{rows}, {size}")),
        (rows, size) => rows.or(size),
    }
}

fn plain(name: &str) -> &str {
    name.split('(').next().unwrap_or(name)
}

fn object_loss(found: &Found<'_>, cascade: Option<bool>) -> Vec<String> {
    let object = found.object;
    let name = format!("{}.{}", found.schema.name, plain(&object.name));
    let mut said = vec![t!("database-loss-object", kind = kind_text(object.kind), name = name.as_str())];
    match object.kind {
        ObjectKind::Table | ObjectKind::ForeignTable | ObjectKind::MaterializedView => {
            said.push(match rows_text(object.rows, object.size) {
                Some(amount) => t!("database-loss-data", amount = amount),
                None => t!("database-loss-data-unknown"),
            });
            if object.kind != ObjectKind::ForeignTable && !object.columns.is_empty() {
                said.push(t!(
                    "database-loss-parts",
                    columns = object.columns.len(),
                    indexes = object.indexes.len(),
                    triggers = object.triggers.len()
                ));
            }
            let pointing = zen_db::schema::references(found.catalog, &found.schema.name, &object.name)
                .into_iter()
                .filter(|(schema, table, _)| !(schema == &found.schema.name && table == &object.name))
                .count();
            if pointing > 0 {
                said.push(t!("database-loss-references", count = pointing));
            }
        }
        ObjectKind::View => said.push(t!("database-loss-view")),
        _ => {}
    }
    if cascade == Some(true) {
        said.push(t!("database-loss-cascade"));
    }
    said
}

fn loss(snapshot: Option<&Snapshot>, path: &[String], cascade: Option<bool>) -> String {
    let mut said = Vec::new();
    match path {
        [catalog, schema] => {
            let count =
                snapshot.and_then(|snapshot| snapshot.schema(catalog, schema)).map_or(0, |known| known.objects.len());
            said.push(t!("database-loss-schema", name = schema.as_str(), count = count));
        }
        [_, _, _, _] => {
            if let Some(found) = snapshot.and_then(|snapshot| zen_db::schema::find_object(snapshot, path)) {
                said.extend(object_loss(&found, cascade));
            }
        }
        [_, _, _, table, folder, name] => {
            let (table, name) = (plain(table), name.as_str());
            said.push(match folder.as_str() {
                "columns" => t!("database-loss-column", name = name, table = table),
                "indexes" => t!("database-loss-index", name = name, table = table),
                _ => t!("database-loss-constraint", name = name, table = table),
            });
            if cascade == Some(true) {
                said.push(t!("database-loss-cascade"));
            }
        }
        _ => {}
    }
    said.join(" ")
}

fn truncate_loss(snapshot: Option<&Snapshot>, path: &[String]) -> String {
    let found = snapshot.and_then(|snapshot| zen_db::schema::find_object(snapshot, path));
    let name = path.get(1).zip(path.get(3)).map(|(schema, table)| format!("{schema}.{table}")).unwrap_or_default();
    match found.and_then(|found| rows_text(found.object.rows, found.object.size)) {
        Some(amount) => t!("database-loss-truncate", name = name.as_str(), amount = amount),
        None => t!("database-loss-truncate-unknown", name = name.as_str()),
    }
}

fn snapshot_of(session: &mut Session, connection: &str) -> Result<Option<Snapshot>, CommandError> {
    let entry = find(session, connection)?;
    crate::database::structure::ensure(session, &entry, true);
    Ok(session.state::<Structure>().and_then(|structure| structure.snapshot(connection)).cloned())
}

fn refuse_running(session: &Session, connection: &str) -> Result<(), CommandError> {
    let busy =
        session.state::<Alterations>().is_some_and(|alterations| alterations.running(&navigator_key(connection)));
    if busy {
        return Err(CommandError::with_code("saving", t!("database-alter-error-running")));
    }
    Ok(())
}

fn refuse_read_only(session: &mut Session, connection: &str) -> Result<(), CommandError> {
    let entry = find(session, connection)?;
    if crate::database::safety::read_only(session, &entry) {
        return Err(CommandError::with_code(
            crate::database::safety::READ_ONLY,
            t!("database-alter-error-read-only", name = entry.source.name.as_str()),
        ));
    }
    Ok(())
}

fn explained(error: CommandError, loss: &str, cascade: bool) -> CommandError {
    if error.code.as_deref() != Some(crate::database::safety::CONFIRM_EXECUTE) || loss.is_empty() {
        return error;
    }
    let mut error = error;
    error.message = loss.to_owned();
    if let Some(data) = error.data.as_mut() {
        data.insert("loss".to_owned(), serde_json::Value::String(loss.to_owned()));
        data.insert("cascade".to_owned(), serde_json::Value::Bool(cascade));
    }
    error
}

fn run_change(
    session: &mut Session,
    connection: &str,
    path: &[String],
    change: Change,
    confirmed: Option<String>,
    retry: (&'static str, serde_json::Value),
) -> Result<Executed, CommandError> {
    let data = ObjectTab { connection: connection.to_owned(), path: path.to_vec(), ..ObjectTab::default() };
    let changes = vec![change];
    let (_, all) = preview_of(session, &data, &changes, LISTED)?;
    start(session, &navigator_key(connection), &data, (changes, all), confirmed, retry)
}

pub struct ObjectDrop;

impl Command for ObjectDrop {
    type Params = DropParams;
    type Output = Executed;
    const ID: &'static str = "db.object.drop";
    const TITLE: &'static str = "Delete Database Object";
    const DESCRIPTION: &'static str = "Delete (⌦ in the Database Navigator): drops the object at `path` of connection `connection` at once, in the background, in the connection's Main context: a schema (`[db, schema]`), an object (`[db, schema, folder, object]`: table, view, materialised view, foreign table, function, procedure, sequence, type or event) or a column, index, constraint or foreign key of a table; `cascade` drops what depends on it too where the server can (PostgreSQL). It always asks first: refused with code `confirmExecute`, whose message and `loss` name what is lost (the rows and size of a table, its columns, indexes and triggers, the foreign keys pointing at it, a schema's objects), `cascade` whether the server can cascade, the statements and a `token`; run again with `confirmed: token`. Production safety applies as for db.structure.execute (`readOnly`, `agentReadOnly`, `agentNotAllowed`); `unsupported` when the server cannot. When it ends, tabs of what was dropped close and only its schema is read again. Answers the query (db.query.state).";

    fn run(&self, session: &mut Session, params: DropParams) -> Result<Executed, CommandError> {
        refuse_running(session, &params.connection)?;
        refuse_read_only(session, &params.connection)?;
        let dialect = dialect(session, &params.connection)?;
        let cascade = supports(Some(dialect)).cascade;
        let change = drop_change(&params.path, params.cascade && cascade)
            .ok_or_else(|| invalid("notDroppable", t!("database-error-not-droppable", path = params.path.join("/"))))?;
        let snapshot = snapshot_of(session, &params.connection)?;
        let loss = loss(snapshot.as_ref(), &params.path, cascade.then_some(params.cascade));
        let retry =
            serde_json::json!({ "connection": params.connection, "path": params.path, "cascade": params.cascade });
        run_change(session, &params.connection, &params.path, change, params.confirmed.clone(), (Self::ID, retry))
            .map_err(|error| explained(error, &loss, cascade))
    }
}

pub struct ObjectRename;

impl Command for ObjectRename {
    type Params = RenameParams;
    type Output = Executed;
    const ID: &'static str = "db.object.rename";
    const TITLE: &'static str = "Rename Database Object";
    const DESCRIPTION: &'static str = "Rename (F2 in the Database Navigator): renames the object at `path` of connection `connection` to `to` at once, in the background, in the connection's Main context: a schema (`[db, schema]`, PostgreSQL), an object (`[db, schema, folder, object]`) or a column of a table (`[db, schema, tables, table, columns, column]`). Refused with code `notRenamable` for anything else, `invalidName` for an empty or unchanged name, `unsupported` when the server cannot; production safety applies as for db.structure.execute (a type that confirms changes refuses with code `confirmExecute` and a `token`: run again with `confirmed: token`). When it ends, tabs of the object open again under the new name. Answers the query (db.query.state).";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<Executed, CommandError> {
        refuse_running(session, &params.connection)?;
        refuse_read_only(session, &params.connection)?;
        dialect(session, &params.connection)?;
        let to = params.to.trim();
        let current = params.path.last().map(|name| plain(name)).unwrap_or_default();
        if to.is_empty() || to == current {
            return Err(invalid("invalidName", t!("database-error-rename-name")));
        }
        let change = rename_change(&params.path, to)
            .ok_or_else(|| invalid("notRenamable", t!("database-error-not-renamable", path = params.path.join("/"))))?;
        let retry = serde_json::json!({ "connection": params.connection, "path": params.path, "to": to });
        run_change(session, &params.connection, &params.path, change, params.confirmed.clone(), (Self::ID, retry))
    }
}

pub struct ObjectTruncate;

impl Command for ObjectTruncate {
    type Params = DropParams;
    type Output = Executed;
    const ID: &'static str = "db.object.truncate";
    const TITLE: &'static str = "Truncate Table";
    const DESCRIPTION: &'static str = "Truncate (Tools ▸ in the Database Navigator): deletes every row of the table at `path` (`[db, schema, tables, table]`) of connection `connection` at once, in the background, in the connection's Main context; the table stays. `TRUNCATE TABLE` (with `cascade`, also the tables whose foreign keys point at it, PostgreSQL), `DELETE FROM` on SQLite. It always asks first: refused with code `confirmExecute`, whose message and `loss` name the rows and size lost, the statement and a `token`; run again with `confirmed: token`. Production safety applies as for db.structure.execute. Refused with code `notTruncatable` for anything but a table. Answers the query (db.query.state).";

    fn run(&self, session: &mut Session, params: DropParams) -> Result<Executed, CommandError> {
        refuse_running(session, &params.connection)?;
        refuse_read_only(session, &params.connection)?;
        let dialect = dialect(session, &params.connection)?;
        let [_, schema, folder, table] = params.path.as_slice() else {
            return Err(invalid("notTruncatable", t!("database-error-not-truncatable")));
        };
        if folder != ObjectKind::Table.folder() {
            return Err(invalid("notTruncatable", t!("database-error-not-truncatable")));
        }
        let cascade = dialect == SqlDialect::Postgres;
        let target = dialect.qualified(schema, table);
        let statement = match dialect {
            SqlDialect::Sqlite => format!("DELETE FROM {target}"),
            SqlDialect::Postgres if params.cascade => format!("TRUNCATE TABLE {target} CASCADE"),
            _ => format!("TRUNCATE TABLE {target}"),
        };
        let snapshot = snapshot_of(session, &params.connection)?;
        let mut loss = truncate_loss(snapshot.as_ref(), &params.path);
        if cascade && params.cascade {
            loss = format!("{loss} {}", t!("database-loss-truncate-cascade"));
        }
        let data =
            ObjectTab { connection: params.connection.clone(), path: params.path.clone(), ..ObjectTab::default() };
        let retry =
            serde_json::json!({ "connection": params.connection, "path": params.path, "cascade": params.cascade });
        start(
            session,
            &navigator_key(&params.connection),
            &data,
            (Vec::new(), vec![statement]),
            params.confirmed.clone(),
            (Self::ID, retry),
        )
        .map_err(|error| explained(error, &loss, cascade))
    }
}

/// What to create from the navigator.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CreateKind {
    /// A table of the schema.
    Table,
    /// A view of the schema.
    View,
    /// A sequence of the schema.
    Sequence,
    /// A function of the schema.
    Function,
    /// Another schema of the database.
    Schema,
    /// A column of the table.
    Column,
    /// An index of the table.
    Index,
}

/// Where to create what.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateParams {
    /// The connection's id.
    pub connection: String,
    /// The navigator row it is created from: a schema, a folder or object below it (`table`, `view`, `sequence`, `function`, `schema`), or a table or a node below it (`column`, `index`).
    pub path: Vec<String>,
    /// What to create.
    pub kind: CreateKind,
}

/// A creation form asked for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CreateAsked {
    /// The object tab the form belongs to, where db.structure.stage takes the change.
    pub tab: String,
    /// What is created.
    pub kind: CreateKind,
}

#[derive(Default)]
pub struct CreateRequest(pub Option<CreateAsked>);

pub struct NavigatorCreate;

impl Command for NavigatorCreate {
    type Params = CreateParams;
    type Output = CreateAsked;
    const ID: &'static str = "db.navigator.create";
    const TITLE: &'static str = "Create Database Object";
    const DESCRIPTION: &'static str = "Create ▸ in the Database Navigator: opens the tab of the schema (`table`, `view`, `sequence`, `function`, `schema`) or of the table (`column`, `index`) that `path` is in, on its Properties page, and the window's form for the new object there; nothing runs until the form is staged and the tab's changes are saved (db.structure.stage, db.structure.preview, db.structure.execute). An agent stages the change on the answered `tab` itself. Refused with code `notCreatable` when `path` has no such parent.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<CreateAsked, CommandError> {
        dialect(session, &params.connection)?;
        let in_table = matches!(params.kind, CreateKind::Column | CreateKind::Index);
        let parent = match (params.path.as_slice(), in_table) {
            ([catalog, schema, ..], false) => vec![catalog.clone(), schema.clone()],
            ([catalog, schema, folder, table, ..], true) if folder == ObjectKind::Table.folder() => {
                vec![catalog.clone(), schema.clone(), folder.clone(), table.clone()]
            }
            _ => return Err(invalid("notCreatable", t!("database-error-not-creatable", path = params.path.join("/")))),
        };
        let opened = ObjectOpen.run(
            session,
            OpenParams { connection: params.connection.clone(), path: parent, page: Some(Page::Properties) },
        )?;
        let tab = opened.tab.id;
        if in_table {
            let section = if params.kind == CreateKind::Column { "columns" } else { "indexes" };
            ShowSection.run(session, SectionParams { tab: Some(tab.clone()), section: section.to_owned() })?;
        }
        let asked = CreateAsked { tab, kind: params.kind };
        session.state_mut::<CreateRequest>().0 = Some(asked.clone());
        Ok(asked)
    }
}

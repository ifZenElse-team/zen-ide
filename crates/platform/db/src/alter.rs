mod rebuild;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::SqlDialect;
use crate::schema::{Catalog, Column, Constraint, ConstraintKind, Index, Object, ObjectKind, Reference, Schema};

/// A column to create.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ColumnSpec {
    /// Its name.
    pub name: String,
    /// Its type as the server writes it (`varchar(40)`, `bigint`).
    #[serde(rename = "type")]
    pub type_name: String,
    /// Whether it refuses nulls.
    pub not_null: bool,
    /// Its default, an SQL expression.
    pub default: Option<String>,
    /// Whether the server numbers it (identity, `AUTO_INCREMENT`, SQLite's `AUTOINCREMENT` primary key).
    pub identity: bool,
    /// Its comment.
    pub comment: Option<String>,
}

/// What changes in a column; absent fields stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ColumnPatch {
    /// Its new name.
    pub name: Option<String>,
    /// Its new type.
    #[serde(rename = "type")]
    pub type_name: Option<String>,
    /// Whether it refuses nulls from now on.
    pub not_null: Option<bool>,
    /// Its new default, an SQL expression; empty drops the default.
    pub default: Option<String>,
    /// Its new comment; empty drops the comment.
    pub comment: Option<String>,
}

/// The table a foreign key to create points at.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ReferenceSpec {
    /// The referenced table's schema; the constrained table's when absent.
    pub schema: Option<String>,
    /// The referenced table.
    pub table: String,
    /// The referenced columns, in order.
    pub columns: Vec<String>,
    /// What deleting a referenced row does: `no action`, `restrict`, `cascade`, `set null` or `set default`.
    pub on_delete: Option<String>,
    /// What updating a referenced key does, from the same choices.
    pub on_update: Option<String>,
}

/// A constraint to add.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintSpec {
    /// Its name; the server names it when absent.
    #[serde(default)]
    pub name: Option<String>,
    /// `primaryKey`, `unique`, `check` or `foreignKey`.
    pub kind: ConstraintKind,
    /// The columns it covers, in order; none for a check.
    #[serde(default)]
    pub columns: Vec<String>,
    /// A check's condition, an SQL expression.
    #[serde(default)]
    pub check: Option<String>,
    /// The table a foreign key points at.
    #[serde(default)]
    pub references: Option<ReferenceSpec>,
}

/// An index to create.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct IndexSpec {
    /// Its name; `<table>_<columns>_idx` (`_key` when unique) when absent.
    pub name: Option<String>,
    /// Its columns, in order.
    pub columns: Vec<String>,
    /// Whether its values are unique.
    pub unique: bool,
}

/// A sequence's settings to create it with; absent ones are the server's defaults.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SequenceSpec {
    /// Its first value.
    pub start: Option<i64>,
    /// Its step.
    pub increment: Option<i64>,
    /// Its smallest value.
    pub min: Option<i64>,
    /// Its largest value.
    pub max: Option<i64>,
    /// Whether it starts over after its last value.
    pub cycle: bool,
}

/// One structure change, staged until the script runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum Change {
    /// Creates a schema (a database on MySQL and MariaDB).
    CreateSchema {
        /// Its name.
        name: String,
    },
    /// Renames a schema.
    RenameSchema {
        /// Its name.
        /// Its name.
        /// Its name.
        name: String,
        /// Its new name.
        to: String,
    },
    /// Drops a schema; with `cascade`, with everything in it.
    DropSchema {
        name: String,
        /// Drop what it holds too.
        #[serde(default)]
        cascade: bool,
    },
    /// Sets or drops a schema's comment.
    CommentSchema {
        name: String,
        /// The comment; empty drops it.
        comment: String,
    },
    /// Creates a table with its columns and primary key.
    CreateTable {
        /// Its schema.
        schema: String,
        /// Its name.
        name: String,
        /// Its columns.
        columns: Vec<ColumnSpec>,
        /// The columns of its primary key, in order.
        #[serde(default, rename = "primaryKey")]
        primary_key: Vec<String>,
        /// Its comment.
        #[serde(default)]
        comment: Option<String>,
    },
    /// Creates a view of a query.
    CreateView {
        /// Its schema.
        schema: String,
        /// Its name.
        name: String,
        /// Its query, one `SELECT`.
        query: String,
    },
    /// Creates a sequence.
    CreateSequence {
        /// Its schema.
        schema: String,
        /// Its name.
        name: String,
        /// Its settings.
        #[serde(default)]
        settings: SequenceSpec,
    },
    /// Creates a function or procedure from its whole `CREATE` statement.
    CreateRoutine {
        /// Its schema.
        schema: String,
        /// Its name.
        name: String,
        /// Its `CREATE FUNCTION` or `CREATE PROCEDURE` statement.
        definition: String,
    },
    /// Renames an object.
    Rename {
        /// Its schema.
        schema: String,
        /// What it is.
        kind: ObjectKind,
        /// Its name; a routine's with its arguments, as its path names it (`total(integer)`).
        name: String,
        /// Its new name, without arguments.
        to: String,
    },
    /// Drops an object; with `cascade`, with the objects that depend on it.
    Drop {
        /// Its schema.
        schema: String,
        /// What it is.
        kind: ObjectKind,
        /// Its name; a routine's with its arguments.
        name: String,
        /// Drop what depends on it too.
        #[serde(default)]
        cascade: bool,
    },
    /// Sets or drops an object's comment.
    Comment {
        /// Its schema.
        schema: String,
        /// What it is.
        kind: ObjectKind,
        /// Its name; a routine's with its arguments.
        name: String,
        /// The comment; empty drops it.
        comment: String,
    },
    /// Adds a column to a table.
    AddColumn {
        /// The table's schema.
        schema: String,
        /// The table.
        table: String,
        /// The column.
        column: ColumnSpec,
    },
    /// Changes a column's name, type, nullability, default or comment.
    AlterColumn {
        /// The table's schema.
        schema: String,
        /// The table.
        table: String,
        /// The column's name.
        column: String,
        /// What changes.
        patch: ColumnPatch,
    },
    /// Drops a column; with `cascade`, with what depends on it.
    DropColumn {
        /// The table's schema.
        schema: String,
        /// The table.
        table: String,
        /// The column's name.
        column: String,
        /// Drop what depends on it too.
        #[serde(default)]
        cascade: bool,
    },
    /// Adds a primary key, unique key, check or foreign key to a table.
    AddConstraint {
        /// The table's schema.
        schema: String,
        /// The table.
        table: String,
        /// The constraint.
        constraint: ConstraintSpec,
    },
    /// Drops a constraint of a table; with `cascade`, with what depends on it.
    DropConstraint {
        /// The table's schema.
        schema: String,
        /// The table.
        table: String,
        /// The constraint's name.
        name: String,
        /// Drop what depends on it too.
        #[serde(default)]
        cascade: bool,
    },
    /// Creates an index of a table.
    CreateIndex {
        /// The table's schema.
        schema: String,
        /// The table.
        table: String,
        /// The index.
        index: IndexSpec,
    },
    /// Drops an index of a table.
    DropIndex {
        /// The table's schema.
        schema: String,
        /// The table.
        table: String,
        /// The index's name.
        name: String,
    },
}

impl Change {
    pub fn schema(&self) -> &str {
        match self {
            Change::CreateSchema { name }
            | Change::RenameSchema { name, .. }
            | Change::DropSchema { name, .. }
            | Change::CommentSchema { name, .. } => name,
            Change::CreateTable { schema, .. }
            | Change::CreateView { schema, .. }
            | Change::CreateSequence { schema, .. }
            | Change::CreateRoutine { schema, .. }
            | Change::Rename { schema, .. }
            | Change::Drop { schema, .. }
            | Change::Comment { schema, .. }
            | Change::AddColumn { schema, .. }
            | Change::AlterColumn { schema, .. }
            | Change::DropColumn { schema, .. }
            | Change::AddConstraint { schema, .. }
            | Change::DropConstraint { schema, .. }
            | Change::CreateIndex { schema, .. }
            | Change::DropIndex { schema, .. } => schema,
        }
    }

    pub fn inside(&self) -> Option<(ObjectKind, &str)> {
        match self {
            Change::AddColumn { table, .. }
            | Change::AlterColumn { table, .. }
            | Change::DropColumn { table, .. }
            | Change::AddConstraint { table, .. }
            | Change::DropConstraint { table, .. }
            | Change::CreateIndex { table, .. }
            | Change::DropIndex { table, .. } => Some((ObjectKind::Table, table)),
            Change::Comment { kind, name, .. } => Some((*kind, name)),
            _ => None,
        }
    }

    pub fn of_schema(&self) -> bool {
        matches!(self, Change::CreateSchema { .. } | Change::RenameSchema { .. } | Change::DropSchema { .. })
    }
}

/// Something to know before the script runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Note {
    /// SQLite cannot alter the table in place: it is created anew, its rows copied, the old one dropped and the new one renamed, its indexes and triggers created again.
    Rebuild { table: String },
    /// MySQL and MariaDB commit every structure statement on its own: a failure leaves the statements before it done.
    ImplicitCommit,
}

/// Why a change cannot be made on a server.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// The driver cannot change structure (ClickHouse here, key-value databases).
    Driver,
    /// SQLite has no schemas to create, rename or drop (they are attached files).
    Schemas,
    /// The server cannot rename this schema (MySQL and MariaDB databases).
    RenameSchema,
    /// The server has no sequences (MySQL, SQLite).
    Sequences,
    /// The server has no functions or procedures written in SQL (SQLite).
    Routines,
    /// The server keeps no comment on this kind of object.
    Comments,
    /// The server cannot rename this kind of object.
    Rename,
    /// The server has no `CASCADE` for this.
    Cascade,
    /// SQLite numbers only a single `INTEGER` primary key column.
    Identity,
    /// Exclusion constraints are not created here.
    ConstraintKind,
    /// A generated column cannot be changed here.
    Generated,
    /// MySQL restates a column whole to change it, and it has an attribute the IDE does not read (`ON UPDATE`, `INVISIBLE`, `SRID`).
    Restate,
    /// SQLite would rebuild the table, and its definition has something the rebuild would lose (`CHECK`, `GENERATED`, `AUTOINCREMENT`, `COLLATE`, `STRICT`, `WITHOUT ROWID`).
    RebuildLoses,
    /// No such schema.
    UnknownSchema,
    /// No such object.
    UnknownObject,
    /// No such column.
    UnknownColumn,
    /// No such constraint.
    UnknownConstraint,
    /// No such index.
    UnknownIndex,
    /// An object of that name already exists.
    Exists,
    /// A name, type, column or query is missing.
    Missing,
    /// SQL written into the change is not exactly one statement or expression.
    NotOneStatement,
    /// The schema holds objects: drop it with `cascade`.
    NotEmpty,
    /// A foreign key action other than `no action`, `restrict`, `cascade`, `set null` or `set default`.
    Action,
    /// The change does nothing.
    Nothing,
    /// SQLite renamed a column of the table earlier in the script and would rebuild it now, which needs its indexes' and triggers' texts as the server rewrote them: run the staged changes first.
    RunFirst,
}

/// A change refused, with why and what it is about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    /// The index of the change among those staged.
    pub change: usize,
    /// Why.
    pub reason: Reason,
    /// The object, column or word it is about.
    pub subject: String,
}

/// The DDL of staged changes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    /// Statements to run before the transaction, if one is opened (SQLite's `PRAGMA foreign_keys`).
    pub before: Vec<String>,
    /// The statements, in order, without delimiters.
    pub statements: Vec<String>,
    /// Whether the server runs structure changes inside a transaction, so the script can be all or nothing.
    pub transactional: bool,
    /// What to know before running it.
    pub notes: Vec<Note>,
}

impl Script {
    pub fn all(&self, transaction: bool) -> Vec<String> {
        let mut all = self.before.clone();
        let wrap = transaction && self.transactional && !self.statements.is_empty();
        if wrap {
            all.push("BEGIN".to_owned());
        }
        all.extend(self.statements.iter().cloned());
        if wrap {
            all.push("COMMIT".to_owned());
        }
        all
    }
}

pub fn text(statements: &[String]) -> String {
    statements.iter().map(|statement| format!("{statement};\n")).collect::<Vec<_>>().join("\n")
}

pub fn script(dialect: SqlDialect, catalog: &Catalog, changes: &[Change]) -> Result<Script, Refusal> {
    let mut writer = Writer {
        dialect,
        model: catalog.clone(),
        script: Script {
            transactional: matches!(dialect, SqlDialect::Postgres | SqlDialect::Sqlite),
            ..Script::default()
        },
        index: 0,
        rebuilt: Vec::new(),
        renamed: Vec::new(),
    };
    for (index, change) in changes.iter().enumerate() {
        writer.index = index;
        if !dialect.edits() {
            return Err(writer.refuse(Reason::Driver, dialect.driver()));
        }
        writer.change(change)?;
    }
    if writer.mysql() && writer.script.statements.len() > 1 {
        writer.script.notes.push(Note::ImplicitCommit);
    }
    Ok(writer.script)
}

pub fn routine_template(dialect: SqlDialect, schema: &str, name: &str, procedure: bool) -> String {
    let target = dialect.qualified(schema, name);
    match (dialect, procedure) {
        (SqlDialect::Postgres, false) => {
            format!("CREATE FUNCTION {target}()\nRETURNS integer\nLANGUAGE sql\nAS $$\nSELECT 1\n$$")
        }
        (SqlDialect::Postgres, true) => format!("CREATE PROCEDURE {target}()\nLANGUAGE sql\nAS $$\nSELECT 1\n$$"),
        (_, false) => format!("CREATE FUNCTION {target}()\nRETURNS INT\nDETERMINISTIC\nRETURN 1"),
        (_, true) => format!("CREATE PROCEDURE {target}()\nSELECT 1"),
    }
}

const ACTIONS: [&str; 5] = ["no action", "restrict", "cascade", "set null", "set default"];

struct Writer {
    dialect: SqlDialect,
    model: Catalog,
    script: Script,
    index: usize,
    rebuilt: Vec<(String, String)>,
    renamed: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
struct ColumnDef {
    name: String,
    type_name: String,
    not_null: bool,
    default: Option<String>,
    identity: bool,
    collation: Option<String>,
    comment: Option<String>,
}

impl ColumnDef {
    fn of_spec(spec: &ColumnSpec) -> Self {
        Self {
            name: spec.name.clone(),
            type_name: spec.type_name.clone(),
            not_null: spec.not_null,
            default: spec.default.clone().filter(|default| !default.trim().is_empty()),
            identity: spec.identity,
            collation: None,
            comment: spec.comment.clone().filter(|comment| !comment.is_empty()),
        }
    }

    fn of_column(column: &Column) -> Self {
        Self {
            name: column.name.clone(),
            type_name: column.type_name.clone(),
            not_null: !column.nullable,
            default: column.default.clone(),
            identity: column.identity.is_some(),
            collation: column.collation.clone(),
            comment: column.comment.clone(),
        }
    }

    fn column(&self, position: i32) -> Column {
        Column {
            name: self.name.clone(),
            position,
            type_name: self.type_name.clone(),
            nullable: !self.not_null,
            default: self.default.clone(),
            identity: self.identity.then(|| "byDefault".to_owned()),
            generated: None,
            collation: self.collation.clone(),
            comment: self.comment.clone(),
        }
    }
}

fn routine_parts(key: &str) -> (&str, Option<&str>) {
    match key.find('(') {
        Some(open) if key.ends_with(')') => (&key[..open], Some(&key[open + 1..key.len() - 1])),
        _ => (key, None),
    }
}

fn keyword(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Table => "TABLE",
        ObjectKind::ForeignTable => "FOREIGN TABLE",
        ObjectKind::View => "VIEW",
        ObjectKind::MaterializedView => "MATERIALIZED VIEW",
        ObjectKind::Function => "FUNCTION",
        ObjectKind::Procedure => "PROCEDURE",
        ObjectKind::Sequence => "SEQUENCE",
        ObjectKind::Type => "TYPE",
        ObjectKind::Event => "EVENT",
        ObjectKind::Package => "PACKAGE",
    }
}

fn blank(text: &str) -> bool {
    text.trim().is_empty()
}

impl Writer {
    fn refuse(&self, reason: Reason, subject: impl Into<String>) -> Refusal {
        Refusal { change: self.index, reason, subject: subject.into() }
    }

    fn mysql(&self) -> bool {
        matches!(self.dialect, SqlDialect::MySql | SqlDialect::MariaDb)
    }

    fn sqlite(&self) -> bool {
        self.dialect == SqlDialect::Sqlite
    }

    fn postgres(&self) -> bool {
        self.dialect == SqlDialect::Postgres
    }

    fn q(&self, name: &str) -> String {
        self.dialect.quote(name)
    }

    fn table(&self, schema: &str, name: &str) -> String {
        self.dialect.qualified(schema, name)
    }

    fn push(&mut self, statement: String) {
        self.script.statements.push(statement);
    }

    fn named(&self, name: &str) -> Result<(), Refusal> {
        if blank(name) { Err(self.refuse(Reason::Missing, "name")) } else { Ok(()) }
    }

    fn fragment(&self, text: &str, what: &str) -> Result<String, Refusal> {
        let trimmed = text.trim().trim_end_matches(';').trim_end();
        if trimmed.is_empty() {
            return Err(self.refuse(Reason::Missing, what));
        }
        let lexing = match self.dialect {
            SqlDialect::Postgres => crate::sql::Dialect::Postgres,
            SqlDialect::MySql | SqlDialect::MariaDb => crate::sql::Dialect::MySql,
            _ => crate::sql::Dialect::Standard,
        };
        let statements = crate::sql::split(trimmed, lexing);
        if statements.len() != 1 || statements.iter().any(|statement| statement.client) {
            return Err(self.refuse(Reason::NotOneStatement, what));
        }
        Ok(trimmed.to_owned())
    }

    fn action(&self, action: Option<&str>) -> Result<Option<&'static str>, Refusal> {
        let Some(action) = action.map(str::trim).filter(|action| !action.is_empty()) else { return Ok(None) };
        let lower = action.to_ascii_lowercase().replace('_', " ");
        let known = ACTIONS
            .iter()
            .find(|known| **known == lower)
            .ok_or_else(|| self.refuse(Reason::Action, action.to_owned()))?;
        Ok(Some(match *known {
            "no action" => "NO ACTION",
            "restrict" => "RESTRICT",
            "cascade" => "CASCADE",
            "set null" => "SET NULL",
            _ => "SET DEFAULT",
        }))
    }

    fn schema_of(&self, name: &str) -> Result<&Schema, Refusal> {
        self.model
            .schemas
            .iter()
            .find(|schema| schema.name == name)
            .ok_or_else(|| self.refuse(Reason::UnknownSchema, name))
    }

    fn schema_mut(&mut self, name: &str) -> Option<&mut Schema> {
        self.model.schemas.iter_mut().find(|schema| schema.name == name)
    }

    fn find(&self, schema: &str, kind: ObjectKind, key: &str) -> Result<&Object, Refusal> {
        self.schema_of(schema)?
            .objects
            .iter()
            .find(|object| object.kind == kind && (object.key() == key || object.name == key))
            .ok_or_else(|| self.refuse(Reason::UnknownObject, key))
    }

    fn object_mut(&mut self, schema: &str, kind: ObjectKind, key: &str) -> Option<&mut Object> {
        self.schema_mut(schema)?
            .objects
            .iter_mut()
            .find(|object| object.kind == kind && (object.key() == key || object.name == key))
    }

    fn exists(&self, schema: &str, name: &str) -> bool {
        self.model
            .schemas
            .iter()
            .find(|known| known.name == schema)
            .is_some_and(|known| known.objects.iter().any(|object| object.name == name && object.routine.is_none()))
    }

    fn fresh(&self, schema: &str, name: &str) -> Result<(), Refusal> {
        self.named(name)?;
        self.schema_of(schema)?;
        if self.exists(schema, name) { Err(self.refuse(Reason::Exists, name)) } else { Ok(()) }
    }

    fn columns(&self, columns: &[String]) -> Result<String, Refusal> {
        if columns.is_empty() || columns.iter().any(|column| blank(column)) {
            return Err(self.refuse(Reason::Missing, "columns"));
        }
        Ok(columns.iter().map(|column| self.q(column.trim())).collect::<Vec<_>>().join(", "))
    }

    fn column_def(&self, column: &ColumnDef, primary_key: bool) -> Result<String, Refusal> {
        self.named(&column.name)?;
        let mut def = format!("{} {}", self.q(&column.name), self.fragment(&column.type_name, "type")?);
        if let Some(collation) = column.collation.as_deref().filter(|collation| !blank(collation)) {
            def.push_str(&format!(" COLLATE {}", self.q(collation)));
        }
        if self.postgres() && column.identity {
            def.push_str(" GENERATED BY DEFAULT AS IDENTITY");
        }
        if self.sqlite() && column.identity {
            if !primary_key || !column.type_name.trim().eq_ignore_ascii_case("integer") {
                return Err(self.refuse(Reason::Identity, column.name.clone()));
            }
            def.push_str(" PRIMARY KEY AUTOINCREMENT");
        }
        if column.not_null {
            def.push_str(" NOT NULL");
        } else if self.mysql() {
            def.push_str(" NULL");
        }
        if let Some(default) = &column.default {
            def.push_str(&format!(" DEFAULT {}", self.fragment(default, "default")?));
        }
        if self.mysql() && column.identity {
            def.push_str(" AUTO_INCREMENT");
        }
        if let Some(comment) = &column.comment {
            if self.mysql() {
                def.push_str(&format!(" COMMENT {}", self.dialect.string(comment)));
            } else if self.sqlite() {
                return Err(self.refuse(Reason::Comments, column.name.clone()));
            }
        }
        Ok(def)
    }

    fn comment_on(&self, target: String, comment: Option<&str>) -> String {
        let value = comment
            .filter(|comment| !comment.is_empty())
            .map_or_else(|| "NULL".to_owned(), |text| self.dialect.string(text));
        format!("COMMENT ON {target} IS {value}")
    }

    fn change(&mut self, change: &Change) -> Result<(), Refusal> {
        match change {
            Change::CreateSchema { name } => self.create_schema(name),
            Change::RenameSchema { name, to } => self.rename_schema(name, to),
            Change::DropSchema { name, cascade } => self.drop_schema(name, *cascade),
            Change::CommentSchema { name, comment } => self.comment_schema(name, comment),
            Change::CreateTable { schema, name, columns, primary_key, comment } => {
                self.create_table(schema, name, columns, primary_key, comment.as_deref())
            }
            Change::CreateView { schema, name, query } => self.create_view(schema, name, query),
            Change::CreateSequence { schema, name, settings } => self.create_sequence(schema, name, settings),
            Change::CreateRoutine { schema, name, definition } => self.create_routine(schema, name, definition),
            Change::Rename { schema, kind, name, to } => self.rename(schema, *kind, name, to),
            Change::Drop { schema, kind, name, cascade } => self.drop(schema, *kind, name, *cascade),
            Change::Comment { schema, kind, name, comment } => self.comment(schema, *kind, name, comment),
            Change::AddColumn { schema, table, column } => self.add_column(schema, table, column),
            Change::AlterColumn { schema, table, column, patch } => self.alter_column(schema, table, column, patch),
            Change::DropColumn { schema, table, column, cascade } => self.drop_column(schema, table, column, *cascade),
            Change::AddConstraint { schema, table, constraint } => self.add_constraint(schema, table, constraint),
            Change::DropConstraint { schema, table, name, cascade } => {
                self.drop_constraint(schema, table, name, *cascade)
            }
            Change::CreateIndex { schema, table, index } => self.create_index(schema, table, index),
            Change::DropIndex { schema, table, name } => self.drop_index(schema, table, name),
        }
    }

    fn create_schema(&mut self, name: &str) -> Result<(), Refusal> {
        self.named(name)?;
        if self.sqlite() {
            return Err(self.refuse(Reason::Schemas, name));
        }
        if self.model.schemas.iter().any(|schema| schema.name == name) {
            return Err(self.refuse(Reason::Exists, name));
        }
        let keyword = if self.mysql() { "DATABASE" } else { "SCHEMA" };
        self.push(format!("CREATE {keyword} {}", self.q(name)));
        self.model.schemas.push(Schema {
            name: name.to_owned(),
            level: crate::schema::Level::Details,
            ..Schema::default()
        });
        Ok(())
    }

    fn rename_schema(&mut self, name: &str, to: &str) -> Result<(), Refusal> {
        self.named(to)?;
        if self.sqlite() {
            return Err(self.refuse(Reason::Schemas, name));
        }
        if self.mysql() {
            return Err(self.refuse(Reason::RenameSchema, name));
        }
        self.schema_of(name)?;
        if self.model.schemas.iter().any(|schema| schema.name == to) {
            return Err(self.refuse(Reason::Exists, to));
        }
        self.push(format!("ALTER SCHEMA {} RENAME TO {}", self.q(name), self.q(to)));
        if let Some(schema) = self.schema_mut(name) {
            to.clone_into(&mut schema.name);
        }
        Ok(())
    }

    fn drop_schema(&mut self, name: &str, cascade: bool) -> Result<(), Refusal> {
        if self.sqlite() {
            return Err(self.refuse(Reason::Schemas, name));
        }
        let empty = self.schema_of(name)?.objects.is_empty();
        let statement = if self.mysql() {
            if !cascade && !empty {
                return Err(self.refuse(Reason::NotEmpty, name));
            }
            format!("DROP DATABASE {}", self.q(name))
        } else {
            format!("DROP SCHEMA {}{}", self.q(name), if cascade { " CASCADE" } else { "" })
        };
        self.push(statement);
        self.model.schemas.retain(|schema| schema.name != name);
        Ok(())
    }

    fn comment_schema(&mut self, name: &str, comment: &str) -> Result<(), Refusal> {
        self.schema_of(name)?;
        let statement = match self.dialect {
            SqlDialect::Postgres => self.comment_on(format!("SCHEMA {}", self.q(name)), Some(comment)),
            SqlDialect::MariaDb => {
                format!("ALTER DATABASE {} COMMENT = {}", self.q(name), self.dialect.string(comment))
            }
            _ => return Err(self.refuse(Reason::Comments, name)),
        };
        self.push(statement);
        if let Some(schema) = self.schema_mut(name) {
            schema.comment = Some(comment.to_owned()).filter(|comment| !comment.is_empty());
        }
        Ok(())
    }

    fn create_table(
        &mut self,
        schema: &str,
        name: &str,
        columns: &[ColumnSpec],
        primary_key: &[String],
        comment: Option<&str>,
    ) -> Result<(), Refusal> {
        self.fresh(schema, name)?;
        if columns.is_empty() {
            return Err(self.refuse(Reason::Missing, "columns"));
        }
        let comment = comment.filter(|comment| !comment.is_empty());
        if self.sqlite() && comment.is_some() {
            return Err(self.refuse(Reason::Comments, name));
        }
        let mut key: Vec<String> = primary_key.iter().map(|column| column.trim().to_owned()).collect();
        if key.is_empty()
            && let [only] = columns.iter().filter(|column| column.identity).collect::<Vec<_>>().as_slice()
            && self.sqlite()
        {
            key.push(only.name.clone());
        }
        if let Some(missing) = key.iter().find(|column| !columns.iter().any(|known| known.name == **column)) {
            return Err(self.refuse(Reason::UnknownColumn, missing.clone()));
        }
        let inline_key = self.sqlite() && key.len() == 1 && columns.iter().any(|column| column.identity);
        let mut lines = Vec::new();
        for column in columns {
            let alone = inline_key && key[0] == column.name;
            lines.push(self.column_def(&ColumnDef::of_spec(column), alone)?);
        }
        if !key.is_empty() && !inline_key {
            lines.push(format!("PRIMARY KEY ({})", self.columns(&key)?));
        }
        let mut statement = format!("CREATE TABLE {} (\n    {}\n)", self.table(schema, name), lines.join(",\n    "));
        if let (true, Some(comment)) = (self.mysql(), comment) {
            statement.push_str(&format!(" COMMENT = {}", self.dialect.string(comment)));
        }
        self.push(statement);
        if self.postgres() {
            if comment.is_some() {
                let target = format!("TABLE {}", self.table(schema, name));
                let statement = self.comment_on(target, comment);
                self.push(statement);
            }
            for column in columns.iter().filter(|column| column.comment.as_deref().is_some_and(|text| !text.is_empty()))
            {
                let target = format!("COLUMN {}.{}", self.table(schema, name), self.q(&column.name));
                let statement = self.comment_on(target, column.comment.as_deref());
                self.push(statement);
            }
        }
        let mut object = Object::new(ObjectKind::Table, name);
        object.detailed = true;
        object.comment = comment.map(str::to_owned);
        object.columns = columns
            .iter()
            .enumerate()
            .map(|(index, column)| {
                let mut column = ColumnDef::of_spec(column).column(i32::try_from(index + 1).unwrap_or(i32::MAX));
                if key.contains(&column.name) {
                    column.nullable = false;
                }
                column
            })
            .collect();
        if !key.is_empty() {
            object.constraints.push(Constraint {
                name: format!("{name}_pkey"),
                kind: ConstraintKind::PrimaryKey,
                definition: format!("PRIMARY KEY ({})", self.columns(&key)?),
                columns: key,
                references: None,
                index: None,
                comment: None,
            });
        }
        if self.sqlite() {
            self.rebuilt.push((schema.to_owned(), name.to_owned()));
        }
        if let Some(found) = self.schema_mut(schema) {
            found.objects.push(object);
        }
        Ok(())
    }

    fn create_view(&mut self, schema: &str, name: &str, query: &str) -> Result<(), Refusal> {
        self.fresh(schema, name)?;
        let query = self.fragment(query, "query")?;
        self.push(format!("CREATE VIEW {} AS\n{query}", self.table(schema, name)));
        let mut object = Object::new(ObjectKind::View, name);
        object.definition = Some(query);
        if let Some(found) = self.schema_mut(schema) {
            found.objects.push(object);
        }
        Ok(())
    }

    fn create_sequence(&mut self, schema: &str, name: &str, settings: &SequenceSpec) -> Result<(), Refusal> {
        if !matches!(self.dialect, SqlDialect::Postgres | SqlDialect::MariaDb) {
            return Err(self.refuse(Reason::Sequences, name));
        }
        self.fresh(schema, name)?;
        let mut statement = format!("CREATE SEQUENCE {}", self.table(schema, name));
        if let Some(increment) = settings.increment {
            statement.push_str(&format!(" INCREMENT BY {increment}"));
        }
        if let Some(min) = settings.min {
            statement.push_str(&format!(" MINVALUE {min}"));
        }
        if let Some(max) = settings.max {
            statement.push_str(&format!(" MAXVALUE {max}"));
        }
        if let Some(start) = settings.start {
            statement.push_str(&format!(" START WITH {start}"));
        }
        if settings.cycle {
            statement.push_str(" CYCLE");
        }
        self.push(statement);
        if let Some(found) = self.schema_mut(schema) {
            found.objects.push(Object::new(ObjectKind::Sequence, name));
        }
        Ok(())
    }

    fn create_routine(&mut self, schema: &str, name: &str, definition: &str) -> Result<(), Refusal> {
        if self.sqlite() {
            return Err(self.refuse(Reason::Routines, name));
        }
        self.named(name)?;
        self.schema_of(schema)?;
        let text = self.fragment(definition, "definition")?;
        let words: Vec<String> =
            text.split_whitespace().take(5).map(|word| word.to_ascii_lowercase()).collect::<Vec<_>>();
        let creates = words.first().is_some_and(|word| word == "create")
            && words.iter().any(|word| word.starts_with("function") || word.starts_with("procedure"));
        if !creates {
            return Err(self.refuse(Reason::NotOneStatement, "definition"));
        }
        let procedure = words.iter().any(|word| word.starts_with("procedure"));
        self.push(text);
        let kind = if procedure { ObjectKind::Procedure } else { ObjectKind::Function };
        if let Some(found) = self.schema_mut(schema) {
            found.objects.push(Object::new(kind, name));
        }
        Ok(())
    }

    fn routine_target(&self, schema: &str, key: &str) -> String {
        let (name, arguments) = routine_parts(key);
        match (self.postgres(), arguments) {
            (true, Some(arguments)) => format!("{}({arguments})", self.table(schema, name)),
            _ => self.table(schema, name),
        }
    }

    fn rename(&mut self, schema: &str, kind: ObjectKind, name: &str, to: &str) -> Result<(), Refusal> {
        self.named(to)?;
        let object = self.find(schema, kind, name)?.clone();
        if self.exists(schema, to) && object.name != to {
            return Err(self.refuse(Reason::Exists, to));
        }
        let statement = match (self.dialect, kind) {
            (SqlDialect::Postgres, ObjectKind::Function | ObjectKind::Procedure) => {
                format!("ALTER {} {} RENAME TO {}", keyword(kind), self.routine_target(schema, name), self.q(to))
            }
            (
                SqlDialect::Postgres,
                ObjectKind::Table
                | ObjectKind::ForeignTable
                | ObjectKind::View
                | ObjectKind::MaterializedView
                | ObjectKind::Sequence
                | ObjectKind::Type,
            ) => format!("ALTER {} {} RENAME TO {}", keyword(kind), self.table(schema, &object.name), self.q(to)),
            (SqlDialect::MySql | SqlDialect::MariaDb, ObjectKind::Table | ObjectKind::View) => {
                format!("RENAME TABLE {} TO {}", self.table(schema, &object.name), self.table(schema, to))
            }
            (SqlDialect::MariaDb, ObjectKind::Sequence) => {
                format!("RENAME TABLE {} TO {}", self.table(schema, &object.name), self.table(schema, to))
            }
            (SqlDialect::MySql | SqlDialect::MariaDb, ObjectKind::Event) => {
                format!("ALTER EVENT {} RENAME TO {}", self.table(schema, &object.name), self.table(schema, to))
            }
            (SqlDialect::Sqlite, ObjectKind::Table) => {
                format!("ALTER TABLE {} RENAME TO {}", self.table(schema, &object.name), self.q(to))
            }
            _ => return Err(self.refuse(Reason::Rename, object.name.clone())),
        };
        self.push(statement);
        if let Some(found) = self.object_mut(schema, kind, name) {
            to.clone_into(&mut found.name);
        }
        Ok(())
    }

    fn drop(&mut self, schema: &str, kind: ObjectKind, name: &str, cascade: bool) -> Result<(), Refusal> {
        let object = self.find(schema, kind, name)?.clone();
        let target = match kind {
            ObjectKind::Function | ObjectKind::Procedure => self.routine_target(schema, name),
            _ => self.table(schema, &object.name),
        };
        let statement = match (self.dialect, kind) {
            (SqlDialect::Postgres, ObjectKind::Event) => return Err(self.refuse(Reason::UnknownObject, name)),
            (SqlDialect::Postgres, _) => {
                format!("DROP {} {target}{}", keyword(kind), if cascade { " CASCADE" } else { "" })
            }
            (_, _) if cascade => return Err(self.refuse(Reason::Cascade, object.name.clone())),
            (
                SqlDialect::MySql | SqlDialect::MariaDb,
                ObjectKind::Table | ObjectKind::View | ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Event,
            )
            | (SqlDialect::MariaDb, ObjectKind::Sequence)
            | (SqlDialect::Sqlite, ObjectKind::Table | ObjectKind::View) => format!("DROP {} {target}", keyword(kind)),
            _ => return Err(self.refuse(Reason::UnknownObject, object.name.clone())),
        };
        self.push(statement);
        if let Some(found) = self.schema_mut(schema) {
            found.objects.retain(|known| !(known.kind == kind && (known.key() == name || known.name == name)));
        }
        Ok(())
    }

    fn comment(&mut self, schema: &str, kind: ObjectKind, name: &str, comment: &str) -> Result<(), Refusal> {
        let object = self.find(schema, kind, name)?.clone();
        let text = self.dialect.string(comment);
        let statement = match (self.dialect, kind) {
            (SqlDialect::Postgres, ObjectKind::Function | ObjectKind::Procedure) => {
                self.comment_on(format!("{} {}", keyword(kind), self.routine_target(schema, name)), Some(comment))
            }
            (SqlDialect::Postgres, ObjectKind::Event) => return Err(self.refuse(Reason::Comments, name)),
            (SqlDialect::Postgres, _) => {
                self.comment_on(format!("{} {}", keyword(kind), self.table(schema, &object.name)), Some(comment))
            }
            (SqlDialect::MySql | SqlDialect::MariaDb, ObjectKind::Table) => {
                format!("ALTER TABLE {} COMMENT = {text}", self.table(schema, &object.name))
            }
            (
                SqlDialect::MySql | SqlDialect::MariaDb,
                ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Event,
            ) => {
                format!("ALTER {} {} COMMENT {text}", keyword(kind), self.table(schema, &object.name))
            }
            _ => return Err(self.refuse(Reason::Comments, object.name.clone())),
        };
        self.push(statement);
        if let Some(found) = self.object_mut(schema, kind, name) {
            found.comment = Some(comment.to_owned()).filter(|comment| !comment.is_empty());
        }
        Ok(())
    }

    fn table_object(&self, schema: &str, table: &str) -> Result<Object, Refusal> {
        self.find(schema, ObjectKind::Table, table).cloned()
    }

    fn column_of<'a>(&self, object: &'a Object, column: &str) -> Result<&'a Column, Refusal> {
        object
            .columns
            .iter()
            .find(|known| known.name == column)
            .ok_or_else(|| self.refuse(Reason::UnknownColumn, column))
    }

    fn store(&mut self, schema: &str, object: Object) {
        if let Some(found) = self.object_mut(schema, ObjectKind::Table, &object.name.clone()) {
            *found = object;
        }
    }

    fn add_column(&mut self, schema: &str, table: &str, spec: &ColumnSpec) -> Result<(), Refusal> {
        let mut object = self.table_object(schema, table)?;
        self.named(&spec.name)?;
        if object.columns.iter().any(|column| column.name == spec.name) {
            return Err(self.refuse(Reason::Exists, spec.name.clone()));
        }
        let def = ColumnDef::of_spec(spec);
        let position = i32::try_from(object.columns.len() + 1).unwrap_or(i32::MAX);
        if self.sqlite() && ((def.not_null && def.default.is_none()) || def.identity) {
            if def.identity {
                return Err(self.refuse(Reason::Identity, spec.name.clone()));
            }
            return self.rebuild(schema, &object.name, |object| {
                object.columns.push(def.column(position));
                Ok(())
            });
        }
        let line = self.column_def(&def, false)?;
        self.push(format!("ALTER TABLE {} ADD COLUMN {line}", self.table(schema, table)));
        if self.postgres() && def.comment.is_some() {
            let target = format!("COLUMN {}.{}", self.table(schema, table), self.q(&def.name));
            let statement = self.comment_on(target, def.comment.as_deref());
            self.push(statement);
        }
        object.columns.push(def.column(position));
        self.store(schema, object);
        Ok(())
    }

    fn alter_column(&mut self, schema: &str, table: &str, column: &str, patch: &ColumnPatch) -> Result<(), Refusal> {
        let object = self.table_object(schema, table)?;
        let current = self.column_of(&object, column)?.clone();
        let rename = patch.name.as_deref().map(str::trim).filter(|name| !name.is_empty() && *name != column);
        if let Some(to) = rename
            && object.columns.iter().any(|known| known.name == to)
        {
            return Err(self.refuse(Reason::Exists, to));
        }
        let type_name =
            patch.type_name.as_deref().map(str::trim).filter(|kind| !kind.is_empty() && *kind != current.type_name);
        let not_null = patch.not_null.filter(|not_null| *not_null == current.nullable);
        let default = patch
            .default
            .as_deref()
            .map(str::trim)
            .filter(|default| Some(*default) != current.default.as_deref().or(Some("")));
        let comment =
            patch.comment.as_deref().filter(|comment| Some(*comment) != current.comment.as_deref().or(Some("")));
        if rename.is_none() && type_name.is_none() && not_null.is_none() && default.is_none() && comment.is_none() {
            return Err(self.refuse(Reason::Nothing, column));
        }
        if current.generated.is_some() && (type_name.is_some() || default.is_some() || not_null.is_some()) {
            return Err(self.refuse(Reason::Generated, column));
        }
        let mut after = ColumnDef::of_column(&current);
        if let Some(type_name) = type_name {
            type_name.clone_into(&mut after.type_name);
        }
        if let Some(not_null) = not_null {
            after.not_null = not_null;
        }
        if let Some(default) = default {
            after.default = Some(default.to_owned()).filter(|default| !default.is_empty());
        }
        if let Some(comment) = comment {
            after.comment = Some(comment.to_owned()).filter(|comment| !comment.is_empty());
        }
        let target = self.table(schema, table);
        match self.dialect {
            SqlDialect::Postgres => {
                let at = format!("ALTER TABLE {target} ALTER COLUMN {}", self.q(column));
                if let Some(type_name) = type_name {
                    let type_name = self.fragment(type_name, "type")?;
                    self.push(format!("{at} TYPE {type_name}"));
                }
                if let Some(not_null) = not_null {
                    self.push(format!("{at} {} NOT NULL", if not_null { "SET" } else { "DROP" }));
                }
                if let Some(default) = default {
                    if default.is_empty() {
                        self.push(format!("{at} DROP DEFAULT"));
                    } else {
                        let default = self.fragment(default, "default")?;
                        self.push(format!("{at} SET DEFAULT {default}"));
                    }
                }
                if let Some(comment) = comment {
                    let statement = self.comment_on(format!("COLUMN {target}.{}", self.q(column)), Some(comment));
                    self.push(statement);
                }
                if let Some(to) = rename {
                    self.push(format!("ALTER TABLE {target} RENAME COLUMN {} TO {}", self.q(column), self.q(to)));
                }
            }
            SqlDialect::MySql | SqlDialect::MariaDb => {
                let only_default = type_name.is_none() && not_null.is_none() && comment.is_none();
                if only_default && rename.is_none() {
                    let default = default.unwrap_or_default();
                    if default.is_empty() {
                        self.push(format!("ALTER TABLE {target} ALTER COLUMN {} DROP DEFAULT", self.q(column)));
                    } else {
                        let default = self.fragment(default, "default")?;
                        self.push(format!(
                            "ALTER TABLE {target} ALTER COLUMN {} SET DEFAULT {default}",
                            self.q(column)
                        ));
                    }
                } else if only_default && default.is_none() {
                    let to = rename.unwrap_or(column);
                    self.push(format!("ALTER TABLE {target} RENAME COLUMN {} TO {}", self.q(column), self.q(to)));
                } else {
                    if current.generated.is_some() {
                        return Err(self.refuse(Reason::Generated, column));
                    }
                    if let Some(attribute) = rebuild::unrestated(object.definition.as_deref(), column) {
                        return Err(self.refuse(Reason::Restate, attribute));
                    }
                    let mut named = after.clone();
                    rename.unwrap_or(column).clone_into(&mut named.name);
                    let line = self.column_def(&named, false)?;
                    self.push(format!("ALTER TABLE {target} CHANGE COLUMN {} {line}", self.q(column)));
                }
            }
            _ => {
                if comment.is_some() {
                    return Err(self.refuse(Reason::Comments, column));
                }
                if type_name.is_some() || not_null.is_some() || default.is_some() {
                    let name = column.to_owned();
                    self.rebuild(schema, table, move |object| {
                        if let Some(found) = object.columns.iter_mut().find(|known| known.name == name) {
                            let mut changed = after.column(found.position);
                            changed.name.clone_from(&name);
                            *found = changed;
                        }
                        Ok(())
                    })?;
                }
                if let Some(to) = rename {
                    self.push(format!("ALTER TABLE {target} RENAME COLUMN {} TO {}", self.q(column), self.q(to)));
                    self.rename_column_in_model(schema, table, column, to);
                    self.renamed.push((schema.to_owned(), table.to_owned()));
                }
                return Ok(());
            }
        }
        let to = rename.unwrap_or(column).to_owned();
        let mut object = object;
        if let Some(found) = object.columns.iter_mut().find(|known| known.name == column) {
            let mut changed = after.column(found.position);
            changed.name.clone_from(&to);
            changed.generated.clone_from(&found.generated);
            *found = changed;
        }
        rename_in(&mut object, column, &to);
        self.store(schema, object);
        Ok(())
    }

    fn rename_column_in_model(&mut self, schema: &str, table: &str, column: &str, to: &str) {
        if let Some(object) = self.object_mut(schema, ObjectKind::Table, table) {
            if let Some(found) = object.columns.iter_mut().find(|known| known.name == column) {
                to.clone_into(&mut found.name);
            }
            rename_in(object, column, to);
        }
    }

    fn drop_column(&mut self, schema: &str, table: &str, column: &str, cascade: bool) -> Result<(), Refusal> {
        let mut object = self.table_object(schema, table)?;
        self.column_of(&object, column)?;
        if cascade && !self.postgres() {
            return Err(self.refuse(Reason::Cascade, column));
        }
        self.push(format!(
            "ALTER TABLE {} DROP COLUMN {}{}",
            self.table(schema, table),
            self.q(column),
            if cascade { " CASCADE" } else { "" }
        ));
        object.columns.retain(|known| known.name != column);
        object.constraints.retain(|constraint| !constraint.columns.iter().any(|known| known == column));
        object.indexes.retain(|index| !index.columns.iter().any(|known| known == column));
        self.store(schema, object);
        Ok(())
    }

    fn constraint_clause(&self, schema: &str, spec: &ConstraintSpec) -> Result<String, Refusal> {
        let clause = match spec.kind {
            ConstraintKind::PrimaryKey => format!("PRIMARY KEY ({})", self.columns(&spec.columns)?),
            ConstraintKind::Unique => format!("UNIQUE ({})", self.columns(&spec.columns)?),
            ConstraintKind::Check => {
                format!("CHECK ({})", self.fragment(spec.check.as_deref().unwrap_or_default(), "check")?)
            }
            ConstraintKind::ForeignKey => {
                let reference = spec.references.as_ref().ok_or_else(|| self.refuse(Reason::Missing, "references"))?;
                self.named(&reference.table)?;
                let target_schema = reference.schema.as_deref().filter(|schema| !blank(schema)).unwrap_or(schema);
                let target =
                    if self.sqlite() { self.q(&reference.table) } else { self.table(target_schema, &reference.table) };
                if reference.columns.len() != spec.columns.len() {
                    return Err(self.refuse(Reason::Missing, "references"));
                }
                let mut clause = format!(
                    "FOREIGN KEY ({}) REFERENCES {target} ({})",
                    self.columns(&spec.columns)?,
                    self.columns(&reference.columns)?
                );
                if let Some(action) = self.action(reference.on_delete.as_deref())? {
                    clause.push_str(&format!(" ON DELETE {action}"));
                }
                if let Some(action) = self.action(reference.on_update.as_deref())? {
                    clause.push_str(&format!(" ON UPDATE {action}"));
                }
                clause
            }
            ConstraintKind::Exclusion => return Err(self.refuse(Reason::ConstraintKind, "exclusion")),
        };
        let name = spec.name.as_deref().map(str::trim).filter(|name| !name.is_empty());
        Ok(match name {
            Some(_) if self.mysql() && spec.kind == ConstraintKind::PrimaryKey => clause,
            Some(name) => format!("CONSTRAINT {} {clause}", self.q(name)),
            None => clause,
        })
    }

    fn add_constraint(&mut self, schema: &str, table: &str, spec: &ConstraintSpec) -> Result<(), Refusal> {
        let mut object = self.table_object(schema, table)?;
        for column in &spec.columns {
            self.column_of(&object, column.trim())?;
        }
        let name = spec.name.as_deref().map(str::trim).filter(|name| !name.is_empty());
        if let Some(name) = name
            && object.constraints.iter().any(|known| known.name == name)
        {
            return Err(self.refuse(Reason::Exists, name));
        }
        let clause = self.constraint_clause(schema, spec)?;
        let constraint = Constraint {
            name: name.map_or_else(|| default_name(table, &spec.columns, spec.kind), str::to_owned),
            kind: spec.kind,
            columns: spec.columns.iter().map(|column| column.trim().to_owned()).collect(),
            definition: clause.clone(),
            references: spec.references.as_ref().map(|reference| Reference {
                schema: reference.schema.clone().unwrap_or_else(|| schema.to_owned()),
                table: reference.table.clone(),
                columns: reference.columns.clone(),
                on_update: reference.on_update.clone().unwrap_or_default(),
                on_delete: reference.on_delete.clone().unwrap_or_default(),
            }),
            index: None,
            comment: None,
        };
        if self.sqlite() {
            return self.rebuild(schema, table, move |object| {
                if constraint.kind == ConstraintKind::PrimaryKey {
                    object.constraints.retain(|known| known.kind != ConstraintKind::PrimaryKey);
                }
                object.constraints.push(constraint);
                Ok(())
            });
        }
        self.push(format!("ALTER TABLE {} ADD {clause}", self.table(schema, table)));
        object.constraints.push(constraint);
        self.store(schema, object);
        Ok(())
    }

    fn drop_constraint(&mut self, schema: &str, table: &str, name: &str, cascade: bool) -> Result<(), Refusal> {
        let mut object = self.table_object(schema, table)?;
        let known = object.constraints.iter().find(|known| known.name == name).cloned();
        if cascade && !self.postgres() {
            return Err(self.refuse(Reason::Cascade, name));
        }
        let target = self.table(schema, table);
        match self.dialect {
            SqlDialect::Postgres => {
                self.push(format!(
                    "ALTER TABLE {target} DROP CONSTRAINT {}{}",
                    self.q(name),
                    if cascade { " CASCADE" } else { "" }
                ));
            }
            SqlDialect::MySql | SqlDialect::MariaDb => {
                let statement = match known.as_ref().map(|known| known.kind) {
                    Some(ConstraintKind::PrimaryKey) => format!("ALTER TABLE {target} DROP PRIMARY KEY"),
                    Some(ConstraintKind::ForeignKey) => {
                        format!("ALTER TABLE {target} DROP FOREIGN KEY {}", self.q(name))
                    }
                    Some(ConstraintKind::Unique) => format!("ALTER TABLE {target} DROP INDEX {}", self.q(name)),
                    Some(ConstraintKind::Check) if self.dialect == SqlDialect::MySql => {
                        format!("ALTER TABLE {target} DROP CHECK {}", self.q(name))
                    }
                    _ => format!("ALTER TABLE {target} DROP CONSTRAINT {}", self.q(name)),
                };
                self.push(statement);
            }
            _ => {
                if known.is_none() {
                    return Err(self.refuse(Reason::UnknownConstraint, name));
                }
                let name = name.to_owned();
                return self.rebuild(schema, table, move |object| {
                    object.constraints.retain(|known| known.name != name);
                    Ok(())
                });
            }
        }
        object.constraints.retain(|known| known.name != name);
        object.indexes.retain(|index| index.constraint.as_deref() != Some(name));
        self.store(schema, object);
        Ok(())
    }

    fn create_index(&mut self, schema: &str, table: &str, spec: &IndexSpec) -> Result<(), Refusal> {
        let mut object = self.table_object(schema, table)?;
        for column in &spec.columns {
            self.column_of(&object, column.trim())?;
        }
        let columns = self.columns(&spec.columns)?;
        let name = spec
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| index_name(table, &spec.columns, spec.unique), str::to_owned);
        if object.indexes.iter().any(|index| index.name == name) {
            return Err(self.refuse(Reason::Exists, name));
        }
        let unique = if spec.unique { "UNIQUE " } else { "" };
        let statement = if self.sqlite() {
            format!("CREATE {unique}INDEX {} ON {} ({columns})", self.table(schema, &name), self.q(table))
        } else {
            format!("CREATE {unique}INDEX {} ON {} ({columns})", self.q(&name), self.table(schema, table))
        };
        let definition = if self.sqlite() {
            format!("CREATE {unique}INDEX {} ON {} ({columns})", self.q(&name), self.q(table))
        } else {
            statement.clone()
        };
        self.push(statement);
        object.indexes.push(Index {
            name,
            columns: spec.columns.iter().map(|column| column.trim().to_owned()).collect(),
            unique: spec.unique,
            definition,
            ..Index::default()
        });
        self.store(schema, object);
        Ok(())
    }

    fn drop_index(&mut self, schema: &str, table: &str, name: &str) -> Result<(), Refusal> {
        let mut object = self.table_object(schema, table)?;
        if !object.indexes.iter().any(|index| index.name == name) {
            return Err(self.refuse(Reason::UnknownIndex, name));
        }
        let statement = if self.mysql() {
            format!("DROP INDEX {} ON {}", self.q(name), self.table(schema, table))
        } else {
            format!("DROP INDEX {}", self.table(schema, name))
        };
        self.push(statement);
        object.indexes.retain(|index| index.name != name);
        self.store(schema, object);
        Ok(())
    }
}

fn rename_in(object: &mut Object, column: &str, to: &str) {
    for constraint in &mut object.constraints {
        for known in &mut constraint.columns {
            if known == column {
                to.clone_into(known);
            }
        }
    }
    for index in &mut object.indexes {
        for known in &mut index.columns {
            if known == column {
                to.clone_into(known);
            }
        }
    }
}

fn default_name(table: &str, columns: &[String], kind: ConstraintKind) -> String {
    let suffix = match kind {
        ConstraintKind::PrimaryKey => "pkey",
        ConstraintKind::Unique => "key",
        ConstraintKind::Check => "check",
        ConstraintKind::Exclusion => "excl",
        ConstraintKind::ForeignKey => "fkey",
    };
    let mut parts = vec![table.to_owned()];
    parts.extend(columns.iter().map(|column| column.trim().to_owned()));
    parts.push(suffix.to_owned());
    parts.join("_")
}

fn index_name(table: &str, columns: &[String], unique: bool) -> String {
    let mut parts = vec![table.to_owned()];
    parts.extend(columns.iter().map(|column| column.trim().to_owned()));
    parts.push(if unique { "key" } else { "idx" }.to_owned());
    parts.join("_")
}

#[cfg(test)]
mod tests;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use crate::dialect::Placeholders;
use crate::dialect::SqlDialect;
use crate::driver::Param;
use crate::schema::{ConstraintKind, Object};
use crate::value::{Column, Typed, Value};

/// What identifies a row of a result when its changes are saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum KeyKind {
    /// The table's primary key.
    PrimaryKey,
    /// A unique key whose columns are never null.
    UniqueKey,
    /// The row's physical address (PostgreSQL's `ctid`) read with the rows.
    RowId,
    /// A key the person chose and saved for the table.
    Virtual,
    /// Every column, as the person chose: several rows may change at once.
    AllColumns,
}

/// The columns that identify a row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RowKey {
    /// What kind of key it is.
    pub kind: KeyKind,
    /// The result columns it is made of, from 0.
    pub columns: Vec<usize>,
    /// Their names in the table.
    pub names: Vec<String>,
}

/// Why a result cannot be edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ReadOnly {
    /// The connection is read-only.
    Connection,
    /// The connection's driver cannot edit results.
    Driver,
    /// The rows come from no table: computed values or a statement that is not a query.
    NoTable,
    /// The rows join several tables.
    SeveralTables,
    /// The table's structure is not known yet; it is read when the connection is connected.
    UnknownTable,
    /// No key identifies the rows: no primary key or unique key of non-null columns is in the result.
    NoKey,
    /// The database has no row identity a change could find one row by (ClickHouse).
    NoRowIdentity,
}

/// A column of a result as editing sees it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EditColumn {
    /// The result column, from 0.
    pub index: usize,
    /// Its name in the table; the result's name when it reads no column of the table.
    pub name: String,
    /// Its type as the table declares it; the result's type otherwise.
    #[serde(rename = "type")]
    pub type_name: String,
    /// Whether its cells can be changed.
    pub editable: bool,
    /// Whether it accepts `NULL`.
    pub nullable: bool,
    /// Its default expression, when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Whether it reads a column of the table.
    pub sourced: bool,
}

/// Whether a result can be edited, and how its rows are identified.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Editing {
    /// Whether staged changes can be saved.
    pub editable: bool,
    /// Why not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ReadOnly>,
    /// The table, qualified and quoted as SQL needs it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// The key that identifies rows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<RowKey>,
    /// The result's columns.
    pub columns: Vec<EditColumn>,
    /// The dialect its statements are written in.
    #[serde(skip)]
    pub dialect: SqlDialect,
    /// Whether the rows are documents of a collection, saved with MongoDB shell statements instead of SQL.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub documents: bool,
    /// The shell expression of the collection documents are saved to, such as `db.getSiblingDB("shop").getCollection("orders")`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
}

impl Editing {
    pub fn read_only(reason: ReadOnly, columns: &[Column]) -> Self {
        Self {
            editable: false,
            reason: Some(reason),
            table: None,
            key: None,
            columns: columns
                .iter()
                .enumerate()
                .map(|(index, column)| EditColumn {
                    index,
                    name: column.name.clone(),
                    type_name: column.type_name.clone(),
                    editable: false,
                    nullable: true,
                    default: None,
                    sourced: false,
                })
                .collect(),
            dialect: SqlDialect::default(),
            documents: false,
            target: None,
        }
    }
}

/// A key the person chose for a table without one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind", content = "columns")]
pub enum KeyChoice {
    /// These columns of the table, by name.
    Columns(Vec<String>),
    /// Every column.
    AllColumns,
}

pub fn edits_documents(capabilities: &crate::model::Capabilities) -> bool {
    capabilities.language == "mongo" && capabilities.row_identity.as_deref() == Some("_id")
}

pub fn edits(capabilities: &crate::model::Capabilities) -> bool {
    edit_dialect(capabilities).is_ok() || edits_documents(capabilities)
}

pub fn parse_cell(editing: &Editing, text: &str, type_name: &str) -> Result<Value, InputError> {
    if editing.documents { document::parse_input(text, type_name) } else { parse_input(text, type_name) }
}

pub fn edit_dialect(capabilities: &crate::model::Capabilities) -> Result<SqlDialect, ReadOnly> {
    let dialect = SqlDialect::of_language(&capabilities.language).ok_or(ReadOnly::Driver)?;
    if capabilities.row_identity.is_none() || !dialect.edits() {
        return Err(ReadOnly::NoRowIdentity);
    }
    Ok(dialect)
}

pub fn source_table(columns: &[Column]) -> Result<u32, ReadOnly> {
    let mut tables = columns.iter().filter_map(|column| column.source.map(|source| source.table));
    let first = tables.next().ok_or(ReadOnly::NoTable)?;
    if tables.all(|table| table == first) { Ok(first) } else { Err(ReadOnly::SeveralTables) }
}

pub fn identify(
    columns: &[Column],
    schema: &str,
    object: &Object,
    choice: Option<&KeyChoice>,
    dialect: SqlDialect,
) -> Editing {
    let table = object.oid;
    let column_of = |index: usize| -> Option<&crate::schema::Column> {
        let column = columns.get(index)?;
        let source = column.source?;
        if source.table != table {
            return None;
        }
        if source.position > 0 {
            return object.columns.iter().find(|known| known.position == i32::from(source.position));
        }
        let origin = column.origin.as_deref()?;
        object.columns.iter().find(|known| known.name == origin)
    };
    let edit_columns: Vec<EditColumn> = columns
        .iter()
        .enumerate()
        .map(|(index, column)| match column_of(index) {
            Some(known) => EditColumn {
                index,
                name: known.name.clone(),
                type_name: known.type_name.clone(),
                editable: known.generated.is_none() && known.identity.as_deref() != Some("always"),
                nullable: known.nullable,
                default: known.default.clone().or_else(|| known.identity.as_ref().map(|_| "identity".to_owned())),
                sourced: true,
            },
            None => EditColumn {
                index,
                name: column.name.clone(),
                type_name: column.type_name.clone(),
                editable: false,
                nullable: true,
                default: None,
                sourced: false,
            },
        })
        .collect();
    let sourced: Vec<&EditColumn> = edit_columns.iter().filter(|column| column.sourced).collect();
    let find = |name: &str| sourced.iter().find(|column| column.name == name).map(|column| column.index);
    let key_of = |names: &[String]| -> Option<Vec<usize>> {
        if names.is_empty() {
            return None;
        }
        names.iter().map(|name| find(name)).collect()
    };
    let not_null = |name: &str| object.columns.iter().any(|known| known.name == name && !known.nullable);
    let primary = object
        .constraints
        .iter()
        .find(|constraint| constraint.kind == ConstraintKind::PrimaryKey)
        .and_then(|constraint| key_of(&constraint.columns).map(|found| (constraint.columns.clone(), found)));
    let unique = || {
        object
            .constraints
            .iter()
            .filter(|constraint| constraint.kind == ConstraintKind::Unique)
            .filter(|constraint| constraint.columns.iter().all(|column| not_null(column)))
            .find_map(|constraint| key_of(&constraint.columns).map(|found| (constraint.columns.clone(), found)))
    };
    let row_id = || {
        if !dialect.row_id() {
            return None;
        }
        columns
            .iter()
            .position(|column| column.name == "ctid" && column.type_name == "tid")
            .map(|index| (vec!["ctid".to_owned()], vec![index]))
    };
    let chosen = || match choice {
        Some(KeyChoice::Columns(names)) => key_of(names).map(|found| (KeyKind::Virtual, names.clone(), found)),
        Some(KeyChoice::AllColumns) if !sourced.is_empty() => Some((
            KeyKind::AllColumns,
            sourced.iter().map(|column| column.name.clone()).collect(),
            sourced.iter().map(|column| column.index).collect(),
        )),
        _ => None,
    };
    let key = primary
        .map(|(names, found)| (KeyKind::PrimaryKey, names, found))
        .or_else(|| unique().map(|(names, found)| (KeyKind::UniqueKey, names, found)))
        .or_else(|| row_id().map(|(names, found)| (KeyKind::RowId, names, found)))
        .or_else(chosen)
        .map(|(kind, names, columns)| RowKey { kind, columns, names });
    let reason = if sourced.is_empty() {
        Some(ReadOnly::NoTable)
    } else if key.is_none() {
        Some(ReadOnly::NoKey)
    } else {
        None
    };
    Editing {
        editable: reason.is_none(),
        reason,
        table: Some(dialect.qualified(schema, &object.name)),
        key,
        columns: edit_columns,
        dialect,
        documents: false,
        target: None,
    }
}

/// Why typed text is not a value of its column's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum InputError {
    /// Not a whole number.
    Integer,
    /// Not a number.
    Number,
    /// Not true or false.
    Boolean,
    /// Not valid JSON.
    Json,
    /// Not a UUID.
    Uuid,
    /// Not bytes written as `\x` and hexadecimal digits.
    Bytes,
    /// Not an ObjectId of 24 hexadecimal digits.
    ObjectId,
}

fn base_type(type_name: &str) -> String {
    let lower = type_name.trim().to_ascii_lowercase();
    let base = lower.split('(').next().unwrap_or_default().trim();
    base.strip_suffix(" unsigned").unwrap_or(base).trim().to_owned()
}

pub fn parse_input(text: &str, type_name: &str) -> Result<Value, InputError> {
    let base = base_type(type_name);
    if base.ends_with("[]") || base.starts_with('_') {
        return Ok(Value::Other(Typed { text: text.to_owned(), type_name: type_name.to_owned() }));
    }
    let trimmed = text.trim();
    match base.as_str() {
        "smallint" | "integer" | "bigint" | "int" | "int2" | "int4" | "int8" | "smallserial" | "serial"
        | "bigserial" | "serial2" | "serial4" | "serial8" | "oid" | "tinyint" | "mediumint" | "year" => {
            trimmed.parse::<i64>().map(Value::Integer).map_err(|_| InputError::Integer)
        }
        "numeric" | "decimal" => {
            let number = trimmed.parse::<f64>().is_ok() || matches!(trimmed, "NaN" | "Infinity" | "-Infinity");
            if number { Ok(Value::Decimal(trimmed.to_owned())) } else { Err(InputError::Number) }
        }
        "real" | "float4" | "double precision" | "float8" | "float" | "double" => match trimmed {
            "NaN" => Ok(Value::Float(f64::NAN)),
            "Infinity" => Ok(Value::Float(f64::INFINITY)),
            "-Infinity" => Ok(Value::Float(f64::NEG_INFINITY)),
            _ => trimmed.parse::<f64>().map(Value::Float).map_err(|_| InputError::Number),
        },
        "boolean" | "bool" => match trimmed.to_ascii_lowercase().as_str() {
            "t" | "true" | "yes" | "y" | "on" | "1" => Ok(Value::Bool(true)),
            "f" | "false" | "no" | "n" | "off" | "0" => Ok(Value::Bool(false)),
            _ => Err(InputError::Boolean),
        },
        "json" | "jsonb" => serde_json::from_str::<serde_json::Value>(text)
            .map(|_| Value::Json(text.to_owned()))
            .map_err(|_| InputError::Json),
        "uuid" => {
            let hex: String = trimmed.chars().filter(|c| *c != '-').collect();
            let shaped = trimmed.len() == 36 && [8, 13, 18, 23].iter().all(|at| trimmed.as_bytes()[*at] == b'-');
            if shaped && hex.len() == 32 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
                Ok(Value::Uuid(trimmed.to_ascii_lowercase()))
            } else {
                Err(InputError::Uuid)
            }
        }
        "bytea" | "blob" | "binary" | "varbinary" | "tinyblob" | "mediumblob" | "longblob" => {
            let digits = trimmed.strip_prefix("\\x").ok_or(InputError::Bytes)?.as_bytes();
            if !digits.len().is_multiple_of(2) || !digits.iter().all(u8::is_ascii_hexdigit) {
                return Err(InputError::Bytes);
            }
            digits
                .chunks(2)
                .map(|pair| {
                    std::str::from_utf8(pair)
                        .ok()
                        .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                        .ok_or(InputError::Bytes)
                })
                .collect::<Result<Vec<u8>, _>>()
                .map(Value::Bytes)
        }
        "text" | "character varying" | "varchar" | "character" | "char" | "bpchar" | "name" | "citext" => {
            Ok(Value::Text(text.to_owned()))
        }
        "interval" => Ok(Value::Interval(trimmed.to_owned())),
        _ if base.starts_with("timestamp") || base.starts_with("time") || base.starts_with("date") => {
            Ok(Value::Temporal(Typed { text: trimmed.to_owned(), type_name: type_name.to_owned() }))
        }
        _ => Ok(Value::Other(Typed { text: text.to_owned(), type_name: type_name.to_owned() })),
    }
}

fn float_text(value: f64) -> String {
    if value.is_nan() {
        "NaN".to_owned()
    } else if value.is_infinite() {
        if value > 0.0 { "Infinity".to_owned() } else { "-Infinity".to_owned() }
    } else {
        value.to_string()
    }
}

pub fn param_text(value: &Value) -> Param {
    Some(match value {
        Value::Null => return None,
        Value::Bool(value) => if *value { "t" } else { "f" }.to_owned(),
        Value::Integer(value) => value.to_string(),
        Value::Float(value) => float_text(*value),
        Value::Decimal(text) | Value::Text(text) | Value::Interval(text) | Value::Uuid(text) | Value::Json(text) => {
            text.clone()
        }
        Value::Bytes(bytes) => format!("\\x{}", crate::value::hex(bytes)),
        Value::Temporal(typed) | Value::Other(typed) => typed.text.clone(),
        Value::Array(items) => crate::present::array_literal(items),
        Value::Map(_) | Value::Document(_) => crate::present::json_text(value),
    })
}

pub fn literal(value: &Value) -> String {
    SqlDialect::Postgres.literal(value)
}

/// A cell's staged value: a value, or the column's default.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum Staged {
    /// This value.
    Value(Value),
    /// The column's default (`DEFAULT`).
    Default,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Updated {
    pub row: usize,
    pub original: Vec<Value>,
    pub cells: Vec<(usize, Staged)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Inserted {
    pub row: usize,
    pub cells: Vec<(usize, Staged)>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Deleted {
    pub row: usize,
    pub original: Vec<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Changes {
    pub deleted: Vec<Deleted>,
    pub inserted: Vec<Inserted>,
    pub updated: Vec<Updated>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.deleted.is_empty() && self.inserted.is_empty() && self.updated.is_empty()
    }

    pub fn rows(&self) -> usize {
        self.deleted.len() + self.inserted.len() + self.updated.len()
    }
}

/// What a planned statement does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    /// Deletes a row.
    Delete,
    /// Inserts a row.
    Insert,
    /// Updates a row.
    Update,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetch {
    pub sql: String,
    pub params: Vec<Param>,
}

/// One statement that saves one row's change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Planned {
    /// What it does.
    pub kind: ChangeKind,
    /// The row in the result.
    pub row: usize,
    /// The statement with its parameters as placeholders.
    pub sql: String,
    /// The parameters' values as text, `None` for `NULL`; never answered to agents.
    #[serde(skip)]
    pub params: Vec<Param>,
    /// The statement with its parameters written as literals, as the preview shows it.
    pub preview: String,
    /// The result columns its `RETURNING`, or its `fetch`, reads, in order.
    #[serde(skip)]
    pub returning: Vec<usize>,
    /// Whether it must change exactly one row: always, so that rows equal in every column of a
    /// key (a virtual key of all columns most of all) are never changed together; more rows than
    /// one roll the save back (`severalRows`).
    #[serde(skip)]
    pub one_row: bool,
    /// The `SELECT` that reads the saved row back after it ran, where the server has no `RETURNING`.
    #[serde(skip)]
    pub fetch: Option<Fetch>,
}

struct Builder {
    dialect: SqlDialect,
    sql: String,
    preview: String,
    params: Vec<Param>,
}

impl Builder {
    fn new(dialect: SqlDialect) -> Self {
        Self { dialect, sql: String::new(), preview: String::new(), params: Vec::new() }
    }

    fn text(&mut self, text: &str) {
        self.sql.push_str(text);
        self.preview.push_str(text);
    }

    fn pair(&mut self, sql: &str, preview: &str) {
        self.sql.push_str(sql);
        self.preview.push_str(preview);
    }

    fn slot(&mut self, value: &Value) -> (String, String) {
        let literal = self.dialect.literal(value);
        if self.dialect.placeholders() == Placeholders::Inline || self.dialect.inlined(value) {
            return (literal.clone(), literal);
        }
        self.params.push(self.dialect.param(value));
        (self.dialect.placeholder(self.params.len(), value), literal)
    }

    fn value(&mut self, value: &Value) {
        let (sql, preview) = self.slot(value);
        self.pair(&sql, &preview);
    }

    fn staged(&mut self, staged: &Staged, column: Option<&EditColumn>) {
        match staged {
            Staged::Value(value) => self.value(value),
            Staged::Default if self.dialect.default_keyword() => self.text("DEFAULT"),
            Staged::Default => {
                let expression = column
                    .and_then(|column| column.default.as_deref())
                    .filter(|default| *default != "identity")
                    .map_or_else(|| "NULL".to_owned(), |default| format!("({default})"));
                self.text(&expression);
            }
        }
    }
}

fn comparable_as_is(value: &Value) -> bool {
    matches!(
        value,
        Value::Bool(_)
            | Value::Integer(_)
            | Value::Decimal(_)
            | Value::Float(_)
            | Value::Text(_)
            | Value::Bytes(_)
            | Value::Temporal(_)
            | Value::Interval(_)
            | Value::Uuid(_)
    )
}

fn comparable(dialect: SqlDialect, value: &Value) -> bool {
    if matches!(value, Value::Float(_)) && !dialect.floats_compare() {
        return false;
    }
    matches!(value, Value::Null | Value::Json(_) | Value::Other(_)) || comparable_as_is(value)
}

fn compare(builder: &mut Builder, name: &str, value: &Value, exact: bool) {
    let dialect = builder.dialect;
    let column = dialect.quote(name);
    match value {
        Value::Null => builder.text(&format!("{column} IS NULL")),
        Value::Json(_) | Value::Other(_) => {
            builder.text(&format!("{} = ", dialect.text(&column)));
            builder.value(&Value::Text(param_text(value).unwrap_or_default()));
        }
        value if exact => {
            builder.text(&format!("{column} = "));
            builder.value(value);
        }
        value => {
            let (sql, preview) = builder.slot(value);
            builder.pair(&dialect.not_distinct(&column, &sql), &dialect.not_distinct(&column, &preview));
        }
    }
}

fn where_key(builder: &mut Builder, key: &RowKey, original: &[Value]) {
    builder.text(" WHERE ");
    let exact = key.kind != KeyKind::AllColumns;
    let mut first = true;
    for (index, name) in key.columns.iter().zip(&key.names) {
        let value = original.get(*index).unwrap_or(&Value::Null);
        if !comparable(builder.dialect, value) {
            continue;
        }
        if !first {
            builder.text(" AND ");
        }
        compare(builder, name, value, exact);
        first = false;
    }
    if first {
        builder.text("FALSE");
    }
}

fn read_back(editing: &Editing) -> Vec<(usize, String)> {
    let mut read: Vec<(usize, String)> = editing
        .columns
        .iter()
        .filter(|column| column.sourced)
        .map(|column| (column.index, column.name.clone()))
        .collect();
    if let Some(key) = editing.key.as_ref().filter(|key| key.kind == KeyKind::RowId) {
        read.extend(key.columns.iter().zip(&key.names).map(|(index, name)| (*index, name.clone())));
    }
    read
}

fn returning(builder: &mut Builder, editing: &Editing) -> Vec<usize> {
    if !editing.dialect.returning() {
        return Vec::new();
    }
    let read = read_back(editing);
    if read.is_empty() {
        return Vec::new();
    }
    let names: Vec<String> = read.iter().map(|(_, name)| editing.dialect.quote(name)).collect();
    builder.text(&format!(" RETURNING {}", names.join(", ")));
    read.into_iter().map(|(index, _)| index).collect()
}

enum KeyPart {
    Value(Value),
    Expression(&'static str),
}

fn fetch(editing: &Editing, key: &RowKey, parts: Option<Vec<KeyPart>>) -> (Option<Fetch>, Vec<usize>) {
    let (Some(table), Some(parts)) = (&editing.table, parts) else { return (None, Vec::new()) };
    let read = read_back(editing);
    if editing.dialect.returning() || read.is_empty() || key.kind == KeyKind::AllColumns {
        return (None, Vec::new());
    }
    let dialect = editing.dialect;
    let mut builder = Builder::new(dialect);
    let names: Vec<String> = read.iter().map(|(_, name)| dialect.quote(name)).collect();
    builder.text(&format!("SELECT {} FROM {table} WHERE ", names.join(", ")));
    for (at, (name, part)) in key.names.iter().zip(parts).enumerate() {
        if at > 0 {
            builder.text(" AND ");
        }
        match part {
            KeyPart::Value(Value::Null) => return (None, Vec::new()),
            KeyPart::Value(value) => {
                builder.text(&format!("{} = ", dialect.quote(name)));
                builder.value(&value);
            }
            KeyPart::Expression(expression) => builder.text(&format!("{} = {expression}", dialect.quote(name))),
        }
    }
    let fetched = Fetch { sql: builder.sql, params: builder.params };
    (Some(fetched), read.into_iter().map(|(index, _)| index).collect())
}

fn inserted_key(editing: &Editing, key: &RowKey, given: &[&(usize, Staged)]) -> Option<Vec<KeyPart>> {
    let mut generated = 0;
    key.columns
        .iter()
        .map(|index| match given.iter().find(|(column, _)| column == index) {
            Some((_, Staged::Value(value))) => Some(KeyPart::Value(value.clone())),
            _ => {
                let identity =
                    editing.columns.get(*index).is_some_and(|column| column.default.as_deref() == Some("identity"));
                generated += 1;
                let expression = editing.dialect.last_insert_id().filter(|_| identity && generated == 1)?;
                Some(KeyPart::Expression(expression))
            }
        })
        .collect()
}

fn updated_key(key: &RowKey, updated: &Updated) -> Option<Vec<KeyPart>> {
    key.columns
        .iter()
        .map(|index| match updated.cells.iter().find(|(column, _)| column == index) {
            Some((_, Staged::Value(value))) => Some(KeyPart::Value(value.clone())),
            Some((_, Staged::Default)) => None,
            None => Some(KeyPart::Value(updated.original.get(*index).cloned().unwrap_or(Value::Null))),
        })
        .collect()
}

pub fn plan(editing: &Editing, changes: &Changes) -> Vec<Planned> {
    if editing.documents {
        return document::plan(editing, changes);
    }
    let (Some(table), Some(key)) = (&editing.table, &editing.key) else { return Vec::new() };
    let dialect = editing.dialect;
    let one_row = true;
    let name_of = |index: usize| editing.columns.get(index).map(|column| column.name.clone()).unwrap_or_default();
    let mut planned = Vec::with_capacity(changes.rows());
    for deleted in &changes.deleted {
        let mut builder = Builder::new(dialect);
        builder.text(&format!("DELETE FROM {table}"));
        where_key(&mut builder, key, &deleted.original);
        planned.push(Planned {
            kind: ChangeKind::Delete,
            row: deleted.row,
            sql: builder.sql,
            params: builder.params,
            preview: builder.preview,
            returning: Vec::new(),
            one_row,
            fetch: None,
        });
    }
    for inserted in &changes.inserted {
        let mut builder = Builder::new(dialect);
        let given: Vec<&(usize, Staged)> = inserted
            .cells
            .iter()
            .filter(|(index, _)| editing.columns.get(*index).is_some_and(|column| column.editable))
            .filter(|(_, staged)| dialect.default_keyword() || *staged != Staged::Default)
            .collect();
        if given.is_empty() {
            builder.text(&dialect.insert_defaults(table));
        } else {
            let names: Vec<String> = given.iter().map(|(index, _)| dialect.quote(&name_of(*index))).collect();
            builder.text(&format!("INSERT INTO {table} ({}) VALUES (", names.join(", ")));
            for (at, (index, staged)) in given.iter().enumerate() {
                if at > 0 {
                    builder.text(", ");
                }
                builder.staged(staged, editing.columns.get(*index));
            }
            builder.text(")");
        }
        let mut read = returning(&mut builder, editing);
        let (fetched, fetched_columns) = fetch(editing, key, inserted_key(editing, key, &given));
        if fetched.is_some() {
            read = fetched_columns;
        }
        planned.push(Planned {
            kind: ChangeKind::Insert,
            row: inserted.row,
            sql: builder.sql,
            params: builder.params,
            preview: builder.preview,
            returning: read,
            one_row: true,
            fetch: fetched,
        });
    }
    for updated in &changes.updated {
        let cells: Vec<&(usize, Staged)> = updated
            .cells
            .iter()
            .filter(|(index, _)| editing.columns.get(*index).is_some_and(|column| column.editable))
            .collect();
        if cells.is_empty() {
            continue;
        }
        let mut builder = Builder::new(dialect);
        builder.text(&format!("UPDATE {table} SET "));
        for (at, (index, staged)) in cells.iter().enumerate() {
            if at > 0 {
                builder.text(", ");
            }
            builder.text(&format!("{} = ", dialect.quote(&name_of(*index))));
            builder.staged(staged, editing.columns.get(*index));
        }
        where_key(&mut builder, key, &updated.original);
        for (index, _) in &cells {
            let value = updated.original.get(*index).unwrap_or(&Value::Null);
            if key.columns.contains(index) || !comparable(dialect, value) {
                continue;
            }
            builder.text(" AND ");
            compare(&mut builder, &name_of(*index), value, false);
        }
        let mut read = returning(&mut builder, editing);
        let (fetched, fetched_columns) = fetch(editing, key, updated_key(key, updated));
        if fetched.is_some() {
            read = fetched_columns;
        }
        planned.push(Planned {
            kind: ChangeKind::Update,
            row: updated.row,
            sql: builder.sql,
            params: builder.params,
            preview: builder.preview,
            returning: read,
            one_row,
            fetch: fetched,
        });
    }
    planned
}

pub fn preview(planned: &[Planned]) -> String {
    planned.iter().map(|statement| format!("{};", statement.preview)).collect::<Vec<_>>().join("\n")
}

/// What SQL to generate from rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Generate {
    /// `SELECT` each row by its key.
    Select,
    /// `INSERT` each row.
    Insert,
    /// `UPDATE` each row's columns by its key.
    Update,
    /// `DELETE` each row by its key.
    Delete,
}

pub fn generate(editing: &Editing, kind: Generate, rows: &[Vec<Value>]) -> Option<String> {
    if editing.documents {
        return document::generate(editing, kind, rows);
    }
    let table = editing.table.as_ref()?;
    let dialect = editing.dialect;
    let sourced: Vec<&EditColumn> = editing.columns.iter().filter(|column| column.sourced).collect();
    let all = RowKey {
        kind: KeyKind::AllColumns,
        columns: sourced.iter().map(|column| column.index).collect(),
        names: sourced.iter().map(|column| column.name.clone()).collect(),
    };
    let key = editing.key.as_ref().unwrap_or(&all);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut builder = Builder::new(dialect);
        match kind {
            Generate::Select => {
                let names: Vec<String> = editing.columns.iter().map(|column| dialect.quote(&column.name)).collect();
                builder.text(&format!("SELECT {} FROM {table}", names.join(", ")));
                where_key(&mut builder, key, row);
            }
            Generate::Insert => {
                let given: Vec<&&EditColumn> = sourced.iter().filter(|column| column.editable).collect();
                let names: Vec<String> = given.iter().map(|column| dialect.quote(&column.name)).collect();
                builder.text(&format!("INSERT INTO {table} ({}) VALUES (", names.join(", ")));
                for (at, column) in given.iter().enumerate() {
                    if at > 0 {
                        builder.text(", ");
                    }
                    builder.value(row.get(column.index).unwrap_or(&Value::Null));
                }
                builder.text(")");
            }
            Generate::Update => {
                let set: Vec<&&EditColumn> =
                    sourced.iter().filter(|column| column.editable && !key.columns.contains(&column.index)).collect();
                builder.text(&format!("UPDATE {table} SET "));
                for (at, column) in set.iter().enumerate() {
                    if at > 0 {
                        builder.text(", ");
                    }
                    builder.text(&format!("{} = ", dialect.quote(&column.name)));
                    builder.value(row.get(column.index).unwrap_or(&Value::Null));
                }
                where_key(&mut builder, key, row);
            }
            Generate::Delete => {
                builder.text(&format!("DELETE FROM {table}"));
                where_key(&mut builder, key, row);
            }
        }
        out.push(format!("{};", builder.preview));
    }
    Some(out.join("\n"))
}

pub mod document;

#[cfg(test)]
mod tests;

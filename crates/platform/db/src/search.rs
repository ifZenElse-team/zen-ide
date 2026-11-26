use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::SqlDialect;
use crate::schema::ObjectKind;
use crate::value::Value;

/// What a column holds, as far as finding text in it goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ColumnClass {
    /// Character types, enums and UUIDs; untyped SQLite columns.
    Text,
    /// Integers, decimals and floating-point numbers.
    Number,
    /// Dates, times, timestamps and intervals.
    Temporal,
    /// Everything else: JSON, XML, binary, arrays, booleans, geometry.
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogColumn {
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogTable {
    pub name: String,
    pub kind: ObjectKind,
    pub columns: Vec<CatalogColumn>,
}

const CLICKHOUSE_EXTERNAL: &[&str] = &[
    "kafka",
    "rabbitmq",
    "nats",
    "filelog",
    "url",
    "s3",
    "s3queue",
    "azureblobstorage",
    "azurequeue",
    "hdfs",
    "mysql",
    "postgresql",
    "mongodb",
    "redis",
    "odbc",
    "jdbc",
    "sqlite",
    "hive",
    "deltalake",
    "iceberg",
    "hudi",
    "externaldistributed",
    "executable",
    "executablepool",
    "generaterandom",
    "dictionary",
    "null",
    "set",
    "keepermap",
];

fn unwrap_clickhouse(name: &str) -> &str {
    let mut name = name;
    loop {
        let inner = ["nullable(", "lowcardinality("]
            .iter()
            .find_map(|wrapper| name.strip_prefix(wrapper).and_then(|rest| rest.strip_suffix(')')));
        match inner {
            Some(inner) => name = inner.trim(),
            None => return name,
        }
    }
}

pub fn classify(type_name: &str) -> ColumnClass {
    let lowered = type_name.trim().to_ascii_lowercase();
    let name = unwrap_clickhouse(&lowered);
    if name.is_empty() {
        return ColumnClass::Text;
    }
    if name.ends_with(']')
        || name.starts_with('_')
        || ["array(", "map(", "tuple(", "nested("].iter().any(|p| name.starts_with(p))
    {
        return ColumnClass::Other;
    }
    let base = name.split('(').next().unwrap_or(name).trim();
    let base = base.strip_suffix(" unsigned").unwrap_or(base).trim();
    let text = base.contains("char")
        || base.ends_with("text")
        || base.ends_with("clob")
        || matches!(base, "name" | "string" | "fixedstring" | "enum" | "enum8" | "enum16" | "set" | "uuid");
    if text {
        return ColumnClass::Text;
    }
    if ["date", "time", "interval", "year"].iter().any(|prefix| base.starts_with(prefix)) {
        return ColumnClass::Temporal;
    }
    let number = matches!(
        base,
        "smallint"
            | "integer"
            | "int"
            | "bigint"
            | "tinyint"
            | "mediumint"
            | "serial"
            | "bigserial"
            | "smallserial"
            | "numeric"
            | "decimal"
            | "real"
            | "double"
            | "double precision"
            | "float"
            | "money"
            | "number"
    ) || ["int", "uint", "float", "decimal"]
        .iter()
        .any(|prefix| base.strip_prefix(prefix).is_some_and(|rest| rest.chars().all(|c| c.is_ascii_digit())));
    if number { ColumnClass::Number } else { ColumnClass::Other }
}

pub fn catalog_query(dialect: SqlDialect, schema: &str) -> String {
    let name = dialect.string(schema);
    match dialect {
        SqlDialect::Postgres => format!(
            "SELECT c.relname::text, a.attname::text, pg_catalog.format_type(a.atttypid, a.atttypmod), c.relkind::text \
             FROM pg_catalog.pg_class c \
             JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace \
             JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped \
             WHERE n.nspname = {name} AND c.relkind IN ('r', 'p', 'v', 'm') AND NOT c.relispartition \
             ORDER BY c.relname, a.attnum"
        ),
        SqlDialect::MySql | SqlDialect::MariaDb => format!(
            "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, t.TABLE_TYPE \
             FROM information_schema.COLUMNS c \
             JOIN information_schema.TABLES t ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME \
             WHERE c.TABLE_SCHEMA = {name} \
             ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION"
        ),
        SqlDialect::Sqlite => format!(
            "SELECT m.name, p.name, p.type, m.type \
             FROM {}.sqlite_master m JOIN pragma_table_info(m.name, {name}) p \
             WHERE m.type IN ('table', 'view') AND m.name NOT LIKE 'sqlite!_%' ESCAPE '!' \
             ORDER BY m.name, p.cid",
            dialect.quote(schema)
        ),
        SqlDialect::ClickHouse => format!(
            "SELECT c.table, c.name, c.type, t.engine \
             FROM system.columns AS c \
             INNER JOIN system.tables AS t ON t.database = c.database AND t.name = c.table \
             WHERE c.database = {name} \
             ORDER BY c.table, c.position"
        ),
        SqlDialect::SqlServer => format!(
            "SELECT o.name, c.name, ty.name, RTRIM(o.type) \
             FROM sys.objects o \
             JOIN sys.schemas s ON s.schema_id = o.schema_id \
             JOIN sys.columns c ON c.object_id = o.object_id \
             JOIN sys.types ty ON ty.user_type_id = c.user_type_id \
             WHERE s.name = {name} AND o.type IN ('U', 'V') \
             ORDER BY o.name, c.column_id"
        ),
        SqlDialect::Oracle => format!(
            "SELECT c.table_name, c.column_name, c.data_type, o.object_type \
             FROM all_tab_columns c \
             JOIN all_objects o ON o.owner = c.owner AND o.object_name = c.table_name \
             AND o.object_type IN ('TABLE', 'VIEW', 'MATERIALIZED VIEW') \
             WHERE c.owner = {name} \
             ORDER BY c.table_name, c.column_id"
        ),
    }
}

fn kind_of(dialect: SqlDialect, raw: &str) -> Option<ObjectKind> {
    match dialect {
        SqlDialect::Postgres => match raw {
            "r" | "p" => Some(ObjectKind::Table),
            "v" => Some(ObjectKind::View),
            "m" => Some(ObjectKind::MaterializedView),
            _ => None,
        },
        SqlDialect::MySql | SqlDialect::MariaDb => match raw.to_ascii_uppercase().as_str() {
            "BASE TABLE" => Some(ObjectKind::Table),
            "VIEW" | "SYSTEM VIEW" => Some(ObjectKind::View),
            _ => None,
        },
        SqlDialect::Sqlite => match raw {
            "table" => Some(ObjectKind::Table),
            "view" => Some(ObjectKind::View),
            _ => None,
        },
        SqlDialect::Oracle => match raw.trim() {
            "TABLE" => Some(ObjectKind::Table),
            "VIEW" => Some(ObjectKind::View),
            "MATERIALIZED VIEW" => Some(ObjectKind::MaterializedView),
            _ => None,
        },
        SqlDialect::SqlServer => match raw.trim() {
            "U" => Some(ObjectKind::Table),
            "V" => Some(ObjectKind::View),
            _ => None,
        },
        SqlDialect::ClickHouse => {
            let engine = raw.to_ascii_lowercase();
            match engine.as_str() {
                "view" | "liveview" | "windowview" => Some(ObjectKind::View),
                "materializedview" => Some(ObjectKind::MaterializedView),
                engine if CLICKHOUSE_EXTERNAL.contains(&engine) => None,
                _ => Some(ObjectKind::Table),
            }
        }
    }
}

pub fn text_of(value: &Value) -> Option<String> {
    match value {
        Value::Text(text) | Value::Decimal(text) | Value::Json(text) | Value::Uuid(text) | Value::Interval(text) => {
            Some(text.clone())
        }
        Value::Other(typed) | Value::Temporal(typed) => Some(typed.text.clone()),
        Value::Bytes(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        Value::Integer(number) => Some(number.to_string()),
        _ => None,
    }
}

pub fn catalog_tables(dialect: SqlDialect, rows: &[Vec<Value>]) -> Vec<CatalogTable> {
    let mut tables: Vec<CatalogTable> = Vec::new();
    for row in rows {
        let [table, column, type_name, kind, ..] = row.as_slice() else { continue };
        let (Some(table), Some(column)) = (text_of(table), text_of(column)) else { continue };
        let Some(kind) = text_of(kind).and_then(|kind| kind_of(dialect, kind.trim())) else { continue };
        let column = CatalogColumn { name: column, type_name: text_of(type_name).unwrap_or_default() };
        match tables.last_mut() {
            Some(last) if last.name == table => last.columns.push(column),
            _ => tables.push(CatalogTable { name: table, kind, columns: vec![column] }),
        }
    }
    tables
}

pub fn matches(dialect: SqlDialect, column: &str, text: &str, case_sensitive: bool) -> String {
    let (pattern, escape) = dialect.like_pattern(text);
    match dialect {
        SqlDialect::Postgres => {
            let operator = if case_sensitive { "LIKE" } else { "ILIKE" };
            format!("{} {operator} {pattern}{escape}", dialect.text(column))
        }
        SqlDialect::MySql | SqlDialect::MariaDb => {
            let cast = format!("CAST({column} AS CHAR CHARACTER SET utf8mb4)");
            if case_sensitive {
                format!("{cast} COLLATE utf8mb4_bin LIKE {pattern}{escape}")
            } else {
                format!("LOWER({cast}) LIKE LOWER({pattern}){escape}")
            }
        }
        SqlDialect::Sqlite if case_sensitive => {
            format!("instr({}, {}) > 0", dialect.text(column), dialect.string(text))
        }
        SqlDialect::Sqlite => format!("{} LIKE {pattern}{escape}", dialect.text(column)),
        SqlDialect::ClickHouse => {
            let operator = if case_sensitive { "LIKE" } else { "ILIKE" };
            format!("{} {operator} {pattern}", dialect.text(column))
        }
        SqlDialect::SqlServer if case_sensitive => {
            format!("{} COLLATE Latin1_General_CS_AS LIKE {pattern}{escape}", dialect.text(column))
        }
        SqlDialect::SqlServer => format!("LOWER({}) LIKE LOWER({pattern}){escape}", dialect.text(column)),
        SqlDialect::Oracle if case_sensitive => format!("{} LIKE {pattern}{escape}", dialect.text(column)),
        SqlDialect::Oracle => format!("LOWER({}) LIKE LOWER({pattern}){escape}", dialect.text(column)),
    }
}

pub fn data_query(
    dialect: SqlDialect,
    (schema, table): (&str, &str),
    columns: &[String],
    text: &str,
    case_sensitive: bool,
    limit: usize,
) -> String {
    let conditions: Vec<String> =
        columns.iter().map(|column| matches(dialect, &dialect.quote(column), text, case_sensitive)).collect();
    let flags: Vec<String> =
        conditions.iter().map(|condition| format!("CASE WHEN {condition} THEN 1 ELSE 0 END")).collect();
    if dialect == SqlDialect::SqlServer {
        return format!(
            "SELECT TOP {limit} {} FROM {} WHERE {}",
            flags.join(", "),
            dialect.qualified(schema, table),
            conditions.join(" OR ")
        );
    }
    if dialect == SqlDialect::Oracle {
        return format!(
            "SELECT {} FROM {} WHERE {} FETCH FIRST {limit} ROWS ONLY",
            flags.join(", "),
            dialect.qualified(schema, table),
            conditions.join(" OR ")
        );
    }
    format!(
        "SELECT {} FROM {} WHERE {} LIMIT {limit}",
        flags.join(", "),
        dialect.qualified(schema, table),
        conditions.join(" OR ")
    )
}

pub fn filter_condition(dialect: SqlDialect, columns: &[String], text: &str, case_sensitive: bool) -> String {
    columns
        .iter()
        .map(|column| matches(dialect, &dialect.quote(column), text, case_sensitive))
        .collect::<Vec<_>>()
        .join(" OR ")
}

pub fn name_matches(name: &str, text: &str, case_sensitive: bool) -> bool {
    if case_sensitive { name.contains(text) } else { name.to_lowercase().contains(&text.to_lowercase()) }
}

#[cfg(test)]
mod tests;

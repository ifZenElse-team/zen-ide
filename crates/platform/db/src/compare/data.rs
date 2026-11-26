use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Change;
use crate::dialect::SqlDialect;
use crate::value::Value;

/// A row that differs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RowDiff {
    /// Only in the source (added), only in the target (removed), or in both with other values (changed).
    pub change: Change,
    /// The key's values.
    pub key: Vec<Value>,
    /// The compared columns whose values differ, for a changed row.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    /// The source's values of the compared columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Vec<Value>>,
    /// The target's values of the compared columns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Vec<Value>>,
}

/// How many rows compared which way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataCounts {
    /// Rows read from the source.
    pub source_rows: u64,
    /// Rows read from the target.
    pub target_rows: u64,
    /// Rows equal on both sides.
    pub same: u64,
    /// Rows with the same key and other values.
    pub changed: u64,
    /// Rows only the source has.
    pub added: u64,
    /// Rows only the target has.
    pub removed: u64,
}

fn list(dialect: SqlDialect, names: &[String]) -> String {
    names.iter().map(|name| dialect.quote(name)).collect::<Vec<_>>().join(", ")
}

fn tuple(dialect: SqlDialect, names: &[String]) -> String {
    match names {
        [one] => dialect.quote(one),
        many => format!("({})", list(dialect, many)),
    }
}

fn literals(dialect: SqlDialect, values: &[Value]) -> String {
    match values {
        [one] => dialect.literal(one),
        many => format!("({})", many.iter().map(|value| dialect.literal(value)).collect::<Vec<_>>().join(", ")),
    }
}

pub fn page_query(
    dialect: SqlDialect,
    table: &str,
    (key, columns): (&[String], &[String]),
    after: Option<&[Value]>,
    limit: usize,
) -> String {
    let mut selected: Vec<String> = key.to_vec();
    selected.extend(columns.iter().filter(|column| !key.contains(column)).cloned());
    let mut query = format!("SELECT {} FROM {table}", list(dialect, &selected));
    if let Some(after) = after {
        query.push_str(&format!(" WHERE {} > {}", tuple(dialect, key), literals(dialect, after)));
    }
    let order = key.iter().map(|name| dialect.quote(name)).collect::<Vec<_>>().join(", ");
    query.push_str(&format!(" ORDER BY {order}{}", dialect.limit(limit)));
    query
}

pub fn lookup_query(
    dialect: SqlDialect,
    table: &str,
    (key, columns): (&[String], &[String]),
    keys: &[Vec<Value>],
) -> String {
    let mut selected: Vec<String> = key.to_vec();
    selected.extend(columns.iter().filter(|column| !key.contains(column)).cloned());
    let condition = match key {
        [one] => format!(
            "{} IN ({})",
            dialect.quote(one),
            keys.iter()
                .filter_map(|values| values.first())
                .map(|value| dialect.literal(value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        many => keys
            .iter()
            .map(|values| {
                let parts: Vec<String> = many
                    .iter()
                    .zip(values)
                    .map(|(name, value)| format!("{} = {}", dialect.quote(name), dialect.literal(value)))
                    .collect();
                format!("({})", parts.join(" AND "))
            })
            .collect::<Vec<_>>()
            .join(" OR "),
    };
    format!("SELECT {} FROM {table} WHERE {condition}", list(dialect, &selected))
}

fn text(value: &Value) -> Option<String> {
    crate::edit::param_text(value)
}

pub fn same_value(source: &Value, target: &Value) -> bool {
    if source == target {
        return true;
    }
    match (source, target) {
        (Value::Null, _) | (_, Value::Null) => false,
        (Value::Bool(flag), other) | (other, Value::Bool(flag)) => match other {
            Value::Integer(number) => (*number != 0) == *flag,
            other => text(other).is_some_and(|text| matches!(text.as_str(), "t" | "true" | "1") == *flag),
        },
        _ => {
            let (left, right) = (text(source), text(target));
            if left == right {
                return true;
            }
            match (left.and_then(|text| text.parse::<f64>().ok()), right.and_then(|text| text.parse::<f64>().ok())) {
                (Some(left), Some(right)) => left == right,
                _ => false,
            }
        }
    }
}

pub fn key_text(values: &[Value]) -> String {
    values
        .iter()
        .map(|value| match value {
            Value::Null => "\u{1}".to_owned(),
            Value::Float(number) if number.fract() == 0.0 && number.abs() < 1e15 => format!("{number:.0}"),
            Value::Decimal(text) => text
                .parse::<f64>()
                .ok()
                .filter(|number| number.fract() == 0.0 && number.abs() < 1e15)
                .map_or_else(|| text.clone(), |number| format!("{number:.0}")),
            other => text(other).unwrap_or_default(),
        })
        .collect::<Vec<_>>()
        .join("\u{0}")
}

pub fn differing(columns: &[String], source: &[Value], target: &[Value]) -> Vec<String> {
    columns
        .iter()
        .enumerate()
        .filter(|(index, _)| match (source.get(*index), target.get(*index)) {
            (Some(left), Some(right)) => !same_value(left, right),
            _ => true,
        })
        .map(|(_, name)| name.clone())
        .collect()
}

/// A data sync script.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataScript {
    /// The statements, one a line.
    pub text: String,
    /// How many statements it has.
    pub statements: usize,
    /// How many rows only the target has that were not deleted because deletes were not asked for.
    pub not_deleted: usize,
}

pub fn data_script(
    dialect: SqlDialect,
    table: &str,
    (key, columns): (&[String], &[String]),
    diffs: &[RowDiff],
    delete: bool,
) -> DataScript {
    let mut script = DataScript::default();
    let position = |name: &String| columns.iter().position(|column| column == name);
    let condition = |values: &[Value]| {
        key.iter()
            .zip(values)
            .map(|(name, value)| format!("{} = {}", dialect.quote(name), dialect.literal(value)))
            .collect::<Vec<_>>()
            .join(" AND ")
    };
    for diff in diffs {
        let line = match (diff.change, &diff.source) {
            (Change::Added, Some(row)) => format!(
                "INSERT INTO {table} ({}) VALUES ({});",
                list(dialect, columns),
                row.iter().map(|value| dialect.literal(value)).collect::<Vec<_>>().join(", ")
            ),
            (Change::Changed, Some(row)) => {
                let set: Vec<String> = diff
                    .columns
                    .iter()
                    .filter_map(|name| {
                        let value = row.get(position(name)?)?;
                        Some(format!("{} = {}", dialect.quote(name), dialect.literal(value)))
                    })
                    .collect();
                if set.is_empty() {
                    continue;
                }
                format!("UPDATE {table} SET {} WHERE {};", set.join(", "), condition(&diff.key))
            }
            (Change::Removed, _) if delete => format!("DELETE FROM {table} WHERE {};", condition(&diff.key)),
            (Change::Removed, _) => {
                script.not_deleted += 1;
                continue;
            }
            _ => continue,
        };
        script.text.push_str(&line);
        script.text.push('\n');
        script.statements += 1;
    }
    script
}

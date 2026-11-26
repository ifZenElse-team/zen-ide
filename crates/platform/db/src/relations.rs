use crate::dialect::SqlDialect;
use crate::schema::{Catalog, ConstraintKind, Object, ObjectKind, Schema, Snapshot};
use crate::value::{Column as ResultColumn, Value};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub name: String,
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub target_schema: String,
    pub target_table: String,
    pub target_columns: Vec<String>,
}

pub fn key_columns(object: &Object) -> Vec<String> {
    object
        .constraints
        .iter()
        .find(|constraint| constraint.kind == ConstraintKind::PrimaryKey)
        .or_else(|| object.constraints.iter().find(|constraint| constraint.kind == ConstraintKind::Unique))
        .map(|constraint| constraint.columns.clone())
        .unwrap_or_default()
}

pub fn find_table<'a>(
    snapshot: &'a Snapshot,
    catalog: &str,
    schema: &str,
    table: &str,
) -> Option<(&'a Catalog, &'a Schema, &'a Object)> {
    let in_catalog = |known: &'a Catalog| {
        let found = known.schemas.iter().find(|candidate| candidate.name == schema)?;
        let object = found.objects.iter().find(|object| object.name == table && has_rows(object.kind))?;
        Some((known, found, object))
    };
    snapshot
        .catalog(catalog)
        .and_then(in_catalog)
        .or_else(|| snapshot.catalogs.iter().filter(|known| known.name != catalog).find_map(in_catalog))
}

fn has_rows(kind: ObjectKind) -> bool {
    matches!(kind, ObjectKind::Table | ObjectKind::ForeignTable | ObjectKind::View | ObjectKind::MaterializedView)
}

pub fn outgoing(snapshot: &Snapshot, catalog: &str, schema: &str, object: &Object) -> Vec<Link> {
    object
        .constraints
        .iter()
        .filter(|constraint| constraint.kind == ConstraintKind::ForeignKey && !constraint.columns.is_empty())
        .filter_map(|constraint| {
            let reference = constraint.references.as_ref()?;
            let target_columns = if reference.columns.is_empty() {
                let (_, _, target) = find_table(snapshot, catalog, &reference.schema, &reference.table)?;
                key_columns(target)
            } else {
                reference.columns.clone()
            };
            (target_columns.len() == constraint.columns.len()).then(|| Link {
                name: constraint.name.clone(),
                schema: schema.to_owned(),
                table: object.name.clone(),
                columns: constraint.columns.clone(),
                target_schema: reference.schema.clone(),
                target_table: reference.table.clone(),
                target_columns,
            })
        })
        .collect()
}

pub fn incoming(snapshot: &Snapshot, catalog: &str, schema: &str, table: &str) -> Vec<Link> {
    let Some(home) = snapshot.catalog(catalog) else { return Vec::new() };
    let mut found = Vec::new();
    for other in &home.schemas {
        for object in other.objects.iter().filter(|object| object.kind == ObjectKind::Table) {
            for link in outgoing(snapshot, catalog, &other.name, object) {
                if link.target_schema == schema && link.target_table == table {
                    found.push(link);
                }
            }
        }
    }
    found
}

pub fn table_columns(columns: &[ResultColumn], object: &Object, table: u32, by_name: bool) -> Vec<Option<String>> {
    columns
        .iter()
        .map(|column| {
            let sourced = column.source.filter(|source| source.table == table).and_then(|source| {
                if source.position > 0 {
                    object.columns.iter().find(|known| known.position == i32::from(source.position))
                } else {
                    let origin = column.origin.as_deref()?;
                    object.columns.iter().find(|known| known.name == origin)
                }
            });
            sourced
                .or_else(|| by_name.then(|| object.columns.iter().find(|known| known.name == column.name)).flatten())
                .map(|known| known.name.clone())
        })
        .collect()
}

pub fn key_condition(pairs: &[(&str, &Value)], dialect: SqlDialect) -> Option<String> {
    if pairs.is_empty() || pairs.iter().any(|(_, value)| matches!(value, Value::Null)) {
        return None;
    }
    let parts: Vec<String> =
        pairs.iter().map(|(name, value)| format!("{} = {}", dialect.quote(name), dialect.literal(value))).collect();
    Some(parts.join(" AND "))
}

pub fn search_condition(columns: &[String], text: &str, dialect: SqlDialect) -> Option<String> {
    let text = text.trim();
    if text.is_empty() || columns.is_empty() {
        return None;
    }
    let parts: Vec<String> = columns.iter().map(|column| dialect.contains(&dialect.quote(column), text)).collect();
    Some(parts.join(" OR "))
}

pub fn lookup_statement(
    dialect: SqlDialect,
    table: &str,
    condition: Option<&str>,
    order: Option<&str>,
    limit: usize,
) -> String {
    let mut sql = format!("SELECT * FROM {table}");
    if let Some(condition) = condition {
        sql.push_str(" WHERE (");
        sql.push_str(condition);
        sql.push(')');
    }
    if let Some(order) = order {
        sql.push_str(" ORDER BY ");
        sql.push_str(order);
    }
    sql.push_str(&dialect.limit(limit));
    sql
}

#[cfg(test)]
mod tests;

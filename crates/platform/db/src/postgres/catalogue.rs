use std::collections::{BTreeMap, HashMap};

use tokio_postgres::{Client, SimpleQueryMessage};

use super::error;
use crate::driver::DbError;
use crate::schema::{
    Catalog, Column, Constraint, ConstraintKind, IntrospectRequest, Introspection, Level, Object, ObjectKind, Schema,
    Snapshot,
};

type Row = Vec<Option<String>>;

const META_TABLES: usize = 500;

pub(super) const QUESTDB_SCHEMA: &str = "public";

pub(super) const SCHEMAS: &str = "SELECT schema_name FROM information_schema.schemata ORDER BY 1";

pub(super) const META_TABLE_LIST: &str = "SELECT table_name FROM tables() ORDER BY 1";

pub(super) fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

fn list(names: &[String]) -> String {
    names.iter().map(|name| literal(name)).collect::<Vec<_>>().join(", ")
}

pub(super) fn system_schema(name: &str) -> bool {
    name.starts_with("pg_")
        || name.starts_with("mz_")
        || matches!(name, "information_schema" | "crdb_internal" | "rw_catalog" | "sys")
}

pub(super) fn tables_sql(schemas: &[String]) -> String {
    format!(
        "SELECT table_schema, table_name, table_type FROM information_schema.tables WHERE table_schema IN ({}) ORDER BY 1, 2",
        list(schemas)
    )
}

pub(super) fn columns_sql(schemas: &[String]) -> String {
    format!(
        "SELECT table_schema, table_name, ordinal_position, column_name, data_type, is_nullable, column_default \
         FROM information_schema.columns WHERE table_schema IN ({}) ORDER BY 1, 2, 3",
        list(schemas)
    )
}

pub(super) fn keys_sql(schemas: &[String]) -> String {
    format!(
        "SELECT tc.table_schema, tc.table_name, tc.constraint_name, tc.constraint_type, k.column_name \
         FROM information_schema.table_constraints tc JOIN information_schema.key_column_usage k \
         ON k.constraint_schema = tc.constraint_schema AND k.constraint_name = tc.constraint_name AND k.table_name = tc.table_name \
         WHERE tc.table_schema IN ({}) AND tc.constraint_type IN ('PRIMARY KEY', 'UNIQUE') ORDER BY 1, 2, 3, k.ordinal_position",
        list(schemas)
    )
}

pub(super) fn meta_columns_sql(table: &str) -> String {
    format!("SELECT * FROM table_columns({})", literal(table))
}

async fn simple(client: &Client, sql: &str) -> Result<Vec<Row>, DbError> {
    let messages = client.simple_query(sql).await.map_err(|failed| error(&failed))?;
    Ok(messages
        .iter()
        .filter_map(|message| match message {
            SimpleQueryMessage::Row(row) => {
                Some((0..row.len()).map(|index| row.get(index).map(str::to_owned)).collect())
            }
            _ => None,
        })
        .collect())
}

fn cell(row: &[Option<String>], index: usize) -> Option<String> {
    row.get(index).cloned().flatten()
}

fn text(row: &[Option<String>], index: usize) -> String {
    cell(row, index).unwrap_or_default()
}

pub(super) fn assemble(
    read: &[String],
    tables: &[Row],
    columns: &[Row],
    keys: &[Row],
) -> BTreeMap<String, Vec<Object>> {
    let mut by_table: HashMap<(String, String), Vec<Column>> = HashMap::new();
    for row in columns {
        by_table.entry((text(row, 0), text(row, 1))).or_default().push(Column {
            name: text(row, 3),
            position: cell(row, 2).and_then(|position| position.parse().ok()).unwrap_or_default(),
            type_name: text(row, 4),
            nullable: !text(row, 5).eq_ignore_ascii_case("no"),
            default: cell(row, 6),
            identity: None,
            generated: None,
            collation: None,
            comment: None,
        });
    }
    let mut constraints: HashMap<(String, String), Vec<Constraint>> = HashMap::new();
    for row in keys {
        let kind = match text(row, 3).as_str() {
            "PRIMARY KEY" => ConstraintKind::PrimaryKey,
            "UNIQUE" => ConstraintKind::Unique,
            _ => continue,
        };
        let found = constraints.entry((text(row, 0), text(row, 1))).or_default();
        let name = text(row, 2);
        let constraint = match found.iter_mut().find(|known| known.name == name) {
            Some(known) => known,
            None => {
                found.push(Constraint {
                    name,
                    kind,
                    columns: Vec::new(),
                    definition: String::new(),
                    references: None,
                    index: None,
                    comment: None,
                });
                found.last_mut().expect("just pushed")
            }
        };
        constraint.columns.push(text(row, 4));
    }
    for list in constraints.values_mut() {
        for constraint in list {
            let words = if constraint.kind == ConstraintKind::PrimaryKey { "PRIMARY KEY" } else { "UNIQUE" };
            constraint.definition = format!("{words} ({})", constraint.columns.join(", "));
        }
    }
    let mut objects: BTreeMap<String, Vec<Object>> = read.iter().map(|name| (name.clone(), Vec::new())).collect();
    for row in tables {
        let (schema, name) = (text(row, 0), text(row, 1));
        let kind = match text(row, 2).to_ascii_uppercase().as_str() {
            "VIEW" | "SYSTEM VIEW" => ObjectKind::View,
            "MATERIALIZED VIEW" => ObjectKind::MaterializedView,
            _ => ObjectKind::Table,
        };
        let key = (schema.clone(), name.clone());
        let mut object = Object::new(kind, name);
        object.columns = by_table.remove(&key).unwrap_or_default();
        object.columns.sort_by_key(|column| column.position);
        if kind == ObjectKind::Table {
            object.constraints = constraints.remove(&key).unwrap_or_default();
        }
        object.system = system_schema(&schema);
        object.detailed = true;
        objects.entry(schema).or_default().push(object);
    }
    for list in objects.values_mut() {
        list.sort_by_key(|object| (object.kind, object.key()));
    }
    objects
}

fn candidates(
    names: &BTreeMap<String, bool>,
    request: &IntrospectRequest,
    earlier: &dyn Fn(&str) -> Option<Level>,
) -> Vec<String> {
    let wanted: Vec<&String> = request.objects.iter().map(|(schema, _)| schema).collect();
    names
        .iter()
        .filter(|(name, system)| {
            if !request.schemas.is_empty() || !wanted.is_empty() {
                request.schemas.contains(name) || wanted.contains(name)
            } else {
                !**system || request.system || earlier(name).is_some_and(|level| level != Level::NotRead)
            }
        })
        .map(|(name, _)| name.clone())
        .collect()
}

fn catalog(
    current: &str,
    names: &BTreeMap<String, bool>,
    mut read: BTreeMap<String, Vec<Object>>,
    previous: Option<&Snapshot>,
) -> Catalog {
    let before = previous.and_then(|snapshot| snapshot.catalog(current));
    Catalog {
        name: current.to_owned(),
        current: true,
        schemas_read: true,
        schemas: names
            .iter()
            .map(|(name, system)| match read.remove(name) {
                Some(objects) => {
                    Schema { name: name.clone(), system: *system, level: Level::Details, objects, ..Schema::default() }
                }
                None => before
                    .and_then(|catalog| catalog.schemas.iter().find(|schema| schema.name == *name))
                    .cloned()
                    .unwrap_or_else(|| Schema { name: name.clone(), system: *system, ..Schema::default() }),
            })
            .collect(),
        ..Catalog::default()
    }
}

pub(super) async fn information_schema(
    client: &Client,
    current: &str,
    request: &IntrospectRequest,
    previous: Option<&Snapshot>,
) -> Result<Introspection, DbError> {
    let names: BTreeMap<String, bool> =
        simple(client, SCHEMAS).await?.iter().map(|row| (text(row, 0), system_schema(&text(row, 0)))).collect();
    let earlier = |name: &str| previous.and_then(|snapshot| snapshot.schema(current, name)).map(|schema| schema.level);
    let wanted = if request.outline { Vec::new() } else { candidates(&names, request, &earlier) };
    let objects = if wanted.is_empty() {
        BTreeMap::new()
    } else {
        let tables = simple(client, &tables_sql(&wanted)).await?;
        let columns = simple(client, &columns_sql(&wanted)).await?;
        let keys = simple(client, &keys_sql(&wanted)).await.unwrap_or_default();
        assemble(&wanted, &tables, &columns, &keys)
    };
    let read = wanted.iter().map(|schema| format!("{current}.{schema}")).collect();
    Ok(Introspection { catalogs: vec![catalog(current, &names, objects, previous)], read, unchanged: 0 })
}

pub(super) async fn meta_functions(
    client: &Client,
    current: &str,
    request: &IntrospectRequest,
    previous: Option<&Snapshot>,
) -> Result<Introspection, DbError> {
    let names: BTreeMap<String, bool> = BTreeMap::from([(QUESTDB_SCHEMA.to_owned(), false)]);
    let wanted: Vec<String> = if request.outline { Vec::new() } else { vec![QUESTDB_SCHEMA.to_owned()] };
    let mut objects = BTreeMap::new();
    if !wanted.is_empty() {
        let listed = simple(client, META_TABLE_LIST).await?;
        let mut tables = Vec::new();
        let mut columns = Vec::new();
        for (index, row) in listed.iter().enumerate() {
            let table = text(row, 0);
            if index < META_TABLES {
                for (position, column) in simple(client, &meta_columns_sql(&table)).await?.iter().enumerate() {
                    columns.push(vec![
                        Some(QUESTDB_SCHEMA.to_owned()),
                        Some(table.clone()),
                        Some((position + 1).to_string()),
                        cell(column, 0),
                        cell(column, 1),
                        Some("YES".to_owned()),
                        None,
                    ]);
                }
            }
            tables.push(vec![Some(QUESTDB_SCHEMA.to_owned()), Some(table), Some("BASE TABLE".to_owned())]);
        }
        objects = assemble(&wanted, &tables, &columns, &[]);
    }
    let read = wanted.iter().map(|schema| format!("{current}.{schema}")).collect();
    Ok(Introspection { catalogs: vec![catalog(current, &names, objects, previous)], read, unchanged: 0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[Option<&str>]) -> Row {
        cells.iter().map(|cell| cell.map(str::to_owned)).collect()
    }

    #[test]
    fn the_queries_name_schemas_as_literals_and_read_only_standard_views() {
        let schemas = vec!["public".to_owned(), "it's".to_owned()];
        let tables = tables_sql(&schemas);
        assert!(tables.contains("information_schema.tables WHERE table_schema IN ('public', 'it''s')"), "{tables}");
        let columns = columns_sql(&schemas);
        assert!(columns.contains("FROM information_schema.columns WHERE table_schema IN ('public', 'it''s')"));
        let keys = keys_sql(&schemas);
        assert!(keys.contains("information_schema.table_constraints") && keys.contains("key_column_usage"));
        for sql in [SCHEMAS, tables.as_str(), columns.as_str(), keys.as_str()] {
            for postgres_only in ["pg_", "xmin", "::", "ANY(", "$1"] {
                assert!(!sql.contains(postgres_only), "{postgres_only} in {sql}");
            }
        }
        assert_eq!(meta_columns_sql("trades"), "SELECT * FROM table_columns('trades')");
        assert_eq!(literal("a'b"), "'a''b'");
    }

    #[test]
    fn rows_become_tables_views_columns_and_keys() {
        let read = vec!["public".to_owned(), "empty".to_owned()];
        let tables = vec![
            row(&[Some("public"), Some("orders"), Some("BASE TABLE")]),
            row(&[Some("public"), Some("big_orders"), Some("VIEW")]),
            row(&[Some("public"), Some("totals"), Some("MATERIALIZED VIEW")]),
        ];
        let columns = vec![
            row(&[Some("public"), Some("orders"), Some("2"), Some("total"), Some("numeric"), Some("YES"), None]),
            row(&[
                Some("public"),
                Some("orders"),
                Some("1"),
                Some("id"),
                Some("bigint"),
                Some("NO"),
                Some("unique_rowid()"),
            ]),
            row(&[Some("public"), Some("big_orders"), Some("1"), Some("id"), Some("bigint"), Some("YES"), None]),
        ];
        let keys = vec![
            row(&[Some("public"), Some("orders"), Some("orders_pkey"), Some("PRIMARY KEY"), Some("id")]),
            row(&[Some("public"), Some("orders"), Some("orders_x"), Some("UNIQUE"), Some("id")]),
            row(&[Some("public"), Some("orders"), Some("orders_x"), Some("UNIQUE"), Some("total")]),
        ];
        let schemas = assemble(&read, &tables, &columns, &keys);
        assert!(schemas["empty"].is_empty());
        let objects = &schemas["public"];
        let kinds: Vec<(ObjectKind, &str)> = objects.iter().map(|object| (object.kind, object.name.as_str())).collect();
        assert_eq!(
            kinds,
            [(ObjectKind::Table, "orders"), (ObjectKind::View, "big_orders"), (ObjectKind::MaterializedView, "totals")]
        );
        let orders = &objects[0];
        assert!(orders.detailed);
        let names: Vec<&str> = orders.columns.iter().map(|column| column.name.as_str()).collect();
        assert_eq!(names, ["id", "total"]);
        assert!(!orders.columns[0].nullable && orders.columns[1].nullable);
        assert_eq!(orders.columns[0].default.as_deref(), Some("unique_rowid()"));
        assert_eq!(orders.constraints.len(), 2);
        assert_eq!(orders.constraints[0].definition, "PRIMARY KEY (id)");
        assert_eq!(orders.constraints[1].definition, "UNIQUE (id, total)");
        assert!(objects[1].constraints.is_empty());
    }

    #[test]
    fn the_servers_own_schemas_are_system_schemas() {
        for name in
            ["pg_catalog", "information_schema", "crdb_internal", "mz_catalog", "mz_internal", "rw_catalog", "sys"]
        {
            assert!(system_schema(name), "{name}");
        }
        assert!(!system_schema("public") && !system_schema("doc"));
    }
}

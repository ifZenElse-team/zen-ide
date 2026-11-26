use std::collections::BTreeMap;

use super::process::{Process, error};
use super::quote::{identifier, literal};
use crate::driver::DbError;
use crate::schema::{
    Catalog, Column, Constraint, ConstraintKind, Index, IntrospectRequest, Introspection, Level, Object, ObjectKind,
    Reference, Schema, Snapshot, Trigger,
};
use crate::value::Value;

type Rows = Vec<Vec<Value>>;

async fn rows(process: &mut Process, sql: &str) -> Result<Rows, DbError> {
    let printed = process.run(sql, usize::MAX).await?;
    if printed.errors.is_empty() { Ok(printed.rows) } else { Err(error(&printed.errors, sql)) }
}

fn text(row: &[Value], index: usize) -> String {
    row.get(index).and_then(Value::display).unwrap_or_default()
}

fn cell(row: &[Value], index: usize) -> Option<String> {
    row.get(index).and_then(Value::display)
}

fn int(row: &[Value], index: usize) -> i64 {
    match row.get(index) {
        Some(Value::Integer(value)) => *value,
        _ => 0,
    }
}

async fn databases(process: &mut Process) -> Result<Vec<(String, String)>, DbError> {
    let mut list = Vec::new();
    for row in rows(process, "SELECT name, file FROM pragma_database_list ORDER BY seq").await? {
        let name = text(&row, 0);
        if name == "temp" {
            let count = rows(process, "SELECT count(*) FROM temp.sqlite_master").await?;
            if count.first().is_none_or(|row| int(row, 0) == 0) {
                continue;
            }
        }
        list.push((name, text(&row, 1)));
    }
    Ok(list)
}

async fn fingerprint(process: &mut Process, schema: &str) -> Result<String, DbError> {
    let read = rows(process, &format!("PRAGMA {}.schema_version", identifier(schema))).await?;
    Ok(read.first().map(|row| text(row, 0)).unwrap_or_default())
}

async fn columns_of(process: &mut Process, schema: &str, objects: &mut [Object]) -> Result<(), DbError> {
    let (quoted, name) = (identifier(schema), literal(schema));
    let bulk = format!(
        "SELECT m.name, p.cid, p.name, p.type, p.\"notnull\", p.dflt_value, p.pk, p.hidden \
         FROM {quoted}.sqlite_master m JOIN pragma_table_xinfo(m.name, {name}) p \
         WHERE m.type IN ('table', 'view') ORDER BY m.name, p.cid"
    );
    let read = match rows(process, &bulk).await {
        Ok(read) => read,
        Err(_) => {
            let mut read = Vec::new();
            for object in objects.iter().filter(|object| object.kind.has_columns()) {
                let one = format!(
                    "SELECT {}, cid, name, type, \"notnull\", dflt_value, pk, hidden FROM pragma_table_xinfo({}, {name})",
                    literal(&object.name),
                    literal(&object.name)
                );
                read.extend(rows(process, &one).await.unwrap_or_default());
            }
            read
        }
    };
    let mut keys: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
    for row in read {
        let table = text(&row, 0);
        let Some(object) = objects.iter_mut().find(|object| object.name == table && object.kind.has_columns()) else {
            continue;
        };
        let column = text(&row, 2);
        let hidden = int(&row, 7);
        let pk = int(&row, 6);
        if pk > 0 {
            keys.entry(table.clone()).or_default().push((pk, column.clone()));
        }
        object.columns.push(Column {
            name: column,
            position: i32::try_from(int(&row, 1) + 1).unwrap_or(0),
            type_name: text(&row, 3),
            nullable: int(&row, 4) == 0 && pk == 0,
            default: cell(&row, 5),
            generated: match hidden {
                2 => Some("virtual".to_owned()),
                3 => Some("stored".to_owned()),
                _ => None,
            },
            ..Column::default()
        });
    }
    for (table, mut key) in keys {
        key.sort();
        if let Some(object) = objects.iter_mut().find(|object| object.name == table && object.kind == ObjectKind::Table)
        {
            let columns: Vec<String> = key.into_iter().map(|(_, column)| column).collect();
            object.constraints.push(Constraint {
                name: format!("{table}_pk"),
                kind: ConstraintKind::PrimaryKey,
                definition: format!(
                    "PRIMARY KEY ({})",
                    columns.iter().map(|column| identifier(column)).collect::<Vec<_>>().join(", ")
                ),
                columns,
                references: None,
                index: None,
                comment: None,
            });
        }
    }
    Ok(())
}

async fn foreign_keys(process: &mut Process, schema: &str, objects: &mut [Object]) -> Result<(), DbError> {
    let sql = format!(
        "SELECT m.name, f.id, f.\"table\", f.\"from\", f.\"to\", f.on_update, f.on_delete \
         FROM {}.sqlite_master m JOIN pragma_foreign_key_list(m.name, {}) f \
         WHERE m.type = 'table' ORDER BY m.name, f.id, f.seq",
        identifier(schema),
        literal(schema)
    );
    let mut found: BTreeMap<(String, i64), Constraint> = BTreeMap::new();
    for row in rows(process, &sql).await.unwrap_or_default() {
        let key = (text(&row, 0), int(&row, 1));
        let constraint = found.entry(key.clone()).or_insert_with(|| Constraint {
            name: format!("{}_fk_{}", key.0, key.1),
            kind: ConstraintKind::ForeignKey,
            columns: Vec::new(),
            definition: String::new(),
            references: Some(Reference {
                schema: schema.to_owned(),
                table: text(&row, 2),
                columns: Vec::new(),
                on_update: text(&row, 5).to_ascii_lowercase(),
                on_delete: text(&row, 6).to_ascii_lowercase(),
            }),
            index: None,
            comment: None,
        });
        constraint.columns.push(text(&row, 3));
        if let (Some(reference), Some(to)) = (constraint.references.as_mut(), cell(&row, 4)) {
            reference.columns.push(to);
        }
    }
    for ((table, _), mut constraint) in found {
        if let Some(reference) = &constraint.references {
            let quote = |list: &[String]| list.iter().map(|name| identifier(name)).collect::<Vec<_>>().join(", ");
            let target = if reference.columns.is_empty() {
                identifier(&reference.table)
            } else {
                format!("{}({})", identifier(&reference.table), quote(&reference.columns))
            };
            constraint.definition = format!("FOREIGN KEY ({}) REFERENCES {target}", quote(&constraint.columns));
        }
        if let Some(object) = objects.iter_mut().find(|object| object.name == table && object.kind == ObjectKind::Table)
        {
            object.constraints.push(constraint);
        }
    }
    Ok(())
}

async fn indexes(
    process: &mut Process,
    schema: &str,
    objects: &mut [Object],
    sql_of: &BTreeMap<String, String>,
) -> Result<(), DbError> {
    let sql = format!(
        "SELECT m.name, l.name, l.\"unique\", l.origin, i.seqno, i.name \
         FROM {}.sqlite_master m JOIN pragma_index_list(m.name, {name}) l JOIN pragma_index_xinfo(l.name, {name}) i \
         WHERE m.type = 'table' AND i.key ORDER BY m.name, l.name, i.seqno",
        identifier(schema),
        name = literal(schema)
    );
    let mut found: BTreeMap<(String, String), Index> = BTreeMap::new();
    for row in rows(process, &sql).await.unwrap_or_default() {
        let (table, name) = (text(&row, 0), text(&row, 1));
        let origin = text(&row, 3);
        let index = found.entry((table, name.clone())).or_insert_with(|| Index {
            name: name.clone(),
            unique: int(&row, 2) != 0,
            primary: origin == "pk",
            method: "btree".to_owned(),
            definition: sql_of.get(&name).cloned().unwrap_or_default(),
            constraint: (origin == "u").then(|| name.clone()),
            ..Index::default()
        });
        index.columns.push(cell(&row, 5).unwrap_or_else(|| "<expression>".to_owned()));
    }
    for ((table, _), index) in found {
        let Some(object) = objects.iter_mut().find(|object| object.name == table && object.kind == ObjectKind::Table)
        else {
            continue;
        };
        if index.constraint.is_some() {
            object.constraints.push(Constraint {
                name: index.name.clone(),
                kind: ConstraintKind::Unique,
                columns: index.columns.clone(),
                definition: format!(
                    "UNIQUE ({})",
                    index.columns.iter().map(|column| identifier(column)).collect::<Vec<_>>().join(", ")
                ),
                references: None,
                index: Some(index.name.clone()),
                comment: None,
            });
        }
        if index.primary
            && let Some(key) = object.constraints.iter_mut().find(|known| known.kind == ConstraintKind::PrimaryKey)
        {
            key.index = Some(index.name.clone());
        }
        object.indexes.push(index);
    }
    Ok(())
}

async fn read_schema(process: &mut Process, name: &str) -> Result<Schema, DbError> {
    let listed = rows(
        process,
        &format!("SELECT type, name, tbl_name, sql FROM {}.sqlite_master ORDER BY type, name", identifier(name)),
    )
    .await?;
    let mut objects = Vec::new();
    let mut index_sql = BTreeMap::new();
    let mut triggers = Vec::new();
    for row in &listed {
        let (kind, object) = (text(row, 0), text(row, 1));
        let definition = cell(row, 3);
        match kind.as_str() {
            "table" | "view" => {
                let mut found =
                    Object::new(if kind == "table" { ObjectKind::Table } else { ObjectKind::View }, object.clone());
                found.system = object.starts_with("sqlite_");
                found.detailed = true;
                found.definition = definition;
                objects.push(found);
            }
            "index" => {
                if let Some(definition) = definition {
                    index_sql.insert(object, definition);
                }
            }
            "trigger" => triggers.push((text(row, 2), object, definition.unwrap_or_default())),
            _ => {}
        }
    }
    columns_of(process, name, &mut objects).await?;
    foreign_keys(process, name, &mut objects).await?;
    indexes(process, name, &mut objects, &index_sql).await?;
    for (table, trigger, definition) in triggers {
        if let Some(object) = objects.iter_mut().find(|object| object.name == table) {
            object.triggers.push(Trigger { name: trigger, definition, enabled: true, function: String::new() });
        }
    }
    Ok(Schema {
        name: name.to_owned(),
        system: false,
        level: Level::Details,
        objects,
        fingerprint: Some(fingerprint(process, name).await?),
        ..Schema::default()
    })
}

pub(super) async fn introspect(
    process: &mut Process,
    catalog: &str,
    request: &IntrospectRequest,
    previous: Option<&Snapshot>,
) -> Result<Introspection, DbError> {
    let before = previous.and_then(|snapshot| snapshot.catalog(catalog));
    let earlier = |name: &str| before.and_then(|catalog| catalog.schemas.iter().find(|schema| schema.name == name));
    let mut read = Vec::new();
    let mut unchanged = 0;
    let mut schemas = Vec::new();
    for (name, _) in databases(process).await? {
        let kept = earlier(&name).cloned();
        let wanted = !request.outline
            && (request.schemas.is_empty()
                || request.schemas.contains(&name)
                || request.objects.iter().any(|(schema, _)| *schema == name));
        if !wanted {
            schemas.push(kept.unwrap_or(Schema { name, ..Schema::default() }));
            continue;
        }
        let print = fingerprint(process, &name).await?;
        let fresh = request.force
            || !request.objects.is_empty()
            || kept.as_ref().is_none_or(|schema| {
                schema.level == Level::NotRead || schema.fingerprint.as_deref() != Some(print.as_str())
            });
        match kept {
            Some(schema) if !fresh => {
                unchanged += 1;
                schemas.push(schema);
            }
            _ => {
                read.push(format!("{catalog}.{name}"));
                schemas.push(read_schema(process, &name).await?);
            }
        }
    }
    let catalog =
        Catalog { name: catalog.to_owned(), current: true, schemas_read: true, schemas, ..Catalog::default() };
    Ok(Introspection { catalogs: vec![catalog], read, unchanged })
}

pub(super) fn ddl(snapshot: &Snapshot, path: &[String], nested: bool) -> Option<String> {
    let found = crate::schema::find_object(snapshot, path)?;
    let object = found.object;
    let mut statements = vec![object.definition.clone()?];
    if nested {
        statements.extend(object.indexes.iter().map(|index| index.definition.clone()).filter(|sql| !sql.is_empty()));
        statements.extend(object.triggers.iter().map(|trigger| trigger.definition.clone()));
    }
    Some(statements.into_iter().map(|sql| format!("{};\n", sql.trim_end().trim_end_matches(';'))).collect::<String>())
}

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value as Json, json};
use tiberius::{ColumnData, Row};
use zen_db_driver_kit::{Answer, Failure};

use crate::connect::Sql;
use crate::session::{failure, flag, int, select, text};
use crate::words::{bracket, literal};

const OBJECT_TYPES: &str = "('U', 'V', 'P', 'PC', 'FN', 'IF', 'TF', 'FS', 'FT', 'SO')";

async fn rows(client: &mut Sql, sql: &str, params: &[&str]) -> Result<Vec<Row>, Failure> {
    let bound: Vec<ColumnData<'_>> = params.iter().map(|param| ColumnData::String(Some((*param).into()))).collect();
    let refs: Vec<&dyn tiberius::ToSql> = bound.iter().map(|value| value as &dyn tiberius::ToSql).collect();
    select(client, sql, &refs).await.map_err(|error| failure(&error, None))
}

pub(crate) async fn children(client: &mut Sql, path: &[String]) -> Result<Vec<Json>, Failure> {
    let node = |path: Vec<&str>, kind: &str, label: &str, has_children: bool| json!({ "path": path, "kind": kind, "label": label, "hasChildren": has_children });
    match path {
        [] => {
            let found = rows(client, "SELECT name, database_id FROM sys.databases ORDER BY name", &[]).await?;
            Ok(found
                .iter()
                .filter_map(|row| {
                    let name = text(row, 0)?;
                    let mut node = node(vec![&name], "catalog", &name, true);
                    node["system"] = json!(int(row, 1).unwrap_or(5) <= 4);
                    Some(node)
                })
                .collect())
        }
        [database] => {
            let sql = format!("SELECT name, schema_id FROM {}.sys.schemas ORDER BY name", bracket(database));
            let found = rows(client, &sql, &[]).await?;
            Ok(found
                .iter()
                .filter_map(|row| {
                    let name = text(row, 0)?;
                    let mut node = node(vec![database, &name], "schema", &name, true);
                    node["system"] = json!(system_schema(&name, int(row, 1).unwrap_or(0)));
                    Some(node)
                })
                .collect())
        }
        [database, schema] => {
            let sql = format!(
                "SELECT o.name, RTRIM(o.type) FROM {db}.sys.objects o JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id \
                 WHERE s.name = @P1 AND o.type IN ('U', 'V') ORDER BY o.name",
                db = bracket(database)
            );
            let found = rows(client, &sql, &[schema]).await?;
            Ok(found
                .iter()
                .filter_map(|row| {
                    let name = text(row, 0)?;
                    let kind = if text(row, 1).as_deref() == Some("V") { "view" } else { "table" };
                    Some(node(vec![database, schema, &name], kind, &name, true))
                })
                .collect())
        }
        [database, schema, object] => {
            let sql = format!(
                "SELECT c.name, t.name, c.max_length, c.precision, c.scale FROM {db}.sys.columns c \
                 JOIN {db}.sys.objects o ON o.object_id = c.object_id JOIN {db}.sys.schemas s ON s.schema_id = o.schema_id \
                 JOIN {db}.sys.types t ON t.user_type_id = c.user_type_id WHERE s.name = @P1 AND o.name = @P2 ORDER BY c.column_id",
                db = bracket(database)
            );
            let found = rows(client, &sql, &[schema, object]).await?;
            Ok(found
                .iter()
                .filter_map(|row| {
                    let name = text(row, 0)?;
                    let mut node = node(vec![database, schema, object, &name], "column", &name, false);
                    node["detail"] = json!(type_text(row, 1));
                    Some(node)
                })
                .collect())
        }
        _ => Ok(Vec::new()),
    }
}

fn system_schema(name: &str, id: i64) -> bool {
    id >= 16_384 || matches!(name, "sys" | "INFORMATION_SCHEMA" | "guest")
}

fn type_text(row: &Row, at: usize) -> String {
    let name = text(row, at).unwrap_or_default();
    let length = int(row, at + 1).unwrap_or(0);
    let precision = int(row, at + 2).unwrap_or(0);
    let scale = int(row, at + 3).unwrap_or(0);
    let size = |divisor: i64| if length < 0 { "max".to_owned() } else { (length / divisor).to_string() };
    match name.as_str() {
        "varchar" | "char" | "varbinary" | "binary" => format!("{name}({})", size(1)),
        "nvarchar" | "nchar" => format!("{name}({})", size(2)),
        "decimal" | "numeric" => format!("{name}({precision},{scale})"),
        "datetime2" | "time" | "datetimeoffset" => format!("{name}({scale})"),
        _ => name,
    }
}

struct Wanted {
    schemas: Vec<String>,
    objects: Vec<(String, String)>,
    force: bool,
    system: bool,
    large: usize,
    outline: bool,
    fingerprints: HashMap<String, String>,
}

impl Wanted {
    fn of(params: &Json) -> Self {
        let texts = |key: &str| -> Vec<String> {
            params
                .get(key)
                .and_then(Json::as_array)
                .map(|list| list.iter().filter_map(Json::as_str).map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let objects = params
            .get("objects")
            .and_then(Json::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|pair| Some((pair.get(0)?.as_str()?.to_owned(), pair.get(1)?.as_str()?.to_owned())))
                    .collect()
            })
            .unwrap_or_default();
        let fingerprints = params
            .get("fingerprints")
            .and_then(Json::as_object)
            .map(|map| map.iter().filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned()))).collect())
            .unwrap_or_default();
        Self {
            schemas: texts("schemas"),
            objects,
            force: params.get("force").and_then(Json::as_bool).unwrap_or(false),
            system: params.get("system").and_then(Json::as_bool).unwrap_or(false),
            large: params
                .get("largeSchema")
                .and_then(Json::as_u64)
                .map_or(2000, |n| usize::try_from(n).unwrap_or(2000)),
            outline: params.get("outline").and_then(Json::as_bool).unwrap_or(false),
            fingerprints,
        }
    }
}

#[derive(Clone)]
struct Found {
    id: i64,
    schema: String,
    name: String,
    kind: &'static str,
}

pub(crate) async fn introspect(client: &mut Sql, params: &Json) -> Answer {
    let wanted = Wanted::of(params);
    let current = rows(client, "SELECT DB_NAME()", &[]).await?.first().and_then(|row| text(row, 0)).unwrap_or_default();
    let databases = rows(client, "SELECT name, database_id FROM sys.databases ORDER BY name", &[]).await?;
    let schema_rows = rows(client, "SELECT name, schema_id FROM sys.schemas ORDER BY name", &[]).await?;
    let all_schemas: Vec<(String, bool)> = schema_rows
        .iter()
        .filter_map(|row| {
            let name = text(row, 0)?;
            let system = system_schema(&name, int(row, 1).unwrap_or(0));
            Some((name, system))
        })
        .collect();
    let mut catalogs: Vec<Json> = databases
        .iter()
        .filter_map(|row| {
            let name = text(row, 0)?;
            Some(json!({
                "name": name, "current": name == current, "system": int(row, 1).unwrap_or(5) <= 4,
                "schemasRead": false, "schemas": [],
            }))
        })
        .collect();
    let index = catalogs.iter().position(|catalog| catalog["name"] == current.as_str()).unwrap_or_else(|| {
        catalogs.push(json!({ "name": current, "current": true, "schemasRead": false, "schemas": [] }));
        catalogs.len() - 1
    });
    if wanted.outline {
        let schemas: Vec<Json> = all_schemas
            .iter()
            .filter(|(_, system)| wanted.system || !system)
            .map(|(name, system)| json!({ "name": name, "system": system }))
            .collect();
        catalogs[index]["schemasRead"] = json!(true);
        catalogs[index]["schemas"] = json!(schemas);
        return Ok(json!({ "catalogs": catalogs, "read": [], "unchanged": [] }));
    }
    let chosen: Vec<(String, bool)> = all_schemas
        .into_iter()
        .filter(
            |(name, system)| {
                if wanted.schemas.is_empty() { wanted.system || !system } else { wanted.schemas.contains(name) }
            },
        )
        .collect();
    let fingerprint_rows = rows(
        client,
        "SELECT SCHEMA_NAME(o.schema_id), COUNT_BIG(*), CONVERT(nvarchar(30), MAX(o.modify_date), 126) \
         FROM sys.objects o WHERE o.is_ms_shipped = 0 GROUP BY o.schema_id",
        &[],
    )
    .await?;
    let fingerprints: HashMap<String, String> = fingerprint_rows
        .iter()
        .filter_map(|row| {
            Some((text(row, 0)?, format!("{}-{}", int(row, 1).unwrap_or(0), text(row, 2).unwrap_or_default())))
        })
        .collect();
    let mut unchanged = Vec::new();
    let mut to_read = Vec::new();
    for (name, system) in &chosen {
        let fingerprint = fingerprints.get(name).cloned().unwrap_or_else(|| "0-".to_owned());
        let key = format!("{current}.{name}");
        let asked = wanted.objects.iter().any(|(schema, _)| schema == name);
        if !wanted.force && !asked && wanted.fingerprints.get(&key) == Some(&fingerprint) {
            unchanged.push(key);
        } else {
            to_read.push((name.clone(), *system, fingerprint));
        }
    }
    let names: Vec<String> = to_read.iter().map(|(name, _, _)| literal(name)).collect();
    let found = if names.is_empty() {
        Vec::new()
    } else {
        let sql = format!(
            "SELECT o.object_id, SCHEMA_NAME(o.schema_id), o.name, RTRIM(o.type) FROM sys.objects o \
             WHERE o.is_ms_shipped = 0 AND o.type IN {OBJECT_TYPES} AND SCHEMA_NAME(o.schema_id) IN ({}) \
             ORDER BY o.name",
            names.join(", ")
        );
        rows(client, &sql, &[])
            .await?
            .iter()
            .filter_map(|row| {
                Some(Found {
                    id: int(row, 0)?,
                    schema: text(row, 1)?,
                    name: text(row, 2)?,
                    kind: kind_of(text(row, 3).as_deref().unwrap_or_default())?,
                })
            })
            .collect::<Vec<_>>()
    };
    let mut per_schema: BTreeMap<String, Vec<Found>> = BTreeMap::new();
    for object in &found {
        per_schema.entry(object.schema.clone()).or_default().push(object.clone());
    }
    let mut detailed: BTreeSet<i64> = BTreeSet::new();
    for (schema, _, _) in &to_read {
        let objects = per_schema.get(schema).map(Vec::as_slice).unwrap_or_default();
        if objects.len() <= wanted.large {
            detailed.extend(objects.iter().map(|object| object.id));
        } else {
            detailed.extend(
                objects
                    .iter()
                    .filter(|object| wanted.objects.iter().any(|(s, n)| *s == object.schema && *n == object.name))
                    .map(|object| object.id),
            );
        }
    }
    let details = if detailed.is_empty() { Details::default() } else { Details::read(client, &detailed).await? };
    let mut read = Vec::new();
    let mut schemas = Vec::new();
    for (schema, system, fingerprint) in &to_read {
        let objects = per_schema.get(schema).map(Vec::as_slice).unwrap_or_default();
        let all_details = objects.iter().all(|object| detailed.contains(&object.id));
        let wire: Vec<Json> =
            objects.iter().map(|object| details.object(object, detailed.contains(&object.id))).collect();
        schemas.push(json!({
            "name": schema, "system": system, "fingerprint": fingerprint,
            "level": if all_details { "details" } else { "names" },
            "objects": wire,
        }));
        read.push(format!("{current}.{schema}"));
    }
    catalogs[index]["schemasRead"] = json!(true);
    catalogs[index]["schemas"] = json!(schemas);
    Ok(json!({ "catalogs": catalogs, "read": read, "unchanged": unchanged }))
}

fn kind_of(code: &str) -> Option<&'static str> {
    Some(match code {
        "U" => "table",
        "V" => "view",
        "P" | "PC" => "procedure",
        "FN" | "IF" | "TF" | "FS" | "FT" => "function",
        "SO" => "sequence",
        _ => return None,
    })
}

#[derive(Default)]
struct Details {
    rows: HashMap<i64, i64>,
    comments: HashMap<(i64, i64), String>,
    columns: HashMap<i64, Vec<Json>>,
    constraints: HashMap<i64, Vec<Json>>,
    indexes: HashMap<i64, Vec<Json>>,
    definitions: HashMap<i64, String>,
}

impl Details {
    async fn read(client: &mut Sql, ids: &BTreeSet<i64>) -> Result<Self, Failure> {
        let list: Vec<String> = ids.iter().map(i64::to_string).collect();
        let among = |column: &str| format!("{column} IN ({})", list.join(", "));
        let mut details = Details::default();
        let sql = format!(
            "SELECT p.object_id, SUM(p.rows) FROM sys.partitions p WHERE p.index_id IN (0, 1) AND {} GROUP BY p.object_id",
            among("p.object_id")
        );
        for row in rows(client, &sql, &[]).await? {
            if let (Some(id), Some(count)) = (int(&row, 0), int(&row, 1)) {
                details.rows.insert(id, count);
            }
        }
        let sql = format!(
            "SELECT ep.major_id, ep.minor_id, CAST(ep.value AS nvarchar(4000)) FROM sys.extended_properties ep \
             WHERE ep.class = 1 AND ep.name = N'MS_Description' AND {}",
            among("ep.major_id")
        );
        for row in rows(client, &sql, &[]).await? {
            if let (Some(id), Some(minor), Some(comment)) = (int(&row, 0), int(&row, 1), text(&row, 2)) {
                details.comments.insert((id, minor), comment);
            }
        }
        let sql = format!(
            "SELECT c.object_id, c.column_id, c.name, t.name, c.max_length, c.precision, c.scale, c.is_nullable, \
             OBJECT_DEFINITION(c.default_object_id), c.is_identity, cc.definition, c.collation_name \
             FROM sys.columns c JOIN sys.types t ON t.user_type_id = c.user_type_id \
             LEFT JOIN sys.computed_columns cc ON cc.object_id = c.object_id AND cc.column_id = c.column_id \
             WHERE {} ORDER BY c.object_id, c.column_id",
            among("c.object_id")
        );
        for row in rows(client, &sql, &[]).await? {
            let (Some(id), Some(position), Some(name)) = (int(&row, 0), int(&row, 1), text(&row, 2)) else { continue };
            let mut column = json!({
                "name": name, "position": position, "typeName": type_text(&row, 3), "nullable": flag(&row, 7),
            });
            if let Some(default) = text(&row, 8) {
                column["default"] = json!(default);
            }
            if flag(&row, 9) {
                column["identity"] = json!("always");
            }
            if let Some(generated) = text(&row, 10) {
                column["generated"] = json!(generated);
            }
            if let Some(collation) = text(&row, 11) {
                column["collation"] = json!(collation);
            }
            if let Some(comment) = details.comments.get(&(id, position)) {
                column["comment"] = json!(comment);
            }
            details.columns.entry(id).or_default().push(column);
        }
        let sql = format!(
            "SELECT kc.parent_object_id, kc.name, kc.type, COL_NAME(ic.object_id, ic.column_id), i.name \
             FROM sys.key_constraints kc JOIN sys.indexes i ON i.object_id = kc.parent_object_id AND i.index_id = kc.unique_index_id \
             JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             WHERE ic.key_ordinal > 0 AND {} ORDER BY kc.parent_object_id, kc.name, ic.key_ordinal",
            among("kc.parent_object_id")
        );
        let mut keys: BTreeMap<(i64, String), Json> = BTreeMap::new();
        for row in rows(client, &sql, &[]).await? {
            let (Some(id), Some(name), Some(column)) = (int(&row, 0), text(&row, 1), text(&row, 3)) else { continue };
            let kind = if text(&row, 2).as_deref().map(str::trim) == Some("PK") { "primaryKey" } else { "unique" };
            let entry = keys.entry((id, name.clone())).or_insert_with(
                || json!({ "name": name, "kind": kind, "columns": [], "definition": "", "index": text(&row, 4) }),
            );
            if let Some(columns) = entry["columns"].as_array_mut() {
                columns.push(json!(column));
            }
        }
        for ((id, _), key) in keys {
            details.constraints.entry(id).or_default().push(key);
        }
        let sql = format!(
            "SELECT fk.parent_object_id, fk.name, COL_NAME(fkc.parent_object_id, fkc.parent_column_id), \
             SCHEMA_NAME(r.schema_id), r.name, COL_NAME(fkc.referenced_object_id, fkc.referenced_column_id), \
             fk.update_referential_action_desc, fk.delete_referential_action_desc \
             FROM sys.foreign_keys fk JOIN sys.foreign_key_columns fkc ON fkc.constraint_object_id = fk.object_id \
             JOIN sys.objects r ON r.object_id = fk.referenced_object_id \
             WHERE {} ORDER BY fk.parent_object_id, fk.name, fkc.constraint_column_id",
            among("fk.parent_object_id")
        );
        let mut foreign: BTreeMap<(i64, String), Json> = BTreeMap::new();
        for row in rows(client, &sql, &[]).await? {
            let (Some(id), Some(name), Some(column), Some(to_column)) =
                (int(&row, 0), text(&row, 1), text(&row, 2), text(&row, 5))
            else {
                continue;
            };
            let action = |at: usize| text(&row, at).unwrap_or_default().replace('_', " ");
            let entry = foreign.entry((id, name.clone())).or_insert_with(|| {
                json!({
                    "name": name, "kind": "foreignKey", "columns": [], "definition": "",
                    "references": {
                        "schema": text(&row, 3).unwrap_or_default(), "table": text(&row, 4).unwrap_or_default(),
                        "columns": [], "onUpdate": action(6), "onDelete": action(7),
                    },
                })
            });
            if let Some(columns) = entry["columns"].as_array_mut() {
                columns.push(json!(column));
            }
            if let Some(columns) = entry["references"]["columns"].as_array_mut() {
                columns.push(json!(to_column));
            }
        }
        for ((id, _), constraint) in foreign {
            details.constraints.entry(id).or_default().push(constraint);
        }
        let sql = format!(
            "SELECT parent_object_id, name, definition FROM sys.check_constraints WHERE {} ORDER BY name",
            among("parent_object_id")
        );
        for row in rows(client, &sql, &[]).await? {
            let (Some(id), Some(name)) = (int(&row, 0), text(&row, 1)) else { continue };
            details.constraints.entry(id).or_default().push(json!({
                "name": name, "kind": "check", "columns": [], "definition": text(&row, 2).unwrap_or_default(),
            }));
        }
        let sql = format!(
            "SELECT i.object_id, i.name, i.is_unique, i.is_primary_key, i.type_desc, COL_NAME(ic.object_id, ic.column_id), \
             i.is_unique_constraint FROM sys.indexes i JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
             WHERE i.index_id > 0 AND ic.key_ordinal > 0 AND {} ORDER BY i.object_id, i.name, ic.key_ordinal",
            among("i.object_id")
        );
        let mut indexes: BTreeMap<(i64, String), Json> = BTreeMap::new();
        for row in rows(client, &sql, &[]).await? {
            let (Some(id), Some(name), Some(column)) = (int(&row, 0), text(&row, 1), text(&row, 5)) else { continue };
            let entry = indexes.entry((id, name.clone())).or_insert_with(|| {
                let constraint = (flag(&row, 3) || flag(&row, 6)).then(|| name.clone());
                json!({
                    "name": name, "columns": [], "unique": flag(&row, 2), "primary": flag(&row, 3),
                    "method": text(&row, 4).unwrap_or_default().to_lowercase(), "definition": "", "constraint": constraint,
                })
            });
            if let Some(columns) = entry["columns"].as_array_mut() {
                columns.push(json!(column));
            }
        }
        for ((id, _), index) in indexes {
            details.indexes.entry(id).or_default().push(index);
        }
        let sql = format!(
            "SELECT o.object_id, OBJECT_DEFINITION(o.object_id) FROM sys.objects o \
             WHERE o.type IN ('V', 'P', 'FN', 'IF', 'TF') AND {}",
            among("o.object_id")
        );
        for row in rows(client, &sql, &[]).await? {
            if let (Some(id), Some(definition)) = (int(&row, 0), text(&row, 1)) {
                details.definitions.insert(id, definition);
            }
        }
        Ok(details)
    }

    fn object(&self, found: &Found, detailed: bool) -> Json {
        let mut object = json!({
            "kind": found.kind, "name": found.name, "oid": u32::try_from(found.id).unwrap_or(0), "detailed": detailed,
        });
        if let Some(comment) = self.comments.get(&(found.id, 0)) {
            object["comment"] = json!(comment);
        }
        if !detailed {
            return object;
        }
        if let Some(rows) = self.rows.get(&found.id) {
            object["rows"] = json!(rows);
        }
        object["columns"] = json!(self.columns.get(&found.id).cloned().unwrap_or_default());
        object["constraints"] = json!(self.constraints.get(&found.id).cloned().unwrap_or_default());
        object["indexes"] = json!(self.indexes.get(&found.id).cloned().unwrap_or_default());
        if let Some(definition) = self.definitions.get(&found.id) {
            object["definition"] = json!(definition);
        }
        object
    }
}

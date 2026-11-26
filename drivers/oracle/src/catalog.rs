use std::collections::{BTreeMap, HashMap};

use serde_json::{Value as Json, json};
use zen_db_driver_kit::{Answer, Failure};

use crate::session::Server;
use crate::statement::literal;

const KINDS: &str = "('TABLE', 'VIEW', 'MATERIALIZED VIEW', 'PROCEDURE', 'FUNCTION', 'PACKAGE', 'SEQUENCE', 'TYPE')";
const CHUNK: usize = 500;

fn text(row: &[Json], at: usize) -> Option<String> {
    let cell = row.get(at)?;
    match &cell["value"] {
        Json::String(text) => Some(text.clone()),
        Json::Number(number) => Some(number.to_string()),
        Json::Object(inner) => inner.get("text").and_then(Json::as_str).map(str::to_owned),
        _ => None,
    }
}

fn int(row: &[Json], at: usize) -> Option<i64> {
    let cell = row.get(at)?;
    cell["value"].as_i64().or_else(|| cell["value"].as_str().and_then(|text| text.parse().ok()))
}

fn list(names: &[String]) -> String {
    names.iter().map(|name| literal(name)).collect::<Vec<_>>().join(", ")
}

fn kind_of(object_type: &str) -> Option<&'static str> {
    Some(match object_type {
        "TABLE" => "table",
        "VIEW" => "view",
        "MATERIALIZED VIEW" => "materializedView",
        "PROCEDURE" => "procedure",
        "FUNCTION" => "function",
        "PACKAGE" => "package",
        "SEQUENCE" => "sequence",
        "TYPE" => "type",
        _ => return None,
    })
}

fn node(path: Vec<String>, kind: &str, has_children: bool) -> Json {
    let label = path.last().cloned().unwrap_or_default();
    json!({ "path": path, "kind": kind, "label": label, "hasChildren": has_children })
}

pub(crate) async fn children(server: &mut Server, path: &[String]) -> Result<Vec<Json>, Failure> {
    match path {
        [] => {
            let rows = server
                .rows("SELECT username, oracle_maintained FROM all_users ORDER BY oracle_maintained, username")
                .await?;
            Ok(rows
                .iter()
                .filter_map(|row| {
                    let mut found = node(vec![text(row, 0)?], "schema", true);
                    found["system"] = json!(text(row, 1).as_deref() == Some("Y"));
                    Some(found)
                })
                .collect())
        }
        [schema] => {
            let rows = server
                .rows(&format!(
                    "SELECT object_name, object_type FROM all_objects WHERE owner = {} \
                     AND object_type IN ('TABLE', 'VIEW') AND generated = 'N' AND object_name NOT LIKE 'BIN$%' ORDER BY object_name",
                    literal(schema)
                ))
                .await?;
            Ok(rows
                .iter()
                .filter_map(|row| {
                    let kind = kind_of(&text(row, 1)?)?;
                    Some(node(vec![schema.clone(), text(row, 0)?], kind, true))
                })
                .collect())
        }
        [schema, table] => {
            let rows = server
                .rows(&format!(
                    "SELECT column_name, data_type, nullable FROM all_tab_columns WHERE owner = {} AND table_name = {} ORDER BY column_id",
                    literal(schema),
                    literal(table)
                ))
                .await?;
            Ok(rows
                .iter()
                .filter_map(|row| {
                    let mut found = node(vec![schema.clone(), table.clone(), text(row, 0)?], "column", false);
                    let nullable = if text(row, 2).as_deref() == Some("N") { " NOT NULL" } else { "" };
                    found["detail"] = json!(format!("{}{nullable}", text(row, 1).unwrap_or_default()));
                    Some(found)
                })
                .collect())
        }
        _ => Ok(Vec::new()),
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
        let strings = |key: &str| -> Vec<String> {
            params
                .get(key)
                .and_then(Json::as_array)
                .map(|items| items.iter().filter_map(Json::as_str).map(str::to_owned).collect())
                .unwrap_or_default()
        };
        Wanted {
            schemas: strings("schemas"),
            objects: params
                .get("objects")
                .and_then(Json::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|pair| Some((pair.get(0)?.as_str()?.to_owned(), pair.get(1)?.as_str()?.to_owned())))
                        .collect()
                })
                .unwrap_or_default(),
            force: params.get("force").and_then(Json::as_bool).unwrap_or(false),
            system: params.get("system").and_then(Json::as_bool).unwrap_or(false),
            large: params
                .get("largeSchema")
                .and_then(Json::as_u64)
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(2000),
            outline: params.get("outline").and_then(Json::as_bool).unwrap_or(false),
            fingerprints: params
                .get("fingerprints")
                .and_then(Json::as_object)
                .map(|map| {
                    map.iter().filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned()))).collect()
                })
                .unwrap_or_default(),
        }
    }
}

#[derive(Clone)]
struct Found {
    id: i64,
    name: String,
    object_type: String,
}

#[derive(Default)]
struct Details {
    columns: HashMap<String, Vec<Json>>,
    constraints: HashMap<String, Vec<Json>>,
    indexes: HashMap<String, Vec<Json>>,
    comments: HashMap<String, String>,
    rows: HashMap<String, i64>,
    definitions: HashMap<String, String>,
    sequences: HashMap<String, Json>,
}

fn type_text(row: &[Json], at: usize) -> String {
    let data_type = text(row, at).unwrap_or_default();
    let length = int(row, at + 1);
    let chars = int(row, at + 2);
    let char_used = text(row, at + 3);
    let precision = int(row, at + 4);
    let scale = int(row, at + 5);
    match data_type.as_str() {
        "VARCHAR2" | "NVARCHAR2" | "CHAR" | "NCHAR" => {
            let unit = if char_used.as_deref() == Some("C") && !data_type.starts_with('N') { " CHAR" } else { "" };
            let size = if char_used.as_deref() == Some("C") { chars.or(length) } else { length };
            size.map_or(data_type.clone(), |size| format!("{data_type}({size}{unit})"))
        }
        "RAW" => length.map_or(data_type.clone(), |size| format!("RAW({size})")),
        "NUMBER" => match (precision, scale) {
            (Some(precision), Some(0)) => format!("NUMBER({precision})"),
            (Some(precision), Some(scale)) => format!("NUMBER({precision},{scale})"),
            (None, Some(0)) => "INTEGER".to_owned(),
            _ => data_type,
        },
        _ => data_type,
    }
}

fn not_null_check(condition: &str) -> bool {
    let condition = condition.trim();
    condition.starts_with('"') && condition.ends_with("\" IS NOT NULL") && condition.matches('"').count() == 2
}

impl Details {
    async fn read(server: &mut Server, schema: &str, names: Option<&[String]>) -> Result<Self, Failure> {
        let mut details = Details::default();
        let chunks: Vec<Option<Vec<String>>> = match names {
            None => vec![None],
            Some(names) => names.chunks(CHUNK).map(|chunk| Some(chunk.to_vec())).collect(),
        };
        let owner = literal(schema);
        for chunk in chunks {
            let among = |column: &str| {
                chunk.as_ref().map_or_else(String::new, |names| format!(" AND {column} IN ({})", list(names)))
            };
            let sql = format!(
                "SELECT table_name, column_id, column_name, data_type, data_length, char_length, char_used, data_precision, \
                 data_scale, nullable, data_default, identity_column FROM all_tab_columns WHERE owner = {owner}{} \
                 ORDER BY table_name, column_id",
                among("table_name")
            );
            for row in server.rows(&sql).await? {
                let (Some(table), Some(name)) = (text(&row, 0), text(&row, 2)) else { continue };
                let mut column = json!({
                    "name": name, "position": int(&row, 1).unwrap_or(0), "typeName": type_text(&row, 3),
                    "nullable": text(&row, 9).as_deref() != Some("N"),
                });
                if let Some(default) =
                    text(&row, 10).map(|default| default.trim().to_owned()).filter(|default| !default.is_empty())
                {
                    column["default"] = json!(default);
                }
                if text(&row, 11).as_deref() == Some("YES") {
                    column["identity"] = json!("byDefault");
                    column.as_object_mut().map(|object| object.remove("default"));
                }
                details.columns.entry(table).or_default().push(column);
            }
            let sql = format!(
                "SELECT table_name, column_name, comments FROM all_col_comments WHERE owner = {owner} AND comments IS NOT NULL{}",
                among("table_name")
            );
            let mut column_comments: HashMap<(String, String), String> = HashMap::new();
            for row in server.rows(&sql).await? {
                if let (Some(table), Some(column), Some(comment)) = (text(&row, 0), text(&row, 1), text(&row, 2)) {
                    column_comments.insert((table, column), comment);
                }
            }
            for (table, columns) in &mut details.columns {
                for column in columns.iter_mut() {
                    let name = column["name"].as_str().unwrap_or_default().to_owned();
                    if let Some(comment) = column_comments.get(&(table.clone(), name)) {
                        column["comment"] = json!(comment);
                    }
                }
            }
            let sql = format!(
                "SELECT table_name, comments FROM all_tab_comments WHERE owner = {owner} AND comments IS NOT NULL{}",
                among("table_name")
            );
            for row in server.rows(&sql).await? {
                if let (Some(table), Some(comment)) = (text(&row, 0), text(&row, 1)) {
                    details.comments.insert(table, comment);
                }
            }
            let sql = format!(
                "SELECT c.table_name, c.constraint_name, c.constraint_type, cc.column_name, r.owner, r.table_name, rc.column_name, \
                 c.delete_rule, c.index_name FROM all_constraints c \
                 JOIN all_cons_columns cc ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name \
                 LEFT JOIN all_constraints r ON r.owner = c.r_owner AND r.constraint_name = c.r_constraint_name \
                 LEFT JOIN all_cons_columns rc ON rc.owner = r.owner AND rc.constraint_name = r.constraint_name AND rc.position = cc.position \
                 WHERE c.owner = {owner} AND c.constraint_type IN ('P', 'U', 'R'){} ORDER BY c.table_name, c.constraint_name, cc.position",
                among("c.table_name")
            );
            let mut keys: BTreeMap<(String, String), Json> = BTreeMap::new();
            for row in server.rows(&sql).await? {
                let (Some(table), Some(name), Some(kind), Some(column)) =
                    (text(&row, 0), text(&row, 1), text(&row, 2), text(&row, 3))
                else {
                    continue;
                };
                let entry = keys.entry((table, name.clone())).or_insert_with(|| {
                    let mut key = json!({
                        "name": name, "columns": [], "definition": "",
                        "kind": match kind.as_str() { "P" => "primaryKey", "U" => "unique", _ => "foreignKey" },
                    });
                    if let Some(index) = text(&row, 8) {
                        key["index"] = json!(index);
                    }
                    if kind == "R" {
                        key["references"] = json!({
                            "schema": text(&row, 4).unwrap_or_default(), "table": text(&row, 5).unwrap_or_default(),
                            "columns": [], "onUpdate": "no action",
                            "onDelete": text(&row, 7).unwrap_or_else(|| "NO ACTION".to_owned()).to_ascii_lowercase(),
                        });
                    }
                    key
                });
                if let Some(columns) = entry["columns"].as_array_mut() {
                    columns.push(json!(column));
                }
                let referenced = entry.get_mut("references").and_then(|references| references.get_mut("columns"));
                if let (Some(columns), Some(to)) = (referenced.and_then(Json::as_array_mut), text(&row, 6)) {
                    columns.push(json!(to));
                }
            }
            for ((table, _), mut key) in keys {
                let columns: Vec<String> = key["columns"]
                    .as_array()
                    .map(|columns| columns.iter().filter_map(Json::as_str).map(|name| format!("\"{name}\"")).collect())
                    .unwrap_or_default();
                key["definition"] = json!(match key["kind"].as_str() {
                    Some("primaryKey") => format!("PRIMARY KEY ({})", columns.join(", ")),
                    Some("unique") => format!("UNIQUE ({})", columns.join(", ")),
                    _ => {
                        let to: Vec<String> = key["references"]["columns"]
                            .as_array()
                            .map(|columns| {
                                columns.iter().filter_map(Json::as_str).map(|name| format!("\"{name}\"")).collect()
                            })
                            .unwrap_or_default();
                        format!(
                            "FOREIGN KEY ({}) REFERENCES \"{}\".\"{}\" ({})",
                            columns.join(", "),
                            key["references"]["schema"].as_str().unwrap_or_default(),
                            key["references"]["table"].as_str().unwrap_or_default(),
                            to.join(", ")
                        )
                    }
                });
                details.constraints.entry(table).or_default().push(key);
            }
            let sql = format!(
                "SELECT table_name, constraint_name, search_condition FROM all_constraints \
                 WHERE owner = {owner} AND constraint_type = 'C'{}",
                among("table_name")
            );
            for row in server.rows(&sql).await? {
                let (Some(table), Some(name)) = (text(&row, 0), text(&row, 1)) else { continue };
                let condition = text(&row, 2).unwrap_or_default();
                if not_null_check(&condition) {
                    continue;
                }
                details.constraints.entry(table).or_default().push(json!({
                    "name": name, "kind": "check", "columns": [], "definition": format!("CHECK ({condition})"),
                }));
            }
            let sql = format!(
                "SELECT i.table_name, i.index_name, i.uniqueness, ic.column_name, i.index_type FROM all_indexes i \
                 JOIN all_ind_columns ic ON ic.index_owner = i.owner AND ic.index_name = i.index_name \
                 WHERE i.table_owner = {owner}{} ORDER BY i.table_name, i.index_name, ic.column_position",
                among("i.table_name")
            );
            let mut indexes: BTreeMap<(String, String), Json> = BTreeMap::new();
            for row in server.rows(&sql).await? {
                let (Some(table), Some(name), Some(column)) = (text(&row, 0), text(&row, 1), text(&row, 3)) else {
                    continue;
                };
                let entry = indexes.entry((table.clone(), name.clone())).or_insert_with(|| {
                    json!({
                        "name": name, "columns": [], "unique": text(&row, 2).as_deref() == Some("UNIQUE"), "primary": false,
                        "method": text(&row, 4).unwrap_or_default().to_ascii_lowercase(), "definition": "",
                    })
                });
                if let Some(columns) = entry["columns"].as_array_mut() {
                    columns.push(json!(column));
                }
            }
            for ((table, name), mut index) in indexes {
                let key = details.constraints.get(&table).and_then(|keys| {
                    keys.iter().find(|key| {
                        key["index"].as_str() == Some(name.as_str()) || key["name"].as_str() == Some(name.as_str())
                    })
                });
                if let Some(key) = key {
                    index["primary"] = json!(key["kind"] == "primaryKey");
                    index["constraint"] = key["name"].clone();
                }
                let columns: Vec<String> = index["columns"]
                    .as_array()
                    .map(|columns| columns.iter().filter_map(Json::as_str).map(|name| format!("\"{name}\"")).collect())
                    .unwrap_or_default();
                index["definition"] = json!(format!(
                    "CREATE {}INDEX \"{schema}\".\"{name}\" ON \"{schema}\".\"{table}\" ({})",
                    if index["unique"] == true { "UNIQUE " } else { "" },
                    columns.join(", ")
                ));
                details.indexes.entry(table).or_default().push(index);
            }
            let sql =
                format!("SELECT table_name, num_rows FROM all_tables WHERE owner = {owner}{}", among("table_name"));
            for row in server.rows(&sql).await? {
                if let (Some(table), Some(count)) = (text(&row, 0), int(&row, 1)) {
                    details.rows.insert(table, count);
                }
            }
            let sql = format!("SELECT view_name, text FROM all_views WHERE owner = {owner}{}", among("view_name"));
            for row in server.rows(&sql).await? {
                if let (Some(view), Some(query)) = (text(&row, 0), text(&row, 1)) {
                    details.definitions.insert(view, query.trim().to_owned());
                }
            }
            let sql = format!(
                "SELECT name, type, line, text FROM all_source WHERE owner = {owner} \
                 AND type IN ('PROCEDURE', 'FUNCTION', 'PACKAGE', 'PACKAGE BODY', 'TYPE', 'TYPE BODY'){} ORDER BY name, type, line",
                among("name")
            );
            let mut sources: BTreeMap<(String, String), String> = BTreeMap::new();
            for row in server.rows(&sql).await? {
                let (Some(name), Some(kind)) = (text(&row, 0), text(&row, 1)) else { continue };
                sources.entry((name, kind)).or_default().push_str(&text(&row, 3).unwrap_or_default());
            }
            let mut joined: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for ((name, _), source) in sources {
                joined.entry(name).or_default().push(format!("CREATE OR REPLACE {}", source.trim_end()));
            }
            for (name, parts) in joined {
                details.definitions.entry(name).or_insert_with(|| parts.join("\n/\n"));
            }
            let sql = format!(
                "SELECT sequence_name, min_value, max_value, increment_by, cycle_flag, cache_size, last_number \
                 FROM all_sequences WHERE sequence_owner = {owner}{}",
                among("sequence_name")
            );
            for row in server.rows(&sql).await? {
                let Some(name) = text(&row, 0) else { continue };
                details.sequences.insert(
                    name,
                    json!({
                        "dataType": "NUMBER", "start": text(&row, 6).unwrap_or_default(),
                        "increment": text(&row, 3).unwrap_or_default(), "min": text(&row, 1).unwrap_or_default(),
                        "max": text(&row, 2).unwrap_or_default(), "cache": text(&row, 5).unwrap_or_default(),
                        "cycle": text(&row, 4).as_deref() == Some("Y"),
                    }),
                );
            }
        }
        Ok(details)
    }

    fn object(&self, found: &Found, detailed: bool) -> Option<Json> {
        let kind = kind_of(&found.object_type)?;
        let mut object = json!({
            "kind": kind, "name": found.name, "oid": u32::try_from(found.id).unwrap_or(0), "detailed": detailed,
        });
        if let Some(comment) = self.comments.get(&found.name) {
            object["comment"] = json!(comment);
        }
        if !detailed {
            return Some(object);
        }
        if let Some(rows) = self.rows.get(&found.name) {
            object["rows"] = json!(rows);
        }
        if matches!(kind, "table" | "view" | "materializedView") {
            object["columns"] = json!(self.columns.get(&found.name).cloned().unwrap_or_default());
            object["constraints"] = json!(self.constraints.get(&found.name).cloned().unwrap_or_default());
            object["indexes"] = json!(self.indexes.get(&found.name).cloned().unwrap_or_default());
        }
        let definition = self.definitions.get(&found.name).cloned();
        match kind {
            "procedure" | "function" => {
                object["routine"] =
                    json!({ "arguments": "", "language": "plsql", "kind": kind, "definition": definition });
            }
            "sequence" => {
                if let Some(sequence) = self.sequences.get(&found.name) {
                    object["sequence"] = sequence.clone();
                }
            }
            _ => {
                if let Some(definition) = definition {
                    object["definition"] = json!(definition);
                }
            }
        }
        Some(object)
    }
}

pub(crate) async fn introspect(server: &mut Server, params: &Json) -> Answer {
    let wanted = Wanted::of(params);
    let names = server
        .rows("SELECT NVL(SYS_CONTEXT('USERENV', 'CON_NAME'), SYS_CONTEXT('USERENV', 'DB_NAME')), SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM dual")
        .await?;
    let catalog = names.first().and_then(|row| text(row, 0)).unwrap_or_else(|| "ORACLE".to_owned());
    let current_schema = names.first().and_then(|row| text(row, 1)).unwrap_or_default();
    let users = server.rows("SELECT username, oracle_maintained FROM all_users ORDER BY username").await?;
    let all: Vec<(String, bool)> = users
        .iter()
        .filter_map(|row| {
            Some((text(row, 0)?, text(row, 1).as_deref() == Some("Y") && text(row, 0)? != current_schema))
        })
        .collect();
    let mut answer = json!({ "name": catalog, "current": true, "schemasRead": true, "schemas": [] });
    if wanted.outline {
        let schemas: Vec<Json> = all
            .iter()
            .filter(|(_, system)| wanted.system || !system)
            .map(|(name, system)| json!({ "name": name, "system": system }))
            .collect();
        answer["schemas"] = json!(schemas);
        return Ok(json!({ "catalogs": [answer], "read": [], "unchanged": [] }));
    }
    let chosen: Vec<(String, bool)> = all
        .into_iter()
        .filter(
            |(name, system)| {
                if wanted.schemas.is_empty() { wanted.system || !system } else { wanted.schemas.contains(name) }
            },
        )
        .collect();
    let mut fingerprints: HashMap<String, String> = HashMap::new();
    for chunk in chosen.chunks(CHUNK) {
        let owners: Vec<String> = chunk.iter().map(|(name, _)| name.clone()).collect();
        let sql = format!(
            "SELECT owner, COUNT(*), TO_CHAR(MAX(last_ddl_time), 'YYYYMMDDHH24MISS') FROM all_objects \
             WHERE owner IN ({}) GROUP BY owner",
            list(&owners)
        );
        for row in server.rows(&sql).await? {
            if let Some(owner) = text(&row, 0) {
                fingerprints
                    .insert(owner, format!("{}-{}", int(&row, 1).unwrap_or(0), text(&row, 2).unwrap_or_default()));
            }
        }
    }
    let mut unchanged = Vec::new();
    let mut read = Vec::new();
    let mut schemas = Vec::new();
    for (name, system) in &chosen {
        let fingerprint = fingerprints.get(name).cloned().unwrap_or_else(|| "0-".to_owned());
        let key = format!("{catalog}.{name}");
        let asked = wanted.objects.iter().any(|(schema, _)| schema == name);
        if !wanted.force && !asked && wanted.fingerprints.get(&key) == Some(&fingerprint) {
            unchanged.push(key);
            continue;
        }
        let sql = format!(
            "SELECT object_id, object_name, object_type FROM all_objects WHERE owner = {} AND object_type IN {KINDS} \
             AND generated = 'N' AND object_name NOT LIKE 'BIN$%' ORDER BY object_name",
            literal(name)
        );
        let mut found: Vec<Found> = server
            .rows(&sql)
            .await?
            .iter()
            .filter_map(|row| Some(Found { id: int(row, 0)?, name: text(row, 1)?, object_type: text(row, 2)? }))
            .collect();
        let views: Vec<String> = found
            .iter()
            .filter(|object| object.object_type == "MATERIALIZED VIEW")
            .map(|object| object.name.clone())
            .collect();
        found.retain(|object| object.object_type != "TABLE" || !views.contains(&object.name));
        let all_details = found.len() <= wanted.large;
        let detailed: Vec<String> = if all_details {
            found.iter().map(|object| object.name.clone()).collect()
        } else {
            found
                .iter()
                .filter(|object| wanted.objects.iter().any(|(schema, table)| schema == name && *table == object.name))
                .map(|object| object.name.clone())
                .collect()
        };
        let details = if detailed.is_empty() {
            Details::default()
        } else {
            Details::read(server, name, if all_details { None } else { Some(&detailed) }).await?
        };
        let objects: Vec<Json> =
            found.iter().filter_map(|object| details.object(object, detailed.contains(&object.name))).collect();
        schemas.push(json!({
            "name": name, "system": system, "fingerprint": fingerprint,
            "level": if all_details { "details" } else { "names" }, "objects": objects,
        }));
        read.push(key);
    }
    answer["schemas"] = json!(schemas);
    Ok(json!({ "catalogs": [answer], "read": read, "unchanged": unchanged }))
}

fn identifier(chars: &[char], at: &mut usize) -> Option<String> {
    while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
        *at += 1;
    }
    let c = *chars.get(*at)?;
    if c == '"' {
        let start = *at + 1;
        let end = start + chars[start..].iter().position(|c| *c == '"')?;
        *at = end + 1;
        return Some(chars[start..end].iter().collect());
    }
    if !c.is_alphabetic() {
        return None;
    }
    let start = *at;
    while chars.get(*at).is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '#')) {
        *at += 1;
    }
    Some(chars[start..*at].iter().collect::<String>().to_uppercase())
}

fn top_level_from(chars: &[char]) -> Option<usize> {
    let mut depth = 0_i32;
    let mut at = 0;
    while at < chars.len() {
        match chars[at] {
            '\'' | '"' => {
                let quote = chars[at];
                at += 1;
                while at < chars.len() && chars[at] != quote {
                    at += 1;
                }
            }
            '-' if chars.get(at + 1) == Some(&'-') => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            '/' if chars.get(at + 1) == Some(&'*') => {
                at += 2;
                while at + 1 < chars.len() && !(chars[at] == '*' && chars[at + 1] == '/') {
                    at += 1;
                }
                at += 1;
            }
            '(' => depth += 1,
            ')' => depth -= 1,
            c if depth == 0 && c.is_alphabetic() => {
                let start = at;
                while at < chars.len() && (chars[at].is_alphanumeric() || chars[at] == '_') {
                    at += 1;
                }
                let before = start == 0 || !chars[start - 1].is_alphanumeric();
                let word: String = chars[start..at].iter().collect();
                if before && word.eq_ignore_ascii_case("from") {
                    return Some(at);
                }
                continue;
            }
            _ => {}
        }
        at += 1;
    }
    None
}

pub(crate) fn plain_table(text: &str) -> Option<(Option<String>, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    if !identifier(&chars, &mut at)?.eq_ignore_ascii_case("SELECT") {
        return None;
    }
    let mut at = top_level_from(&chars)?;
    let first = identifier(&chars, &mut at)?;
    let (schema, table) = if chars.get(at) == Some(&'.') {
        at += 1;
        (Some(first), identifier(&chars, &mut at)?)
    } else {
        (None, first)
    };
    let rest: String = chars[at..].iter().collect();
    let rest = rest.trim_start();
    if rest.starts_with('@') {
        return None;
    }
    let mut words = rest.split_whitespace().map(str::to_ascii_uppercase);
    let ends = |word: Option<&str>| matches!(word, None | Some("WHERE" | "ORDER" | "FETCH" | "OFFSET" | "FOR"));
    let next = words.next();
    if !ends(next.as_deref()) {
        let alias = next?;
        if !alias.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '#' | '"'))
            || matches!(
                alias.as_str(),
                "JOIN"
                    | "INNER"
                    | "LEFT"
                    | "RIGHT"
                    | "FULL"
                    | "CROSS"
                    | "NATURAL"
                    | "GROUP"
                    | "CONNECT"
                    | "START"
                    | "UNION"
                    | "MINUS"
                    | "INTERSECT"
                    | "PARTITION"
                    | "SAMPLE"
                    | "AS"
            )
        {
            return None;
        }
        if !ends(words.next().as_deref()) {
            return None;
        }
    }
    let upper = rest.to_ascii_uppercase();
    if ["GROUP BY", " UNION ", " MINUS ", " INTERSECT ", "CONNECT BY"].iter().any(|word| upper.contains(word)) {
        return None;
    }
    Some((schema, table))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_queries_name_their_table() {
        assert_eq!(plain_table("SELECT * FROM hr.employees"), Some((Some("HR".into()), "EMPLOYEES".into())));
        assert_eq!(
            plain_table("select id, name from \"Mixed\" t where id > 1 order by name, id"),
            Some((None, "Mixed".into()))
        );
        assert_eq!(
            plain_table("SELECT (SELECT 1 FROM dual) x FROM people FETCH FIRST 10 ROWS ONLY"),
            Some((None, "PEOPLE".into()))
        );
        assert_eq!(plain_table("SELECT * FROM a JOIN b ON a.id = b.id"), None);
        assert_eq!(plain_table("SELECT * FROM a, b"), None);
        assert_eq!(plain_table("SELECT * FROM (SELECT 1 FROM dual)"), None);
        assert_eq!(plain_table("SELECT * FROM t@remote"), None);
        assert_eq!(plain_table("SELECT kind, COUNT(*) FROM t GROUP BY kind"), None);
        assert_eq!(plain_table("WITH a AS (SELECT 1 FROM dual) SELECT * FROM a"), None);
    }

    #[test]
    fn column_types_are_written_as_oracle_writes_them() {
        let row = |values: Json| {
            values.as_array().unwrap().iter().map(|value| json!({ "kind": "x", "value": value })).collect::<Vec<_>>()
        };
        assert_eq!(type_text(&row(json!(["VARCHAR2", 160, 40, "C", null, null])), 0), "VARCHAR2(40 CHAR)");
        assert_eq!(type_text(&row(json!(["VARCHAR2", 40, 40, "B", null, null])), 0), "VARCHAR2(40)");
        assert_eq!(type_text(&row(json!(["NUMBER", 22, 0, null, 10, 2])), 0), "NUMBER(10,2)");
        assert_eq!(type_text(&row(json!(["NUMBER", 22, 0, null, null, 0])), 0), "INTEGER");
        assert_eq!(type_text(&row(json!(["DATE", 7, 0, null, null, null])), 0), "DATE");
        assert!(not_null_check("\"ID\" IS NOT NULL"));
        assert!(!not_null_check("price > 0"));
    }
}

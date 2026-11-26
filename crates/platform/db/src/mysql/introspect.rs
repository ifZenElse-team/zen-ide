use std::collections::{BTreeMap, HashMap, HashSet};

use mysql_async::Conn;

use super::{Dialect, quote_ident, sets};
use crate::driver::DbError;
use crate::schema::{
    Catalog, Column, Constraint, ConstraintKind, Index, IntrospectRequest, Introspection, Level, Object, ObjectKind,
    Reference, Routine, Schema, Snapshot, Trigger,
};

pub const CATALOG: &str = "def";

const SYSTEM: [&str; 4] = ["information_schema", "mysql", "performance_schema", "sys"];

const SHOW_CREATE_BATCH: usize = 50;

type Row = Vec<Option<String>>;
type Rows = Vec<Row>;
type Key = (String, String);

fn cell(row: &[Option<String>], index: usize) -> Option<String> {
    row.get(index).cloned().flatten()
}

fn text(row: &[Option<String>], index: usize) -> String {
    cell(row, index).unwrap_or_default()
}

fn filled(row: &[Option<String>], index: usize) -> Option<String> {
    cell(row, index).filter(|value| !value.is_empty())
}

fn number<T: std::str::FromStr>(row: &[Option<String>], index: usize) -> Option<T> {
    cell(row, index).and_then(|value| value.parse().ok())
}

fn system_schema(name: &str) -> bool {
    SYSTEM.iter().any(|system| system.eq_ignore_ascii_case(name))
}

fn by_table(rows: Rows) -> HashMap<Key, Rows> {
    let mut grouped: HashMap<Key, Rows> = HashMap::new();
    for row in rows {
        grouped.entry((text(&row, 0), text(&row, 1))).or_default().push(row);
    }
    grouped
}

fn fingerprints_sql(dialect: Dialect, schemas: &[String]) -> Vec<String> {
    let list = dialect.list(schemas);
    let sum = |schema: &str, fields: &str, table: &str| {
        format!(
            "SELECT {schema}, COUNT(*), COALESCE(SUM(CRC32(CONCAT_WS('|', {fields}))), 0) FROM information_schema.{table} \
             WHERE {schema} IN ({list}) GROUP BY {schema}"
        )
    };
    vec![
        sum(
            "TABLE_SCHEMA",
            "TABLE_NAME, TABLE_TYPE, ENGINE, CREATE_TIME, TABLE_COLLATION, CREATE_OPTIONS, TABLE_COMMENT",
            "TABLES",
        ),
        sum(
            "TABLE_SCHEMA",
            "TABLE_NAME, COLUMN_NAME, ORDINAL_POSITION, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA, COLLATION_NAME, COLUMN_COMMENT, GENERATION_EXPRESSION",
            "COLUMNS",
        ),
        sum(
            "TABLE_SCHEMA",
            "TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX, COLUMN_NAME, NON_UNIQUE, INDEX_TYPE, SUB_PART, INDEX_COMMENT",
            "STATISTICS",
        ),
        sum(
            "TABLE_SCHEMA",
            "TABLE_NAME, CONSTRAINT_NAME, COLUMN_NAME, ORDINAL_POSITION, REFERENCED_TABLE_SCHEMA, REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME",
            "KEY_COLUMN_USAGE",
        ),
        sum("CONSTRAINT_SCHEMA", "TABLE_NAME, CONSTRAINT_NAME, UPDATE_RULE, DELETE_RULE", "REFERENTIAL_CONSTRAINTS"),
        sum("CONSTRAINT_SCHEMA", "CONSTRAINT_NAME, CHECK_CLAUSE", "CHECK_CONSTRAINTS"),
        sum("TABLE_SCHEMA", "TABLE_NAME, VIEW_DEFINITION, CHECK_OPTION, SECURITY_TYPE", "VIEWS"),
        sum("ROUTINE_SCHEMA", "ROUTINE_NAME, ROUTINE_TYPE, CREATED, LAST_ALTERED, ROUTINE_COMMENT", "ROUTINES"),
        sum(
            "TRIGGER_SCHEMA",
            "TRIGGER_NAME, EVENT_OBJECT_TABLE, ACTION_TIMING, EVENT_MANIPULATION, ACTION_ORDER, CREATED, ACTION_STATEMENT",
            "TRIGGERS",
        ),
        sum("EVENT_SCHEMA", "EVENT_NAME, STATUS, LAST_ALTERED, EVENT_DEFINITION", "EVENTS"),
    ]
}

async fn fingerprints(
    conn: &mut Conn,
    dialect: Dialect,
    schemas: &[String],
) -> Result<HashMap<String, (String, usize)>, DbError> {
    if schemas.is_empty() {
        return Ok(HashMap::new());
    }
    let read = sets(conn, &fingerprints_sql(dialect, schemas)).await?;
    let counted = [0, 7, 9];
    let mut parts: HashMap<String, (Vec<String>, usize)> =
        schemas.iter().map(|schema| (schema.clone(), (vec!["0:0".to_owned(); read.len()], 0))).collect();
    for (kind, rows) in read.iter().enumerate() {
        for row in rows {
            let Some((prints, count)) = parts.get_mut(&text(row, 0)) else { continue };
            prints[kind] = format!("{}:{}", text(row, 1), text(row, 2));
            if counted.contains(&kind) {
                *count += number::<usize>(row, 1).unwrap_or(0);
            }
        }
    }
    Ok(parts.into_iter().map(|(schema, (prints, count))| (schema, (prints.join("/"), count))).collect())
}

fn table_filter(dialect: Dialect, schema_column: &str, table_column: &str, tables: Option<&[Key]>) -> String {
    match tables {
        None => String::new(),
        Some(tables) => {
            let pairs: Vec<String> = tables
                .iter()
                .map(|(schema, table)| format!("({}, {})", dialect.literal(schema), dialect.literal(table)))
                .collect();
            format!(" AND ({schema_column}, {table_column}) IN ({})", pairs.join(", "))
        }
    }
}

struct Details {
    columns: HashMap<Key, Rows>,
    indexes: HashMap<Key, Rows>,
    constraints: HashMap<Key, Rows>,
    key_columns: HashMap<Key, Rows>,
    rules: HashMap<(String, String, String), Row>,
    checks: HashMap<(String, String), Rows>,
    triggers: HashMap<Key, Rows>,
}

impl Details {
    async fn read(
        conn: &mut Conn,
        dialect: Dialect,
        schemas: &[String],
        tables: Option<&[Key]>,
    ) -> Result<Self, DbError> {
        let list = dialect.list(schemas);
        let filter = |schema: &str, table: &str| table_filter(dialect, schema, table, tables);
        let expression = if dialect.mariadb { "NULL" } else { "EXPRESSION" };
        let check_table = if dialect.mariadb { "TABLE_NAME" } else { "NULL" };
        let statements = vec![
            format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION, COLUMN_NAME, COLUMN_TYPE, IS_NULLABLE, COLUMN_DEFAULT, EXTRA, \
                 COLLATION_NAME, COLUMN_COMMENT, GENERATION_EXPRESSION, DATA_TYPE FROM information_schema.COLUMNS \
                 WHERE TABLE_SCHEMA IN ({list}){} ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION",
                filter("TABLE_SCHEMA", "TABLE_NAME")
            ),
            format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, NON_UNIQUE, SEQ_IN_INDEX, COLUMN_NAME, SUB_PART, INDEX_TYPE, \
                 INDEX_COMMENT, {expression} FROM information_schema.STATISTICS WHERE TABLE_SCHEMA IN ({list}){} \
                 ORDER BY TABLE_SCHEMA, TABLE_NAME, INDEX_NAME, SEQ_IN_INDEX",
                filter("TABLE_SCHEMA", "TABLE_NAME")
            ),
            format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, CONSTRAINT_TYPE FROM information_schema.TABLE_CONSTRAINTS \
                 WHERE TABLE_SCHEMA IN ({list}){} ORDER BY TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME",
                filter("TABLE_SCHEMA", "TABLE_NAME")
            ),
            format!(
                "SELECT TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, COLUMN_NAME, ORDINAL_POSITION, REFERENCED_TABLE_SCHEMA, \
                 REFERENCED_TABLE_NAME, REFERENCED_COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE \
                 WHERE TABLE_SCHEMA IN ({list}){} ORDER BY TABLE_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, ORDINAL_POSITION",
                filter("TABLE_SCHEMA", "TABLE_NAME")
            ),
            format!(
                "SELECT CONSTRAINT_SCHEMA, TABLE_NAME, CONSTRAINT_NAME, UPDATE_RULE, DELETE_RULE \
                 FROM information_schema.REFERENTIAL_CONSTRAINTS WHERE CONSTRAINT_SCHEMA IN ({list}){}",
                filter("CONSTRAINT_SCHEMA", "TABLE_NAME")
            ),
            format!(
                "SELECT CONSTRAINT_SCHEMA, {check_table}, CONSTRAINT_NAME, CHECK_CLAUSE FROM information_schema.CHECK_CONSTRAINTS \
                 WHERE CONSTRAINT_SCHEMA IN ({list})"
            ),
            format!(
                "SELECT TRIGGER_SCHEMA, EVENT_OBJECT_TABLE, TRIGGER_NAME, ACTION_TIMING, EVENT_MANIPULATION, ACTION_STATEMENT \
                 FROM information_schema.TRIGGERS WHERE TRIGGER_SCHEMA IN ({list}){} \
                 ORDER BY TRIGGER_SCHEMA, EVENT_OBJECT_TABLE, ACTION_TIMING, EVENT_MANIPULATION, ACTION_ORDER",
                filter("TRIGGER_SCHEMA", "EVENT_OBJECT_TABLE")
            ),
        ];
        let mut read = sets(conn, &statements).await?.into_iter();
        let mut next = || read.next().unwrap_or_default();
        let (columns, indexes, constraints, key_columns, rules, checks, triggers) =
            (next(), next(), next(), next(), next(), next(), next());
        let mut grouped_checks: HashMap<(String, String), Rows> = HashMap::new();
        for row in checks {
            grouped_checks.entry((text(&row, 0), text(&row, 2))).or_default().push(row);
        }
        Ok(Self {
            columns: by_table(columns),
            indexes: by_table(indexes),
            constraints: by_table(constraints),
            key_columns: by_table(key_columns),
            rules: rules.into_iter().map(|row| ((text(&row, 0), text(&row, 1), text(&row, 2)), row)).collect(),
            checks: grouped_checks,
            triggers: by_table(triggers),
        })
    }

    fn fill(&mut self, dialect: Dialect, schema: &str, object: &mut Object, collation: Option<&str>) {
        let key = (schema.to_owned(), object.name.clone());
        object.columns =
            self.columns.remove(&key).unwrap_or_default().iter().map(|row| column(dialect, row, collation)).collect();
        if object.kind == ObjectKind::Table {
            object.constraints = self.constraints(schema, &object.name);
            object.indexes = self.indexes(schema, &object.name, &object.constraints);
        }
        object.triggers = self
            .triggers
            .remove(&key)
            .unwrap_or_default()
            .iter()
            .map(|row| {
                let (timing, event) = (text(row, 3), text(row, 4));
                Trigger {
                    name: text(row, 2),
                    definition: format!(
                        "CREATE TRIGGER {} {timing} {event} ON {} FOR EACH ROW {}",
                        quote_ident(&text(row, 2)),
                        quote_ident(&object.name),
                        text(row, 5)
                    ),
                    enabled: true,
                    function: format!("{timing} {event}"),
                }
            })
            .collect();
        object.detailed = true;
    }

    fn constraints(&mut self, schema: &str, table: &str) -> Vec<Constraint> {
        let key = (schema.to_owned(), table.to_owned());
        let mut columns: HashMap<String, Rows> = HashMap::new();
        for row in self.key_columns.remove(&key).unwrap_or_default() {
            columns.entry(text(&row, 2)).or_default().push(row);
        }
        let names = |rows: &Rows, index: usize| rows.iter().map(|row| text(row, index)).collect::<Vec<_>>();
        self.constraints
            .remove(&key)
            .unwrap_or_default()
            .iter()
            .filter_map(|row| {
                let name = text(row, 2);
                let used = columns.get(&name).cloned().unwrap_or_default();
                let listed = names(&used, 3);
                let joined = listed.iter().map(|column| quote_ident(column)).collect::<Vec<_>>().join(", ");
                let (kind, definition, references, index) = match text(row, 3).as_str() {
                    "PRIMARY KEY" => {
                        (ConstraintKind::PrimaryKey, format!("PRIMARY KEY ({joined})"), None, Some(name.clone()))
                    }
                    "UNIQUE" => (ConstraintKind::Unique, format!("UNIQUE ({joined})"), None, Some(name.clone())),
                    "FOREIGN KEY" => {
                        let rule = self.rules.get(&(schema.to_owned(), table.to_owned(), name.clone()));
                        let first = used.first().cloned().unwrap_or_default();
                        let reference = Reference {
                            schema: text(&first, 5),
                            table: text(&first, 6),
                            columns: names(&used, 7),
                            on_update: rule.map(|rule| text(rule, 3).to_ascii_lowercase()).unwrap_or_default(),
                            on_delete: rule.map(|rule| text(rule, 4).to_ascii_lowercase()).unwrap_or_default(),
                        };
                        let target = reference.columns.iter().map(|column| quote_ident(column)).collect::<Vec<_>>();
                        let definition = format!(
                            "FOREIGN KEY ({joined}) REFERENCES {}.{} ({}) ON DELETE {} ON UPDATE {}",
                            quote_ident(&reference.schema),
                            quote_ident(&reference.table),
                            target.join(", "),
                            reference.on_delete.to_ascii_uppercase(),
                            reference.on_update.to_ascii_uppercase()
                        );
                        (ConstraintKind::ForeignKey, definition, Some(reference), None)
                    }
                    "CHECK" => {
                        let clause = self
                            .checks
                            .get(&(schema.to_owned(), name.clone()))
                            .and_then(|rows| {
                                rows.iter().find(|check| cell(check, 1).is_none_or(|owner| owner == table)).cloned()
                            })
                            .map(|check| text(&check, 3))
                            .unwrap_or_default();
                        (ConstraintKind::Check, format!("CHECK ({clause})"), None, None)
                    }
                    _ => return None,
                };
                Some(Constraint { name, kind, columns: listed, definition, references, index, comment: None })
            })
            .collect()
    }

    fn indexes(&mut self, schema: &str, table: &str, constraints: &[Constraint]) -> Vec<Index> {
        let mut grouped: BTreeMap<String, Rows> = BTreeMap::new();
        for row in self.indexes.remove(&(schema.to_owned(), table.to_owned())).unwrap_or_default() {
            grouped.entry(text(&row, 2)).or_default().push(row);
        }
        grouped
            .into_iter()
            .map(|(name, rows)| {
                let first = rows.first().cloned().unwrap_or_default();
                let unique = text(&first, 3) == "0";
                let method = text(&first, 7).to_ascii_lowercase();
                let columns: Vec<String> = rows
                    .iter()
                    .map(|row| match (cell(row, 5), cell(row, 6), filled(row, 9)) {
                        (Some(column), Some(part), _) => format!("{}({part})", quote_ident(&column)),
                        (Some(column), None, _) => quote_ident(&column),
                        (None, _, Some(expression)) => format!("({expression})"),
                        (None, _, None) => String::new(),
                    })
                    .collect();
                let primary = name == "PRIMARY";
                let target = format!("{}.{}", quote_ident(schema), quote_ident(table));
                let definition = if primary {
                    format!("ALTER TABLE {target} ADD PRIMARY KEY ({})", columns.join(", "))
                } else {
                    let prefix = match method.as_str() {
                        "fulltext" => "FULLTEXT ",
                        "spatial" => "SPATIAL ",
                        _ if unique => "UNIQUE ",
                        _ => "",
                    };
                    format!("CREATE {prefix}INDEX {} ON {target} ({})", quote_ident(&name), columns.join(", "))
                };
                let constraint = constraints
                    .iter()
                    .find(|constraint| {
                        constraint.name == name
                            && matches!(constraint.kind, ConstraintKind::PrimaryKey | ConstraintKind::Unique)
                    })
                    .map(|constraint| constraint.name.clone());
                Index {
                    columns: rows
                        .iter()
                        .map(|row| cell(row, 5).or_else(|| filled(row, 9)).unwrap_or_default())
                        .collect(),
                    unique,
                    primary,
                    method,
                    definition,
                    constraint,
                    size: None,
                    comment: filled(&first, 8),
                    name,
                }
            })
            .collect()
    }
}

fn numeric(data_type: &str) -> bool {
    matches!(
        data_type,
        "tinyint"
            | "smallint"
            | "mediumint"
            | "int"
            | "integer"
            | "bigint"
            | "decimal"
            | "numeric"
            | "float"
            | "double"
            | "real"
            | "bit"
            | "year"
    )
}

fn column(dialect: Dialect, row: &[Option<String>], collation: Option<&str>) -> Column {
    let extra = text(row, 7).to_ascii_lowercase();
    let raw = cell(row, 6);
    let default =
        if dialect.mariadb {
            raw.filter(|value| value != "NULL")
        } else if extra.contains("default_generated") {
            raw.map(|value| {
                if value.to_ascii_uppercase().starts_with("CURRENT_TIMESTAMP") { value } else { format!("({value})") }
            })
        } else if numeric(&text(row, 11).to_ascii_lowercase()) {
            raw
        } else {
            raw.map(|value| dialect.literal(&value))
        };
    let generated = filled(row, 10).map(|expression| {
        let stored = if extra.contains("stored") || extra.contains("persistent") { "STORED" } else { "VIRTUAL" };
        format!("({expression}) {stored}")
    });
    Column {
        position: number(row, 2).unwrap_or_default(),
        name: text(row, 3),
        type_name: text(row, 4),
        nullable: text(row, 5) == "YES",
        default: if generated.is_some() { None } else { default },
        identity: extra.contains("auto_increment").then(|| "byDefault".to_owned()),
        generated,
        collation: cell(row, 8).filter(|own| Some(own.as_str()) != collation),
        comment: filled(row, 9),
    }
}

fn relation(row: &[Option<String>]) -> Option<Object> {
    let kind = match text(row, 2).as_str() {
        "BASE TABLE" | "SYSTEM VERSIONED" => ObjectKind::Table,
        "VIEW" | "SYSTEM VIEW" => ObjectKind::View,
        "SEQUENCE" => ObjectKind::Sequence,
        _ => return None,
    };
    let mut object = Object::new(kind, text(row, 1));
    if kind == ObjectKind::Table {
        object.rows = number(row, 4);
        object.size = number(row, 5);
    }
    object.comment = filled(row, 6).filter(|comment| kind != ObjectKind::View || comment != "VIEW");
    object.system = system_schema(&text(row, 0)) || text(row, 2) == "SYSTEM VIEW";
    Some(object)
}

fn routine(row: &[Option<String>], parameters: &HashMap<Key, Rows>) -> Object {
    let function = text(row, 2) == "FUNCTION";
    let kind = if function { ObjectKind::Function } else { ObjectKind::Procedure };
    let mut object = Object::new(kind, text(row, 1));
    object.owner = filled(row, 5);
    object.comment = filled(row, 4);
    object.system = system_schema(&text(row, 0));
    let arguments = parameters
        .get(&(text(row, 0), text(row, 1)))
        .into_iter()
        .flatten()
        .filter(|parameter| text(parameter, 6) == text(row, 2))
        .map(|parameter| {
            let name = format!("{} {}", text(parameter, 4), text(parameter, 5));
            match filled(parameter, 3) {
                Some(mode) if !function => format!("{mode} {name}"),
                _ => name,
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    object.routine = Some(Routine {
        arguments,
        result: filled(row, 3).filter(|_| function),
        language: "SQL".to_owned(),
        kind: if function { "function" } else { "procedure" }.to_owned(),
        definition: None,
    });
    object
}

fn event(row: &[Option<String>]) -> Object {
    let mut object = Object::new(ObjectKind::Event, text(row, 1));
    object.comment = filled(row, 2);
    object.owner = filled(row, 3);
    object.system = system_schema(&text(row, 0));
    object
}

fn show_create(schema: &str, object: &Object) -> Option<(String, usize)> {
    let name = format!("{}.{}", quote_ident(schema), quote_ident(&object.name));
    Some(match object.kind {
        ObjectKind::Table => (format!("SHOW CREATE TABLE {name}"), 1),
        ObjectKind::View => (format!("SHOW CREATE VIEW {name}"), 1),
        ObjectKind::Sequence => (format!("SHOW CREATE SEQUENCE {name}"), 1),
        ObjectKind::Procedure => (format!("SHOW CREATE PROCEDURE {name}"), 2),
        ObjectKind::Function => (format!("SHOW CREATE FUNCTION {name}"), 2),
        ObjectKind::Event => (format!("SHOW CREATE EVENT {name}"), 3),
        _ => return None,
    })
}

async fn create_statements(
    conn: &mut Conn,
    wanted: &[(String, &Object)],
) -> HashMap<(String, ObjectKind, String), String> {
    let mut found = HashMap::new();
    let queries: Vec<((String, ObjectKind, String), String, usize)> = wanted
        .iter()
        .filter(|(schema, _)| !system_schema(schema))
        .filter_map(|(schema, object)| {
            show_create(schema, object).map(|(sql, at)| ((schema.clone(), object.kind, object.name.clone()), sql, at))
        })
        .collect();
    for batch in queries.chunks(SHOW_CREATE_BATCH) {
        let statements: Vec<String> = batch.iter().map(|(_, sql, _)| sql.clone()).collect();
        let read = match sets(conn, &statements).await {
            Ok(read) => read.into_iter().map(Some).collect(),
            Err(_) => {
                let mut one_by_one = Vec::with_capacity(batch.len());
                for statement in &statements {
                    one_by_one
                        .push(sets(conn, std::slice::from_ref(statement)).await.ok().and_then(|mut read| read.pop()));
                }
                one_by_one
            }
        };
        for ((key, _, at), rows) in batch.iter().zip(read) {
            if let Some(statement) = rows.and_then(|rows| rows.into_iter().next()).and_then(|row| cell(&row, *at)) {
                found.insert(key.clone(), statement);
            }
        }
    }
    found
}

async fn read_schemas(
    conn: &mut Conn,
    dialect: Dialect,
    changed: &[String],
    detailed: &[String],
    names: &BTreeMap<String, bool>,
) -> Result<Vec<Schema>, DbError> {
    let list = dialect.list(changed);
    let statements = vec![
        format!(
            "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE, ENGINE, TABLE_ROWS, DATA_LENGTH + INDEX_LENGTH, TABLE_COMMENT, \
             TABLE_COLLATION FROM information_schema.TABLES WHERE TABLE_SCHEMA IN ({list}) ORDER BY TABLE_SCHEMA, TABLE_NAME"
        ),
        format!(
            "SELECT ROUTINE_SCHEMA, ROUTINE_NAME, ROUTINE_TYPE, DTD_IDENTIFIER, ROUTINE_COMMENT, DEFINER \
             FROM information_schema.ROUTINES WHERE ROUTINE_SCHEMA IN ({list}) ORDER BY ROUTINE_SCHEMA, ROUTINE_NAME"
        ),
        format!(
            "SELECT SPECIFIC_SCHEMA, SPECIFIC_NAME, ORDINAL_POSITION, PARAMETER_MODE, PARAMETER_NAME, DTD_IDENTIFIER, ROUTINE_TYPE \
             FROM information_schema.PARAMETERS WHERE SPECIFIC_SCHEMA IN ({list}) AND ORDINAL_POSITION > 0 \
             ORDER BY SPECIFIC_SCHEMA, SPECIFIC_NAME, ORDINAL_POSITION"
        ),
        format!(
            "SELECT EVENT_SCHEMA, EVENT_NAME, EVENT_COMMENT, DEFINER FROM information_schema.EVENTS \
             WHERE EVENT_SCHEMA IN ({list}) ORDER BY EVENT_SCHEMA, EVENT_NAME"
        ),
    ];
    let mut read = sets(conn, &statements).await?.into_iter();
    let mut next = || read.next().unwrap_or_default();
    let (tables, routines, parameters, events) = (next(), next(), next(), next());
    let parameters = by_table(parameters);
    let detailed: HashSet<&String> = detailed.iter().collect();
    let mut details = if detailed.is_empty() {
        None
    } else {
        let schemas: Vec<String> = detailed.iter().map(|schema| (*schema).clone()).collect();
        Some(Details::read(conn, dialect, &schemas, None).await?)
    };
    let mut objects: BTreeMap<String, Vec<Object>> = changed.iter().map(|name| (name.clone(), Vec::new())).collect();
    for row in &tables {
        let schema = text(row, 0);
        let Some(mut object) = relation(row) else { continue };
        if let Some(details) = details.as_mut().filter(|_| detailed.contains(&schema)) {
            details.fill(dialect, &schema, &mut object, cell(row, 7).as_deref());
        }
        objects.entry(schema).or_default().push(object);
    }
    for row in &routines {
        objects.entry(text(row, 0)).or_default().push(routine(row, &parameters));
    }
    for row in &events {
        objects.entry(text(row, 0)).or_default().push(event(row));
    }
    let wanted: Vec<(String, &Object)> = objects
        .iter()
        .filter(|(schema, _)| detailed.contains(schema))
        .flat_map(|(schema, objects)| objects.iter().map(move |object| (schema.clone(), object)))
        .collect();
    let mut created = create_statements(conn, &wanted).await;
    Ok(objects
        .into_iter()
        .map(|(name, mut objects)| {
            let is_detailed = detailed.contains(&name);
            for object in &mut objects {
                if is_detailed {
                    attach(&name, object, &mut created);
                }
            }
            objects.sort_by_key(|object| (object.kind, object.key()));
            Schema {
                level: if is_detailed { Level::Details } else { Level::Names },
                system: names.get(&name).copied().unwrap_or(false),
                name,
                owner: None,
                comment: None,
                fingerprint: None,
                objects,
            }
        })
        .collect())
}

fn attach(schema: &str, object: &mut Object, created: &mut HashMap<(String, ObjectKind, String), String>) {
    let statement = created.remove(&(schema.to_owned(), object.kind, object.name.clone()));
    match object.routine.as_mut() {
        Some(routine) => routine.definition = statement,
        None => object.definition = statement,
    }
    if matches!(object.kind, ObjectKind::Function | ObjectKind::Procedure | ObjectKind::Event | ObjectKind::Sequence) {
        object.detailed = true;
    }
}

async fn read_objects(
    conn: &mut Conn,
    dialect: Dialect,
    catalog: &mut Catalog,
    wanted: &[(String, String)],
) -> Result<(), DbError> {
    let mut tables: Vec<Key> = Vec::new();
    let mut schemas: Vec<String> = Vec::new();
    for (schema, key) in wanted {
        let Some(found) = catalog.schemas.iter().find(|known| known.name == *schema) else { continue };
        for object in found.objects.iter().filter(|object| object.key() == *key || object.name == *key) {
            tables.push((schema.clone(), object.name.clone()));
            if !schemas.contains(schema) {
                schemas.push(schema.clone());
            }
        }
    }
    if tables.is_empty() {
        return Ok(());
    }
    let collations: HashMap<Key, Option<String>> = sets(
        conn,
        &[format!(
            "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_COLLATION FROM information_schema.TABLES WHERE TABLE_SCHEMA IN ({}){}",
            dialect.list(&schemas),
            table_filter(dialect, "TABLE_SCHEMA", "TABLE_NAME", Some(&tables))
        )],
    )
    .await?
    .into_iter()
    .flatten()
    .map(|row| ((text(&row, 0), text(&row, 1)), cell(&row, 2)))
    .collect();
    let mut details = Details::read(conn, dialect, &schemas, Some(&tables)).await?;
    let picked: Vec<(String, Object)> = catalog
        .schemas
        .iter()
        .flat_map(|schema| schema.objects.iter().map(move |object| (schema.name.clone(), object)))
        .filter(|(schema, object)| tables.contains(&(schema.clone(), object.name.clone())))
        .map(|(schema, object)| (schema, object.clone()))
        .collect();
    let refs: Vec<(String, &Object)> = picked.iter().map(|(schema, object)| (schema.clone(), object)).collect();
    let mut created = create_statements(conn, &refs).await;
    for schema in &mut catalog.schemas {
        let name = schema.name.clone();
        for object in &mut schema.objects {
            let key = (name.clone(), object.name.clone());
            if !tables.contains(&key) {
                continue;
            }
            if object.kind.has_columns() {
                let collation = collations.get(&key).cloned().flatten();
                details.fill(dialect, &name, object, collation.as_deref());
            }
            attach(&name, object, &mut created);
        }
    }
    Ok(())
}

pub(super) async fn introspect(
    conn: &mut Conn,
    dialect: Dialect,
    request: &IntrospectRequest,
    previous: Option<&Snapshot>,
    fingerprinted: bool,
) -> Result<Introspection, DbError> {
    let listed =
        sets(conn, &["SELECT SCHEMA_NAME FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME".to_owned()]).await?;
    let names: BTreeMap<String, bool> = listed
        .into_iter()
        .flatten()
        .map(|row| {
            let name = text(&row, 0);
            let system = system_schema(&name);
            (name, system)
        })
        .collect();
    let before = previous.and_then(|snapshot| snapshot.catalog(CATALOG));
    let earlier = |name: &str| before.and_then(|catalog| catalog.schemas.iter().find(|schema| schema.name == name));
    let mut catalog = Catalog {
        name: CATALOG.to_owned(),
        current: true,
        schemas_read: true,
        schemas: names
            .iter()
            .map(|(name, system)| {
                let mut schema = earlier(name).cloned().unwrap_or_default();
                schema.name.clone_from(name);
                schema.system = *system;
                schema
            })
            .collect(),
        ..Catalog::default()
    };
    let mut read = Vec::new();
    let mut unchanged = 0;
    if request.outline {
    } else if !request.objects.is_empty() && request.schemas.is_empty() {
        read_objects(conn, dialect, &mut catalog, &request.objects).await?;
    } else {
        let candidates: Vec<String> = names
            .iter()
            .filter(|(name, system)| {
                if request.schemas.is_empty() {
                    !**system || request.system || earlier(name).is_some_and(|schema| schema.level != Level::NotRead)
                } else {
                    request.schemas.contains(name)
                }
            })
            .map(|(name, _)| name.clone())
            .collect();
        let prints = if fingerprinted { fingerprints(conn, dialect, &candidates).await? } else { HashMap::new() };
        let changed: Vec<String> = candidates
            .iter()
            .filter(|name| {
                request.force
                    || !fingerprinted
                    || earlier(name).is_none_or(|schema| {
                        schema.level == Level::NotRead
                            || schema.fingerprint.as_ref() != prints.get(*name).map(|(print, _)| print)
                    })
            })
            .cloned()
            .collect();
        unchanged = candidates.len() - changed.len();
        let large = request.large_schema.max(1);
        let detailed: Vec<String> =
            changed.iter().filter(|name| prints.get(*name).is_none_or(|(_, count)| *count <= large)).cloned().collect();
        if !changed.is_empty() {
            for mut schema in read_schemas(conn, dialect, &changed, &detailed, &names).await? {
                schema.fingerprint = prints.get(&schema.name).map(|(print, _)| print.clone());
                read.push(format!("{CATALOG}.{}", schema.name));
                if let Some(slot) = catalog.schemas.iter_mut().find(|known| known.name == schema.name) {
                    *slot = schema;
                }
            }
        }
        if !request.objects.is_empty() {
            read_objects(conn, dialect, &mut catalog, &request.objects).await?;
        }
    }
    Ok(Introspection { catalogs: vec![catalog], read, unchanged })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cells: &[Option<&str>]) -> Row {
        cells.iter().map(|cell| cell.map(str::to_owned)).collect()
    }

    #[test]
    fn information_schema_is_joined_here_and_never_on_the_server() {
        let code = include_str!("introspect.rs").split("mod tests").next().unwrap().to_uppercase();
        assert!(!code.contains(" JOIN "));
    }

    #[test]
    fn fingerprints_never_concatenate_groups() {
        let dialect = Dialect { mariadb: false, backslashes: true };
        let statements = fingerprints_sql(dialect, &["shop".to_owned()]);
        assert_eq!(statements.len(), 10);
        assert!(statements.iter().all(|sql| sql.contains("CRC32") && !sql.contains("GROUP_CONCAT")));
        assert!(statements[0].contains("WHERE TABLE_SCHEMA IN ('shop')"));
    }

    #[test]
    fn columns_keep_their_defaults_as_sql() {
        let mysql = Dialect { mariadb: false, backslashes: true };
        let text_default = row(&[
            Some("s"),
            Some("t"),
            Some("2"),
            Some("name"),
            Some("varchar(10)"),
            Some("NO"),
            Some("it's"),
            Some(""),
            Some("utf8mb4_bin"),
            Some("who"),
            Some(""),
            Some("varchar"),
        ]);
        let column = column(mysql, &text_default, Some("utf8mb4_0900_ai_ci"));
        assert_eq!(column.default.as_deref(), Some("'it''s'"));
        assert_eq!(column.collation.as_deref(), Some("utf8mb4_bin"));
        assert_eq!((column.nullable, column.comment.as_deref()), (false, Some("who")));
        let generated = row(&[
            Some("s"),
            Some("t"),
            Some("3"),
            Some("total"),
            Some("int"),
            Some("YES"),
            None,
            Some("STORED GENERATED"),
            None,
            Some(""),
            Some("`a` * 2"),
            Some("int"),
        ]);
        let generated = super::column(mysql, &generated, None);
        assert_eq!(generated.generated.as_deref(), Some("(`a` * 2) STORED"));
        let mariadb = Dialect { mariadb: true, backslashes: true };
        let quoted = row(&[
            Some("s"),
            Some("t"),
            Some("1"),
            Some("id"),
            Some("int(11)"),
            Some("YES"),
            Some("NULL"),
            Some("auto_increment"),
            None,
            None,
            None,
            Some("int"),
        ]);
        let quoted = super::column(mariadb, &quoted, None);
        assert_eq!((quoted.default, quoted.identity.as_deref()), (None, Some("byDefault")));
    }
}

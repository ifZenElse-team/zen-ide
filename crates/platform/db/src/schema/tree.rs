use super::{CachedNode, Catalog, ConstraintKind, Level, Object, ObjectKind, Schema, Snapshot};
use crate::model::{Node, NodeKind};
use crate::registry::ObjectFilter;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeOptions {
    pub show_system: bool,
    pub simple: bool,
    pub filters: Vec<ObjectFilter>,
}

const SUBFOLDERS: [&str; 8] =
    ["columns", "constraints", "foreignKeys", "indexes", "triggers", "references", "policies", "partitions"];

fn matches(mask: &str, name: &str) -> bool {
    fn walk(mask: &[char], name: &[char]) -> bool {
        match mask.split_first() {
            None => name.is_empty(),
            Some(('*', rest)) => (0..=name.len()).any(|skip| walk(rest, &name[skip..])),
            Some(('?', rest)) => !name.is_empty() && walk(rest, &name[1..]),
            Some((c, rest)) => name.first().is_some_and(|n| n.eq_ignore_ascii_case(c)) && walk(rest, &name[1..]),
        }
    }
    let mask: Vec<char> = mask.chars().collect();
    let name: Vec<char> = name.chars().collect();
    walk(&mask, &name)
}

impl TreeOptions {
    fn shows(&self, kind: &str, name: &str, system: bool) -> bool {
        if system && !self.show_system {
            return false;
        }
        self.filters.iter().filter(|filter| filter.kind == kind).all(|filter| {
            (filter.include.is_empty() || filter.include.iter().any(|mask| matches(mask, name)))
                && !filter.exclude.iter().any(|mask| matches(mask, name))
        })
    }
}

fn hidden(snapshot: &Snapshot) -> &'static [&'static str] {
    snapshot.profile.as_deref().and_then(crate::profile::get).map_or(&[], |profile| profile.quirks.hidden)
}

fn kind_name(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Table => "table",
        ObjectKind::ForeignTable => "foreignTable",
        ObjectKind::View => "view",
        ObjectKind::MaterializedView => "materializedView",
        ObjectKind::Function => "function",
        ObjectKind::Procedure => "procedure",
        ObjectKind::Sequence => "sequence",
        ObjectKind::Type => "type",
        ObjectKind::Event => "event",
        ObjectKind::Package => "package",
    }
}

fn folder_label(id: &str) -> &'static str {
    match id {
        "tables" => "Tables",
        "foreignTables" => "Foreign Tables",
        "views" => "Views",
        "materializedViews" => "Materialized Views",
        "functions" => "Functions",
        "procedures" => "Procedures",
        "sequences" => "Sequences",
        "types" => "Types",
        "events" => "Events",
        "packages" => "Packages",
        "columns" => "Columns",
        "constraints" => "Constraints",
        "foreignKeys" => "Foreign Keys",
        "indexes" => "Indexes",
        "triggers" => "Triggers",
        "references" => "References",
        "policies" => "Policies",
        "partitions" => "Partitions",
        "aggregates" => "Aggregate Functions",
        EXTENSIONS => "Extensions",
        EVENT_TRIGGERS => "Event Triggers",
        TABLESPACES => "Tablespaces",
        ROLES => "Roles",
        _ => "",
    }
}

fn reserved(word: &str) -> bool {
    const RESERVED: [&str; 40] = [
        "all",
        "and",
        "any",
        "as",
        "asc",
        "both",
        "case",
        "check",
        "column",
        "constraint",
        "create",
        "default",
        "desc",
        "distinct",
        "do",
        "else",
        "end",
        "except",
        "false",
        "for",
        "foreign",
        "from",
        "grant",
        "group",
        "having",
        "in",
        "into",
        "is",
        "limit",
        "not",
        "null",
        "on",
        "or",
        "order",
        "primary",
        "references",
        "select",
        "table",
        "user",
        "where",
    ];
    RESERVED.contains(&word)
}

pub fn quote(name: &str) -> String {
    let plain = name.chars().next().is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$');
    if plain && !reserved(name) { name.to_owned() } else { format!("\"{}\"", name.replace('"', "\"\"")) }
}

pub fn qualified(schema: &str, name: &str) -> String {
    format!("{}.{}", quote(schema), quote(name))
}

fn backticks(driver: &str) -> bool {
    matches!(driver, "mysql" | "mariadb")
}

pub fn quote_for(driver: &str, name: &str) -> String {
    if driver == "sqlserver" {
        return format!("[{}]", name.replace(']', "]]"));
    }
    if backticks(driver) {
        return format!("`{}`", name.replace('`', "``"));
    }
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn quote_when_needed(driver: &str, name: &str) -> String {
    if driver == "sqlserver" {
        return format!("[{}]", name.replace(']', "]]"));
    }
    if driver == "oracle" {
        let plain = name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
            && name.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '_' | '$' | '#'));
        return if plain && !reserved(&name.to_ascii_lowercase()) {
            name.to_owned()
        } else {
            format!("\"{}\"", name.replace('"', "\"\""))
        };
    }
    if !backticks(driver) {
        return quote(name);
    }
    let plain = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$');
    if plain && !reserved(&name.to_ascii_lowercase()) {
        name.to_owned()
    } else {
        format!("`{}`", name.replace('`', "``"))
    }
}

pub fn qualified_for(driver: &str, schema: &str, name: &str) -> String {
    format!("{}.{}", quote_for(driver, schema), quote_for(driver, name))
}

#[derive(Clone, Copy, Debug)]
pub struct Found<'a> {
    pub catalog: &'a Catalog,
    pub schema: &'a Schema,
    pub object: &'a Object,
    pub sub: Option<&'a str>,
    pub item: Option<&'a str>,
}

pub fn find_object<'a>(snapshot: &'a Snapshot, path: &'a [String]) -> Option<Found<'a>> {
    let [catalog, schema, folder, key, rest @ ..] = path else { return None };
    let kind = if folder == AGGREGATES { ObjectKind::Function } else { ObjectKind::from_folder(folder)? };
    let catalog = snapshot.catalog(catalog)?;
    let schema = catalog.schemas.iter().find(|known| known.name == *schema)?;
    let object = schema.objects.iter().find(|object| object.kind == kind && object.key() == *key)?;
    Some(Found {
        catalog,
        schema,
        object,
        sub: rest.first().map(String::as_str),
        item: rest.get(1).map(String::as_str),
    })
}

pub fn references(catalog: &Catalog, schema: &str, table: &str) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for other in &catalog.schemas {
        for object in &other.objects {
            for constraint in &object.constraints {
                if let Some(reference) = &constraint.references
                    && reference.schema == schema
                    && reference.table == table
                {
                    found.push((other.name.clone(), object.name.clone(), constraint.name.clone()));
                }
            }
        }
    }
    found
}

fn path_of(base: &[String], segment: &str) -> Vec<String> {
    let mut path = base.to_vec();
    path.push(segment.to_owned());
    path
}

pub const EXTENSIONS: &str = "#extensions";
pub const EVENT_TRIGGERS: &str = "#eventTriggers";
pub const TABLESPACES: &str = "#tablespaces";
pub const ROLES: &str = "#roles";
pub const AGGREGATES: &str = "aggregates";
pub const SCHEMA_INDEXES: &str = "indexes";

fn aggregate(object: &Object) -> bool {
    object.routine.as_ref().is_some_and(|routine| routine.kind == "aggregate")
}

fn postgres(snapshot: &Snapshot) -> bool {
    snapshot.driver == "postgresql"
}

fn database_items(catalog: &Catalog, base: &[String], folder: &str, options: &TreeOptions) -> Option<Vec<Node>> {
    let leaf = |name: &str, kind: &str, detail: Option<String>| {
        Node::new(path_of(base, name), NodeKind::Other(kind.to_owned()), name, false).with_detail(detail)
    };
    Some(match folder {
        EXTENSIONS => catalog
            .extensions
            .iter()
            .map(|extension| {
                let mut node = leaf(&extension.name, "extension", Some(extension.version.clone()));
                node.comment.clone_from(&extension.comment);
                node
            })
            .collect(),
        EVENT_TRIGGERS => catalog
            .event_triggers
            .iter()
            .map(|trigger| leaf(&trigger.name, "eventTrigger", Some(trigger.event.clone())))
            .collect(),
        TABLESPACES => catalog
            .tablespaces
            .iter()
            .map(|tablespace| {
                let mut node = leaf(&tablespace.name, "tablespace", None);
                node.size = tablespace.size;
                node
            })
            .collect(),
        ROLES => catalog
            .roles
            .iter()
            .filter(|role| !role.system || options.show_system)
            .map(|role| {
                let mut node = leaf(&role.name, "role", None);
                node.system = role.system;
                node
            })
            .collect(),
        _ => return None,
    })
}

fn folder(base: &[String], id: &str) -> Node {
    Node::new(path_of(base, id), NodeKind::Folder, folder_label(id), true)
}

fn cached(node: &CachedNode) -> Node {
    let mut shown = Node::new(node.path.clone(), node.kind.clone(), node.label.clone(), node.has_children)
        .with_detail(node.detail.clone());
    shown.system = node.system;
    shown.size = node.size;
    shown.rows = node.rows;
    shown.comment.clone_from(&node.comment);
    shown
}

pub fn children(snapshot: &Snapshot, path: &[String], options: &TreeOptions) -> Option<Vec<Node>> {
    if snapshot.catalogs.is_empty() {
        return snapshot
            .cached(path)
            .map(|nodes| nodes.iter().filter(|node| !node.system || options.show_system).map(cached).collect());
    }
    match path {
        [] => Some(
            snapshot
                .catalogs
                .iter()
                .filter(|catalog| options.shows("catalog", &catalog.name, catalog.system))
                .map(|catalog| {
                    let mut node = Node::new(path_of(path, &catalog.name), NodeKind::Catalog, &catalog.name, true);
                    node.comment.clone_from(&catalog.comment);
                    node.system = catalog.system;
                    node
                })
                .collect(),
        ),
        [catalog] => {
            let catalog = snapshot.catalog(catalog)?;
            if !catalog.schemas_read {
                return None;
            }
            let mut nodes: Vec<Node> = catalog
                .schemas
                .iter()
                .filter(|schema| options.shows("schema", &schema.name, schema.system))
                .map(|schema| {
                    let mut node = Node::new(path_of(path, &schema.name), NodeKind::Schema, &schema.name, true);
                    node.comment.clone_from(&schema.comment);
                    node.system = schema.system;
                    node
                })
                .collect();
            if postgres(snapshot) && catalog.current && !options.simple {
                let hidden = hidden(snapshot);
                nodes.extend(
                    [EXTENSIONS, EVENT_TRIGGERS, TABLESPACES, ROLES]
                        .into_iter()
                        .filter(|id| !hidden.contains(id))
                        .map(|id| folder(path, id)),
                );
            }
            Some(nodes)
        }
        [catalog, folder, ..] if folder.starts_with('#') => {
            let catalog = snapshot.catalog(catalog)?;
            match path.len() {
                2 => database_items(catalog, path, folder, options),
                _ => Some(Vec::new()),
            }
        }
        [catalog, schema] => {
            let schema = snapshot.schema(catalog, schema)?;
            if schema.level == Level::NotRead {
                return None;
            }
            let shown: &[ObjectKind] = if options.simple {
                &[ObjectKind::Table, ObjectKind::View, ObjectKind::MaterializedView]
            } else {
                &ObjectKind::ALL
            };
            let always: &[ObjectKind] = match snapshot.driver.as_str() {
                "postgresql" => &[ObjectKind::Table, ObjectKind::View, ObjectKind::Function, ObjectKind::Sequence],
                "mysql" | "mariadb" => &[
                    ObjectKind::Table,
                    ObjectKind::View,
                    ObjectKind::Function,
                    ObjectKind::Procedure,
                    ObjectKind::Event,
                ],
                _ => &[ObjectKind::Table, ObjectKind::View],
            };
            let hidden = hidden(snapshot);
            let listed = |kind: &ObjectKind| {
                postgres(snapshot)
                    && !options.simple
                    && matches!(kind, ObjectKind::ForeignTable | ObjectKind::MaterializedView)
            };
            let mut ids: Vec<&str> = shown
                .iter()
                .filter(|kind| {
                    always.contains(kind) || listed(kind) || schema.objects.iter().any(|object| object.kind == **kind)
                })
                .map(|kind| kind.folder())
                .collect();
            if postgres(snapshot) && !options.simple {
                let after =
                    ids.iter().position(|id| *id == ObjectKind::MaterializedView.folder()).map_or(0, |at| at + 1);
                ids.insert(after, SCHEMA_INDEXES);
                let last = ids.iter().position(|id| *id == ObjectKind::Event.folder()).unwrap_or(ids.len());
                ids.insert(last, AGGREGATES);
            }
            Some(ids.into_iter().filter(|id| !hidden.contains(id)).map(|id| folder(path, id)).collect())
        }
        [catalog, schema, kind] if postgres(snapshot) && (kind == SCHEMA_INDEXES || kind == AGGREGATES) => {
            let schema = snapshot.schema(catalog, schema)?;
            if schema.level == Level::NotRead {
                return None;
            }
            let nodes = if kind == AGGREGATES {
                schema
                    .objects
                    .iter()
                    .filter(|object| aggregate(object) && options.shows("function", &object.name, object.system))
                    .map(|object| {
                        let mut node = Node::new(
                            path_of(path, &object.key()),
                            ObjectKind::Function.node_kind(),
                            object.key(),
                            false,
                        );
                        node.system = object.system;
                        node.comment.clone_from(&object.comment);
                        node.detail = object.routine.as_ref().and_then(|routine| routine.result.clone());
                        node
                    })
                    .collect()
            } else {
                let mut indexes: Vec<Node> = schema
                    .objects
                    .iter()
                    .filter(|object| options.shows(kind_name(object.kind), &object.name, object.system))
                    .flat_map(|object| object.indexes.iter().map(move |index| (object, index)))
                    .map(|(object, index)| {
                        let mut node = Node::new(path_of(path, &index.name), NodeKind::Index, &index.name, false)
                            .with_detail(Some(object.name.clone()));
                        node.size = index.size;
                        node.system = object.system;
                        node
                    })
                    .collect();
                indexes.sort_by(|a, b| a.label.cmp(&b.label));
                indexes
            };
            Some(nodes)
        }
        [catalog, schema, kind] => {
            let schema = snapshot.schema(catalog, schema)?;
            let kind = ObjectKind::from_folder(kind)?;
            if schema.level == Level::NotRead {
                return None;
            }
            Some(
                schema
                    .objects
                    .iter()
                    .filter(|object| object.kind == kind && options.shows(kind_name(kind), &object.name, object.system))
                    .filter(|object| !(postgres(snapshot) && aggregate(object)))
                    .map(|object| {
                        let mut node =
                            Node::new(path_of(path, &object.key()), kind.node_kind(), object.key(), kind.has_columns());
                        node.system = object.system;
                        node.size = object.size;
                        node.rows = object.rows;
                        node.comment.clone_from(&object.comment);
                        node.detail = match (&object.routine, &object.partitioning) {
                            (Some(routine), _) => routine.result.clone(),
                            (None, Some(partitioning)) if partitioning.parent.is_some() => partitioning.bound.clone(),
                            _ => None,
                        };
                        node
                    })
                    .collect(),
            )
        }
        [_, _, _, _] => {
            let found = find_object(snapshot, path)?;
            let object = found.object;
            if !object.detailed {
                return None;
            }
            let hidden = hidden(snapshot);
            let subfolders: Vec<&str> = SUBFOLDERS
                .iter()
                .copied()
                .filter(|id| !hidden.contains(id))
                .filter(|id| match *id {
                    "columns" => object.kind.has_columns(),
                    _ if options.simple => false,
                    "constraints" => object.kind == ObjectKind::Table,
                    "foreignKeys" | "references" => object.kind == ObjectKind::Table,
                    "indexes" => matches!(object.kind, ObjectKind::Table | ObjectKind::MaterializedView),
                    "triggers" => {
                        object.kind == ObjectKind::Table
                            || (object.kind == ObjectKind::View && !object.triggers.is_empty())
                    }
                    "policies" => {
                        !object.policies.is_empty()
                            || object.row_security
                            || (postgres(snapshot) && object.kind == ObjectKind::Table)
                    }
                    "partitions" => {
                        object.partitioning.as_ref().is_some_and(|partitioning| partitioning.key.is_some())
                            || (postgres(snapshot) && object.kind == ObjectKind::Table)
                    }
                    _ => false,
                })
                .collect();
            Some(subfolders.into_iter().map(|id| folder(path, id)).collect())
        }
        [_, _, _, _, sub] => {
            let found = find_object(snapshot, path)?;
            let object = found.object;
            if !object.detailed {
                return None;
            }
            let leaf = |label: &str, kind: NodeKind, detail: Option<String>| {
                Node::new(path_of(path, label), kind, label, false).with_detail(detail)
            };
            let nodes = match sub.as_str() {
                "columns" => object
                    .columns
                    .iter()
                    .map(|column| {
                        let mut node = leaf(&column.name, NodeKind::Column, Some(column.type_name.clone()));
                        node.comment.clone_from(&column.comment);
                        node
                    })
                    .collect(),
                "constraints" => object
                    .constraints
                    .iter()
                    .filter(|constraint| constraint.kind != ConstraintKind::ForeignKey)
                    .map(|constraint| {
                        leaf(
                            &constraint.name,
                            NodeKind::Other("constraint".to_owned()),
                            Some(constraint.definition.clone()),
                        )
                    })
                    .collect(),
                "foreignKeys" => object
                    .constraints
                    .iter()
                    .filter(|constraint| constraint.kind == ConstraintKind::ForeignKey)
                    .map(|constraint| {
                        let target = constraint
                            .references
                            .as_ref()
                            .map(|reference| format!("{}.{}", reference.schema, reference.table));
                        leaf(&constraint.name, NodeKind::Other("foreignKey".to_owned()), target)
                    })
                    .collect(),
                "indexes" => object
                    .indexes
                    .iter()
                    .map(|index| {
                        let mut node = leaf(&index.name, NodeKind::Index, Some(index.columns.join(", ")));
                        node.size = index.size;
                        node
                    })
                    .collect(),
                "triggers" => object
                    .triggers
                    .iter()
                    .map(|trigger| {
                        leaf(&trigger.name, NodeKind::Other("trigger".to_owned()), Some(trigger.function.clone()))
                    })
                    .collect(),
                "policies" => object
                    .policies
                    .iter()
                    .map(|policy| {
                        leaf(&policy.name, NodeKind::Other("policy".to_owned()), Some(policy.command.clone()))
                    })
                    .collect(),
                "references" => references(found.catalog, &found.schema.name, &object.name)
                    .into_iter()
                    .map(|(schema, table, constraint)| {
                        let label = format!("{schema}.{table}.{constraint}");
                        let mut node = leaf(&label, NodeKind::Other("reference".to_owned()), None);
                        node.label = format!("{table} ({constraint})");
                        node
                    })
                    .collect(),
                "partitions" => {
                    let parent = qualified(&found.schema.name, &object.name);
                    found
                        .catalog
                        .schemas
                        .iter()
                        .flat_map(|schema| schema.objects.iter().map(move |child| (schema, child)))
                        .filter(|(_, child)| {
                            child.partitioning.as_ref().and_then(|partitioning| partitioning.parent.as_deref())
                                == Some(parent.as_str())
                        })
                        .map(|(_, child)| {
                            let bound = child.partitioning.as_ref().and_then(|partitioning| partitioning.bound.clone());
                            let mut node = leaf(&child.name, NodeKind::Table, bound);
                            node.size = child.size;
                            node.rows = child.rows;
                            node
                        })
                        .collect()
                }
                _ => return None,
            };
            Some(nodes)
        }
        _ => Some(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Column, Constraint, Reference};

    fn snapshot() -> Snapshot {
        let mut customers = Object::new(ObjectKind::Table, "customers");
        customers.detailed = true;
        customers.columns =
            vec![Column { name: "id".into(), position: 1, type_name: "integer".into(), ..Column::default() }];
        let mut orders = Object::new(ObjectKind::Table, "orders");
        orders.detailed = true;
        orders.size = Some(8192);
        orders.constraints = vec![Constraint {
            name: "orders_customer_fk".into(),
            kind: ConstraintKind::ForeignKey,
            columns: vec!["customer".into()],
            definition: "FOREIGN KEY (customer) REFERENCES customers(id)".into(),
            references: Some(Reference {
                schema: "public".into(),
                table: "customers".into(),
                columns: vec!["id".into()],
                ..Reference::default()
            }),
            index: None,
            comment: None,
        }];
        let mut audit = Object::new(ObjectKind::Table, "audit_log");
        audit.system = true;
        Snapshot {
            catalogs: vec![Catalog {
                name: "shop".into(),
                current: true,
                schemas_read: true,
                schemas: vec![
                    Schema {
                        name: "public".into(),
                        level: Level::Details,
                        objects: vec![customers, orders, audit],
                        ..Schema::default()
                    },
                    Schema { name: "pg_catalog".into(), system: true, ..Schema::default() },
                ],
                ..Catalog::default()
            }],
            ..Snapshot::new("pg", "postgresql")
        }
    }

    fn labels(nodes: Option<Vec<Node>>) -> Vec<String> {
        nodes.unwrap().into_iter().map(|node| node.label).collect()
    }

    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    #[test]
    fn the_tree_has_folders_per_kind_references_and_hides_system_objects_and_filtered_names() {
        let snapshot = snapshot();
        let options = TreeOptions::default();
        assert_eq!(labels(children(&snapshot, &[], &options)), ["shop"]);
        assert_eq!(
            labels(children(&snapshot, &path(&["shop"]), &options)),
            ["public", "Extensions", "Event Triggers", "Tablespaces", "Roles"]
        );
        assert_eq!(
            labels(children(&snapshot, &path(&["shop", "public"]), &options)),
            [
                "Tables",
                "Foreign Tables",
                "Views",
                "Materialized Views",
                "Indexes",
                "Functions",
                "Sequences",
                "Aggregate Functions"
            ]
        );
        let tables = path(&["shop", "public", "tables"]);
        assert_eq!(labels(children(&snapshot, &tables, &options)), ["customers", "orders"]);
        let shown = TreeOptions { show_system: true, ..TreeOptions::default() };
        assert_eq!(labels(children(&snapshot, &tables, &shown)), ["customers", "orders", "audit_log"]);
        let filtered = TreeOptions {
            filters: vec![ObjectFilter { kind: "table".into(), include: vec!["c*".into()], exclude: Vec::new() }],
            ..TreeOptions::default()
        };
        assert_eq!(labels(children(&snapshot, &tables, &filtered)), ["customers"]);
        let references = path(&["shop", "public", "tables", "customers", "references"]);
        assert_eq!(labels(children(&snapshot, &references, &options)), ["orders (orders_customer_fk)"]);
        let keys = path(&["shop", "public", "tables", "orders", "foreignKeys"]);
        let fk = children(&snapshot, &keys, &options).unwrap().remove(0);
        assert_eq!(fk.detail.as_deref(), Some("public.customers"));
        assert_eq!(children(&snapshot, &path(&["shop", "pg_catalog"]), &options), None, "not read yet");
        let simple = TreeOptions { simple: true, ..TreeOptions::default() };
        assert_eq!(labels(children(&snapshot, &path(&["shop", "public", "tables", "orders"]), &simple)), ["Columns"]);
    }

    #[test]
    fn generated_identifiers_are_always_quoted_and_completions_only_when_needed() {
        assert_eq!(qualified("public", "orders"), "public.orders", "the form PostgreSQL prints, for identities");
        assert_eq!(qualified_for("postgresql", "public", "orders"), "\"public\".\"orders\"");
        assert_eq!(quote_when_needed("postgresql", "orders"), "orders");
        assert_eq!(quote_when_needed("postgresql", "user"), "\"user\"");
        assert_eq!(quote_when_needed("mysql", "Total"), "Total");
        assert_eq!(quote_when_needed("oracle", "ORDERS"), "ORDERS");
        assert_eq!(qualified("Sales", "user"), "\"Sales\".\"user\"");
        assert_eq!(quote("we\"ird"), "\"we\"\"ird\"");
        assert_eq!(qualified_for("mysql", "Shop", "orders"), "`Shop`.`orders`");
        assert_eq!(qualified_for("mariadb", "my shop", "order"), "`my shop`.`order`");
        assert_eq!(quote_for("mysql", "we`ird"), "`we``ird`");
        assert_eq!(quote_for("postgresql", "Shop"), "\"Shop\"");
    }

    #[test]
    fn postgresql_databases_list_extensions_roles_and_schemas_list_indexes_and_aggregates() {
        use crate::schema::{EventTrigger, Extension, Index, Role, Routine, Tablespace};
        let mut snapshot = snapshot();
        let catalog = &mut snapshot.catalogs[0];
        catalog.extensions = vec![Extension { name: "pgcrypto".into(), version: "1.3".into(), ..Extension::default() }];
        catalog.event_triggers =
            vec![EventTrigger { name: "audit_ddl".into(), event: "ddl_command_end".into(), ..EventTrigger::default() }];
        catalog.tablespaces = vec![Tablespace { name: "pg_default".into(), size: Some(4096), ..Tablespace::default() }];
        catalog.roles = vec![
            Role { name: "app".into(), login: true, ..Role::default() },
            Role { name: "pg_monitor".into(), system: true, ..Role::default() },
        ];
        let schema = &mut catalog.schemas[0];
        schema.objects[1].indexes =
            vec![Index { name: "orders_pkey".into(), size: Some(16384), primary: true, ..Index::default() }];
        let mut total = Object::new(ObjectKind::Function, "total");
        total.routine = Some(Routine { arguments: "numeric".into(), kind: "aggregate".into(), ..Routine::default() });
        total.detailed = true;
        let mut plain = Object::new(ObjectKind::Function, "add_one");
        plain.routine = Some(Routine { arguments: "integer".into(), kind: "function".into(), ..Routine::default() });
        schema.objects.extend([total, plain]);
        let options = TreeOptions::default();
        let nodes = |parts: &[&str]| children(&snapshot, &path(parts), &options).unwrap();
        let extensions = nodes(&["shop", EXTENSIONS]);
        assert_eq!(
            (extensions[0].label.as_str(), extensions[0].detail.as_deref(), extensions[0].has_children),
            ("pgcrypto", Some("1.3"), false)
        );
        assert_eq!(extensions[0].path, path(&["shop", EXTENSIONS, "pgcrypto"]));
        assert_eq!(labels(Some(nodes(&["shop", EVENT_TRIGGERS]))), ["audit_ddl"]);
        assert_eq!(nodes(&["shop", TABLESPACES])[0].size, Some(4096));
        assert_eq!(labels(Some(nodes(&["shop", ROLES]))), ["app"], "the server's own roles are system objects");
        let shown = TreeOptions { show_system: true, ..TreeOptions::default() };
        let roles = children(&snapshot, &path(&["shop", ROLES]), &shown).unwrap();
        assert_eq!(labels(Some(roles)), ["app", "pg_monitor"]);
        assert_eq!(nodes(&["shop", ROLES, "app"]), Vec::new());
        let indexes = nodes(&["shop", "public", SCHEMA_INDEXES]);
        assert_eq!(
            (indexes[0].label.as_str(), indexes[0].detail.as_deref(), indexes[0].kind.clone()),
            ("orders_pkey", Some("orders"), NodeKind::Index)
        );
        assert_eq!(labels(Some(nodes(&["shop", "public", AGGREGATES]))), ["total(numeric)"]);
        assert_eq!(labels(Some(nodes(&["shop", "public", "functions"]))), ["add_one(integer)"]);
        let aggregate = path(&["shop", "public", AGGREGATES, "total(numeric)"]);
        assert_eq!(find_object(&snapshot, &aggregate).map(|found| found.object.name.as_str()), Some("total"));
        assert_eq!(
            labels(Some(nodes(&["shop", "public", "tables", "customers"]))),
            ["Columns", "Constraints", "Foreign Keys", "Indexes", "Triggers", "References", "Policies", "Partitions"],
            "a PostgreSQL table always has Policies and Partitions"
        );
        let simple = TreeOptions { simple: true, ..TreeOptions::default() };
        assert_eq!(labels(children(&snapshot, &path(&["shop"]), &simple)), ["public"]);
        snapshot.driver = "mysql".into();
        assert_eq!(labels(children(&snapshot, &path(&["shop"]), &options)), ["public"]);
    }
}

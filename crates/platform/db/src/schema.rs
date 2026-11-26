mod snapshot;
mod tree;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::{Node, NodeKind};

pub use snapshot::{SNAPSHOT_VERSION, read_snapshot, write_snapshot};
pub use tree::{
    AGGREGATES, EVENT_TRIGGERS, EXTENSIONS, Found, ROLES, SCHEMA_INDEXES, TABLESPACES, TreeOptions, children,
    find_object, qualified, qualified_for, quote, quote_for, quote_when_needed, references,
};

/// The kind of a schema object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ObjectKind {
    /// A table, partitioned or not.
    Table,
    /// A table of a foreign server.
    ForeignTable,
    /// A view.
    View,
    /// A materialised view.
    MaterializedView,
    /// A function, aggregate or window function.
    Function,
    /// A procedure.
    Procedure,
    /// A sequence.
    Sequence,
    /// A user-defined type: enum, composite, domain or range.
    Type,
    /// A scheduled event (MySQL and MariaDB).
    Event,
    /// A package of procedures, functions and types (Oracle).
    Package,
}

impl ObjectKind {
    pub const ALL: [ObjectKind; 10] = [
        ObjectKind::Table,
        ObjectKind::ForeignTable,
        ObjectKind::View,
        ObjectKind::MaterializedView,
        ObjectKind::Function,
        ObjectKind::Procedure,
        ObjectKind::Package,
        ObjectKind::Sequence,
        ObjectKind::Type,
        ObjectKind::Event,
    ];

    pub fn folder(self) -> &'static str {
        match self {
            ObjectKind::Table => "tables",
            ObjectKind::ForeignTable => "foreignTables",
            ObjectKind::View => "views",
            ObjectKind::MaterializedView => "materializedViews",
            ObjectKind::Function => "functions",
            ObjectKind::Procedure => "procedures",
            ObjectKind::Sequence => "sequences",
            ObjectKind::Type => "types",
            ObjectKind::Event => "events",
            ObjectKind::Package => "packages",
        }
    }

    pub fn from_folder(folder: &str) -> Option<ObjectKind> {
        Self::ALL.into_iter().find(|kind| kind.folder() == folder)
    }

    pub fn has_columns(self) -> bool {
        matches!(self, ObjectKind::Table | ObjectKind::ForeignTable | ObjectKind::View | ObjectKind::MaterializedView)
    }

    pub fn node_kind(self) -> NodeKind {
        match self {
            ObjectKind::Table => NodeKind::Table,
            ObjectKind::View => NodeKind::View,
            ObjectKind::ForeignTable => NodeKind::Other("foreignTable".to_owned()),
            ObjectKind::MaterializedView => NodeKind::Other("materializedView".to_owned()),
            ObjectKind::Function => NodeKind::Other("function".to_owned()),
            ObjectKind::Procedure => NodeKind::Other("procedure".to_owned()),
            ObjectKind::Sequence => NodeKind::Other("sequence".to_owned()),
            ObjectKind::Type => NodeKind::Other("type".to_owned()),
            ObjectKind::Event => NodeKind::Other("event".to_owned()),
            ObjectKind::Package => NodeKind::Other("package".to_owned()),
        }
    }
}

/// How much of a schema was read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Only its name is known.
    #[default]
    NotRead,
    /// Its objects' names and kinds; details are read per object on demand (a large schema).
    Names,
    /// Everything: columns, keys, indexes, triggers, policies, definitions.
    Details,
}

/// A column of a table, view or composite type.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Column {
    /// Its name.
    pub name: String,
    /// Its position, from 1.
    pub position: i32,
    /// Its type as the server writes it (`character varying(40)`).
    pub type_name: String,
    /// Whether it accepts nulls.
    pub nullable: bool,
    /// Its default expression.
    pub default: Option<String>,
    /// `always` or `byDefault` for an identity column.
    pub identity: Option<String>,
    /// The expression of a generated column.
    pub generated: Option<String>,
    /// Its collation when it differs from its type's.
    pub collation: Option<String>,
    /// Its comment.
    pub comment: Option<String>,
}

/// What a constraint enforces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ConstraintKind {
    /// A primary key.
    PrimaryKey,
    /// A unique key.
    Unique,
    /// A check.
    Check,
    /// An exclusion constraint.
    Exclusion,
    /// A foreign key.
    ForeignKey,
}

/// The table a foreign key points at.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Reference {
    /// The referenced table's schema.
    pub schema: String,
    /// The referenced table.
    pub table: String,
    /// The referenced columns, in order.
    pub columns: Vec<String>,
    /// What an update of the referenced key does (`no action`, `cascade`, …).
    pub on_update: String,
    /// What a delete of the referenced row does.
    pub on_delete: String,
}

/// A constraint of a table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Constraint {
    /// Its name.
    pub name: String,
    /// What it enforces.
    pub kind: ConstraintKind,
    /// The columns it covers, in order.
    #[serde(default)]
    pub columns: Vec<String>,
    /// Its definition as the server writes it (`PRIMARY KEY (id)`).
    #[serde(default)]
    pub definition: String,
    /// The referenced table of a foreign key.
    pub references: Option<Reference>,
    /// The index that enforces a key.
    pub index: Option<String>,
    /// Its comment.
    pub comment: Option<String>,
}

/// An index of a table or materialised view.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Index {
    /// Its name.
    pub name: String,
    /// Its key columns or expressions, in order.
    pub columns: Vec<String>,
    /// Whether it is unique.
    pub unique: bool,
    /// Whether it is the primary key's.
    pub primary: bool,
    /// Its access method (`btree`, `gin`).
    pub method: String,
    /// Its `CREATE INDEX` statement.
    pub definition: String,
    /// The constraint it enforces, if any.
    pub constraint: Option<String>,
    /// Its size on disk in bytes.
    pub size: Option<u64>,
    /// Its comment.
    pub comment: Option<String>,
}

/// A trigger of a table or view.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Trigger {
    /// Its name.
    pub name: String,
    /// Its `CREATE TRIGGER` statement.
    pub definition: String,
    /// Whether it fires.
    pub enabled: bool,
    /// The function it calls.
    pub function: String,
}

/// A row security policy of a table.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Policy {
    /// Its name.
    pub name: String,
    /// The command it applies to: `all`, `select`, `insert`, `update` or `delete`.
    pub command: String,
    /// Whether it is permissive (or restrictive).
    pub permissive: bool,
    /// The roles it applies to.
    pub roles: Vec<String>,
    /// Its `USING` expression.
    pub using: Option<String>,
    /// Its `WITH CHECK` expression.
    pub check: Option<String>,
}

/// How a table is partitioned, or which table it is a partition of.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Partitioning {
    /// The partition key of a partitioned table (`RANGE (created)`).
    pub key: Option<String>,
    /// The table this one is a partition of, qualified.
    pub parent: Option<String>,
    /// This partition's bound (`FOR VALUES FROM (…) TO (…)`).
    pub bound: Option<String>,
}

/// A function's or procedure's signature and source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Routine {
    /// Its arguments as they identify it (`a integer, b text`).
    pub arguments: String,
    /// Its result type.
    pub result: Option<String>,
    /// Its language.
    pub language: String,
    /// `function`, `procedure`, `aggregate` or `window`.
    pub kind: String,
    /// Its full `CREATE` statement, read with details.
    pub definition: Option<String>,
}

/// A sequence's settings.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SequenceInfo {
    /// Its data type.
    pub data_type: String,
    /// Its start value.
    pub start: String,
    /// Its increment.
    pub increment: String,
    /// Its minimum.
    pub min: String,
    /// Its maximum.
    pub max: String,
    /// How many values are cached.
    pub cache: String,
    /// Whether it cycles.
    pub cycle: bool,
    /// The column that owns it, qualified.
    pub owned_by: Option<String>,
}

/// A user-defined type's shape.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TypeInfo {
    /// `enum`, `composite`, `domain` or `range`.
    pub kind: String,
    /// An enum's labels, in order.
    pub labels: Vec<String>,
    /// A domain's base type or a range's subtype.
    pub base: Option<String>,
    /// Whether a domain is NOT NULL.
    pub not_null: bool,
    /// A domain's default.
    pub default: Option<String>,
    /// A domain's check constraints.
    pub checks: Vec<String>,
}

/// An object of a schema: a table, view, function, sequence or type, with what was read of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Object {
    /// What it is.
    pub kind: ObjectKind,
    /// Its name.
    pub name: String,
    /// The server's id for it.
    #[serde(default)]
    pub oid: u32,
    /// Its owner.
    pub owner: Option<String>,
    /// The tablespace it is stored in, when it is not the database's default.
    pub tablespace: Option<String>,
    /// Its comment.
    pub comment: Option<String>,
    /// Whether it belongs to the system or an extension.
    #[serde(default)]
    pub system: bool,
    /// Whether its details were read.
    #[serde(default)]
    pub detailed: bool,
    /// Its size on disk with indexes, in bytes.
    pub size: Option<u64>,
    /// The server's estimate of its rows.
    pub rows: Option<i64>,
    /// Its columns.
    #[serde(default)]
    pub columns: Vec<Column>,
    /// Its constraints.
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    /// Its indexes.
    #[serde(default)]
    pub indexes: Vec<Index>,
    /// Its triggers.
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    /// Its row security policies.
    #[serde(default)]
    pub policies: Vec<Policy>,
    /// Whether row security is on.
    #[serde(default)]
    pub row_security: bool,
    /// Its partitioning.
    pub partitioning: Option<Partitioning>,
    /// A view's query.
    pub definition: Option<String>,
    /// A function's or procedure's signature and source.
    pub routine: Option<Routine>,
    /// A sequence's settings.
    pub sequence: Option<SequenceInfo>,
    /// A type's shape.
    pub type_info: Option<TypeInfo>,
    /// Its privileges as the server lists them (`ann=arwd/owner`).
    #[serde(default)]
    pub acl: Vec<String>,
    /// Its rewrite rules (PostgreSQL), without a view's own.
    #[serde(default)]
    pub rules: Vec<Rule>,
    /// The objects it uses and those that use it.
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
}

/// A rewrite rule of a table or view.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Rule {
    /// Its name.
    pub name: String,
    /// The command it rewrites: `select`, `update`, `insert` or `delete`.
    pub event: String,
    /// Whether it runs instead of the command (or also).
    pub instead: bool,
    /// Its whole `CREATE RULE` statement.
    pub definition: String,
}

/// An object another one depends on, or that depends on it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Dependency {
    /// `uses` when the object holding it uses the named one, `usedBy` when the named one uses it.
    pub direction: String,
    /// Its schema.
    pub schema: String,
    /// Its name.
    pub name: String,
    /// What it is, as a navigator kind (`table`, `view`, `materializedView`, `function`, `sequence`, `type`).
    pub kind: String,
}

impl Object {
    pub fn new(kind: ObjectKind, name: impl Into<String>) -> Self {
        Self {
            kind,
            name: name.into(),
            oid: 0,
            owner: None,
            tablespace: None,
            comment: None,
            system: false,
            detailed: false,
            size: None,
            rows: None,
            columns: Vec::new(),
            constraints: Vec::new(),
            indexes: Vec::new(),
            triggers: Vec::new(),
            policies: Vec::new(),
            row_security: false,
            partitioning: None,
            definition: None,
            routine: None,
            sequence: None,
            type_info: None,
            acl: Vec::new(),
            rules: Vec::new(),
            dependencies: Vec::new(),
        }
    }

    pub fn key(&self) -> String {
        match &self.routine {
            Some(routine) => format!("{}({})", self.name, routine.arguments),
            None => self.name.clone(),
        }
    }
}

/// A schema and its objects.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Schema {
    /// Its name.
    pub name: String,
    /// Its owner.
    pub owner: Option<String>,
    /// Its comment.
    pub comment: Option<String>,
    /// Whether it is a system schema.
    pub system: bool,
    /// The change fingerprint it was read at; a different one means it changed.
    pub fingerprint: Option<String>,
    /// How much of it was read.
    pub level: Level,
    /// Its objects, by kind and name.
    pub objects: Vec<Object>,
}

/// A database (catalogue) of the server.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Catalog {
    /// Its name.
    pub name: String,
    /// Its comment.
    pub comment: Option<String>,
    /// Whether it is a template or system database.
    pub system: bool,
    /// Whether it is the one the connection's sessions are in.
    pub current: bool,
    /// Whether its schemas were listed.
    pub schemas_read: bool,
    /// Its schemas.
    pub schemas: Vec<Schema>,
    /// Its extensions (PostgreSQL), when it is the current database.
    pub extensions: Vec<Extension>,
    /// Its event triggers (PostgreSQL), when it is the current database.
    pub event_triggers: Vec<EventTrigger>,
    /// The server's roles, as read from this database.
    pub roles: Vec<Role>,
    /// The server's tablespaces, as read from this database.
    pub tablespaces: Vec<Tablespace>,
}

/// An extension installed in a database.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Extension {
    /// Its name.
    pub name: String,
    /// Its installed version.
    pub version: String,
    /// The schema its objects are in.
    pub schema: String,
    /// Whether its objects can move to another schema.
    pub relocatable: bool,
    /// Its comment.
    pub comment: Option<String>,
}

/// An event trigger of a database.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EventTrigger {
    /// Its name.
    pub name: String,
    /// The event it fires on: `ddl_command_start`, `ddl_command_end`, `sql_drop`, `table_rewrite` or `login`.
    pub event: String,
    /// The function it runs, qualified.
    pub function: String,
    /// When it fires: `origin` (`O`), `disabled` (`D`), `replica` (`R`) or `always` (`A`).
    pub enabled: String,
    /// Its owner.
    pub owner: Option<String>,
    /// The command tags it is limited to.
    pub tags: Vec<String>,
    /// Its comment.
    pub comment: Option<String>,
}

/// A role of the server.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Role {
    /// Its name.
    pub name: String,
    /// Whether it can log in.
    pub login: bool,
    /// Whether it is a superuser.
    pub superuser: bool,
    /// Whether it can create databases.
    pub create_database: bool,
    /// Whether it can create roles.
    pub create_role: bool,
    /// Whether it can start replication.
    pub replication: bool,
    /// Whether it bypasses row security.
    pub bypass_row_security: bool,
    /// How many connections it may have; -1 for no limit.
    pub connection_limit: i64,
    /// When its password stops working.
    pub valid_until: Option<String>,
    /// The roles it is a member of.
    pub member_of: Vec<String>,
    /// Whether it is one of the server's own roles (`pg_*`).
    pub system: bool,
    /// Its comment.
    pub comment: Option<String>,
}

/// A tablespace of the server.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Tablespace {
    /// Its name.
    pub name: String,
    /// Its owner.
    pub owner: Option<String>,
    /// Its directory; empty for the built-in ones.
    pub location: String,
    /// Its size in bytes, when the connection's role may read it.
    pub size: Option<u64>,
    /// Its options (`random_page_cost=1.1`).
    pub options: Vec<String>,
    /// Its comment.
    pub comment: Option<String>,
}

/// A node kept from a driver's own tree (key-value and document drivers), as `children` answered it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CachedNode {
    /// Its path.
    pub path: Vec<String>,
    /// What it is.
    pub kind: NodeKind,
    /// Its label.
    pub label: String,
    /// Whether it can have children.
    pub has_children: bool,
    /// Its detail.
    pub detail: Option<String>,
    /// Whether it belongs to the system, hidden unless system objects are shown.
    #[serde(default)]
    pub system: bool,
    /// Its size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// The server's estimate of its rows.
    #[serde(default)]
    pub rows: Option<i64>,
    /// Its comment.
    #[serde(default)]
    pub comment: Option<String>,
}

impl From<Node> for CachedNode {
    fn from(node: Node) -> Self {
        Self {
            path: node.path,
            kind: node.kind,
            label: node.label,
            has_children: node.has_children,
            detail: node.detail,
            system: node.system,
            size: node.size,
            rows: node.rows,
            comment: node.comment,
        }
    }
}

/// The children of one path of a driver's own tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CachedChildren {
    /// The parent's path.
    pub path: Vec<String>,
    /// Its children.
    pub nodes: Vec<CachedNode>,
}

/// Everything known about one connection's structure; kept on disk so the navigator and completion have it before connecting. Never holds data values or secrets.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// The connection's id.
    pub connection: String,
    /// The driver's id.
    pub driver: String,
    /// The server's name and version.
    pub server_version: Option<String>,
    /// The database profile in effect (ADR-0173): the connection's own, else the one its server turned out to be.
    pub profile: Option<String>,
    /// When it was last refreshed, in seconds since 1970.
    pub taken: u64,
    /// The databases of a relational driver.
    pub catalogs: Vec<Catalog>,
    /// The nodes of a driver that supplies its own tree.
    pub nodes: Vec<CachedChildren>,
}

impl Snapshot {
    pub fn new(connection: impl Into<String>, driver: impl Into<String>) -> Self {
        Self { connection: connection.into(), driver: driver.into(), ..Self::default() }
    }

    pub fn catalog(&self, name: &str) -> Option<&Catalog> {
        self.catalogs.iter().find(|catalog| catalog.name == name)
    }

    pub fn schema(&self, catalog: &str, schema: &str) -> Option<&Schema> {
        self.catalog(catalog)?.schemas.iter().find(|known| known.name == schema)
    }

    pub fn cached(&self, path: &[String]) -> Option<&[CachedNode]> {
        self.nodes.iter().find(|kept| kept.path == path).map(|kept| kept.nodes.as_slice())
    }

    pub fn keep(&mut self, path: &[String], nodes: Vec<CachedNode>) {
        self.nodes.retain(|kept| {
            !kept.path.starts_with(path)
                || kept.path.len() == path.len()
                || nodes.iter().any(|node| kept.path.starts_with(&node.path))
        });
        match self.nodes.iter_mut().find(|kept| kept.path == path) {
            Some(kept) => kept.nodes = nodes,
            None => self.nodes.push(CachedChildren { path: path.to_vec(), nodes }),
        }
    }

    pub fn object_count(&self) -> usize {
        self.catalogs.iter().flat_map(|catalog| &catalog.schemas).map(|schema| schema.objects.len()).sum::<usize>()
            + self.nodes.iter().map(|kept| kept.nodes.len()).sum::<usize>()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IntrospectRequest {
    pub schemas: Vec<String>,
    pub objects: Vec<(String, String)>,
    pub force: bool,
    pub system: bool,
    pub large_schema: usize,
    pub outline: bool,
}

/// What an introspection read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Introspected {
    /// The schemas read again, `catalog.schema`.
    pub read: Vec<String>,
    /// How many schemas were unchanged and kept.
    pub unchanged: usize,
    /// How many objects the snapshot holds.
    pub objects: usize,
    /// Whether the snapshot was written to disk.
    pub saved: bool,
}

pub const LARGE_SCHEMA: usize = 2000;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Introspection {
    pub catalogs: Vec<Catalog>,
    pub read: Vec<String>,
    pub unchanged: usize,
}

/// What DDL to generate besides the object's own statement; everything is off by default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DdlOptions {
    /// A table's indexes, triggers and policies.
    pub nested: bool,
    /// `GRANT` statements for the object's privileges.
    pub permissions: bool,
    /// Foreign keys as `ALTER TABLE … ADD CONSTRAINT` after the table instead of inside it.
    pub separate_foreign_keys: bool,
    /// Primary and unique keys as `ALTER TABLE … ADD CONSTRAINT` after the table instead of inside it.
    pub separate_constraint_indexes: bool,
    /// `COMMENT ON` statements.
    pub comments: bool,
    /// A partitioned table's partitions.
    pub partitions: bool,
}

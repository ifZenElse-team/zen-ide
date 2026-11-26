use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How a database organises its data.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DataModel {
    /// Tables of rows queried with SQL.
    Relational,
    /// Keys with values of a few types (Redis, Valkey).
    KeyValue,
    /// Collections of documents (MongoDB).
    Document,
    /// Indexes of documents searched by relevance (Elasticsearch, OpenSearch).
    Search,
    /// Partitioned rows with flexible columns (Cassandra, ScyllaDB).
    WideColumn,
}

/// The kind of a node in a driver's tree; views draw whatever comes back.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum NodeKind {
    /// A database or catalogue.
    Catalog,
    /// A schema or namespace.
    Schema,
    /// A folder grouping nodes of one kind (Tables, Views).
    Folder,
    /// A table.
    Table,
    /// A view.
    View,
    /// A column of a table or view.
    Column,
    /// An index.
    Index,
    /// A group of keys sharing a prefix up to the key separator.
    KeyPrefix,
    /// A key.
    Key,
    /// A collection of documents.
    Collection,
    /// An index of a search engine.
    SearchIndex,
    /// A kind only this driver has, by its name.
    Other(String),
}

/// A node of a driver's tree.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// The path from the root, which `children` takes to list this node's children.
    pub path: Vec<String>,
    /// What the node is.
    pub kind: NodeKind,
    /// The text shown for it.
    pub label: String,
    /// Whether it can have children.
    pub has_children: bool,
    /// A short detail shown after the label (a column's type, a key's type).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Whether it belongs to the system or an extension, hidden unless system objects are shown.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub system: bool,
    /// Its size on disk in bytes, for the navigator's size bar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// The server's estimate of its rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<i64>,
    /// Its comment, shown when the navigator shows descriptions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

impl Node {
    pub fn new(path: Vec<String>, kind: NodeKind, label: impl Into<String>, has_children: bool) -> Self {
        Self {
            path,
            kind,
            label: label.into(),
            has_children,
            detail: None,
            system: false,
            size: None,
            rows: None,
            comment: None,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: Option<String>) -> Self {
        self.detail = detail;
        self
    }
}

/// A setting a driver's connections have, which the connection dialog shows as a field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingField {
    /// The setting's key in the connection's settings (`host`, `port`, `mode`).
    pub key: String,
    /// What kind of value it takes: `text`, `number`, `bool`, `path` or `choice`.
    pub kind: String,
    /// The choices of a `choice` setting.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub choices: Vec<String>,
    /// The value used when the setting is absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

/// What a driver can do; views ask this instead of assuming tables and SQL.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    /// The data models the driver serves, the main one first.
    pub models: Vec<DataModel>,
    /// The query language and its dialect, such as `sql:postgres`, `redis` or `mongo`.
    pub language: String,
    /// Whether it has transactions.
    pub transactions: bool,
    /// The isolation levels it can set, by their names.
    pub isolation_levels: Vec<String>,
    /// Whether it has catalogues (databases inside one server).
    pub catalogs: bool,
    /// Whether it has schemas.
    pub schemas: bool,
    /// The node kinds its tree has.
    pub node_kinds: Vec<NodeKind>,
    /// Whether it explains query plans.
    pub explain: bool,
    /// Whether a running statement can be cancelled on the server.
    pub server_cancel: bool,
    /// Whether it binds parameters for real instead of writing them into the text.
    pub bind_parameters: bool,
    /// Whether results can be edited, and what identifies a row (`primaryKey`, `key`, `_id`); `None` when they cannot.
    pub row_identity: Option<String>,
    /// Whether it shows the statements a structure change will run before running them.
    pub ddl_preview: bool,
    /// Whether results are paged on the server.
    pub server_paging: bool,
    /// Whether it lists the server's sessions and can end them.
    pub sessions: bool,
    /// Whether a session can be made read-only on the server.
    pub read_only_sessions: bool,
    /// The URI schemes a connection can be pasted as (`postgresql`, `redis`).
    pub uri_schemes: Vec<String>,
    /// Whether every execution context shares one connection, as an embedded database does.
    pub shared_connection: bool,
    /// Whether it asks for a password by default.
    pub password: bool,
    /// The settings its connections have, beyond the common ones.
    pub settings: Vec<SettingField>,
    /// Whether it reads its structure into the shared snapshot model (`DriverSession::introspect`);
    /// otherwise its tree is its own (`DriverSession::children`), as ClickHouse's and Redis's are.
    #[serde(default)]
    pub introspection: bool,
}

/// What a statement does, as far as reconnecting is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StatementKind {
    /// A plain read without side effects, safe to run again after the connection was lost.
    Read,
    /// Anything that may change data or session state; never run again by itself.
    Write,
}

mod scope;
mod usages;
mod words;

use std::ops::Range;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::SqlDialect;
use crate::model::NodeKind;
use crate::schema::{Catalog, Column, ConstraintKind, Level, Object, ObjectKind, Schema, Snapshot};
use scope::{Block, Clause, Name, Output, Parsed, Source, TableRef, Tk, name_of};
pub use usages::{Access, Usage, mentions, usages};

const PROBE: &str = "zen_caret_probe_";

const OTHER_SCHEMA_RELATIONS: usize = 2000;

/// What a suggestion is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    /// A keyword.
    Keyword,
    /// A schema or database.
    Schema,
    /// A table, foreign table or CTE.
    Table,
    /// A view or materialised view.
    View,
    /// A column.
    Column,
    /// A function.
    Function,
    /// An alias or a table's name in the statement.
    Alias,
    /// A join with its condition from a foreign key.
    Join,
    /// A command of a query language that is not SQL.
    Command,
    /// A key of a key-value store.
    Key,
}

/// A suggestion at the caret.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// What is shown.
    pub label: String,
    /// What it is.
    pub kind: ItemKind,
    /// The text that replaces the range from [`Suggestions::from`] to the caret.
    pub insert: String,
    /// Where the caret goes in `insert`, in bytes; at its end when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caret: Option<usize>,
    /// What is shown after the label: a column's type, a table's schema, a function's signature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The text typing is matched against.
    pub filter: String,
    /// Its group, the most relevant first.
    pub rank: u8,
}

/// The suggestions at a caret.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    /// Where the replaced text starts, a byte offset not after the caret.
    pub from: usize,
    /// The suggestions, by rank.
    pub items: Vec<Suggestion>,
    /// The paths of objects whose columns were needed but not read yet (`[db, schema, folder, object]`).
    pub missing: Vec<Vec<String>>,
}

/// What a name in a statement refers to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Target {
    /// A schema, by its path `[db, schema]`.
    Schema {
        /// Its path.
        /// The table's path.
        path: Vec<String>,
    },
    /// An object: a table, view, function and the rest.
    Object {
        /// Its path `[db, schema, folder, object]`.
        path: Vec<String>,
    },
    /// A column of a table or view.
    Column {
        path: Vec<String>,
        /// The column's name.
        column: String,
    },
}

impl Target {
    pub fn path(&self) -> &[String] {
        match self {
            Target::Schema { path } | Target::Object { path } | Target::Column { path, .. } => path,
        }
    }
}

/// A name resolved.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    /// The name's bytes in the statement.
    pub range: Range<usize>,
    /// What it refers to.
    pub target: Target,
}

/// What kind of name is not known.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UnknownKind {
    /// A table or view.
    Table,
    /// A column.
    Column,
}

/// A name a statement uses that the snapshot does not have.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Unknown {
    /// The name's bytes in the statement.
    pub range: Range<usize>,
    /// What it should be.
    pub kind: UnknownKind,
    /// The name as written, without quotes.
    pub name: String,
    /// The table it was looked for in, for a column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
}

pub struct Catalogue<'a> {
    snapshot: &'a Snapshot,
    dialect: SqlDialect,
    catalog: Option<&'a Catalog>,
    default: Option<&'a Schema>,
    path: Vec<&'a Schema>,
}

enum Lookup<'a> {
    Found(&'a Catalog, &'a Schema, &'a Object),
    Missing { certain: bool },
}

fn object_path(catalog: &Catalog, schema: &Schema, object: &Object) -> Vec<String> {
    vec![catalog.name.clone(), schema.name.clone(), object.kind.folder().to_owned(), object.key()]
}

impl<'a> Catalogue<'a> {
    pub fn new(snapshot: &'a Snapshot, dialect: SqlDialect, schema: Option<&str>, database: Option<&str>) -> Self {
        let catalog = snapshot.catalogs.iter().find(|catalog| catalog.current).or_else(|| snapshot.catalogs.first());
        let find = |name: &str| {
            catalog.and_then(|catalog| {
                catalog
                    .schemas
                    .iter()
                    .find(|known| known.name == name)
                    .or_else(|| catalog.schemas.iter().find(|known| known.name.eq_ignore_ascii_case(name)))
            })
        };
        let usual = match dialect {
            SqlDialect::Postgres => Some("public"),
            SqlDialect::Sqlite => Some("main"),
            SqlDialect::MySql | SqlDialect::MariaDb => database,
            SqlDialect::ClickHouse => database.or(Some("default")),
            SqlDialect::SqlServer => Some("dbo"),
            SqlDialect::Oracle => None,
        };
        let default = schema.and_then(find).or_else(|| usual.and_then(find)).or_else(|| {
            let users: Vec<&Schema> = catalog
                .map(|catalog| catalog.schemas.iter().filter(|schema| !schema.system).collect())
                .unwrap_or_default();
            users.first().copied()
        });
        let mut path: Vec<&Schema> = default.into_iter().collect();
        if dialect == SqlDialect::Postgres
            && let Some(system) = find("pg_catalog")
        {
            path.push(system);
        }
        Self { snapshot, dialect, catalog, default, path }
    }

    pub fn default_schema(&self) -> Option<&str> {
        self.default.map(|schema| schema.name.as_str())
    }

    pub fn dialect(&self) -> SqlDialect {
        self.dialect
    }

    fn schema_named(&self, name: &Name) -> Option<&'a Schema> {
        let catalog = self.catalog?;
        catalog
            .schemas
            .iter()
            .find(|schema| schema.name == name.text)
            .or_else(|| catalog.schemas.iter().find(|schema| name.matches(&schema.name)))
    }

    fn catalog_named(&self, name: &Name) -> Option<&'a Catalog> {
        self.snapshot.catalogs.iter().find(|catalog| name.matches(&catalog.name))
    }

    fn relation_in(schema: &'a Schema, name: &Name) -> Option<&'a Object> {
        let relations = || schema.objects.iter().filter(|object| object.kind.has_columns());
        relations()
            .find(|object| object.name == name.text)
            .or_else(|| relations().find(|object| name.matches(&object.name)))
    }

    fn relation(&self, parts: &[Name]) -> Lookup<'a> {
        let Some(catalog) = self.catalog else { return Lookup::Missing { certain: false } };
        match parts {
            [name] => {
                for schema in &self.path {
                    if let Some(object) = Self::relation_in(schema, name) {
                        return Lookup::Found(catalog, schema, object);
                    }
                }
                for schema in &catalog.schemas {
                    if let Some(object) = Self::relation_in(schema, name) {
                        return Lookup::Found(catalog, schema, object);
                    }
                }
                let system = match self.dialect {
                    SqlDialect::Postgres => name.text.to_ascii_lowercase().starts_with("pg_"),
                    SqlDialect::Sqlite => name.text.to_ascii_lowercase().starts_with("sqlite_"),
                    SqlDialect::MySql | SqlDialect::MariaDb => name.text.eq_ignore_ascii_case("dual"),
                    SqlDialect::ClickHouse => false,
                    SqlDialect::SqlServer => name.text.to_ascii_lowercase().starts_with("sys"),
                    SqlDialect::Oracle => {
                        let lower = name.text.to_ascii_lowercase();
                        lower == "dual"
                            || lower.starts_with("all_")
                            || lower.starts_with("user_")
                            || lower.starts_with("dba_")
                            || lower.starts_with("v$")
                    }
                };
                let certain = !system && self.default.is_some_and(|schema| schema.level == Level::Details);
                Lookup::Missing { certain }
            }
            [schema, name] => match self.schema_named(schema) {
                Some(found) => match Self::relation_in(found, name) {
                    Some(object) => Lookup::Found(catalog, found, object),
                    None => Lookup::Missing { certain: found.level == Level::Details },
                },
                None => Lookup::Missing { certain: false },
            },
            [database, schema, name] => {
                let Some(other) = self.catalog_named(database) else { return Lookup::Missing { certain: false } };
                let found = other.schemas.iter().find(|known| schema.matches(&known.name));
                match found.and_then(|found| Self::relation_in(found, name).map(|object| (found, object))) {
                    Some((found, object)) => Lookup::Found(other, found, object),
                    None => Lookup::Missing {
                        certain: other.current && found.is_some_and(|found| found.level == Level::Details),
                    },
                }
            }
            _ => Lookup::Missing { certain: false },
        }
    }

    fn function(&self, name: &Name) -> Option<(&'a Catalog, &'a Schema, &'a Object)> {
        let catalog = self.catalog?;
        self.path.iter().copied().chain(catalog.schemas.iter()).find_map(|schema| {
            schema
                .objects
                .iter()
                .find(|object| {
                    matches!(object.kind, ObjectKind::Function | ObjectKind::Procedure) && name.matches(&object.name)
                })
                .map(|object| (catalog, schema, object))
        })
    }
}

#[derive(Clone, Debug, Default)]
struct Columns {
    list: Vec<(String, Option<Column>)>,
    known: bool,
    missing: Option<Vec<String>>,
}

impl Columns {
    fn names(&self) -> impl Iterator<Item = &str> {
        self.list.iter().map(|(name, _)| name.as_str())
    }

    fn has(&self, name: &Name) -> bool {
        self.list.iter().any(|(known, _)| name.matches(known))
    }
}

struct Reader<'p, 'a> {
    text: &'p str,
    parsed: &'p Parsed,
    view: &'p Catalogue<'a>,
}

impl Reader<'_, '_> {
    fn columns(&self, block: usize, table: &TableRef, depth: usize) -> Columns {
        if depth > 8 {
            return Columns::default();
        }
        match &table.source {
            Source::Relation(parts) => {
                if let [name] = parts.as_slice()
                    && let Some(cte) = self.parsed.cte(block, name)
                {
                    if !cte.columns.is_empty() {
                        return Columns {
                            list: cte.columns.iter().map(|name| (name.clone(), None)).collect(),
                            known: true,
                            ..Columns::default()
                        };
                    }
                    return match cte.block {
                        Some(body) if body != block => self.outputs(body, depth + 1),
                        _ => Columns::default(),
                    };
                }
                match self.view.relation(parts) {
                    Lookup::Found(catalog, schema, object) => {
                        let path = object_path(catalog, schema, object);
                        Columns {
                            list: object
                                .columns
                                .iter()
                                .map(|column| (column.name.clone(), Some(column.clone())))
                                .collect(),
                            known: object.detailed,
                            missing: (!object.detailed).then_some(path),
                        }
                    }
                    Lookup::Missing { .. } => Columns::default(),
                }
            }
            Source::Derived(body) => self.outputs(*body, depth + 1),
            Source::Function => Columns::default(),
        }
    }

    fn outputs(&self, block: usize, depth: usize) -> Columns {
        let mut found = Columns { known: true, ..Columns::default() };
        let Some(body) = self.parsed.blocks.get(block) else { return Columns::default() };
        if body.outputs.is_empty() {
            found.known = false;
        }
        for output in &body.outputs {
            match output {
                Output::Named(name) => found.list.push((name.clone(), None)),
                Output::Star(qualifier) => {
                    for table in &body.tables {
                        if qualifier
                            .as_ref()
                            .is_some_and(|qualifier| !table.visible().is_some_and(|seen| qualifier.matches(&seen.text)))
                        {
                            continue;
                        }
                        let more = self.columns(block, table, depth);
                        found.known &= more.known;
                        if found.missing.is_none() {
                            found.missing = more.missing;
                        }
                        found.list.extend(more.list);
                    }
                }
            }
        }
        found
    }

    fn scope(&self, block: usize) -> Vec<(usize, &TableRef)> {
        self.parsed
            .chain(block)
            .into_iter()
            .flat_map(|at| self.parsed.blocks[at].tables.iter().map(move |table| (at, table)))
            .collect()
    }

    fn visible(&self, block: usize, name: &Name) -> Option<(usize, &TableRef)> {
        self.scope(block)
            .into_iter()
            .find(|(_, table)| table.alias.as_ref().is_some_and(|alias| name.matches(&alias.text)))
            .or_else(|| {
                self.scope(block).into_iter().find(|(_, table)| {
                    table.alias.is_none() && table.visible().is_some_and(|seen| name.matches(&seen.text))
                })
            })
    }

    fn significant_before(&self, at: usize) -> Option<usize> {
        self.parsed.toks.iter().rposition(|tok| tok.end <= at)
    }

    fn word(&self, index: usize) -> Option<String> {
        let tok = self.parsed.toks.get(index)?;
        (tok.kind == Tk::Word).then(|| self.text[tok.start..tok.end].to_ascii_lowercase())
    }

    fn symbol(&self, index: usize) -> Option<&str> {
        let tok = self.parsed.toks.get(index)?;
        (tok.kind == Tk::Symbol).then(|| &self.text[tok.start..tok.end])
    }

    fn qualifiers(&self, mut index: Option<usize>) -> Vec<Name> {
        let mut parts = Vec::new();
        while let Some(dot) = index {
            if self.symbol(dot) != Some(".") || dot == 0 {
                break;
            }
            let Some(tok) = self.parsed.toks.get(dot - 1).filter(|tok| matches!(tok.kind, Tk::Word | Tk::Quoted))
            else {
                break;
            };
            parts.insert(0, name_of(self.text, tok));
            index = dot.checked_sub(2);
        }
        parts
    }
}

fn keyword_case(keyword: &str, prefix: &str) -> String {
    let lower = prefix.chars().any(char::is_alphabetic) && !prefix.chars().any(char::is_uppercase);
    if lower { keyword.to_lowercase() } else { keyword.to_owned() }
}

fn quote_part(dialect: SqlDialect, name: &str) -> String {
    crate::schema::quote_when_needed(dialect.driver(), name)
}

struct Builder<'v, 'a> {
    view: &'v Catalogue<'a>,
    prefix: String,
    items: Vec<Suggestion>,
    missing: Vec<Vec<String>>,
}

impl Builder<'_, '_> {
    fn push(&mut self, label: String, kind: ItemKind, insert: String, detail: Option<String>, rank: u8) {
        let filter = label.clone();
        self.items.push(Suggestion { label, kind, insert, caret: None, detail, filter, rank });
    }

    fn keywords(&mut self, words: &[&str], rank: u8) {
        for word in words {
            let text = keyword_case(word, &self.prefix);
            self.push(text.clone(), ItemKind::Keyword, text, None, rank);
        }
    }

    fn functions(&mut self, rank: u8) {
        let mut seen: Vec<String> = Vec::new();
        for schema in &self.view.path {
            for object in schema.objects.iter().filter(|object| object.kind == ObjectKind::Function && !object.system) {
                if seen.contains(&object.name) {
                    continue;
                }
                seen.push(object.name.clone());
                let detail = object.routine.as_ref().map(|routine| match &routine.result {
                    Some(result) => format!("({}) → {result}", routine.arguments),
                    None => format!("({})", routine.arguments),
                });
                self.function(&object.name.clone(), detail, true, rank);
            }
        }
        for (name, signature) in words::functions(self.view.dialect) {
            if seen.iter().any(|known| known.eq_ignore_ascii_case(name)) {
                continue;
            }
            self.function(name, Some(signature.to_owned()), signature.contains('('), rank);
        }
    }

    fn function(&mut self, name: &str, detail: Option<String>, call: bool, rank: u8) {
        let quoted = quote_part(self.view.dialect, name);
        let (insert, caret) = if call { (format!("{quoted}()"), Some(quoted.len() + 1)) } else { (quoted, None) };
        self.items.push(Suggestion {
            label: name.to_owned(),
            kind: ItemKind::Function,
            insert,
            caret,
            detail,
            filter: name.to_owned(),
            rank,
        });
    }

    fn relations(&mut self, schema: &Schema, qualify: bool, rank: u8, budget: &mut usize) {
        for object in schema.objects.iter().filter(|object| object.kind.has_columns()) {
            if *budget == 0 {
                return;
            }
            *budget -= 1;
            let kind = match object.kind {
                ObjectKind::View | ObjectKind::MaterializedView => ItemKind::View,
                _ => ItemKind::Table,
            };
            let insert = if qualify {
                format!(
                    "{}.{}",
                    quote_part(self.view.dialect, &schema.name),
                    quote_part(self.view.dialect, &object.name)
                )
            } else {
                quote_part(self.view.dialect, &object.name)
            };
            self.items.push(Suggestion {
                label: object.name.clone(),
                kind,
                insert,
                caret: None,
                detail: Some(schema.name.clone()),
                filter: object.name.clone(),
                rank,
            });
        }
    }

    fn schemas(&mut self, catalog: &Catalog, rank: u8) {
        for schema in catalog.schemas.iter().filter(|schema| !schema.system || schema.level != Level::NotRead) {
            self.push(schema.name.clone(), ItemKind::Schema, quote_part(self.view.dialect, &schema.name), None, rank);
        }
    }

    fn columns(&mut self, columns: &Columns, qualifier: Option<&str>, rank: u8) {
        if let Some(missing) = &columns.missing
            && !self.missing.contains(missing)
        {
            self.missing.push(missing.clone());
        }
        for (name, column) in &columns.list {
            let column_part = quote_part(self.view.dialect, name);
            let insert = match qualifier {
                Some(qualifier) => format!("{qualifier}.{column_part}"),
                None => column_part,
            };
            let detail = match (column.as_ref().map(|column| column.type_name.clone()), qualifier) {
                (Some(type_name), Some(qualifier)) => Some(format!("{type_name} · {qualifier}")),
                (Some(type_name), None) => Some(type_name),
                (None, Some(qualifier)) => Some(qualifier.to_owned()),
                (None, None) => None,
            };
            self.items.push(Suggestion {
                label: name.clone(),
                kind: ItemKind::Column,
                insert,
                caret: None,
                detail,
                filter: name.clone(),
                rank,
            });
        }
    }
}

fn initials(name: &str) -> String {
    let mut out: String = name
        .split(['_', ' '])
        .filter_map(|part| part.chars().next())
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect();
    if out.is_empty() {
        out.push('t');
    }
    if !out.chars().next().is_some_and(char::is_alphabetic) {
        out.insert(0, 't');
    }
    out
}

fn join_condition(
    dialect: SqlDialect,
    left: &str,
    left_columns: &[String],
    right: &str,
    right_columns: &[String],
) -> String {
    left_columns
        .iter()
        .zip(right_columns)
        .map(|(l, r)| format!("{left}.{} = {right}.{}", quote_part(dialect, l), quote_part(dialect, r)))
        .collect::<Vec<_>>()
        .join(" AND ")
}

pub fn is_sql_keyword(word: &str) -> bool {
    static WORDS: std::sync::LazyLock<std::collections::HashSet<String>> = std::sync::LazyLock::new(|| {
        [SqlDialect::Postgres, SqlDialect::MySql, SqlDialect::Sqlite, SqlDialect::ClickHouse]
            .into_iter()
            .flat_map(|dialect| words::statement_starts(dialect).into_iter().chain(words::general(dialect)))
            .flat_map(str::split_whitespace)
            .map(str::to_ascii_uppercase)
            .collect()
    });
    let lower = word.to_ascii_lowercase();
    words::is_reserved(&lower) || WORDS.contains(&word.to_ascii_uppercase())
}
pub fn complete(text: &str, caret: usize, view: &Catalogue<'_>) -> Option<Suggestions> {
    let caret = caret.min(text.len());
    let original = Parsed::new(text, view.dialect);
    for (index, tok) in original.all.iter().enumerate() {
        let last = index + 1 == original.all.len();
        let inside = match tok.kind {
            Tk::Comment => {
                let line = text[tok.start..].starts_with("--") || text[tok.start..].starts_with('#');
                tok.start < caret
                    && (caret < tok.end || (caret == tok.end && (line || (last && original.unterminated))))
            }
            Tk::Literal => {
                tok.start < caret && (caret < tok.end || (caret == tok.end && last && original.unterminated))
            }
            Tk::Number => tok.start < caret && caret <= tok.end,
            _ => false,
        };
        if inside {
            return None;
        }
    }
    let typed = original
        .all
        .iter()
        .find(|tok| matches!(tok.kind, Tk::Word | Tk::Quoted) && tok.start < caret && caret <= tok.end)
        .copied();
    let (from, replace_end) = typed.map_or((caret, caret), |tok| (tok.start, tok.end));
    let prefix = match typed {
        Some(tok) if tok.kind == Tk::Quoted => text[tok.start + 1..caret].to_owned(),
        Some(tok) => text[tok.start..caret].to_owned(),
        None => String::new(),
    };
    let probe_text = format!("{}{PROBE}{}", &text[..from], &text[replace_end..]);
    let parsed = Parsed::new(&probe_text, view.dialect);
    let probe = from..from + PROBE.len();
    let reader = Reader { text: &probe_text, parsed: &parsed, view };
    let mut builder = Builder { view, prefix, items: Vec::new(), missing: Vec::new() };
    let block = parsed.block_at(from);
    let before = reader.significant_before(from);
    let previous = before.and_then(|at| reader.word(at).or_else(|| reader.symbol(at).map(str::to_owned)));

    let placed = parsed.blocks.iter().enumerate().find_map(|(at, block)| {
        block
            .tables
            .iter()
            .enumerate()
            .find_map(|(index, table)| match &table.source {
                Source::Relation(parts) => {
                    parts.iter().position(|part| part.range == probe).map(|part| (at, index, Some(part)))
                }
                _ => None,
            })
            .or_else(|| {
                block
                    .tables
                    .iter()
                    .position(|table| table.alias.as_ref().is_some_and(|alias| alias.range == probe))
                    .map(|index| (at, index, None))
            })
    });
    if let Some((at, index, part)) = placed {
        let table = &parsed.blocks[at].tables[index];
        match (part, &table.source) {
            (Some(0), _) => relation_items(&reader, &mut builder, at, table, previous.as_deref()),
            (Some(part), Source::Relation(parts)) => qualified_relations(&mut builder, view, &parts[..part]),
            _ => builder.keywords(words::AFTER_TABLE, 0),
        }
        return Some(finish(builder, from));
    }
    if let Some(list) = parsed.column_lists.iter().find(|list| list.range.start <= from && from <= list.range.end) {
        let table = &parsed.blocks[list.block].tables[list.table];
        let columns = reader.columns(list.block, table, 0);
        builder.columns(&columns, None, 0);
        return Some(finish(builder, from));
    }
    if parsed.created.iter().any(|name| name.range == probe)
        || parsed.blocks.iter().any(|block| block.ctes.iter().any(|cte| cte.name.range == probe))
    {
        return Some(finish(builder, from));
    }
    let qualifiers = reader.qualifiers(before);
    if !qualifiers.is_empty() {
        qualified_items(&reader, &mut builder, block, &qualifiers);
        return Some(finish(builder, from));
    }
    if before.is_none() {
        builder.keywords(&words::statement_starts(view.dialect), 0);
        return Some(finish(builder, from));
    }
    if matches!(previous.as_deref(), Some("schema" | "use" | "database")) {
        if let Some(catalog) = view.catalog {
            builder.schemas(catalog, 0);
        }
        return Some(finish(builder, from));
    }
    let clause = parsed.blocks[block].clause_at(from);
    if clause.takes_columns() {
        if clause == Clause::On && previous.as_deref() == Some("on") {
            join_conditions(&reader, &mut builder, block, from);
        }
        expression_items(&reader, &mut builder, block, clause);
    } else {
        if matches!(clause, Clause::Values | Clause::Limit) {
            builder.functions(2);
        }
        builder.keywords(&words::general(view.dialect), 3);
    }
    Some(finish(builder, from))
}

fn finish(builder: Builder<'_, '_>, from: usize) -> Suggestions {
    let mut items = builder.items;
    items.sort_by_key(|item| item.rank);
    Suggestions { from, items, missing: builder.missing }
}

fn relation_items(
    reader: &Reader<'_, '_>,
    builder: &mut Builder<'_, '_>,
    block: usize,
    table: &TableRef,
    previous: Option<&str>,
) {
    let view = reader.view;
    if previous == Some("join") && !table.ddl {
        join_items(reader, builder, block, table.at);
    }
    for at in reader.parsed.chain(block) {
        for cte in &reader.parsed.blocks[at].ctes {
            if cte.name.text != PROBE {
                builder.push(cte.name.text.clone(), ItemKind::Table, quote_part(view.dialect, &cte.name.text), None, 1);
            }
        }
    }
    let mut budget = usize::MAX;
    for schema in &view.path {
        builder.relations(schema, false, 1, &mut budget);
    }
    if let Some(catalog) = view.catalog {
        builder.schemas(catalog, 2);
        let mut budget = OTHER_SCHEMA_RELATIONS;
        for schema in catalog.schemas.iter().filter(|schema| !schema.system) {
            if view.path.iter().any(|known| known.name == schema.name) {
                continue;
            }
            builder.relations(schema, true, 3, &mut budget);
        }
    }
    if table.ddl {
        return;
    }
    if view.dialect == SqlDialect::Postgres {
        builder.functions(4);
    }
}

fn qualified_relations(builder: &mut Builder<'_, '_>, view: &Catalogue<'_>, parts: &[Name]) {
    let mut budget = usize::MAX;
    match parts {
        [schema] => {
            if let Some(found) = view.schema_named(schema) {
                builder.relations(found, false, 0, &mut budget);
            } else if let Some(catalog) = view.catalog_named(schema) {
                builder.schemas(catalog, 0);
            }
        }
        [catalog, schema] => {
            if let Some(found) = view
                .catalog_named(catalog)
                .and_then(|catalog| catalog.schemas.iter().find(|known| schema.matches(&known.name)))
            {
                builder.relations(found, false, 0, &mut budget);
            }
        }
        _ => {}
    }
}

fn qualified_items(reader: &Reader<'_, '_>, builder: &mut Builder<'_, '_>, block: usize, qualifiers: &[Name]) {
    let view = reader.view;
    match qualifiers {
        [single] => {
            if let Some((at, table)) = reader.visible(block, single) {
                let columns = reader.columns(at, table, 0);
                builder.columns(&columns, None, 0);
                return;
            }
            if let Some(cte) = reader.parsed.cte(block, single) {
                let table = TableRef {
                    source: Source::Relation(vec![cte.name.clone()]),
                    alias: None,
                    at: cte.name.range.start,
                    ddl: false,
                };
                let columns = reader.columns(block, &table, 0);
                builder.columns(&columns, None, 0);
                return;
            }
            if let Some(schema) = view.schema_named(single) {
                let mut budget = usize::MAX;
                builder.relations(schema, false, 0, &mut budget);
                let functions: Vec<&Object> =
                    schema.objects.iter().filter(|object| object.kind == ObjectKind::Function).collect();
                for function in functions {
                    builder.function(&function.name, None, true, 1);
                }
                return;
            }
            if let Some(catalog) = view.catalog_named(single) {
                builder.schemas(catalog, 0);
                return;
            }
            relation_columns(reader, builder, block, qualifiers);
        }
        [first, second] => {
            if view.schema_named(first).is_some() {
                relation_columns(reader, builder, block, qualifiers);
            } else if let Some(catalog) = view.catalog_named(first)
                && let Some(schema) = catalog.schemas.iter().find(|known| second.matches(&known.name))
            {
                let mut budget = usize::MAX;
                builder.relations(schema, false, 0, &mut budget);
            }
        }
        _ => relation_columns(reader, builder, block, qualifiers),
    }
}

fn relation_columns(reader: &Reader<'_, '_>, builder: &mut Builder<'_, '_>, block: usize, parts: &[Name]) {
    let table = TableRef { source: Source::Relation(parts.to_vec()), alias: None, at: 0, ddl: false };
    let columns = reader.columns(block, &table, 0);
    builder.columns(&columns, None, 0);
}

fn expression_items(reader: &Reader<'_, '_>, builder: &mut Builder<'_, '_>, block: usize, clause: Clause) {
    let view = reader.view;
    let scope = reader.scope(block);
    let mut gathered: Vec<(String, Columns, u8)> = Vec::new();
    for (at, table) in &scope {
        let Some(seen) = table.visible() else {
            let columns = reader.columns(*at, table, 0);
            gathered.push((String::new(), columns, if *at == block { 0 } else { 1 }));
            continue;
        };
        let columns = reader.columns(*at, table, 0);
        gathered.push((seen.text.clone(), columns, if *at == block { 0 } else { 1 }));
    }
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (_, columns, rank) in &gathered {
        if *rank == 0 {
            for name in columns.names() {
                *counts.entry(name.to_ascii_lowercase()).or_default() += 1;
            }
        }
    }
    for (seen, columns, rank) in &gathered {
        let qualifier = (!seen.is_empty()).then(|| quote_part(view.dialect, seen));
        if columns.list.iter().any(|(name, _)| counts.get(&name.to_ascii_lowercase()).copied().unwrap_or(0) > 1) {
            let mut plain = Columns { missing: columns.missing.clone(), ..Columns::default() };
            let mut shared = Columns::default();
            for entry in &columns.list {
                if counts.get(&entry.0.to_ascii_lowercase()).copied().unwrap_or(0) > 1 {
                    shared.list.push(entry.clone());
                } else {
                    plain.list.push(entry.clone());
                }
            }
            builder.columns(&plain, None, *rank);
            builder.columns(&shared, qualifier.as_deref(), *rank);
        } else {
            builder.columns(columns, None, *rank);
        }
    }
    if matches!(clause, Clause::OrderBy | Clause::GroupBy | Clause::Having) {
        for output in &reader.parsed.blocks[block].outputs {
            if let Output::Named(name) = output
                && !builder.items.iter().any(|item| item.kind == ItemKind::Column && item.label == *name)
            {
                builder.push(name.clone(), ItemKind::Alias, quote_part(view.dialect, name), None, 1);
            }
        }
    }
    for (_, table) in &scope {
        if let Some(seen) = table.visible().filter(|seen| seen.text != PROBE) {
            builder.push(seen.text.clone(), ItemKind::Alias, quote_part(view.dialect, &seen.text), None, 2);
        }
    }
    builder.functions(3);
    builder.keywords(&words::general(view.dialect), 4);
}

fn resolved_object<'a>(
    reader: &Reader<'_, 'a>,
    block: usize,
    table: &TableRef,
) -> Option<(&'a Catalog, &'a Schema, &'a Object)> {
    let Source::Relation(parts) = &table.source else { return None };
    if let [name] = parts.as_slice()
        && reader.parsed.cte(block, name).is_some()
    {
        return None;
    }
    match reader.view.relation(parts) {
        Lookup::Found(catalog, schema, object) => Some((catalog, schema, object)),
        Lookup::Missing { .. } => None,
    }
}

fn unique_alias(base: &str, taken: &[String]) -> String {
    let mut alias = base.to_owned();
    let mut n = 1;
    while taken.iter().any(|known| known.eq_ignore_ascii_case(&alias)) || words::is_reserved(&alias) {
        alias = format!("{base}{n}");
        n += 1;
    }
    alias
}

fn join_items(reader: &Reader<'_, '_>, builder: &mut Builder<'_, '_>, block: usize, at: usize) {
    let view = reader.view;
    let existing: Vec<&TableRef> = reader.parsed.blocks[block].tables.iter().filter(|table| table.at < at).collect();
    let taken: Vec<String> =
        existing.iter().filter_map(|table| table.visible().map(|seen| seen.text.clone())).collect();
    let Some(catalog) = view.catalog else { return };
    for table in existing {
        let Some((_, schema, object)) = resolved_object(reader, block, table) else { continue };
        let Some(seen) = table.visible() else { continue };
        let left = quote_part(view.dialect, &seen.text);
        let mut offer = |target_schema: &Schema,
                         target: &Object,
                         target_columns: &[String],
                         own_columns: &[String],
                         constraint: &str| {
            let alias = unique_alias(&initials(&target.name), &taken);
            let name = if view.path.iter().any(|known| known.name == target_schema.name) {
                quote_part(view.dialect, &target.name)
            } else {
                format!("{}.{}", quote_part(view.dialect, &target_schema.name), quote_part(view.dialect, &target.name))
            };
            let condition = join_condition(view.dialect, &alias, target_columns, &left, own_columns);
            let insert = format!("{name} {alias} ON {condition}");
            builder.items.push(Suggestion {
                label: insert.clone(),
                kind: ItemKind::Join,
                insert,
                caret: None,
                detail: Some(constraint.to_owned()),
                filter: target.name.clone(),
                rank: 0,
            });
        };
        for constraint in object.constraints.iter().filter(|constraint| constraint.kind == ConstraintKind::ForeignKey) {
            let Some(reference) = &constraint.references else { continue };
            let Some(target_schema) = catalog.schemas.iter().find(|known| known.name == reference.schema) else {
                continue;
            };
            let Some(target) =
                target_schema.objects.iter().find(|known| known.kind.has_columns() && known.name == reference.table)
            else {
                continue;
            };
            offer(target_schema, target, &reference.columns, &constraint.columns, &constraint.name);
        }
        for (other_schema, other_table, name) in crate::schema::references(catalog, &schema.name, &object.name) {
            let Some(target_schema) = catalog.schemas.iter().find(|known| known.name == other_schema) else { continue };
            let Some(target) =
                target_schema.objects.iter().find(|known| known.kind.has_columns() && known.name == other_table)
            else {
                continue;
            };
            let Some(constraint) = target.constraints.iter().find(|constraint| constraint.name == name) else {
                continue;
            };
            let Some(reference) = &constraint.references else { continue };
            offer(target_schema, target, &constraint.columns, &reference.columns, &constraint.name);
        }
    }
}

fn join_conditions(reader: &Reader<'_, '_>, builder: &mut Builder<'_, '_>, block: usize, at: usize) {
    let view = reader.view;
    let tables: Vec<&TableRef> = reader.parsed.blocks[block].tables.iter().filter(|table| table.at < at).collect();
    let Some((joined, earlier)) = tables.split_last() else { return };
    let Some((_, joined_schema, object)) = resolved_object(reader, block, joined) else { return };
    let Some(joined_name) = joined.visible().map(|seen| quote_part(view.dialect, &seen.text)) else { return };
    for other in earlier {
        let Some((_, other_schema, other_object)) = resolved_object(reader, block, other) else { continue };
        let Some(other_name) = other.visible().map(|seen| quote_part(view.dialect, &seen.text)) else { continue };
        let mut offer = |condition: String, constraint: &str| {
            builder.items.push(Suggestion {
                label: condition.clone(),
                kind: ItemKind::Join,
                insert: condition.clone(),
                caret: None,
                detail: Some(constraint.to_owned()),
                filter: condition,
                rank: 0,
            });
        };
        for constraint in object.constraints.iter().filter(|constraint| constraint.kind == ConstraintKind::ForeignKey) {
            if let Some(reference) = &constraint.references
                && reference.schema == other_schema.name
                && reference.table == other_object.name
            {
                offer(
                    join_condition(view.dialect, &joined_name, &constraint.columns, &other_name, &reference.columns),
                    &constraint.name,
                );
            }
        }
        for constraint in
            other_object.constraints.iter().filter(|constraint| constraint.kind == ConstraintKind::ForeignKey)
        {
            if let Some(reference) = &constraint.references
                && reference.schema == joined_schema.name
                && reference.table == object.name
            {
                offer(
                    join_condition(view.dialect, &joined_name, &reference.columns, &other_name, &constraint.columns),
                    &constraint.name,
                );
            }
        }
    }
}

pub fn resolve(text: &str, offset: usize, view: &Catalogue<'_>) -> Option<Resolved> {
    let parsed = Parsed::new(text, view.dialect);
    let reader = Reader { text, parsed: &parsed, view };
    let index = parsed.token_at(offset.min(text.len()))?;
    resolve_token(&reader, index)
}

fn resolve_token(reader: &Reader<'_, '_>, index: usize) -> Option<Resolved> {
    let (text, parsed, view) = (reader.text, reader.parsed, reader.view);
    let tok = parsed.toks[index];
    let range = tok.start..tok.end;
    let name = name_of(text, &tok);
    let block = parsed.block_at(tok.start);
    for (at, body) in parsed.blocks.iter().enumerate() {
        for table in &body.tables {
            if let Source::Relation(parts) = &table.source
                && let Some(part) = parts.iter().position(|known| known.range == range)
            {
                if part + 1 < parts.len() {
                    return schema_target(view, &parts[..=part]).map(|target| Resolved { range, target });
                }
                return resolved_object(reader, at, table).map(|(catalog, schema, object)| Resolved {
                    range,
                    target: Target::Object { path: object_path(catalog, schema, object) },
                });
            }
            if table.alias.as_ref().is_some_and(|alias| alias.range == range) {
                return resolved_object(reader, at, table).map(|(catalog, schema, object)| Resolved {
                    range,
                    target: Target::Object { path: object_path(catalog, schema, object) },
                });
            }
        }
    }
    if let Some(list) =
        parsed.column_lists.iter().find(|list| list.range.start <= tok.start && tok.end <= list.range.end)
    {
        let table = &parsed.blocks[list.block].tables[list.table];
        return column_target(reader, list.block, table, &name).map(|target| Resolved { range, target });
    }
    let mut chain = reader.qualifiers(index.checked_sub(1));
    let position = chain.len();
    chain.push(name.clone());
    let mut next = index + 1;
    while reader.symbol(next) == Some(".")
        && parsed.toks.get(next + 1).is_some_and(|tok| matches!(tok.kind, Tk::Word | Tk::Quoted))
    {
        chain.push(name_of(text, &parsed.toks[next + 1]));
        next += 2;
    }
    let target = if chain.len() > 1 {
        qualified_target(reader, block, &chain, position)
    } else {
        plain_target(reader, block, &name, reader.symbol(index + 1) == Some("("), tok.start)
    };
    target.map(|target| Resolved { range, target })
}

fn schema_target(view: &Catalogue<'_>, parts: &[Name]) -> Option<Target> {
    match parts {
        [schema] => {
            let catalog = view.catalog?;
            view.schema_named(schema)
                .map(|found| Target::Schema { path: vec![catalog.name.clone(), found.name.clone()] })
        }
        [catalog, schema] => {
            let catalog = view.catalog_named(catalog)?;
            let found = catalog.schemas.iter().find(|known| schema.matches(&known.name))?;
            Some(Target::Schema { path: vec![catalog.name.clone(), found.name.clone()] })
        }
        _ => None,
    }
}

fn column_target(reader: &Reader<'_, '_>, block: usize, table: &TableRef, name: &Name) -> Option<Target> {
    let (catalog, schema, object) = resolved_object(reader, block, table)?;
    let column = object
        .columns
        .iter()
        .find(|column| column.name == name.text)
        .or_else(|| object.columns.iter().find(|column| name.matches(&column.name)))?;
    Some(Target::Column { path: object_path(catalog, schema, object), column: column.name.clone() })
}

fn qualified_target(reader: &Reader<'_, '_>, block: usize, chain: &[Name], position: usize) -> Option<Target> {
    let view = reader.view;
    if let Some((at, table)) = reader.visible(block, &chain[0]) {
        return match position {
            0 => resolved_object(reader, at, table)
                .map(|(catalog, schema, object)| Target::Object { path: object_path(catalog, schema, object) }),
            1 => column_target(reader, at, table, &chain[1]),
            _ => None,
        };
    }
    for length in 1..=chain.len().min(3) {
        if let Lookup::Found(catalog, schema, object) = view.relation(&chain[..length]) {
            if position + 1 == length {
                return Some(Target::Object { path: object_path(catalog, schema, object) });
            }
            if position == length {
                let column = object.columns.iter().find(|column| chain[position].matches(&column.name))?;
                return Some(Target::Column {
                    path: object_path(catalog, schema, object),
                    column: column.name.clone(),
                });
            }
            if position + 1 < length {
                return schema_target(view, &chain[..=position]);
            }
        }
    }
    schema_target(view, &chain[..=position])
}

fn plain_target(reader: &Reader<'_, '_>, block: usize, name: &Name, call: bool, at: usize) -> Option<Target> {
    let view = reader.view;
    if call {
        if let Some((catalog, schema, object)) = view.function(name) {
            return Some(Target::Object { path: object_path(catalog, schema, object) });
        }
        return None;
    }
    let clause = reader.parsed.blocks[block].clause_at(at);
    if clause.takes_columns() {
        for (scope_block, table) in reader.scope(block) {
            if let Some(target) = column_target(reader, scope_block, table, name) {
                return Some(target);
            }
        }
    }
    if let Some((at, table)) = reader.visible(block, name) {
        return resolved_object(reader, at, table)
            .map(|(catalog, schema, object)| Target::Object { path: object_path(catalog, schema, object) });
    }
    match view.relation(std::slice::from_ref(name)) {
        Lookup::Found(catalog, schema, object) => Some(Target::Object { path: object_path(catalog, schema, object) }),
        Lookup::Missing { .. } => schema_target(view, std::slice::from_ref(name)),
    }
}

fn checks_columns(first: Option<&str>) -> bool {
    matches!(first, Some("select" | "with" | "insert" | "update" | "delete" | "replace" | "merge" | "values" | "table"))
}

pub fn created(text: &str, dialect: SqlDialect) -> Vec<String> {
    Parsed::new(text, dialect).created.into_iter().map(|name| name.text).collect()
}

pub fn check(text: &str, view: &Catalogue<'_>, created: &[String]) -> Vec<Unknown> {
    let parsed = Parsed::new(text, view.dialect);
    let reader = Reader { text, parsed: &parsed, view };
    let mut found = Vec::new();
    for (at, block) in parsed.blocks.iter().enumerate() {
        for table in &block.tables {
            let Source::Relation(parts) = &table.source else { continue };
            if table.ddl {
                continue;
            }
            if let [name] = parts.as_slice()
                && (parsed.cte(at, name).is_some()
                    || parsed.created.iter().any(|known| name.matches(&known.text))
                    || created.iter().any(|known| name.matches(known)))
            {
                continue;
            }
            if let Lookup::Missing { certain: true } = view.relation(parts)
                && let Some(last) = parts.last()
            {
                found.push(Unknown {
                    range: last.range.clone(),
                    kind: UnknownKind::Table,
                    name: last.text.clone(),
                    table: None,
                });
            }
        }
    }
    if !checks_columns(parsed.blocks.first().and_then(|block| block.first.as_deref())) {
        return found;
    }
    for (index, tok) in parsed.toks.iter().enumerate() {
        if !matches!(tok.kind, Tk::Word | Tk::Quoted) {
            continue;
        }
        let owner = parsed.block_at(tok.start);
        let block = &parsed.blocks[owner];
        let clause = block.clause_at(tok.start);
        if !clause.takes_columns() || reader.symbol(index + 1) == Some(".") {
            continue;
        }
        let name = name_of(text, tok);
        if index >= 2 && reader.symbol(index - 1) == Some(".") {
            if index >= 3 && reader.symbol(index - 3) == Some(".") {
                continue;
            }
            let Some(qualifier) = parsed.toks.get(index - 2).filter(|tok| matches!(tok.kind, Tk::Word | Tk::Quoted))
            else {
                continue;
            };
            let qualifier = name_of(text, qualifier);
            let Some((at, table)) = reader.visible(owner, &qualifier) else { continue };
            if resolved_object(&reader, at, table).is_none() {
                continue;
            }
            let columns = reader.columns(at, table, 0);
            if columns.known && !columns.has(&name) {
                found.push(Unknown {
                    range: name.range.clone(),
                    kind: UnknownKind::Column,
                    name: name.text.clone(),
                    table: Some(qualifier.text),
                });
            }
            continue;
        }
        if unqualified_unknown(&reader, owner, block, index, &name, clause) {
            found.push(Unknown {
                range: name.range.clone(),
                kind: UnknownKind::Column,
                name: name.text.clone(),
                table: None,
            });
        }
    }
    found.sort_by_key(|unknown| unknown.range.start);
    found
}

fn unqualified_unknown(
    reader: &Reader<'_, '_>,
    owner: usize,
    block: &Block,
    index: usize,
    name: &Name,
    clause: Clause,
) -> bool {
    let parsed = reader.parsed;
    let tok = parsed.toks[index];
    if block.in_parens(tok.start) || name.text == PROBE {
        return false;
    }
    if tok.kind == Tk::Word && (words::is_reserved(&name.text) || is_keyword(reader.view.dialect, &name.text)) {
        return false;
    }
    if let Some(next) = parsed.toks.get(index + 1)
        && (matches!(next.kind, Tk::Literal) || reader.symbol(index + 1) == Some("("))
    {
        return false;
    }
    if let Some(previous) = index.checked_sub(1).and_then(|at| parsed.toks.get(at)) {
        let symbol = reader.symbol(index - 1);
        if matches!(symbol, Some(":" | "@" | "$" | "?" | "#")) {
            return false;
        }
        let word = reader.word(index - 1);
        if matches!(word.as_deref(), Some("as" | "collate" | "interval" | "at" | "zone")) {
            return false;
        }
        if matches!(clause, Clause::Select | Clause::Returning) {
            let alias = match previous.kind {
                Tk::Quoted | Tk::Number | Tk::Literal => true,
                Tk::Word => word
                    .as_deref()
                    .is_some_and(|word| !words::is_reserved(word) && !is_keyword(reader.view.dialect, word)),
                Tk::Symbol => symbol == Some(")") || symbol == Some("*"),
                Tk::Comment => false,
            };
            if alias {
                return false;
            }
        }
    }
    if parsed.cte(owner, name).is_some() {
        return false;
    }
    let scope = reader.scope(owner);
    if scope.is_empty() {
        return false;
    }
    if scope.iter().any(|(_, table)| table.visible().is_some_and(|seen| name.matches(&seen.text))) {
        return false;
    }
    if clause != Clause::Select
        && parsed.chain(owner).iter().any(|at| {
            parsed.blocks[*at]
                .outputs
                .iter()
                .any(|output| matches!(output, Output::Named(known) if name.matches(known)))
        })
    {
        return false;
    }
    for (at, table) in &scope {
        if resolved_object(reader, *at, table).is_none() {
            return false;
        }
        let columns = reader.columns(*at, table, 0);
        if !columns.known {
            return false;
        }
        if columns.has(name) {
            return false;
        }
    }
    true
}

fn is_keyword(dialect: SqlDialect, word: &str) -> bool {
    words::general(dialect)
        .iter()
        .chain(words::statement_starts(dialect).iter())
        .any(|keyword| keyword.split(' ').any(|part| part.eq_ignore_ascii_case(word)))
}

/// What the description of an object or column says.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Description {
    /// The object's kind.
    pub kind: ObjectKind,
    /// Its schema.
    pub schema: String,
    /// Its name.
    pub name: String,
    /// Its comment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    /// Its columns, at most `limit`.
    pub columns: Vec<Column>,
    /// The column the name is, for a column.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<Column>,
    /// The columns of its primary key.
    pub primary_key: Vec<String>,
    /// A routine's arguments and result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Whether its details were read.
    pub detailed: bool,
    /// Whether columns were left out because of `limit`.
    pub truncated: bool,
}

pub fn describe(snapshot: &Snapshot, target: &Target, limit: usize) -> Option<Description> {
    let path = target.path();
    let found = crate::schema::find_object(snapshot, path)?;
    let object = found.object;
    let primary_key = object
        .constraints
        .iter()
        .find(|constraint| constraint.kind == ConstraintKind::PrimaryKey)
        .map(|constraint| constraint.columns.clone())
        .unwrap_or_default();
    let column = match target {
        Target::Column { column, .. } => object.columns.iter().find(|known| known.name == *column).cloned(),
        _ => None,
    };
    let signature = object.routine.as_ref().map(|routine| match &routine.result {
        Some(result) => format!("({}) → {result}", routine.arguments),
        None => format!("({})", routine.arguments),
    });
    Some(Description {
        kind: object.kind,
        schema: found.schema.name.clone(),
        name: object.name.clone(),
        comment: object.comment.clone(),
        columns: object.columns.iter().take(limit).cloned().collect(),
        column,
        primary_key,
        signature,
        detailed: object.detailed,
        truncated: object.columns.len() > limit,
    })
}

pub fn relational_view(snapshot: &Snapshot) -> Option<Snapshot> {
    if !snapshot.catalogs.is_empty() {
        return None;
    }
    let mut schemas: Vec<Schema> = Vec::new();
    for kept in snapshot.nodes.iter().filter(|kept| kept.path.len() == 2) {
        for node in kept.nodes.iter().filter(|node| matches!(node.kind, NodeKind::Table | NodeKind::View)) {
            let kind = if node.kind == NodeKind::View { ObjectKind::View } else { ObjectKind::Table };
            let mut object = Object::new(kind, node.label.clone());
            object.comment = node.comment.clone();
            object.system = node.system;
            object.rows = node.rows;
            if let Some(columns) = snapshot.cached(&node.path) {
                object.detailed = true;
                object.columns = columns
                    .iter()
                    .filter(|column| column.kind == NodeKind::Column)
                    .enumerate()
                    .map(|(position, column)| Column {
                        name: column.label.clone(),
                        position: i32::try_from(position + 1).unwrap_or(i32::MAX),
                        type_name: column.detail.clone().unwrap_or_default(),
                        nullable: column.detail.as_deref().is_some_and(|detail| detail.starts_with("Nullable")),
                        comment: column.comment.clone(),
                        ..Column::default()
                    })
                    .collect();
            }
            let database = &kept.path[0];
            match schemas.iter_mut().find(|schema| schema.name == *database) {
                Some(schema) => schema.objects.push(object),
                None => schemas.push(Schema {
                    name: database.clone(),
                    level: Level::Names,
                    system: matches!(database.as_str(), "system" | "INFORMATION_SCHEMA" | "information_schema"),
                    objects: vec![object],
                    ..Schema::default()
                }),
            }
        }
    }
    if schemas.is_empty() {
        return None;
    }
    Some(Snapshot {
        catalogs: vec![Catalog {
            name: String::new(),
            current: true,
            schemas_read: true,
            schemas,
            ..Catalog::default()
        }],
        ..Snapshot::new(snapshot.connection.clone(), snapshot.driver.clone())
    })
}

pub fn own_tree_path(snapshot: &Snapshot, path: &[String]) -> Option<Vec<String>> {
    let [_, database, _, name, ..] = path else { return None };
    snapshot.nodes.iter().filter(|kept| kept.path.len() == 2 && kept.path[0] == *database).find_map(|kept| {
        kept.nodes
            .iter()
            .find(|node| node.label == *name && matches!(node.kind, NodeKind::Table | NodeKind::View))
            .map(|node| node.path.clone())
    })
}

fn redis_words(line: &str) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();
    let mut words = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        if bytes[at] == b'"' || bytes[at] == b'\'' {
            let quote = bytes[at];
            at += 1;
            while at < bytes.len() && bytes[at] != quote {
                if bytes[at] == b'\\' && quote == b'"' {
                    at += 1;
                }
                at += 1;
            }
            at = (at + 1).min(bytes.len());
        } else {
            while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                at += 1;
            }
        }
        words.push(start..at);
    }
    words
}

fn redis_quote(key: &str) -> String {
    let plain = !key.is_empty() && key.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\'));
    if plain {
        return key.to_owned();
    }
    let mut out = String::from("\"");
    for c in key.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn complete_redis(line: &str, caret: usize, snapshot: &Snapshot, limit: usize) -> Option<Suggestions> {
    let caret = caret.min(line.len());
    let words = redis_words(line);
    let position = words.iter().position(|word| word.start <= caret && caret <= word.end);
    let index = position.unwrap_or_else(|| words.iter().filter(|word| word.end < caret).count());
    let from = position.map_or(caret, |at| words[at].start);
    let prefix = &line[from..caret];
    if prefix.starts_with(['"', '\'']) {
        return None;
    }
    let mut items = Vec::new();
    if index == 0 {
        for command in words::REDIS {
            let insert = keyword_case(command.name, prefix);
            items.push(Suggestion {
                label: insert.clone(),
                kind: ItemKind::Command,
                insert,
                caret: None,
                detail: Some(command.arguments.to_owned()).filter(|arguments| !arguments.is_empty()),
                filter: command.name.to_owned(),
                rank: 0,
            });
        }
        return Some(Suggestions { from, items, missing: Vec::new() });
    }
    let name = line[words[0].clone()].to_ascii_uppercase();
    let command = words::REDIS.iter().find(|command| command.name == name)?;
    let takes_key = match command.keys {
        words::Keys::None => false,
        words::Keys::First => index == 1,
        words::Keys::Two => index <= 2,
        words::Keys::All => true,
    };
    if !takes_key {
        return Some(Suggestions { from, items, missing: Vec::new() });
    }
    let mut seen = std::collections::HashSet::new();
    'kept: for kept in &snapshot.nodes {
        for node in kept.nodes.iter().filter(|node| node.kind == NodeKind::Key) {
            let key: String = node.path.iter().skip(1).map(String::as_str).collect();
            if !key.starts_with(prefix) || !seen.insert(key.clone()) {
                continue;
            }
            items.push(Suggestion {
                label: key.clone(),
                kind: ItemKind::Key,
                insert: redis_quote(&key),
                caret: None,
                detail: node.detail.clone(),
                filter: key,
                rank: 0,
            });
            if items.len() >= limit {
                break 'kept;
            }
        }
    }
    Some(Suggestions { from, items, missing: Vec::new() })
}

#[cfg(test)]
mod tests;

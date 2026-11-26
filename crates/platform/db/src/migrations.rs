pub mod command;

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::dialect::SqlDialect;

pub const FILE_LIMIT: u64 = 256 * 1024;

pub const MIGRATION_LIMIT: usize = 5000;

/// A migration tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    /// Flyway: `V<version>__<name>.sql` files, table `flyway_schema_history`.
    Flyway,
    /// Liquibase: changelogs of change sets, table `databasechangelog`.
    Liquibase,
    /// golang-migrate: `<version>_<name>.up.sql` and `.down.sql`, table `schema_migrations`.
    GolangMigrate,
    /// goose: `<version>_<name>.sql` with `-- +goose Up`, table `goose_db_version`.
    Goose,
    /// Atlas: `<version>_<name>.sql` beside `atlas.sum`, table `atlas_schema_revisions`.
    Atlas,
    /// sqlx: `<version>_<name>.sql` or `.up.sql` of a Rust project using sqlx, table `_sqlx_migrations`.
    Sqlx,
    /// Diesel: `<version>_<name>/up.sql` folders, table `__diesel_schema_migrations`.
    Diesel,
    /// Alembic: revisions in `versions/*.py`, table `alembic_version`.
    Alembic,
    /// Django: `<app>/migrations/<number>_<name>.py`, table `django_migrations`.
    Django,
    /// Prisma: `<time>_<name>/migration.sql` folders, table `_prisma_migrations`.
    Prisma,
    /// Rails: `db/migrate/<time>_<name>.rb`, table `schema_migrations`.
    Rails,
    /// Knex: `<time>_<name>.js` beside a `knexfile`, table `knex_migrations`.
    Knex,
}

impl Tool {
    pub const ALL: [Tool; 12] = [
        Tool::Flyway,
        Tool::Liquibase,
        Tool::GolangMigrate,
        Tool::Goose,
        Tool::Atlas,
        Tool::Sqlx,
        Tool::Diesel,
        Tool::Alembic,
        Tool::Django,
        Tool::Prisma,
        Tool::Rails,
        Tool::Knex,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Tool::Flyway => "flyway",
            Tool::Liquibase => "liquibase",
            Tool::GolangMigrate => "golangMigrate",
            Tool::Goose => "goose",
            Tool::Atlas => "atlas",
            Tool::Sqlx => "sqlx",
            Tool::Diesel => "diesel",
            Tool::Alembic => "alembic",
            Tool::Django => "django",
            Tool::Prisma => "prisma",
            Tool::Rails => "rails",
            Tool::Knex => "knex",
        }
    }

    pub fn from_id(id: &str) -> Option<Tool> {
        Tool::ALL.into_iter().find(|tool| tool.id() == id)
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::Flyway => "Flyway",
            Tool::Liquibase => "Liquibase",
            Tool::GolangMigrate => "golang-migrate",
            Tool::Goose => "goose",
            Tool::Atlas => "Atlas",
            Tool::Sqlx => "sqlx",
            Tool::Diesel => "Diesel",
            Tool::Alembic => "Alembic",
            Tool::Django => "Django",
            Tool::Prisma => "Prisma",
            Tool::Rails => "Rails",
            Tool::Knex => "Knex",
        }
    }

    pub fn history_table(self) -> &'static str {
        match self {
            Tool::Flyway => "flyway_schema_history",
            Tool::Liquibase => "databasechangelog",
            Tool::GolangMigrate | Tool::Rails => "schema_migrations",
            Tool::Goose => "goose_db_version",
            Tool::Atlas => "atlas_schema_revisions",
            Tool::Sqlx => "_sqlx_migrations",
            Tool::Diesel => "__diesel_schema_migrations",
            Tool::Alembic => "alembic_version",
            Tool::Django => "django_migrations",
            Tool::Prisma => "_prisma_migrations",
            Tool::Knex => "knex_migrations",
        }
    }

    pub fn reverts(self) -> bool {
        self != Tool::Prisma
    }
}

/// One migration of a folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Migration {
    /// The key its tool records it under (`1.1`, `20240101120000`, `0002_orders`, `id::author`).
    pub version: String,
    /// Its name.
    pub name: String,
    /// Its file or folder, relative to the project.
    pub file: String,
    /// Whether it can be reverted (a down file or section, Diesel's `down.sql`).
    pub reversible: bool,
    /// The revisions it follows (Alembic's `down_revision`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parents: Vec<String>,
}

/// The migrations of one tool in one folder of a project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MigrationSet {
    /// A stable id from the tool and the folder.
    pub id: String,
    /// The tool.
    pub tool: Tool,
    /// The folder of the migrations, relative to the project (`.` for the project itself).
    pub folder: String,
    /// The folder the tool runs in, relative to the project.
    pub base: String,
    /// The tool's configuration file, relative to the project, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Django's application the migrations belong to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// The migrations, oldest first.
    pub migrations: Vec<Migration>,
    /// Whether migrations were left out past [`MIGRATION_LIMIT`].
    pub truncated: bool,
}

/// Where a migration stands on a database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Applied.
    Applied,
    /// Not applied yet.
    Pending,
    /// Started and failed, or left dirty.
    Failed,
    /// Not known: the database was not read.
    Unknown,
}

/// A migration with where it stands; an applied one without a file in the project has no `file`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The key its tool records it under.
    pub version: String,
    /// Its name.
    pub name: String,
    /// Its file, relative to the project; `None` when the database has it and the project does not.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// Where it stands.
    pub state: State,
}

/// One row of a tool's history table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// The key the tool recorded.
    pub version: String,
    /// Whether it ended well.
    pub ok: bool,
}

/// What a tool's history table says.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct History {
    /// Whether the table exists; without it nothing is applied.
    pub table: bool,
    /// The migrations it records as applied (golang-migrate: the current version; Alembic: the heads).
    pub entries: Vec<HistoryEntry>,
}

pub fn history_query(tool: Tool, dialect: SqlDialect) -> String {
    let liquibase = match dialect {
        SqlDialect::MySql | SqlDialect::MariaDb => "DATABASECHANGELOG",
        _ => "databasechangelog",
    };
    let atlas = match dialect {
        SqlDialect::Sqlite => "atlas_schema_revisions",
        _ => "atlas_schema_revisions.atlas_schema_revisions",
    };
    match tool {
        Tool::Flyway => {
            "SELECT version, description, success FROM flyway_schema_history ORDER BY installed_rank".into()
        }
        Tool::Liquibase => format!("SELECT id, author, filename FROM {liquibase} ORDER BY orderexecuted"),
        Tool::GolangMigrate => "SELECT version, dirty FROM schema_migrations".into(),
        Tool::Goose => "SELECT version_id, is_applied FROM goose_db_version ORDER BY id".into(),
        Tool::Atlas => format!("SELECT version, applied, total FROM {atlas} ORDER BY version"),
        Tool::Sqlx => "SELECT version, success FROM _sqlx_migrations ORDER BY version".into(),
        Tool::Diesel => "SELECT version FROM __diesel_schema_migrations ORDER BY version".into(),
        Tool::Alembic => "SELECT version_num FROM alembic_version".into(),
        Tool::Django => "SELECT app, name FROM django_migrations ORDER BY id".into(),
        Tool::Prisma => {
            "SELECT migration_name, finished_at, rolled_back_at FROM _prisma_migrations ORDER BY started_at".into()
        }
        Tool::Rails => "SELECT version FROM schema_migrations ORDER BY version".into(),
        Tool::Knex => "SELECT name FROM knex_migrations ORDER BY id".into(),
    }
}

pub fn missing_table(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    ["does not exist", "doesn't exist", "no such table", "42p01", "42s02", "unknown database", "1146"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn truthy(value: Option<&String>) -> bool {
    value.is_some_and(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "t" | "true" | "1" | "yes" | "y"))
}

fn cell(row: &[Option<String>], index: usize) -> Option<&String> {
    row.get(index).and_then(Option::as_ref)
}

pub fn read_history(tool: Tool, app: Option<&str>, rows: &[Vec<Option<String>>]) -> History {
    let entry = |version: &str, ok: bool| HistoryEntry { version: version.trim().to_owned(), ok };
    let mut entries = Vec::new();
    match tool {
        Tool::Flyway => {
            for row in rows {
                let ok = truthy(cell(row, 2));
                match cell(row, 0) {
                    Some(version) if !version.trim().is_empty() => entries.push(entry(version, ok)),
                    _ => {
                        if let Some(description) = cell(row, 1) {
                            entries.push(entry(&format!("R:{}", description.trim()), ok));
                        }
                    }
                }
            }
        }
        Tool::Liquibase => {
            for row in rows {
                if let (Some(id), Some(author)) = (cell(row, 0), cell(row, 1)) {
                    entries.push(entry(&format!("{}::{}", id.trim(), author.trim()), true));
                }
            }
        }
        Tool::GolangMigrate => {
            if let Some(version) = rows.first().and_then(|row| cell(row, 0)) {
                entries.push(entry(version, !truthy(rows.first().and_then(|row| cell(row, 1)))));
            }
        }
        Tool::Goose => {
            let mut last: Vec<(String, bool)> = Vec::new();
            for row in rows {
                let Some(version) = cell(row, 0).map(|version| version.trim().to_owned()) else { continue };
                if version == "0" {
                    continue;
                }
                let applied = truthy(cell(row, 1));
                match last.iter_mut().find(|(known, _)| *known == version) {
                    Some(known) => known.1 = applied,
                    None => last.push((version, applied)),
                }
            }
            entries.extend(last.into_iter().filter(|(_, applied)| *applied).map(|(version, _)| entry(&version, true)));
        }
        Tool::Atlas => {
            for row in rows {
                if let Some(version) = cell(row, 0) {
                    let applied = cell(row, 1).and_then(|value| value.trim().parse::<i64>().ok());
                    let total = cell(row, 2).and_then(|value| value.trim().parse::<i64>().ok());
                    entries.push(entry(version, applied == total));
                }
            }
        }
        Tool::Sqlx => {
            for row in rows {
                if let Some(version) = cell(row, 0) {
                    entries.push(entry(version, truthy(cell(row, 1))));
                }
            }
        }
        Tool::Prisma => {
            for row in rows {
                let Some(name) = cell(row, 0) else { continue };
                if cell(row, 2).is_some() {
                    continue;
                }
                entries.push(entry(name, cell(row, 1).is_some()));
            }
        }
        Tool::Django => {
            for row in rows {
                if let (Some(row_app), Some(name)) = (cell(row, 0), cell(row, 1))
                    && app.is_none_or(|app| app == row_app.trim())
                {
                    entries.push(entry(name, true));
                }
            }
        }
        Tool::Diesel | Tool::Alembic | Tool::Rails | Tool::Knex => {
            for row in rows {
                if let Some(version) = cell(row, 0) {
                    entries.push(entry(version, true));
                }
            }
        }
    }
    History { table: true, entries }
}

pub fn statuses(set: &MigrationSet, history: Option<&History>) -> Vec<Status> {
    let status = |migration: &Migration, state: State| Status {
        version: migration.version.clone(),
        name: migration.name.clone(),
        file: Some(migration.file.clone()),
        state,
    };
    let Some(history) = history else {
        return set.migrations.iter().map(|migration| status(migration, State::Unknown)).collect();
    };
    if !history.table {
        return set.migrations.iter().map(|migration| status(migration, State::Pending)).collect();
    }
    match set.tool {
        Tool::GolangMigrate => {
            let current = history.entries.first();
            let at = current.and_then(|entry| entry.version.parse::<u64>().ok());
            let dirty = current.is_some_and(|entry| !entry.ok);
            let mut found = false;
            let mut list: Vec<Status> = set
                .migrations
                .iter()
                .map(|migration| {
                    let version = migration.version.parse::<u64>().ok();
                    let state = match (version, at) {
                        (Some(version), Some(at)) if version == at => {
                            found = true;
                            if dirty { State::Failed } else { State::Applied }
                        }
                        (Some(version), Some(at)) if version < at => State::Applied,
                        _ => State::Pending,
                    };
                    status(migration, state)
                })
                .collect();
            if let (Some(entry), false) = (current, found) {
                let state = if dirty { State::Failed } else { State::Applied };
                list.push(Status { version: entry.version.clone(), name: String::new(), file: None, state });
            }
            list
        }
        Tool::Alembic => {
            let parents: HashMap<&str, &[String]> = set
                .migrations
                .iter()
                .map(|migration| (migration.version.as_str(), migration.parents.as_slice()))
                .collect();
            let mut applied: HashSet<String> = HashSet::new();
            let mut stack: Vec<String> = history.entries.iter().map(|entry| entry.version.clone()).collect();
            while let Some(revision) = stack.pop() {
                if !applied.insert(revision.clone()) {
                    continue;
                }
                if let Some(above) = parents.get(revision.as_str()) {
                    stack.extend(above.iter().cloned());
                }
            }
            let mut list: Vec<Status> = set
                .migrations
                .iter()
                .map(|migration| {
                    let state = if applied.contains(&migration.version) { State::Applied } else { State::Pending };
                    status(migration, state)
                })
                .collect();
            for entry in &history.entries {
                if !parents.contains_key(entry.version.as_str()) {
                    list.push(Status {
                        version: entry.version.clone(),
                        name: String::new(),
                        file: None,
                        state: State::Applied,
                    });
                }
            }
            list
        }
        _ => {
            let recorded: HashMap<&str, bool> =
                history.entries.iter().map(|entry| (entry.version.as_str(), entry.ok)).collect();
            let mut list: Vec<Status> = set
                .migrations
                .iter()
                .map(|migration| {
                    let state = match recorded.get(migration.version.as_str()) {
                        Some(true) => State::Applied,
                        Some(false) => State::Failed,
                        None => State::Pending,
                    };
                    status(migration, state)
                })
                .collect();
            let known: HashSet<&str> = set.migrations.iter().map(|migration| migration.version.as_str()).collect();
            let mut seen = HashSet::new();
            for entry in &history.entries {
                if known.contains(entry.version.as_str()) || !seen.insert(entry.version.as_str()) {
                    continue;
                }
                list.push(Status {
                    version: entry.version.clone(),
                    name: String::new(),
                    file: None,
                    state: if entry.ok { State::Applied } else { State::Failed },
                });
            }
            list
        }
    }
}

pub fn django_previous(statuses: &[Status]) -> Option<String> {
    let applied: Vec<&Status> =
        statuses.iter().filter(|status| status.file.is_some() && status.state == State::Applied).collect();
    let last = applied.last()?;
    let before = statuses
        .iter()
        .filter(|status| status.file.is_some())
        .take_while(|status| status.version != last.version)
        .filter(|status| status.state == State::Applied)
        .last();
    Some(before.map_or_else(|| "zero".to_owned(), |status| status.version.clone()))
}

pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |text: &[u8]| text.iter().take_while(|byte| byte.is_ascii_digit()).count();
                let (n, m) = (take(a), take(b));
                let trim = |digits: &[u8]| {
                    let start = digits.iter().position(|byte| *byte != b'0').unwrap_or(digits.len());
                    digits[start..].to_vec()
                };
                let (x, y) = (trim(&a[..n]), trim(&b[..m]));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(&y));
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[n..];
                b = &b[m..];
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(y);
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

pub fn set_id(tool: Tool, folder: &str, app: Option<&str>) -> String {
    let text = format!("{}\u{1f}{folder}\u{1f}{}", tool.id(), app.unwrap_or_default());
    format!("{}-{:010x}", tool.id(), crate::detect::fnv(&text) & 0xff_ffff_ffff)
}

fn relative(root: &Path, path: &Path) -> String {
    let text = path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/");
    if text.is_empty() { ".".to_owned() } else { text }
}

fn join(folder: &str, name: &str) -> String {
    if folder == "." { name.to_owned() } else { format!("{folder}/{name}") }
}

fn read(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > FILE_LIMIT {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

fn entries(folder: &Path) -> (Vec<String>, Vec<String>) {
    let (mut files, mut folders) = (Vec::new(), Vec::new());
    for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => folders.push(name),
            Ok(_) => files.push(name),
            Err(_) => {}
        }
    }
    files.sort();
    folders.sort();
    (files, folders)
}

fn ancestor_with(root: &Path, folder: &Path, names: &[&str]) -> Option<(std::path::PathBuf, String)> {
    let mut at = Some(folder);
    while let Some(here) = at {
        if !here.starts_with(root) {
            return None;
        }
        for name in names {
            if here.join(name).is_file() {
                return Some((here.to_path_buf(), (*name).to_owned()));
            }
        }
        if here == root {
            return None;
        }
        at = here.parent();
    }
    None
}

fn mentions(root: &Path, folder: &Path, file: &str, needle: &str) -> bool {
    ancestor_with(root, folder, &[file])
        .and_then(|(found, name)| read(&found.join(name)))
        .is_some_and(|text| text.contains(needle))
}

fn named_migrations(folder: &Path) -> bool {
    folder.file_name().is_some_and(|name| name.to_string_lossy().to_ascii_lowercase().contains("migrat"))
}

fn numbered(name: &str) -> Option<(&str, &str)> {
    let digits = name.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let rest = name[digits..].strip_prefix('_')?;
    Some((&name[..digits], rest))
}

fn words(name: &str) -> String {
    name.replace('_', " ")
}

fn flyway_file(name: &str) -> Option<(String, String, bool)> {
    let stem = name.strip_suffix(".sql")?;
    if let Some(rest) = stem.strip_prefix("R__") {
        return Some((format!("R:{}", words(rest)), words(rest), true));
    }
    let rest = stem.strip_prefix('V')?;
    let (version, description) = rest.split_once("__")?;
    if version.is_empty() || !version.bytes().all(|byte| byte.is_ascii_digit() || byte == b'_' || byte == b'.') {
        return None;
    }
    Some((version.replace('_', "."), words(description), false))
}

fn undo_versions(files: &[String]) -> HashSet<String> {
    files
        .iter()
        .filter_map(|name| {
            let stem = name.strip_suffix(".sql")?.strip_prefix('U')?;
            let (version, _) = stem.split_once("__")?;
            Some(version.replace('_', "."))
        })
        .collect()
}

fn python_value(text: &str, key: &str) -> Option<Vec<String>> {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(key) else { continue };
        let rest = rest.trim_start();
        let rest = match rest.strip_prefix(':') {
            Some(annotated) => annotated.split_once('=').map(|(_, value)| value)?,
            None => rest.strip_prefix('=')?,
        };
        let value = rest.trim();
        if value.starts_with("None") {
            return Some(Vec::new());
        }
        let found: Vec<String> = value
            .split(['\'', '"'])
            .enumerate()
            .filter(|(index, _)| index % 2 == 1)
            .map(|(_, text)| text.to_owned())
            .filter(|text| !text.is_empty())
            .collect();
        return Some(found);
    }
    None
}

fn changesets(text: &str, file: &str) -> Vec<Migration> {
    let mut found = Vec::new();
    let migration = |id: &str, author: &str| Migration {
        version: format!("{id}::{author}"),
        name: id.to_owned(),
        file: file.to_owned(),
        reversible: true,
        parents: Vec::new(),
    };
    if file.ends_with(".sql") {
        for line in text.lines() {
            if let Some(rest) = line.trim().strip_prefix("--changeset")
                && let Some((author, rest)) = rest.trim().split_once(':')
            {
                let id = rest.split_whitespace().next().unwrap_or_default();
                if !id.is_empty() {
                    found.push(migration(id, author.trim()));
                }
            }
        }
        return found;
    }
    let attribute = |tag: &str, key: &str| -> Option<String> {
        let at = tag.find(&format!("{key}="))? + key.len() + 1;
        let quote = tag[at..].chars().next()?;
        let rest = &tag[at + 1..];
        Some(rest[..rest.find(quote)?].to_owned())
    };
    if file.ends_with(".xml") {
        for (index, _) in text.match_indices("<changeSet") {
            let tag = &text[index..text[index..].find('>').map_or(text.len(), |end| index + end)];
            if let (Some(id), Some(author)) = (attribute(tag, "id"), attribute(tag, "author")) {
                found.push(migration(&id, &author));
            }
        }
        return found;
    }
    let mut id: Option<String> = None;
    let mut author: Option<String> = None;
    let value = |line: &str, key: &str| -> Option<String> {
        let line = line.trim().trim_start_matches('-').trim().trim_start_matches('"');
        let rest = line.strip_prefix(key)?.trim_start_matches('"').trim_start().strip_prefix(':')?;
        Some(rest.trim().trim_end_matches(',').trim_matches(['"', '\'']).to_owned())
    };
    for line in text.lines() {
        if line.contains("changeSet") {
            id = None;
            author = None;
        }
        if let Some(found_id) = value(line, "id") {
            id = Some(found_id);
        } else if let Some(found_author) = value(line, "author") {
            author = Some(found_author);
        }
        if let (Some(set_id), Some(set_author)) = (&id, &author) {
            found.push(migration(set_id, set_author));
            id = None;
            author = None;
        }
    }
    found
}

fn is_changelog(name: &str, text: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let known = [".xml", ".yaml", ".yml", ".json", ".sql"].iter().any(|extension| lower.ends_with(extension));
    known
        && lower.contains("changelog")
        && (text.contains("databaseChangeLog") || text.contains("--liquibase formatted sql"))
}

struct Found {
    tool: Tool,
    migrations: Vec<Migration>,
    app: Option<String>,
}

fn sort(tool: Tool, migrations: &mut Vec<Migration>) {
    match tool {
        Tool::Liquibase => {}
        Tool::Alembic => {
            let mut left: Vec<Migration> = std::mem::take(migrations);
            left.sort_by(|a, b| a.file.cmp(&b.file));
            let mut done: HashSet<String> = HashSet::new();
            let known: HashSet<String> = left.iter().map(|migration| migration.version.clone()).collect();
            while !left.is_empty() {
                let ready = left.iter().position(|migration| {
                    migration.parents.iter().all(|parent| done.contains(parent) || !known.contains(parent))
                });
                let next = left.remove(ready.unwrap_or(0));
                done.insert(next.version.clone());
                migrations.push(next);
            }
        }
        Tool::Flyway => migrations.sort_by(|a, b| {
            let repeatable = |migration: &Migration| migration.version.starts_with("R:");
            repeatable(a).cmp(&repeatable(b)).then_with(|| natural(&a.version, &b.version))
        }),
        _ => migrations.sort_by(|a, b| natural(&a.version, &b.version).then_with(|| a.file.cmp(&b.file))),
    }
}

fn recognise_here(root: &Path, folder: &Path, rel: &str) -> Vec<Found> {
    let (files, folders) = entries(folder);
    let mut found = Vec::new();
    let file_of = |name: &str| join(rel, name);

    let undo = undo_versions(&files);
    let flyway: Vec<Migration> = files
        .iter()
        .filter_map(|name| {
            let (version, description, repeatable) = flyway_file(name)?;
            let reversible = !repeatable && undo.contains(&version);
            Some(Migration { version, name: description, file: file_of(name), reversible, parents: Vec::new() })
        })
        .collect();
    if !flyway.is_empty() {
        found.push(Found { tool: Tool::Flyway, migrations: flyway, app: None });
    }

    let sqlx = mentions(root, folder, "Cargo.toml", "sqlx");
    let paired: Vec<Migration> = files
        .iter()
        .filter_map(|name| {
            let stem = name.strip_suffix(".up.sql")?;
            let (version, rest) = numbered(stem)?;
            Some(Migration {
                version: version.to_owned(),
                name: words(rest),
                file: file_of(name),
                reversible: files.contains(&format!("{stem}.down.sql")),
                parents: Vec::new(),
            })
        })
        .collect();
    if !paired.is_empty() {
        let tool = if sqlx { Tool::Sqlx } else { Tool::GolangMigrate };
        found.push(Found { tool, migrations: paired, app: None });
    }

    let plain: Vec<(&String, &str, &str)> = files
        .iter()
        .filter(|name| !name.ends_with(".up.sql") && !name.ends_with(".down.sql"))
        .filter_map(|name| {
            let stem = name.strip_suffix(".sql")?;
            let (version, rest) = numbered(stem)?;
            Some((name, version, rest))
        })
        .collect();
    let goose_go: Vec<(&String, &str, &str)> = files
        .iter()
        .filter_map(|name| {
            let stem = name.strip_suffix(".go")?;
            let (version, rest) = numbered(stem)?;
            Some((name, version, rest))
        })
        .collect();
    if !plain.is_empty() || !goose_go.is_empty() {
        let annotated = |name: &str| read(&folder.join(name)).is_some_and(|text| text.contains("+goose"));
        let goose_marked = plain.iter().take(3).any(|(name, _, _)| annotated(name))
            || goose_go.iter().take(3).any(|(name, _, _)| annotated(name));
        let tool = if files.iter().any(|name| name == "atlas.sum") {
            Some(Tool::Atlas)
        } else if goose_marked
            || (!plain.is_empty() && named_migrations(folder) && mentions(root, folder, "go.mod", "pressly/goose"))
        {
            Some(Tool::Goose)
        } else if sqlx && !plain.is_empty() && named_migrations(folder) {
            Some(Tool::Sqlx)
        } else {
            None
        };
        if let Some(tool) = tool {
            let mut list: Vec<(&String, &str, &str)> = plain;
            if tool == Tool::Goose {
                list.extend(goose_go);
            }
            let migrations = list
                .into_iter()
                .map(|(name, version, rest)| {
                    let reversible = match tool {
                        Tool::Goose => read(&folder.join(name.as_str()))
                            .is_some_and(|text| text.contains("+goose Down") || name.ends_with(".go")),
                        _ => false,
                    };
                    let rest = rest.strip_suffix(".go").unwrap_or(rest);
                    Migration {
                        version: version.to_owned(),
                        name: words(rest),
                        file: file_of(name),
                        reversible,
                        parents: Vec::new(),
                    }
                })
                .collect();
            found.push(Found { tool, migrations, app: None });
        }
    }

    let mut diesel = Vec::new();
    let mut prisma = Vec::new();
    for name in &folders {
        let prefix = name.bytes().take_while(|byte| byte.is_ascii_digit() || *byte == b'-').count();
        if prefix == 0 || name.as_bytes().get(prefix) != Some(&b'_') {
            continue;
        }
        let inner = folder.join(name);
        let version = name[..prefix].replace('-', "");
        let rest = &name[prefix + 1..];
        if inner.join("up.sql").is_file() {
            diesel.push(Migration {
                version,
                name: words(rest),
                file: file_of(name),
                reversible: inner.join("down.sql").is_file(),
                parents: Vec::new(),
            });
        } else if inner.join("migration.sql").is_file() {
            prisma.push(Migration {
                version: name.clone(),
                name: words(rest),
                file: file_of(name),
                reversible: false,
                parents: Vec::new(),
            });
        }
    }
    if !diesel.is_empty() {
        found.push(Found { tool: Tool::Diesel, migrations: diesel, app: None });
    }
    if !prisma.is_empty() {
        found.push(Found { tool: Tool::Prisma, migrations: prisma, app: None });
    }

    if files.iter().any(|name| name == "__init__.py") {
        let django: Vec<Migration> = files
            .iter()
            .filter_map(|name| {
                let stem = name.strip_suffix(".py")?;
                let (number, rest) = numbered(stem)?;
                (number.len() == 4).then(|| Migration {
                    version: stem.to_owned(),
                    name: words(rest),
                    file: file_of(name),
                    reversible: true,
                    parents: Vec::new(),
                })
            })
            .collect();
        let named = folder.file_name().is_some_and(|name| name == "migrations");
        if named && !django.is_empty() {
            let app = folder.parent().and_then(Path::file_name).map(|name| name.to_string_lossy().into_owned());
            found.push(Found { tool: Tool::Django, migrations: django, app });
        }
    }

    let versions_folder = folder.file_name().is_some_and(|name| name == "versions");
    if versions_folder {
        let alembic: Vec<Migration> = files
            .iter()
            .filter(|name| name.ends_with(".py") && *name != "__init__.py")
            .filter_map(|name| {
                let text = read(&folder.join(name))?;
                let revision = python_value(&text, "revision")?.into_iter().next()?;
                let parents = python_value(&text, "down_revision").unwrap_or_default();
                let stem = name.trim_end_matches(".py");
                let title = stem.strip_prefix(&format!("{revision}_")).unwrap_or(stem);
                Some(Migration {
                    version: revision,
                    name: words(title),
                    file: file_of(name),
                    reversible: text.contains("def downgrade"),
                    parents,
                })
            })
            .collect();
        if !alembic.is_empty() {
            found.push(Found { tool: Tool::Alembic, migrations: alembic, app: None });
        }
    }

    if rel == "db/migrate" || rel.ends_with("/db/migrate") {
        let rails: Vec<Migration> = files
            .iter()
            .filter_map(|name| {
                let stem = name.strip_suffix(".rb")?;
                let (version, rest) = numbered(stem)?;
                Some(Migration {
                    version: version.to_owned(),
                    name: words(rest),
                    file: file_of(name),
                    reversible: true,
                    parents: Vec::new(),
                })
            })
            .collect();
        if !rails.is_empty() {
            found.push(Found { tool: Tool::Rails, migrations: rails, app: None });
        }
    }

    let knexfile = ["knexfile.js", "knexfile.ts", "knexfile.cjs", "knexfile.mjs"];
    if named_migrations(folder) && ancestor_with(root, folder, &knexfile).is_some() {
        let knex: Vec<Migration> = files
            .iter()
            .filter_map(|name| {
                let stem = [".js", ".ts", ".cjs", ".mjs"].iter().find_map(|extension| name.strip_suffix(extension))?;
                let (_, rest) = numbered(stem)?;
                Some(Migration {
                    version: name.clone(),
                    name: words(rest),
                    file: file_of(name),
                    reversible: true,
                    parents: Vec::new(),
                })
            })
            .collect();
        if !knex.is_empty() {
            found.push(Found { tool: Tool::Knex, migrations: knex, app: None });
        }
    }

    let mut liquibase = Vec::new();
    for name in &files {
        if !name.to_ascii_lowercase().contains("changelog") {
            continue;
        }
        let Some(text) = read(&folder.join(name)) else { continue };
        if is_changelog(name, &text) {
            liquibase.extend(changesets(&text, &file_of(name)));
        }
    }
    if !liquibase.is_empty() {
        found.push(Found { tool: Tool::Liquibase, migrations: liquibase, app: None });
    }
    found
}

fn config_names(tool: Tool) -> &'static [&'static str] {
    match tool {
        Tool::Flyway => &["flyway.conf", "flyway.toml"],
        Tool::Liquibase => &["liquibase.properties"],
        Tool::GolangMigrate | Tool::Goose => &["go.mod"],
        Tool::Atlas => &["atlas.hcl"],
        Tool::Sqlx => &["Cargo.toml"],
        Tool::Diesel => &["diesel.toml", "Cargo.toml"],
        Tool::Alembic => &["alembic.ini"],
        Tool::Django => &["manage.py"],
        Tool::Prisma => &["package.json"],
        Tool::Rails => &["Gemfile"],
        Tool::Knex => &["knexfile.js", "knexfile.ts", "knexfile.cjs", "knexfile.mjs"],
    }
}

pub fn recognise(root: &Path, folder: &Path) -> Vec<MigrationSet> {
    let rel = relative(root, folder);
    recognise_here(root, folder, &rel)
        .into_iter()
        .map(|Found { tool, mut migrations, app }| {
            sort(tool, &mut migrations);
            let truncated = migrations.len() > MIGRATION_LIMIT;
            migrations.truncate(MIGRATION_LIMIT);
            let start = match tool {
                Tool::Alembic | Tool::Prisma => folder.parent().unwrap_or(folder),
                _ => folder,
            };
            let (base, config) = match ancestor_with(root, start, config_names(tool)) {
                Some((found, name)) => {
                    let base = relative(root, &found);
                    let config = join(&base, &name);
                    (base, Some(config))
                }
                None => {
                    let base = match tool {
                        Tool::Rails => folder.parent().and_then(Path::parent).unwrap_or(root),
                        Tool::Alembic | Tool::Prisma | Tool::Django => start.parent().unwrap_or(root),
                        _ => root,
                    };
                    (relative(root, base), None)
                }
            };
            let config = match tool {
                Tool::Prisma => {
                    let schema = start.join("schema.prisma");
                    if schema.is_file() { Some(relative(root, &schema)) } else { config }
                }
                Tool::GolangMigrate | Tool::Goose | Tool::Sqlx => None,
                _ => config,
            };
            MigrationSet {
                id: set_id(tool, &rel, app.as_deref()),
                tool,
                folder: rel.clone(),
                base,
                config,
                app,
                migrations,
                truncated,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;

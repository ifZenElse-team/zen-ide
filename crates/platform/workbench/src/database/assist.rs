use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::assist::{Catalogue, Description, ItemKind, Suggestions, Target, Unknown, UnknownKind};
use zen_db::dialect::SqlDialect;
use zen_db::engine::ConnectionState;
use zen_db::registry::Entry;
use zen_db::schema::{ObjectKind, Snapshot};
use zen_db::sql::Dialect;
use zen_i18n::t;

use super::WORKSPACE;
use super::consoles::{connection_of, schema_of, statement_around};
use super::injected::{self, Found};
use super::objects::{ObjectOpen, OpenParams, SectionParams, ShowSection};
use super::sources::{Database, find};
use super::structure::{self, Structure};
use crate::language::{Problem, Severity, byte_offset, char_offset};
use crate::workspaces::{InnerTab, Workspaces};
use crate::{Document, Workbench};

const LISTED: usize = 50;

const KEYS_OFFERED: usize = 200;

const CHECKED_BYTES: usize = 256 * 1024;

const DESCRIBED_COLUMNS: usize = 30;

const PEEKED_COLUMNS: usize = 8;

struct Bound {
    connection: String,
    entry: Entry,
    language: String,
    schema: Option<String>,
    injected: bool,
}

impl Bound {
    fn sql(&self) -> Option<SqlDialect> {
        SqlDialect::of_language(&self.language)
            .or_else(|| self.language.starts_with("sql").then_some(SqlDialect::Postgres))
    }

    fn split(&self) -> Dialect {
        Dialect::of_language(&self.language)
    }
}

fn in_workspace(session: &Session, path: &Path) -> bool {
    session.state::<Workspaces>().is_some_and(|workspaces| {
        workspaces.is_console(path) || workspaces.tabs(WORKSPACE).iter().any(|tab| tab.path.as_deref() == Some(path))
    })
}

fn bound(session: &Session, path: &Path) -> Option<Bound> {
    if injected::hosts(path) && !in_workspace(session, path) {
        if !injected::has_sql(session, path) {
            return None;
        }
        let (binding, _) = injected::binding(session, path)?;
        return bound_to(session, binding.connection, binding.schema, true);
    }
    if !in_workspace(session, path) && super::scripts::bound(session, path).is_none() {
        return None;
    }
    let connection = connection_of(session, path)?;
    bound_to(session, connection, schema_of(session, path), false)
}

fn bound_to(session: &Session, connection: String, schema: Option<String>, injected: bool) -> Option<Bound> {
    let entry = find(session, &connection).ok()?;
    let language = session
        .state::<Database>()?
        .drivers()
        .iter()
        .find(|driver| driver.id() == entry.source.driver)
        .map(|driver| driver.capabilities().language.clone())?;
    Some(Bound { connection, entry, language, schema, injected })
}

pub(crate) fn dialects(session: &Session, connection: &str) -> Option<(SqlDialect, Dialect, Option<String>)> {
    let bound = bound_to(session, connection.to_owned(), None, false)?;
    Some((bound.sql()?, bound.split(), bound.entry.source.database.clone()))
}

pub(crate) fn target_at(session: &mut Session, path: &Path, offset: usize) -> Option<(String, Target)> {
    let bound = bound(session, path)?;
    structure::ensure(session, &bound.entry, true);
    let (text, _) = document_text(session, path)?;
    let byte = byte_offset(&text, offset);
    let (_, target) = resolved_at(session, path, &bound, byte)?;
    Some((bound.connection, target))
}

pub(crate) fn is_bound(session: &Session, path: &Path) -> bool {
    bound(session, path).is_some()
}

pub(crate) fn dialect(session: &Session, path: &Path) -> Option<String> {
    bound(session, path)?.language.strip_prefix("sql:").filter(|dialect| !dialect.is_empty()).map(str::to_owned)
}

fn connected(session: &Session, connection: &str) -> bool {
    session.state::<Database>().and_then(Database::started).is_some_and(|engine| {
        matches!(engine.status(connection).state, ConnectionState::Connected | ConnectionState::Closed)
    })
}

fn document_text(session: &Session, path: &Path) -> Option<(String, u64)> {
    session
        .state::<Workbench>()?
        .documents()
        .iter()
        .find(|document| document.path() == path)
        .map(|document| (document.text().to_owned(), document.version()))
}

fn with_view<T>(session: &Session, bound: &Bound, use_it: impl FnOnce(&Catalogue<'_>, &Snapshot) -> T) -> Option<T> {
    let snapshot = session.state::<Structure>()?.snapshot(&bound.connection)?;
    let own = zen_db::assist::relational_view(snapshot);
    let shown = own.as_ref().unwrap_or(snapshot);
    if shown.catalogs.is_empty() && bound.sql().is_some() {
        return None;
    }
    let dialect = bound.sql().unwrap_or_default();
    let database = bound.entry.source.database.as_deref();
    let view = Catalogue::new(shown, dialect, bound.schema.as_deref(), database);
    Some(use_it(&view, snapshot))
}

pub(crate) struct Offered {
    pub suggestions: Option<Suggestions>,
    pub loading: bool,
}

pub(crate) fn suggest(session: &mut Session, path: &Path, caret: usize, read: bool) -> Option<Offered> {
    if injected::hosts(path) && !in_workspace(session, path) {
        return suggest_in_code(session, path, caret, read);
    }
    let bound = bound(session, path)?;
    structure::ensure(session, &bound.entry, false);
    let (start, statement) = statement_around(session, path, bound.split(), caret)?;
    let at = caret.saturating_sub(start).min(statement.len());
    let suggestions = match bound.sql() {
        Some(dialect) => with_view(session, &bound, |view, _| zen_db::assist::complete(&statement, at, view))
            .unwrap_or_else(|| {
                let empty = Snapshot::default();
                zen_db::assist::complete(&statement, at, &Catalogue::new(&empty, dialect, None, None))
            }),
        None => session
            .state::<Structure>()
            .and_then(|structure| structure.snapshot(&bound.connection))
            .and_then(|snapshot| zen_db::assist::complete_redis(&statement, at, snapshot, KEYS_OFFERED)),
    };
    let Some(mut suggestions) = suggestions else { return Some(Offered { suggestions: None, loading: false }) };
    suggestions.from += start;
    let missing = std::mem::take(&mut suggestions.missing);
    let started = read && read_missing(session, &bound, &missing);
    let (from_disk, busy) = session
        .state::<Structure>()
        .map_or((false, false), |structure| (structure.loading(&bound.connection), structure.busy(&bound.connection)));
    let loading = from_disk || (!missing.is_empty() && (started || busy));
    Some(Offered { suggestions: Some(suggestions), loading })
}

fn statement_in(code: &str, split: Dialect, at: usize) -> (usize, String) {
    let statements = zen_db::sql::split(code, split);
    match statements.iter().rev().find(|statement| statement.start <= at) {
        Some(statement) if statement.delimiter_end > statement.end && at >= statement.delimiter_end => {
            (at, String::new())
        }
        Some(statement) => (statement.start, code[statement.start..statement.end.max(at).min(code.len())].to_owned()),
        None => (at, String::new()),
    }
}

fn escaped(insert: &str, literal: &zen_syntax::inject::Literal) -> String {
    if !literal.escapes {
        return insert.to_owned();
    }
    let quote = char::from(literal.quote);
    let mut out = String::with_capacity(insert.len());
    for c in insert.chars() {
        if c == '\\' || c == quote {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn suggest_in_code(session: &mut Session, path: &Path, caret: usize, read: bool) -> Option<Offered> {
    let Found { literal, code } = injected::found_at(session, path, caret)?;
    let bound = bound(session, path).filter(|bound| bound.sql().is_some());
    if let Some(bound) = &bound {
        structure::ensure(session, &bound.entry, false);
    }
    let at = code.from_text(caret);
    let split = bound.as_ref().map_or(Dialect::Standard, Bound::split);
    let (start, statement) = statement_in(&code.text, split, at);
    let relative = at - start;
    let keywords_only = |dialect: SqlDialect| {
        let empty = Snapshot::default();
        zen_db::assist::complete(&statement, relative, &Catalogue::new(&empty, dialect, None, None))
    };
    let suggestions = match &bound {
        Some(bound) => with_view(session, bound, |view, _| zen_db::assist::complete(&statement, relative, view))
            .unwrap_or_else(|| keywords_only(bound.sql().unwrap_or_default())),
        None => keywords_only(SqlDialect::default()),
    };
    let Some(mut suggestions) = suggestions else { return Some(Offered { suggestions: None, loading: false }) };
    suggestions.from = code.to_text(start + suggestions.from);
    for item in &mut suggestions.items {
        let before = item.caret.map(|caret| escaped(&item.insert[..caret.min(item.insert.len())], &literal).len());
        item.insert = escaped(&item.insert, &literal);
        item.caret = before;
    }
    let missing = std::mem::take(&mut suggestions.missing);
    let Some(bound) = bound else { return Some(Offered { suggestions: Some(suggestions), loading: false }) };
    let started = read && read_missing(session, &bound, &missing);
    let (from_disk, busy) = session
        .state::<Structure>()
        .map_or((false, false), |structure| (structure.loading(&bound.connection), structure.busy(&bound.connection)));
    let loading = from_disk || (!missing.is_empty() && (started || busy));
    Some(Offered { suggestions: Some(suggestions), loading })
}

fn read_missing(session: &mut Session, bound: &Bound, missing: &[Vec<String>]) -> bool {
    if missing.is_empty() || !connected(session, &bound.connection) {
        return false;
    }
    let own = session
        .state::<Structure>()
        .and_then(|structure| structure.snapshot(&bound.connection))
        .is_some_and(|snapshot| snapshot.catalogs.is_empty());
    let mut loading = false;
    for path in missing {
        if own {
            let tree = session
                .state::<Structure>()
                .and_then(|structure| structure.snapshot(&bound.connection))
                .and_then(|snapshot| zen_db::assist::own_tree_path(snapshot, path));
            if let Some(tree) = tree {
                let options = structure::tree_options(&bound.entry, true, false);
                loading |= structure::children_of(session, &bound.entry, &tree, &options, true).loading;
            }
        } else {
            loading |= structure::details(session, &bound.entry, path).ok().flatten().is_some();
        }
    }
    loading
}

pub(crate) fn revision(session: &Session) -> u64 {
    session.state::<Structure>().map_or(0, Structure::revision)
}

pub(crate) fn poll(session: &mut Session, path: &Path) {
    if let Some(bound) = bound(session, path) {
        structure::ensure(session, &bound.entry, false);
    }
}

fn resolved_at(
    session: &mut Session,
    path: &Path,
    bound: &Bound,
    offset: usize,
) -> Option<(std::ops::Range<usize>, Target)> {
    bound.sql()?;
    structure::ensure(session, &bound.entry, false);
    if bound.injected {
        let found = injected::found_at(session, path, offset)?;
        let at = found.code.from_text(offset);
        let (start, statement) = statement_in(&found.code.text, bound.split(), at);
        let resolved = with_view(session, bound, |view, _| zen_db::assist::resolve(&statement, at - start, view))??;
        let range = found.code.to_text(start + resolved.range.start)..found.code.to_text(start + resolved.range.end);
        return Some((range, resolved.target));
    }
    let (start, statement) = statement_around(session, path, bound.split(), offset)?;
    let found =
        with_view(session, bound, |view, _| zen_db::assist::resolve(&statement, offset.saturating_sub(start), view))??;
    Some((found.range.start + start..found.range.end + start, found.target))
}

fn own_path(session: &Session, bound: &Bound, target: &Target) -> Option<Vec<String>> {
    let snapshot = session.state::<Structure>()?.snapshot(&bound.connection)?;
    if !snapshot.catalogs.is_empty() {
        return None;
    }
    zen_db::assist::own_tree_path(snapshot, target.path())
}

fn open_target(session: &mut Session, bound: &Bound, target: &Target) -> Result<InnerTab, CommandError> {
    let path = own_path(session, bound, target).unwrap_or_else(|| target.path().to_vec());
    let opened = ObjectOpen.run(session, OpenParams { connection: bound.connection.clone(), path, page: None })?;
    if let Target::Column { .. } = target
        && opened.path.is_none()
    {
        let state =
            ShowSection.run(session, SectionParams { tab: Some(opened.tab.id.clone()), section: "columns".to_owned() });
        if state.is_ok() {
            return Ok(opened.tab);
        }
    }
    Ok(opened.tab)
}

pub(crate) fn go_to(session: &mut Session, path: &Path, offset: usize) -> Option<Result<(), String>> {
    let bound = bound(session, path)?;
    let (text, _) = document_text(session, path)?;
    let byte = byte_offset(&text, offset);
    let Some((_, target)) = resolved_at(session, path, &bound, byte) else {
        return Some(Err(t!("workbench-nav-no-declaration")));
    };
    Some(open_target(session, &bound, &target).map(|_| ()).map_err(|error| error.to_string()))
}

pub(crate) fn kind_word(kind: ObjectKind) -> String {
    match kind {
        ObjectKind::Table => t!("database-sql-kind-table"),
        ObjectKind::ForeignTable => t!("database-sql-kind-foreign-table"),
        ObjectKind::View => t!("database-sql-kind-view"),
        ObjectKind::MaterializedView => t!("database-sql-kind-materialized-view"),
        ObjectKind::Function => t!("database-sql-kind-function"),
        ObjectKind::Procedure => t!("database-sql-kind-procedure"),
        ObjectKind::Sequence => t!("database-sql-kind-sequence"),
        ObjectKind::Type => t!("database-sql-kind-type"),
        ObjectKind::Event => t!("database-sql-kind-event"),
        ObjectKind::Package => t!("database-sql-kind-package"),
    }
}

fn sql_kind(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Table => "TABLE",
        ObjectKind::ForeignTable => "FOREIGN TABLE",
        ObjectKind::View => "VIEW",
        ObjectKind::MaterializedView => "MATERIALIZED VIEW",
        ObjectKind::Function => "FUNCTION",
        ObjectKind::Procedure => "PROCEDURE",
        ObjectKind::Sequence => "SEQUENCE",
        ObjectKind::Type => "TYPE",
        ObjectKind::Event => "EVENT",
        ObjectKind::Package => "PACKAGE",
    }
}

fn column_line(column: &zen_db::schema::Column, primary: &[String]) -> String {
    let mut parts = vec![format!("`{}` {}", column.name, column.type_name)];
    if !column.nullable {
        parts.push(t!("database-sql-not-null"));
    }
    if primary.contains(&column.name) {
        parts.push(t!("database-sql-primary-key"));
    }
    let mut line = parts.join(", ");
    if let Some(comment) = column.comment.as_deref().filter(|comment| !comment.is_empty()) {
        line.push_str(" — ");
        line.push_str(comment);
    }
    line
}

fn markdown(description: &Description, total: usize) -> String {
    let mut out = String::new();
    if let Some(column) = &description.column {
        out.push_str(&format!("**{}** `{}.{}`\n\n", t!("database-sql-kind-column"), description.name, column.name));
        out.push_str(&column_line(column, &description.primary_key));
        out.push_str("\n\n");
    }
    out.push_str(&format!("**{}** `{}.{}`", kind_word(description.kind), description.schema, description.name));
    if let Some(signature) = &description.signature {
        out.push_str(&format!(" `{signature}`"));
    }
    out.push_str("\n\n");
    if let Some(comment) = description.comment.as_deref().filter(|comment| !comment.is_empty()) {
        out.push_str(comment);
        out.push_str("\n\n");
    }
    if !description.detailed && description.kind.has_columns() {
        out.push_str(&t!("database-sql-not-read"));
        out.push('\n');
    }
    for column in &description.columns {
        out.push_str("- ");
        out.push_str(&column_line(column, &description.primary_key));
        out.push('\n');
    }
    if description.truncated {
        out.push_str(&format!(
            "- {}\n",
            t!("database-sql-more-columns", count = total.saturating_sub(description.columns.len()))
        ));
    }
    out
}

fn declaration(description: &Description) -> String {
    if let Some(column) = &description.column {
        return format!("COLUMN {}.{} {}", description.name, column.name, column.type_name);
    }
    let mut line = format!("{} {}.{}", sql_kind(description.kind), description.schema, description.name);
    if let Some(signature) = &description.signature {
        line.push_str(signature);
    } else if !description.columns.is_empty() {
        let shown: Vec<String> = description
            .columns
            .iter()
            .take(PEEKED_COLUMNS)
            .map(|column| format!("{} {}", column.name, column.type_name))
            .collect();
        let more = if description.columns.len() > PEEKED_COLUMNS || description.truncated { ", …" } else { "" };
        line.push_str(&format!(" ({}{more})", shown.join(", ")));
    }
    line
}

pub(crate) struct Documented {
    pub markdown: String,
    pub declaration: String,
    pub range: [usize; 2],
}

fn described(session: &Session, bound: &Bound, target: &Target, limit: usize) -> Option<(Description, usize)> {
    let snapshot = session.state::<Structure>()?.snapshot(&bound.connection)?;
    let own = zen_db::assist::relational_view(snapshot);
    let shown = own.as_ref().unwrap_or(snapshot);
    let total = zen_db::schema::find_object(shown, target.path()).map_or(0, |found| found.object.columns.len());
    zen_db::assist::describe(shown, target, limit).map(|description| (description, total))
}

pub(crate) fn documentation(session: &mut Session, path: &Path, offset: usize) -> Option<Option<Documented>> {
    let bound = bound(session, path)?;
    let (text, _) = document_text(session, path)?;
    let byte = byte_offset(&text, offset);
    let Some((range, target)) = resolved_at(session, path, &bound, byte) else { return Some(None) };
    if let Some(missing) = Some(target.path().to_vec()).filter(|path| path.len() >= 4) {
        let detailed = described(session, &bound, &target, 0).is_some_and(|(description, _)| description.detailed);
        if !detailed {
            read_missing(session, &bound, &[missing]);
        }
    }
    let Some((description, total)) = described(session, &bound, &target, DESCRIBED_COLUMNS) else {
        return Some(None);
    };
    Some(Some(Documented {
        markdown: markdown(&description, total),
        declaration: declaration(&description),
        range: [char_offset(&text, range.start), char_offset(&text, range.end)],
    }))
}

#[derive(Default)]
pub struct Checks(Mutex<HashMap<PathBuf, Checked>>);

struct Checked {
    version: u64,
    revision: u64,
    problems: Vec<Problem>,
}

fn message(unknown: &Unknown) -> String {
    match (unknown.kind, &unknown.table) {
        (UnknownKind::Table, _) => t!("database-sql-unknown-table", name = unknown.name.as_str()),
        (UnknownKind::Column, Some(table)) => {
            t!("database-sql-unknown-column-of", name = unknown.name.as_str(), table = table.as_str())
        }
        (UnknownKind::Column, None) => t!("database-sql-unknown-column", name = unknown.name.as_str()),
    }
}

pub(crate) fn problems(session: &Session, path: &Path) -> Vec<Problem> {
    let Some(bound) = bound(session, path) else { return Vec::new() };
    let Some(dialect) = bound.sql() else { return Vec::new() };
    let Some(document) = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
    else {
        return Vec::new();
    };
    let version = document.version();
    let revision = revision(session);
    let checks = session.state::<Checks>();
    if let Some(checks) = checks
        && let Some(kept) = checks.0.lock().unwrap_or_else(PoisonError::into_inner).get(path)
        && kept.version == version
        && kept.revision == revision
    {
        return kept.problems.clone();
    }
    let found = check(session, &bound, dialect, document);
    if let Some(checks) = checks {
        let mut kept = checks.0.lock().unwrap_or_else(PoisonError::into_inner);
        kept.retain(|known, _| known.exists());
        kept.insert(path.to_owned(), Checked { version, revision, problems: found.clone() });
    }
    found
}

fn check(session: &Session, bound: &Bound, dialect: SqlDialect, document: &Document) -> Vec<Problem> {
    let text = document.text();
    if text.len() > CHECKED_BYTES {
        return Vec::new();
    }
    let loaded = session
        .state::<Structure>()
        .and_then(|structure| structure.snapshot(&bound.connection))
        .is_some_and(|snapshot| snapshot.taken > 0 && !snapshot.catalogs.is_empty());
    if !loaded {
        return Vec::new();
    }
    let pieces: Vec<(Option<zen_syntax::inject::Code>, String)> = if bound.injected {
        injected::found(session, document.path())
            .into_iter()
            .map(|found| (Some(found.code.clone()), found.code.text))
            .collect()
    } else {
        vec![(None, text.to_owned())]
    };
    let statements: Vec<(usize, zen_db::sql::Statement)> = pieces
        .iter()
        .enumerate()
        .flat_map(|(piece, (_, code))| {
            zen_db::sql::split(code, bound.split()).into_iter().map(move |statement| (piece, statement))
        })
        .collect();
    let created: Vec<String> = statements
        .iter()
        .flat_map(|(piece, statement)| zen_db::assist::created(statement.text(&pieces[*piece].1), dialect))
        .collect();
    let unknown: Vec<(usize, usize, Unknown)> = with_view(session, bound, |view, _| {
        statements
            .iter()
            .flat_map(|(piece, statement)| {
                zen_db::assist::check(statement.text(&pieces[*piece].1), view, &created)
                    .into_iter()
                    .map(|unknown| (*piece, statement.start, unknown))
                    .collect::<Vec<_>>()
            })
            .collect()
    })
    .unwrap_or_default();
    let snapshot = document.snapshot();
    let in_text = |piece: usize, at: usize| match &pieces[piece].0 {
        Some(code) => code.to_text(at),
        None => at,
    };
    unknown
        .into_iter()
        .filter_map(|(piece, start, unknown)| {
            let from = char_offset(text, in_text(piece, start + unknown.range.start));
            let to = char_offset(text, in_text(piece, start + unknown.range.end));
            let point = snapshot.offset_to_point(from).ok()?;
            Some(Problem {
                start: from,
                end: to,
                line: u32::try_from(point.line + 1).unwrap_or(u32::MAX),
                column: u32::try_from(point.column + 1).unwrap_or(u32::MAX),
                severity: Severity::Warning,
                message: message(&unknown),
                source: Some("database".to_owned()),
                code: Some(match unknown.kind {
                    UnknownKind::Table => "unknownTable".to_owned(),
                    UnknownKind::Column => "unknownColumn".to_owned(),
                }),
                note: None,
                rows: Vec::new(),
                details: None,
                fixes: Vec::new(),
                deprecated: false,
                unused: false,
            })
        })
        .collect()
}

/// Where in a console.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaceParams {
    /// The console; the active one when absent.
    pub path: Option<PathBuf>,
    /// The character offset; the caret when absent.
    pub offset: Option<usize>,
    /// The most items or columns to answer; 50 when absent.
    pub limit: Option<usize>,
}

fn place(session: &Session, params: &PlaceParams) -> Result<(PathBuf, usize, String), CommandError> {
    let path = match injected::code_path(session, params.path.as_deref()) {
        Some(path) => path,
        None => super::consoles::resolve(session, params.path.clone())?,
    };
    let document = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path));
    let (text, caret) = match document {
        Some(document) => (document.text().to_owned(), document.selections().primary().head),
        None => {
            let text = std::fs::read_to_string(&path).map_err(|error| {
                CommandError::new(t!(
                    "database-error-read-console",
                    path = crate::display(&path),
                    error = error.to_string()
                ))
            })?;
            (text, 0)
        }
    };
    let offset = params.offset.unwrap_or(caret).min(text.chars().count());
    Ok((path, offset, text))
}

fn unbound(session: &Session, path: &Path) -> CommandError {
    if injected::hosts(path) && !in_workspace(session, path) {
        return injected::not_bound(session, path);
    }
    if in_workspace(session, path) {
        CommandError::with_code("noConnection", super::no_console_connection(session))
    } else {
        CommandError::with_code("noConsole", t!("database-error-no-console"))
    }
}

/// An item offered.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompletionItem {
    /// What is shown.
    pub label: String,
    /// What it is.
    pub kind: ItemKind,
    /// What accepting it puts in place of `prefix`.
    pub insert: String,
    /// A column's type, a table's schema, a function's signature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// What `db.sql.complete` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CompleteAnswer {
    /// The console's connection.
    pub connection: String,
    /// Where the replaced text starts, in characters.
    pub from: usize,
    /// What has been typed of the name.
    pub prefix: String,
    /// The items that match it, best first, at most `limit`.
    pub items: Vec<CompletionItem>,
    /// How many matched.
    pub total: usize,
    /// Whether items were left out because of `limit`.
    pub truncated: bool,
    /// Whether columns the items need are being read; ask again when they are.
    pub loading: bool,
}

pub struct Complete;

impl Command for Complete {
    type Params = PlaceParams;
    type Output = CompleteAnswer;
    const ID: &'static str = "db.sql.complete";
    const TITLE: &'static str = "Complete SQL";
    const DESCRIPTION: &'static str = "What can be typed at a place of a console (`path`, else the active one; `offset` in characters, else the caret), from its connection's snapshot without a round trip: keywords of the dialect, schemas, tables and views, the columns of the tables in scope with aliases, CTEs and subqueries resolved, functions, joins and join conditions from foreign keys; Redis commands and the keys the tree has read. Matching what has been typed, best first, at most `limit` (50) with `total` and `truncated`. Never connects; when columns are missing from the snapshot and the connection is connected they are read in the background (`loading`).";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<CompleteAnswer, CommandError> {
        let (path, offset, text) = place(session, &params)?;
        let bound = bound(session, &path).ok_or_else(|| unbound(session, &path))?;
        let byte = byte_offset(&text, offset);
        let offered = suggest(session, &path, byte, true).ok_or_else(|| unbound(session, &path))?;
        let limit = params.limit.unwrap_or(LISTED);
        let Some(suggestions) = offered.suggestions else {
            return Ok(CompleteAnswer {
                connection: bound.connection,
                from: offset,
                prefix: String::new(),
                items: Vec::new(),
                total: 0,
                truncated: false,
                loading: false,
            });
        };
        let from = suggestions.from.min(byte);
        let prefix: String = text[from..byte].trim_start_matches(['"', '`', '[']).to_owned();
        let mut scored: Vec<(i64, u8, usize)> = suggestions
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let score = if prefix.is_empty() { 0 } else { zen_index::score(&prefix, &item.filter, &item.filter)? };
                Some((score, item.rank, index))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
        let total = scored.len();
        let items = scored
            .into_iter()
            .take(limit)
            .map(|(_, _, index)| {
                let item = &suggestions.items[index];
                CompletionItem {
                    label: item.label.clone(),
                    kind: item.kind,
                    insert: item.insert.clone(),
                    detail: item.detail.clone(),
                }
            })
            .collect();
        Ok(CompleteAnswer {
            connection: bound.connection,
            from: char_offset(&text, from),
            prefix,
            items,
            total,
            truncated: total > limit,
            loading: offered.loading,
        })
    }
}

/// What `db.sql.resolve` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResolveAnswer {
    /// The console's connection.
    pub connection: String,
    /// The name's characters.
    pub range: [usize; 2],
    /// What it refers to.
    pub target: Target,
    /// The object it is or belongs to, its columns at most `limit`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Description>,
}

pub struct Resolve;

impl Command for Resolve {
    type Params = PlaceParams;
    type Output = Option<ResolveAnswer>;
    const ID: &'static str = "db.sql.resolve";
    const TITLE: &'static str = "Resolve SQL Name";
    const DESCRIPTION: &'static str = "What the name at a place of a console refers to (`path`, else the active one; `offset` in characters, else the caret), resolved offline from its connection's snapshot through aliases, CTEs and subqueries: a schema (`path` `[db, schema]`), an object (`[db, schema, folder, object]`, as db.object.open takes it) or a column of one, with the object's kind, comment, primary key and columns (at most `limit`, 50). `null` when the name is not known. Never connects.";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<Option<ResolveAnswer>, CommandError> {
        let (path, offset, text) = place(session, &params)?;
        let bound = bound(session, &path).ok_or_else(|| unbound(session, &path))?;
        let byte = byte_offset(&text, offset);
        let Some((range, target)) = resolved_at(session, &path, &bound, byte) else { return Ok(None) };
        let description = described(session, &bound, &target, params.limit.unwrap_or(LISTED)).map(|(found, _)| found);
        Ok(Some(ResolveAnswer {
            connection: bound.connection,
            range: [char_offset(&text, range.start), char_offset(&text, range.end)],
            target,
            description,
        }))
    }
}

/// What `db.sql.navigate` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NavigateAnswer {
    /// What the name refers to.
    pub target: Target,
    /// The tab it opened in.
    pub tab: InnerTab,
}

pub struct Navigate;

impl Command for Navigate {
    type Params = PlaceParams;
    type Output = NavigateAnswer;
    const ID: &'static str = "db.sql.navigate";
    const TITLE: &'static str = "Open Object at Caret";
    const DESCRIPTION: &'static str = "Opens the object the name at a place of a console refers to (`path`, else the active one; `offset` in characters, else the caret) in its tab of the Database workspace, as F4, ⌘B and ⌘-click do: a table or view on its Data page, a column on its table's Columns, a schema, a function. Refused with code `unknownName` when the snapshot has no such name. Never connects.";

    fn run(&self, session: &mut Session, params: PlaceParams) -> Result<NavigateAnswer, CommandError> {
        let (path, offset, text) = place(session, &params)?;
        let bound = bound(session, &path).ok_or_else(|| unbound(session, &path))?;
        let byte = byte_offset(&text, offset);
        let (_, target) = resolved_at(session, &path, &bound, byte)
            .ok_or_else(|| CommandError::with_code("unknownName", t!("workbench-nav-no-declaration")))?;
        let tab = open_target(session, &bound, &target)?;
        Ok(NavigateAnswer { target, tab })
    }
}

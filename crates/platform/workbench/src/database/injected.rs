use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::engine::QueryStatement;
use zen_db::sql::Dialect;
use zen_i18n::t;
use zen_syntax::inject::{Code, Literal};

use super::WORKSPACE;
use super::consoles::{self, Scope, known_connections};
use super::safety::Confirmation;
use super::scripts::{self, ScriptBinding, ScriptRun};
use super::sources::find;
use crate::Workbench;
use crate::brackets::Highlighters;
use crate::language::{byte_offset, char_offset};
use crate::workspaces::OpenTabParams;

const LISTED: usize = 50;

const SHOWN_CHARS: usize = 200;

pub fn hosts(path: &Path) -> bool {
    path.file_name().is_some_and(|name| zen_db::inject::hosts(&name.to_string_lossy()))
}

pub(crate) struct Found {
    pub literal: Literal,
    pub code: Code,
}

fn with_fragments<T>(
    session: &Session,
    path: &Path,
    use_them: impl FnOnce(&str, &zen_syntax::Highlighting) -> T,
) -> Option<T> {
    let document = session
        .state::<Workbench>()?
        .documents()
        .iter()
        .find(|document| document.path() == path && document.image().is_none())?;
    let highlighting = session.state::<Highlighters>()?.highlighting_of(document)?;
    let highlighting = highlighting.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Some(use_them(document.text(), &highlighting))
}

pub fn has_sql(session: &Session, path: &Path) -> bool {
    hosts(path)
        && with_fragments(session, path, |_, highlighting| {
            highlighting.fragments().iter().any(|fragment| &*fragment.language == "sql")
        })
        .unwrap_or(false)
}

pub(crate) fn found(session: &Session, path: &Path) -> Vec<Found> {
    if !hosts(path) {
        return Vec::new();
    }
    with_fragments(session, path, |text, highlighting| {
        highlighting
            .fragments()
            .iter()
            .filter(|fragment| &*fragment.language == "sql")
            .map(|fragment| Found { literal: fragment.literal.clone(), code: fragment.code(text) })
            .collect()
    })
    .unwrap_or_default()
}

pub(crate) fn found_at(session: &Session, path: &Path, offset: usize) -> Option<Found> {
    if !hosts(path) {
        return None;
    }
    with_fragments(session, path, |text, highlighting| {
        highlighting
            .fragment_at(offset)
            .filter(|fragment| &*fragment.language == "sql")
            .map(|fragment| Found { literal: fragment.literal.clone(), code: fragment.code(text) })
    })
    .flatten()
}

fn found_around(session: &Session, path: &Path, offset: usize) -> Option<Found> {
    found(session, path)
        .into_iter()
        .find(|found| found.literal.range.start <= offset && offset <= found.literal.range.end)
}

fn place(session: &Session, path: &Path) -> Option<(String, (usize, usize))> {
    let document = session.state::<Workbench>()?.documents().iter().find(|document| document.path() == path)?;
    let range = document.selections().primary().range();
    Some((document.text().to_owned(), (range.start, range.end)))
}

pub fn sql_at(session: &Session, path: &Path, offset: Option<usize>) -> bool {
    let Some((text, (_, caret))) = place(session, path) else { return false };
    hosts(path) && found_around(session, path, byte_offset(&text, offset.unwrap_or(caret))).is_some()
}

fn no_fragment() -> CommandError {
    CommandError::with_code("noFragment", t!("database-error-no-fragment"))
}

pub(super) fn statements(
    session: &Session,
    path: &Path,
    (scope, offset): (Scope, Option<usize>),
    dialect: Dialect,
) -> Result<Vec<QueryStatement>, CommandError> {
    let (text, selection) = place(session, path).ok_or_else(no_fragment)?;
    let (from, to) = offset.map_or(selection, |at| (at, at));
    let (from, to) = (byte_offset(&text, from.min(to)), byte_offset(&text, from.max(to)));
    let found =
        found_around(session, path, to).or_else(|| found_around(session, path, from)).ok_or_else(no_fragment)?;
    let code = &found.code;
    let content = found.literal.content.clone();
    let inside = |at: usize| code.from_text(at.clamp(content.start, content.end));
    let pieces: Vec<(usize, usize, bool)> = if from == to {
        let caret = inside(to);
        let all = zen_db::sql::split(&code.text, dialect);
        let at = all.iter().position(|statement| caret <= statement.delimiter_end).unwrap_or(all.len().max(1) - 1);
        let taken = match scope {
            Scope::Statement => all.get(at..=at).unwrap_or_default(),
            Scope::Script => &all[..],
            Scope::FromCaret => all.get(at..).unwrap_or_default(),
        };
        taken.iter().map(|statement| (statement.start, statement.end, statement.client)).collect()
    } else {
        let (start, end) = (inside(from), inside(to));
        let selected = code.text.get(start..end).unwrap_or_default();
        match scope {
            Scope::Statement => vec![(start, end, zen_db::params::is_script_command(selected))],
            Scope::Script | Scope::FromCaret => zen_db::sql::split(selected, dialect)
                .into_iter()
                .map(|statement| (statement.start + start, statement.end + start, statement.client))
                .collect(),
        }
    };
    Ok(pieces
        .into_iter()
        .filter_map(|(start, end, client)| {
            let sql = code.text.get(start..end)?;
            (!sql.trim().is_empty()).then(|| QueryStatement {
                text: sql.to_owned(),
                start: char_offset(&text, code.to_text(start)),
                end: char_offset(&text, code.to_text(end)),
                client,
                params: Vec::new(),
                kept: None,
            })
        })
        .collect())
}

/// Where the connection of SQL in a code file comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BindingSource {
    /// The file's own binding.
    File,
    /// The project's default.
    Project,
    /// None: SQL is highlighted and completed with keywords only.
    None,
}

pub fn binding(session: &Session, path: &Path) -> Option<(ScriptBinding, BindingSource)> {
    let known = known_connections(session);
    if let Some(own) = scripts::bound(session, path).filter(|binding| known.contains(&binding.connection)) {
        return Some((own, BindingSource::File));
    }
    scripts::project_default(session)
        .filter(|binding| known.contains(&binding.connection))
        .map(|binding| (binding, BindingSource::Project))
}

pub(crate) fn not_bound(session: &Session, path: &Path) -> CommandError {
    let data = scripts::not_bound(session, path).data.map(serde_json::Value::Object).unwrap_or_default();
    let message = super::for_caller(
        session,
        t!("database-error-code-not-bound", path = crate::display(path)),
        t!("database-error-code-not-bound-agent", path = crate::display(path)),
    );
    CommandError::with_code("noBinding", message).with_data(data)
}

pub(crate) fn code_path(session: &Session, path: Option<&Path>) -> Option<PathBuf> {
    let path = match path {
        Some(path) => session.state::<Workbench>().map_or_else(|| path.to_owned(), |workbench| workbench.resolve(path)),
        None => session.state::<Workbench>()?.active_document()?.path().to_owned(),
    };
    hosts(&path).then_some(path)
}

fn required_code_path(session: &Session, path: Option<&Path>) -> Result<PathBuf, CommandError> {
    code_path(session, path).ok_or_else(|| CommandError::with_code("notCode", t!("database-error-not-code")))
}

/// Which code file.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InjectionParams {
    /// The code file: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
    /// The most fragments to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// An SQL fragment as `db.injection.list` answers it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FragmentInfo {
    /// The first character of its SQL, inside the quotes.
    pub start: usize,
    /// The character after its SQL.
    pub end: usize,
    /// The line its literal starts on, from 1.
    pub line: usize,
    /// The injected language, `sql`.
    pub language: String,
    /// Its SQL with escapes read, at most 200 characters.
    pub text: String,
    /// Whether `text` was cut.
    pub text_truncated: bool,
}

/// What `db.injection.list` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InjectionList {
    /// The code file.
    pub path: PathBuf,
    /// The connection its SQL is read against.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Where the connection comes from.
    pub binding: BindingSource,
    /// The fragments, in order, at most `limit`.
    pub fragments: Vec<FragmentInfo>,
    /// How many there are.
    pub total: usize,
    /// Whether fragments were left out because of `limit`.
    pub truncated: bool,
}

pub struct ListInjections;

impl Command for ListInjections {
    type Params = InjectionParams;
    type Output = InjectionList;
    const ID: &'static str = "db.injection.list";
    const TITLE: &'static str = "List SQL in Code";
    const DESCRIPTION: &'static str = "The SQL fragments of an open Go, Rust, Python, JavaScript, TypeScript, Java or Kotlin file (`path`, else the active one): string literals marked by a `language=SQL` comment on the line before or a `sql` template tag, or whose text reads as a statement (`SELECT … FROM`, `INSERT INTO`, `UPDATE … SET`, `DELETE FROM`, DDL). Each with its characters, line and SQL (escapes read, at most 200 characters), at most `limit` (50) with `total` and `truncated`; and the connection and schema they are completed and checked against (`binding`: `file`, `project` or `none`). db.sql.complete, db.sql.resolve and db.sql.navigate take a place inside a fragment. Refused with code `notCode` for other files. Never connects.";

    fn run(&self, session: &mut Session, params: InjectionParams) -> Result<InjectionList, CommandError> {
        let path = required_code_path(session, params.path.as_deref())?;
        let text = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .map(|document| document.text().to_owned())
            .unwrap_or_default();
        let found = found(session, &path);
        let limit = params.limit.unwrap_or(LISTED);
        let fragments = found
            .iter()
            .take(limit)
            .map(|found| {
                let shown: String = found.code.text.chars().take(SHOWN_CHARS).collect();
                FragmentInfo {
                    start: char_offset(&text, found.literal.content.start),
                    end: char_offset(&text, found.literal.content.end),
                    line: text[..found.literal.range.start].matches('\n').count() + 1,
                    language: "sql".to_owned(),
                    text_truncated: shown.len() < found.code.text.len(),
                    text: shown,
                }
            })
            .collect();
        let bound = binding(session, &path);
        Ok(InjectionList {
            connection: bound.as_ref().map(|(binding, _)| binding.connection.clone()),
            schema: bound.as_ref().and_then(|(binding, _)| binding.schema.clone()),
            binding: bound.map_or(BindingSource::None, |(_, source)| source),
            fragments,
            total: found.len(),
            truncated: found.len() > limit,
            path,
        })
    }
}

/// What `db.injection.bind` changes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BindInjectionParams {
    /// The code file: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
    /// The connection's id, from db.dataSource.list; absent or `null` makes the file follow the project's default.
    pub connection: Option<String>,
    /// The schema; the connection's default when absent.
    pub schema: Option<String>,
}

/// The connection SQL in a code file is read against.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InjectionBinding {
    /// The code file.
    pub path: PathBuf,
    /// The connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Where the connection comes from.
    pub binding: BindingSource,
}

fn checked(
    session: &Session,
    connection: Option<String>,
    schema: Option<String>,
) -> Result<Option<ScriptBinding>, CommandError> {
    let Some(connection) = connection else { return Ok(None) };
    find(session, &connection)?;
    Ok(Some(ScriptBinding { connection, schema: schema.filter(|schema| !schema.is_empty()) }))
}

pub struct BindInjection;

impl Command for BindInjection {
    type Params = BindInjectionParams;
    type Output = InjectionBinding;
    const ID: &'static str = "db.injection.bind";
    const TITLE: &'static str = "Set SQL Connection for File";
    const DESCRIPTION: &'static str = "Sets the connection (`connection`, an id from db.dataSource.list) and schema (`schema`) that the SQL in the string literals of a code file (`path`, else the active one) is completed and checked against, kept per project in the IDE's data folder with the bindings of scripts (`databases/scripts/<project key>.json`), never in the file. Without `connection` the file follows the project's default (db.injection.setDefault) again. Refused with code `notCode` for files that are not code. Connects nowhere. Answers the binding in effect.";

    fn run(&self, session: &mut Session, params: BindInjectionParams) -> Result<InjectionBinding, CommandError> {
        let path = required_code_path(session, params.path.as_deref())?;
        let binding = checked(session, params.connection, params.schema)?;
        scripts::store(session, &path, binding)?;
        let bound = self::binding(session, &path);
        Ok(InjectionBinding {
            connection: bound.as_ref().map(|(binding, _)| binding.connection.clone()),
            schema: bound.as_ref().and_then(|(binding, _)| binding.schema.clone()),
            binding: bound.map_or(BindingSource::None, |(_, source)| source),
            path,
        })
    }
}

/// What `db.injection.setDefault` sets.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DefaultParams {
    /// The connection's id, from db.dataSource.list; absent or `null` clears the default.
    pub connection: Option<String>,
    /// The schema; the connection's default when absent.
    pub schema: Option<String>,
}

/// The project's default connection for SQL in code.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDefault {
    /// The connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

pub struct SetDefault;

impl Command for SetDefault {
    type Params = DefaultParams;
    type Output = ProjectDefault;
    const ID: &'static str = "db.injection.setDefault";
    const TITLE: &'static str = "Set Project's SQL Connection";
    const DESCRIPTION: &'static str = "Sets the project's default connection (`connection`, an id from db.dataSource.list) and schema (`schema`) for the SQL in the string literals of its code files, used by every file without its own (db.injection.bind); without `connection` it clears it. Kept per project in the IDE's data folder, never in the repository, as a connection may be personal. Connects nowhere.";

    fn run(&self, session: &mut Session, params: DefaultParams) -> Result<ProjectDefault, CommandError> {
        let binding = checked(session, params.connection, params.schema)?;
        scripts::store_default(session, binding.clone())?;
        Ok(ProjectDefault {
            connection: binding.as_ref().map(|binding| binding.connection.clone()),
            schema: binding.and_then(|binding| binding.schema),
        })
    }
}

/// How to run the SQL of a string literal in a code file.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunInjectionParams {
    /// The code file: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
    /// What to run of the literal: `statement` (the selection, else the statement under the caret or at `offset`; the default), `script` (every statement of the literal) or `fromCaret`.
    pub scope: Scope,
    /// A character offset in the file to run at instead of the caret and selection, as a run icon in the gutter does.
    pub offset: Option<usize>,
    /// Keep the result tabs there are.
    pub new_tab: bool,
    /// The confirmation of a `confirmExecute` refusal: its `token` and the statements to skip.
    #[serde(flatten)]
    pub confirmation: Confirmation,
    /// Values of bind parameters and variables.
    #[serde(flatten)]
    pub given: super::variables::Given,
}

pub struct RunInjection;

impl Command for RunInjection {
    type Params = RunInjectionParams;
    type Output = ScriptRun;
    const ID: &'static str = "db.injection.run";
    const TITLE: &'static str = "Run SQL in Code";
    const DESCRIPTION: &'static str = "Runs the SQL in the string literal at the caret (or character `offset`) of an open Go, Rust, Python, JavaScript, TypeScript, Java or Kotlin file (`path`, else the active one), as ⌘⏎, the run icon in the gutter and the ⌥⏎ action Run SQL do: `scope` `statement` (the selection inside the literal, else the statement under the caret; the default), `script` (every statement of the literal) or `fromCaret`. Escapes are read (`\\\"` is a quote). It runs on the file's connection and schema (db.injection.bind, else the project's default db.injection.setDefault), in the file's own session, in the background; the file opens as a tab of the Database workspace, which expands and shows the results below it. Refused with code `notCode` for other files, `noFragment` when no literal there holds SQL, and `noBinding` (with the connections to choose from) when the file has no connection: nothing connects then. Placeholders (`$1`, `?`, `:name`) are bind parameters: without `parameters` the run is refused with code `parametersNeeded`. Confirmations and refusals of the connection's type as db.console.execute (`confirmed`, `skip`). Answers the query id at once (db.query.state, db.console.state).";

    fn run(&self, session: &mut Session, params: RunInjectionParams) -> Result<ScriptRun, CommandError> {
        let path = required_code_path(session, params.path.as_deref())?;
        let (text, (_, caret)) = place(session, &path).ok_or_else(no_fragment)?;
        if found_around(session, &path, byte_offset(&text, params.offset.unwrap_or(caret))).is_none() {
            return Err(no_fragment());
        }
        if binding(session, &path).is_none() {
            return Err(not_bound(session, &path));
        }
        let open =
            OpenTabParams { workspace: WORKSPACE.to_owned(), path: Some(path.clone()), ..OpenTabParams::default() };
        let tab = crate::workspaces::open_code_tab(session, open)?;
        let run = consoles::start(
            session,
            &path,
            (params.scope, params.offset),
            (params.new_tab, Self::ID),
            (&params.confirmation, &params.given),
        )?;
        Ok(ScriptRun { run, tab })
    }
}

pub const RUN_ACTION: &str = "db.injection.run";

pub const CHOOSE_ACTION: &str = "db.injection.choose";

pub(crate) fn intentions(session: &Session, path: Option<&Path>, offset: Option<usize>) -> Vec<(&'static str, String)> {
    let Some(path) = code_path(session, path) else { return Vec::new() };
    if !sql_at(session, &path, offset) {
        return Vec::new();
    }
    let choose = match binding(session, &path) {
        Some((bound, _)) => {
            let name = find(session, &bound.connection).map_or(bound.connection, |entry| entry.source.name);
            t!("database-injection-change-connection", name = name)
        }
        None => t!("database-injection-choose-connection"),
    };
    vec![(RUN_ACTION, t!("database-injection-run")), (CHOOSE_ACTION, choose)]
}

pub(crate) fn choose_refusal(session: &Session) -> CommandError {
    let path = code_path(session, None).unwrap_or_default();
    let data = scripts::not_bound(session, &path).data.map(serde_json::Value::Object).unwrap_or_default();
    let message = super::for_caller(
        session,
        t!("database-error-choose-connection"),
        t!("database-error-choose-connection-agent"),
    );
    CommandError::with_code("chooseConnection", message).with_data(data)
}

#[cfg(test)]
pub(in crate::database) mod tests;

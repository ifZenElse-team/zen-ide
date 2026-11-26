use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::dialect::SqlDialect;
use zen_db::engine::{Message, QueryStatement};
use zen_db::params::{self, NamedValue, ParamError, ParamType, ParamValue, PlaceholderKind, ScriptCommand};
use zen_db::sql::Dialect;
use zen_db::value::Value;
use zen_i18n::t;

use super::consoles::{self, Scope, console_path};
use super::sources::find;
use super::structure::{self, Structure};
use crate::Workbench;
use crate::workspaces::Workspaces;

pub const PARAMETERS_NEEDED: &str = "parametersNeeded";

const MOST_LISTED: usize = 200;
const MOST_INCLUDES: usize = 8;

/// Values given with a run.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Given {
    /// Values of bind parameters by name (`id` for `:id`, `$1`, `?1` for the first `?`): a string, number, boolean or null, or `{"type": "text" | "number" | "boolean" | "null", "value": "…"}`. When given, even empty, remembered values fill the rest and nothing is asked; when absent, a run with placeholders is refused with code `parametersNeeded` so that a person checks the values.
    pub parameters: Option<BTreeMap<String, serde_json::Value>>,
    /// Values of `${name}` variables, remembered by the console.
    pub variables: BTreeMap<String, String>,
}

impl Given {
    pub fn is_empty(&self) -> bool {
        self.parameters.is_none() && self.variables.is_empty()
    }

    pub fn add_to(&self, params: &mut serde_json::Value) {
        if let Some(parameters) = &self.parameters {
            params["parameters"] = serde_json::json!(parameters);
        }
        if !self.variables.is_empty() {
            params["variables"] = serde_json::json!(self.variables);
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Remembered {
    variables: BTreeMap<String, String>,
    parameters: BTreeMap<String, ParamValue>,
}

impl Remembered {
    fn is_empty(&self) -> bool {
        self.variables.is_empty() && self.parameters.is_empty()
    }

    fn without_secrets(&self) -> Self {
        Self {
            variables: self
                .variables
                .iter()
                .filter(|(name, _)| !params::secret_name(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
            parameters: self
                .parameters
                .iter()
                .filter(|(name, _)| !params::secret_name(name))
                .map(|(name, value)| (name.clone(), value.clone()))
                .collect(),
        }
    }
}

#[derive(Default)]
pub struct Variables {
    loaded: bool,
    scopes: BTreeMap<String, Remembered>,
}

fn variables_file(session: &Session) -> Option<PathBuf> {
    session
        .state::<Workspaces>()
        .and_then(Workspaces::data_dir)
        .map(|data| data.join("databases").join("variables.json"))
}

fn key(path: &Path) -> String {
    path.display().to_string()
}

fn load(session: &mut Session) {
    if session.state::<Variables>().is_some_and(|variables| variables.loaded) {
        return;
    }
    let read: BTreeMap<String, Remembered> = variables_file(session)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let state = session.state_mut::<Variables>();
    state.loaded = true;
    for (path, remembered) in read {
        state.scopes.entry(path).or_insert(remembered);
    }
}

fn remembered(session: &mut Session, path: &Path) -> Remembered {
    load(session);
    session.state::<Variables>().and_then(|state| state.scopes.get(&key(path)).cloned()).unwrap_or_default()
}

fn store(session: &mut Session, path: &Path, remembered: Remembered) -> Result<(), CommandError> {
    load(session);
    let state = session.state_mut::<Variables>();
    if remembered.is_empty() {
        state.scopes.remove(&key(path));
    } else {
        state.scopes.insert(key(path), remembered);
    }
    let kept: BTreeMap<String, Remembered> = state
        .scopes
        .iter()
        .map(|(path, remembered)| (path.clone(), remembered.without_secrets()))
        .filter(|(_, remembered)| !remembered.is_empty())
        .collect();
    let Some(file) = variables_file(session) else { return Ok(()) };
    let text = serde_json::to_string_pretty(&kept).unwrap_or_default();
    let temporary = file.with_extension("json.tmp");
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temporary, text))
        .and_then(|()| std::fs::rename(&temporary, &file));
    written.map_err(|error| CommandError::new(t!("database-error-variables-not-saved", error = error.to_string())))
}

/// A placeholder whose value a run needs, as a refusal and `db.console.parameters` list it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Asked {
    /// Its name: `id` for `:id`, `$1`, `?1` for the first `?`.
    pub name: String,
    /// How it is written.
    pub kind: PlaceholderKind,
    /// The value given or remembered; never for a secret-looking name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<ParamValue>,
    /// Whether its name looks like it holds a secret: its value is not kept in files and not answered.
    pub secret: bool,
    /// The type its value likely has, read from where it is used (next to a number, in arithmetic, after `LIMIT`, cast, or compared with a numeric or boolean column of the connection's cached structure); `text` when nothing tells.
    pub inferred: ParamType,
}

/// A `${name}` variable a run uses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AskedVariable {
    /// Its name.
    pub name: String,
    /// Its value; absent when it has none or its name looks like a secret.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Whether its name looks like it holds a secret.
    pub secret: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Target {
    pub dialect: Dialect,
    pub sql: SqlDialect,
    pub binds: bool,
}

pub fn target(session: &mut Session, connection: &str) -> Target {
    let driver =
        find(session, connection).ok().and_then(|entry| consoles::engine(session).driver(&entry.source.driver));
    let language = driver.as_ref().map(|driver| driver.capabilities().language.clone()).unwrap_or_default();
    let sql = SqlDialect::of_language(&language).unwrap_or_default();
    let dialect = match driver {
        Some(_) if sql == SqlDialect::ClickHouse => Dialect::MySql,
        Some(_) => Dialect::of_language(&language),
        None => Dialect::Standard,
    };
    Target { dialect, sql, binds: driver.as_ref().is_some_and(|driver| driver.capabilities().bind_parameters) }
}

#[derive(Clone, Debug, Default)]
pub struct Prepared {
    pub statements: Vec<QueryStatement>,
    pub notes: Vec<Message>,
    pub inlined: Vec<String>,
}

struct Expanded {
    statements: Vec<QueryStatement>,
    variables: BTreeMap<String, String>,
    missing: Vec<String>,
    used: Vec<String>,
}

fn include_file(session: &Session, place: Option<&Path>, file: &str) -> Option<PathBuf> {
    let written = PathBuf::from(file);
    if written.is_absolute() {
        return written.is_file().then_some(written);
    }
    let script_folder = place.filter(|path| !consoles::is_console(session, path)).and_then(Path::parent);
    let project = session.state::<Workbench>().and_then(Workbench::project);
    [script_folder, project].into_iter().flatten().map(|folder| folder.join(&written)).find(|path| path.is_file())
}

fn failed_include(file: &str, why: &str) -> CommandError {
    CommandError::with_code("includeFailed", t!("database-error-include", file = file, error = why))
        .with_data(serde_json::json!({ "file": file }))
}

fn expand(
    session: &Session,
    place: Option<&Path>,
    dialect: Dialect,
    statements: Vec<QueryStatement>,
    mut variables: BTreeMap<String, String>,
) -> Result<Expanded, CommandError> {
    let mut out =
        Expanded { statements: Vec::new(), variables: BTreeMap::new(), missing: Vec::new(), used: Vec::new() };
    let mut seen = BTreeSet::new();
    walk(session, (place, dialect, 0), statements, &mut variables, &mut seen, &mut out)?;
    out.variables = variables;
    Ok(out)
}

fn note_missing(out: &mut Expanded, names: Vec<String>) {
    for name in names {
        if !out.missing.contains(&name) {
            out.missing.push(name);
        }
    }
}

fn walk(
    session: &Session,
    (place, dialect, depth): (Option<&Path>, Dialect, usize),
    statements: Vec<QueryStatement>,
    variables: &mut BTreeMap<String, String>,
    seen: &mut BTreeSet<PathBuf>,
    out: &mut Expanded,
) -> Result<(), CommandError> {
    for mut statement in statements {
        for (_, name) in params::variables(&statement.text) {
            if !out.used.contains(&name) {
                out.used.push(name);
            }
        }
        let command = if statement.client { params::script_command(&statement.text) } else { None };
        match command {
            Some(Err(why)) => {
                return Err(CommandError::with_code(
                    "badScriptCommand",
                    t!("database-error-script-command", error = param_error(&why)),
                ));
            }
            Some(Ok(ScriptCommand::Set { name, value })) => {
                match params::expand(&value, variables) {
                    Ok(value) => {
                        variables.insert(name.clone(), value);
                    }
                    Err(missing) => note_missing(out, missing),
                }
                if !out.used.contains(&name) {
                    out.used.push(name);
                }
                out.statements.push(statement);
            }
            Some(Ok(ScriptCommand::Include { file })) => {
                let file = params::expand(&file, variables).unwrap_or(file);
                if depth >= MOST_INCLUDES {
                    return Err(failed_include(&file, &t!("database-include-too-deep", most = MOST_INCLUDES)));
                }
                let found = include_file(session, place, &file)
                    .ok_or_else(|| failed_include(&file, &t!("database-include-missing")))?;
                let canonical = found.canonicalize().unwrap_or_else(|_| found.clone());
                if !seen.insert(canonical.clone()) {
                    return Err(failed_include(&file, &t!("database-include-cycle")));
                }
                let text =
                    std::fs::read_to_string(&found).map_err(|error| failed_include(&file, &error.to_string()))?;
                let (start, end) = (statement.start, statement.end);
                let inner: Vec<QueryStatement> = zen_db::sql::split(&text, dialect)
                    .iter()
                    .filter(|split| !split.text(&text).trim().is_empty())
                    .map(|split| QueryStatement {
                        text: split.text(&text).to_owned(),
                        start,
                        end,
                        client: split.client,
                        params: Vec::new(),
                        kept: None,
                    })
                    .collect();
                out.statements.push(statement);
                walk(session, (Some(&found), dialect, depth + 1), inner, variables, seen, out)?;
                seen.remove(&canonical);
            }
            None if statement.client => out.statements.push(statement),
            None => {
                match params::expand(&statement.text, variables) {
                    Ok(text) => {
                        if text != statement.text {
                            let masked: BTreeMap<String, String> = variables
                                .iter()
                                .map(|(name, value)| {
                                    let shown = if params::secret_name(name) { zen_db::history::MASK } else { value };
                                    (name.clone(), shown.to_owned())
                                })
                                .collect();
                            statement.kept = params::expand(&statement.text, &masked).ok();
                        }
                        statement.text = text;
                    }
                    Err(missing) => note_missing(out, missing),
                }
                out.statements.push(statement);
            }
        }
    }
    Ok(())
}

fn asked_of(statements: &[QueryStatement], dialect: Dialect) -> Vec<(String, PlaceholderKind)> {
    let mut asked: Vec<(String, PlaceholderKind)> = Vec::new();
    for statement in statements.iter().filter(|statement| !statement.client) {
        for placeholder in params::placeholders(&statement.text, dialect) {
            if !asked.iter().any(|(name, _)| *name == placeholder.name) {
                asked.push((placeholder.name, placeholder.kind));
            }
        }
    }
    asked
}

fn known_columns(session: &Session, connection: &str, texts: &[&str]) -> BTreeMap<String, Option<ParamType>> {
    let mut known: BTreeMap<String, Option<ParamType>> = BTreeMap::new();
    let Some(snapshot) = session.state::<Structure>().and_then(|structure| structure.snapshot(connection)) else {
        return known;
    };
    let objects = snapshot.catalogs.iter().flat_map(|catalog| &catalog.schemas).flat_map(|schema| &schema.objects);
    for object in objects.filter(|object| !object.columns.is_empty()) {
        if !texts.iter().any(|text| params::mentions(text, &object.name)) {
            continue;
        }
        for column in &object.columns {
            let kind = params::column_type(&column.type_name);
            let name = column.name.to_ascii_lowercase();
            match known.get(&name) {
                Some(seen) if *seen != kind => {
                    known.insert(name, None);
                }
                Some(_) => {}
                None => {
                    known.insert(name, kind);
                }
            }
        }
    }
    known
}

fn inferred_of(
    session: &mut Session,
    connection: &str,
    statements: &[QueryStatement],
    dialect: Dialect,
) -> BTreeMap<String, ParamType> {
    if let Ok(entry) = find(session, connection) {
        structure::ensure(session, &entry, true);
    }
    let texts: Vec<&str> =
        statements.iter().filter(|statement| !statement.client).map(|statement| statement.text.as_str()).collect();
    let known = known_columns(session, connection, &texts);
    let columns = |name: &str| known.get(name).copied().flatten();
    let mut inferred = BTreeMap::new();
    for text in texts {
        for (name, kind) in params::infer_types(text, dialect, &columns) {
            inferred.entry(name).or_insert(kind);
        }
    }
    inferred
}

fn variables_of(expanded: &Expanded) -> Vec<AskedVariable> {
    expanded
        .used
        .iter()
        .chain(expanded.missing.iter().filter(|name| !expanded.used.contains(name)))
        .take(MOST_LISTED)
        .map(|name| AskedVariable {
            name: name.clone(),
            value: expanded.variables.get(name).filter(|_| !params::secret_name(name)).cloned(),
            secret: params::secret_name(name),
        })
        .collect()
}

fn shown_value(name: &str, value: Option<&ParamValue>) -> Option<ParamValue> {
    value.filter(|_| !params::secret_name(name)).cloned()
}

fn given_values(given: &Given) -> Result<BTreeMap<String, ParamValue>, CommandError> {
    let mut values = BTreeMap::new();
    for (name, json) in given.parameters.iter().flatten() {
        let value = ParamValue::of_json(json).map_err(|why| invalid(name, &param_error(&why)))?;
        values.insert(name.trim_start_matches(':').to_owned(), value);
    }
    Ok(values)
}

pub fn param_error(error: &ParamError) -> String {
    match error {
        ParamError::NotANumber(text) => t!("database-param-error-number", text = text.as_str()),
        ParamError::NotABoolean(text) => t!("database-param-error-boolean", text = text.as_str()),
        ParamError::NotAValue => t!("database-param-error-value"),
        ParamError::IncludeNeedsFile => t!("database-param-error-include"),
        ParamError::SetNeedsValue => t!("database-param-error-set"),
        ParamError::NotAVariableName(name) => t!("database-param-error-name", name = name.as_str()),
    }
}

fn invalid(name: &str, why: &str) -> CommandError {
    CommandError::with_code("invalidParameter", t!("database-error-invalid-parameter", name = name, error = why))
        .with_data(serde_json::json!({ "name": name }))
}

pub fn prepare(
    session: &mut Session,
    place: Option<&Path>,
    connection: &str,
    statements: Vec<QueryStatement>,
    (given, retry): (&Given, (&str, serde_json::Value)),
) -> Result<Prepared, CommandError> {
    let target = target(session, connection);
    let mut memory = place.map(|path| remembered(session, path)).unwrap_or_default();
    let mut variables = memory.variables.clone();
    variables.extend(given.variables.clone());
    let expanded = expand(session, place, target.dialect, statements, variables)?;
    let names = asked_of(&expanded.statements, target.dialect);
    let given_values = given_values(given)?;
    let value_of = |name: &str| given_values.get(name).or_else(|| memory.parameters.get(name)).cloned();
    let missing_parameters = names.iter().any(|(name, _)| value_of(name).is_none());
    if !expanded.missing.is_empty() || (!names.is_empty() && (given.parameters.is_none() || missing_parameters)) {
        let inferred = inferred_of(session, connection, &expanded.statements, target.dialect);
        let asked: Vec<Asked> = names
            .iter()
            .map(|(name, kind)| Asked {
                name: name.clone(),
                kind: *kind,
                value: shown_value(name, value_of(name).as_ref()),
                secret: params::secret_name(name),
                inferred: inferred.get(name).copied().unwrap_or_default(),
            })
            .collect();
        let variables = variables_of(&expanded);
        let count = asked.len() + variables.len();
        let (command, params) = retry;
        return Err(CommandError::with_code(PARAMETERS_NEEDED, t!("database-error-parameters-needed", count = count))
            .with_data(serde_json::json!({
                "parameters": asked,
                "variables": variables,
                "inlined": !target.binds,
                "retry": { "command": command, "params": params },
            })));
    }
    let mut values: BTreeMap<String, Value> = BTreeMap::new();
    for (name, _) in &names {
        let value = value_of(name).unwrap_or_default();
        values.insert(name.clone(), value.to_value().map_err(|why| invalid(name, &param_error(&why)))?);
    }
    let mut prepared = Prepared::default();
    for (index, mut statement) in expanded.statements.into_iter().enumerate() {
        if !statement.client {
            let kept = statement.kept.take().unwrap_or_else(|| statement.text.clone());
            let bound = params::bind(&statement.text, target.dialect, (target.sql, target.binds), &values)
                .map_err(|missing| invalid(&missing.join(", "), &t!("database-param-error-no-value")))?;
            if !bound.inlined.is_empty() {
                for shown in params::shown(&bound.inlined, target.sql) {
                    if !prepared.inlined.contains(&shown) {
                        prepared.inlined.push(shown);
                    }
                }
                prepared.notes.push(Message {
                    statement: Some(index),
                    severity: "INFO".to_owned(),
                    text: t!(
                        "database-parameters-inlined",
                        values = params::shown(&bound.inlined, target.sql).join(", ")
                    ),
                    code: Some("parametersInlined".to_owned()),
                });
            }
            statement.text = bound.text;
            statement.params = bound.params;
            if kept != statement.text {
                statement.kept = Some(kept);
            }
        }
        prepared.statements.push(statement);
    }
    if let Some(path) = place {
        memory.variables.extend(expanded.variables.iter().map(|(name, value)| (name.clone(), value.clone())));
        memory.parameters.extend(given_values);
        let before = remembered(session, path);
        if before != memory {
            store(session, path, memory)?;
        }
    }
    Ok(prepared)
}

/// What `db.console.parameters` lists.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ParametersParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// Which statements: `statement` (the selection, else the statement under the caret), `script` or `fromCaret`.
    pub scope: Scope,
    /// A character offset to take the statement at instead of the caret.
    pub offset: Option<usize>,
}

/// The placeholders and variables of the statements a run would take.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParametersAnswer {
    /// The placeholders, each once, with the value the console remembers.
    pub parameters: Vec<Asked>,
    /// The `${name}` variables they use or set, with their values.
    pub variables: Vec<AskedVariable>,
    /// Whether values are written into the text as quoted literals because the driver has no bind parameters.
    pub inlined: bool,
}

pub struct ListParameters;

impl Command for ListParameters {
    type Params = ParametersParams;
    type Output = ParametersAnswer;
    const ID: &'static str = "db.console.parameters";
    const TITLE: &'static str = "List Query Parameters";
    const DESCRIPTION: &'static str = "Lists the bind parameters (`:name`; `$1` on PostgreSQL; `?` elsewhere; never inside strings, comments, quoted names or dollar quotes) and the `${name}` variables of the statements a run of a console or `.sql` file would take (`scope`: `statement`, `script`, `fromCaret`), with the values the console remembers and `inferred`, the type each placeholder's place suggests (`number` next to a number, in arithmetic, after `LIMIT` or against a numeric column of the cached structure; `boolean`; a cast's type; else `text`); `@set` lines and `@include` files are applied as a run applies them. Secret-looking values (`password`, `token`…) are never answered. Pass values to db.console.execute, db.console.executeScript, db.console.explain, db.script.run or db.script.runOnConnections as `parameters` and `variables`. Connects nowhere.";

    fn run(&self, session: &mut Session, params: ParametersParams) -> Result<ParametersAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        let (connection, statements) = consoles::pick_bound(session, &path, (params.scope, params.offset))?;
        let target = target(session, &connection);
        let memory = remembered(session, &path);
        let expanded = expand(session, Some(&path), target.dialect, statements, memory.variables.clone())?;
        let inferred = inferred_of(session, &connection, &expanded.statements, target.dialect);
        let parameters = asked_of(&expanded.statements, target.dialect)
            .into_iter()
            .take(MOST_LISTED)
            .map(|(name, kind)| Asked {
                value: shown_value(&name, memory.parameters.get(&name)),
                secret: params::secret_name(&name),
                inferred: inferred.get(&name).copied().unwrap_or_default(),
                name,
                kind,
            })
            .collect();
        let variables = variables_of(&expanded);
        Ok(ParametersAnswer { parameters, variables, inlined: !target.binds })
    }
}

/// Which console's variables to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct VariablesParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// The most variables and parameters answered of each; 200 when absent.
    pub limit: Option<usize>,
}

/// A remembered parameter value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RememberedParameter {
    /// The placeholder's name.
    pub name: String,
    /// Its value; absent for a secret-looking name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<ParamValue>,
    /// Whether its name looks like it holds a secret.
    pub secret: bool,
}

/// The variables and parameter values a console remembers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VariablesAnswer {
    /// The console or file.
    pub path: PathBuf,
    /// Its `${name}` variables.
    pub variables: Vec<AskedVariable>,
    /// The values of bind parameters it remembers.
    pub parameters: Vec<RememberedParameter>,
    /// Whether some were left out past `limit`.
    pub truncated: bool,
}

fn answer(session: &mut Session, path: &Path, limit: Option<usize>) -> VariablesAnswer {
    let limit = limit.unwrap_or(MOST_LISTED).max(1);
    let memory = remembered(session, path);
    let truncated = memory.variables.len() > limit || memory.parameters.len() > limit;
    let variables = memory
        .variables
        .iter()
        .take(limit)
        .map(|(name, value)| AskedVariable {
            name: name.clone(),
            value: (!params::secret_name(name)).then(|| value.clone()),
            secret: params::secret_name(name),
        })
        .collect();
    let parameters = memory
        .parameters
        .iter()
        .take(limit)
        .map(|(name, value)| RememberedParameter {
            name: name.clone(),
            value: shown_value(name, Some(value)),
            secret: params::secret_name(name),
        })
        .collect();
    VariablesAnswer { path: path.to_owned(), variables, parameters, truncated }
}

pub struct ListVariables;

impl Command for ListVariables {
    type Params = VariablesParams;
    type Output = VariablesAnswer;
    const ID: &'static str = "db.console.variables";
    const TITLE: &'static str = "List Script Variables";
    const DESCRIPTION: &'static str = "Lists the `${name}` variables (set by `@set name = value`, by db.console.setVariable or given with a run) and the bind parameter values a console or `.sql` file (the active one) remembers, at most `limit` (200) of each with `truncated`. They are kept across restarts in `<data folder>/databases/variables.json`, except secret-looking ones (`password`, `token`, `secret`, `api_key`…), which live only until the IDE closes and are never answered. Connects nowhere.";

    fn run(&self, session: &mut Session, params: VariablesParams) -> Result<VariablesAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        Ok(answer(session, &path, params.limit))
    }
}

/// The variable to set.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SetVariableParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// The variable's name.
    pub name: String,
    /// Its value; absent to remove it.
    pub value: Option<String>,
}

pub struct SetVariable;

impl Command for SetVariable {
    type Params = SetVariableParams;
    type Output = VariablesAnswer;
    const ID: &'static str = "db.console.setVariable";
    const TITLE: &'static str = "Set Script Variable";
    const DESCRIPTION: &'static str = "Sets the `${name}` variable of a console or `.sql` file (the active one) to `value`, as `@set name = value` does, or removes it when `value` is absent. Statements get its value written into their text before they run (a variable is text, such as a table name; values belong in bind parameters). Answers the console's variables as db.console.variables.";

    fn run(&self, session: &mut Session, params: SetVariableParams) -> Result<VariablesAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        let name = params.name.trim().trim_start_matches("${").trim_end_matches('}').to_owned();
        if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(CommandError::with_code("invalidName", t!("database-error-variable-name", name = name)));
        }
        let mut memory = remembered(session, &path);
        match params.value {
            Some(value) => memory.variables.insert(name, value),
            None => memory.variables.remove(&name),
        };
        store(session, &path, memory)?;
        Ok(answer(session, &path, None))
    }
}

/// The parameter value to remember.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SetParameterParams {
    /// The console or `.sql` file; the active one when absent.
    pub path: Option<PathBuf>,
    /// The placeholder's name (`id`, `$1`, `?1`).
    pub name: String,
    /// The value: a string, number, boolean or null, or `{"type": …, "value": …}`.
    pub value: Option<serde_json::Value>,
    /// Forget the value instead.
    pub forget: bool,
}

pub struct SetParameter;

impl Command for SetParameter {
    type Params = SetParameterParams;
    type Output = VariablesAnswer;
    const ID: &'static str = "db.console.setParameter";
    const TITLE: &'static str = "Set Query Parameter";
    const DESCRIPTION: &'static str = "Remembers `value` for the bind parameter `name` of a console or `.sql` file (the active one), as the parameters dialog does, or forgets it with `forget`. A value is a string, number, boolean or null, or `{\"type\": \"text\" | \"number\" | \"boolean\" | \"null\", \"value\": \"…\"}`; a number or boolean that does not read as one is refused with code `invalidParameter`. Answers the console's variables and parameters as db.console.variables.";

    fn run(&self, session: &mut Session, params: SetParameterParams) -> Result<VariablesAnswer, CommandError> {
        let path = console_path(session, params.path)?;
        let name = params.name.trim().trim_start_matches(':').to_owned();
        if name.is_empty() {
            return Err(CommandError::with_code("invalidName", t!("database-error-variable-name", name = name)));
        }
        let mut memory = remembered(session, &path);
        if params.forget {
            memory.parameters.remove(&name);
        } else {
            let json = params.value.unwrap_or(serde_json::Value::Null);
            let value = ParamValue::of_json(&json).map_err(|why| invalid(&name, &param_error(&why)))?;
            value.to_value().map_err(|why| invalid(&name, &param_error(&why)))?;
            memory.parameters.insert(name, value);
        }
        store(session, &path, memory)?;
        Ok(answer(session, &path, None))
    }
}

pub fn positional(values: &[Value], dialect: SqlDialect) -> Vec<NamedValue> {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| NamedValue {
            name: match dialect {
                SqlDialect::Postgres => format!("${}", index + 1),
                _ => format!("?{}", index + 1),
            },
            value: value.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests;

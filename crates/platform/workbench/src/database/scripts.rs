use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::engine::{ConnectionState, TransactionPolicy};
use zen_db::sql::Dialect;
use zen_i18n::t;

use super::consoles::{self, Binding, RunStarted, Scope};
use super::safety::Confirmation;
use super::sources::{Database, connection_types, files, find};
use super::{WORKSPACE, project_key};
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams, Workspaces};
use crate::{Workbench, display};

const CHOICES: usize = 50;

const TARGETS: usize = 2000;

const LISTED: usize = 100;

const LABEL: usize = 60;

/// The connection and schema a script of the project is bound to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptBinding {
    /// The connection's id.
    pub connection: String,
    /// The schema its statements run in; the connection's default when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BindingsFile {
    scripts: BTreeMap<String, ScriptBinding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    default: Option<ScriptBinding>,
}

#[derive(Default)]
pub struct Bindings {
    read: Mutex<Option<(PathBuf, BindingsFile)>>,
}

fn bindings_file(session: &Session) -> Option<PathBuf> {
    let data = session.state::<Workspaces>()?.data_dir()?;
    let project = session.state::<Workbench>()?.project()?;
    Some(data.join("databases").join("scripts").join(format!("{}.json", project_key(project))))
}

fn read_file(file: &Path) -> BindingsFile {
    std::fs::read_to_string(file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
}

fn key_of(session: &Session, path: &Path) -> String {
    let project = session.state::<Workbench>().and_then(Workbench::project);
    match project.and_then(|root| path.strip_prefix(root).ok()) {
        Some(relative) => {
            relative.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
        }
        None => path.display().to_string(),
    }
}

fn path_of(session: &Session, key: &str) -> PathBuf {
    let path = PathBuf::from(key);
    if path.is_absolute() {
        return path;
    }
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf).unwrap_or_default();
    key.split('/').fold(root, |path, part| path.join(part))
}

fn with_bindings<T>(session: &Session, use_them: impl FnOnce(&BindingsFile) -> T) -> Option<T> {
    let file = bindings_file(session)?;
    let Some(bindings) = session.state::<Bindings>() else { return Some(use_them(&read_file(&file))) };
    let mut read = bindings.read.lock().unwrap_or_else(PoisonError::into_inner);
    if read.as_ref().is_none_or(|(known, _)| *known != file) {
        *read = Some((file.clone(), read_file(&file)));
    }
    read.as_ref().map(|(_, bindings)| use_them(bindings))
}

pub fn bound(session: &Session, path: &Path) -> Option<ScriptBinding> {
    let key = key_of(session, path);
    with_bindings(session, |bindings| bindings.scripts.get(&key).cloned()).flatten()
}

pub(crate) fn all_bound(session: &Session) -> Vec<(PathBuf, ScriptBinding)> {
    let keys: Vec<(String, ScriptBinding)> = with_bindings(session, |bindings| {
        bindings.scripts.iter().map(|(key, binding)| (key.clone(), binding.clone())).collect()
    })
    .unwrap_or_default();
    keys.into_iter().map(|(key, binding)| (path_of(session, &key), binding)).collect()
}

pub fn store(session: &mut Session, path: &Path, binding: Option<ScriptBinding>) -> Result<(), CommandError> {
    let key = key_of(session, path);
    write_bindings(session, |bindings| match binding {
        Some(binding) => {
            bindings.scripts.insert(key, binding);
        }
        None => {
            bindings.scripts.remove(&key);
        }
    })
}

pub fn project_default(session: &Session) -> Option<ScriptBinding> {
    with_bindings(session, |bindings| bindings.default.clone()).flatten()
}

pub fn store_default(session: &mut Session, binding: Option<ScriptBinding>) -> Result<(), CommandError> {
    write_bindings(session, |bindings| bindings.default = binding)
}

fn write_bindings(session: &mut Session, change: impl FnOnce(&mut BindingsFile)) -> Result<(), CommandError> {
    let file = bindings_file(session).ok_or_else(|| CommandError::new(t!("database-error-no-data-folder")))?;
    let mut changed = with_bindings(session, BindingsFile::clone).unwrap_or_default();
    change(&mut changed);
    let text = serde_json::to_string_pretty(&changed).unwrap_or_default();
    file.parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&file, text))
        .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
    *session.state_mut::<Bindings>().read.lock().unwrap_or_else(PoisonError::into_inner) = Some((file, changed));
    Ok(())
}

/// A connection a script can be bound to.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    /// The connection's id.
    pub id: String,
    /// Its name.
    pub name: String,
}

fn choices(session: &Session) -> (Vec<Choice>, usize) {
    let entries = files(session).entries().unwrap_or_default();
    let total = entries.len();
    let listed = entries
        .into_iter()
        .take(CHOICES)
        .map(|entry| Choice { id: entry.source.id, name: entry.source.name })
        .collect();
    (listed, total)
}

pub fn not_bound(session: &Session, path: &Path) -> CommandError {
    let (connections, total) = choices(session);
    let message = super::for_caller(
        session,
        t!("database-error-script-not-bound", path = display(path)),
        t!("database-error-script-not-bound-agent", path = display(path)),
    );
    CommandError::with_code("noBinding", message).with_data(
        serde_json::json!({ "path": path, "connections": connections, "total": total, "truncated": total > CHOICES }),
    )
}

pub(super) fn script_path(session: &Session, path: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    let path = path.map(|path| match session.state::<Workbench>() {
        Some(workbench) => workbench.resolve(&path),
        None => path,
    });
    consoles::resolve(session, path)
}

fn given<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

/// What `db.script.bind` changes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BindParams {
    /// The script or console: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
    /// The connection's id; `null` unbinds a script; absent keeps the connection.
    #[serde(deserialize_with = "given")]
    pub connection: Option<Option<String>>,
    /// The schema; `null` goes back to the connection's default; absent keeps it (or forgets it when the connection changes).
    #[serde(deserialize_with = "given")]
    pub schema: Option<Option<String>>,
    /// Changes pending on the connection it leaves: `ask` (default) refuses with code `openTransactions`, `commit` or `rollback` ends them first.
    pub transactions: TransactionPolicy,
}

pub struct BindScript;

impl Command for BindScript {
    type Params = BindParams;
    type Output = Binding;
    const ID: &'static str = "db.script.bind";
    const TITLE: &'static str = "Bind Script to Connection";
    const DESCRIPTION: &'static str = "Binds a `.sql` file of the project or a console (the active one) to a connection (`connection`, an id from db.dataSource.list) and schema (`schema`), as the selectors above the editor and in the Database workspace do. A script's binding is kept per project in the IDE's data folder (`databases/scripts/<project key>.json`), never in the file or the repository; `connection: null` unbinds it. Leaving a connection closes the script's session there: changes pending refuse it with code `openTransactions` unless `transactions` is `commit` or `rollback`. Connects nowhere. Answers the binding.";

    fn run(&self, session: &mut Session, params: BindParams) -> Result<Binding, CommandError> {
        let path = script_path(session, params.path)?;
        let mut binding = match params.connection {
            Some(Some(connection)) => {
                consoles::choose_connection(session, &path, connection, params.transactions, Self::ID)?
            }
            Some(None) if !consoles::is_console(session, &path) => {
                if consoles::connection_of(session, &path).is_some() {
                    let retry = serde_json::json!({ "path": path, "connection": null });
                    super::transactions::close_console_session(
                        session,
                        &path,
                        params.transactions,
                        Some((Self::ID, retry)),
                    )?;
                }
                store(session, &path, None)?;
                Binding { path: path.clone(), connection: None, schema: None }
            }
            Some(None) => {
                return Err(CommandError::with_code("console", t!("database-error-console-unbind")));
            }
            None => Binding {
                path: path.clone(),
                connection: consoles::connection_of(session, &path),
                schema: consoles::schema_of(session, &path),
            },
        };
        if let Some(schema) = params.schema {
            binding = consoles::choose_schema(session, &path, schema)?;
        }
        Ok(binding)
    }
}

/// Which script.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptParams {
    /// The script or console: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
}

/// A script's binding as the selectors above the editor show it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptState {
    /// The script.
    pub path: PathBuf,
    /// Whether it is a console of the data folder rather than a file of the project.
    pub console: bool,
    /// Whether it has a connection to run on.
    pub bound: bool,
    /// Its connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// The connection's name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection_name: Option<String>,
    /// The connection type's colour (`red`, `green`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// The connection's state.
    pub state: ConnectionState,
    /// Its schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// The dialect its text is split and completed in, the connection's.
    pub dialect: Dialect,
    /// Whether it is open in the Database workspace, where its results show.
    pub in_workspace: bool,
}

fn open_in_workspace(session: &Session, path: &Path) -> bool {
    session
        .state::<Workspaces>()
        .is_some_and(|workspaces| workspaces.tabs(WORKSPACE).iter().any(|tab| tab.path.as_deref() == Some(path)))
}

fn state_of(session: &mut Session, path: &Path) -> ScriptState {
    let connection = consoles::connection_of(session, path);
    let entry = connection.as_deref().and_then(|id| find(session, id).ok());
    let color = entry.as_ref().and_then(|entry| {
        let (types, _) = connection_types(session);
        Some(types.resolve(&entry.source.connection_type).color.clone()).filter(|color| color != "none")
    });
    let state = match (session.state::<Database>().and_then(Database::started), &connection) {
        (Some(engine), Some(id)) => engine.status(id).state,
        _ => ConnectionState::Disconnected,
    };
    let dialect = consoles::dialect_of(session, connection.as_deref());
    ScriptState {
        path: path.to_owned(),
        console: consoles::is_console(session, path),
        bound: connection.is_some(),
        connection_name: entry.map(|entry| entry.source.name),
        color,
        state,
        schema: consoles::schema_of(session, path),
        dialect,
        in_workspace: open_in_workspace(session, path),
        connection,
    }
}

pub struct ScriptStateCommand;

impl Command for ScriptStateCommand {
    type Params = ScriptParams;
    type Output = ScriptState;
    const ID: &'static str = "db.script.state";
    const TITLE: &'static str = "Script State";
    const DESCRIPTION: &'static str = "The binding of a `.sql` file of the project or a console (the active one): whether it is bound, its connection (id, name, state, type colour), its schema, the dialect it is split and completed in (the connection's), and whether it is open in the Database workspace. Never connects; results are read with db.console.state.";

    fn run(&self, session: &mut Session, params: ScriptParams) -> Result<ScriptState, CommandError> {
        let path = script_path(session, params.path)?;
        Ok(state_of(session, &path))
    }
}

/// How to run a script.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunParams {
    /// The script or console: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
    /// What to run: `statement` (the selection, else the statement under the caret; the default), `script` or `fromCaret`.
    pub scope: Scope,
    /// A character offset to run at instead of the caret and selection, as a run icon in the gutter does.
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

/// What a script's run started.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRun {
    /// The run.
    #[serde(flatten)]
    pub run: RunStarted,
    /// The script's tab in the Database workspace, where its results show.
    pub tab: InnerTab,
}

pub struct RunScript;

impl Command for RunScript {
    type Params = RunParams;
    type Output = ScriptRun;
    const ID: &'static str = "db.script.run";
    const TITLE: &'static str = "Run SQL Script";
    const DESCRIPTION: &'static str = "Runs a `.sql` file of the project (the active one) on the connection and schema it is bound to, as ⌘⏎ and the run icons in the gutter of a `.sql` file do: `scope` `statement` (the selection, else the statement under the caret or at `offset`), `script` or `fromCaret`. The file opens as a tab of the Database workspace, which expands and shows its results below it. Never runs without a binding: an unbound script is refused with code `noBinding` and the connections to choose from (bind with db.script.bind). Answers the query id at once (db.query.state, db.console.state). Confirmations and refusals as db.console.execute (`confirmed`, `skip`).";

    fn run(&self, session: &mut Session, params: RunParams) -> Result<ScriptRun, CommandError> {
        let path = script_path(session, params.path)?;
        if consoles::connection_of(session, &path).is_none() {
            return Err(if consoles::is_console(session, &path) {
                CommandError::with_code("noConnection", super::no_console_connection(session))
            } else {
                not_bound(session, &path)
            });
        }
        let open =
            OpenTabParams { workspace: WORKSPACE.to_owned(), path: Some(path.clone()), ..OpenTabParams::default() };
        let tab = OpenTab.run(session, open)?;
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

/// Which run icons to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TargetsParams {
    /// The script: absolute, or relative to the project; the active one when absent.
    pub path: Option<PathBuf>,
    /// The most statements to answer; 2000 when absent.
    pub limit: Option<usize>,
}

/// A statement of a script, where its run icon is drawn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// The line it starts on, one-based.
    pub line: usize,
    /// Its first character, to run it with db.script.run's `offset`.
    pub offset: usize,
    /// Its first line, cut to 60 characters.
    pub label: String,
}

/// The statements of a script.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Targets {
    /// The statements, at most `limit`, in order.
    pub targets: Vec<Target>,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct ScriptTargets;

impl Command for ScriptTargets {
    type Params = TargetsParams;
    type Output = Targets;
    const ID: &'static str = "db.script.targets";
    const TITLE: &'static str = "List Script Statements";
    const DESCRIPTION: &'static str = "The statements of a `.sql` file (the active one) as the gutter shows their run icons: each one's first line (one-based), first character and first line of text cut to 60 characters, split in the dialect of the connection the file is bound to; at most `limit` (2000) with `total` and `truncated`. Client commands (psql meta-commands) are left out. Never connects.";

    fn run(&self, session: &mut Session, params: TargetsParams) -> Result<Targets, CommandError> {
        let path = script_path(session, params.path)?;
        let limit = params.limit.unwrap_or(TARGETS);
        let split = consoles::SplitScript.run(
            session,
            consoles::SplitParams { path: Some(path), limit: Some(usize::MAX), ..consoles::SplitParams::default() },
        )?;
        let found: Vec<Target> = split
            .statements
            .into_iter()
            .filter(|statement| !statement.client && !statement.text.trim().is_empty())
            .map(|statement| {
                let first = statement.text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
                let mut label: String = first.chars().take(LABEL).collect();
                if first.chars().count() > LABEL {
                    label.push('…');
                }
                Target { line: statement.line, offset: statement.start, label }
            })
            .collect();
        let total = found.len();
        Ok(Targets { targets: found.into_iter().take(limit).collect(), total, truncated: total > limit })
    }
}

/// Which scripts to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// Only the scripts and consoles of this connection.
    pub connection: Option<String>,
    /// The most entries to answer; 100 when absent.
    pub limit: Option<usize>,
}

/// A script or console of a connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptEntry {
    /// Its file.
    pub path: PathBuf,
    /// Its name.
    pub name: String,
    /// Its place: relative to the project for a script, `consoles/<connection folder>` for a console.
    pub place: String,
    /// Whether it is a console of the data folder rather than a file of the project.
    pub console: bool,
    /// The connection it runs on.
    pub connection: String,
    /// Its schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Whether its file exists.
    pub exists: bool,
    /// Whether it is open in the Database workspace.
    pub open: bool,
}

/// The scripts and consoles of connections.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScriptList {
    /// The entries: scripts of the project by path, then consoles, at most `limit`.
    pub scripts: Vec<ScriptEntry>,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

pub struct ListScripts;

impl Command for ListScripts {
    type Params = ListParams;
    type Output = ScriptList;
    const ID: &'static str = "db.script.list";
    const TITLE: &'static str = "List Scripts";
    const DESCRIPTION: &'static str = "The scripts of the open project bound to a connection and the consoles that run on one (only those of `connection` when given), as the navigator's SQL Editor → Scripts lists them: each one's path, name, place, connection, schema, whether its file exists and whether it is open in the Database workspace; scripts by path, then consoles, at most `limit` (100) with `total` and `truncated`. Never reads their text.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ScriptList, CommandError> {
        let limit = params.limit.unwrap_or(LISTED);
        let wanted = params.connection.filter(|id| !id.trim().is_empty());
        if let Some(id) = &wanted {
            find(session, id)?;
        }
        let bound: Vec<(String, ScriptBinding)> = with_bindings(session, |bindings| {
            bindings.scripts.iter().map(|(key, binding)| (key.clone(), binding.clone())).collect()
        })
        .unwrap_or_default();
        let mut found = Vec::new();
        for (key, binding) in bound {
            if wanted.as_ref().is_some_and(|id| *id != binding.connection) {
                continue;
            }
            let path = path_of(session, &key);
            found.push(ScriptEntry {
                name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                place: key,
                console: false,
                connection: binding.connection,
                schema: binding.schema,
                exists: path.is_file(),
                open: open_in_workspace(session, &path),
                path,
            });
        }
        let consoles = super::ListConsoles
            .run(session, super::ListConsolesParams { connection: None, limit: Some(usize::MAX) })
            .map(|listed| listed.consoles)
            .unwrap_or_default();
        let mut listed_consoles = Vec::new();
        for console in consoles {
            let Some(connection) = consoles::connection_of(session, &console.path) else { continue };
            if wanted.as_ref().is_some_and(|id| *id != connection) {
                continue;
            }
            listed_consoles.push(ScriptEntry {
                place: format!("consoles/{}", console.connection),
                schema: consoles::schema_of(session, &console.path),
                exists: true,
                open: console.open,
                console: true,
                connection,
                name: console.name,
                path: console.path,
            });
        }
        found.extend(listed_consoles);
        let total = found.len();
        Ok(ScriptList { scripts: found.into_iter().take(limit).collect(), total, truncated: total > limit })
    }
}

/// What `db.script.new` creates.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct NewParams {
    /// The connection the script is bound to.
    pub connection: String,
    /// The folder of the project it goes in: relative to the project or absolute inside it; the project's folder when absent.
    pub folder: Option<PathBuf>,
    /// Its file name, `.sql` added when missing; `script.sql`, `script_1.sql`, ... when absent.
    pub name: Option<String>,
    /// The schema it is bound to.
    pub schema: Option<String>,
    /// The text it starts with.
    pub text: Option<String>,
}

/// A script created.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NewScript {
    /// Its file.
    pub path: PathBuf,
    /// Its binding.
    pub binding: Binding,
    /// Its tab in the Database workspace.
    pub tab: InnerTab,
}

fn invalid(path: &Path) -> CommandError {
    CommandError::with_code("invalidPath", t!("database-error-script-folder", path = display(path)))
}

fn script_folder(session: &Session, folder: Option<PathBuf>) -> Result<PathBuf, CommandError> {
    let root = session
        .state::<Workbench>()
        .and_then(Workbench::project)
        .map(Path::to_path_buf)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
    let folder = match folder {
        Some(folder) if folder.is_absolute() => folder,
        Some(folder) => root.join(folder),
        None => root.clone(),
    };
    super::guard_agent_new_path(session, &folder)?;
    let inside = folder.components().all(|part| !matches!(part, std::path::Component::ParentDir));
    if !inside || !folder.starts_with(&root) {
        return Err(invalid(&folder));
    }
    let hidden = folder
        .strip_prefix(&root)
        .is_ok_and(|relative| relative.components().any(|part| part.as_os_str().to_string_lossy().starts_with('.')));
    if hidden {
        return Err(invalid(&folder));
    }
    Ok(folder)
}

pub struct NewScriptCommand;

impl Command for NewScriptCommand {
    type Params = NewParams;
    type Output = NewScript;
    const ID: &'static str = "db.script.new";
    const TITLE: &'static str = "New SQL Script";
    const DESCRIPTION: &'static str = "Creates a `.sql` file in a folder of the project (`folder`, relative to the project; its root when absent; never outside it or in a hidden folder such as `.git`), named `name` (`.sql` added) or `script.sql`, `script_1.sql`, ..., with `text`; binds it to `connection` and `schema` and opens it as a tab of the Database workspace. Never replaces a file: a name that exists is refused with code `exists`. Connects nowhere. Answers its path, binding and tab.";

    fn run(&self, session: &mut Session, params: NewParams) -> Result<NewScript, CommandError> {
        find(session, &params.connection)?;
        let folder = script_folder(session, params.folder)?;
        let failed = |error: std::io::Error| {
            CommandError::new(t!("workbench-error-create", path = display(&folder), error = error.to_string()))
        };
        std::fs::create_dir_all(&folder).map_err(failed)?;
        let text = params.text.unwrap_or_default();
        let path = match params.name.map(|name| name.trim().to_owned()).filter(|name| !name.is_empty()) {
            Some(name) => {
                let plain =
                    !name.starts_with('.') && !name.contains(['/', '\\', ':']) && !name.chars().any(char::is_control);
                if !plain {
                    return Err(invalid(&folder.join(&name)));
                }
                let name =
                    if Path::new(&name).extension().is_some_and(|extension| extension.eq_ignore_ascii_case("sql")) {
                        name
                    } else {
                        format!("{name}.sql")
                    };
                let path = folder.join(name);
                super::guard_agent_new_path(session, &path)?;
                match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                    Ok(mut file) => std::io::Write::write_all(&mut file, text.as_bytes()).map_err(failed)?,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                        return Err(CommandError::with_code(
                            "exists",
                            t!("database-error-script-exists", path = display(&path)),
                        ));
                    }
                    Err(error) => return Err(failed(error)),
                }
                path
            }
            None => crate::scratch::create_new(&folder, "script", "sql", &text).map_err(failed)?,
        };
        let schema = params.schema.filter(|schema| !schema.trim().is_empty());
        store(session, &path, Some(ScriptBinding { connection: params.connection.clone(), schema: schema.clone() }))?;
        let open =
            OpenTabParams { workspace: WORKSPACE.to_owned(), path: Some(path.clone()), ..OpenTabParams::default() };
        let tab = OpenTab.run(session, open)?;
        session.state_mut::<Workbench>().revision += 1;
        let binding = Binding { path: path.clone(), connection: Some(params.connection), schema };
        Ok(NewScript { path, binding, tab })
    }
}

#[cfg(test)]
mod tests;

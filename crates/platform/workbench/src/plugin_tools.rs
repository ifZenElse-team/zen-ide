use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::Configuration;
use zen_syntax::{ToolAction, ToolActionKind, ToolChoice, ToolRow, ToolSection, ToolWindowDeclaration, localized};

use crate::notifications::{Group, Kind, NewNotification};
use crate::plugin_runs::{CONFIRMATION_NEEDED, KIND, PluginRunOptions};

pub mod inputs;
pub mod state;

use inputs::Inputs;

pub const BACKOFF: Duration = Duration::from_secs(30);

const LIST_TIMEOUT: Duration = Duration::from_secs(20);

const CHANGE_TIMEOUT: Duration = Duration::from_secs(120);

const MAX_OUTPUT: usize = 4000;

const MAX_NAMED_ROWS: usize = 10;

pub const CHOICE_NEEDED: &str = "choiceNeeded";

pub const CHOICES_FILE: &str = "plugin-tool-choices.json";

type Key = (String, String, String);

type WindowKey = (String, String);

type Values = BTreeMap<String, String>;

#[derive(Clone, Debug, Default)]
struct Chosen {
    options: Vec<ToolRow>,
    value: Option<String>,
    valid: bool,
    note: Option<String>,
}

struct Done {
    rows: Result<Vec<ToolRow>, String>,
    choices: Vec<(String, Chosen)>,
    values: Values,
    notes: Vec<String>,
}

type Listing = Arc<Mutex<Option<Done>>>;

#[derive(Default)]
struct Listed {
    rows: Vec<ToolRow>,
    values: Values,
    error: Option<String>,
    notes: Vec<String>,
    failed_at: Option<Instant>,
    listed: bool,
    pending: Option<Listing>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Remembered {
    projects: BTreeMap<String, BTreeMap<String, Values>>,
}

type Running = Arc<Mutex<Option<Vec<Outcome>>>>;

#[derive(Default)]
struct ToolState {
    sections: BTreeMap<Key, Listed>,
    running: BTreeMap<Key, Running>,
    asked: BTreeMap<String, Key>,
    allowed: BTreeSet<String>,
    next: u64,
    folders: Option<Vec<PathBuf>>,
    choices: BTreeMap<WindowKey, BTreeMap<String, Chosen>>,
    remembered: Option<Remembered>,
    choices_file: Option<PathBuf>,
    wanting: BTreeMap<String, InputRequest>,
    entered: BTreeMap<String, Inputs>,
    entering: Option<InputRequest>,
}

/// An agent's action that waits for values the person types.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InputRequest {
    /// The confirmation the agent calls again with.
    pub token: String,
    /// The plug-in's id.
    pub plugin: String,
    /// The window's id.
    pub window: String,
    /// The section's id.
    pub section: String,
    /// The action's id.
    pub action: String,
    /// The rows it acts on.
    pub rows: Vec<String>,
    /// The action's question, with `${input.<id>}` left for the values the person types.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub question: Option<String>,
}

pub fn take_entering(session: &mut Session) -> Option<InputRequest> {
    session.state_mut::<ToolState>().entering.take()
}

fn choices_file(session: &Session) -> Option<PathBuf> {
    if let Some(file) = session.state::<ToolState>().and_then(|state| state.choices_file.clone()) {
        return Some(file);
    }
    session
        .state::<crate::build_tools::BuildEnv>()
        .and_then(crate::build_tools::BuildEnv::data_dir)
        .or_else(|| crate::build_tools::BuildEnv::global().data_dir())
        .map(|data| data.join(CHOICES_FILE))
}

fn window_name(window: &WindowKey) -> String {
    format!("{}/{}", window.0, window.1)
}

fn remembered(session: &mut Session, root: &Path, window: &WindowKey) -> Values {
    let file = choices_file(session);
    let state = session.state_mut::<ToolState>();
    let saved = state.remembered.get_or_insert_with(|| {
        file.and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    });
    saved
        .projects
        .get(&root.to_string_lossy().into_owned())
        .and_then(|windows| windows.get(&window_name(window)))
        .cloned()
        .unwrap_or_default()
}

fn remember(session: &mut Session, root: &Path, window: &WindowKey) {
    remembered(session, root, window);
    let file = choices_file(session);
    let state = session.state_mut::<ToolState>();
    let values: Values = state
        .choices
        .get(window)
        .map(|choices| choices.iter().filter_map(|(id, chosen)| Some((id.clone(), chosen.value.clone()?))).collect())
        .unwrap_or_default();
    let Some(saved) = state.remembered.as_mut() else { return };
    let windows = saved.projects.entry(root.to_string_lossy().into_owned()).or_default();
    windows.insert(window_name(window), values);
    if let Some(file) = file
        && let Ok(bytes) = serde_json::to_vec_pretty(saved)
    {
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(file, bytes);
    }
}

fn values_of(session: &Session, window: &WindowKey) -> Values {
    session
        .state::<ToolState>()
        .and_then(|state| state.choices.get(window))
        .map(|choices| choices.iter().filter_map(|(id, chosen)| Some((id.clone(), chosen.value.clone()?))).collect())
        .unwrap_or_default()
}

fn wanted(session: &mut Session, root: &Path, window: &WindowKey) -> Values {
    if !session.state::<ToolState>().is_some_and(|state| state.choices.contains_key(window)) {
        let saved = remembered(session, root, window);
        let choices =
            saved.into_iter().map(|(id, value)| (id, Chosen { value: Some(value), ..Chosen::default() })).collect();
        session.state_mut::<ToolState>().choices.insert(window.clone(), choices);
    }
    values_of(session, window)
}

pub fn set_folders(session: &mut Session, folders: Vec<PathBuf>) {
    session.state_mut::<ToolState>().folders = Some(folders);
}

fn settle(session: &mut Session, key: &Key) -> bool {
    let state = session.state_mut::<ToolState>();
    let Some(running) = state.running.get(key) else { return false };
    let Some(outcomes) = running.lock().ok().and_then(|mut done| done.take()) else { return true };
    state.running.remove(key);
    for outcome in outcomes.iter().filter(|outcome| !outcome.ok) {
        let body = t!("workbench-tool-failed", action = outcome.action.as_str(), error = outcome.output.as_str());
        crate::notifications::post(session, NewNotification::message(Group::General, Kind::Error, body));
    }
    if let Some(listed) = session.state_mut::<ToolState>().sections.get_mut(key) {
        listed.listed = false;
        listed.failed_at = None;
    }
    false
}

fn window_of(
    plugin: &str,
    window: &str,
) -> Result<(Arc<dyn zen_syntax::TextFeatures>, ToolWindowDeclaration), CommandError> {
    let found = crate::plugin_runs::plugin_named(plugin)
        .ok_or_else(|| CommandError::new(t!("workbench-tool-unknown-plugin", plugin = plugin)))?;
    let declared = found
        .tool_windows()
        .into_iter()
        .find(|declared| declared.id == window)
        .ok_or_else(|| CommandError::new(t!("workbench-tool-unknown-window", window = window, plugin = plugin)))?;
    Ok((found, declared))
}

fn section_of<'a>(window: &'a ToolWindowDeclaration, section: &str) -> Result<&'a ToolSection, CommandError> {
    window
        .sections
        .iter()
        .find(|declared| declared.id == section)
        .ok_or_else(|| CommandError::new(t!("workbench-tool-unknown-section", section = section)))
}

fn guard_trust(session: &mut Session) -> Result<PathBuf, CommandError> {
    let root = crate::run::project_root(session)?;
    if !crate::trust::trusted(session, &root) {
        return Err(crate::trust::refused(session, &root, t!("workbench-tool-untrusted")));
    }
    Ok(root)
}

fn program(plugin: &dyn zen_syntax::TextFeatures, name: &str, folders: &[PathBuf]) -> Result<PathBuf, String> {
    if name.contains(['/', '\\']) || !plugin.runs().iter().any(|declared| declared == name) {
        let id = plugin.plugin_id().unwrap_or_default();
        return Err(t!("workbench-plugin-run-undeclared", plugin = id, program = name));
    }
    zen_lsp::servers::find_executable(name, folders).ok_or_else(|| t!("workbench-plugin-run-not-found", program = name))
}

fn environment(folders: &[PathBuf]) -> Vec<(String, String)> {
    std::env::join_paths(folders)
        .map(|path| vec![("PATH".to_owned(), path.to_string_lossy().into_owned())])
        .unwrap_or_default()
}

fn capture(
    program: &Path,
    args: &[String],
    root: &Path,
    folders: &[PathBuf],
    wait: Duration,
) -> Result<String, String> {
    let mut command = std::process::Command::new(program);
    zen_env::apply(&mut command);
    command.args(args).current_dir(root).envs(environment(folders));
    let captured = zen_run::capture(&mut command, wait)?;
    if captured.status.success() {
        return Ok(String::from_utf8_lossy(&captured.stdout).into_owned());
    }
    let mut error = String::from_utf8_lossy(&captured.stderr).trim().to_owned();
    if error.is_empty() {
        error = String::from_utf8_lossy(&captured.stdout).trim().to_owned();
    }
    Err(error.chars().take(MAX_OUTPUT).collect())
}

pub fn filled(args: &[String], row: Option<&ToolRow>, columns: &[String]) -> Result<Vec<String>, String> {
    filled_in(args, row, columns, &Values::new())
}

pub fn filled_in(
    args: &[String],
    row: Option<&ToolRow>,
    columns: &[String],
    values: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    args.iter()
        .map(|arg| {
            if let Some(name) = arg.strip_prefix("${window.").and_then(|rest| rest.strip_suffix('}')) {
                return values.get(name).cloned().ok_or_else(|| t!("workbench-tool-bad-argument", arg = arg.as_str()));
            }
            let Some(name) = arg.strip_prefix("${row.").and_then(|rest| rest.strip_suffix('}')) else {
                return if arg.contains("${row.") || arg.contains("${window.") {
                    Err(t!("workbench-tool-bad-argument", arg = arg.as_str()))
                } else {
                    Ok(arg.clone())
                };
            };
            let row = row.ok_or_else(|| t!("workbench-tool-needs-rows"))?;
            if name == "id" {
                return Ok(row.id.clone());
            }
            let index = columns
                .iter()
                .position(|column| column == name)
                .ok_or_else(|| t!("workbench-tool-bad-argument", arg = arg.as_str()))?;
            Ok(row.columns.get(index).cloned().unwrap_or_default())
        })
        .collect()
}

fn folders(session: &Session) -> Vec<PathBuf> {
    session
        .state::<ToolState>()
        .and_then(|state| state.folders.clone())
        .unwrap_or_else(zen_lsp::servers::search_folders)
}

/// A tool window a plug-in declares, as `plugins.toolWindow.list` answers it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    /// The plug-in's id.
    pub plugin: String,
    /// The window's id.
    pub window: String,
    /// Its title.
    pub title: String,
    /// Where it opens the first time: `left`, `right` or `bottom`.
    pub side: String,
    /// Whether it declares an icon of its own that the IDE can draw.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub icon: bool,
    /// What the person chooses in its header (a cluster's context and namespace), passed to every listing and action.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<ChoiceDeclared>,
    /// Its sections, each with its columns and actions.
    pub sections: Vec<SectionInfo>,
}

/// A choice of a window's header.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceDeclared {
    /// Its id, as `${window.<id>}` in the declared arguments.
    pub id: String,
    /// Its title.
    pub title: String,
}

/// A section of a plug-in's tool window.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SectionInfo {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// The names of its columns.
    pub columns: Vec<String>,
    /// Their titles, as the window shows them.
    pub column_titles: Vec<String>,
    /// What can be done.
    pub actions: Vec<ActionInfo>,
    /// `index` when its rows are the plug-in's project index (ADR-0293) and no program runs.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub rows: String,
    /// Whether its index nodes also get a cluster state (`cluster` in `plugins.toolWindow.rows`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub state: bool,
}

/// An action of a section.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ActionInfo {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// `stream` (a run tab), `terminal`, `change` or `destroy`, which asks the person every time.
    pub kind: String,
    /// Whether it acts on chosen rows rather than on the whole section.
    pub rows: bool,
    /// Whether it asks the person every time, with a question naming the window's choices and the rows.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub confirms: bool,
    /// The values the person types for it in the window's dialog; agents never pass them.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<inputs::InputInfo>,
}

fn kind_name(kind: ToolActionKind) -> &'static str {
    match kind {
        ToolActionKind::Stream => "stream",
        ToolActionKind::Terminal => "terminal",
        ToolActionKind::Change => "change",
        ToolActionKind::Destroy => "destroy",
    }
}

fn confirms(action: &ToolAction) -> bool {
    matches!(action.kind, ToolActionKind::Change | ToolActionKind::Destroy) && !action.confirm.is_empty()
}

pub fn side_of(window: &ToolWindowDeclaration) -> &'static str {
    match window.side.as_str() {
        "left" => "left",
        "right" => "right",
        _ => "bottom",
    }
}

pub fn icon(plugin: &str, window: &str) -> Option<String> {
    let declared = crate::plugin_runs::plugin_named(plugin)?.tool_windows().into_iter().find(|w| w.id == window)?;
    zen_image::icon::problem(&declared.icon).is_none().then_some(declared.icon)
}

fn info(plugin: &str, window: &ToolWindowDeclaration) -> WindowInfo {
    let language = zen_i18n::language();
    WindowInfo {
        plugin: plugin.to_owned(),
        window: window.id.clone(),
        title: localized(&window.title, &language),
        side: side_of(window).to_owned(),
        icon: zen_image::icon::problem(&window.icon).is_none(),
        choices: window
            .choices
            .iter()
            .map(|choice| ChoiceDeclared { id: choice.id.clone(), title: localized(&choice.title, &language) })
            .collect(),
        sections: window
            .sections
            .iter()
            .map(|section| SectionInfo {
                id: section.id.clone(),
                title: localized(&section.title, &language),
                columns: section.columns.clone(),
                column_titles: section
                    .columns
                    .iter()
                    .map(|column| {
                        section
                            .column_titles
                            .get(column)
                            .map(|titles| localized(titles, &language))
                            .filter(|title| !title.is_empty())
                            .unwrap_or_else(|| column.clone())
                    })
                    .collect(),
                actions: section
                    .actions
                    .iter()
                    .map(|action| ActionInfo {
                        id: action.id.clone(),
                        title: localized(&action.title, &language),
                        kind: kind_name(action.kind).to_owned(),
                        rows: action.rows,
                        confirms: confirms(action),
                        inputs: action.inputs.iter().map(|input| inputs::info(input, &language)).collect(),
                    })
                    .collect(),
                rows: section.rows.clone(),
                state: section.state.is_some(),
            })
            .collect(),
    }
}

/// The tool windows of the plug-ins.
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct WindowList {
    /// Each window, by plug-in.
    pub windows: Vec<WindowInfo>,
}

pub struct ListWindows;

impl Command for ListWindows {
    type Params = NoParams;
    type Output = WindowList;
    const ID: &'static str = "plugins.toolWindow.list";
    const TITLE: &'static str = "Plug-in Tool Windows";
    const DESCRIPTION: &'static str = "The tool windows the plug-ins declare, with their sections, columns and actions (stream, terminal, change, destroy).";

    fn run(&self, _session: &mut Session, _: NoParams) -> Result<WindowList, CommandError> {
        let windows = crate::plugin_runs::plugins()
            .into_iter()
            .filter_map(|plugin| Some((plugin.plugin_id()?.to_owned(), plugin.tool_windows())))
            .flat_map(|(id, windows)| windows.into_iter().map(move |window| info(&id, &window)))
            .collect();
        Ok(WindowList { windows })
    }
}

/// The parameters of `plugins.toolWindow.rows`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RowsParams {
    /// The plug-in's id.
    pub plugin: String,
    /// The window's id.
    pub window: String,
    /// The section's id.
    pub section: String,
    /// List again rather than answer the rows listed before.
    pub refresh: bool,
    /// List again even within 30 s of a failed listing (the window's Refresh button).
    pub force: bool,
    /// How long to wait for a listing (at most 20000; 20000 by default); the window passes 0 and asks again.
    pub wait_ms: Option<u64>,
}

/// A row of a section.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RowInfo {
    /// The id to pass to `plugins.toolWindow.act`.
    pub id: String,
    /// Its values, one per column.
    pub columns: Vec<String>,
    /// Its state (for example running or exited), shown as a dot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// For an index section with a cluster state: what the cluster says of this node.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<state::ClusterRow>,
}

/// The rows of a section.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Rows {
    /// The rows, as the plug-in read them from the listing.
    pub rows: Vec<RowInfo>,
    /// Why the last listing failed (for example the program's daemon is not running).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether an action on the section is running, so the rows were not listed again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub busy: bool,
    /// Whether a listing still runs (with a short `waitMs`); the rows are those listed before.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub listing: bool,
    /// After a failure: how long the window waits before it lists again by itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
    /// The window's choices as they were when the rows were listed: every listing and action passes their values.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<ChoiceInfo>,
    /// For an index section that declares a cluster state: where it came from, and why it is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<state::ClusterInfo>,
}

/// A choice of a window's header and what it may be.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceInfo {
    /// Its id.
    pub id: String,
    /// Its title.
    pub title: String,
    /// The value chosen, which every listing and action passes; absent before the first listing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Whether the value is among the options listed last; when not, nothing is listed or done until the person chooses.
    pub valid: bool,
    /// What it may be, as listed with the window's rows.
    pub options: Vec<ChoiceOption>,
    /// Why its options could not be listed, when it fell back to its first value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// The files its options are read from, as the programs see them: the paths of the variable the plug-in names (from the login shell's environment, ADR-0294), or its default.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<SourceFile>,
}

/// A file a choice's options are read from.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    /// The file.
    pub path: String,
    /// Whether it exists (a missing one is skipped, as the program skips it).
    pub exists: bool,
}

pub fn source_files(source: &zen_syntax::ToolSource) -> Vec<SourceFile> {
    let home = zen_env::var("HOME").map(PathBuf::from);
    let expand = |path: PathBuf| match (path.strip_prefix("~"), &home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path,
    };
    let named = zen_env::var(&source.variable).filter(|value| !value.is_empty());
    let paths: Vec<PathBuf> = match named {
        Some(value) => std::env::split_paths(&value).filter(|path| !path.as_os_str().is_empty()).collect(),
        None if source.default.is_empty() => Vec::new(),
        None => vec![PathBuf::from(&source.default)],
    };
    paths
        .into_iter()
        .map(expand)
        .map(|path| SourceFile { exists: path.is_file(), path: path.to_string_lossy().into_owned() })
        .collect()
}

/// An option of a choice.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceOption {
    /// The value, for `plugins.toolWindow.choose`.
    pub id: String,
    /// How it is shown.
    pub label: String,
}

fn initial_value(choice: &ToolChoice, before: &[(String, Chosen)], choices: &[ToolChoice]) -> Option<String> {
    let (from, column) = choice.initial_from.split_once('.')?;
    let (_, earlier) = before.iter().find(|(id, _)| id == from)?;
    let declared = choices.iter().find(|declared| declared.id == from)?;
    let index = declared.columns.iter().position(|name| name == column)?;
    let value = earlier.value.as_ref()?;
    let row = earlier.options.iter().find(|row| &row.id == value)?;
    row.columns.get(index).filter(|value| !value.is_empty()).cloned()
}

fn snapshot(
    plugin: &dyn zen_syntax::TextFeatures,
    choices: &[ToolChoice],
    wanted: &Values,
    root: &Path,
    folders: &[PathBuf],
) -> (Vec<(String, Chosen)>, Values, Result<(), String>) {
    let language = zen_i18n::language();
    let mut values = Values::new();
    let mut out: Vec<(String, Chosen)> = Vec::new();
    for choice in choices {
        let title = localized(&choice.title, &language);
        let asked = wanted.get(&choice.id).cloned();
        let initial = initial_value(choice, &out, choices);
        let listed = filled_in(&choice.list.args, None, &[], &values).and_then(|args| {
            let found = program(plugin, &choice.list.program, folders)?;
            capture(&found, &args, root, folders, LIST_TIMEOUT)
        });
        let chosen = match listed {
            Ok(output) => {
                let options = plugin.tool_rows(&format!("choice:{}", choice.id), &output).unwrap_or_default();
                let value = asked
                    .or_else(|| initial.filter(|value| options.iter().any(|row| &row.id == value)))
                    .or_else(|| {
                        options.iter().find(|row| row.state.as_deref() == Some("current")).map(|row| row.id.clone())
                    })
                    .or_else(|| options.first().map(|row| row.id.clone()));
                let valid = value.as_ref().is_some_and(|value| options.iter().any(|row| &row.id == value));
                Chosen { options, value, valid, note: None }
            }
            Err(error) => match asked.or(initial).filter(|_| !choice.initial_from.is_empty()) {
                Some(value) => {
                    let only = ToolRow { id: value.clone(), columns: vec![value.clone()], state: None };
                    Chosen { options: vec![only], value: Some(value), valid: true, note: Some(error) }
                }
                None => {
                    out.push((choice.id.clone(), Chosen { note: Some(error.clone()), ..Chosen::default() }));
                    return (out, values, Err(error));
                }
            },
        };
        let Some(value) = chosen.value.clone().filter(|_| chosen.valid) else {
            let error = match &chosen.value {
                Some(value) => t!("workbench-tool-choice-gone", value = value.as_str(), choice = title.as_str()),
                None => t!("workbench-tool-choice-none", choice = title.as_str()),
            };
            out.push((choice.id.clone(), chosen));
            return (out, values, Err(error));
        };
        values.insert(choice.id.clone(), value);
        out.push((choice.id.clone(), chosen));
    }
    (out, values, Ok(()))
}

pub(crate) fn current_values(
    session: &mut Session,
    plugin_id: &str,
    window_id: &str,
) -> Result<Vec<(String, String, String)>, CommandError> {
    let root = guard_trust(session)?;
    let (plugin, window) = window_of(plugin_id, window_id)?;
    let key = (plugin_id.to_owned(), window_id.to_owned());
    let language = zen_i18n::language();
    let known = |session: &Session| -> Option<Vec<(String, String, String)>> {
        let choices = session.state::<ToolState>()?.choices.get(&key)?;
        window
            .choices
            .iter()
            .map(|choice| {
                let chosen = choices.get(&choice.id).filter(|chosen| chosen.valid)?;
                Some((choice.id.clone(), localized(&choice.title, &language), chosen.value.clone()?))
            })
            .collect()
    };
    if let Some(values) = known(session) {
        return Ok(values);
    }
    let wanted = wanted(session, &root, &key);
    let folders = folders(session);
    let (choices, _, ready) = snapshot(plugin.as_ref(), &window.choices, &wanted, &root, &folders);
    let before = values_of(session, &key);
    let state = session.state_mut::<ToolState>().choices.entry(key.clone()).or_default();
    for (id, chosen) in choices {
        state.insert(id, chosen);
    }
    if values_of(session, &key) != before {
        remember(session, &root, &key);
    }
    if let Err(error) = ready {
        return Err(CommandError::with_code(CHOICE_NEEDED, error));
    }
    known(session).ok_or_else(|| {
        let title = window.choices.first().map(|choice| localized(&choice.title, &language)).unwrap_or_default();
        CommandError::with_code(CHOICE_NEEDED, t!("workbench-tool-choose", choice = title.as_str()))
    })
}

fn list(
    session: &mut Session,
    key: &Key,
    window: &ToolWindowDeclaration,
    section: &ToolSection,
    plugin: Arc<dyn zen_syntax::TextFeatures>,
    root: &Path,
) {
    let folders = folders(session);
    let wanted = wanted(session, root, &(key.0.clone(), key.1.clone()));
    let listing: Listing = Arc::new(Mutex::new(None));
    let state = session.state_mut::<ToolState>().sections.entry(key.clone()).or_default();
    if state.pending.is_some() {
        return;
    }
    state.listed = true;
    state.pending = Some(Arc::clone(&listing));
    let (section, root, choices) = (section.clone(), root.to_path_buf(), window.choices.clone());
    std::thread::spawn(move || {
        let (choices, values, ready) = snapshot(plugin.as_ref(), &choices, &wanted, &root, &folders);
        let rows = ready.and_then(|()| {
            let args = filled_in(&section.list.args, None, &[], &values)?;
            let found = program(plugin.as_ref(), &section.list.program, &folders)?;
            let output = capture(&found, &args, &root, &folders, LIST_TIMEOUT)?;
            Ok(plugin.tool_rows(&section.id, &output).unwrap_or_default())
        });
        if let Ok(mut slot) = listing.lock() {
            *slot = Some(Done { rows, choices, values, notes: Vec::new() });
        }
    });
}

fn collect(session: &mut Session, key: &Key, wait: Duration) {
    let Some(state) = session.state_mut::<ToolState>().sections.get_mut(key) else { return };
    let Some(pending) = state.pending.clone() else { return };
    let started = Instant::now();
    let done = loop {
        if let Some(done) = pending.lock().ok().and_then(|mut slot| slot.take()) {
            break done;
        }
        if started.elapsed() >= wait {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    state.pending = None;
    state.notes = done.notes;
    match done.rows {
        Ok(rows) => {
            state.rows = rows;
            state.values = done.values;
            state.error = None;
            state.failed_at = None;
        }
        Err(error) => {
            state.rows.clear();
            state.values.clear();
            state.error = Some(error);
            state.failed_at = Some(Instant::now());
        }
    }
    if done.choices.is_empty() {
        return;
    }
    let window = (key.0.clone(), key.1.clone());
    let before = values_of(session, &window);
    let choices = session.state_mut::<ToolState>().choices.entry(window.clone()).or_default();
    for (id, chosen) in done.choices {
        choices.insert(id, chosen);
    }
    if values_of(session, &window) != before
        && let Ok(root) = crate::run::project_root(session)
    {
        remember(session, &root, &window);
    }
}

fn answer(session: &Session, key: &Key, window: &ToolWindowDeclaration) -> Rows {
    let Some(state) = session.state::<ToolState>() else { return Rows::default() };
    let language = zen_i18n::language();
    let known = state.choices.get(&(key.0.clone(), key.1.clone()));
    let choices = window
        .choices
        .iter()
        .map(|choice| {
            let chosen = known.and_then(|known| known.get(&choice.id)).cloned().unwrap_or_default();
            ChoiceInfo {
                id: choice.id.clone(),
                title: localized(&choice.title, &language),
                value: chosen.value,
                valid: chosen.valid,
                options: chosen
                    .options
                    .iter()
                    .map(|row| ChoiceOption {
                        id: row.id.clone(),
                        label: row.columns.first().cloned().unwrap_or_else(|| row.id.clone()),
                    })
                    .collect(),
                note: chosen.note,
                source: choice.source.as_ref().map(source_files).unwrap_or_default(),
            }
        })
        .collect();
    let busy = state.running.contains_key(key);
    let Some(listed) = state.sections.get(key) else { return Rows { busy, choices, ..Rows::default() } };
    let retry_after_ms =
        listed.failed_at.map(|at| u64::try_from(BACKOFF.saturating_sub(at.elapsed()).as_millis()).unwrap_or(u64::MAX));
    let rows = listed
        .rows
        .iter()
        .map(|row| RowInfo {
            id: row.id.clone(),
            columns: row.columns.clone(),
            state: row.state.clone(),
            cluster: None,
        })
        .collect();
    Rows {
        rows,
        error: listed.error.clone(),
        busy,
        listing: listed.pending.is_some(),
        retry_after_ms,
        choices,
        cluster: None,
    }
}

pub struct ListRows;

impl Command for ListRows {
    type Params = RowsParams;
    type Output = Rows;
    const ID: &'static str = "plugins.toolWindow.rows";
    const TITLE: &'static str = "Plug-in Tool Window Rows";
    const DESCRIPTION: &'static str = "The rows of a section of a plug-in's tool window, read-only; `refresh` runs its \
 listing again (never while an action on the section runs, and not within 30 s of a failed listing unless \
 `force`). Never in a project nobody trusted.";

    fn run(&self, session: &mut Session, params: RowsParams) -> Result<Rows, CommandError> {
        crate::login_env::sync(session);
        let (_, declared) = window_of(&params.plugin, &params.window)?;
        if section_of(&declared, &params.section)?.rows == zen_syntax::INDEX_ROWS {
            return Ok(index_rows(session, &params));
        }
        let root = guard_trust(session)?;
        let (plugin, window) = window_of(&params.plugin, &params.window)?;
        let section = section_of(&window, &params.section)?.clone();
        let key = (params.plugin.clone(), params.window.clone(), params.section.clone());
        let busy = settle(session, &key);
        let state = session.state_mut::<ToolState>();
        let known = state.sections.get(&key);
        let waiting = known.and_then(|listed| listed.failed_at).is_some_and(|at| at.elapsed() < BACKOFF);
        let first = known.is_none_or(|listed| !listed.listed);
        if !busy && (first || (params.refresh && (params.force || !waiting))) {
            list(session, &key, &window, &section, plugin, &root);
        }
        let wait = Duration::from_millis(params.wait_ms.unwrap_or(20_000).min(20_000));
        collect(session, &key, wait);
        Ok(answer(session, &key, &window))
    }
}

fn index_rows(session: &mut Session, params: &RowsParams) -> Rows {
    let get = crate::project_index::GetParams {
        plugin: Some(params.plugin.clone()),
        wait_ms: params.wait_ms.unwrap_or(0).min(20_000),
        ..crate::project_index::GetParams::default()
    };
    let answer = crate::project_index::GetIndex.run(session, get);
    let (indexes, error) = match answer {
        Ok(list) => (list.indexes, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let listing = indexes.iter().any(|index| index.indexing);
    let mut rows = Rows { rows: index_rows_of(indexes), error, listing, ..Rows::default() };
    if let Ok((plugin, window)) = window_of(&params.plugin, &params.window)
        && let Ok(section) = section_of(&window, &params.section).cloned()
        && let Some(declared) = section.state.clone()
    {
        let cluster = state::listed(session, params, plugin, &window, &section, &declared);
        state::attach(&mut rows.rows, &cluster.1);
        rows.cluster = Some(cluster.0);
    }
    rows
}

pub(crate) fn index_rows_of(indexes: Vec<crate::project_index::IndexAnswer>) -> Vec<RowInfo> {
    indexes
        .into_iter()
        .flat_map(|index| {
            let problems = index.index.problems;
            index.index.nodes.into_iter().filter(|node| node.in_graph).map(move |node| {
                let state = problems
                    .iter()
                    .filter(|problem| {
                        problem.path == node.path && node.start <= problem.start && problem.end <= node.end
                    })
                    .map(|problem| problem.severity.clone())
                    .min_by_key(|severity| severity != "error");
                RowInfo {
                    id: node.id,
                    columns: vec![node.name, node.kind, node.namespace, node.path],
                    state,
                    cluster: None,
                }
            })
        })
        .collect()
}

/// The parameters of `plugins.toolWindow.act`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ActParams {
    /// The plug-in's id.
    pub plugin: String,
    /// The window's id.
    pub window: String,
    /// The section's id.
    pub section: String,
    /// The action's id.
    pub action: String,
    /// The ids of the rows it acts on, as `plugins.toolWindow.rows` listed them.
    pub rows: Vec<String>,
    /// After `confirmationNeeded`: its confirmation, once the person allowed it.
    pub confirmation: Option<String>,
    /// The person's yes to a destroying action, given in the window's dialog.
    pub confirmed: bool,
    /// For a change or destroy: wait this long (at most 120000) for it to end and answer its outcome.
    pub wait_ms: Option<u64>,
    /// The values the person typed in the window's dialog, by input id; refused from agents.
    pub inputs: Inputs,
}

/// What an action did.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Acted {
    /// A stream action's run (`run.output.get`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// A terminal action's session (`terminal.session.read`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<u64>,
    /// A change's or destroy's outcome per row (the whole section when it takes no rows), once it ended.
    pub done: Vec<Outcome>,
    /// Whether a change or destroy still runs; the section's rows are listed again when it ends.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub running: bool,
}

/// The outcome of a change on one row.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// The row, or empty for the section.
    pub row: String,
    /// The action with its rows, as the person reads it.
    pub action: String,
    /// Whether the program succeeded.
    pub ok: bool,
    /// What it printed, cut.
    pub output: String,
}

fn ask_person(
    session: &mut Session,
    key: &Key,
    label: &str,
    question: Option<&str>,
    confirmation: Option<&str>,
) -> Result<(), CommandError> {
    let state = session.state_mut::<ToolState>();
    if let Some(token) = confirmation
        && state.allowed.remove(token)
    {
        state.asked.remove(token);
        return Ok(());
    }
    state.next += 1;
    let token = format!("tool-{}-{}", state.next, crate::notifications::now_ms());
    state.asked.insert(token.clone(), key.clone());
    let body = match question {
        Some(question) => t!("workbench-tool-agent-confirm", question = question),
        None => t!("workbench-tool-agent-destroy", action = label),
    };
    let notice = NewNotification::message(Group::General, Kind::Warning, body)
        .action(crate::notifications::Action {
            id: "allow".to_owned(),
            label: t!("workbench-plugin-run-allow"),
            command: AllowDestroy::ID.to_owned(),
            params: serde_json::json!({ "token": token }),
            once: true,
        })
        .sticky();
    crate::notifications::post(session, notice);
    Err(CommandError::with_code(CONFIRMATION_NEEDED, t!("workbench-plugin-run-agent-asked", name = label))
        .with_data(serde_json::json!({ "confirmation": token })))
}

fn question(action: &ToolAction, values: &Values, names: &[&str]) -> Option<String> {
    if !confirms(action) {
        return None;
    }
    let mut text = localized(&action.confirm, &zen_i18n::language());
    let mut shown: Vec<&str> = names.iter().copied().take(MAX_NAMED_ROWS).collect();
    let more = names.len().saturating_sub(MAX_NAMED_ROWS);
    let more_text = t!("workbench-tool-more-rows", count = more);
    if more > 0 {
        shown.push(&more_text);
    }
    text = text.replace("${rows}", &shown.join(", "));
    for (id, value) in values {
        text = text.replace(&format!("${{window.{id}}}"), value);
    }
    Some(text)
}

fn guard_action(
    session: &mut Session,
    key: &Key,
    action: &ToolAction,
    label: &str,
    asked: Option<&str>,
    params: &ActParams,
) -> Result<(), CommandError> {
    let by_agent = zen_core::macros::by_agent(session);
    if let Some(question) = asked {
        if !by_agent {
            return if params.confirmed {
                Ok(())
            } else {
                Err(CommandError::with_code(CONFIRMATION_NEEDED, question.to_owned()))
            };
        }
        if crate::plugin_runs::agent_access(session, &key.0) == crate::plugin_runs::AgentAccess::None {
            return Err(CommandError::with_code(
                crate::trust::AGENT_NOT_ALLOWED,
                t!("workbench-plugin-run-agent-none", plugin = key.0.as_str(), name = label),
            ));
        }
        return ask_person(session, key, label, Some(question), params.confirmation.as_deref());
    }
    match (action.kind, by_agent) {
        (ToolActionKind::Destroy, false) if params.confirmed => Ok(()),
        (ToolActionKind::Destroy, false) => {
            Err(CommandError::with_code(CONFIRMATION_NEEDED, t!("workbench-tool-confirm", action = label)))
        }
        (ToolActionKind::Destroy, true) => {
            if crate::plugin_runs::agent_access(session, &key.0) == crate::plugin_runs::AgentAccess::None {
                return Err(CommandError::with_code(
                    crate::trust::AGENT_NOT_ALLOWED,
                    t!("workbench-plugin-run-agent-none", plugin = key.0.as_str(), name = label),
                ));
            }
            ask_person(session, key, label, None, params.confirmation.as_deref())
        }
        (ToolActionKind::Stream, true) => Ok(()),
        (_, true) => {
            crate::plugin_runs::guard_agent_action(session, &key.0, label, params.confirmation.as_deref(), false)
        }
        (_, false) => Ok(()),
    }
}

fn scope(session: &Session, key: &Key, window: &ToolWindowDeclaration) -> Result<Values, CommandError> {
    if !window.window_values.is_empty() {
        return state::scope(session, key, window);
    }
    if window.choices.is_empty() {
        return Ok(Values::new());
    }
    let language = zen_i18n::language();
    let state = session.state::<ToolState>();
    let known = state.and_then(|state| state.choices.get(&(key.0.clone(), key.1.clone())));
    let mut values = Values::new();
    for choice in &window.choices {
        let chosen = known.and_then(|known| known.get(&choice.id));
        let Some(value) = chosen.filter(|chosen| chosen.valid).and_then(|chosen| chosen.value.clone()) else {
            let title = localized(&choice.title, &language);
            return Err(CommandError::with_code(CHOICE_NEEDED, t!("workbench-tool-choose", choice = title.as_str())));
        };
        values.insert(choice.id.clone(), value);
    }
    let listed = state.and_then(|state| state.sections.get(key)).map(|listed| &listed.values);
    if listed != Some(&values) {
        return Err(CommandError::new(t!("workbench-tool-list-again")));
    }
    Ok(values)
}

fn columns_of(section: &ToolSection) -> &[String] {
    section.state.as_ref().map_or(&section.columns, |state| &state.columns)
}

fn chosen(session: &Session, key: &Key, ids: &[String]) -> Result<Vec<ToolRow>, CommandError> {
    let listed = session.state::<ToolState>().and_then(|state| state.sections.get(key));
    ids.iter()
        .map(|id| {
            listed
                .and_then(|listed| listed.rows.iter().find(|row| &row.id == id))
                .cloned()
                .ok_or_else(|| CommandError::new(t!("workbench-tool-unknown-row", row = id.as_str())))
        })
        .collect()
}

pub struct Act;

impl Command for Act {
    type Params = ActParams;
    type Output = Acted;
    const ID: &'static str = "plugins.toolWindow.act";
    const TITLE: &'static str = "Plug-in Tool Window Action";
    const DESCRIPTION: &'static str = "Runs an action of a plug-in's tool window on rows listed before (by their ids). \
 Agents: as the plug-in's run access allows; a destroying action always asks the person (confirmationNeeded). \
 Never in a project nobody trusted.";

    fn run(&self, session: &mut Session, params: ActParams) -> Result<Acted, CommandError> {
        crate::login_env::sync(session);
        let root = guard_trust(session)?;
        let (plugin, window) = window_of(&params.plugin, &params.window)?;
        let section = section_of(&window, &params.section)?.clone();
        let action =
            section.actions.iter().find(|declared| declared.id == params.action).cloned().ok_or_else(|| {
                CommandError::new(t!("workbench-tool-unknown-action", action = params.action.as_str()))
            })?;
        let key = (params.plugin.clone(), params.window.clone(), params.section.clone());
        let values = scope(session, &key, &window)?;
        let rows = chosen(session, &key, &params.rows)?;
        if action.rows && rows.is_empty() {
            return Err(CommandError::new(t!("workbench-tool-needs-rows")));
        }
        let names: Vec<&str> =
            rows.iter().map(|row| row.columns.first().map_or(row.id.as_str(), String::as_str)).collect();
        let label =
            format!("{} {}", localized(&action.title, &zen_i18n::language()), names.join(", ")).trim().to_owned();
        let mut launch_confirmation = params.confirmation.clone();
        let typed = if action.inputs.is_empty() {
            if !params.inputs.is_empty() {
                return Err(CommandError::new(t!(
                    "workbench-tool-input-unknown",
                    input = params.inputs.keys().next().map(String::as_str).unwrap_or_default()
                )));
            }
            let asked = question(&action, &values, &names);
            guard_action(session, &key, &action, &label, asked.as_deref(), &params)?;
            Inputs::new()
        } else if zen_core::macros::by_agent(session) {
            let asked = question(&action, &values, &names);
            let typed = typed_for_agent(session, &key, &params, &label, &values, asked)?;
            launch_confirmation = Some(crate::plugin_runs::granted_once(session, &label));
            typed
        } else {
            let typed = inputs::values(&action, &params.inputs).map_err(CommandError::new)?;
            let asked = question(&action, &values, &names).map(|text| inputs::in_question(text, &typed));
            guard_action(session, &key, &action, &label, asked.as_deref(), &params)?;
            typed
        };
        let folders = folders(session);
        let found = program(plugin.as_ref(), &action.program, &folders).map_err(CommandError::new)?;
        let targets: Vec<Option<&ToolRow>> = if action.rows { rows.iter().map(Some).collect() } else { vec![None] };
        match action.kind {
            ToolActionKind::Stream => {
                let args = filled_in(&action.args, targets[0], columns_of(&section), &values)
                    .and_then(|args| inputs::fill(args, &action.args, &action, &typed))
                    .map_err(CommandError::new)?;
                let options = PluginRunOptions {
                    plugin: params.plugin.clone(),
                    target: format!("{}:{}", params.section, params.action),
                    program: action.program.clone(),
                    args,
                    cwd: Some(root.to_string_lossy().into_owned()),
                    ..PluginRunOptions::default()
                };
                let configuration = Configuration {
                    name: label,
                    kind: KIND.to_owned(),
                    options: serde_json::to_value(options).unwrap_or_default(),
                    ..Configuration::default()
                };
                let launch = crate::run::Launch { confirmation: launch_confirmation, ..crate::run::Launch::default() };
                let started = crate::run::start_with(session, configuration, launch, &[])?;
                Ok(Acted { run_id: Some(started.run_id), ..Acted::default() })
            }
            ToolActionKind::Terminal => {
                let args = filled_in(&action.args, targets[0], columns_of(&section), &values)
                    .and_then(|args| inputs::fill(args, &action.args, &action, &typed))
                    .map_err(CommandError::new)?;
                let mut options = zen_terminal::Options::shell(&root);
                options.program = Some(found);
                options.args = args;
                options.env = environment(&folders);
                let id = session.state_mut::<crate::terminal::Terminals>().run_in_new_tab(options, label)?;
                Ok(Acted { terminal: Some(id), ..Acted::default() })
            }
            ToolActionKind::Change | ToolActionKind::Destroy => {
                if session.state::<ToolState>().is_some_and(|state| state.running.contains_key(&key)) {
                    return Err(CommandError::new(t!("workbench-tool-busy")));
                }
                let mut calls = Vec::new();
                for row in targets {
                    let args = filled_in(&action.args, row, columns_of(&section), &values)
                        .and_then(|args| inputs::fill(args, &action.args, &action, &typed))
                        .map_err(CommandError::new)?;
                    calls.push((row.map(|row| row.id.clone()).unwrap_or_default(), args));
                }
                let running: Running = Arc::new(Mutex::new(None));
                session.state_mut::<ToolState>().running.insert(key.clone(), Arc::clone(&running));
                let worker = Arc::clone(&running);
                let folder = root.clone();
                let finished = std::thread::spawn(move || {
                    let done = calls
                        .into_iter()
                        .map(|(row, args)| {
                            let (ok, output) = match capture(&found, &args, &folder, &folders, CHANGE_TIMEOUT) {
                                Ok(output) => (true, output.chars().take(MAX_OUTPUT).collect()),
                                Err(error) => (false, error),
                            };
                            Outcome { row, action: label.clone(), ok, output }
                        })
                        .collect();
                    if let Ok(mut slot) = worker.lock() {
                        *slot = Some(done);
                    }
                });
                let wait = Duration::from_millis(params.wait_ms.unwrap_or(0).min(120_000));
                let started = Instant::now();
                while !finished.is_finished() && started.elapsed() < wait {
                    std::thread::sleep(Duration::from_millis(20));
                }
                if !finished.is_finished() {
                    return Ok(Acted { running: true, ..Acted::default() });
                }
                let done = running.lock().ok().and_then(|slot| slot.clone()).unwrap_or_default();
                settle(session, &key);
                list(session, &key, &window, &section, plugin, &root);
                collect(session, &key, LIST_TIMEOUT);
                Ok(Acted { done, ..Acted::default() })
            }
        }
    }
}

fn typed_for_agent(
    session: &mut Session,
    key: &Key,
    params: &ActParams,
    label: &str,
    values: &Values,
    asked: Option<String>,
) -> Result<Inputs, CommandError> {
    if !params.inputs.is_empty() {
        return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("workbench-tool-input-agent")));
    }
    if crate::plugin_runs::agent_access(session, &key.0) == crate::plugin_runs::AgentAccess::None {
        return Err(CommandError::with_code(
            crate::trust::AGENT_NOT_ALLOWED,
            t!("workbench-plugin-run-agent-none", plugin = key.0.as_str(), name = label),
        ));
    }
    let request = |token: String| InputRequest {
        token,
        plugin: key.0.clone(),
        window: key.1.clone(),
        section: key.2.clone(),
        action: params.action.clone(),
        rows: params.rows.clone(),
        question: asked.clone(),
    };
    let state = session.state_mut::<ToolState>();
    if let Some(token) = params.confirmation.as_deref()
        && state.wanting.get(token).is_some_and(|wanted| *wanted == request(token.to_owned()))
        && let Some(typed) = state.entered.remove(token)
    {
        state.wanting.remove(token);
        return Ok(typed);
    }
    state.next += 1;
    let token = format!("tool-{}-{}", state.next, crate::notifications::now_ms());
    state.wanting.insert(token.clone(), request(token.clone()));
    let scope: Vec<&str> = values.values().map(String::as_str).collect();
    let body = if scope.is_empty() {
        t!("workbench-tool-agent-values", action = label)
    } else {
        t!("workbench-tool-agent-values-in", action = label, scope = scope.join(" / "))
    };
    let notice = NewNotification::message(Group::General, Kind::Warning, body)
        .action(crate::notifications::Action {
            id: "values".to_owned(),
            label: t!("workbench-tool-enter-values"),
            command: EnterValues::ID.to_owned(),
            params: serde_json::json!({ "token": token }),
            once: false,
        })
        .sticky();
    crate::notifications::post(session, notice);
    Err(CommandError::with_code(CONFIRMATION_NEEDED, t!("workbench-tool-agent-values-asked", name = label))
        .with_data(serde_json::json!({ "confirmation": token })))
}

/// The parameters of `plugins.toolWindow.enterValues`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct EnterParams {
    /// The `confirmation` of the agent's action that waits for values.
    pub token: String,
}

pub struct EnterValues;

impl Command for EnterValues {
    type Params = EnterParams;
    type Output = InputRequest;
    const ID: &'static str = "plugins.toolWindow.enterValues";
    const TITLE: &'static str = "Enter Values for an Agent's Action";
    const DESCRIPTION: &'static str = "Opens the window's dialog where the person types the values an agent's \
         plug-in tool window action waits for. The person's alone.";

    fn run(&self, session: &mut Session, params: EnterParams) -> Result<InputRequest, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("workbench-tool-input-agent")));
        }
        let state = session.state_mut::<ToolState>();
        let request = state
            .wanting
            .get(&params.token)
            .cloned()
            .ok_or_else(|| CommandError::new(t!("workbench-plugin-run-no-question")))?;
        state.entering = Some(request.clone());
        Ok(request)
    }
}

/// The parameters of `plugins.toolWindow.allow`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct AllowParams {
    /// The `confirmation` of the agent's refused action.
    pub token: String,
    /// For an action that waits for values: the values the person typed, by input id.
    #[serde(default)]
    pub inputs: Inputs,
}

pub struct AllowDestroy;

impl Command for AllowDestroy {
    type Params = AllowParams;
    type Output = ();
    const ID: &'static str = "plugins.toolWindow.allow";
    const TITLE: &'static str = "Allow an Agent's Destroying Action";
    const DESCRIPTION: &'static str = "The person's yes to an agent's destroying action of a plug-in tool window.";

    fn run(&self, session: &mut Session, params: AllowParams) -> Result<(), CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(
                crate::trust::AGENT_NOT_ALLOWED,
                t!("workbench-plugin-run-access-person"),
            ));
        }
        if let Some(request) = session.state::<ToolState>().and_then(|state| state.wanting.get(&params.token)).cloned()
        {
            let (_, window) = window_of(&request.plugin, &request.window)?;
            let action = section_of(&window, &request.section)?
                .actions
                .iter()
                .find(|declared| declared.id == request.action)
                .cloned()
                .ok_or_else(|| {
                    CommandError::new(t!("workbench-tool-unknown-action", action = request.action.as_str()))
                })?;
            let typed = inputs::values(&action, &params.inputs).map_err(CommandError::new)?;
            session.state_mut::<ToolState>().entered.insert(params.token, typed);
            return Ok(());
        }
        let state = session.state_mut::<ToolState>();
        if !state.asked.contains_key(&params.token) {
            return Err(CommandError::new(t!("workbench-plugin-run-no-question")));
        }
        state.allowed.insert(params.token);
        Ok(())
    }
}

/// The parameters of `plugins.toolWindow.choose`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ChooseParams {
    /// The plug-in's id.
    pub plugin: String,
    /// The window's id.
    pub window: String,
    /// The choice's id, such as `context`.
    pub choice: String,
    /// One of the options `plugins.toolWindow.rows` listed for it.
    pub value: String,
}

pub struct Choose;

impl Command for Choose {
    type Params = ChooseParams;
    type Output = ();
    const ID: &'static str = "plugins.toolWindow.choose";
    const TITLE: &'static str = "Choose in a Plug-in Tool Window";
    const DESCRIPTION: &'static str = "The person's choice in a plug-in tool window's header (a cluster's context or namespace), among the options listed with its rows; the window's rows are listed again under it. Refused to agents. It changes no file of the program's own: the value is passed to every listing and action.";

    fn run(&self, session: &mut Session, params: ChooseParams) -> Result<(), CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("workbench-tool-choose-person")));
        }
        let root = guard_trust(session)?;
        let (_, window) = window_of(&params.plugin, &params.window)?;
        let at =
            window.choices.iter().position(|choice| choice.id == params.choice).ok_or_else(|| {
                CommandError::new(t!("workbench-tool-unknown-choice", choice = params.choice.as_str()))
            })?;
        let key = (params.plugin.clone(), params.window.clone());
        wanted(session, &root, &key);
        let state = session.state_mut::<ToolState>();
        let choices = state.choices.entry(key.clone()).or_default();
        let chosen = choices.entry(params.choice.clone()).or_default();
        if !chosen.options.iter().any(|row| row.id == params.value) {
            return Err(CommandError::new(t!(
                "workbench-tool-unknown-option",
                value = params.value.as_str(),
                choice = params.choice.as_str()
            )));
        }
        chosen.value = Some(params.value.clone());
        chosen.valid = true;
        chosen.note = None;
        for later in &window.choices[at + 1..] {
            choices.remove(&later.id);
        }
        for (section, listed) in &mut state.sections {
            if section.0 == key.0 && section.1 == key.1 {
                listed.rows.clear();
                listed.values.clear();
                listed.listed = false;
                listed.error = None;
                listed.failed_at = None;
                listed.pending = None;
            }
        }
        remember(session, &root, &key);
        Ok(())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListWindows)?;
    registry.register(Choose)?;
    registry.register(EnterValues)?;
    registry.register(ListRows)?;
    registry.register(Act)?;
    registry.register(AllowDestroy)?;
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod input_tests;

#[cfg(test)]
pub(crate) mod choice_tests;

#[cfg(test)]
pub(crate) mod state_tests;

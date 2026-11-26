use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{
    CommandLine, Configuration, FieldKind, FieldSpec, KindSpec, OutputParser, PlainParser, RunContext, RunProvider,
    RunTarget, TargetKind,
};
use zen_syntax::{Feature, RunOffer, TextFeatures};

use crate::notifications::{Action, Group, Kind, NewNotification};
use crate::trust::AGENT_NOT_ALLOWED;

pub const KIND: &str = "plugin";

pub const CONFIRMATION_NEEDED: &str = "confirmationNeeded";

pub const PERSON_CONFIRMATION: &str = "personConfirmation";

/// A run that only a person starts, and only after reading what it runs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PersonQuestion {
    /// Passed back as `confirmation` to `run.process.start` once the person said Run.
    #[serde(skip)]
    pub token: String,
    /// The configuration to start, as it was asked for.
    #[serde(skip)]
    pub configuration: Configuration,
    /// Its name.
    pub name: String,
    /// The plug-in whose target it is.
    pub plugin: String,
    /// The whole command line, as it will run.
    pub command_line: String,
    /// The folder it runs in.
    pub folder: PathBuf,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Caller {
    #[default]
    Unknown,
    Agent,
    Person(Option<String>),
}

pub(crate) fn person_only(configuration: &Configuration, lines: &[CommandLine]) -> bool {
    if configuration.kind != KIND {
        return false;
    }
    let options = options(configuration);
    let Some(plugin) = plugin_named(&options.plugin) else { return true };
    let rules = plugin.person_only();
    lines.iter().any(|line| zen_syntax::is_person_only(&rules, &options.program, &line.args))
}

pub(crate) fn is_diff(configuration: &Configuration) -> bool {
    configuration.kind == KIND && options(configuration).diff
}

pub(crate) fn document_type(configuration: &Configuration) -> Option<String> {
    (configuration.kind == KIND).then(|| options(configuration).document).flatten()
}

pub(crate) fn shown_command(configuration: &Configuration) -> String {
    let options = options(configuration);
    let file = Path::new(&options.file);
    let folder = file.parent().map(|parent| parent.to_string_lossy().into_owned()).filter(|folder| !folder.is_empty());
    let shown = |arg: &String| {
        arg.replace("${fileDir}", folder.as_deref().unwrap_or("."))
            .replace("${file}", &options.file)
            .replace("${projectDir}", ".")
    };
    std::iter::once(options.program.clone()).chain(options.args.iter().map(shown)).collect::<Vec<_>>().join(" ")
}

pub(crate) fn diff_file(configuration: &Configuration) -> String {
    options(configuration).file
}

fn window_reference(arg: &str) -> Option<(&str, &str)> {
    arg.strip_prefix("${window.")?.strip_suffix('}')?.split_once('.')
}

#[cfg(not(target_arch = "wasm32"))]
fn window_values(
    session: &mut Session,
    plugin: &str,
    window: &str,
) -> Result<Vec<(String, String, String)>, CommandError> {
    crate::plugin_tools::current_values(session, plugin, window)
}

#[cfg(target_arch = "wasm32")]
fn window_values(
    _session: &mut Session,
    _plugin: &str,
    window: &str,
) -> Result<Vec<(String, String, String)>, CommandError> {
    Err(CommandError::with_code("choiceNeeded", t!("workbench-tool-choose", choice = window)))
}

pub(crate) fn with_window_values(
    session: &mut Session,
    configuration: Configuration,
) -> Result<(Configuration, Vec<(String, String)>), CommandError> {
    if configuration.kind != KIND {
        return Ok((configuration, Vec::new()));
    }
    let mut options = options(&configuration);
    let mut texts: Vec<&mut String> = options.args.iter_mut().collect();
    texts.extend(options.cwd.iter_mut());
    texts.extend(options.env.iter_mut().map(|(_, value)| value));
    let mut windows: Vec<String> = Vec::new();
    for text in &texts {
        match window_reference(text) {
            Some((window, _)) if !windows.iter().any(|known| known == window) => windows.push(window.to_owned()),
            Some(_) => {}
            None if text.contains("${window.") => {
                return Err(CommandError::new(t!("workbench-tool-bad-argument", arg = text.as_str())));
            }
            None => {}
        }
    }
    if windows.is_empty() {
        return Ok((configuration, Vec::new()));
    }
    let mut values: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut shown = Vec::new();
    for window in &windows {
        for (choice, title, value) in window_values(session, &options.plugin, window)? {
            shown.push((title, value.clone()));
            values.insert((window.clone(), choice), value);
        }
    }
    for text in texts {
        if let Some((window, choice)) = window_reference(text) {
            let key = (window.to_owned(), choice.to_owned());
            let value = values
                .get(&key)
                .cloned()
                .ok_or_else(|| CommandError::new(t!("workbench-tool-bad-argument", arg = text.as_str())))?;
            *text = value;
        }
    }
    let configuration = Configuration { options: serde_json::to_value(&options).unwrap_or_default(), ..configuration };
    Ok((configuration, shown))
}

const ACCESS_FILE: &str = "plugin-runs.json";

pub type PluginSource = Arc<dyn Fn() -> Vec<Arc<dyn TextFeatures>> + Send + Sync>;

fn source() -> &'static RwLock<Option<PluginSource>> {
    static SOURCE: std::sync::OnceLock<RwLock<Option<PluginSource>>> = std::sync::OnceLock::new();
    SOURCE.get_or_init(|| RwLock::new(None))
}

pub fn set_plugins(plugins: Option<PluginSource>) {
    if let Ok(mut slot) = source().write() {
        *slot = plugins;
    }
}

pub(crate) fn plugins() -> Vec<Arc<dyn TextFeatures>> {
    source().read().ok().and_then(|slot| slot.clone()).map(|source| source()).unwrap_or_default()
}

pub const MAX_LISTED_FILES: usize = 1000;

fn glob_name(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, rest)) => {
            let Some(name) = name.strip_prefix(head) else { return false };
            rest.is_empty()
                || name
                    .char_indices()
                    .map(|(at, _)| at)
                    .chain(std::iter::once(name.len()))
                    .any(|at| glob_name(rest, &name[at..]))
        }
    }
}

fn serves_as_targets(plugin: &dyn TextFeatures, file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    plugin.target_files().iter().any(|pattern| glob_name(&pattern.to_ascii_lowercase(), &lower))
}

fn plugins_for(path: &Path) -> Vec<Arc<dyn TextFeatures>> {
    let runnable: Vec<Arc<dyn TextFeatures>> = plugins()
        .into_iter()
        .filter(|plugin| plugin.supports(Feature::RunTargets) && plugin.plugin_id().is_some())
        .collect();
    if path.is_dir() {
        return runnable.into_iter().filter(|plugin| !plugin.target_files().is_empty()).collect();
    }
    let mut chosen: Vec<Arc<dyn TextFeatures>> = Vec::new();
    if let Some(name) = zen_vfs::file_types::known_name_at(path, None)
        && let Some(handler) =
            runnable.iter().filter(|plugin| plugin.handles(&name)).max_by_key(|plugin| plugin.specificity(&name))
    {
        chosen.push(Arc::clone(handler));
    }
    let file_name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    for plugin in runnable {
        let known = chosen.iter().any(|seen| seen.plugin_id() == plugin.plugin_id());
        if !known && serves_as_targets(plugin.as_ref(), &file_name) {
            chosen.push(plugin);
        }
    }
    chosen
}

fn listing(folder: &Path, root: &Path, plugin: &dyn TextFeatures) -> String {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            if name.starts_with('.') || kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                pending.push(entry.path());
            } else if serves_as_targets(plugin, &name) && found.len() < MAX_LISTED_FILES {
                found.push(relative(root, &entry.path()));
            }
        }
    }
    found.sort();
    found.join("\n")
}

pub(crate) fn plugin_named(id: &str) -> Option<Arc<dyn TextFeatures>> {
    plugins().into_iter().find(|plugin| plugin.plugin_id() == Some(id))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginRunOptions {
    pub plugin: String,
    pub target: String,
    pub file: String,
    pub program: String,
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<(String, String)>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub diff: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
}

fn options(configuration: &Configuration) -> PluginRunOptions {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

pub fn configuration(plugin: &str, file: &str, offer: &RunOffer) -> Configuration {
    let options = PluginRunOptions {
        plugin: plugin.to_owned(),
        target: offer.id.clone(),
        file: file.to_owned(),
        program: offer.program.clone(),
        args: offer.args.clone(),
        cwd: offer.cwd.clone(),
        env: offer.env.clone(),
        diff: offer.diff,
        document: offer.document.clone(),
    };
    Configuration {
        name: offer.name.clone(),
        kind: KIND.to_owned(),
        options: serde_json::to_value(options).unwrap_or_default(),
        ..Configuration::default()
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn filled(text: &str, file: &Path, root: &Path) -> String {
    let folder = file.parent().unwrap_or(root);
    text.replace("${fileDir}", &folder.to_string_lossy())
        .replace("${file}", &file.to_string_lossy())
        .replace("${projectDir}", &root.to_string_lossy())
}

#[derive(Default)]
pub struct PluginRun {
    folders: Option<Vec<PathBuf>>,
}

impl PluginRun {
    pub fn in_folders(folders: Vec<PathBuf>) -> Self {
        Self { folders: Some(folders) }
    }

    fn folders(&self) -> Vec<PathBuf> {
        self.folders.clone().unwrap_or_else(zen_lsp::servers::search_folders)
    }
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

impl RunProvider for PluginRun {
    fn id(&self) -> &str {
        KIND
    }

    fn kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: KIND.to_owned(),
            name_message: "workbench-plugin-run-kind".to_owned(),
            fields: vec![
                field("plugin", "workbench-plugin-run-field-plugin", FieldKind::Text),
                field("file", "workbench-plugin-run-field-file", FieldKind::Path),
                field("program", "workbench-plugin-run-field-program", FieldKind::Text),
            ],
            defaults: json!({
                "plugin": "", "target": "", "file": "", "program": "", "args": [], "cwd": null, "env": [], "diff": false,
                "document": null,
            }),
        }]
    }

    fn targets(&self, context: &RunContext, path: &Path, text: &str) -> Vec<RunTarget> {
        let folder = path.is_dir();
        let file = relative(&context.root, path);
        let mut targets = Vec::new();
        for plugin in plugins_for(path) {
            let Some(id) = plugin.plugin_id().map(str::to_owned) else { continue };
            let offers = if folder {
                let listed = listing(path, &context.root, plugin.as_ref());
                if listed.is_empty() {
                    continue;
                }
                plugin.run_targets(&format!("{}/", file.trim_end_matches('/')), &listed)
            } else {
                plugin.run_targets(&file, text)
            };
            targets.extend(offers.into_iter().map(|offer| RunTarget {
                line: offer.line,
                kind: if offer.test { TargetKind::Test } else { TargetKind::Main },
                name: offer.name.clone(),
                package: String::new(),
                configuration: configuration(&id, &file, &offer),
            }));
        }
        targets
    }

    fn context(&self, root: &Path, path: &Path, text: &str, offset: usize) -> Option<Configuration> {
        let context = RunContext { root: root.to_owned(), ..RunContext::default() };
        let line = u32::try_from(text.get(..offset.min(text.len()))?.matches('\n').count()).ok()?;
        self.targets(&context, path, text)
            .into_iter()
            .find(|target| target.line == line)
            .map(|target| target.configuration)
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        let options = options(configuration);
        let declared = plugin_named(&options.plugin).map(|plugin| plugin.runs()).unwrap_or_default();
        if options.program.contains(['/', '\\']) || !declared.contains(&options.program) {
            return Err(t!(
                "workbench-plugin-run-undeclared",
                plugin = options.plugin.as_str(),
                program = options.program.as_str()
            ));
        }
        let folders = self.folders();
        let program = zen_lsp::servers::find_executable(&options.program, &folders)
            .ok_or_else(|| t!("workbench-plugin-run-not-found", program = options.program.as_str()))?;
        let file = context.root.join(&options.file);
        let mut args: Vec<String> = options.args.iter().map(|arg| filled(arg, &file, &context.root)).collect();
        args.extend(zen_run::args::split(&configuration.common.program_arguments));
        let cwd = match &configuration.common.working_directory {
            Some(folder) if !folder.as_os_str().is_empty() => context.root.join(folder),
            _ => options
                .cwd
                .as_deref()
                .map(|cwd| PathBuf::from(filled(cwd, &file, &context.root)))
                .unwrap_or_else(|| file.parent().map_or_else(|| context.root.clone(), Path::to_owned)),
        };
        let mut env = Vec::new();
        if let Ok(path) = std::env::join_paths(&folders) {
            env.push(("PATH".to_owned(), path.to_string_lossy().into_owned()));
        }
        env.extend(
            options
                .env
                .iter()
                .filter(|(name, _)| zen_syntax::kept_variable(name))
                .map(|(name, value)| (name.clone(), filled(value, &file, &context.root))),
        );
        env.extend(configuration.common.env.iter().cloned());
        let display =
            std::iter::once(options.program.clone()).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ");
        Ok(vec![CommandLine { program, args, cwd, env, display }])
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }
}

/// What an agent may do with a plug-in's run targets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AgentAccess {
    /// Agents may not start them.
    #[default]
    None,
    /// An agent's start asks the person first, in a notification.
    Ask,
    /// Agents start them as a person does.
    Full,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    agents: BTreeMap<String, AgentAccess>,
}

#[derive(Default)]
struct Access {
    loaded: bool,
    saved: Saved,
    asked: BTreeMap<String, String>,
    allowed: BTreeSet<String>,
    next: u64,
}

fn access_file() -> Option<PathBuf> {
    crate::build_tools::BuildEnv::global().data_dir().map(|data| data.join(ACCESS_FILE))
}

fn access(session: &mut Session) -> &mut Access {
    let state = session.state_mut::<Access>();
    if !state.loaded {
        state.loaded = true;
        state.saved = access_file()
            .and_then(|file| std::fs::read(file).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
    }
    state
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn granted_once(session: &mut Session, name: &str) -> String {
    let state = access(session);
    state.next += 1;
    let token = format!("run-{}-{}", state.next, crate::notifications::now_ms());
    state.asked.insert(token.clone(), name.to_owned());
    state.allowed.insert(token.clone());
    token
}

pub fn agent_access(session: &mut Session, plugin: &str) -> AgentAccess {
    access(session).saved.agents.get(plugin).copied().unwrap_or_default()
}

pub(crate) fn guard_agent(
    session: &mut Session,
    configuration: &Configuration,
    confirmation: Option<&str>,
) -> Result<(), CommandError> {
    if configuration.kind != KIND || !zen_core::macros::by_agent(session) {
        return Ok(());
    }
    let plugin = options(configuration).plugin;
    guard_agent_action(session, &plugin, &configuration.name, confirmation, false)
}

pub(crate) fn guard_agent_action(
    session: &mut Session,
    plugin: &str,
    name: &str,
    confirmation: Option<&str>,
    always_ask: bool,
) -> Result<(), CommandError> {
    let ask = match agent_access(session, plugin) {
        AgentAccess::None => {
            return Err(CommandError::with_code(
                AGENT_NOT_ALLOWED,
                t!("workbench-plugin-run-agent-none", plugin = plugin, name = name),
            ));
        }
        AgentAccess::Full => always_ask,
        AgentAccess::Ask => true,
    };
    if !ask {
        return Ok(());
    }
    let state = access(session);
    if let Some(token) = confirmation
        && state.allowed.remove(token)
    {
        state.asked.remove(token);
        return Ok(());
    }
    state.next += 1;
    let token = format!("run-{}-{}", state.next, crate::notifications::now_ms());
    state.asked.insert(token.clone(), name.to_owned());
    let notice = NewNotification::message(
        Group::General,
        Kind::Warning,
        t!("workbench-plugin-run-agent-ask", plugin = plugin, name = name),
    )
    .action(Action {
        id: "allow".to_owned(),
        label: t!("workbench-plugin-run-allow"),
        command: AllowAgentRun::ID.to_owned(),
        params: json!({ "token": token }),
        once: true,
    })
    .sticky();
    crate::notifications::post(session, notice);
    Err(CommandError::with_code(CONFIRMATION_NEEDED, t!("workbench-plugin-run-agent-asked", name = name))
        .with_data(json!({ "confirmation": token })))
}

/// The parameters of `plugins.runAccess.set`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetAccessParams {
    /// The plug-in's id.
    pub plugin: String,
    /// `none`, `ask` or `full`.
    pub access: AgentAccess,
}

/// What agents may do with each plug-in's run targets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AccessList {
    /// By plug-in id; a plug-in not listed is `none`.
    pub agents: BTreeMap<String, AgentAccess>,
}

pub struct GetAccess;

impl Command for GetAccess {
    type Params = zen_core::NoParams;
    type Output = AccessList;
    const ID: &'static str = "plugins.runAccess.get";
    const TITLE: &'static str = "Plug-in Run Access";
    const DESCRIPTION: &'static str =
        "What agents may do with each plug-in's run targets: none (the default), ask (a person says yes) or full.";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<AccessList, CommandError> {
        Ok(AccessList { agents: access(session).saved.agents.clone() })
    }
}

pub struct SetAccess;

impl Command for SetAccess {
    type Params = SetAccessParams;
    type Output = AccessList;
    const ID: &'static str = "plugins.runAccess.set";
    const TITLE: &'static str = "Set Plug-in Run Access";
    const DESCRIPTION: &'static str = "Sets what agents may do with a plug-in's run targets; the person's alone.";

    fn run(&self, session: &mut Session, params: SetAccessParams) -> Result<AccessList, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(AGENT_NOT_ALLOWED, t!("workbench-plugin-run-access-person")));
        }
        let state = access(session);
        if params.access == AgentAccess::None {
            state.saved.agents.remove(&params.plugin);
        } else {
            state.saved.agents.insert(params.plugin, params.access);
        }
        let list = AccessList { agents: state.saved.agents.clone() };
        if let Some(file) = access_file() {
            let bytes =
                serde_json::to_vec_pretty(&state.saved).map_err(|error| CommandError::new(error.to_string()))?;
            zen_vfs::write_atomic(&file, &bytes).map_err(|error| CommandError::new(error.to_string()))?;
        }
        Ok(list)
    }
}

/// The parameters of `plugins.runAccess.allow`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct AllowParams {
    /// The `confirmation` of the agent's refused start.
    pub token: String,
}

pub struct AllowAgentRun;

impl Command for AllowAgentRun {
    type Params = AllowParams;
    type Output = ();
    const ID: &'static str = "plugins.runAccess.allow";
    const TITLE: &'static str = "Allow an Agent's Run";
    const DESCRIPTION: &'static str = "The person's yes to an agent's start of a plug-in run that asked first.";

    fn run(&self, session: &mut Session, params: AllowParams) -> Result<(), CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(AGENT_NOT_ALLOWED, t!("workbench-plugin-run-access-person")));
        }
        let state = access(session);
        if !state.asked.contains_key(&params.token) {
            return Err(CommandError::new(t!("workbench-plugin-run-no-question")));
        }
        state.allowed.insert(params.token);
        Ok(())
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(GetAccess)?;
    registry.register(SetAccess)?;
    registry.register(AllowAgentRun)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;

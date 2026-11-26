use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::keymap::{Binding, Context, EXTERNAL_TOOL_ACTION};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::external_tool::{KIND, OutputFilter, ToolLaunch};
use zen_run::macros::{self, Macro, MacroContext};
use zen_run::{Common, Configuration, RunOutput};

use crate::disk::{SyncParams, SyncWithDisk};
use crate::keymaps::{self, SharedKeymaps};
use crate::run::{RunStarted, Runs, project_root, start, wait_for_run};
use crate::{Workbench, file_clipboard};

pub const FILE_NAME: &str = "externalTools.json";

pub const FORMAT_VERSION: u32 = 1;

pub const DEFAULT_GROUP: &str = "External Tools";

pub const RUN_ACTION: &str = "view.externalTools.run";

/// An external program set up once and run on the current file or project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ExternalTool {
    /// The stable id, kept when the tool is renamed; the keymap's action is `run.externalTools.run/<id>`.
    pub id: String,
    /// The name shown in menus and in the Run tool window's tab.
    pub name: String,
    /// The group: the context menus' submenu the tool is in; empty means `External Tools`.
    pub group: String,
    /// What it does, in the user's words.
    pub description: String,
    /// Whether it is shown in menus and can run.
    pub enabled: bool,
    /// The program: a path or a name found on `PATH`, with macros.
    pub program: String,
    /// The arguments as typed, split as a shell splits them (quotes keep spaces), with macros.
    pub arguments: String,
    /// The folder it runs in, with macros; the project's root when empty, relative to it otherwise.
    pub working_directory: String,
    /// Read the open files again from disk once the program has ended.
    pub synchronize: bool,
    /// Show the program's output in a Run tool window tab.
    pub open_console: bool,
    /// Bring the Run tool window forward when the program writes to its standard output.
    pub activate_on_stdout: bool,
    /// Bring the Run tool window forward when the program writes to its standard error.
    pub activate_on_stderr: bool,
    /// Regular expressions with `$FILE_PATH$`, `$LINE$` and `$COLUMN$` whose matches in the output become file links.
    pub output_filters: Vec<String>,
}

impl Default for ExternalTool {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            group: DEFAULT_GROUP.to_owned(),
            description: String::new(),
            enabled: true,
            program: String::new(),
            arguments: String::new(),
            working_directory: String::new(),
            synchronize: true,
            open_console: true,
            activate_on_stdout: false,
            activate_on_stderr: false,
            output_filters: Vec::new(),
        }
    }
}

impl ExternalTool {
    pub fn action(&self) -> String {
        format!("{EXTERNAL_TOOL_ACTION}{}", self.id)
    }

    pub fn in_default_group(&self) -> bool {
        let group = self.group.trim();
        group.is_empty() || group == DEFAULT_GROUP
    }

    pub fn prompts(&self) -> Vec<Macro> {
        let mut found = Vec::new();
        for text in [&self.program, &self.arguments, &self.working_directory] {
            for item in macros::used(text) {
                if item.is_prompt() && !found.contains(&item) {
                    found.push(item);
                }
            }
        }
        found
    }

    fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err(t!("workbench-external-tools-no-name"));
        }
        if self.program.trim().is_empty() {
            return Err(t!("workbench-external-tools-no-program", name = self.name.as_str()));
        }
        for filter in self.output_filters.iter().filter(|filter| !filter.trim().is_empty()) {
            OutputFilter::compile(filter).map_err(|error| {
                t!("workbench-external-tools-bad-filter", filter = filter.as_str(), error = error.as_str())
            })?;
        }
        Ok(())
    }

    fn binding(&self) -> Binding {
        Binding {
            action: self.action(),
            command: RUN_ACTION.to_owned(),
            params: serde_json::json!({ "toolId": self.id }),
            context: Context::Global,
            keystrokes: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Saved {
    format_version: u32,
    tools: Vec<ExternalTool>,
}

#[derive(Default)]
struct Inner {
    file: Option<PathBuf>,
    tools: Vec<ExternalTool>,
    generation: u64,
    notice: Option<String>,
    keymaps: Option<SharedKeymaps>,
}

#[derive(Clone, Default)]
pub struct ExternalTools {
    inner: Arc<Mutex<Inner>>,
}

fn read_saved(file: &Path) -> (Vec<ExternalTool>, Option<String>) {
    let bytes = match std::fs::read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return (Vec::new(), None),
        Err(error) => return (Vec::new(), Some(t!("workbench-external-tools-unreadable", detail = error.to_string()))),
    };
    match serde_json::from_slice::<Saved>(&bytes) {
        Ok(saved) => {
            let mut tools: Vec<ExternalTool> = Vec::new();
            for tool in saved.tools {
                if tool.id.is_empty() || tools.iter().any(|known| known.id == tool.id) {
                    continue;
                }
                tools.push(tool);
            }
            (tools, None)
        }
        Err(error) => {
            let kept = file.with_extension("json.corrupt");
            let kept = if std::fs::rename(file, &kept).is_ok() { kept } else { file.to_owned() };
            let notice =
                t!("workbench-external-tools-corrupt", detail = error.to_string(), file = kept.display().to_string());
            (Vec::new(), Some(notice))
        }
    }
}

fn slug(name: &str) -> String {
    let mut slug = String::new();
    for c in name.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    let slug = slug.trim_end_matches('-').to_owned();
    if slug.is_empty() { "tool".to_owned() } else { slug }
}

impl Inner {
    fn free_id(&self, name: &str) -> String {
        let base = slug(name);
        let mut id = base.clone();
        let mut suffix = 1;
        while self.tools.iter().any(|tool| tool.id == id) {
            suffix += 1;
            id = format!("{base}-{suffix}");
        }
        id
    }

    fn index(&self, id: &str) -> Result<usize, CommandError> {
        self.tools
            .iter()
            .position(|tool| tool.id == id)
            .ok_or_else(|| CommandError::new(t!("workbench-external-tools-unknown", id = id)))
    }

    fn publish(&self) {
        if let Some(store) = &self.keymaps {
            keymaps::lock(store).set_extra(self.tools.iter().map(ExternalTool::binding).collect());
        }
    }

    fn drop_shortcuts(&self, ids: &[String]) {
        if let Some(store) = &self.keymaps {
            let mut store = keymaps::lock(store);
            for id in ids {
                let _ = store.move_action(&format!("{EXTERNAL_TOOL_ACTION}{id}"), None);
            }
        }
    }

    fn save(&mut self) -> Result<(), CommandError> {
        self.generation += 1;
        self.publish();
        let Some(file) = &self.file else { return Ok(()) };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let saved = Saved { format_version: FORMAT_VERSION, tools: self.tools.clone() };
        let json = serde_json::to_vec_pretty(&saved).unwrap_or_default();
        zen_vfs::write_atomic(file, &json)
            .map_err(|error| CommandError::new(t!("workbench-external-tools-save-failed", detail = error.to_string())))
    }
}

impl ExternalTools {
    pub fn load(file: Option<PathBuf>) -> Self {
        let (tools, notice) = file.as_deref().map_or_else(|| (Vec::new(), None), read_saved);
        Self { inner: Arc::new(Mutex::new(Inner { file, tools, notice, ..Inner::default() })) }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn take_notice(&self) -> Option<String> {
        self.inner().notice.take()
    }

    pub fn connect_keymaps(&self, store: SharedKeymaps) {
        let mut inner = self.inner();
        inner.keymaps = Some(store);
        inner.publish();
    }

    pub fn tools(&self) -> Vec<ExternalTool> {
        self.inner().tools.clone()
    }

    pub fn get(&self, id: &str) -> Option<ExternalTool> {
        self.inner().tools.iter().find(|tool| tool.id == id).cloned()
    }

    pub fn generation(&self) -> u64 {
        self.inner().generation
    }

    pub fn file(&self) -> Option<PathBuf> {
        self.inner().file.clone()
    }

    pub fn save_tool(&self, mut tool: ExternalTool) -> Result<String, CommandError> {
        tool.validate().map_err(CommandError::new)?;
        let mut inner = self.inner();
        match inner.tools.iter().position(|known| !tool.id.is_empty() && known.id == tool.id) {
            Some(index) => inner.tools[index] = tool.clone(),
            None => {
                if tool.id.is_empty() {
                    tool.id = inner.free_id(&tool.name);
                }
                inner.tools.push(tool.clone());
            }
        }
        inner.save()?;
        Ok(tool.id)
    }

    pub fn replace(&self, tools: Vec<ExternalTool>) -> Result<Vec<ExternalTool>, CommandError> {
        for tool in &tools {
            tool.validate().map_err(CommandError::new)?;
        }
        let mut inner = self.inner();
        let before: Vec<String> = inner.tools.iter().map(|tool| tool.id.clone()).collect();
        let mut seen = BTreeSet::new();
        inner.tools = tools
            .into_iter()
            .map(|mut tool| {
                if !seen.insert(tool.id.clone()) {
                    tool.id.clear();
                }
                tool
            })
            .collect();
        for index in 0..inner.tools.len() {
            if inner.tools[index].id.is_empty() {
                let name = inner.tools[index].name.clone();
                inner.tools[index].id = inner.free_id(&name);
            }
        }
        let gone: Vec<String> =
            before.into_iter().filter(|id| !inner.tools.iter().any(|tool| tool.id == *id)).collect();
        inner.drop_shortcuts(&gone);
        inner.save()?;
        Ok(inner.tools.clone())
    }

    pub fn remove(&self, id: &str) -> Result<(), CommandError> {
        let mut inner = self.inner();
        let index = inner.index(id)?;
        inner.tools.remove(index);
        inner.drop_shortcuts(&[id.to_owned()]);
        inner.save()
    }

    pub fn move_tool(&self, id: &str, up: bool) -> Result<bool, CommandError> {
        let mut inner = self.inner();
        let index = inner.index(id)?;
        let group = inner.tools[index].group.clone();
        let neighbour = if up {
            inner.tools[..index].iter().rposition(|tool| tool.group == group)
        } else {
            inner.tools[index + 1..].iter().position(|tool| tool.group == group).map(|at| index + 1 + at)
        };
        let Some(neighbour) = neighbour else { return Ok(false) };
        inner.tools.swap(index, neighbour);
        inner.save()?;
        Ok(true)
    }

    pub fn copy(&self, id: &str) -> Result<String, CommandError> {
        let mut inner = self.inner();
        let index = inner.index(id)?;
        let mut copy = inner.tools[index].clone();
        copy.name = t!("workbench-external-tools-copy-name", name = copy.name.as_str());
        copy.id = inner.free_id(&copy.name);
        let new_id = copy.id.clone();
        inner.tools.insert(index + 1, copy);
        inner.save()?;
        Ok(new_id)
    }
}

#[derive(Default)]
struct Synchronized(BTreeSet<u64>);

pub fn settle(session: &mut Session) -> bool {
    let ended: Vec<u64> = session
        .state::<Runs>()
        .map(|runs| {
            runs.runs()
                .iter()
                .filter(|run| ToolLaunch::of(&run.configuration).is_some_and(|launch| launch.synchronize))
                .filter(|run| !run.output.read(RunOutput::is_running))
                .map(|run| run.id)
                .collect()
        })
        .unwrap_or_default();
    let synchronized = session.state_mut::<Synchronized>();
    synchronized.0.retain(|id| ended.contains(id));
    let fresh: Vec<u64> = ended.into_iter().filter(|id| !synchronized.0.contains(id)).collect();
    if fresh.is_empty() {
        return false;
    }
    synchronized.0.extend(fresh);
    let _ = SyncWithDisk.run(session, SyncParams::default());
    true
}

fn env_path(session: &Session, name: &str) -> Option<PathBuf> {
    let from_runs = session
        .state::<Runs>()
        .and_then(|runs| runs.env().env.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone()));
    from_runs
        .or_else(|| std::env::var(name).ok())
        .and_then(|value| std::env::split_paths(&value).find(|path| !path.as_os_str().is_empty()))
}

pub fn macro_context(session: &mut Session, file: Option<&Path>) -> MacroContext {
    let clipboard = Some(file_clipboard::clipboard_text(session)).filter(|text| !text.is_empty());
    let workbench = session.state::<Workbench>();
    let project = workbench.and_then(Workbench::project).map(Path::to_owned);
    let active = workbench.and_then(Workbench::active_document);
    let mut context = MacroContext {
        project,
        clipboard,
        goroot: env_path(session, "GOROOT"),
        gopath: env_path(session, "GOPATH")
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join("go"))),
        temp_dir: Some(std::env::temp_dir()),
        ..MacroContext::default()
    };
    let editor_file = active.map(|document| document.path().to_owned());
    context.file = file.map(Path::to_owned).or_else(|| editor_file.clone());
    context.go_import_path = context.file.as_deref().and_then(Path::parent).and_then(macros::go_import_path);
    if let Some(document) = active.filter(|_| context.file == editor_file) {
        let text = document.snapshot();
        let primary = document.selections().primary();
        if let Ok(point) = text.offset_to_point(primary.head) {
            context.line = u32::try_from(point.line + 1).ok();
            context.column = u32::try_from(point.column + 1).ok();
        }
        if !primary.is_empty() {
            context.selection = text.slice(primary.range()).ok().map(|slice| slice.into_owned());
        }
    }
    // WHY: the person's clipboard and selection can hold secrets or text of files hidden from agents.
    if zen_core::macros::by_agent(session) {
        context.clipboard = None;
        context.selection = None;
    }
    context
}

fn macro_description(item: Macro) -> String {
    let key = match item {
        Macro::FilePath => "workbench-macro-file-path",
        Macro::FileDir => "workbench-macro-file-dir",
        Macro::FileName => "workbench-macro-file-name",
        Macro::FileNameWithoutExtension => "workbench-macro-file-name-without-extension",
        Macro::ProjectFileDir => "workbench-macro-project-file-dir",
        Macro::ProjectName => "workbench-macro-project-name",
        Macro::SelectedText => "workbench-macro-selected-text",
        Macro::LineNumber => "workbench-macro-line-number",
        Macro::ColumnNumber => "workbench-macro-column-number",
        Macro::Prompt => "workbench-macro-prompt",
        Macro::FilePrompt => "workbench-macro-file-prompt",
        Macro::ClipboardContent => "workbench-macro-clipboard-content",
        Macro::GoRoot => "workbench-macro-goroot",
        Macro::GoPath => "workbench-macro-gopath",
        Macro::GoExecPath => "workbench-macro-go-exec-path",
        Macro::GoFileImportPath => "workbench-macro-go-file-import-path",
        Macro::OsName => "workbench-macro-os-name",
        Macro::TempDir => "workbench-macro-temp-dir",
    };
    t!(key)
}

pub fn missing_reason(item: Macro) -> String {
    let key = match item {
        Macro::GoFileImportPath => "workbench-macro-missing-go-module",
        _ if item.needs_file() && item != Macro::SelectedText => "workbench-macro-missing-file",
        Macro::SelectedText => "workbench-macro-missing-selection",
        Macro::ProjectFileDir | Macro::ProjectName => "workbench-macro-missing-project",
        Macro::Prompt | Macro::FilePrompt => "workbench-macro-missing-prompt",
        Macro::ClipboardContent => "workbench-macro-missing-clipboard",
        _ => "workbench-macro-missing-go",
    };
    t!(key)
}

fn no_value(tool: &str, item: Macro) -> CommandError {
    CommandError::new(t!(
        "workbench-external-tools-no-value",
        name = tool,
        token = item.token(),
        reason = missing_reason(item)
    ))
}

pub fn launch(tool: &ExternalTool, context: &MacroContext) -> Result<ToolLaunch, CommandError> {
    let fail = |item: Macro| no_value(&tool.name, item);
    let program = macros::expand(tool.program.trim(), context).map_err(fail)?;
    let args = macros::expand_arguments(&tool.arguments, context).map_err(fail)?;
    let folder = macros::expand(tool.working_directory.trim(), context).map_err(fail)?;
    let root = context.project.clone().unwrap_or_default();
    let cwd = if folder.is_empty() { root } else { root.join(folder) };
    Ok(ToolLaunch {
        tool_id: tool.id.clone(),
        program: PathBuf::from(program),
        args,
        cwd,
        filters: tool.output_filters.iter().filter(|filter| !filter.trim().is_empty()).cloned().collect(),
        synchronize: tool.synchronize,
        open_console: tool.open_console,
        activate_on_stdout: tool.activate_on_stdout,
        activate_on_stderr: tool.activate_on_stderr,
        env: Vec::new(),
        console_on_error: false,
    })
}

pub fn configuration(tool: &ExternalTool, launch: &ToolLaunch) -> Configuration {
    Configuration {
        id: format!("externalTool:{}", tool.id),
        name: tool.name.clone(),
        kind: KIND.to_owned(),
        temporary: false,
        common: Common::default(),
        options: serde_json::to_value(launch).unwrap_or_default(),
        ..Configuration::default()
    }
}

fn store(session: &mut Session) -> ExternalTools {
    session.state_mut::<ExternalTools>().clone()
}

/// What `run.externalTools.list` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolList {
    /// The tools in the user's order.
    pub tools: Vec<ExternalTool>,
    /// The file they are kept in, if the IDE has a data folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
}

pub struct ListTools;

impl Command for ListTools {
    type Params = NoParams;
    type Output = ToolList;
    const ID: &'static str = "run.externalTools.list";
    const TITLE: &'static str = "External Tools";
    const DESCRIPTION: &'static str = "Lists the external tools of Settings → Tools → External Tools in their order: id, name, group, description, enabled, program, arguments, workingDirectory, the advanced options and outputFilters, with the file they are kept in.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ToolList, CommandError> {
        let tools = store(session);
        Ok(ToolList { tools: tools.tools(), file: tools.file() })
    }
}

/// The parameters of `run.externalTools.save`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveParams {
    /// The tool; an empty or unknown id adds a new one, a known id replaces that tool.
    pub tool: ExternalTool,
}

/// A tool's id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolId {
    /// The tool's id.
    pub id: String,
}

pub struct SaveTool;

impl Command for SaveTool {
    type Params = SaveParams;
    type Output = ToolId;
    const ID: &'static str = "run.externalTools.save";
    const TITLE: &'static str = "Save External Tool";
    const DESCRIPTION: &'static str = "Adds an external tool or changes one, by id (Create Tool and Edit Tool): it needs a name and a program, and every output filter must be a regular expression with $FILE_PATH$. Returns the id; its keymap action is run.externalTools.run/<id>.";

    fn run(&self, session: &mut Session, params: SaveParams) -> Result<ToolId, CommandError> {
        Ok(ToolId { id: store(session).save_tool(params.tool)? })
    }
}

/// The parameters of `run.externalTools.replace`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceParams {
    /// Every tool, in the order wanted.
    pub tools: Vec<ExternalTool>,
}

pub struct ReplaceTools;

impl Command for ReplaceTools {
    type Params = ReplaceParams;
    type Output = ToolList;
    const ID: &'static str = "run.externalTools.replace";
    const TITLE: &'static str = "Apply External Tools";
    const DESCRIPTION: &'static str = "Replaces the whole list of external tools, as OK in Settings → Tools → External Tools does; a tool without an id gets one. Returns the list.";

    fn run(&self, session: &mut Session, params: ReplaceParams) -> Result<ToolList, CommandError> {
        let tools = store(session);
        Ok(ToolList { tools: tools.replace(params.tools)?, file: tools.file() })
    }
}

pub struct RemoveTool;

impl Command for RemoveTool {
    type Params = ToolId;
    type Output = NoParams;
    const ID: &'static str = "run.externalTools.remove";
    const TITLE: &'static str = "Remove External Tool";
    const DESCRIPTION: &'static str = "Removes an external tool (− in Settings → Tools → External Tools).";

    fn run(&self, session: &mut Session, params: ToolId) -> Result<NoParams, CommandError> {
        store(session).remove(&params.id)?;
        Ok(NoParams {})
    }
}

/// The parameters of `run.externalTools.move`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MoveParams {
    /// The tool's id.
    pub id: String,
    /// Up, else down.
    pub up: bool,
}

/// Whether something changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Moved {
    /// False when the tool was first (or last) in its group already.
    pub moved: bool,
}

pub struct MoveTool;

impl Command for MoveTool {
    type Params = MoveParams;
    type Output = Moved;
    const ID: &'static str = "run.externalTools.move";
    const TITLE: &'static str = "Move External Tool";
    const DESCRIPTION: &'static str = "Moves an external tool one place up or down among the tools of its group (Up and Down in Settings → Tools → External Tools).";

    fn run(&self, session: &mut Session, params: MoveParams) -> Result<Moved, CommandError> {
        Ok(Moved { moved: store(session).move_tool(&params.id, params.up)? })
    }
}

pub struct CopyTool;

impl Command for CopyTool {
    type Params = ToolId;
    type Output = ToolId;
    const ID: &'static str = "run.externalTools.copy";
    const TITLE: &'static str = "Copy External Tool";
    const DESCRIPTION: &'static str =
        "Adds a copy of an external tool right after it, named as a copy; returns the copy's id.";

    fn run(&self, session: &mut Session, params: ToolId) -> Result<ToolId, CommandError> {
        Ok(ToolId { id: store(session).copy(&params.id)? })
    }
}

/// A macro as the Macros dialog lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MacroInfo {
    /// The macro as written, e.g. `$FilePath$`.
    pub token: String,
    /// What it stands for.
    pub description: String,
    /// Its value now; absent when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Why it has no value, when it has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing: Option<String>,
}

/// The parameters of `run.externalTools.macros`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MacrosParams {
    /// The file to take the values from instead of the editor's.
    pub file: Option<PathBuf>,
}

pub struct ListMacros;

impl Command for ListMacros {
    type Params = MacrosParams;
    type Output = Vec<MacroInfo>;
    const ID: &'static str = "run.externalTools.macros";
    const TITLE: &'static str = "Macros";
    const DESCRIPTION: &'static str = "Lists the macros of external tools and run configurations (Insert Macro) with what each stands for and its value now, from the editor's file, caret and selection, the project, the clipboard, the Go SDK and the system; a macro without a value says why.";

    fn run(&self, session: &mut Session, params: MacrosParams) -> Result<Vec<MacroInfo>, CommandError> {
        let context = macro_context(session, params.file.as_deref());
        Ok(Macro::ALL
            .into_iter()
            .map(|item| {
                let value = if item.is_prompt() { None } else { context.value(item) };
                MacroInfo {
                    token: item.token(),
                    description: macro_description(item),
                    missing: value.is_none().then(|| missing_reason(item)),
                    value,
                }
            })
            .collect())
    }
}

/// The parameters of `run.externalTools.preview`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewParams {
    /// The program, with macros.
    pub program: String,
    /// The arguments, with macros.
    pub arguments: String,
    /// The working directory, with macros.
    pub working_directory: String,
    /// The file to take the values from instead of the editor's.
    pub file: Option<PathBuf>,
}

/// What `run.externalTools.preview` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The program expanded.
    pub program: String,
    /// The arguments, one per item, as the program gets them.
    pub arguments: Vec<String>,
    /// The folder it would run in.
    pub working_directory: String,
    /// The macros without a value now, written `<Name>` in the preview.
    pub missing: Vec<String>,
}

pub struct PreviewTool;

impl Command for PreviewTool {
    type Params = PreviewParams;
    type Output = Preview;
    const ID: &'static str = "run.externalTools.preview";
    const TITLE: &'static str = "Preview External Tool";
    const DESCRIPTION: &'static str = "Expands a tool's program, arguments and working directory now, as the Create Tool dialog previews them: the arguments one per item as the program gets them, a macro without a value written <Name> and listed in missing.";

    fn run(&self, session: &mut Session, params: PreviewParams) -> Result<Preview, CommandError> {
        let context = macro_context(session, params.file.as_deref());
        let mut missing = Vec::new();
        for text in [&params.program, &params.arguments, &params.working_directory] {
            for item in macros::used(text) {
                let token = item.token();
                if context.value(item).is_none() && !missing.contains(&token) {
                    missing.push(token);
                }
            }
        }
        let folder = macros::preview(params.working_directory.trim(), &context);
        let root = context.project.clone().unwrap_or_default();
        let folder = if folder.is_empty() { root } else { root.join(folder) };
        Ok(Preview {
            program: macros::preview(params.program.trim(), &context),
            arguments: macros::preview_arguments(&params.arguments, &context),
            working_directory: folder.display().to_string(),
            missing,
        })
    }
}

/// The parameters of `run.externalTools.run`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunParams {
    /// The tool's id.
    pub id: String,
    /// The file the file macros name instead of the editor's, as from the Project view's context menu.
    pub file: Option<PathBuf>,
    /// The answer to `$Prompt$`.
    pub prompt: Option<String>,
    /// The answer to `$FilePrompt$`.
    pub file_prompt: Option<PathBuf>,
    /// Waits up to this long for the run to end (at most 10 minutes) and answers its summary as well.
    pub wait_ms: Option<u64>,
}

pub struct RunTool;

impl Command for RunTool {
    type Params = RunParams;
    type Output = RunStarted;
    const ID: &'static str = "run.externalTools.run";
    const TITLE: &'static str = "Run External Tool";
    const DESCRIPTION: &'static str = "Runs an enabled external tool: saves the files, expands its macros (prompt and filePrompt answer $Prompt$ and $FilePrompt$; a macro without a value fails the run with why), starts the program with the arguments as separate items, never through a shell, in a Run tool window tab named after the tool, and returns the run's id for run.output.get. With waitMs it answers once the run has ended, after Synchronize read the files again.";

    fn run(&self, session: &mut Session, params: RunParams) -> Result<RunStarted, CommandError> {
        crate::trust::guard_agent_project(session)?;
        let tool = store(session)
            .get(&params.id)
            .ok_or_else(|| CommandError::new(t!("workbench-external-tools-unknown", id = params.id.as_str())))?;
        if !tool.enabled {
            return Err(CommandError::new(t!("workbench-external-tools-disabled", name = tool.name.as_str())));
        }
        project_root(session)?;
        let mut context = macro_context(session, params.file.as_deref());
        context.prompt = params.prompt;
        context.file_prompt = params.file_prompt;
        let launch = launch(&tool, &context)?;
        let started = start(session, configuration(&tool, &launch))?;
        match params.wait_ms {
            Some(wait) => {
                let summary = wait_for_run(session, started.run_id, Duration::from_millis(wait))?;
                settle(session);
                Ok(RunStarted { summary: Some(summary), ..started })
            }
            None => Ok(started),
        }
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTools)?;
    registry.register(SaveTool)?;
    registry.register(ReplaceTools)?;
    registry.register(RemoveTool)?;
    registry.register(MoveTool)?;
    registry.register(CopyTool)?;
    registry.register(ListMacros)?;
    registry.register(PreviewTool)?;
    registry.register(RunTool)?;
    Ok(())
}

#[cfg(test)]
mod tests;

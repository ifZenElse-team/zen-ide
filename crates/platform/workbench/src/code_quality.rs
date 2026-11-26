use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_modules::form::{Action, Choice, Form, Item, Tone};
use zen_project::support::DeclaredFormatter;

use crate::Workbench;
use crate::language::servers::{LanguageServers, ServerSource, project_program};
use crate::plugin_projects::PluginProjects;

pub const PAGE: &str = "codeQuality";

pub const FORMATTER_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectFormatter {
    pub plugin: String,
    pub declared: DeclaredFormatter,
    pub program: PathBuf,
    pub root: PathBuf,
}

pub fn formatter_for(session: &Session, path: &Path, on_save: bool) -> Option<ProjectFormatter> {
    let root = session.state::<Workbench>()?.project()?.to_owned();
    if !crate::trust::trusted(session, &root) {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    let plugins = session.state::<PluginProjects>()?.supporting(&root);
    plugins.into_iter().find_map(|plugin| {
        plugin.support.formatters.into_iter().find_map(|declared| {
            if !declared.formats(&name) {
                return None;
            }
            let on = if on_save {
                !declared.on_save.is_empty() && session.settings().get_or(&declared.on_save, false)
            } else {
                !declared.fixer
                    && (declared.on_reformat.is_empty() || session.settings().get_or(&declared.on_reformat, true))
            };
            if !on {
                return None;
            }
            let program = project_program(&declared.command, &root)?;
            Some(ProjectFormatter { plugin: plugin.name.clone(), declared, program, root: root.clone() })
        })
    })
}

pub struct FormatterRun {
    pub path: PathBuf,
    pub document_version: u64,
    pub name: String,
    pub text: String,
    answer: Receiver<Result<String, String>>,
}

impl FormatterRun {
    pub fn start(formatter: &ProjectFormatter, path: &Path, text: String, document_version: u64) -> Self {
        let (sender, answer) = std::sync::mpsc::channel();
        let relative = path.strip_prefix(&formatter.root).unwrap_or(path);
        let (program, args, root) =
            (formatter.program.clone(), formatter.declared.args_for(relative), formatter.root.clone());
        let kept = text.clone();
        std::thread::spawn(move || {
            let _ = sender.send(run(&program, &args, &root, &text));
        });
        Self { path: path.to_owned(), document_version, name: formatter.declared.name.clone(), text: kept, answer }
    }

    pub fn wait(&self, wait: Duration) -> Option<Result<String, String>> {
        match self.answer.recv_timeout(wait) {
            Ok(answer) => Some(answer),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Err(t!("workbench-format-formatter-stopped"))),
        }
    }
}

fn run(program: &Path, args: &[String], root: &Path, text: &str) -> Result<String, String> {
    let mut child = Process::new(program)
        .args(args)
        .current_dir(root)
        .envs(zen_lsp::servers::default_environment())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        let input = text.to_owned();
        std::thread::spawn(move || {
            let _ = stdin.write_all(input.as_bytes());
        });
    }
    let mut errors = String::new();
    let stderr = child.stderr.take();
    let reader = std::thread::spawn(move || {
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_string(&mut errors);
        }
        errors
    });
    let mut output = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        stdout.read_to_string(&mut output).map_err(|error| error.to_string())?;
    }
    let status = child.wait().map_err(|error| error.to_string())?;
    let errors = reader.join().unwrap_or_default();
    if status.success() {
        Ok(output)
    } else {
        let detail = errors.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default().to_owned();
        Err(if detail.is_empty() { status.to_string() } else { detail })
    }
}

pub fn apply_formatted(
    workbench: &mut Workbench,
    path: &Path,
    document_version: u64,
    formatted: &str,
) -> Result<bool, String> {
    let document = workbench
        .documents
        .iter_mut()
        .find(|document| document.path() == path)
        .ok_or_else(|| t!("workbench-error-no-document"))?;
    if document.version() != document_version {
        return Err(t!("workbench-format-outdated"));
    }
    let old: Vec<char> = document.text().chars().collect();
    let new: Vec<char> = formatted.chars().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..].iter().rev().zip(new[prefix..].iter().rev()).take_while(|(a, b)| a == b).count();
    if prefix == old.len() && prefix == new.len() {
        return Ok(false);
    }
    let middle: String = new[prefix..new.len() - suffix].iter().collect();
    Ok(document.replace_ranges(vec![(prefix..old.len() - suffix, middle)]))
}

/// A linter that serves files alongside their language's server, as `codeQuality.tools.list` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LinterInfo {
    /// The server's id, for `lsp.server.configure`.
    pub id: String,
    /// Its name.
    pub name: String,
    /// Whether it is on (Automatic configuration) or off (Disabled).
    pub enabled: bool,
    /// Where its program is: `local`, `path`, `custom`, `project` or `missing`.
    pub source: ServerSource,
    /// Whether it formats the files it serves (Reformat Code).
    pub formats: bool,
    /// The setting that applies its fixes on save, and whether it is on.
    pub fix_on_save: Option<(String, bool)>,
}

/// A project's formatter, as `codeQuality.tools.list` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormatterInfo {
    /// The plug-in that declares it.
    pub plugin: String,
    /// Its id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The project's program, when the project has it.
    pub program: Option<PathBuf>,
    /// The extensions of the files it formats.
    pub extensions: Vec<String>,
    /// The setting that makes Reformat Code use it, and whether it is on.
    pub on_reformat: Option<(String, bool)>,
    /// The setting that makes saving run it, and whether it is on.
    pub on_save: Option<(String, bool)>,
}

/// What `codeQuality.tools.list` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolList {
    /// The linters the IDE knows.
    pub linters: Vec<LinterInfo>,
    /// The formatters of the plug-ins that support the open project.
    pub formatters: Vec<FormatterInfo>,
}

fn tools(session: &Session) -> ToolList {
    let registry = session.state::<LanguageServers>().cloned().unwrap_or_default();
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
    let linters = registry
        .declarations()
        .into_iter()
        .filter(|server| server.alongside)
        .map(|server| {
            let source = match registry.executable_in(&server.id, root.as_deref()) {
                Some(_) if server.from_project && registry.configured(&server.id).path.is_none() => {
                    ServerSource::Project
                }
                _ => registry.source(&server.id),
            };
            LinterInfo {
                enabled: registry.is_enabled(&server.id),
                fix_on_save: server
                    .fix_on_save
                    .as_ref()
                    .map(|fix| (fix.setting.clone(), session.settings().get_or(&fix.setting, false))),
                source,
                formats: server.formats,
                id: server.id,
                name: server.name,
            }
        })
        .collect();
    let plugins = match (&root, session.state::<PluginProjects>()) {
        (Some(root), Some(projects)) => projects.supporting(root),
        _ => Vec::new(),
    };
    let setting =
        |key: &str, default: bool| (!key.is_empty()).then(|| (key.to_owned(), session.settings().get_or(key, default)));
    let formatters = plugins
        .into_iter()
        .flat_map(|plugin| plugin.support.formatters.into_iter().map(move |declared| (plugin.name.clone(), declared)))
        .map(|(plugin, declared)| FormatterInfo {
            program: root.as_deref().and_then(|root| project_program(&declared.command, root)),
            on_reformat: setting(&declared.on_reformat, true),
            on_save: setting(&declared.on_save, false),
            extensions: declared.extensions.clone(),
            id: declared.id,
            name: declared.name,
            plugin,
        })
        .collect();
    ToolList { linters, formatters }
}

pub struct ListTools;

impl Command for ListTools {
    type Params = NoParams;
    type Output = ToolList;
    const ID: &'static str = "codeQuality.tools.list";
    const TITLE: &'static str = "List Code Quality Tools";
    const DESCRIPTION: &'static str = "The linters that serve files alongside their language's server (ESLint, Biome: id, \
         on or off, where the program is, whether it formats, the setting of its fixes on save) and the formatters of \
         the plug-ins that support the open project (Prettier: the project's program, its files, the settings that run \
         it on Reformat Code and on save). Turn a linter on or off with lsp.server.configure; the settings are kept in \
         settings.json (declaredSettings) and set with set_settings.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ToolList, CommandError> {
        Ok(tools(session))
    }
}

pub struct CodeQualityForm;

impl Command for CodeQualityForm {
    type Params = NoParams;
    type Output = Form;
    const ID: &'static str = "codeQuality.settings.form";
    const TITLE: &'static str = "Code Quality Tools Settings";
    const DESCRIPTION: &'static str = "The page of Settings → Languages & Frameworks → Code Quality Tools as a form: for each \
         linter, Automatic configuration or Disabled and its fixes on save; for each formatter of the project, whether \
         Reformat Code and saving run it.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Form, CommandError> {
        Ok(form(&tools(session)))
    }
}

fn form(tools: &ToolList) -> Form {
    let mut items = Vec::new();
    if tools.linters.is_empty() && tools.formatters.is_empty() {
        items.push(Item::Text { text: t!("code-quality-none"), tone: Tone::Secondary, spinner: false });
    }
    for linter in &tools.linters {
        items.push(Item::Group { title: linter.name.clone() });
        items.push(Item::Choice {
            label: linter.name.clone(),
            options: vec![
                Choice { value: "on".to_owned(), label: t!("code-quality-automatic", tool = linter.name.as_str()) },
                Choice { value: "off".to_owned(), label: t!("code-quality-disabled", tool = linter.name.as_str()) },
            ],
            selected: Some(if linter.enabled { "on" } else { "off" }.to_owned()),
            action: Action::with_value(SetLinter::ID, "mode").param("id", linter.id.clone()),
        });
        let found = match linter.source {
            ServerSource::Missing => t!("code-quality-not-found", tool = linter.name.as_str()),
            ServerSource::Project => t!("code-quality-in-project", tool = linter.name.as_str()),
            _ => t!("code-quality-installed", tool = linter.name.as_str()),
        };
        items.push(Item::Text { text: found, tone: Tone::Secondary, spinner: false });
        if let Some((key, _)) = &linter.fix_on_save {
            items.push(Item::Setting {
                key: key.clone(),
                label: t!("code-quality-fix-on-save", tool = linter.name.as_str()),
                default: false,
            });
        }
    }
    for formatter in &tools.formatters {
        items.push(Item::Group { title: formatter.name.clone() });
        let found = match &formatter.program {
            Some(program) => {
                t!("code-quality-formatter-found", tool = formatter.name.as_str(), path = program.display().to_string())
            }
            None => t!("code-quality-formatter-missing", tool = formatter.name.as_str()),
        };
        items.push(Item::Text { text: found, tone: Tone::Secondary, spinner: false });
        if let Some((key, _)) = &formatter.on_reformat {
            items.push(Item::Setting { key: key.clone(), label: t!("code-quality-on-reformat"), default: true });
        }
        if let Some((key, _)) = &formatter.on_save {
            items.push(Item::Setting { key: key.clone(), label: t!("code-quality-on-save"), default: false });
        }
        items.push(Item::Text {
            text: t!("code-quality-files", extensions = formatter.extensions.join(", ")),
            tone: Tone::Secondary,
            spinner: false,
        });
    }
    Form { items, busy: false }
}

/// The parameters of `codeQuality.linter.set`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetLinterParams {
    /// The linter's server id, from `codeQuality.tools.list`.
    pub id: String,
    /// `on` (Automatic configuration) or `off` (Disabled).
    pub mode: String,
}

pub struct SetLinter;

impl Command for SetLinter {
    type Params = SetLinterParams;
    type Output = NoParams;
    const ID: &'static str = "codeQuality.linter.set";
    const TITLE: &'static str = "Set Linter Configuration";
    const DESCRIPTION: &'static str = "Turns a linter on (`on`, Automatic configuration) or off (`off`, Disabled), as the Code Quality Tools page does; the same as lsp.server.configure with enabled.";

    fn run(&self, session: &mut Session, params: SetLinterParams) -> Result<NoParams, CommandError> {
        let enabled = match params.mode.as_str() {
            "on" => true,
            "off" => false,
            other => return Err(CommandError::new(t!("code-quality-bad-mode", mode = other))),
        };
        let configure =
            crate::language::servers::ConfigureParams { id: params.id, path: None, args: None, enabled: Some(enabled) };
        crate::language::servers::ConfigureServer.run(session, configure)
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListTools)?;
    registry.register(CodeQualityForm)?;
    registry.register(SetLinter)?;
    Ok(())
}

#[cfg(test)]
mod tests;

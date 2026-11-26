use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::file_actions::{Revealed, reveal};
use crate::ide_log;
use crate::ide_settings::zip::ZipWriter;
use crate::language::servers::LanguageServers;
use crate::run::Runs;
use crate::tasks;

const SECRET_WORDS: [&str; 10] = [
    "password",
    "passphrase",
    "passwd",
    "secret",
    "token",
    "apikey",
    "credential",
    "privatekey",
    "accesskey",
    "cookie",
];
const HIDDEN: &str = "<hidden>";

#[derive(Debug, Default)]
pub struct Diagnostics {
    data: Option<PathBuf>,
    product: String,
    version: String,
}

impl Diagnostics {
    pub fn set_product(&mut self, data: PathBuf, product: &str, version: &str) {
        self.data = Some(data);
        self.product = product.to_owned();
        self.version = version.to_owned();
    }

    fn data(&self) -> Result<&Path, CommandError> {
        self.data.as_deref().ok_or_else(|| CommandError::new(t!("diagnostics-no-data-folder")))
    }

    fn logs(&self) -> Result<PathBuf, CommandError> {
        match ide_log::installed_folder() {
            Some(folder) => Ok(folder.to_owned()),
            None => self.data().map(|data| data.join("logs")),
        }
    }
}

fn state(session: &Session) -> Result<&Diagnostics, CommandError> {
    session.state::<Diagnostics>().ok_or_else(|| CommandError::new(t!("diagnostics-no-data-folder")))
}

/// Whether to start the file manager.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ShowLogParams {
    /// Only answers with the command line.
    pub dry_run: bool,
}

pub struct ShowLog;

impl Command for ShowLog {
    type Params = ShowLogParams;
    type Output = Revealed;
    const ID: &'static str = "diagnostics.log.show";
    const TITLE: &'static str = "Show Log";
    const DESCRIPTION: &'static str = "Shows the IDE's current log file selected in the system's file manager; \
         `dryRun` only answers with the command line.";

    fn run(&self, session: &mut Session, params: ShowLogParams) -> Result<Revealed, CommandError> {
        let folder = state(session)?.logs()?;
        std::fs::create_dir_all(&folder).map_err(|error| CommandError::new(error.to_string()))?;
        let file = folder.join(ide_log::FILE_NAME);
        if !file.exists() {
            std::fs::write(&file, "").map_err(|error| CommandError::new(error.to_string()))?;
        }
        reveal(&file, params.dry_run)
    }
}

fn hide_secrets(value: &mut Value) {
    match value {
        Value::Object(fields) => hide_in_fields(fields),
        Value::Array(items) => items.iter_mut().for_each(hide_secrets),
        Value::String(text) => *text = ide_log::without_credentials(text),
        _ => {}
    }
}

fn hide_in_fields(fields: &mut Map<String, Value>) {
    for (name, value) in fields {
        let lower = name.to_lowercase().replace(['_', '-'], "");
        if SECRET_WORDS.iter().any(|word| lower.contains(word)) {
            *value = Value::String(HIDDEN.to_owned());
        } else {
            hide_secrets(value);
        }
    }
}

fn settings_without_secrets(data: &Path) -> Option<Vec<u8>> {
    let text = std::fs::read(data.join("settings.json")).ok()?;
    let mut settings: Value = serde_json::from_slice(&text).ok()?;
    hide_secrets(&mut settings);
    serde_json::to_vec_pretty(&settings).ok()
}

/// Where to write the archive.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectParams {
    /// The ZIP file; a dated file in the log folder when absent.
    pub path: Option<PathBuf>,
    /// Does not show the archive in the file manager.
    pub no_reveal: bool,
    /// Waits until the file is written; otherwise a background task writes it.
    pub wait: bool,
}

/// The archive Collect Logs and Diagnostic Data writes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Collected {
    /// The ZIP file.
    pub path: PathBuf,
    /// What it holds; the IDE's log files only with `wait`.
    pub entries: Vec<String>,
    /// Its size in bytes, with `wait`.
    pub bytes: Option<u64>,
}

type Files = Vec<(String, Vec<u8>)>;

fn log_files(folder: &Path) -> Files {
    let mut names = vec![ide_log::FILE_NAME.to_owned()];
    names.extend((1..=ide_log::ROTATED).map(ide_log::rotated_name));
    names
        .into_iter()
        .filter_map(|name| Some((format!("logs/{name}"), std::fs::read(folder.join(&name)).ok()?)))
        .collect()
}

fn server_logs(session: &mut Session) -> Files {
    let servers = session.state_mut::<LanguageServers>().clone();
    let logs = servers.logged_ids().into_iter().map(|id| (servers.log(&id).since(0).0, id));
    let text = |lines: Vec<String>| ide_log::without_credentials(&lines.join("\n")).into_bytes();
    logs.filter(|(lines, _)| !lines.is_empty())
        .map(|(lines, id)| (format!("language-servers/{}.log", safe_name(&id)), text(lines)))
        .collect()
}

fn run_outputs(session: &Session) -> Files {
    let Some(runs) = session.state::<Runs>() else { return Vec::new() };
    let text = |run: &crate::run::Run| {
        let lines = run
            .output
            .read(|output| output.console().since(0).iter().map(|line| line.text.clone()).collect::<Vec<_>>());
        ide_log::without_credentials(&lines.join("\n"))
    };
    let named = |run: &crate::run::Run| format!("runs/{}-{}.log", run.id, safe_name(&run.title()));
    runs.runs().iter().map(|run| (named(run), text(run).into_bytes())).collect()
}

fn safe_name(name: &str) -> String {
    name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect()
}

fn versions(product: &str, version: &str) -> Vec<u8> {
    format!("{product} {version}\n{} {}\n", std::env::consts::OS, std::env::consts::ARCH).into_bytes()
}

fn default_path(folder: &Path) -> PathBuf {
    let stamp = jiff::Timestamp::now().strftime("%Y%m%d-%H%M%S");
    folder.join(format!("diagnostics-{stamp}.zip"))
}

struct Bundle {
    files: Files,
    folder: PathBuf,
    path: PathBuf,
    reveal: bool,
}

impl Bundle {
    fn write(mut self) -> Result<Collected, String> {
        self.files.extend(log_files(&self.folder));
        let seconds = u64::try_from(jiff::Timestamp::now().as_second()).unwrap_or(0);
        let mut writer = ZipWriter::new(seconds);
        for (name, bytes) in &self.files {
            writer.add(name, bytes).map_err(|error| format!("{name}: {error:?}"))?;
        }
        let bytes = writer.finish();
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut file = ide_log::private(std::fs::File::options().create(true).write(true).truncate(true))
            .open(&self.path)
            .map_err(|error| error.to_string())?;
        std::io::Write::write_all(&mut file, &bytes).map_err(|error| error.to_string())?;
        if self.reveal {
            reveal(&self.path, false).map_err(|error| error.message)?;
        }
        let entries = self.files.into_iter().map(|(name, _)| name).collect();
        Ok(Collected { path: self.path, entries, bytes: Some(bytes.len() as u64) })
    }
}

fn chosen_path(session: &mut Session, path: &Path) -> Result<PathBuf, CommandError> {
    let workbench = session.state_mut::<crate::Workbench>();
    let path = workbench.resolve(path);
    if !zen_core::macros::by_agent(session) {
        return Ok(path);
    }
    let refused = || CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("diagnostics-agent-path"));
    let root = session.state::<crate::Workbench>().and_then(crate::Workbench::project).map(Path::to_owned);
    let root = root.ok_or_else(refused)?;
    if !crate::inside_project(&root, &path) || path.exists() || crate::guard::refuses_in(Some(&root), &path) {
        return Err(refused());
    }
    crate::hidden_files::guard_change(session, &root, &[&path])?;
    Ok(path)
}

pub struct CollectLogs;

impl Command for CollectLogs {
    type Params = CollectParams;
    type Output = Collected;
    const ID: &'static str = "diagnostics.logs.collect";
    const TITLE: &'static str = "Collect Logs and Diagnostic Data";
    const DESCRIPTION: &'static str = "Writes a ZIP of the IDE's logs, the language servers' logs, the output of this \
         window's runs, the versions and the settings with their secrets hidden, and shows it in the file manager \
         unless `noReveal`. Nothing is uploaded. A background task writes it unless `wait`. An agent's `path` must be a \
         new file inside the project, not hidden from agents. Answers the file.";

    fn run(&self, session: &mut Session, params: CollectParams) -> Result<Collected, CommandError> {
        let diagnostics = state(session)?;
        let (data, folder) = (diagnostics.data()?.to_owned(), diagnostics.logs()?);
        let version_file = versions(&diagnostics.product, &diagnostics.version);
        let mut files = server_logs(session);
        files.extend(run_outputs(session));
        files.push(("versions.txt".to_owned(), version_file));
        files.extend(settings_without_secrets(&data).map(|text| ("settings.json".to_owned(), text)));
        let path = match params.path {
            Some(path) => chosen_path(session, &path)?,
            None => default_path(&folder),
        };
        let entries: Vec<String> = files.iter().map(|(name, _)| name.clone()).collect();
        let bundle = Bundle { files, folder, path: path.clone(), reveal: !params.no_reveal };
        if params.wait {
            return bundle.write().map_err(CommandError::new);
        }
        let task = session.state_mut::<tasks::Tasks>().start(&t!("diagnostics-collect-title"), "diagnostics", false);
        let done = path.clone();
        let write = move || {
            let result = bundle.write().map(|_| t!("diagnostics-collect-done", path = done.display().to_string()));
            task.finish(result);
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(write);
        #[cfg(target_arch = "wasm32")]
        write();
        Ok(Collected { path, entries, bytes: None })
    }
}

/// Debug Log Settings: one category per line, `:trace` after it for everything it writes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Categories {
    /// The categories, each a Rust module path such as `zen_lsp::client`.
    pub categories: Vec<String>,
}

pub struct GetCategories;

impl Command for GetCategories {
    type Params = NoParams;
    type Output = Categories;
    const ID: &'static str = "diagnostics.logCategories.get";
    const TITLE: &'static str = "Debug Log Settings";
    const DESCRIPTION: &'static str =
        "The categories the IDE's log writes at debug level, `:trace` after those it writes in full.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Categories, CommandError> {
        Ok(Categories { categories: ide_log::read_categories(state(session)?.data()?) })
    }
}

pub struct SetCategories;

impl Command for SetCategories {
    type Params = Categories;
    type Output = Categories;
    const ID: &'static str = "diagnostics.logCategories.set";
    const TITLE: &'static str = "Set Debug Log Settings";
    const DESCRIPTION: &'static str = "Replaces the categories of the IDE's log, one per item, with an optional `:trace` \
         suffix; kept per IDE and applied at once. Empty lines are dropped. Answers what is kept. Refused for an agent \
         (agentNotAllowed): a library's trace lines can hold secrets.";

    fn run(&self, session: &mut Session, params: Categories) -> Result<Categories, CommandError> {
        if zen_core::macros::by_agent(session) {
            return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("diagnostics-agent-categories")));
        }
        let lines: Vec<String> = params
            .categories
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        let data = state(session)?.data()?;
        ide_log::write_categories(data, &lines).map_err(|error| CommandError::new(error.to_string()))?;
        Ok(Categories { categories: lines })
    }
}

/// A folder or file Special Files and Folders lists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SpecialFolder {
    /// What it is, in the interface's language.
    pub name: String,
    /// Where it is.
    pub path: PathBuf,
}

pub struct ListFolders;

impl Command for ListFolders {
    type Params = NoParams;
    type Output = Vec<SpecialFolder>;
    const ID: &'static str = "diagnostics.folders.list";
    const TITLE: &'static str = "Special Files and Folders";
    const DESCRIPTION: &'static str = "Where this IDE keeps its settings, data, plug-ins, caches and logs.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<SpecialFolder>, CommandError> {
        let diagnostics = state(session)?;
        let data = diagnostics.data()?;
        let folder = |name: String, path: PathBuf| SpecialFolder { name, path };
        Ok(vec![
            folder(t!("diagnostics-folder-settings"), data.join("settings.json")),
            folder(t!("diagnostics-folder-data"), data.to_owned()),
            folder(t!("diagnostics-folder-plugins"), data.join("plugins")),
            folder(t!("diagnostics-folder-caches"), data.join("caches")),
            folder(t!("diagnostics-folder-logs"), diagnostics.logs()?),
        ])
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ShowLog)?;
    registry.register(CollectLogs)?;
    registry.register(GetCategories)?;
    registry.register(SetCategories)?;
    registry.register(ListFolders)?;
    Ok(())
}

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_run::{
    CommandLine, Configuration, ConsoleLine, RunOutput, RunState, SharedOutput, TestCounts, TestNode, TestStatus,
};

use crate::Workbench;
use crate::run::{RunControl, Runs};

pub const HISTORY_LIMIT: usize = 20;

pub const CONSOLE_LIMIT: usize = 20_000;

const INDEX: &str = "index.json";

/// A test run kept in the history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    /// The entry's id, for run.history.open.
    pub id: String,
    /// The configuration's name.
    pub name: String,
    /// When the run ended, in milliseconds since 1970.
    pub finished_at: u64,
    pub status: TestStatus,
    pub counts: TestCounts,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stored {
    entry: HistoryEntry,
    configuration: Configuration,
    provider: String,
    command_lines: Vec<CommandLine>,
    state: RunState,
    exit_code: Option<i32>,
    tree: TestNode,
    console: Vec<ConsoleLine>,
}

type Unsaved = Arc<Mutex<HashMap<String, Arc<Stored>>>>;

#[derive(Default)]
pub struct TestHistory {
    folder: Option<PathBuf>,
    loaded: Option<(PathBuf, Vec<HistoryEntry>)>,
    recorded: HashSet<u64>,
    restored: HashSet<u64>,
    unsaved: Unsaved,
    writer: Option<JoinHandle<()>>,
}

impl TestHistory {
    pub fn set_data_dir(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
        self.loaded = None;
    }

    pub fn flush(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }

    fn project_folder(&self, root: &Path) -> Option<PathBuf> {
        Some(self.folder.as_ref()?.join(crate::database::project_key(root)))
    }

    fn entries(&mut self, root: &Path) -> &mut Vec<HistoryEntry> {
        let fresh = self.loaded.as_ref().is_none_or(|(loaded, _)| loaded != root);
        if fresh {
            let read = self
                .project_folder(root)
                .and_then(|folder| std::fs::read(folder.join(INDEX)).ok())
                .and_then(|bytes| serde_json::from_slice::<Vec<HistoryEntry>>(&bytes).ok())
                .unwrap_or_default();
            self.loaded = Some((root.to_owned(), read));
        }
        &mut self.loaded.get_or_insert_with(|| (root.to_owned(), Vec::new())).1
    }
}

struct Restored;

impl RunControl for Restored {
    fn stop(&self) -> bool {
        false
    }

    fn kill(&self) {}
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
}

fn project_root(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned)
}

fn snapshot(run: &crate::run::Run, id: String) -> Option<Stored> {
    run.output.read(|output| {
        if output.is_running() || output.tree().is_empty() {
            return None;
        }
        let tree = output.tree().root().clone();
        let console = output.console().lines().iter().take(CONSOLE_LIMIT).cloned().collect();
        let entry = HistoryEntry {
            id,
            name: run.configuration.name.clone(),
            finished_at: now_ms(),
            status: tree.status,
            counts: output.tree().counts(),
        };
        Some(Stored {
            entry,
            configuration: run.configuration.clone(),
            provider: run.provider.clone(),
            command_lines: run.command_lines.clone(),
            state: output.state(),
            exit_code: output.exit_code(),
            tree,
            console,
        })
    })
}

fn write(folder: &Path, stored: &[Arc<Stored>], index: &[HistoryEntry]) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    for stored in stored {
        let bytes = serde_json::to_vec(stored.as_ref()).map_err(std::io::Error::other)?;
        zen_vfs::write_atomic(&folder.join(format!("{}.json", stored.entry.id)), &bytes)
            .map_err(std::io::Error::other)?;
    }
    let bytes = serde_json::to_vec_pretty(index).map_err(std::io::Error::other)?;
    zen_vfs::write_atomic(&folder.join(INDEX), &bytes).map_err(std::io::Error::other)?;
    let kept: HashSet<String> = index.iter().map(|entry| format!("{}.json", entry.id)).collect();
    for file in std::fs::read_dir(folder)?.flatten() {
        let name = file.file_name().to_string_lossy().into_owned();
        if name != INDEX && name.ends_with(".json") && !kept.contains(&name) {
            let _ = std::fs::remove_file(file.path());
        }
    }
    Ok(())
}

pub fn settle(session: &mut Session) -> bool {
    let Some(root) = project_root(session) else { return false };
    let history = session.state_mut::<TestHistory>();
    if history.folder.is_none() {
        return false;
    }
    let seen: Vec<u64> = history.recorded.iter().chain(&history.restored).copied().collect();
    let Some(runs) = session.state::<Runs>() else { return false };
    let mut fresh = Vec::new();
    let mut ended = Vec::new();
    for run in runs.runs().iter().filter(|run| !seen.contains(&run.id)) {
        if run.output.read(RunOutput::is_running) {
            continue;
        }
        ended.push(run.id);
        let id = format!("{}-{}", now_ms(), run.id);
        if let Some(stored) = snapshot(run, id) {
            fresh.push(Arc::new(stored));
        }
    }
    let history = session.state_mut::<TestHistory>();
    history.recorded.extend(ended);
    if fresh.is_empty() {
        return false;
    }
    let Some(folder) = history.project_folder(&root) else { return false };
    let entries = history.entries(&root);
    for stored in &fresh {
        entries.insert(0, stored.entry.clone());
    }
    entries.truncate(HISTORY_LIMIT);
    let index = entries.clone();
    let unsaved = Arc::clone(&history.unsaved);
    if let Ok(mut pending) = unsaved.lock() {
        for stored in &fresh {
            pending.insert(stored.entry.id.clone(), Arc::clone(stored));
        }
    }
    let previous = history.writer.take();
    history.writer = Some(std::thread::spawn(move || {
        if let Some(previous) = previous {
            let _ = previous.join();
        }
        let _ = write(&folder, &fresh, &index);
        if let Ok(mut pending) = unsaved.lock() {
            for stored in &fresh {
                pending.remove(&stored.entry.id);
            }
        }
    }));
    true
}

/// The test runs kept for the project, newest first.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HistoryList {
    pub entries: Vec<HistoryEntry>,
}

pub struct ListHistory;

impl Command for ListHistory {
    type Params = NoParams;
    type Output = HistoryList;
    const ID: &'static str = "run.history.list";
    const TITLE: &'static str = "Test History";
    const DESCRIPTION: &'static str = "The project's earlier test runs, newest first, as Run → Test History and the Run tool \
         window's Test History button list them: every run that ended with a test tree, the last 20 kept on disk per project \
         with their tree and console. run.history.open reopens one.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<HistoryList, CommandError> {
        settle(session);
        let Some(root) = project_root(session) else { return Ok(HistoryList { entries: Vec::new() }) };
        Ok(HistoryList { entries: session.state_mut::<TestHistory>().entries(&root).clone() })
    }
}

/// The history entry to reopen.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OpenParams {
    /// The entry's id from run.history.list.
    pub id: String,
}

/// The run the entry is reopened as.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reopened {
    /// The new run's id, for run.output.get and run.tests.get; Rerun starts its configuration again.
    pub run_id: u64,
}

fn load(session: &mut Session, id: &str) -> Result<Arc<Stored>, CommandError> {
    let unknown = || CommandError::new(t!("workbench-test-history-unknown", id = id));
    let root = project_root(session).ok_or_else(unknown)?;
    let history = session.state_mut::<TestHistory>();
    if !history.entries(&root).iter().any(|entry| entry.id == id) {
        return Err(unknown());
    }
    if let Some(stored) = history.unsaved.lock().ok().and_then(|pending| pending.get(id).cloned()) {
        return Ok(stored);
    }
    let file = history.project_folder(&root).ok_or_else(unknown)?.join(format!("{id}.json"));
    std::fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Stored>(&bytes).ok())
        .map(Arc::new)
        .ok_or_else(unknown)
}

pub struct OpenHistory;

impl Command for OpenHistory {
    type Params = OpenParams;
    type Output = Reopened;
    const ID: &'static str = "run.history.open";
    const TITLE: &'static str = "Open Test History Entry";
    const DESCRIPTION: &'static str = "Reopens an earlier test run from run.history.list in a new tab of the Run tool window, \
         with its test tree and console as they were; Rerun on it starts its configuration again.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<Reopened, CommandError> {
        let stored = load(session, &params.id)?;
        let stored = Arc::unwrap_or_clone(stored);
        let output = RunOutput::restored(stored.console, stored.tree, stored.state, stored.exit_code);
        let runs = session.state_mut::<Runs>();
        let output = SharedOutput::new(output, runs.waker().cloned());
        let configuration = Configuration { id: String::new(), ..stored.configuration };
        let run_id = runs.insert_task(configuration, stored.provider, stored.command_lines, output, Arc::new(Restored));
        session.state_mut::<TestHistory>().restored.insert(run_id);
        Ok(Reopened { run_id })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListHistory)?;
    registry.register(OpenHistory)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zen_run::Stream;
    use zen_run::fake::FakeProvider;

    use super::*;

    fn fixture() -> (Registry, Session, tempfile::TempDir, tempfile::TempDir) {
        zen_i18n::register(crate::LOCALES);
        let project = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        crate::trust::trust_session_in_tests(&mut session, project.path());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        session.state_mut::<TestHistory>().set_data_dir(data.path().to_owned());
        (registry, session, project, data)
    }

    fn run_tests(registry: &Registry, session: &mut Session, lines: &[&str]) -> u64 {
        let fake = FakeProvider::new();
        let script: Vec<(Stream, &str)> = lines.iter().map(|line| (Stream::Stdout, *line)).collect();
        fake.set_script(&script, 1);
        session.state_mut::<Runs>().set_providers(vec![Arc::new(fake)], zen_run::RunEnv::default());
        let configuration = serde_json::to_value(FakeProvider::configuration("shop tests")).unwrap();
        let started = registry
            .dispatch(session, "run.process.start", json!({ "configuration": configuration, "waitMs": 10_000 }))
            .unwrap();
        started["runId"].as_u64().unwrap()
    }

    #[test]
    fn a_finished_test_run_is_kept_on_disk_per_project_and_reopens_with_its_tree_and_console() {
        let (registry, mut session, _project, data) = fixture();
        let run = run_tests(&registry, &mut session, &["@fail app TestA 9", "@fail app - 9"]);
        let list = registry.dispatch(&mut session, "run.history.list", json!({})).unwrap();
        let entries = list["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1, "{list}");
        assert_eq!((entries[0]["name"].as_str(), entries[0]["status"].as_str()), (Some("shop tests"), Some("failed")));
        session.state_mut::<TestHistory>().flush();
        let mut again = Session::new();
        registry.dispatch(&mut again, "file.project.open", json!({ "path": _project.path() })).unwrap();
        again.state_mut::<TestHistory>().set_data_dir(data.path().to_owned());
        let id = entries[0]["id"].as_str().unwrap().to_owned();
        assert_eq!(registry.dispatch(&mut again, "run.history.list", json!({})).unwrap()["entries"][0]["id"], id);
        let reopened = registry.dispatch(&mut again, "run.history.open", json!({ "id": id })).unwrap();
        let run_id = reopened["runId"].as_u64().unwrap();
        let tests = registry.dispatch(&mut again, "run.tests.get", json!({ "runId": run_id })).unwrap();
        let original = registry.dispatch(&mut session, "run.tests.get", json!({ "runId": run })).unwrap();
        assert_eq!(tests["tree"], original["tree"]);
        assert_eq!(tests["done"], true);
        let output = registry.dispatch(&mut again, "run.output.get", json!({ "runId": run_id, "from": 0 })).unwrap();
        let first = registry.dispatch(&mut session, "run.output.get", json!({ "runId": run, "from": 0 })).unwrap();
        assert_eq!(output["lines"], first["lines"]);
        assert_eq!(
            registry.dispatch(&mut again, "run.history.list", json!({})).unwrap()["entries"].as_array().unwrap().len(),
            1,
            "a reopened run is not kept again"
        );
        assert!(registry.dispatch(&mut again, "run.history.open", json!({ "id": "nope" })).is_err());
    }

    #[test]
    fn the_history_keeps_the_last_twenty_test_runs_and_no_runs_without_tests() {
        let (registry, mut session, _project, data) = fixture();
        run_tests(&registry, &mut session, &["just output"]);
        assert_eq!(registry.dispatch(&mut session, "run.history.list", json!({})).unwrap()["entries"], json!([]));
        for _ in 0..HISTORY_LIMIT + 2 {
            run_tests(&registry, &mut session, &["@pass app TestA 1"]);
            settle(&mut session);
        }
        let entries = registry.dispatch(&mut session, "run.history.list", json!({})).unwrap()["entries"].clone();
        assert_eq!(entries.as_array().unwrap().len(), HISTORY_LIMIT);
        session.state_mut::<TestHistory>().flush();
        let folder = std::fs::read_dir(data.path()).unwrap().next().unwrap().unwrap().path();
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), HISTORY_LIMIT + 1, "the entries and the index");
    }
}

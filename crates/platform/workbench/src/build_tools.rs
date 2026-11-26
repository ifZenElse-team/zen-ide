use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};

use schemars::JsonSchema;
use serde::Serialize;
use serde::de::DeserializeOwned;

pub mod found;

pub use found::{Executable, ToolSource};

pub type JdkLookup = Arc<dyn Fn(&Path) -> Option<PathBuf> + Send + Sync>;

pub type JvmReader = Arc<dyn Fn(&Path, &str) -> Option<zen_syntax::JvmFile> + Send + Sync>;

pub type ReimportHook = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub struct BuildEnv {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct Inner {
    jdk: RwLock<Option<JdkLookup>>,
    reimport: RwLock<Option<ReimportHook>>,
    jvm: RwLock<Option<JvmReader>>,
    folders: RwLock<Option<Vec<PathBuf>>>,
    data: RwLock<Option<PathBuf>>,
    home: RwLock<Option<PathBuf>>,
    loads: Mutex<HashMap<String, Load>>,
    kept: Mutex<serde_json::Map<String, serde_json::Value>>,
    gradle_models: Mutex<HashMap<PathBuf, Arc<crate::gradle::build_model::Kept>>>,
    read: Mutex<Option<ReadSettings>>,
}

struct ReadSettings {
    file: PathBuf,
    changed: Option<std::time::SystemTime>,
    settings: serde_json::Map<String, serde_json::Value>,
}

pub const SETTINGS_FILE: &str = "build-tools.json";

impl std::fmt::Debug for BuildEnv {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildEnv").finish_non_exhaustive()
    }
}

/// Where a piece of background work is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum LoadState {
    /// Running.
    Loading,
    /// Ended well; its result is there.
    Loaded,
    /// Ended badly; its error is there.
    Failed,
}

#[derive(Clone, Debug)]
pub struct Load {
    pub state: LoadState,
    pub output: String,
    pub error: Option<String>,
    pub task: Option<u64>,
    pub started: std::time::Instant,
    pub took: Option<std::time::Duration>,
}

fn read<T: Clone>(lock: &RwLock<Option<T>>) -> Option<T> {
    lock.read().unwrap_or_else(PoisonError::into_inner).clone()
}

fn write<T>(lock: &RwLock<Option<T>>, value: Option<T>) {
    *lock.write().unwrap_or_else(PoisonError::into_inner) = value;
}

impl BuildEnv {
    pub fn global() -> Self {
        static GLOBAL: std::sync::OnceLock<BuildEnv> = std::sync::OnceLock::new();
        GLOBAL.get_or_init(BuildEnv::default).clone()
    }

    pub fn set_jdk(&self, lookup: Option<JdkLookup>) {
        write(&self.inner.jdk, lookup);
    }

    pub fn set_reimport(&self, hook: Option<ReimportHook>) {
        write(&self.inner.reimport, hook);
    }

    pub fn set_jvm_reader(&self, reader: Option<JvmReader>) {
        write(&self.inner.jvm, reader);
    }

    pub fn jvm_file(&self, path: &Path, text: &str) -> Option<zen_syntax::JvmFile> {
        read(&self.inner.jvm).and_then(|reader| reader(path, text))
    }

    pub fn set_folders(&self, folders: Option<Vec<PathBuf>>) {
        write(&self.inner.folders, folders);
    }

    pub fn set_data_dir(&self, data: Option<PathBuf>) {
        write(&self.inner.data, data);
    }

    pub fn set_home(&self, home: Option<PathBuf>) {
        write(&self.inner.home, home);
    }

    pub fn home(&self) -> PathBuf {
        read(&self.inner.home).unwrap_or_else(|| std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default())
    }

    pub fn user_home(&self) -> Option<PathBuf> {
        if read(&self.inner.folders).is_some() && read(&self.inner.home).is_none() {
            return None;
        }
        Some(self.home())
    }

    pub fn data_dir(&self) -> Option<PathBuf> {
        read(&self.inner.data)
    }

    pub fn reimport(&self, root: &Path) -> bool {
        read(&self.inner.reimport).is_some_and(|hook| hook(root))
    }

    pub fn java_home(&self, root: &Path) -> Option<PathBuf> {
        read(&self.inner.jdk)
            .and_then(|lookup| lookup(root))
            .or_else(|| crate::jdk::project_home(root))
            .or_else(|| std::env::var_os("JAVA_HOME").filter(|home| !home.is_empty()).map(PathBuf::from))
            .filter(|home| home.is_dir())
    }

    pub fn folders(&self, candidate: &str) -> Vec<PathBuf> {
        if let Some(folders) = read(&self.inner.folders) {
            return folders;
        }
        let mut folders = zen_lsp::servers::search_folders();
        if let Some(home) = std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from) {
            folders.push(home.join(".sdkman/candidates").join(candidate).join("current/bin"));
        }
        if let Some(data) = self.data_dir() {
            folders.extend(downloaded(&data, candidate));
        }
        folders
    }

    fn current_settings(&self) -> serde_json::Map<String, serde_json::Value> {
        let Some(data) = self.data_dir() else {
            return self.inner.kept.lock().unwrap_or_else(PoisonError::into_inner).clone();
        };
        let file = data.join(SETTINGS_FILE);
        let changed = std::fs::metadata(&file).and_then(|meta| meta.modified()).ok();
        let mut read = self.inner.read.lock().unwrap_or_else(PoisonError::into_inner);
        match read.as_ref() {
            Some(kept) if kept.file == file && kept.changed == changed && changed.is_some() => kept.settings.clone(),
            _ => {
                let settings = read_settings(&data);
                *read = Some(ReadSettings { file, changed, settings: settings.clone() });
                settings
            }
        }
    }

    pub fn settings<T: DeserializeOwned + Default>(&self, tool: &str) -> T {
        self.current_settings()
            .get(tool)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }

    pub fn set_settings<T: Serialize>(&self, tool: &str, value: &T) -> Result<(), String> {
        let value = serde_json::to_value(value).map_err(|error| error.to_string())?;
        self.change_settings(|all| {
            all.insert(tool.to_owned(), value);
            Ok(())
        })
    }

    pub fn project_settings<T: DeserializeOwned + Default>(&self, tool: &str, root: &Path) -> T {
        let all = self.current_settings();
        all.get(tool)
            .and_then(|projects| projects.get(root.to_string_lossy().as_ref()))
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }

    pub fn set_project_settings<T: Serialize + Default + PartialEq>(
        &self,
        tool: &str,
        root: &Path,
        value: &T,
    ) -> Result<(), String> {
        self.change_settings(|all| {
            let projects = all.entry(tool.to_owned()).or_insert_with(|| serde_json::json!({}));
            if let Some(projects) = projects.as_object_mut() {
                let key = root.to_string_lossy().into_owned();
                if *value == T::default() {
                    projects.remove(&key);
                } else {
                    projects.insert(key, serde_json::to_value(value).map_err(|error| error.to_string())?);
                }
            }
            Ok(())
        })
    }

    fn change_settings(
        &self,
        change: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut kept = self.inner.kept.lock().unwrap_or_else(PoisonError::into_inner);
        let data = self.data_dir();
        let mut all = match &data {
            Some(data) => read_settings(data),
            None => kept.clone(),
        };
        change(&mut all)?;
        *self.inner.read.lock().unwrap_or_else(PoisonError::into_inner) = None;
        match &data {
            Some(data) => {
                std::fs::create_dir_all(data).map_err(|error| error.to_string())?;
                let text = serde_json::to_vec_pretty(&all).map_err(|error| error.to_string())?;
                zen_vfs::write_atomic(&data.join(SETTINGS_FILE), &text).map_err(|error| error.to_string())
            }
            None => {
                *kept = all;
                Ok(())
            }
        }
    }

    pub fn environment(&self, root: &Path, folders: &[PathBuf]) -> Vec<(String, String)> {
        self.environment_with(self.java_home(root), folders)
    }

    pub fn environment_with(&self, java: Option<PathBuf>, folders: &[PathBuf]) -> Vec<(String, String)> {
        let mut path: Vec<PathBuf> = java.iter().map(|home| home.join("bin")).collect();
        let inherited: Vec<PathBuf> =
            std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect()).unwrap_or_default();
        let system = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"].map(PathBuf::from);
        for folder in folders.iter().cloned().chain(inherited).chain(system) {
            if !path.contains(&folder) {
                path.push(folder);
            }
        }
        let mut env = Vec::new();
        if let Ok(joined) = std::env::join_paths(&path) {
            env.push(("PATH".to_owned(), joined.to_string_lossy().into_owned()));
        }
        if let Some(home) = java {
            env.push(("JAVA_HOME".to_owned(), home.to_string_lossy().into_owned()));
        }
        env
    }

    pub fn gradle_model(&self, folder: &Path) -> Option<Arc<crate::gradle::build_model::Kept>> {
        self.inner.gradle_models.lock().unwrap_or_else(PoisonError::into_inner).get(folder).cloned()
    }

    pub fn set_gradle_model(&self, folder: &Path, model: Arc<crate::gradle::build_model::Kept>) {
        self.inner.gradle_models.lock().unwrap_or_else(PoisonError::into_inner).insert(folder.to_owned(), model);
    }

    pub fn load(&self, key: &str) -> Option<Load> {
        self.inner.loads.lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
    }

    pub fn forget(&self, key: &str) {
        self.inner.loads.lock().unwrap_or_else(PoisonError::into_inner).remove(key);
    }

    pub fn set_load(&self, key: &str, load: Load) {
        self.inner.loads.lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_owned(), load);
    }

    pub fn spawn(
        &self,
        key: &str,
        line: zen_run::CommandLine,
        task: Option<crate::tasks::TaskHandle>,
        result: impl FnOnce(&str) -> Result<String, String> + Send + 'static,
    ) -> bool {
        if !crate::trust::is_trusted(self, &line.cwd) {
            let message = zen_i18n::t!("workbench-trust-build-load", project = crate::display(&line.cwd));
            self.set_load(
                key,
                Load {
                    state: LoadState::Failed,
                    output: String::new(),
                    error: Some(message.clone()),
                    task: task.as_ref().map(crate::tasks::TaskHandle::id),
                    started: std::time::Instant::now(),
                    took: None,
                },
            );
            if let Some(task) = task {
                task.finish(Err(message));
            }
            return false;
        }
        {
            let mut loads = self.inner.loads.lock().unwrap_or_else(PoisonError::into_inner);
            if loads.get(key).is_some_and(|load| load.state == LoadState::Loading) {
                return false;
            }
            let id = task.as_ref().map(crate::tasks::TaskHandle::id);
            loads.insert(
                key.to_owned(),
                Load {
                    state: LoadState::Loading,
                    output: String::new(),
                    error: None,
                    task: id,
                    started: std::time::Instant::now(),
                    took: None,
                },
            );
        }
        let env = self.clone();
        let key = key.to_owned();
        let id = task.as_ref().map(crate::tasks::TaskHandle::id);
        std::thread::spawn(move || {
            let cancel = Arc::new(AtomicBool::new(false));
            let programs = zen_run::Programs::default();
            if let Some(task) = &task {
                task.cancel_with(cancel.clone());
                task.cancel_kills(Arc::new(programs.clone()));
            }
            let started = env.load(&key).map_or_else(std::time::Instant::now, |load| load.started);
            let ran = run_captured(&line, task.as_ref(), &cancel, &programs)
                .and_then(|output| result(&output).map_err(|error| Failure { message: error, output: String::new() }));
            let took = Some(started.elapsed());
            let load = match &ran {
                Ok(output) => {
                    Load { state: LoadState::Loaded, output: output.clone(), error: None, task: id, started, took }
                }
                Err(failure) => Load {
                    state: LoadState::Failed,
                    output: failure.output.clone(),
                    error: Some(failure.message.clone()),
                    task: id,
                    started,
                    took,
                },
            };
            env.set_load(&key, load);
            if let Some(task) = task {
                task.finish(ran.map(|_| String::new()).map_err(|failure| failure.message));
            }
        });
        true
    }
}

fn read_settings(data: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read(data.join(SETTINGS_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn downloaded(data: &Path, candidate: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(data.join("tools").join(candidate))
        .map(|entries| entries.flatten().map(|entry| entry.path().join("bin")).filter(|bin| bin.is_dir()).collect())
        .unwrap_or_default();
    found.sort();
    found.reverse();
    found
}

const KEPT_ON_FAILURE: usize = 400;

struct Failure {
    message: String,
    output: String,
}

pub fn capture(
    line: &zen_run::CommandLine,
    task: Option<&crate::tasks::TaskHandle>,
    cancel: &AtomicBool,
) -> Result<String, String> {
    run_captured(line, task, cancel, &zen_run::Programs::default()).map_err(|failure| failure.message)
}

fn run_captured(
    line: &zen_run::CommandLine,
    task: Option<&crate::tasks::TaskHandle>,
    cancel: &AtomicBool,
    programs: &zen_run::Programs,
) -> Result<String, Failure> {
    let failed = |message: String| Failure { message, output: String::new() };
    if let Some(task) = task {
        task.log(&format!("$ {}", line.display));
    }
    let mut command = Process::new(&line.program);
    command
        .args(&line.args)
        .current_dir(&line.cwd)
        .envs(line.env.iter().map(|(name, value)| (name.as_str(), value.as_str())))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child =
        programs.spawn(&mut command).map_err(|error| failed(format!("{}: {error}", line.program.display())))?;
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut lines = Vec::new();
        if let Some(stderr) = stderr {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                lines.push(line);
            }
        }
        lines
    });
    let mut output = String::new();
    let mut tail: Vec<String> = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        for text in BufReader::new(stdout).lines().map_while(Result::ok) {
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                break;
            }
            if let Some(task) = task {
                task.log(&text);
            }
            tail.push(text.clone());
            if tail.len() > KEPT_ON_FAILURE {
                tail.remove(0);
            }
            output.push_str(&text);
            output.push('\n');
        }
    }
    let status = child.wait().map_err(|error| failed(error.to_string()))?;
    let errors = errors.join().unwrap_or_default();
    if let Some(task) = task {
        for text in &errors {
            task.log(text);
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(failed(zen_i18n::t!("workbench-build-cancelled")));
    }
    if !status.success() {
        let all: Vec<String> = tail.into_iter().chain(errors).filter(|line| !line.trim().is_empty()).collect();
        let said = &all[all.len().saturating_sub(12)..];
        let kept = &all[all.len().saturating_sub(KEPT_ON_FAILURE)..];
        return Err(Failure {
            message: zen_i18n::t!(
                "workbench-build-failed",
                program = line.display.as_str(),
                code = status.code().map_or_else(|| "?".to_owned(), |code| code.to_string()),
                output = said.join("\n")
            ),
            output: kept.join("\n"),
        });
    }
    Ok(output)
}

pub fn wrapper(folder: &Path, root: &Path, name: &str) -> Option<PathBuf> {
    let file = if cfg!(windows) { format!("{name}.cmd") } else { name.to_owned() };
    let mut at = Some(folder);
    while let Some(dir) = at {
        let candidate = dir.join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir == root {
            break;
        }
        at = dir.parent().filter(|parent| parent.starts_with(root));
    }
    None
}

pub fn foreign_list(options: &std::collections::BTreeMap<String, String>, name: &str) -> Vec<String> {
    let marker = format!("{name}.list.option");
    let mut found: Vec<(usize, String)> = options
        .iter()
        .filter_map(|(key, value)| {
            let at = key.find(&marker)?;
            let at_name = at == 0 || key[..at].ends_with('.');
            let rest = &key[at + marker.len()..];
            let index = match rest.strip_prefix('.') {
                Some(index) => index.parse().ok()?,
                None if rest.is_empty() => 0,
                None => return None,
            };
            at_name.then(|| (index, value.clone()))
        })
        .collect();
    found.sort_by_key(|(index, _)| *index);
    found.into_iter().map(|(_, value)| value).filter(|value| !value.is_empty()).collect()
}

pub fn foreign_option(options: &std::collections::BTreeMap<String, String>, name: &str) -> String {
    options
        .iter()
        .find(|(key, _)| *key == name || key.ends_with(&format!(".{name}")))
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default()
}

const HISTORY_LIMIT: usize = 20;

pub fn remember_line(history: &mut Vec<String>, line: &str) {
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return;
    }
    history.retain(|known| *known != line);
    history.insert(0, line);
    history.truncate(HISTORY_LIMIT);
}

/// What a suggestion of a build tool's Execute popup is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionKind {
    /// A whole command line run before.
    History,
    /// A lifecycle phase.
    Phase,
    /// A plug-in goal, `prefix:goal`.
    Goal,
    /// A Gradle task.
    Task,
    /// A command line option.
    Option,
}

/// A suggestion of a build tool's Execute popup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    /// The whole command line once it is taken.
    pub text: String,
    /// What it is.
    pub kind: SuggestionKind,
}

/// What a build tool's completion answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Suggestions {
    /// History first (latest first), then what completes the last word.
    pub suggestions: Vec<Suggestion>,
}

pub fn complete_line<'a>(
    text: &str,
    history: &[String],
    names: impl IntoIterator<Item = (&'a str, SuggestionKind)>,
) -> Vec<Suggestion> {
    let (head, word) = match text.rfind(char::is_whitespace) {
        Some(at) => (&text[..=at], &text[at + 1..]),
        None => ("", text),
    };
    let typed = text.trim_start();
    let mut found: Vec<Suggestion> = history
        .iter()
        .filter(|line| line.starts_with(typed))
        .map(|line| Suggestion { text: line.clone(), kind: SuggestionKind::History })
        .collect();
    for (name, kind) in names {
        if name.starts_with(word) && name != word {
            let text = format!("{head}{name}");
            if !found.iter().any(|known| known.text == text) {
                found.push(Suggestion { text, kind });
            }
        }
    }
    found
}

pub fn relative_to(root: &Path, path: &str) -> String {
    match Path::new(path).strip_prefix(root) {
        Ok(inside) => inside.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"),
        Err(_) => path.to_owned(),
    }
}

#[cfg(test)]
pub(crate) fn real_tool_in_tests(variable: &str) -> PathBuf {
    std::env::var_os(variable).map(PathBuf::from).unwrap_or_else(|| panic!("set {variable}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrapper_is_found_in_the_folder_or_above_it_within_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let module = root.join("core/api");
        std::fs::create_dir_all(&module).unwrap();
        assert_eq!(wrapper(&module, root, "mvnw"), None);
        std::fs::write(root.join("mvnw"), "").unwrap();
        assert_eq!(wrapper(&module, root, "mvnw"), Some(root.join("mvnw")));
        std::fs::write(module.join("mvnw"), "").unwrap();
        assert_eq!(wrapper(&module, root, "mvnw"), Some(module.join("mvnw")));
        assert_eq!(wrapper(root, &root.join("core"), "gradlew"), None, "never above the project");
    }

    #[test]
    fn the_jdk_is_the_toolchains_else_java_home_and_comes_first_on_path() {
        let dir = tempfile::tempdir().unwrap();
        let jdk = dir.path().join("jdk");
        std::fs::create_dir_all(jdk.join("bin")).unwrap();
        let env = BuildEnv::default();
        let found = jdk.clone();
        env.set_jdk(Some(Arc::new(move |_root: &Path| Some(found.clone()))));
        assert_eq!(env.java_home(dir.path()), Some(jdk.clone()));
        let vars = env.environment(dir.path(), &[PathBuf::from("/usr/bin")]);
        let path = &vars.iter().find(|(name, _)| name == "PATH").unwrap().1;
        assert!(path.starts_with(&*jdk.join("bin").to_string_lossy()), "{path}");
        assert!(vars.iter().any(|(name, value)| name == "JAVA_HOME" && *value == jdk.to_string_lossy()));
        env.set_jdk(Some(Arc::new(|_root: &Path| Some(PathBuf::from("/nowhere/jdk")))));
        assert!(
            env.java_home(dir.path()).is_none_or(|home| home != Path::new("/nowhere/jdk")),
            "a missing JDK is not used"
        );
    }

    #[test]
    fn list_options_of_another_ides_configuration_are_read_in_order() {
        let options: std::collections::BTreeMap<String, String> = [
            ("Settings.goals.list.option.10", "k"),
            ("Settings.goals.list.option.2", "b"),
            ("Settings.goals.list.option.0", "a"),
            ("Settings.subgoals.list.option.0", "no"),
            ("Settings.workingDirPath", "/p/core"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
        assert_eq!(foreign_list(&options, "goals"), ["a", "b", "k"]);
        assert_eq!(foreign_option(&options, "workingDirPath"), "/p/core");
        assert_eq!(relative_to(Path::new("/p"), "/p/core/x"), "core/x");
        assert_eq!(relative_to(Path::new("/p"), "/q"), "/q");
    }

    #[test]
    fn reimport_asks_the_hook_once_it_is_set() {
        let env = BuildEnv::default();
        assert!(!env.reimport(Path::new("/p")), "no language server yet");
        let asked = Arc::new(Mutex::new(Vec::new()));
        let seen = asked.clone();
        env.set_reimport(Some(Arc::new(move |root: &Path| {
            seen.lock().unwrap().push(root.to_owned());
            true
        })));
        assert!(env.clone().reimport(Path::new("/p")), "clones share the hook");
        assert_eq!(*asked.lock().unwrap(), [PathBuf::from("/p")]);
    }

    #[cfg(unix)]
    #[test]
    fn background_work_keeps_its_result_or_its_error() {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let env = BuildEnv::default();
        crate::trust::trust_in_tests(&env, dir.path());
        let line = |script: &str| zen_run::CommandLine {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_owned(), script.to_owned()],
            cwd: dir.path().to_owned(),
            env: Vec::new(),
            display: script.to_owned(),
        };
        assert!(env.spawn("ok", line("echo one; echo two"), None, |out| Ok(out.to_uppercase())));
        assert!(env.spawn("bad", line("echo nope; exit 3"), None, |out| Ok(out.to_owned())));
        let wait = |key: &str| {
            for _ in 0..500 {
                if env.load(key).is_some_and(|load| load.state != LoadState::Loading) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            env.load(key).unwrap()
        };
        let ok = wait("ok");
        assert_eq!((ok.state, ok.output.as_str()), (LoadState::Loaded, "ONE\nTWO\n"));
        let bad = wait("bad");
        assert_eq!(bad.state, LoadState::Failed);
        let error = bad.error.unwrap();
        assert!(error.contains("nope") && error.contains('3'), "{error}");
        assert_eq!(bad.output, "nope", "the output stays for the Sync view");
        assert!(bad.took.is_some() && ok.took.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn nothing_loads_a_build_in_a_folder_not_trusted() {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran");
        let env = BuildEnv::default();
        let line = zen_run::CommandLine {
            program: "sh".into(),
            args: vec!["-c".to_owned(), format!("touch '{}'", marker.display())],
            cwd: dir.path().to_owned(),
            ..zen_run::CommandLine::default()
        };
        assert!(!env.spawn("load", line.clone(), None, |_| Ok(String::new())), "refused");
        assert_eq!(env.load("load").map(|load| load.state), Some(LoadState::Failed));
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!marker.exists(), "the program never started");
        crate::trust::trust_in_tests(&env, dir.path());
        assert!(env.spawn("load", line, None, |_| Ok(String::new())));
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_a_build_tool_kills_it_even_when_it_prints_nothing() {
        use zen_project::inspect::Programs as _;
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut tasks = crate::tasks::Tasks::default();
        let task = tasks.start("Reloading", "maven", false);
        let (cancel, programs) = (Arc::new(AtomicBool::new(false)), zen_run::Programs::default());
        task.cancel_with(cancel.clone());
        task.cancel_kills(Arc::new(programs.clone()));
        let line = zen_run::CommandLine {
            program: "sh".into(),
            args: vec!["-c".to_owned(), "sleep 60; sleep 60".to_owned()],
            cwd: std::env::temp_dir(),
            ..zen_run::CommandLine::default()
        };
        let running = programs.clone();
        let worker = task.clone();
        let build = std::thread::spawn(move || {
            run_captured(&line, Some(&worker), &cancel, &running).map_err(|failure| failure.message)
        });
        while !programs.running() {
            std::thread::yield_now();
        }
        tasks.cancel(None).unwrap();
        assert_eq!(build.join().unwrap().unwrap_err(), "Cancelled.");
    }
}

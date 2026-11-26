pub mod adoptium;
pub mod commands;
pub mod detect;
#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use commands::{FORM, PAGE, register};
pub use detect::{Jdk, JdkSource, Sources};

use crate::jvm_modules::BuildLevel;

pub const FILE: &str = "jdks.json";

pub const FOLDER: &str = "jdks";

const LEVEL_LOOK_AGAIN: Duration = Duration::from_secs(2);

/// What a project uses: its JDK and language level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProjectJdk {
    /// The JDK.
    pub jdk: Jdk,
    /// Whether the project chose it; otherwise it is the installed JDK of the build's level, else the
    /// default (`JAVA_HOME`'s, else the newest).
    pub chosen: bool,
    /// The language level, a feature release such as `17`: the project's, else the build's when the
    /// JDK can compile it, else the JDK's.
    pub language_level: u32,
    /// Whether the project set no language level of its own ("SDK default").
    pub language_level_default: bool,
    /// The feature release the build asks for, when its files say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_level: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProjectChoice {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    home: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language_level: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    #[serde(default)]
    added: Vec<PathBuf>,
    #[serde(default)]
    projects: BTreeMap<String, ProjectChoice>,
}

struct ReadLevel {
    at: Instant,
    stamp: Vec<Option<(SystemTime, u64)>>,
    level: Option<BuildLevel>,
}

#[derive(Default)]
pub struct Store {
    data: Option<PathBuf>,
    kept: Kept,
    found: Option<Vec<Jdk>>,
    sources: Option<Sources>,
    levels: HashMap<PathBuf, ReadLevel>,
    pub(crate) downloading: Option<u32>,
}

fn build_stamp(root: &Path) -> Vec<Option<(SystemTime, u64)>> {
    ["pom.xml", "build.gradle", "build.gradle.kts"]
        .iter()
        .map(|file| {
            let metadata = std::fs::metadata(root.join(file)).ok()?;
            Some((metadata.modified().ok()?, metadata.len()))
        })
        .collect()
}

impl Store {
    pub fn with_sources(sources: Sources) -> Self {
        Self { sources: Some(sources), ..Self::default() }
    }

    pub fn set_data_dir(&mut self, data: PathBuf) {
        self.kept = std::fs::read(data.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        self.data = Some(data);
        self.found = None;
    }

    pub fn download_folder(&self) -> Option<PathBuf> {
        self.data.as_ref().map(|data| data.join(FOLDER))
    }

    pub fn jdks(&mut self) -> Vec<Jdk> {
        if self.found.is_none() {
            let mut sources = match &self.sources {
                Some(sources) => sources.clone(),
                None => Sources::of_machine(self.download_folder(), Vec::new()),
            };
            sources.added.extend(self.kept.added.iter().cloned());
            if sources.downloaded.is_none() {
                sources.downloaded = self.download_folder();
            }
            self.found = Some(detect::detect(&sources));
        }
        self.found.clone().unwrap_or_default()
    }

    pub fn refresh(&mut self) {
        self.found = None;
        self.levels.clear();
    }

    pub fn build_level(&mut self, root: &Path) -> Option<BuildLevel> {
        let stamp = build_stamp(root);
        if let Some(read) = self.levels.get(root)
            && read.stamp == stamp
            && read.at.elapsed() < LEVEL_LOOK_AGAIN
        {
            return read.level.clone();
        }
        let level = crate::jvm_modules::build_level(root);
        self.levels.insert(root.to_owned(), ReadLevel { at: Instant::now(), stamp, level: level.clone() });
        level
    }

    pub fn jdk_of(&mut self, level: u32) -> Option<Jdk> {
        matching_jdk(&self.jdks(), level).cloned()
    }

    pub fn add(&mut self, home: &Path) -> Result<Jdk, String> {
        let jdk = detect::read(home, JdkSource::Added).ok_or_else(|| home.display().to_string())?;
        if !self.kept.added.contains(&jdk.home) {
            self.kept.added.push(jdk.home.clone());
        }
        self.refresh();
        self.save();
        Ok(jdk)
    }

    pub fn remove(&mut self, home: &Path) -> bool {
        let before = self.kept.added.len();
        self.kept.added.retain(|added| added != home);
        let mut removed = before != self.kept.added.len();
        if let Some(folder) = self.download_folder()
            && let Some(top) = home.strip_prefix(&folder).ok().and_then(|rest| rest.components().next())
        {
            removed |= std::fs::remove_dir_all(folder.join(top)).is_ok();
        }
        for choice in self.kept.projects.values_mut() {
            if choice.home.as_deref() == Some(home) {
                choice.home = None;
            }
        }
        self.refresh();
        self.save();
        removed
    }

    pub fn project(&mut self, root: &Path) -> Option<ProjectJdk> {
        let choice = self.kept.projects.get(&key(root)).cloned().unwrap_or_default();
        let jdks = self.jdks();
        let chosen = choice.home.as_ref().and_then(|home| jdks.iter().find(|jdk| &jdk.home == home).cloned());
        let build_level = self.build_level(root).map(|level| level.level);
        let (jdk, is_chosen) = match chosen {
            Some(jdk) => (jdk, true),
            None => {
                let matching = build_level.and_then(|level| matching_jdk(&jdks, level));
                (matching.or_else(|| default_jdk(&jdks))?.clone(), false)
            }
        };
        let language_level_default = choice.language_level.is_none();
        let compiled = build_level.filter(|level| *level <= jdk.major);
        Some(ProjectJdk {
            language_level: choice.language_level.or(compiled).unwrap_or(jdk.major),
            language_level_default,
            chosen: is_chosen,
            jdk,
            build_level,
        })
    }

    pub fn choose(&mut self, root: &Path, home: Option<&Path>) -> Result<(), String> {
        let home = match home {
            Some(home) => {
                let jdks = self.jdks();
                let wanted =
                    detect::read(home, JdkSource::Added).map(|jdk| jdk.home).unwrap_or_else(|| home.to_owned());
                if !jdks.iter().any(|jdk| jdk.home == wanted) {
                    return Err(home.display().to_string());
                }
                Some(wanted)
            }
            None => None,
        };
        self.kept.projects.entry(key(root)).or_default().home = home;
        self.save();
        Ok(())
    }

    pub fn set_language_level(&mut self, root: &Path, level: Option<u32>) {
        self.kept.projects.entry(key(root)).or_default().language_level = level;
        self.save();
    }

    fn save(&mut self) {
        self.kept.projects.retain(|_, choice| *choice != ProjectChoice::default());
        if let Some(data) = &self.data {
            let _ = std::fs::create_dir_all(data);
            let _ = zen_vfs::write_atomic(&data.join(FILE), &serde_json::to_vec_pretty(&self.kept).unwrap_or_default());
        }
    }
}

fn matching_jdk(jdks: &[Jdk], level: u32) -> Option<&Jdk> {
    jdks.iter().find(|jdk| jdk.major == level && jdk.compiler)
}

pub(crate) fn default_jdk(jdks: &[Jdk]) -> Option<&Jdk> {
    jdks.iter().find(|jdk| jdk.source == JdkSource::JavaHome).or_else(|| {
        let newest = |compiler: bool| {
            jdks.iter().filter(|jdk| !compiler || jdk.compiler).max_by(|a, b| {
                a.major.cmp(&b.major).then_with(|| detect::major_of(&a.version).cmp(&detect::major_of(&b.version)))
            })
        };
        newest(true).or_else(|| newest(false))
    })
}

fn key(root: &Path) -> String {
    root.display().to_string()
}

#[derive(Clone)]
pub struct Jdks(Arc<Mutex<Store>>);

impl Default for Jdks {
    fn default() -> Self {
        Self(Arc::clone(shared()))
    }
}

impl Jdks {
    pub fn new(store: Store) -> Self {
        Self(Arc::new(Mutex::new(store)))
    }

    pub fn store(&self) -> MutexGuard<'_, Store> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn shared() -> &'static Arc<Mutex<Store>> {
    static SHARED: OnceLock<Arc<Mutex<Store>>> = OnceLock::new();
    SHARED.get_or_init(Arc::default)
}

pub fn set_data_dir(data: PathBuf) {
    let mut store = shared().lock().unwrap_or_else(PoisonError::into_inner);
    if store.data.as_ref() != Some(&data) {
        store.set_data_dir(data);
    }
}

pub fn project_jdk(root: &Path) -> Option<ProjectJdk> {
    shared().lock().unwrap_or_else(PoisonError::into_inner).project(root)
}

pub fn newest_at_least(version: u32) -> Option<PathBuf> {
    let jdks = shared().lock().unwrap_or_else(PoisonError::into_inner).jdks();
    jdks.into_iter().filter(|jdk| jdk.major >= version).max_by_key(|jdk| jdk.major).map(|jdk| jdk.home)
}

pub fn project_home(root: &Path) -> Option<PathBuf> {
    project_jdk(root).map(|project| project.jdk.home)
}

pub fn with_project_jdk<K, V>(env: &mut Vec<(K, V)>, project: Option<&ProjectJdk>)
where
    K: From<String> + AsRef<std::ffi::OsStr>,
    V: From<String> + AsRef<std::ffi::OsStr>,
{
    let Some(project) = project else { return };
    let home = project.jdk.home.display().to_string();
    env.retain(|(key, _)| key.as_ref() != "JAVA_HOME");
    env.push((K::from("JAVA_HOME".to_owned()), V::from(home)));
    let bin = project.jdk.home.join("bin");
    let path = env
        .iter()
        .position(|(key, _)| key.as_ref() == "PATH")
        .map(|at| OsString::from(env.remove(at).1.as_ref()))
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    let mut paths = vec![bin.clone()];
    paths.extend(std::env::split_paths(&path).filter(|folder| *folder != bin));
    let joined = std::env::join_paths(paths).map(|joined| joined.to_string_lossy().into_owned()).unwrap_or_default();
    env.push((K::from("PATH".to_owned()), V::from(joined)));
}

pub const RUNTIMES: &str = "${javaRuntimes}";

const JVM_LANGUAGES: &[&str] = &["java", "kotlin", "groovy", "scala", "clojure"];

const JVM_MARKERS: &[&str] = &[
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "settings.gradle",
    "settings.gradle.kts",
    "mvnw",
    "gradlew",
    ".java-version",
    ".sdkmanrc",
];

pub fn is_jvm_project(root: &Path) -> bool {
    JVM_MARKERS.iter().any(|marker| root.join(marker).exists())
}

pub fn for_server(server: &zen_lsp::ServerDeclaration, root: &Path) -> Option<ProjectJdk> {
    let jvm = server.languages.iter().any(|language| JVM_LANGUAGES.contains(&language.as_str()));
    let names = |text: &str| text.contains("${jdkHome}") || text.contains("${java");
    let named = names(&server.command)
        || server.args.iter().any(|arg| names(arg))
        || names(&server.initialization_options.to_string());
    (jvm || named).then(|| project_jdk(root)).flatten()
}

pub fn expand_text(text: &str, project: Option<&ProjectJdk>) -> String {
    let Some(project) = project else { return text.to_owned() };
    text.replace("${jdkHome}", &project.jdk.home.display().to_string())
        .replace("${javaVersion}", &project.jdk.version)
        .replace("${javaLanguageLevel}", &project.language_level.to_string())
}

fn runtime_name(major: u32) -> String {
    if major <= 8 { format!("JavaSE-1.{major}") } else { format!("JavaSE-{major}") }
}

pub fn expand(value: &serde_json::Value, project: Option<&ProjectJdk>) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) if text == RUNTIMES => project
            .map(|project| {
                let name = runtime_name(project.jdk.major);
                serde_json::json!([{ "name": name, "path": project.jdk.home, "default": true }])
            })
            .unwrap_or_else(|| serde_json::json!([])),
        serde_json::Value::String(text) if text.contains("${") => serde_json::Value::String(expand_text(text, project)),
        serde_json::Value::Array(items) => items.iter().map(|item| expand(item, project)).collect(),
        serde_json::Value::Object(fields) => {
            fields.iter().map(|(name, item)| (name.clone(), expand(item, project))).collect()
        }
        other => other.clone(),
    }
}

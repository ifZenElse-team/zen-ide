use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use crate::build_tools::{BuildEnv, Load, LoadState};
use crate::ide_settings::zip::{Archive, Limits};

pub const KEY: &str = "gradle.download";

pub const SERVICE: &str = "https://services.gradle.org/";

pub const CURRENT: &str = "https://services.gradle.org/versions/current";

const LIMITS: Limits =
    Limits { archive: 512 * 1024 * 1024, entries: 20_000, entry: 256 * 1024 * 1024, total: 1024 * 1024 * 1024 };

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub download_url: String,
    pub checksum_url: String,
}

pub fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && version.len() <= 40
        && version.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        && !version.starts_with(['.', '-'])
}

pub fn release(version: &str) -> Release {
    let download_url = format!("{SERVICE}distributions/gradle-{version}-bin.zip");
    Release { version: version.to_owned(), checksum_url: format!("{download_url}.sha256"), download_url }
}

pub fn parse_current(bytes: &[u8]) -> Result<Release, String> {
    let found: Release = serde_json::from_slice(bytes).map_err(|error| format!("{CURRENT}: {error}"))?;
    if !valid_version(&found.version)
        || !found.download_url.starts_with(SERVICE)
        || !found.checksum_url.starts_with(SERVICE)
    {
        return Err(t!("gradle-download-unexpected", url = CURRENT));
    }
    Ok(found)
}

pub fn folder(data: &Path, version: &str) -> PathBuf {
    data.join("tools/gradle").join(version)
}

fn program(folder: &Path) -> PathBuf {
    folder.join("bin").join(if cfg!(windows) { "gradle.bat" } else { "gradle" })
}

pub fn install(archive: Vec<u8>, sha256: &str, version: &str, target: &Path) -> Result<(), String> {
    let expected = sha256.split_whitespace().next().unwrap_or_default();
    if !zen_update::sha256_hex(&archive).eq_ignore_ascii_case(expected) {
        return Err(t!("gradle-download-checksum", version = version));
    }
    let archive = Archive::parse(archive, LIMITS).map_err(|error| error.to_string())?;
    let unpacked = target.with_extension("partial");
    let _ = std::fs::remove_dir_all(&unpacked);
    for entry in archive.entries() {
        let path = unpacked.join(&entry.name);
        if entry.folder {
            std::fs::create_dir_all(&path).map_err(|error| error.to_string())?;
            continue;
        }
        if entry.link {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, archive.read(entry).map_err(|error| error.to_string())?)
            .map_err(|error| error.to_string())?;
    }
    let inner = unpacked.join(format!("gradle-{version}"));
    if !program(&inner).is_file() {
        let _ = std::fs::remove_dir_all(&unpacked);
        return Err(t!("gradle-download-not-gradle", version = version));
    }
    #[cfg(unix)]
    for entry in std::fs::read_dir(inner.join("bin")).map_err(|error| error.to_string())?.flatten() {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(entry.path(), std::fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
    }
    let _ = std::fs::remove_dir_all(target);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    std::fs::rename(&inner, target).map_err(|error| error.to_string())?;
    let _ = std::fs::remove_dir_all(&unpacked);
    Ok(())
}

/// The parameters of `gradle.distribution.download`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DownloadParams {
    /// The Gradle version (`9.2.1`); the current release that services.gradle.org names when absent.
    #[serde(default)]
    pub version: Option<String>,
}

/// What `gradle.distribution.download` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    /// The version, when it was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Where it is unpacked, when the version is known; its `bin/gradle` is Gradle on this computer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// The background task that downloads it (`tasks.log.read`), when it is not there already.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task: Option<u64>,
}

pub struct DownloadGradle;

impl Command for DownloadGradle {
    type Params = DownloadParams;
    type Output = Download;
    const ID: &'static str = "gradle.distribution.download";
    const TITLE: &'static str = "Download Gradle";
    const DESCRIPTION: &'static str = "Downloads a Gradle distribution from services.gradle.org (the current release \
         that services.gradle.org/versions/current names, or the version given), checks it against the SHA-256 \
         published next to it, and unpacks it into the IDE's data folder in a background task, where it is Gradle on \
         this computer for builds without a wrapper. The network is used only when this command runs.";

    fn run(&self, session: &mut Session, params: DownloadParams) -> Result<Download, CommandError> {
        let env = session.state::<BuildEnv>().cloned().unwrap_or_default();
        let data = env.data_dir().ok_or_else(|| CommandError::new(t!("gradle-download-no-folder")))?;
        let version = params.version.map(|version| version.trim().to_owned()).filter(|version| !version.is_empty());
        if let Some(version) = &version {
            if !valid_version(version) {
                return Err(CommandError::new(t!("gradle-download-bad-version", version = version.as_str())));
            }
            let target = folder(&data, version);
            if program(&target).is_file() {
                return Ok(Download { version: Some(version.clone()), folder: Some(target), task: None });
            }
        }
        if let Some(load) = env.load(KEY).filter(|load| load.state == LoadState::Loading) {
            return Ok(Download { version, folder: None, task: load.task });
        }
        let title = t!("gradle-download-task", version = version.clone().unwrap_or_default());
        let task = session.state_mut::<crate::tasks::Tasks>().start(title.trim(), "gradle", true);
        let id = task.id();
        let started = std::time::Instant::now();
        let loading =
            Load { state: LoadState::Loading, output: String::new(), error: None, task: Some(id), started, took: None };
        env.set_load(KEY, loading);
        let wanted = version.clone();
        let target = version.as_deref().map(|version| folder(&data, version));
        std::thread::spawn(move || {
            let ran = download(&data, wanted.as_deref(), &task);
            let took = Some(started.elapsed());
            let load = match &ran {
                Ok(done) => {
                    Load { state: LoadState::Loaded, output: done.clone(), error: None, task: Some(id), started, took }
                }
                Err(error) => Load {
                    state: LoadState::Failed,
                    output: String::new(),
                    error: Some(error.clone()),
                    task: Some(id),
                    started,
                    took,
                },
            };
            env.set_load(KEY, load);
            task.finish(ran);
        });
        Ok(Download { version, folder: target, task: Some(id) })
    }
}

#[cfg(target_arch = "wasm32")]
fn download(_data: &Path, _version: Option<&str>, _task: &crate::tasks::TaskHandle) -> Result<String, String> {
    Err(t!("gradle-download-unavailable"))
}

#[cfg(not(target_arch = "wasm32"))]
fn download(data: &Path, version: Option<&str>, task: &crate::tasks::TaskHandle) -> Result<String, String> {
    use std::time::Duration;
    let get = |url: &str, limit: u64, wait: u64| {
        task.log(url);
        zen_update::fetch::document(url, limit, Duration::from_secs(wait))
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("{url}: not found"))
    };
    let found = match version {
        Some(version) => release(version),
        None => parse_current(&get(CURRENT, 64 * 1024, 60)?)?,
    };
    let target = folder(data, &found.version);
    if program(&target).is_file() {
        return Ok(t!("gradle-download-done", folder = target.display().to_string()));
    }
    let sha256 = String::from_utf8_lossy(&get(&found.checksum_url, 1024, 60)?).into_owned();
    let archive = get(&found.download_url, LIMITS.archive, 1800)?;
    install(archive, &sha256, &found.version, &target)?;
    Ok(t!("gradle-download-done", folder = target.display().to_string()))
}

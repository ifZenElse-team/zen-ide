use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::detect::{self, Finding};
use zen_db::registry::{Origin, OriginKind, Scope};
use zen_i18n::t;

use super::project_key;
use super::sources::{self, Database, SourceAnswer, SourceFields};
use crate::Workbench;
use crate::hidden_files;
use crate::workspaces::Workspaces;

const LISTED: usize = 50;
const DEPTH: usize = 4;
const ENTRIES: usize = 20_000;
const FILES: usize = 64;

#[derive(Default)]
pub struct Detections {
    root: Option<PathBuf>,
    found: Vec<Finding>,
    scanned: bool,
    stale: bool,
    running: Option<Running>,
}

struct Running {
    cancel: Arc<AtomicBool>,
    result: Arc<Mutex<Option<Vec<Finding>>>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl Detections {
    pub fn scanning(&self) -> bool {
        self.running.is_some()
    }
}

pub fn files_changed(session: &mut Session, paths: &[PathBuf]) {
    let touched =
        paths.iter().any(|path| path.file_name().is_some_and(|name| detect::is_candidate(&name.to_string_lossy())));
    if touched {
        session.state_mut::<Detections>().stale = true;
    }
}

pub fn poll(session: &mut Session) {
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf);
    let index = session.state::<Workbench>().and_then(Workbench::file_index).cloned();
    let waker = session.state::<Database>().and_then(Database::waker);
    let detections = session.state_mut::<Detections>();
    if detections.root != root {
        *detections = Detections { root: root.clone(), stale: true, ..Detections::default() };
    }
    if let Some(running) = &detections.running {
        let finished = running.result.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(found) = finished {
            detections.found = found;
            detections.scanned = true;
            detections.running = None;
        }
    }
    let Some(root) = root else { return };
    if !detections.stale || detections.running.is_some() {
        return;
    }
    detections.stale = false;
    let cancel = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None));
    detections.running = Some(Running { cancel: Arc::clone(&cancel), result: Arc::clone(&result) });
    let exclusions = index.as_ref().map(|index| index.exclusions().clone()).unwrap_or_default();
    let spawned = std::thread::Builder::new().name("zen-db-detect".to_owned()).spawn(move || {
        let found = scan(&root, &exclusions, index.as_ref(), &cancel);
        *result.lock().unwrap_or_else(PoisonError::into_inner) = Some(found);
        if let Some(waker) = waker {
            waker();
        }
    });
    if spawned.is_err() {
        detections.running = None;
    }
}

fn skipped(name: &str) -> bool {
    zen_vfs::HIDDEN.contains(&name) || zen_vfs::TOOL_FOLDERS.contains(&name)
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn read_bounded(path: &Path) -> Option<String> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > detect::FILE_LIMIT {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

pub fn scan(
    root: &Path,
    exclusions: &zen_index::Exclusions,
    index: Option<&zen_index::FileIndex>,
    cancel: &AtomicBool,
) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let (mut seen, mut read) = (0usize, 0usize);
    'folders: while let Some((folder, depth)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return Vec::new();
        }
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        let (mut envs, mut composes) = (Vec::new(), Vec::new());
        for entry in entries {
            seen += 1;
            if seen > ENTRIES {
                break 'folders;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                let walk = depth < DEPTH
                    && !skipped(&name)
                    && !exclusions.excludes_entry(&path)
                    && !zen_vfs::excluded_folders::is_excluded(&path)
                    && !index.is_some_and(|index| index.git_ignores(&path));
                if walk {
                    queue.push_back((path, depth + 1));
                }
            } else if !exclusions.excludes_entry(&path) && (kind.is_file() || path.is_file()) {
                if detect::is_env_file(&name) {
                    envs.push((detect::env_rank(&name), name, path));
                } else if detect::is_compose_file(&name) {
                    composes.push(path);
                }
            }
        }
        envs.sort();
        let mut vars = HashMap::new();
        let mut env_file = None;
        for (_, name, path) in envs {
            if read >= FILES {
                break 'folders;
            }
            read += 1;
            let Some(text) = read_bounded(&path) else { continue };
            let file = relative(root, &path);
            if name == ".env" {
                vars = detect::parse_env(&text).into_iter().collect();
                env_file = Some(file.clone());
            }
            found.extend(detect::from_env(&file, &text));
        }
        for path in composes {
            if read >= FILES {
                break 'folders;
            }
            read += 1;
            let Some(text) = read_bounded(&path) else { continue };
            found.extend(detect::from_compose(&relative(root, &path), &text, &vars, env_file.as_deref()));
        }
    }
    let mut fingerprints = std::collections::HashSet::new();
    found.retain(|finding| fingerprints.insert(finding.fingerprint()));
    found
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Dismissed {
    #[serde(default = "one")]
    version: u32,
    #[serde(default)]
    dismissed: BTreeMap<String, String>,
}

fn one() -> u32 {
    1
}

fn dismissed_file(session: &Session) -> Option<PathBuf> {
    let data = session.state::<Workspaces>().and_then(Workspaces::data_dir)?;
    let project = session.state::<Workbench>().and_then(Workbench::project)?;
    Some(data.join("databases").join(project_key(project)).join("detections.json"))
}

fn read_dismissed(session: &Session) -> Dismissed {
    dismissed_file(session).and_then(|file| zen_db::registry::read_json(&file).ok().flatten()).unwrap_or_default()
}

/// Which detections to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// The most detections to answer; 50 when absent.
    pub limit: Option<usize>,
    /// Also those the person dismissed.
    pub include_dismissed: bool,
    /// Scan the project's files again first (in the background; the answer says `scanning`).
    pub refresh: bool,
}

/// A database the project names, offered as a connection; never with its password.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DetectionAnswer {
    /// Its id, for `db.detect.accept` and `db.detect.dismiss`.
    pub id: String,
    /// The kind of file: `env` or `compose`.
    pub kind: OriginKind,
    /// The file, relative to the project.
    pub file: String,
    /// The key (`DATABASE_URL`), group (`POSTGRES_*`) or Compose service.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The driver's id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    /// The product's name (`PostgreSQL`, `MariaDB`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    /// The name a connection made from it takes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The port; the driver's default when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// The user name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// For a Compose service: whether its port is published to this computer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<bool>,
    /// Whether the file holds a password, which Add stores in the Keychain.
    pub has_password: bool,
    /// Whether the IDE has its driver, so that it can be added.
    pub supported: bool,
    /// Whether the person dismissed it.
    pub dismissed: bool,
    /// The connection added from it earlier, when the file changed since.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// Whether its values are left out because its file is hidden from agents (ADR-0143).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub redacted: bool,
}

/// The detections of the open project.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAnswer {
    /// The offers, in the order of their files.
    pub detections: Vec<DetectionAnswer>,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
    /// Whether a scan is running; list again for its result.
    pub scanning: bool,
    /// How many the person dismissed and are not listed.
    pub dismissed: usize,
}

fn same_place(a: Option<&str>, b: Option<&str>) -> bool {
    a.unwrap_or_default().eq_ignore_ascii_case(b.unwrap_or_default())
}

enum Known {
    Added,
    Changed(String),
    New,
}

fn known(finding: &Finding, entries: &[zen_db::registry::Entry]) -> Known {
    let fingerprint = finding.fingerprint();
    if let Some(entry) = entries.iter().find(|entry| {
        entry.source.origin.as_ref().and_then(|origin| origin.detection.as_deref()) == Some(finding.id.as_str())
    }) {
        let unchanged =
            entry.source.origin.as_ref().and_then(|origin| origin.fingerprint.as_deref()) == Some(fingerprint.as_str());
        return if unchanged { Known::Added } else { Known::Changed(entry.source.id.clone()) };
    }
    let same = entries.iter().any(|entry| {
        entry.source.driver == finding.driver
            && same_place(entry.source.host.as_deref(), finding.host.as_deref())
            && entry.source.port == finding.port
            && same_place(entry.source.database.as_deref(), finding.database.as_deref())
    });
    if same { Known::Added } else { Known::New }
}

fn redacted_for(session: &Session, finding: &Finding) -> bool {
    let Some(patterns) = hidden_files::for_caller(session) else { return false };
    let root = session.state::<Workbench>().and_then(Workbench::project);
    finding.files.iter().any(|file| {
        hidden_files::is_hidden(&patterns, root, &root.map_or_else(|| PathBuf::from(file), |root| root.join(file)))
    })
}

pub fn offer(session: &mut Session, id: &str) -> Option<Finding> {
    poll(session);
    session.state::<Detections>()?.found.iter().find(|finding| finding.id == id).cloned()
}

pub fn hidden(session: &Session, id: &str, patterns: &[String]) -> bool {
    let root = session.state::<Workbench>().and_then(Workbench::project);
    session
        .state::<Detections>()
        .and_then(|detections| detections.found.iter().find(|finding| finding.id == id))
        .is_some_and(|finding| {
            finding.files.iter().any(|file| {
                hidden_files::is_hidden(
                    patterns,
                    root,
                    &root.map_or_else(|| PathBuf::from(file), |root| root.join(file)),
                )
            })
        })
}

pub struct ListDetections;

impl Command for ListDetections {
    type Params = ListParams;
    type Output = ListAnswer;
    const ID: &'static str = "db.detect.list";
    const TITLE: &'static str = "List Detected Databases";
    const DESCRIPTION: &'static str = "Lists the databases the open project names, offered as connections: `DATABASE_URL`-style values and the `PG*`, `POSTGRES_*` and `MYSQL_*` groups of `.env` files, and PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, ClickHouse, Elasticsearch, OpenSearch, Cassandra and ScyllaDB services of Compose files with their published ports. Files are scanned in the background (`scanning` until done; `refresh` scans again), four folders deep, leaving out excluded and ignored folders. Offers already added, or the same as a connection, are left out; a changed file offers again with the `connection` it would change. Never answers a password (`hasPassword` only); for agents, offers from files hidden from them come without values (`redacted`). Connects nowhere.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ListAnswer, CommandError> {
        if params.refresh {
            session.state_mut::<Detections>().stale = true;
        }
        poll(session);
        let entries = sources::files(session).entries().unwrap_or_default();
        let dismissed = read_dismissed(session);
        let drivers: Vec<String> =
            session.state_mut::<Database>().drivers().iter().map(|driver| driver.id().to_owned()).collect();
        let detections = session.state::<Detections>();
        let scanning = detections.is_some_and(Detections::scanning);
        let found: Vec<Finding> = detections.map(|detections| detections.found.clone()).unwrap_or_default();
        let mut hidden_dismissed = 0;
        let mut offers = Vec::new();
        for finding in &found {
            let connection = match known(finding, &entries) {
                Known::Added => continue,
                Known::Changed(id) => Some(id),
                Known::New => None,
            };
            let is_dismissed = dismissed.dismissed.get(&finding.id) == Some(&finding.fingerprint());
            if is_dismissed && !params.include_dismissed {
                hidden_dismissed += 1;
                continue;
            }
            let supported = drivers.contains(&finding.driver);
            let mut offer = DetectionAnswer {
                id: finding.id.clone(),
                kind: finding.kind,
                file: finding.file.clone(),
                key: Some(finding.key.clone()),
                driver: Some(finding.driver.clone()),
                product: Some(finding.product.clone()),
                name: Some(finding.name()),
                host: finding.host.clone(),
                port: finding.port,
                database: finding.database.clone(),
                user: finding.user.clone(),
                published: (finding.kind == OriginKind::Compose).then_some(finding.published),
                has_password: finding.password.is_some(),
                supported,
                dismissed: is_dismissed,
                connection,
                redacted: false,
            };
            if redacted_for(session, finding) {
                offer = DetectionAnswer {
                    key: None,
                    driver: None,
                    product: None,
                    name: None,
                    host: None,
                    port: None,
                    database: None,
                    user: None,
                    published: None,
                    has_password: false,
                    redacted: true,
                    ..offer
                };
            }
            offers.push(offer);
        }
        let total = offers.len();
        let limit = params.limit.unwrap_or(LISTED);
        offers.truncate(limit);
        Ok(ListAnswer { detections: offers, total, truncated: total > limit, scanning, dismissed: hidden_dismissed })
    }
}

/// A detection to add as a connection.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct AcceptParams {
    /// The detection's id from `db.detect.list`.
    pub id: String,
    /// Where to keep a new connection: `global` (default) or `project`.
    pub scope: Scope,
    /// Settings that replace the detected ones (the connection dialog's fields).
    #[serde(flatten)]
    pub fields: SourceFields,
}

/// A connection made from a detection.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Accepted {
    /// The connection, without secrets.
    pub connection: SourceAnswer,
    /// Whether it changed the connection added from this detection earlier rather than adding one.
    pub replaced: bool,
}

fn unknown(session: &Session, id: &str) -> CommandError {
    let message = super::for_caller(
        session,
        t!("database-error-unknown-detection"),
        t!("database-error-unknown-detection-agent", id = id),
    );
    CommandError::with_code("unknownDetection", message)
}

pub struct AcceptDetection;

impl Command for AcceptDetection {
    type Params = AcceptParams;
    type Output = Accepted;
    const ID: &'static str = "db.detect.accept";
    const TITLE: &'static str = "Add Detected Database";
    const DESCRIPTION: &'static str = "Adds the detection `id` of `db.detect.list` as a connection (`scope`: `global` by default, or `project`), marked with its file (`origin`), any field of `db.dataSource.add` replacing the detected one. A password found in the file is stored in the Keychain only now, never in a file, and is never answered. When a connection was added from it before and the file changed, that connection is changed instead (`replaced`). Agents cannot add one from a file hidden from them. Connects nowhere.";

    fn run(&self, session: &mut Session, params: AcceptParams) -> Result<Accepted, CommandError> {
        poll(session);
        let finding = session
            .state::<Detections>()
            .and_then(|detections| detections.found.iter().find(|finding| finding.id == params.id).cloned())
            .ok_or_else(|| unknown(session, &params.id))?;
        if redacted_for(session, &finding) {
            return Err(hidden_files::refused(&finding.file));
        }
        let detected = SourceFields {
            name: Some(finding.name()),
            driver: Some(finding.driver.clone()),
            host: finding.host.clone(),
            port: finding.port,
            database: finding.database.clone(),
            user: finding.user.clone(),
            password: finding.password.as_ref().map(|secret| secret.expose().to_owned()),
            ..SourceFields::default()
        };
        let origin = Origin {
            kind: finding.kind,
            file: finding.file.clone(),
            key: Some(finding.key.clone()),
            detection: Some(finding.id.clone()),
            fingerprint: Some(finding.fingerprint()),
        };
        let entries = sources::files(session).entries().unwrap_or_default();
        let earlier = match known(&finding, &entries) {
            Known::Changed(id) => Some(id),
            Known::Added | Known::New => None,
        };
        let replaced = earlier.is_some();
        let connection = match earlier {
            Some(id) => {
                let mut changed = detected;
                changed.name = None;
                sources::update(session, &id, &[&changed, &params.fields], Some(origin))?
            }
            None => sources::add(session, params.scope, &[&detected, &params.fields], Some(origin))?,
        };
        Ok(Accepted { connection, replaced })
    }
}

/// A detection to dismiss.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DismissParams {
    /// The detection's id from `db.detect.list`.
    pub id: String,
    /// Offer it again instead.
    pub undo: bool,
}

/// A detection dismissed or offered again.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DismissAnswer {
    /// The detection's id.
    pub id: String,
    /// Whether it is dismissed now.
    pub dismissed: bool,
}

pub struct DismissDetection;

impl Command for DismissDetection {
    type Params = DismissParams;
    type Output = DismissAnswer;
    const ID: &'static str = "db.detect.dismiss";
    const TITLE: &'static str = "Dismiss Detected Database";
    const DESCRIPTION: &'static str = "Stops offering the detection `id` of `db.detect.list` in the open project, remembered in the IDE's data folder (`databases/<project key>/detections.json`, never in the project) until its file names another database; `undo` offers it again. Connects nowhere.";

    fn run(&self, session: &mut Session, params: DismissParams) -> Result<DismissAnswer, CommandError> {
        poll(session);
        let fingerprint = session
            .state::<Detections>()
            .and_then(|detections| detections.found.iter().find(|finding| finding.id == params.id))
            .map(Finding::fingerprint);
        let file = dismissed_file(session).ok_or_else(|| CommandError::new(t!("database-error-no-data-folder")))?;
        let mut dismissed = read_dismissed(session);
        if params.undo {
            dismissed.dismissed.remove(&params.id);
        } else {
            let fingerprint = fingerprint.ok_or_else(|| unknown(session, &params.id))?;
            dismissed.dismissed.insert(params.id.clone(), fingerprint);
        }
        dismissed.version = 1;
        zen_db::registry::write_json(&file, &dismissed)
            .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
        session.state_mut::<Workbench>().revision += 1;
        Ok(DismissAnswer { id: params.id, dismissed: !params.undo })
    }
}

#[cfg(test)]
mod tests;

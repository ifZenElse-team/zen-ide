use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::backup::{
    Detection, DockerState, Family, Found, MysqlRestoreOptions, MysqldumpOptions, Origin, PgDumpOptions, PgFormat,
    PgRestoreOptions, Probe, Target, Tool, VersionProblem,
};
use zen_db::registry::{AuthKind, Entry};
use zen_i18n::t;
use zen_run::{CommandLine, Configuration, Process, ProcessEvent, ProcessSink, RunOutput, RunState, SharedOutput};
use zen_secrets::database::Kind as SecretKind;
use zen_secrets::{Secret, SecretError, SecretId, SecretStore};

use super::safety::{AGENT_NOT_ALLOWED, CONFIRM_EXECUTE, READ_ONLY, agent_access, agent_changes, read_only, target};
use super::sources::{find, has_password, secret_id, secret_store};
use crate::run::{RunControl, Runs};
use crate::workspaces::Workspaces;

pub const RUN_KIND: &str = "databaseBackup";

const LISTED: usize = 20;
const WAIT_LIMIT: u64 = 60_000;

/// The backup settings kept in `<data folder>/databases/backup.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Remembered {
    /// The file's version, 1.
    pub version: u32,
    /// The folder backups go to.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<PathBuf>,
    /// The options of `pg_dump`.
    pub pg_dump: PgDumpOptions,
    /// The options of `mysqldump`.
    pub mysqldump: MysqldumpOptions,
    /// The options of `pg_restore` and `psql`.
    pub pg_restore: PgRestoreOptions,
    /// The options of `mysql`.
    pub mysql: MysqlRestoreOptions,
}

#[derive(Default)]
struct Slot {
    key: String,
    detection: Option<Detection>,
    running: bool,
}

type SharedSlot = Arc<Mutex<Slot>>;

#[derive(Clone)]
struct Listed {
    run_id: u64,
    job: Arc<Job>,
}

#[derive(Default)]
pub struct Backups {
    detections: HashMap<String, SharedSlot>,
    asked: Option<Asked>,
    jobs: Vec<Listed>,
    remembered: Option<Remembered>,
    probe: Option<Probe>,
}

pub fn set_probe(session: &mut Session, probe: Probe) {
    let backups = session.state_mut::<Backups>();
    backups.probe = Some(probe);
    backups.detections.clear();
}

fn probe(session: &mut Session) -> Probe {
    let project = session.state::<crate::Workbench>().and_then(crate::Workbench::project).map(Path::to_path_buf);
    session.state_mut::<Backups>().probe.clone().unwrap_or_else(|| Probe::system_outside(project.as_deref()))
}

fn agent_arguments(session: &Session, extra: &[&[String]], remember: Option<bool>) -> Result<bool, CommandError> {
    if !zen_core::macros::by_agent(session) {
        return Ok(remember.unwrap_or(true));
    }
    if extra.iter().any(|args| !args.is_empty()) {
        return Err(CommandError::with_code("agentExtraArgs", t!("database-backup-agent-extra-args")));
    }
    Ok(false)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn file_path(session: &Session) -> Option<PathBuf> {
    session.state::<Workspaces>().and_then(Workspaces::data_dir).map(|data| data.join("databases").join("backup.json"))
}

pub fn remembered(session: &mut Session) -> Remembered {
    if let Some(remembered) = session.state::<Backups>().and_then(|backups| backups.remembered.clone()) {
        return remembered;
    }
    let read = file_path(session)
        .and_then(|file| zen_db::registry::read_json::<Remembered>(&file).ok().flatten())
        .unwrap_or(Remembered { version: 1, ..Remembered::default() });
    session.state_mut::<Backups>().remembered = Some(read.clone());
    read
}

fn remember(session: &mut Session, remembered: Remembered) -> Result<(), CommandError> {
    if session.state::<Backups>().and_then(|backups| backups.remembered.as_ref()) == Some(&remembered) {
        return Ok(());
    }
    if let Some(path) = file_path(session) {
        zen_db::registry::write_json(&path, &remembered)
            .map_err(|error| CommandError::new(t!("database-error-files", error = error.to_string())))?;
    }
    session.state_mut::<Backups>().remembered = Some(remembered);
    Ok(())
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

struct Resolved {
    entry: Entry,
    family: Family,
    target: Target,
}

fn resolve(
    session: &mut Session,
    connection: &str,
    database: Option<&str>,
    needed: bool,
) -> Result<Resolved, CommandError> {
    let entry = find(session, connection)?;
    let family = Family::of(&entry.source.driver).ok_or_else(|| {
        CommandError::with_code(
            zen_db::backup::UNSUPPORTED,
            t!("database-backup-unsupported", name = entry.source.name.as_str()),
        )
    })?;
    if entry.source.ssh.as_ref().is_some_and(|ssh| !ssh.host.trim().is_empty()) {
        return Err(CommandError::with_code("sshTunnel", t!("database-backup-ssh", name = entry.source.name.as_str())));
    }
    let database = database
        .map(str::trim)
        .filter(|database| !database.is_empty())
        .map(str::to_owned)
        .or_else(|| entry.source.database.clone().filter(|database| !database.trim().is_empty()))
        .or_else(|| (!needed).then(String::new))
        .ok_or_else(|| {
            CommandError::with_code("noDatabase", t!("database-backup-no-database", name = entry.source.name.as_str()))
        })?;
    let target = Target {
        family: Some(family),
        host: entry.source.host.clone().unwrap_or_default(),
        port: entry.source.port.unwrap_or_else(|| family.default_port()),
        database,
        user: entry.source.user.clone(),
        tls: entry.source.tls.clone(),
    };
    Ok(Resolved { entry, family, target })
}

fn slot_key(target: &Target) -> String {
    format!("{:?}|{}|{}", target.family, target.host.trim().to_ascii_lowercase(), target.port)
}

fn slot(session: &mut Session, connection: &str, target: &Target, refresh: bool) -> (SharedSlot, bool) {
    let key = slot_key(target);
    let slot = Arc::clone(session.state_mut::<Backups>().detections.entry(connection.to_owned()).or_default());
    let start = {
        let mut held = lock(&slot);
        if held.key != key || refresh {
            held.key.clone_from(&key);
            held.detection = None;
        }
        let start = held.detection.is_none() && !held.running;
        held.running |= start;
        start
    };
    (slot, start)
}

fn detect_into(slot: &SharedSlot, probe: &Probe, family: Family, target: &Target) -> Detection {
    let key = slot_key(target);
    let detection = zen_db::backup::detect(probe, family, &target.host, target.port);
    let mut held = lock(slot);
    if held.key == key {
        held.detection = Some(detection.clone());
        held.running = false;
    }
    detection
}

fn wait_for(slot: &SharedSlot, wait: Duration) -> Option<Detection> {
    let deadline = Instant::now() + wait;
    loop {
        if let Some(detection) = lock(slot).detection.clone() {
            return Some(detection);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Which connection's tools to find.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolsParams {
    /// The connection's id.
    pub connection: String,
    /// Look again instead of answering what was found before.
    pub refresh: bool,
    /// How long to wait for the search to end, in milliseconds (at most 60 000); answers at once when absent.
    pub wait_ms: Option<u64>,
}

/// The tools found for a connection.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToolsAnswer {
    /// The connection's id.
    pub connection: String,
    /// The server's family.
    pub family: Family,
    /// `searching` while the tools are looked for, then `done`.
    pub status: String,
    /// Every tool found: on this computer first, then in the container.
    pub tools: Vec<Found>,
    /// What was learnt of Docker.
    pub docker: DockerState,
    /// The tool each job uses unless told otherwise: the backup tool, then the restore tools.
    pub chosen: Vec<Found>,
}

pub struct ListTools;

impl Command for ListTools {
    type Params = ToolsParams;
    type Output = ToolsAnswer;
    const ID: &'static str = "db.backup.tools";
    const TITLE: &'static str = "Find Backup Tools";
    const DESCRIPTION: &'static str = "Looks in the background for the backup and restore tools of connection `connection` (PostgreSQL: pg_dump, pg_restore, psql; MySQL and MariaDB: mysqldump or mariadb-dump, mysql or mariadb): on PATH, in Homebrew's folders (keg-only libpq, postgresql@N, mysql-client too), in Postgres.app, and, for a server on this computer, inside the running Docker container that publishes its port (run with docker exec). Each with its origin (path, homebrew, app, docker), program, container and version. `chosen` is the tool each job uses: the newest on this computer unless the container's is newer. Answers `status: searching` until done (`waitMs` waits up to 60 000 ms); `refresh` looks again. Never connects to the database; refused with `unsupported` for other drivers and `sshTunnel` for connections through SSH.";

    fn run(&self, session: &mut Session, params: ToolsParams) -> Result<ToolsAnswer, CommandError> {
        let resolved = resolve(session, &params.connection, None, false)?;
        let (slot, start) = slot(session, &params.connection, &resolved.target, params.refresh);
        if start {
            let probe = probe(session);
            let (slot, family, target) = (Arc::clone(&slot), resolved.family, resolved.target.clone());
            let waker = session.state::<Runs>().and_then(|runs| runs.waker().cloned());
            std::thread::Builder::new()
                .name("zen-db-backup-tools".to_owned())
                .spawn(move || {
                    detect_into(&slot, &probe, family, &target);
                    if let Some(waker) = waker {
                        waker();
                    }
                })
                .map_err(|error| CommandError::new(error.to_string()))?;
        }
        let wait = Duration::from_millis(params.wait_ms.unwrap_or(0).min(WAIT_LIMIT));
        let detection = wait_for(&slot, wait);
        let status = if detection.is_some() { "done" } else { "searching" }.to_owned();
        let detection = detection.unwrap_or_default();
        let chosen = resolved
            .family
            .tools()
            .iter()
            .filter_map(|&tool| zen_db::backup::choose(&detection, tool).cloned())
            .collect();
        Ok(ToolsAnswer {
            connection: params.connection,
            family: resolved.family,
            status,
            tools: detection.tools,
            docker: detection.docker,
            chosen,
        })
    }
}

/// What `db.backup.settings` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SettingsAnswer {
    /// The remembered settings.
    #[serde(flatten)]
    pub remembered: Remembered,
    /// The folder backups go to when none is remembered.
    pub default_folder: PathBuf,
}

pub struct BackupSettings;

impl Command for BackupSettings {
    type Params = zen_core::NoParams;
    type Output = SettingsAnswer;
    const ID: &'static str = "db.backup.settings";
    const TITLE: &'static str = "Backup Settings";
    const DESCRIPTION: &'static str = "The backup and restore settings remembered from the last run (`<data folder>/databases/backup.json`): the folder and the options of pg_dump, mysqldump, pg_restore (also psql's) and mysql, with the folder used when none is remembered.";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<SettingsAnswer, CommandError> {
        Ok(SettingsAnswer { remembered: remembered(session), default_folder: home() })
    }
}

/// Which tool to use instead of the chosen one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolChoice {
    /// Where it was found: `path`, `homebrew`, `app` or `docker`.
    pub origin: Option<Origin>,
    /// Its program, as `db.backup.tools` lists it.
    pub program: Option<PathBuf>,
}

impl ToolChoice {
    fn pick<'a>(&self, detection: &'a Detection, tool: Tool) -> Option<&'a Found> {
        if self.origin.is_none() && self.program.is_none() {
            return zen_db::backup::choose(detection, tool);
        }
        detection.tools.iter().find(|found| {
            found.tool == tool
                && self.origin.is_none_or(|origin| origin == found.origin)
                && self.program.as_ref().is_none_or(|program| *program == found.program)
        })
    }
}

/// What `db.backup.start` backs up and how.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct BackupParams {
    /// The connection's id.
    pub connection: String,
    /// The database; the connection's when absent.
    pub database: Option<String>,
    /// The file (a folder for pg_dump's directory format); in `folder`, named after the database and the time, when absent.
    pub file: Option<PathBuf>,
    /// The folder of a file not named; the remembered one, else the home folder, when absent.
    pub folder: Option<PathBuf>,
    /// The tool to use; the chosen one when absent.
    pub tool: ToolChoice,
    /// The options of `pg_dump`; the remembered ones when absent.
    pub pg_dump: Option<PgDumpOptions>,
    /// The options of `mysqldump`; the remembered ones when absent.
    pub mysqldump: Option<MysqldumpOptions>,
    /// Replace a file that exists; refused with code `exists` otherwise.
    pub overwrite: bool,
    /// The password for this run only, when none is saved; never stored or answered.
    #[serde(skip_serializing)]
    pub password: Option<String>,
    /// Keep the folder and the options for next time; true when absent.
    pub remember: Option<bool>,
}

/// What `db.backup.restore` restores and how.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RestoreParams {
    /// The connection's id.
    pub connection: String,
    /// The database restored into; the connection's when absent.
    pub database: Option<String>,
    /// The backup: a file, or the folder of pg_dump's directory format.
    pub file: PathBuf,
    /// The tool to use; the chosen one for the backup's format when absent.
    pub tool: ToolChoice,
    /// The options of `pg_restore` (and of `psql` for a plain SQL backup); the remembered ones when absent.
    pub pg_restore: Option<PgRestoreOptions>,
    /// The options of `mysql`; the remembered ones when absent.
    pub mysql: Option<MysqlRestoreOptions>,
    /// The `token` of the `confirmExecute` refusal, to go on.
    #[serde(skip_serializing)]
    pub confirmed: Option<String>,
    /// The password for this run only, when none is saved; never stored or answered.
    #[serde(skip_serializing)]
    pub password: Option<String>,
    /// Keep the options for next time; true when absent.
    pub remember: Option<bool>,
}

/// A backup or restore started as a run.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The run's id in the Run tool window, for `db.backup.state`, `db.backup.cancel` and `run.output.get`.
    pub run_id: u64,
    /// `backup` or `restore`.
    pub kind: String,
    /// The database.
    pub database: String,
    /// The backup's file or folder.
    pub file: PathBuf,
    /// The tool, when it was already known; the run finds it otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<Found>,
}

/// One backup or restore, as `db.backup.state` answers it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JobState {
    /// The run's id.
    pub run_id: u64,
    /// `backup` or `restore`.
    pub kind: String,
    /// The connection's id.
    pub connection: String,
    /// The database.
    pub database: String,
    /// The backup's file or folder.
    pub file: PathBuf,
    /// The tool, once known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<Found>,
    /// `running`, `finished`, `failed` or `stopped`.
    pub status: String,
    /// The program's exit code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Why it failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Milliseconds since it started, until it ended.
    pub elapsed_ms: u64,
    /// The size of the backup written, once a backup finished.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// Which backups and restores to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StateParams {
    /// One run; every one of the window, the newest first, when absent.
    pub run_id: Option<u64>,
    /// How long to wait for it to end, in milliseconds (at most 60 000).
    pub wait_ms: Option<u64>,
    /// The most to list; 20 when absent.
    pub limit: Option<usize>,
}

/// The backups and restores of the window.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StateAnswer {
    /// The newest first, at most `limit`.
    pub jobs: Vec<JobState>,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
}

/// The run to stop.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelParams {
    /// The run's id.
    pub run_id: u64,
}

/// Whether a run was stopped.
#[derive(Clone, Copy, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Cancelled {
    /// False when it had ended.
    pub cancelled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Backup,
    Restore,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Backup => "backup",
            Kind::Restore => "restore",
        }
    }
}

#[derive(Default)]
struct Progress {
    tool: Option<Found>,
    process: Option<Process>,
    ended: Option<Instant>,
    error: Option<String>,
    bytes: Option<u64>,
}

struct Job {
    kind: Kind,
    connection: String,
    database: String,
    file: PathBuf,
    part: Option<PathBuf>,
    overwrite: bool,
    output: SharedOutput,
    started: Instant,
    stopped: AtomicBool,
    progress: Mutex<Progress>,
    secret: Mutex<Option<SecretFile>>,
}

struct SecretFile {
    path: PathBuf,
    copied: Option<(PathBuf, String, String)>,
}

impl Job {
    fn forget_secret(&self) {
        let Some(secret) = lock(&self.secret).take() else { return };
        let _ = std::fs::remove_file(&secret.path);
        if let Some((docker, container, inside)) = secret.copied {
            let _ = std::process::Command::new(docker)
                .args(["exec", container.as_str(), "rm", "-f", "--", inside.as_str()])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }

    fn fail(&self, message: String) {
        self.forget_secret();
        self.remove_part();
        let mut progress = lock(&self.progress);
        progress.ended.get_or_insert_with(Instant::now);
        progress.error = Some(message.clone());
        drop(progress);
        self.output.update(|output| {
            output.system(&message);
            output.fail(message);
        });
    }

    fn remove_part(&self) {
        if let Some(part) = &self.part {
            let _ = if part.is_dir() { std::fs::remove_dir_all(part) } else { std::fs::remove_file(part) };
        }
    }

    fn event(&self, event: ProcessEvent) {
        match event {
            ProcessEvent::Output { stream, text } => self.output.update(|output| output.feed(stream, &text)),
            ProcessEvent::Exited { code } => self.exited(code),
        }
    }

    fn exited(&self, code: Option<i32>) {
        self.forget_secret();
        let stopped = self.stopped.load(Ordering::SeqCst);
        lock(&self.progress).process = None;
        if stopped {
            self.remove_part();
            lock(&self.progress).ended.get_or_insert_with(Instant::now);
            self.output.update(|output| {
                output.system(&t!("workbench-run-process-stopped"));
                output.end(RunState::Stopped, None);
            });
            return;
        }
        if code != Some(0) {
            self.remove_part();
            let message = match code {
                Some(code) => t!("database-backup-failed", code = code),
                None => t!("workbench-run-process-interrupted"),
            };
            let mut progress = lock(&self.progress);
            progress.ended.get_or_insert_with(Instant::now);
            progress.error = Some(message.clone());
            drop(progress);
            self.output.update(|output| {
                output.system(&message);
                output.end(RunState::Finished, code);
            });
            return;
        }
        let done = match (&self.part, self.kind) {
            (Some(part), Kind::Backup) => self.place(part),
            _ => Ok(t!("database-restore-done", database = self.database.as_str())),
        };
        match done {
            Ok(message) => {
                lock(&self.progress).ended.get_or_insert_with(Instant::now);
                self.output.update(|output| {
                    output.system(&message);
                    output.end(RunState::Finished, Some(0));
                });
            }
            Err(error) => self.fail(error),
        }
    }

    fn place(&self, part: &Path) -> Result<String, String> {
        let moved = (|| {
            if self.overwrite && self.file.is_dir() && part.is_dir() {
                std::fs::remove_dir_all(&self.file)?;
            }
            std::fs::rename(part, &self.file)
        })();
        if let Err(error) = moved {
            return Err(t!(
                "database-backup-move-failed",
                path = crate::display(&self.file),
                error = error.to_string()
            ));
        }
        let bytes = size_of(&self.file);
        lock(&self.progress).bytes = Some(bytes);
        Ok(t!("database-backup-written", path = crate::display(&self.file), size = super::objects::size_text(bytes)))
    }

    fn state(&self, run_id: u64) -> JobState {
        let progress = lock(&self.progress);
        let (run_state, exit_code) = self.output.read(|output| (output.state(), output.exit_code()));
        let status = match run_state {
            _ if progress.ended.is_none() && !matches!(run_state, RunState::Stopped) => "running",
            RunState::Stopped => "stopped",
            _ if progress.error.is_some() => "failed",
            _ => "finished",
        };
        let ended = progress.ended.unwrap_or_else(Instant::now);
        JobState {
            run_id,
            kind: self.kind.name().to_owned(),
            connection: self.connection.clone(),
            database: self.database.clone(),
            file: self.file.clone(),
            tool: progress.tool.clone(),
            status: status.to_owned(),
            exit_code,
            error: progress.error.clone(),
            elapsed_ms: u64::try_from(ended.duration_since(self.started).as_millis()).unwrap_or(u64::MAX),
            bytes: progress.bytes,
        }
    }
}

impl RunControl for Job {
    fn stop(&self) -> bool {
        if lock(&self.progress).ended.is_some() {
            return false;
        }
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(process) = &lock(&self.progress).process {
            process.stop();
        }
        true
    }

    fn kill(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(process) = &lock(&self.progress).process {
            process.kill();
        }
        if lock(&self.progress).ended.is_none() {
            self.remove_part();
        }
        self.forget_secret();
    }
}

fn size_of(path: &Path) -> u64 {
    if path.is_dir() {
        std::fs::read_dir(path).into_iter().flatten().flatten().map(|entry| size_of(&entry.path())).sum()
    } else {
        std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
    }
}

#[derive(Clone)]
struct Password {
    given: Option<Secret>,
    stored: Option<(Arc<dyn SecretStore>, SecretId)>,
    needed: bool,
}

impl Password {
    fn read(self) -> Result<Option<Secret>, String> {
        if let Some(given) = self.given {
            return Ok(Some(given));
        }
        match self.stored {
            Some((store, id)) => match store.get(&id) {
                Ok(Some(secret)) => Ok(Some(secret)),
                Ok(None) if self.needed => Err(t!("database-error-password-needed")),
                Ok(None) => Ok(None),
                Err(SecretError::Denied) => Err(t!("database-error-secret-denied")),
                Err(error) => Err(t!("database-error-secret-failed", reason = error.to_string())),
            },
            None => Ok(None),
        }
    }
}

fn password(session: &Session, entry: &Entry, given: Option<String>) -> Result<Password, CommandError> {
    let needed = entry.source.auth == AuthKind::Password;
    let given = given.filter(|password| !password.is_empty()).map(Secret::new);
    let stored = (given.is_none() && has_password(entry))
        .then(|| -> Result<_, CommandError> {
            Ok((secret_store(session)?, secret_id(session, entry, SecretKind::Password)?))
        })
        .transpose()?;
    if needed && given.is_none() && stored.is_none() {
        return Err(CommandError::with_code(
            "passwordNeeded",
            t!("database-backup-password-needed", name = entry.source.name.as_str()),
        ));
    }
    Ok(Password { given, stored, needed })
}

struct Plan {
    kind: Kind,
    family: Family,
    target: Target,
    choice: ToolChoice,
    pg_dump: PgDumpOptions,
    mysqldump: MysqldumpOptions,
    pg_restore: PgRestoreOptions,
    mysql: MysqlRestoreOptions,
    password: Password,
    slot: SharedSlot,
    probe: Probe,
    detect: bool,
}

fn tool_for(plan: &Plan, job: &Job) -> Result<(Found, PgFormat), String> {
    let detection = if plan.detect {
        job.output.update(|output| output.system(&t!("database-backup-searching")));
        detect_into(&plan.slot, &plan.probe, plan.family, &plan.target)
    } else {
        wait_for(&plan.slot, Duration::from_secs(60)).unwrap_or_default()
    };
    let (tool, format) = match plan.kind {
        Kind::Backup => (plan.family.backup_tool(), plan.pg_dump.format),
        Kind::Restore => {
            let format = match plan.family {
                Family::Postgres => zen_db::backup::pg_format(&job.file).map_err(|error| {
                    t!("database-restore-unreadable", path = crate::display(&job.file), error = error.to_string())
                })?,
                Family::Mysql | Family::Mariadb => PgFormat::Plain,
            };
            (zen_db::backup::restore_tool(plan.family, format), format)
        }
    };
    let chosen = plan.choice.pick(&detection, tool).cloned().ok_or_else(|| missing(tool, &detection))?;
    if plan.family != Family::Postgres {
        return Ok((chosen, format));
    }
    let server = server_major(plan, &detection, job);
    let explicit = plan.choice.origin.is_some() || plan.choice.program.is_some();
    let candidates: Vec<&Found> =
        if explicit { vec![&chosen] } else { detection.tools.iter().filter(|found| found.tool == tool).collect() };
    match zen_db::backup::for_server(&candidates, Some(&chosen), server) {
        Ok(found) => Ok((found.cloned().unwrap_or(chosen), format)),
        Err(VersionProblem::TooNew { tool: major, server }) => {
            Err(t!("database-backup-tool-too-new", tool = tool.name(), major = major, server = server))
        }
        Err(VersionProblem::TooOld { tool: major, server }) => {
            Err(t!("database-backup-tool-too-old", tool = tool.name(), major = major, server = server))
        }
    }
}

fn server_major(plan: &Plan, detection: &Detection, job: &Job) -> Option<u32> {
    let psql = zen_db::backup::choose(detection, Tool::Psql)?;
    let mut asked = zen_db::backup::server_version(psql, &plan.target).ok()?;
    let secret = Password { given: plan.password.given.clone(), stored: plan.password.stored.clone(), needed: false }
        .read()
        .ok()
        .flatten();
    let mut file = None;
    if let Some(secret) = &secret {
        let text = asked.password_file(secret.expose())?;
        let path = super::private_file("zen-db-version", &text).ok()?;
        if asked.container.is_some() {
            let _ = std::fs::remove_file(&path);
        } else {
            asked.with_password_file(&path.display().to_string());
            file = Some(path);
        }
    }
    job.output.update(|output| output.system(&t!("database-backup-asking-version")));
    let mut command = std::process::Command::new(&asked.program);
    command
        .args(&asked.args)
        .envs(asked.env.iter().map(|(name, value)| (name, value)))
        .env_remove("PGPASSWORD")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let answer = command.output();
    if let Some(file) = file {
        let _ = std::fs::remove_file(file);
    }
    zen_db::backup::server_major(&String::from_utf8_lossy(&answer.ok()?.stdout))
}

fn missing(tool: Tool, detection: &Detection) -> String {
    let docker = match &detection.docker {
        DockerState::Container { container } => {
            t!("database-backup-docker-without", container = container.name.as_str())
        }
        DockerState::NoContainer => t!("database-backup-docker-none"),
        DockerState::NotRunning { .. } => t!("database-backup-docker-not-running"),
        DockerState::NotFound | DockerState::NotLocal => String::new(),
    };
    let install = match tool {
        Tool::PgDump | Tool::PgRestore | Tool::Psql => "brew install libpq",
        Tool::Mysqldump | Tool::Mysql => "brew install mysql-client",
    };
    t!("database-backup-no-tool", tool = tool.name(), docker = docker, install = install)
}

fn describe_tool(found: &Found) -> String {
    let version = found.version.clone().unwrap_or_else(|| found.tool.name().to_owned());
    match (&found.container, found.origin) {
        (Some(container), _) => {
            t!("database-backup-tool-docker", version = version, container = container.name.as_str())
        }
        (None, Origin::Homebrew) => {
            t!("database-backup-tool-homebrew", version = version, path = crate::display(&found.program))
        }
        (None, _) => t!("database-backup-tool-path", version = version, path = crate::display(&found.program)),
    }
}

fn run_job(job: &Arc<Job>, plan: Plan) {
    let (found, format) = match tool_for(&plan, job) {
        Ok(found) => found,
        Err(message) => return job.fail(message),
    };
    lock(&job.progress).tool = Some(found.clone());
    let written = job.part.as_deref().unwrap_or(&job.file);
    let built = match plan.kind {
        Kind::Backup => zen_db::backup::backup(&found, &plan.target, &plan.pg_dump, &plan.mysqldump, written),
        Kind::Restore => {
            zen_db::backup::restore(&found, &plan.target, format, &plan.pg_restore, &plan.mysql, &job.file)
        }
    };
    let mut invocation = match built {
        Ok(invocation) => invocation,
        Err(error) if error.code == zen_db::backup::NOT_IN_CONTAINER => {
            return job.fail(t!("database-backup-not-in-container", tool = found.tool.name()));
        }
        Err(error) => return job.fail(error.detail),
    };
    let secret = match plan.password.read() {
        Ok(secret) => secret,
        Err(message) => return job.fail(message),
    };
    if let Some(secret) = &secret
        && let Err(message) = hand_over(job, &mut invocation, secret)
    {
        return job.fail(message);
    }
    let display = invocation.display();
    job.output.update(|output| {
        output.system(&describe_tool(&found));
        output.system(&display);
    });
    let (program, args, env) = invocation.shell();
    let cwd = job.file.parent().map(Path::to_path_buf).unwrap_or_default();
    let line = CommandLine { program, args, cwd, env, display };
    let mut progress = lock(&job.progress);
    if job.stopped.load(Ordering::SeqCst) {
        drop(progress);
        return job.exited(None);
    }
    let sink_job = Arc::clone(job);
    let sink: ProcessSink = Arc::new(move |event| sink_job.event(event));
    match Process::spawn(&line, sink) {
        Ok(process) => progress.process = Some(process),
        Err(error) => {
            drop(progress);
            job.fail(error);
        }
    }
}

fn hand_over(job: &Job, invocation: &mut zen_db::backup::Invocation, secret: &Secret) -> Result<(), String> {
    let text = invocation.password_file(secret.expose()).ok_or_else(|| t!("database-backup-password-unwritable"))?;
    let path = super::private_file("zen-db-backup", &text)
        .map_err(|error| t!("database-client-secret-file", error = error.to_string()))?;
    let mut kept = SecretFile { path: path.clone(), copied: None };
    let pointed = match (&invocation.container, &invocation.program) {
        (Some(container), docker) => {
            let inside = format!("/tmp/{}", path.file_name().map(|name| name.to_string_lossy()).unwrap_or_default());
            let copied = std::process::Command::new(docker)
                .args(["cp", "--", &path.display().to_string(), &format!("{container}:{inside}")])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
            kept.copied = Some((docker.clone(), container.clone(), inside.clone()));
            *lock(&job.secret) = Some(kept);
            if !copied.is_ok_and(|status| status.success()) {
                return Err(t!("database-backup-password-copy"));
            }
            inside
        }
        (None, _) => {
            *lock(&job.secret) = Some(kept);
            path.display().to_string()
        }
    };
    invocation.with_password_file(&pointed);
    Ok(())
}

fn part_path(file: &Path) -> PathBuf {
    let name = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    file.with_file_name(format!(".{name}.{}.part", std::process::id()))
}

fn start_run(
    session: &mut Session,
    job: Arc<Job>,
    plan: Plan,
    name: String,
    params: serde_json::Value,
) -> Result<u64, CommandError> {
    let configuration = Configuration {
        id: String::new(),
        name,
        kind: RUN_KIND.to_owned(),
        temporary: true,
        options: serde_json::json!({
            "kind": job.kind.name(),
            "connection": job.connection,
            "database": job.database,
            "file": job.file,
            "params": params,
        }),
        ..Configuration::default()
    };
    let line = CommandLine {
        program: PathBuf::new(),
        args: Vec::new(),
        cwd: job.file.parent().map(Path::to_path_buf).unwrap_or_default(),
        env: Vec::new(),
        display: String::new(),
    };
    let control: Arc<dyn RunControl> = Arc::clone(&job) as Arc<dyn RunControl>;
    let run_id = session.state_mut::<Runs>().insert_task(
        configuration,
        RUN_KIND.to_owned(),
        vec![line],
        job.output.clone(),
        control,
    );
    session.state_mut::<Backups>().jobs.push(Listed { run_id, job: Arc::clone(&job) });
    let worker = Arc::clone(&job);
    std::thread::Builder::new().name("zen-db-backup".to_owned()).spawn(move || run_job(&worker, plan)).map_err(
        |error| {
            job.fail(error.to_string());
            CommandError::new(error.to_string())
        },
    )?;
    Ok(run_id)
}

fn new_output(session: &Session) -> SharedOutput {
    let waker = session.state::<Runs>().and_then(|runs| runs.waker().cloned());
    SharedOutput::new(RunOutput::new(Box::new(zen_run::PlainParser)), waker)
}

fn prepared_tool(slot: &SharedSlot, choice: &ToolChoice, tool: Tool) -> Option<Found> {
    lock(slot).detection.as_ref().and_then(|detection| choice.pick(detection, tool).cloned())
}

pub struct StartBackup;

impl Command for StartBackup {
    type Params = BackupParams;
    type Output = Started;
    const ID: &'static str = "db.backup.start";
    const TITLE: &'static str = "Backup Database";
    const DESCRIPTION: &'static str = "Backs up database `database` (the connection's when absent) of PostgreSQL, MySQL or MariaDB connection `connection` with pg_dump or mysqldump, as a run of the Run tool window: in the background, cancellable (db.backup.cancel or run.process.stop), its output in the console. The tool is the one db.backup.tools chose, or `tool { origin, program }`; one inside the database's Docker container runs with docker exec. `pgDump` options: format (plain, custom, directory, tar), compression, content (all, schemaOnly, dataOnly), schemas, tables, excludeTables, inserts, clean, ifExists, create, noOwner, noPrivileges, encoding, jobs, verbose, extraArgs. `mysqldump` options: content, tables, method (singleTransaction, lockTables, none), addDropTable, routines, events, triggers, extendedInsert, completeInsert, hexBlob, comments, verbose, extraArgs. The password (a saved one, or `password` for this run) goes in a new file only the user can read that the tool is pointed at (PGPASSFILE, --defaults-extra-file; copied into the container and removed there for docker exec), never in its environment or arguments; the file goes when the run ends. Agents may not pass extraArgs and never change the remembered settings, and a file hidden from agents is refused. Writes `file`, or a file named after the database and the time in `folder`, under a hidden name moved into place when complete; an existing file is refused with code `exists` unless `overwrite`. Answers the run's id at once; follow it with db.backup.state.";

    fn run(&self, session: &mut Session, params: BackupParams) -> Result<Started, CommandError> {
        let resolved = resolve(session, &params.connection, params.database.as_deref(), true)?;
        if zen_core::macros::by_agent(session)
            && agent_access(session, &resolved.entry) == zen_db::types::AgentAccess::None
        {
            let kind = super::safety::type_of(session, &resolved.entry);
            return Err(CommandError::with_code(
                AGENT_NOT_ALLOWED,
                t!(
                    "database-error-agent-not-allowed",
                    name = resolved.entry.source.name.as_str(),
                    kind = kind.name.as_str()
                ),
            ));
        }
        let known = remembered(session);
        let pg_dump = params.pg_dump.clone().unwrap_or_else(|| known.pg_dump.clone());
        let mysqldump = params.mysqldump.clone().unwrap_or_else(|| known.mysqldump.clone());
        let folder = params.folder.clone().or_else(|| known.folder.clone()).unwrap_or_else(home);
        let file = match &params.file {
            Some(file) => file.clone(),
            None => {
                let (_, stamp) = zen_db::engine::now_stamps();
                folder.join(zen_db::backup::file_name(
                    &resolved.target.database,
                    &stamp,
                    resolved.family,
                    pg_dump.format,
                ))
            }
        };
        if !file.is_absolute() {
            return Err(CommandError::with_code(
                "relativePath",
                t!("database-backup-relative", path = crate::display(&file)),
            ));
        }
        let parent = file.parent().map(Path::to_path_buf).unwrap_or_default();
        if !parent.is_dir() {
            return Err(CommandError::with_code(
                "noFolder",
                t!("database-backup-no-folder", path = crate::display(&parent)),
            ));
        }
        if file.exists() && !params.overwrite {
            return Err(CommandError::with_code("exists", t!("database-backup-exists", path = crate::display(&file)))
                .with_data(serde_json::json!({ "paths": [file] })));
        }
        super::guard_agent_file(session, &file)?;
        super::guard_agent_file(session, &part_path(&file))?;
        let remembers = agent_arguments(
            session,
            &[
                params.pg_dump.as_ref().map_or(&[][..], |pg| &pg.extra_args[..]),
                params.mysqldump.as_ref().map_or(&[][..], |my| &my.extra_args[..]),
            ],
            params.remember,
        )?;
        let password = password(session, &resolved.entry, params.password.clone())?;
        let (slot, detect) = slot(session, &params.connection, &resolved.target, false);
        let tool = prepared_tool(&slot, &params.tool, resolved.family.backup_tool());
        if remembers {
            let mut kept = known;
            kept.version = 1;
            kept.folder = Some(parent.clone());
            if params.pg_dump.is_some() {
                kept.pg_dump = pg_dump.clone();
            }
            if params.mysqldump.is_some() {
                kept.mysqldump = mysqldump.clone();
            }
            remember(session, kept)?;
        }
        let job = Arc::new(Job {
            kind: Kind::Backup,
            connection: params.connection.clone(),
            database: resolved.target.database.clone(),
            part: Some(part_path(&file)),
            file: file.clone(),
            overwrite: params.overwrite,
            output: new_output(session),
            started: Instant::now(),
            stopped: AtomicBool::new(false),
            progress: Mutex::new(Progress::default()),
            secret: Mutex::new(None),
        });
        let plan = Plan {
            kind: Kind::Backup,
            family: resolved.family,
            target: resolved.target.clone(),
            choice: params.tool.clone(),
            pg_dump,
            mysqldump,
            pg_restore: PgRestoreOptions::default(),
            mysql: MysqlRestoreOptions::default(),
            password,
            slot,
            probe: probe(session),
            detect,
        };
        let name = t!("database-backup-run-name", database = resolved.target.database.as_str());
        let mut kept = params.clone();
        kept.file = Some(file.clone());
        let kept = serde_json::to_value(&kept).unwrap_or_default();
        let run_id = start_run(session, job, plan, name, kept)?;
        Ok(Started { run_id, kind: "backup".to_owned(), database: resolved.target.database, file, tool })
    }
}

fn restore_token(connection: &str, params: &RestoreParams, database: &str) -> String {
    let mut hasher = DefaultHasher::new();
    connection.hash(&mut hasher);
    database.hash(&mut hasher);
    params.file.hash(&mut hasher);
    serde_json::to_string(&(&params.pg_restore, &params.mysql)).unwrap_or_default().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub struct StartRestore;

impl Command for StartRestore {
    type Params = RestoreParams;
    type Output = Started;
    const ID: &'static str = "db.backup.restore";
    const TITLE: &'static str = "Restore Database";
    const DESCRIPTION: &'static str = "Restores backup `file` into database `database` (the connection's when absent) of PostgreSQL, MySQL or MariaDB connection `connection`, as a run of the Run tool window: in the background, cancellable (db.backup.cancel or run.process.stop). PostgreSQL's custom, tar and directory backups go through pg_restore, a plain SQL backup through psql (the format is read from the file); MySQL and MariaDB backups through mysql reading the file. The tool is the one db.backup.tools chose, or `tool { origin, program }`; inside the database's Docker container it runs with docker exec, the file on its standard input. `pgRestore` options: content (all, schemaOnly, dataOnly), clean, ifExists, create (restored into a new database; connects to `postgres`), noOwner, noPrivileges, singleTransaction, exitOnError, jobs, verbose, extraArgs (psql takes singleTransaction, exitOnError and extraArgs); `mysql` options: force, extraArgs. A restore changes the database, so it always asks first: refused with code `confirmExecute` and a `token` to pass back as `confirmed`; read-only connections refuse (`readOnly`), agents need full access to the connection's type. The password goes in a new file only the user can read (PGPASSFILE, --defaults-extra-file), removed when the run ends. Agents may not pass extraArgs, and a file hidden from agents is refused. Answers the run's id at once; follow it with db.backup.state.";

    fn run(&self, session: &mut Session, params: RestoreParams) -> Result<Started, CommandError> {
        let resolved = resolve(session, &params.connection, params.database.as_deref(), true)?;
        let entry = &resolved.entry;
        let name = entry.source.name.as_str();
        if !params.file.is_absolute() {
            return Err(CommandError::with_code(
                "relativePath",
                t!("database-backup-relative", path = crate::display(&params.file)),
            ));
        }
        if !params.file.exists() {
            return Err(CommandError::with_code(
                "noFile",
                t!("database-restore-no-file", path = crate::display(&params.file)),
            ));
        }
        if read_only(session, entry) {
            return Err(CommandError::with_code(READ_ONLY, t!("database-restore-read-only", name = name)));
        }
        agent_changes(session, entry)?;
        super::guard_agent_file(session, &params.file)?;
        let remembers = agent_arguments(
            session,
            &[
                params.pg_restore.as_ref().map_or(&[][..], |pg| &pg.extra_args[..]),
                params.mysql.as_ref().map_or(&[][..], |my| &my.extra_args[..]),
            ],
            params.remember,
        )?;
        let token = restore_token(&params.connection, &params, &resolved.target.database);
        if params.confirmed.as_deref() != Some(token.as_str()) {
            let mut retry = serde_json::to_value(&params).unwrap_or_default();
            if let Some(object) = retry.as_object_mut() {
                object.insert("confirmed".to_owned(), serde_json::json!(token));
            }
            let message = t!(
                "database-restore-confirm",
                path = crate::display(&params.file),
                database = resolved.target.database.as_str(),
                name = name
            );
            return Err(CommandError::with_code(CONFIRM_EXECUTE, message).with_data(serde_json::json!({
                "token": token,
                "target": target(session, entry),
                "database": resolved.target.database,
                "file": params.file,
                "retry": { "command": Self::ID, "params": retry },
            })));
        }
        let known = remembered(session);
        let pg_restore = params.pg_restore.clone().unwrap_or_else(|| known.pg_restore.clone());
        let mysql = params.mysql.clone().unwrap_or_else(|| known.mysql.clone());
        let password = password(session, entry, params.password.clone())?;
        let (slot, detect) = slot(session, &params.connection, &resolved.target, false);
        if remembers && (params.pg_restore.is_some() || params.mysql.is_some()) {
            let mut kept = known;
            kept.version = 1;
            kept.pg_restore = pg_restore.clone();
            kept.mysql = mysql.clone();
            remember(session, kept)?;
        }
        let job = Arc::new(Job {
            kind: Kind::Restore,
            connection: params.connection.clone(),
            database: resolved.target.database.clone(),
            part: None,
            file: params.file.clone(),
            overwrite: false,
            output: new_output(session),
            started: Instant::now(),
            stopped: AtomicBool::new(false),
            progress: Mutex::new(Progress::default()),
            secret: Mutex::new(None),
        });
        let plan = Plan {
            kind: Kind::Restore,
            family: resolved.family,
            target: resolved.target.clone(),
            choice: params.tool.clone(),
            pg_dump: PgDumpOptions::default(),
            mysqldump: MysqldumpOptions::default(),
            pg_restore,
            mysql,
            password,
            slot,
            probe: probe(session),
            detect,
        };
        let run_name = t!("database-restore-run-name", database = resolved.target.database.as_str());
        let kept = serde_json::to_value(&params).unwrap_or_default();
        let run_id = start_run(session, job, plan, run_name, kept)?;
        Ok(Started {
            run_id,
            kind: "restore".to_owned(),
            database: resolved.target.database,
            file: params.file,
            tool: None,
        })
    }
}

pub struct BackupState;

impl Command for BackupState {
    type Params = StateParams;
    type Output = StateAnswer;
    const ID: &'static str = "db.backup.state";
    const TITLE: &'static str = "Backup State";
    const DESCRIPTION: &'static str = "The backups and restores of the window, the newest first (at most `limit`, 20 by default, with `truncated`), or run `runId` only (`waitMs` waits up to 60 000 ms for it to end): kind, connection, database, file, the tool used, status (running, finished, failed, stopped), exit code, error, elapsed time and the size of a finished backup. The console's lines are read with run.output.get; never secrets.";

    fn run(&self, session: &mut Session, params: StateParams) -> Result<StateAnswer, CommandError> {
        let jobs: Vec<Listed> =
            session.state::<Backups>().map(|backups| backups.jobs.iter().rev().cloned().collect()).unwrap_or_default();
        let jobs: Vec<Listed> = match params.run_id {
            Some(run_id) => {
                let job = jobs.into_iter().filter(|listed| listed.run_id == run_id).collect::<Vec<_>>();
                if job.is_empty() {
                    return Err(CommandError::with_code("unknownRun", t!("database-backup-unknown-run", id = run_id)));
                }
                let wait = Duration::from_millis(params.wait_ms.unwrap_or(0).min(WAIT_LIMIT));
                let deadline = Instant::now() + wait;
                while Instant::now() < deadline && job[0].job.state(run_id).status == "running" {
                    std::thread::sleep(Duration::from_millis(20));
                }
                job
            }
            None => jobs,
        };
        let limit = params.limit.unwrap_or(LISTED);
        let truncated = jobs.len() > limit;
        Ok(StateAnswer {
            jobs: jobs.iter().take(limit).map(|listed| listed.job.state(listed.run_id)).collect(),
            truncated,
        })
    }
}

pub struct CancelBackup;

impl Command for CancelBackup {
    type Params = CancelParams;
    type Output = Cancelled;
    const ID: &'static str = "db.backup.cancel";
    const TITLE: &'static str = "Cancel Backup";
    const DESCRIPTION: &'static str = "Stops backup or restore run `runId` (as run.process.stop does): the program is asked to stop, then ended; a backup's unfinished file is removed. A restore stopped half way leaves what it had restored, unless it ran in one transaction.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<Cancelled, CommandError> {
        let job = session
            .state::<Backups>()
            .and_then(|backups| backups.jobs.iter().find(|listed| listed.run_id == params.run_id).cloned())
            .ok_or_else(|| {
                CommandError::with_code("unknownRun", t!("database-backup-unknown-run", id = params.run_id))
            })?;
        Ok(Cancelled { cancelled: RunControl::stop(job.job.as_ref()) })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asked {
    pub restore: bool,
    pub params: serde_json::Value,
    pub code: String,
    pub message: String,
    pub token: Option<String>,
}

pub fn take_asked(session: &mut Session) -> Option<Asked> {
    session.state_mut::<Backups>().asked.take()
}

pub fn has_asked(session: &Session) -> bool {
    session.state::<Backups>().is_some_and(|backups| backups.asked.is_some())
}

pub fn rerun(session: &mut Session, run_id: u64, configuration: &Configuration) -> Result<u64, CommandError> {
    let restore = configuration.options["kind"] == "restore";
    let mut params = configuration.options["params"].clone();
    let used = session
        .state::<Backups>()
        .and_then(|backups| backups.jobs.iter().find(|listed| listed.run_id == run_id).cloned())
        .and_then(|listed| lock(&listed.job.progress).tool.clone());
    if let (Some(tool), Some(object)) = (used, params.as_object_mut()) {
        object.insert("tool".to_owned(), serde_json::json!({ "origin": tool.origin, "program": tool.program }));
    }
    let parsed = if restore {
        let restore_params: RestoreParams =
            serde_json::from_value(params.clone()).map_err(|error| CommandError::new(error.to_string()))?;
        StartRestore.run(session, restore_params)
    } else {
        if let Some(object) = params.as_object_mut() {
            object.insert("overwrite".to_owned(), serde_json::json!(true));
        }
        let backup_params: BackupParams =
            serde_json::from_value(params.clone()).map_err(|error| CommandError::new(error.to_string()))?;
        StartBackup.run(session, backup_params)
    };
    match parsed {
        Ok(started) => Ok(started.run_id),
        Err(error) => {
            let code = error.code.clone().unwrap_or_default();
            if code == CONFIRM_EXECUTE || code == "passwordNeeded" {
                let token = error.data.as_ref().and_then(|data| data.get("token")).and_then(|token| token.as_str());
                session.state_mut::<Backups>().asked = Some(Asked {
                    restore,
                    params,
                    code,
                    message: error.message.clone(),
                    token: token.map(str::to_owned),
                });
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests;

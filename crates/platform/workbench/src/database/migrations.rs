use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::dialect::SqlDialect;
use zen_db::driver::{ContextKind, ExecOptions};
use zen_db::engine::{ConnectionState, OperationStatus};
use zen_db::migrations::command::{Direction, Invocation, Refusal, Target, invocation};
use zen_db::migrations::{self, History, MigrationSet, State, Status, Tool};
use zen_db::registry::{AuthKind, Entry, Files, parse_uri};
use zen_db::types::AgentAccess;
use zen_i18n::t;
use zen_run::{
    CommandLine, Configuration, FieldKind, FieldSpec, KindSpec, OutputParser, PlainParser, RunContext, RunProvider,
    RunState, RunTarget,
};
use zen_secrets::Secret;
use zen_secrets::database::Kind as SecretKind;

use super::safety::{AGENT_NOT_ALLOWED, AGENT_READ_ONLY, CONFIRM_EXECUTE, READ_ONLY, target, type_of};
use super::sources::{Database, find, has_password, secret_id, secret_store};
use super::{WORKSPACE, project_key};
use crate::Workbench;
use crate::run::Runs;
use crate::workspaces::{InnerTab, OpenTab, OpenTabParams};

pub const TAB: &str = "migrations";

pub const KIND: &str = "db.migration";

pub const UNKNOWN_SET: &str = "unknownMigrations";

pub const UNSUPPORTED: &str = "unsupported";

pub const NEEDS_STATE: &str = "needsState";

const LISTED: usize = 50;
const MIGRATIONS_LISTED: usize = 200;
const DEPTH: usize = 7;
const ENTRIES: usize = 30_000;
const HISTORY_ROWS: usize = 100_000;
const HISTORY_TIMEOUT: Duration = Duration::from_secs(30);
const SKIPPED: [&str; 7] = ["node_modules", "target", ".venv", "venv", "vendor", "build", "dist"];

type Slot<T> = Arc<Mutex<Option<T>>>;

struct Scan {
    cancel: Arc<AtomicBool>,
    result: Slot<Vec<MigrationSet>>,
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct Read {
    operation: Option<u64>,
    history: Option<History>,
    error: Option<String>,
}

struct Waiting {
    key: String,
    connection: String,
    set: String,
    direction: Direction,
    configuration: Configuration,
    slot: Slot<Result<Option<Secret>, String>>,
}

struct Tracked {
    run_id: u64,
    set: String,
    connection: String,
    direction: Direction,
    ended: bool,
    refresh: Option<u64>,
}

#[derive(Default)]
pub struct Migrations {
    root: Option<PathBuf>,
    sets: Vec<MigrationSet>,
    scanned: bool,
    stale: bool,
    scan: Option<Scan>,
    reads: HashMap<(String, String), Read>,
    waiting: Vec<Waiting>,
    tracked: Vec<Tracked>,
    errors: HashMap<(String, String), String>,
    revision: u64,
}

impl Migrations {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    fn set(&self, id: &str) -> Option<&MigrationSet> {
        self.sets.iter().find(|set| set.id == id)
    }
}

fn credentials() -> &'static Mutex<HashMap<String, Secret>> {
    static CREDENTIALS: OnceLock<Mutex<HashMap<String, Secret>>> = OnceLock::new();
    CREDENTIALS.get_or_init(Mutex::default)
}

fn credential_key(root: &Path, connection: &str) -> String {
    format!("{}/{connection}", project_key(root))
}

fn remember(key: &str, secret: Secret) {
    credentials().lock().unwrap_or_else(PoisonError::into_inner).insert(key.to_owned(), secret);
}

fn remembered(key: &str) -> Option<Secret> {
    credentials().lock().unwrap_or_else(PoisonError::into_inner).get(key).cloned()
}

fn touched(path: &Path) -> bool {
    let text = path.to_string_lossy().to_ascii_lowercase();
    let name = path.file_name().map(|name| name.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let configs = [
        "flyway.conf",
        "flyway.toml",
        "liquibase.properties",
        "atlas.hcl",
        "atlas.sum",
        "diesel.toml",
        "alembic.ini",
        "manage.py",
        "schema.prisma",
        "cargo.toml",
        "go.mod",
    ];
    configs.contains(&name.as_str())
        || name.starts_with("knexfile")
        || ["migrat", "changelog", "versions", "db/migrate"].iter().any(|part| text.contains(part))
}

pub fn files_changed(session: &mut Session, paths: &[PathBuf]) {
    if paths.iter().any(|path| touched(path)) {
        session.state_mut::<Migrations>().stale = true;
    }
}

fn skipped(name: &str) -> bool {
    zen_vfs::HIDDEN.contains(&name) || zen_vfs::TOOL_FOLDERS.contains(&name) || SKIPPED.contains(&name)
}

pub fn scan(
    root: &Path,
    exclusions: &zen_index::Exclusions,
    index: Option<&zen_index::FileIndex>,
    cancel: &AtomicBool,
) -> Vec<MigrationSet> {
    let mut found: Vec<MigrationSet> = Vec::new();
    let mut queue = VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut seen = 0usize;
    while let Some((folder, depth)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return Vec::new();
        }
        found.extend(migrations::recognise(root, &folder));
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        let mut folders: Vec<PathBuf> = Vec::new();
        for entry in entries.flatten() {
            seen += 1;
            if seen > ENTRIES {
                return dedupe(found);
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let walk = depth < DEPTH
                && entry.file_type().is_ok_and(|kind| kind.is_dir())
                && !skipped(&name)
                && !exclusions.excludes_entry(&path)
                && !zen_vfs::excluded_folders::is_excluded(&path)
                && !index.is_some_and(|index| index.git_ignores(&path));
            if walk {
                folders.push(path);
            }
        }
        folders.sort();
        queue.extend(folders.into_iter().map(|path| (path, depth + 1)));
    }
    dedupe(found)
}

fn dedupe(found: Vec<MigrationSet>) -> Vec<MigrationSet> {
    let mut ids = HashSet::new();
    let mut sets: Vec<MigrationSet> = found.into_iter().filter(|set| ids.insert(set.id.clone())).collect();
    sets.sort_by(|a, b| a.folder.cmp(&b.folder).then_with(|| a.tool.cmp(&b.tool)));
    sets
}

fn connected(session: &Session, connection: &str) -> bool {
    session.state::<Database>().and_then(Database::started).is_some_and(|engine| {
        matches!(engine.status(connection).state, ConnectionState::Connected | ConnectionState::Closed)
    })
}

fn dialect(driver: &str) -> SqlDialect {
    match driver {
        "mysql" => SqlDialect::MySql,
        "mariadb" => SqlDialect::MariaDb,
        "sqlite" => SqlDialect::Sqlite,
        "clickhouse" => SqlDialect::ClickHouse,
        _ => SqlDialect::Postgres,
    }
}

fn take_reads(session: &mut Session) -> bool {
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else { return false };
    let mut changed = false;
    let migrations = session.state_mut::<Migrations>();
    let sets: HashMap<String, (Tool, Option<String>)> =
        migrations.sets.iter().map(|set| (set.id.clone(), (set.tool, set.app.clone()))).collect();
    for ((set, _), read) in &mut migrations.reads {
        let Some(operation) = read.operation else { continue };
        let Some(state) = engine.operation(operation) else {
            read.operation = None;
            changed = true;
            continue;
        };
        if state.status == OperationStatus::Running {
            continue;
        }
        read.operation = None;
        changed = true;
        let Some((tool, app)) = sets.get(set) else { continue };
        match (state.status, state.execution, state.error) {
            (OperationStatus::Done, Some(execution), _) => {
                let rows: Vec<Vec<Option<String>>> =
                    execution.rows.iter().map(|row| row.iter().map(zen_db::value::Value::display).collect()).collect();
                read.history = Some(migrations::read_history(*tool, app.as_deref(), &rows));
                read.error = None;
            }
            (_, _, Some(error))
                if migrations::missing_table(&format!(
                    "{} {}",
                    error.detail,
                    error.code.clone().unwrap_or_default()
                )) =>
            {
                read.history = Some(History { table: false, entries: Vec::new() });
                read.error = None;
            }
            (_, _, Some(error)) => read.error = Some(super::connections::describe(&error)),
            _ => read.error = Some(t!("database-migration-read-failed")),
        }
    }
    changed
}

fn start_read(session: &mut Session, set: &MigrationSet, entry: &Entry) -> Result<(), CommandError> {
    let key = (set.id.clone(), entry.source.id.clone());
    if session
        .state::<Migrations>()
        .and_then(|migrations| migrations.reads.get(&key))
        .is_some_and(|read| read.operation.is_some())
    {
        return Ok(());
    }
    let sql = migrations::history_query(set.tool, dialect(&entry.source.driver));
    super::safety::agent_reads(session, entry, &sql)?;
    let engine = super::consoles::engine(session);
    let options = ExecOptions { max_rows: HISTORY_ROWS, timeout: Some(HISTORY_TIMEOUT) };
    let operation = engine
        .execute(&entry.source.id, ContextKind::Metadata, &sql, options)
        .map_err(|error| super::connections::failed(&error))?;
    let migrations = session.state_mut::<Migrations>();
    migrations.reads.entry(key).or_default().operation = Some(operation);
    migrations.revision += 1;
    Ok(())
}

fn take_secrets(session: &mut Session) {
    let waiting = std::mem::take(&mut session.state_mut::<Migrations>().waiting);
    let mut left = Vec::new();
    for item in waiting {
        let done = item.slot.lock().unwrap_or_else(PoisonError::into_inner).take();
        let Some(done) = done else {
            left.push(item);
            continue;
        };
        let key = (item.set.clone(), item.connection.clone());
        match done {
            Ok(secret) => {
                if let Some(secret) = secret {
                    remember(&item.key, secret);
                }
                match crate::run::start(session, item.configuration) {
                    Ok(started) => track(session, started.run_id, &item.set, &item.connection, item.direction),
                    Err(error) => {
                        session.state_mut::<Migrations>().errors.insert(key, error.to_string());
                    }
                }
            }
            Err(error) => {
                session.state_mut::<Migrations>().errors.insert(key, error);
            }
        }
        session.state_mut::<Migrations>().revision += 1;
    }
    session.state_mut::<Migrations>().waiting.extend(left);
}

fn track(session: &mut Session, run_id: u64, set: &str, connection: &str, direction: Direction) {
    let migrations = session.state_mut::<Migrations>();
    if migrations.tracked.iter().any(|tracked| tracked.run_id == run_id) {
        return;
    }
    migrations.tracked.push(Tracked {
        run_id,
        set: set.to_owned(),
        connection: connection.to_owned(),
        direction,
        ended: false,
        refresh: None,
    });
    migrations.errors.remove(&(set.to_owned(), connection.to_owned()));
    migrations.revision += 1;
}

fn adopt_runs(session: &mut Session) {
    let runs: Vec<(u64, Configuration)> = session
        .state::<Runs>()
        .map(|runs| {
            runs.runs()
                .iter()
                .filter(|run| run.provider == KIND)
                .map(|run| (run.id, run.configuration.clone()))
                .collect()
        })
        .unwrap_or_default();
    for (run_id, configuration) in runs {
        if session
            .state::<Migrations>()
            .is_some_and(|migrations| migrations.tracked.iter().any(|tracked| tracked.run_id == run_id))
        {
            continue;
        }
        let options = options(&configuration);
        let tool = Tool::from_id(&options.tool);
        let set = session.state::<Migrations>().and_then(|migrations| {
            migrations
                .sets
                .iter()
                .find(|set| Some(set.tool) == tool && set.folder == options.folder)
                .map(|set| set.id.clone())
        });
        let direction = Direction::from_id(&options.direction).unwrap_or_default();
        track(session, run_id, &set.unwrap_or_default(), &options.connection, direction);
    }
}

fn finish_runs(session: &mut Session) {
    let ended: Vec<(usize, String, String)> = {
        let runs = session.state::<Runs>();
        let Some(migrations) = session.state::<Migrations>() else { return };
        migrations
            .tracked
            .iter()
            .enumerate()
            .filter(|(_, tracked)| !tracked.ended)
            .filter(|(_, tracked)| {
                runs.and_then(|runs| runs.run(tracked.run_id))
                    .is_none_or(|run| !run.output.read(zen_run::RunOutput::is_running))
            })
            .map(|(index, tracked)| (index, tracked.set.clone(), tracked.connection.clone()))
            .collect()
    };
    for (index, set, connection) in ended {
        session.state_mut::<Migrations>().tracked[index].ended = true;
        let Ok(entry) = find(session, &connection) else { continue };
        let refresh = super::structure::refresh(session, &entry, super::structure::full_request(false), Vec::new())
            .ok()
            .flatten();
        let migrations = session.state_mut::<Migrations>();
        migrations.tracked[index].refresh = refresh;
        migrations.revision += 1;
        if let Some(read) = migrations.reads.get_mut(&(set.clone(), connection.clone())) {
            read.history = None;
        }
        let found = session.state::<Migrations>().and_then(|migrations| migrations.set(&set).cloned());
        if let Some(found) = found
            && connected(session, &connection)
        {
            let _ = start_read(session, &found, &entry);
        }
    }
}

pub fn step(session: &mut Session) {
    poll(session);
    if take_reads(session) {
        session.state_mut::<Migrations>().revision += 1;
    }
    take_secrets(session);
    adopt_runs(session);
    finish_runs(session);
}

pub fn poll(session: &mut Session) {
    let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf);
    let index = session.state::<Workbench>().and_then(Workbench::file_index).cloned();
    let waker = session.state::<Database>().and_then(Database::waker);
    let migrations = session.state_mut::<Migrations>();
    if migrations.root != root {
        *migrations =
            Migrations { root: root.clone(), stale: true, revision: migrations.revision + 1, ..Migrations::default() };
    }
    if let Some(scan) = &migrations.scan {
        let finished = scan.result.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(found) = finished {
            migrations.sets = found;
            migrations.scanned = true;
            migrations.scan = None;
            migrations.revision += 1;
        }
    }
    let Some(root) = root else { return };
    if !migrations.stale || migrations.scan.is_some() {
        return;
    }
    migrations.stale = false;
    let cancel = Arc::new(AtomicBool::new(false));
    let result = Arc::new(Mutex::new(None));
    migrations.scan = Some(Scan { cancel: Arc::clone(&cancel), result: Arc::clone(&result) });
    let exclusions = index.as_ref().map(|index| index.exclusions().clone()).unwrap_or_default();
    let spawned = std::thread::Builder::new().name("zen-db-migrations".to_owned()).spawn(move || {
        let found = scan(&root, &exclusions, index.as_ref(), &cancel);
        *result.lock().unwrap_or_else(PoisonError::into_inner) = Some(found);
        if let Some(waker) = waker {
            waker();
        }
    });
    if spawned.is_err() {
        migrations.scan = None;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MigrationOptions {
    pub tool: String,
    pub folder: String,
    pub connection: String,
    pub direction: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub approval: String,
}

fn approvals() -> &'static Mutex<HashSet<String>> {
    static APPROVALS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    APPROVALS.get_or_init(Mutex::default)
}

fn approve() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let approval = format!("{stamp:x}-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    approvals().lock().unwrap_or_else(PoisonError::into_inner).insert(approval.clone());
    approval
}

const WRAPPER: &str = "secret=\"$1\"; shift; \
    if [ -n \"$secret\" ]; then trap '/bin/rm -f -- \"$secret\"' EXIT HUP INT TERM; fi; \
    unset PGPASSWORD MYSQL_PWD REDISCLI_AUTH CLICKHOUSE_PASSWORD FLYWAY_PASSWORD LIQUIBASE_COMMAND_PASSWORD; \
    \"$@\"";

fn options(configuration: &Configuration) -> MigrationOptions {
    serde_json::from_value(configuration.options.clone()).unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
pub struct MigrationRun {
    data: Option<PathBuf>,
    folders: Option<Vec<PathBuf>>,
}

impl MigrationRun {
    pub fn new(data: Option<PathBuf>) -> Self {
        Self { data, folders: None }
    }

    pub fn in_folders(data: Option<PathBuf>, folders: Vec<PathBuf>) -> Self {
        Self { data, folders: Some(folders) }
    }

    fn folders(&self) -> Vec<PathBuf> {
        self.folders.clone().unwrap_or_else(zen_lsp::servers::search_folders)
    }
}

fn field(key: &str, label: &str, kind: FieldKind) -> FieldSpec {
    FieldSpec { key: key.to_owned(), label_message: label.to_owned(), kind }
}

fn tls_mode(entry: &Entry) -> Option<String> {
    serde_json::to_value(entry.source.tls.mode).ok().and_then(|value| value.as_str().map(str::to_owned))
}

fn target_of(entry: &Entry, root: &Path, password: Option<String>) -> Result<Target, String> {
    let source = &entry.source;
    if source.ssh.as_ref().is_some_and(|ssh| !ssh.host.trim().is_empty()) {
        return Err(t!("database-migration-ssh", name = source.name.as_str()));
    }
    if !migrations::command::DRIVERS.contains(&source.driver.as_str()) {
        return Err(t!("database-migration-driver", name = source.name.as_str(), driver = source.driver.as_str()));
    }
    let parsed = source.url.as_deref().and_then(parse_uri);
    let pick = |own: &Option<String>, from: Option<&String>| {
        own.clone().filter(|value| !value.is_empty()).or_else(|| from.cloned())
    };
    let host = pick(&source.host, parsed.as_ref().and_then(|uri| uri.host.as_ref()));
    let database = pick(&source.database, parsed.as_ref().and_then(|uri| uri.database.as_ref())).unwrap_or_default();
    let user = pick(&source.user, parsed.as_ref().and_then(|uri| uri.user.as_ref()));
    let port = source.port.or_else(|| parsed.as_ref().and_then(|uri| uri.port));
    let database = if source.driver == "sqlite" && !database.is_empty() && !Path::new(&database).is_absolute() {
        root.join(&database).display().to_string()
    } else {
        database
    };
    Ok(Target {
        driver: source.driver.clone(),
        host: host.unwrap_or_else(|| "localhost".to_owned()),
        port,
        database,
        user,
        password,
        tls: tls_mode(entry),
    })
}

fn refusal_text(refusal: &Refusal, set: &MigrationSet) -> String {
    match refusal {
        Refusal::NoDown => t!("database-migration-no-down", tool = set.tool.name()),
        Refusal::Driver(driver) => t!("database-migration-driver", name = set.tool.name(), driver = driver.as_str()),
        Refusal::NeedsTarget => t!("database-migration-needs-state", tool = set.tool.name()),
        Refusal::PasswordInUrl => t!("database-migration-password-in-url", tool = set.tool.name()),
        Refusal::BadName(name) => t!("database-migration-bad-name", name = name.as_str()),
    }
}

fn secret_variable(name: &str) -> bool {
    zen_db::client::PASSWORD_VARIABLES.contains(&name)
        || matches!(name, "FLYWAY_PASSWORD" | "LIQUIBASE_COMMAND_PASSWORD" | "DATABASE_URL")
}

fn needs_password(entry: &Entry) -> bool {
    entry.source.auth == AuthKind::Password && entry.source.driver != "sqlite"
}

impl RunProvider for MigrationRun {
    fn id(&self) -> &str {
        KIND
    }

    fn kinds(&self) -> Vec<KindSpec> {
        let tools: Vec<String> = Tool::ALL.iter().map(|tool| tool.id().to_owned()).collect();
        vec![KindSpec {
            id: KIND.to_owned(),
            name_message: "workbench-dbmigration-kind".to_owned(),
            fields: vec![
                field("tool", "workbench-dbmigration-field-tool", FieldKind::Choice(tools)),
                field("folder", "workbench-dbmigration-field-folder", FieldKind::Path),
                field("connection", "workbench-dbmigration-field-connection", FieldKind::Text),
                field(
                    "direction",
                    "workbench-dbmigration-field-direction",
                    FieldKind::Choice(vec!["up".to_owned(), "down".to_owned()]),
                ),
                field("target", "workbench-dbmigration-field-target", FieldKind::Text),
            ],
            defaults: serde_json::to_value(MigrationOptions {
                direction: "up".to_owned(),
                ..MigrationOptions::default()
            })
            .unwrap_or_default(),
        }]
    }

    fn targets(&self, _context: &RunContext, _path: &Path, _text: &str) -> Vec<RunTarget> {
        Vec::new()
    }

    fn context(&self, _root: &Path, _path: &Path, _text: &str, _offset: usize) -> Option<Configuration> {
        None
    }

    fn command_lines(&self, configuration: &Configuration, context: &RunContext) -> Result<Vec<CommandLine>, String> {
        let options = options(configuration);
        if !approvals().lock().unwrap_or_else(PoisonError::into_inner).remove(&options.approval) {
            return Err(t!("database-migration-not-approved"));
        }
        let tool = Tool::from_id(&options.tool)
            .ok_or_else(|| t!("database-migration-unknown-tool", tool = options.tool.as_str()))?;
        let direction = Direction::from_id(&options.direction)
            .ok_or_else(|| t!("database-migration-unknown-direction", direction = options.direction.as_str()))?;
        let root = &context.root;
        let folder = root.join(&options.folder);
        let set = migrations::recognise(root, &folder)
            .into_iter()
            .find(|set| set.tool == tool)
            .ok_or_else(|| t!("database-migration-no-folder", tool = tool.name(), folder = options.folder.as_str()))?;
        let files = Files::new(self.data.as_deref(), Some((root.as_path(), project_key(root).as_str())));
        let entry = files
            .entries()
            .map_err(|error| error.to_string())?
            .into_iter()
            .find(|entry| entry.source.id == options.connection)
            .ok_or_else(|| t!("database-error-unknown-connection", id = options.connection.as_str()))?;
        let key = credential_key(root, &entry.source.id);
        let password = remembered(&key).map(|secret| secret.expose().to_owned());
        if password.is_none() && needs_password(&entry) && has_password(&entry) {
            return Err(t!("database-migration-password-unread", name = entry.source.name.as_str()));
        }
        let target = target_of(&entry, root, password)?;
        let down_to = Some(options.target.as_str()).filter(|target| !target.is_empty());
        let Invocation { program, mut args, env, secret, display } =
            invocation(&set, direction, &target, down_to).map_err(|refusal| refusal_text(&refusal, &set))?;
        let cwd = if set.base == "." { root.clone() } else { root.join(&set.base) };
        let folders: Vec<PathBuf> =
            self.folders().into_iter().filter(|folder| zen_db::backup::outside(folder, Some(root))).collect();
        let found = zen_lsp::servers::find_executable(&program, &folders)
            .ok_or_else(|| t!("database-migration-not-found", program = program.as_str(), tool = tool.name()))?;
        let mut all = Vec::new();
        if let Ok(path) = std::env::join_paths(&folders) {
            all.push(("PATH".to_owned(), path.to_string_lossy().into_owned()));
        }
        all.extend(env);
        all.extend(configuration.common.env.iter().filter(|&(name, _)| !secret_variable(name)).cloned());
        let file = match secret {
            Some(secret) => {
                if !cfg!(unix) {
                    return Err(t!("database-migration-password-unsupported"));
                }
                let file = super::private_file("zen-db-migration", &secret.text)
                    .map_err(|error| t!("database-client-secret-file", error = error.to_string()))?;
                let path = file.display().to_string();
                match secret.via {
                    migrations::command::Via::Env(name) => all.push((name.to_owned(), path.clone())),
                    migrations::command::Via::FirstArg(option) => args.insert(0, format!("{option}{path}")),
                }
                Some(path)
            }
            None => None,
        };
        let (program, args) = if cfg!(unix) {
            let mut wrapped = vec!["-c".to_owned(), WRAPPER.to_owned(), "zen-db-migration".to_owned()];
            wrapped.push(file.unwrap_or_default());
            wrapped.push(found.display().to_string());
            wrapped.extend(args);
            (PathBuf::from("/bin/sh"), wrapped)
        } else {
            (found, args)
        };
        Ok(vec![CommandLine { program, args, cwd, env: all, display }])
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }
}

/// A migration set as the commands answer it, without its migrations.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetAnswer {
    /// Its id, for `db.migration.status` and `db.migration.run`.
    pub id: String,
    /// The tool.
    pub tool: Tool,
    /// The tool's name as shown.
    pub tool_name: String,
    /// The migrations folder, relative to the project.
    pub folder: String,
    /// The folder the tool runs in, relative to the project.
    pub base: String,
    /// The tool's configuration file, relative to the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Django's application.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// How many migrations the folder has.
    pub count: usize,
    /// Whether the tool can revert migrations.
    pub reverts: bool,
    /// The table the tool records applied migrations in.
    pub table: String,
}

fn set_answer(set: &MigrationSet) -> SetAnswer {
    SetAnswer {
        id: set.id.clone(),
        tool: set.tool,
        tool_name: set.tool.name().to_owned(),
        folder: set.folder.clone(),
        base: set.base.clone(),
        config: set.config.clone(),
        app: set.app.clone(),
        count: set.migrations.len(),
        reverts: set.tool.reverts(),
        table: set.tool.history_table().to_owned(),
    }
}

/// Which migration sets to list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// The most sets to answer; 50 when absent.
    pub limit: Option<usize>,
    /// Scan the project's files again first (in the background; the answer says `scanning`).
    pub refresh: bool,
}

/// The migration sets of the open project.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListAnswer {
    /// The sets, by folder.
    pub sets: Vec<SetAnswer>,
    /// How many there are.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
    /// Whether a scan is running; list again for its result.
    pub scanning: bool,
}

pub struct ListMigrations;

impl Command for ListMigrations {
    type Params = ListParams;
    type Output = ListAnswer;
    const ID: &'static str = "db.migration.list";
    const TITLE: &'static str = "List Database Migrations";
    const DESCRIPTION: &'static str = "Lists the migration folders of the open project with their tool, recognised from the files: Flyway (`V1__name.sql`), Liquibase (changelogs), golang-migrate (`1_name.up.sql`), goose (`-- +goose Up`), Atlas (`atlas.sum`), sqlx (a Rust project using sqlx), Diesel (`<version>_name/up.sql`), Alembic (`versions/*.py`), Django (`<app>/migrations/0001_name.py`), Prisma (`<time>_name/migration.sql`), Rails (`db/migrate`) and Knex (a `knexfile`). Each set has its id, folder, the folder the tool runs in, its configuration file, how many migrations it has and the table the tool records them in. Scanned in the background (`scanning` until done; `refresh` scans again), seven folders deep, leaving out dependency, build, excluded and ignored folders; at most `limit` (50) with `total` and `truncated`. Connects nowhere; db.migration.status tells what a connection has applied.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ListAnswer, CommandError> {
        if params.refresh {
            session.state_mut::<Migrations>().stale = true;
        }
        poll(session);
        let migrations = session.state_mut::<Migrations>();
        let total = migrations.sets.len();
        let limit = params.limit.unwrap_or(LISTED);
        let sets = migrations.sets.iter().take(limit).map(set_answer).collect();
        Ok(ListAnswer { sets, total, truncated: total > limit, scanning: migrations.scanning() })
    }
}

fn unknown_set(session: &Session, id: &str) -> CommandError {
    let message = super::for_caller(
        session,
        t!("database-migration-unknown-set", id = id),
        t!("database-migration-unknown-set-agent", id = id),
    );
    CommandError::with_code(UNKNOWN_SET, message)
}

fn set_of(session: &mut Session, id: &str) -> Result<MigrationSet, CommandError> {
    poll(session);
    session
        .state::<Migrations>()
        .and_then(|migrations| migrations.set(id).cloned())
        .ok_or_else(|| unknown_set(session, id))
}

/// A set and a connection to tell the state of.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusParams {
    /// The set's id from `db.migration.list`.
    pub set: String,
    /// The connection whose applied migrations to read; without it every migration is `unknown`.
    pub connection: Option<String>,
    /// Read the tool's table again, though it was read before.
    pub refresh: bool,
    /// Only the migrations in this state.
    pub state: Option<State>,
    /// The most migrations to answer; 200 when absent.
    pub limit: Option<usize>,
}

/// How many migrations are in each state.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    /// Applied.
    pub applied: usize,
    /// Not applied yet.
    pub pending: usize,
    /// Failed or left dirty.
    pub failed: usize,
    /// Not known: the connection was not read.
    pub unknown: usize,
    /// Applied on the database without a file in the project.
    pub missing: usize,
}

/// The last migration run of a set on a connection.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RunAnswer {
    /// The run's id, for run.output.get; absent while the password is read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// `up` or `down`.
    pub direction: Direction,
    /// Where the run is; absent while the password is read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<RunState>,
    /// The tool's exit code, once it ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Whether the connection's password is being read from the Keychain before the run starts.
    pub waiting: bool,
    /// The operation reading the connection's structure again after the run, when it was connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh: Option<u64>,
}

/// Where a set's migrations stand on a connection.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusAnswer {
    /// The set.
    pub set: SetAnswer,
    /// The connection asked about.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// Whether the connection is connected; a disconnected one is never connected by this command.
    pub connected: bool,
    /// Whether the connection is read-only, so that Up and Down are refused.
    pub read_only: bool,
    /// Whether the tool's table is being read; ask again.
    pub loading: bool,
    /// Whether the tool's table exists, once read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table: Option<bool>,
    /// How many migrations are in each state.
    pub counts: Counts,
    /// The migrations, oldest first, then those only the database has; at most `limit`.
    pub migrations: Vec<Status>,
    /// How many migrations matched.
    pub total: usize,
    /// Whether some were left out because of `limit`.
    pub truncated: bool,
    /// Why reading the table or the last run failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The last run of the set on the connection.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run: Option<RunAnswer>,
}

fn last_run(session: &Session, set: &str, connection: &str) -> Option<RunAnswer> {
    let migrations = session.state::<Migrations>()?;
    if let Some(waiting) =
        migrations.waiting.iter().rev().find(|waiting| waiting.set == set && waiting.connection == connection)
    {
        return Some(RunAnswer {
            run_id: None,
            direction: waiting.direction,
            state: None,
            exit_code: None,
            waiting: true,
            refresh: None,
        });
    }
    let tracked =
        migrations.tracked.iter().rev().find(|tracked| tracked.set == set && tracked.connection == connection)?;
    let run = session.state::<Runs>().and_then(|runs| runs.run(tracked.run_id));
    let (state, exit_code) =
        run.map(|run| run.output.read(|output| (Some(output.state()), output.exit_code()))).unwrap_or((None, None));
    Some(RunAnswer {
        run_id: Some(tracked.run_id),
        direction: tracked.direction,
        state,
        exit_code,
        waiting: false,
        refresh: tracked.refresh,
    })
}

pub fn status(
    session: &mut Session,
    set: &MigrationSet,
    connection: Option<&str>,
    refresh: bool,
) -> Result<(Vec<Status>, StatusAnswer), CommandError> {
    take_reads(session);
    let entry = connection.map(|connection| find(session, connection)).transpose()?;
    let is_connected = entry.as_ref().is_some_and(|entry| connected(session, &entry.source.id));
    if let Some(entry) = &entry
        && is_connected
    {
        let key = (set.id.clone(), entry.source.id.clone());
        let read = session.state::<Migrations>().and_then(|migrations| migrations.reads.get(&key));
        let fresh = read.is_some_and(|read| read.history.is_some() || read.error.is_some() || read.operation.is_some());
        if refresh || !fresh {
            start_read(session, set, entry)?;
        }
    }
    let key = connection.map(|connection| (set.id.clone(), connection.to_owned()));
    let migrations = session.state::<Migrations>();
    let read = key.as_ref().and_then(|key| migrations.and_then(|migrations| migrations.reads.get(key)));
    let history = read.and_then(|read| read.history.as_ref()).filter(|_| is_connected);
    let list = migrations::statuses(set, history);
    let mut counts = Counts::default();
    for status in &list {
        match (status.state, status.file.is_some()) {
            (_, false) => counts.missing += 1,
            (State::Applied, true) => counts.applied += 1,
            (State::Pending, true) => counts.pending += 1,
            (State::Failed, true) => counts.failed += 1,
            (State::Unknown, true) => counts.unknown += 1,
        }
    }
    let error = key.as_ref().and_then(|key| {
        migrations
            .and_then(|migrations| migrations.errors.get(key).cloned())
            .or_else(|| read.and_then(|read| read.error.clone()))
    });
    let answer = StatusAnswer {
        set: set_answer(set),
        connection: connection.map(str::to_owned),
        connected: is_connected,
        read_only: entry.as_ref().is_some_and(|entry| super::safety::read_only(session, entry)),
        loading: is_connected && read.is_some_and(|read| read.operation.is_some()),
        table: history.map(|history| history.table),
        counts,
        migrations: Vec::new(),
        total: 0,
        truncated: false,
        error,
        run: connection.and_then(|connection| last_run(session, &set.id, connection)),
    };
    Ok((list, answer))
}

pub struct MigrationStatus;

impl Command for MigrationStatus {
    type Params = StatusParams;
    type Output = StatusAnswer;
    const ID: &'static str = "db.migration.status";
    const TITLE: &'static str = "Database Migration Status";
    const DESCRIPTION: &'static str = "Tells where each migration of set `set` (db.migration.list) stands on connection `connection`: `applied`, `pending`, `failed` (left dirty or half-applied) or `unknown`, oldest first, then those the database has without a file in the project (no `file`). Offline first: the files are listed without a connection, and a disconnected connection is never connected (`connected: false`, every one `unknown`). On a connected one the tool's own table (`flyway_schema_history`, `schema_migrations`, `_sqlx_migrations`...) is read in the background in the Metadata context (`loading`; ask again), once, or again with `refresh`; a missing table means nothing is applied (`table: false`). Answers `counts`, at most `limit` (200) migrations, `state` keeping one state, and the set's last run (`run`: its id, state and exit code, and the structure read again after it); `readOnly` tells that db.migration.run is refused on the connection. Agents follow the connection type (ADR-0172).";

    fn run(&self, session: &mut Session, params: StatusParams) -> Result<StatusAnswer, CommandError> {
        let set = set_of(session, &params.set)?;
        let (list, mut answer) = status(session, &set, params.connection.as_deref(), params.refresh)?;
        let matching: Vec<Status> =
            list.into_iter().filter(|status| params.state.is_none_or(|state| status.state == state)).collect();
        let limit = params.limit.unwrap_or(MIGRATIONS_LISTED);
        answer.total = matching.len();
        answer.truncated = matching.len() > limit;
        answer.migrations = matching.into_iter().take(limit).collect();
        Ok(answer)
    }
}

/// A migration run to start.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RunParams {
    /// The set's id from `db.migration.list`.
    pub set: String,
    /// The connection to run against.
    pub connection: String,
    /// `up` applies every pending migration; `down` reverts the last applied one.
    pub direction: Direction,
    /// The `token` of a `confirmExecute` refusal: runs what it asked about.
    pub confirmed: Option<String>,
    /// The password for this run, when none is stored; kept in memory for this IDE's runs, never stored or answered.
    pub password: Option<String>,
    /// Also save it as a run configuration of the project, to run again from the Run widget.
    pub save: bool,
}

/// A migration run started, or waiting for the connection's password.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The run's id, for run.output.get and run.process.stop; absent while the password is read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<u64>,
    /// Whether the connection's password is being read from the Keychain first; db.migration.status tells when the run started.
    pub waiting: bool,
    /// The command line, without the password.
    pub command: String,
    /// The run configuration saved with `save`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration_id: Option<String>,
}

fn token_of(connection: &str, set: &MigrationSet, direction: Direction, list: &[Status]) -> String {
    let mut hasher = DefaultHasher::new();
    (connection, &set.id, direction).hash(&mut hasher);
    for status in list {
        (&status.version, status.state == State::Applied).hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn configuration_of(set: &MigrationSet, entry: &Entry, direction: Direction, target: Option<String>) -> Configuration {
    let options = MigrationOptions {
        tool: set.tool.id().to_owned(),
        folder: set.folder.clone(),
        connection: entry.source.id.clone(),
        direction: direction.id().to_owned(),
        target: target.unwrap_or_default(),
        approval: String::new(),
    };
    let name = match direction {
        Direction::Up => t!("database-migration-name-up", tool = set.tool.name(), name = entry.source.name.as_str()),
        Direction::Down => {
            t!("database-migration-name-down", tool = set.tool.name(), name = entry.source.name.as_str())
        }
    };
    Configuration {
        name,
        kind: KIND.to_owned(),
        options: serde_json::to_value(options).unwrap_or_default(),
        ..Configuration::default()
    }
}

fn ask(
    session: &Session,
    (entry, set, direction): (&Entry, &MigrationSet, Direction),
    (list, command, token): (&[Status], &str, &str),
    params: &RunParams,
) -> CommandError {
    let name = entry.source.name.as_str();
    let pending: Vec<&Status> = list.iter().filter(|status| status.state == State::Pending).collect();
    let last = list.iter().rfind(|status| status.state == State::Applied && status.file.is_some());
    let message = match direction {
        Direction::Up => {
            t!("database-migration-confirm-up", tool = set.tool.name(), name = name, count = pending.len())
        }
        Direction::Down => t!(
            "database-migration-confirm-down",
            tool = set.tool.name(),
            name = name,
            migration = last.map_or_else(String::new, |status| status.version.clone())
        ),
    };
    let mut risks = Vec::new();
    if type_of(session, entry).confirm_execute {
        risks.push(t!("database-migration-risk-type", kind = type_of(session, entry).name.as_str()));
    }
    if direction == Direction::Down {
        risks.push(t!("database-migration-risk-down"));
    }
    let dangers = if direction == Direction::Down {
        serde_json::json!([{ "risk": "revertMigration", "object": last.map(|status| status.version.clone()) }])
    } else {
        serde_json::json!([])
    };
    let retried = serde_json::json!({
        "set": set.id,
        "connection": entry.source.id,
        "direction": direction,
        "confirmed": token,
        "save": params.save,
    });
    CommandError::with_code(CONFIRM_EXECUTE, message).with_data(serde_json::json!({
        "token": token,
        "target": target(session, entry),
        "statements": [{
            "index": 0,
            "text": command,
            "cut": false,
            "effect": "structure",
            "dangers": dangers,
            "risks": risks,
        }],
        "pending": pending.iter().take(MIGRATIONS_LISTED).map(|status| status.version.clone()).collect::<Vec<_>>(),
        "asked": 1,
        "total": 1,
        "truncated": pending.len() > MIGRATIONS_LISTED,
        "dangerous": direction == Direction::Down,
        "skippable": false,
        "retry": { "command": RunMigrations::ID, "params": retried },
    }))
}

fn guard(session: &Session, entry: &Entry, set: &MigrationSet) -> Result<(), CommandError> {
    let kind = type_of(session, entry);
    let name = entry.source.name.as_str();
    let agent = zen_core::macros::by_agent(session);
    if agent && kind.agent_access() == AgentAccess::None {
        return Err(CommandError::with_code(
            AGENT_NOT_ALLOWED,
            t!("database-error-agent-not-allowed", name = name, kind = kind.name.as_str()),
        ));
    }
    if super::safety::read_only(session, entry) {
        return Err(CommandError::with_code(READ_ONLY, t!("database-migration-read-only", name = name)));
    }
    if agent && kind.agent_access() == AgentAccess::ReadOnly {
        let hand_off = serde_json::json!({
            "handOff": { "command": OpenMigrations::ID, "params": { "connection": entry.source.id, "set": set.id } }
        });
        return Err(CommandError::with_code(
            AGENT_READ_ONLY,
            t!("database-migration-agent-read-only", name = name, kind = kind.name.as_str()),
        )
        .with_data(hand_off));
    }
    Ok(())
}

pub struct RunMigrations;

impl Command for RunMigrations {
    type Params = RunParams;
    type Output = Started;
    const ID: &'static str = "db.migration.run";
    const TITLE: &'static str = "Run Database Migrations";
    const DESCRIPTION: &'static str = "Runs the tool of set `set` (db.migration.list) against connection `connection` as a run configuration (kind `db.migration`) in the Run window: `direction` `up` applies every pending migration (flyway migrate, migrate up, goose up, sqlx migrate run, diesel migration run, alembic upgrade head, manage.py migrate, prisma migrate deploy, rails db:migrate, knex migrate:latest...), `down` reverts the last applied one (Prisma cannot; Django needs the applied state read first, code `needsState`). The tool gets the connection's host, port, database and user, and its password in a new file only the user can read that it is pointed at (PGPASSFILE, -configFiles=, --defaults-file=), removed when it ends — never its environment or arguments, never answered; tools that take a password only in a URL are refused where one is needed (`passwordInUrl` text). A stored password is read from the Keychain in the background first (`waiting`). Refused on a closed connection (`notConnected`, with a handOff to db.connection.connect). Programs come only from PATH outside the project (ADR-0193). A run starts only from this command; a saved configuration or Rerun is sent back here. Production safety (ADR-0172): refused on a read-only connection (`readOnly`); `down` always asks, and `up` asks where the connection type confirms SQL execution: refused with code `confirmExecute`, the command line, the pending migrations and a `token`, run again with `confirmed: token` (the refusal's `retry`); agents may not run it where they only read (`agentReadOnly`, with a `handOff`) or run nothing (`agentNotAllowed`). Connections through SSH and drivers other than PostgreSQL, MySQL, MariaDB and SQLite are refused (`unsupported`). When the run ends, the connection's structure is read again if it is connected, and the applied state too. `save` also keeps it as a project run configuration. Answers the run's id; read its output with run.output.get.";

    fn run(&self, session: &mut Session, params: RunParams) -> Result<Started, CommandError> {
        let set = set_of(session, &params.set)?;
        let entry = find(session, &params.connection)?;
        let root = session
            .state::<Workbench>()
            .and_then(Workbench::project)
            .map(Path::to_path_buf)
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        guard(session, &entry, &set)?;
        crate::trust::guard_agent_build(session, &root)?;
        let shown = target_of(&entry, &root, None).map_err(|error| CommandError::with_code(UNSUPPORTED, error))?;
        let (list, answer) = status(session, &set, Some(&entry.source.id), false)?;
        if !answer.connected {
            return Err(CommandError::with_code(
                "notConnected",
                t!("database-migration-not-connected", name = entry.source.name.as_str()),
            )
            .with_data(serde_json::json!({
                "handOff": { "command": "db.connection.connect", "params": { "connection": entry.source.id } }
            })));
        }
        let known = answer.connected && answer.table.is_some();
        let direction = params.direction;
        if direction == Direction::Down && known && answer.counts.applied == 0 {
            return Err(CommandError::with_code(
                "nothingApplied",
                t!("database-migration-nothing-applied", tool = set.tool.name()),
            ));
        }
        let down_to = (set.tool == Tool::Django && direction == Direction::Down)
            .then(|| if known { migrations::django_previous(&list) } else { None })
            .flatten();
        let command = match invocation(&set, direction, &shown, down_to.as_deref()) {
            Ok(built) => built.display,
            Err(Refusal::NeedsTarget) => {
                return Err(CommandError::with_code(NEEDS_STATE, refusal_text(&Refusal::NeedsTarget, &set)));
            }
            Err(refusal) => return Err(CommandError::with_code(UNSUPPORTED, refusal_text(&refusal, &set))),
        };
        let asks = direction == Direction::Down || type_of(session, &entry).confirm_execute;
        let token = token_of(&entry.source.id, &set, direction, &list);
        if asks && params.confirmed.as_deref() != Some(token.as_str()) {
            return Err(ask(session, (&entry, &set, direction), (&list, &command, &token), &params));
        }
        let configuration = configuration_of(&set, &entry, direction, down_to);
        let configuration_id = if params.save {
            crate::run::add_missing(session, &root, vec![configuration.clone()])?.first().map(|saved| saved.id.clone())
        } else {
            None
        };
        let mut configuration = configuration;
        if let Some(object) = configuration.options.as_object_mut() {
            object.insert("approval".to_owned(), serde_json::json!(approve()));
        }
        let key = credential_key(&root, &entry.source.id);
        if let Some(password) = params.password.filter(|password| !password.is_empty()) {
            remember(&key, Secret::new(password));
        }
        let state_key = (set.id.clone(), entry.source.id.clone());
        session.state_mut::<Migrations>().errors.remove(&state_key);
        if remembered(&key).is_none() && needs_password(&entry) && has_password(&entry) {
            let store = secret_store(session)?;
            let id = secret_id(session, &entry, SecretKind::Password)?;
            let slot: Slot<Result<Option<Secret>, String>> = Arc::new(Mutex::new(None));
            let filled = Arc::clone(&slot);
            let waker = session.state::<Database>().and_then(Database::waker);
            let name = entry.source.id.clone();
            std::thread::Builder::new()
                .name("zen-db-migration-secret".to_owned())
                .spawn(move || {
                    let read = store.get(&id).map_err(|error| super::sources::secret_text(&error, &name));
                    *filled.lock().unwrap_or_else(PoisonError::into_inner) = Some(read);
                    if let Some(waker) = waker {
                        waker();
                    }
                })
                .map_err(|error| CommandError::new(error.to_string()))?;
            let migrations = session.state_mut::<Migrations>();
            migrations.waiting.push(Waiting {
                key,
                connection: entry.source.id.clone(),
                set: set.id.clone(),
                direction,
                configuration,
                slot,
            });
            migrations.revision += 1;
            return Ok(Started { run_id: None, waiting: true, command, configuration_id });
        }
        let started = crate::run::start(session, configuration)?;
        track(session, started.run_id, &set.id, &entry.source.id, direction);
        Ok(Started { run_id: Some(started.run_id), waiting: false, command, configuration_id })
    }
}

/// What the Migrations tab shows.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenParams {
    /// The connection to show the applied state of; the tab's last one when absent.
    pub connection: Option<String>,
    /// The set to select; the tab's last one when absent.
    pub set: Option<String>,
}

pub struct OpenMigrations;

impl Command for OpenMigrations {
    type Params = OpenParams;
    type Output = InnerTab;
    const ID: &'static str = "db.migration.open";
    const TITLE: &'static str = "Open Migrations";
    const DESCRIPTION: &'static str = "Opens the Migrations tab of the Database workspace (kind `migrations`), or brings it to front and changes what it shows, and expands the workspace: the project's migration sets, the chosen set's migrations with their state on connection `connection`, and Up and Down. It never connects by itself. Answers the tab; read what it shows with db.migration.list and db.migration.status.";

    fn run(&self, session: &mut Session, params: OpenParams) -> Result<InnerTab, CommandError> {
        if let Some(connection) = &params.connection {
            find(session, connection)?;
        }
        poll(session);
        let existing = session.state::<crate::workspaces::Workspaces>().and_then(|workspaces| {
            workspaces.tabs(WORKSPACE).iter().find(|tab| tab.id == TAB).map(|tab| tab.data.clone())
        });
        let mut data = existing.filter(serde_json::Value::is_object).unwrap_or_else(|| serde_json::json!({}));
        if let Some(connection) = params.connection {
            data["connection"] = serde_json::json!(connection);
        }
        if let Some(set) = params.set {
            data["set"] = serde_json::json!(set);
        }
        OpenTab.run(
            session,
            OpenTabParams {
                workspace: WORKSPACE.to_owned(),
                kind: Some(TAB.to_owned()),
                title: Some(t!("database-migration-title")),
                id: Some(TAB.to_owned()),
                closable: Some(true),
                data: Some(data),
                path: None,
                group: None,
            },
        )
    }
}

#[cfg(test)]
mod tests;

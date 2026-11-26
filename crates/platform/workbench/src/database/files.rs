use std::io::Read;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::WORKSPACE;
use super::navigator::{NavigatorSelect, RowParams};
use super::sources::{Database, files};
use crate::Workbench;
use crate::workspaces::{Expand, WorkspaceParams};

pub const EXTENSIONS: [&str; 5] = ["db", "sqlite", "sqlite3", "db3", "s3db"];

const HEADER: &[u8; 16] = b"SQLite format 3\0";

fn file_driver(session: &mut Session, driver: &str) -> bool {
    session
        .state_mut::<Database>()
        .drivers()
        .iter()
        .any(|known| known.id() == driver && known.capabilities().shared_connection)
}

fn project(session: &Session) -> Option<PathBuf> {
    session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf)
}

pub fn resolve(session: &mut Session, driver: &str, database: String) -> String {
    let path = database.trim();
    if path.is_empty() || path == ":memory:" || Path::new(path).is_absolute() || !file_driver(session, driver) {
        return database;
    }
    match project(session) {
        Some(root) => root.join(path).display().to_string(),
        None => database,
    }
}

pub fn is_database_file(path: &Path) -> bool {
    let known = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(extension)));
    if !known {
        return false;
    }
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut head = Vec::with_capacity(HEADER.len());
    if file.by_ref().take(HEADER.len() as u64).read_to_end(&mut head).is_err() {
        return false;
    }
    head.is_empty() || head == HEADER
}

fn absolute(session: &Session, path: &str) -> Result<PathBuf, CommandError> {
    let path = path.trim();
    if path.is_empty() {
        return Err(CommandError::with_code("invalidPath", t!("database-file-no-path")));
    }
    let path = PathBuf::from(path);
    if path.is_absolute() {
        return Ok(path);
    }
    project(session).map(|root| root.join(&path)).ok_or_else(|| {
        CommandError::with_code("invalidPath", t!("database-file-not-absolute", path = path.display().to_string()))
    })
}

/// A database file to create or open.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FileParams {
    /// The file's path: absolute, or relative to the open project's folder.
    pub path: String,
}

/// A database file created.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    /// Its absolute path.
    pub path: String,
}

pub struct CreateFile;

impl Command for CreateFile {
    type Params = FileParams;
    type Output = Created;
    const ID: &'static str = "db.databaseFile.create";
    const TITLE: &'static str = "Create Database File";
    const DESCRIPTION: &'static str = "Creates an empty SQLite database file at `path` (absolute, or relative to the project's folder), for a new connection's Path (Create… in the connection dialog). Never replaces anything: a path that exists is refused with code `exists`; the folder must exist. Connects nowhere. Answers the absolute path.";

    fn run(&self, session: &mut Session, params: FileParams) -> Result<Created, CommandError> {
        let path = absolute(session, &params.path)?;
        super::guard_agent_file(session, &path)?;
        let shown = path.display().to_string();
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => Ok(Created { path: shown }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(CommandError::with_code("exists", t!("database-file-exists", path = shown.as_str())))
            }
            Err(error) => {
                Err(CommandError::new(t!("database-file-failed", path = shown.as_str(), error = error.to_string())))
            }
        }
    }
}

/// The fields of a new connection that opens a database file, as `db.dataSource.add` takes them.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Offer {
    /// The driver's id (`sqlite`).
    pub driver: String,
    /// The file's name, as the connection's name.
    pub name: String,
    /// The file's absolute path.
    pub database: String,
}

/// What opening a database file found.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    /// The file's absolute path.
    pub path: String,
    /// The connection that already opens it, now selected in the Database Navigator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
    /// A new connection to offer when none opens it yet.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offer: Option<Offer>,
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

pub struct OpenFile;

impl Command for OpenFile {
    type Params = FileParams;
    type Output = Opened;
    const ID: &'static str = "db.databaseFile.open";
    const TITLE: &'static str = "Open Database File";
    const DESCRIPTION: &'static str = "Opens an SQLite database file (absolute, or relative to the project's folder) in the Database workspace, as opening a .db or .sqlite file from the Project tool window does: when a connection already opens it, the workspace expands and selects it in the navigator (`connection`); otherwise answers `offer`, the fields of a new connection for db.dataSource.add, which the window shows in the connection dialog. Never connects and never creates a file.";

    fn run(&self, session: &mut Session, params: FileParams) -> Result<Opened, CommandError> {
        let path = absolute(session, &params.path)?;
        super::guard_agent_file(session, &path)?;
        if !path.is_file() {
            return Err(CommandError::with_code(
                zen_db::sqlite::NO_FILE,
                t!("database-file-missing", path = path.display().to_string()),
            ));
        }
        let entries = files(session).entries().unwrap_or_default();
        let mut found = None;
        for entry in entries {
            let driver = entry.source.driver.clone();
            if !file_driver(session, &driver) {
                continue;
            }
            let Some(database) = entry.source.database.clone() else { continue };
            let opened = resolve(session, &driver, database);
            if same_file(Path::new(&opened), &path) {
                found = Some(entry.source.id);
                break;
            }
        }
        let shown = path.display().to_string();
        if let Some(id) = found {
            Expand.run(session, WorkspaceParams { id: WORKSPACE.to_owned() })?;
            NavigatorSelect.run(session, RowParams { connection: Some(id.clone()), ..RowParams::default() })?;
            return Ok(Opened { path: shown, connection: Some(id), offer: None });
        }
        let name = path.file_name().map_or_else(|| shown.clone(), |name| name.to_string_lossy().into_owned());
        let offer = Offer { driver: zen_db::sqlite::ID.to_owned(), name, database: shown.clone() };
        Ok(Opened { path: shown, connection: None, offer: Some(offer) })
    }
}

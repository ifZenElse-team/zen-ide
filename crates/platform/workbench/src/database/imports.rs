use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::import::{self, Imported};
use zen_db::registry::{Origin, OriginKind, Scope};
use zen_i18n::t;

use super::sources::{self, Database, SourceFields};
use crate::Workbench;
use crate::hidden_files;

const LISTED: usize = 50;

/// What to import.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportParams {
    /// DBeaver: its workspace folder, a project folder in it, or a `data-sources.json`; JetBrains: a project folder or its `.idea`. DBeaver's usual workspace or the open project when absent.
    pub path: Option<PathBuf>,
    /// Where to keep the connections: `global` (default) or `project`.
    pub scope: Scope,
    /// The `key`s of the entries to take; every one that can be taken when absent.
    pub keys: Option<Vec<String>>,
    /// Only list what would be imported; nothing is added.
    pub preview: bool,
    /// The most entries to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// What became of an entry of the other tool's file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ImportStatus {
    /// It can be imported (a preview).
    Ready,
    /// It was imported.
    Added,
    /// The same connection is there already; it was left alone.
    Exists,
    /// The IDE has no driver for its database; it was skipped.
    Unsupported,
    /// It was not among the `keys` asked for.
    NotChosen,
}

/// An entry of the other tool's file; never a secret.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportEntry {
    /// Its id in that file, for `keys`.
    pub key: String,
    /// Its name.
    pub name: String,
    /// The other tool's name of its driver.
    pub product: String,
    /// Our driver's id, when we know its database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    /// The host.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// The port.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// The database.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    /// The user name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Its folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// Its connection type's id.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub connection_type: Option<String>,
    /// The SSH host it is reached through.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh: Option<String>,
    /// What became of it.
    pub status: ImportStatus,
    /// Why it was skipped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The connection it became or is the same as.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<String>,
}

/// What an import read and did.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportAnswer {
    /// The files read.
    pub files: Vec<PathBuf>,
    /// The entries, at most `limit` of them.
    pub entries: Vec<ImportEntry>,
    /// How many connections were added.
    pub added: usize,
    /// How many entries there are.
    pub total: usize,
    /// Whether entries were left out because of `limit`.
    pub truncated: bool,
}

fn refused(message: String) -> CommandError {
    CommandError::with_code("importFailed", message)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").filter(|home| !home.is_empty()).map(PathBuf::from)
}

pub fn dbeaver_workspace() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        home().map(|home| home.join("Library/DBeaverData/workspace6"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|data| PathBuf::from(data).join("DBeaverData/workspace6"))
    } else {
        home().map(|home| home.join(".local/share/DBeaverData/workspace6"))
    }
}

fn data_source_files(folder: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("data-sources") && name.ends_with(".json"))
        })
        .collect();
    files.sort();
    files
}

fn dbeaver_files(path: &Path) -> Vec<PathBuf> {
    if path.is_file() {
        return vec![path.to_path_buf()];
    }
    let mut files = data_source_files(path);
    files.extend(data_source_files(&path.join(".dbeaver")));
    let mut projects: Vec<PathBuf> = std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join(".dbeaver"))
        .filter(|folder| folder.is_dir())
        .collect();
    projects.sort();
    for folder in projects {
        files.extend(data_source_files(&folder));
    }
    files
}

fn read(file: &Path) -> Result<String, CommandError> {
    let failed = |error: String| refused(t!("database-error-import-read", file = crate::display(file), error = error));
    let size = std::fs::metadata(file).map_err(|error| failed(error.to_string()))?.len();
    if size > import::FILE_LIMIT {
        return Err(failed(t!("database-error-import-too-large")));
    }
    std::fs::read_to_string(file).map_err(|error| failed(error.to_string()))
}

fn guard(session: &Session, files: &[PathBuf]) -> Result<(), CommandError> {
    let Some(patterns) = hidden_files::for_caller(session) else { return Ok(()) };
    let root = session.state::<Workbench>().and_then(Workbench::project);
    match files.iter().find(|file| hidden_files::is_hidden(&patterns, root, file)) {
        Some(file) => Err(hidden_files::refused(&crate::display(file))),
        None => Ok(()),
    }
}

fn same(entry: &zen_db::registry::Entry, found: &Imported, kind: OriginKind) -> bool {
    let origin = entry.source.origin.as_ref();
    if origin.is_some_and(|origin| origin.kind == kind && origin.key.as_deref() == Some(found.key.as_str())) {
        return true;
    }
    let place = |a: Option<&str>, b: Option<&str>| a.unwrap_or_default().eq_ignore_ascii_case(b.unwrap_or_default());
    found.driver.as_deref() == Some(entry.source.driver.as_str())
        && place(entry.source.host.as_deref(), found.host.as_deref())
        && entry.source.port == found.port
        && place(entry.source.database.as_deref(), found.database.as_deref())
        && place(entry.source.user.as_deref(), found.user.as_deref())
}

fn run_import(
    session: &mut Session,
    params: ImportParams,
    kind: OriginKind,
    files: Vec<PathBuf>,
    found: Vec<(PathBuf, Imported)>,
) -> Result<ImportAnswer, CommandError> {
    let drivers: Vec<String> =
        session.state_mut::<Database>().drivers().iter().map(|driver| driver.id().to_owned()).collect();
    let mut entries = Vec::new();
    let mut added = 0;
    for (file, found) in found {
        let mut entry = ImportEntry {
            key: found.key.clone(),
            name: found.name.clone(),
            product: found.product.clone(),
            driver: found.driver.clone(),
            host: found.host.clone(),
            port: found.port,
            database: found.database.clone(),
            user: found.user.clone(),
            folder: found.folder.clone(),
            connection_type: found.connection_type.clone(),
            ssh: found.ssh.as_ref().map(|ssh| ssh.host.clone()),
            status: ImportStatus::Ready,
            reason: None,
            connection: None,
        };
        let chosen = params.keys.as_ref().is_none_or(|keys| keys.contains(&found.key));
        let known = sources::files(session).entries().unwrap_or_default();
        if let Some(existing) = known.iter().find(|existing| same(existing, &found, kind)) {
            entry.status = ImportStatus::Exists;
            entry.connection = Some(existing.source.id.clone());
        } else if !found.driver.as_ref().is_some_and(|driver| drivers.contains(driver)) {
            entry.status = ImportStatus::Unsupported;
            entry.reason = Some(t!("database-import-unsupported", product = found.product.as_str()));
        } else if !chosen {
            entry.status = ImportStatus::NotChosen;
        } else if !params.preview {
            let fields = SourceFields {
                name: Some(found.name.clone()),
                folder: found.folder.clone(),
                driver: found.driver.clone(),
                host: found.host.clone(),
                port: found.port,
                database: found.database.clone(),
                user: found.user.clone(),
                connection_type: found.connection_type.clone(),
                read_only: Some(found.read_only),
                ssh: found.ssh.clone(),
                ..SourceFields::default()
            };
            let origin = Origin {
                kind,
                file: file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                key: Some(found.key.clone()),
                detection: None,
                fingerprint: None,
            };
            let connection = sources::add(session, params.scope, &[&fields], Some(origin))?;
            entry.status = ImportStatus::Added;
            entry.connection = Some(connection.entry.source.id.clone());
            added += 1;
        }
        entries.push(entry);
    }
    let total = entries.len();
    let limit = params.limit.unwrap_or(LISTED);
    entries.truncate(limit);
    Ok(ImportAnswer { files, entries, added, total, truncated: total > limit })
}

pub struct ImportDbeaver;

impl Command for ImportDbeaver {
    type Params = ImportParams;
    type Output = ImportAnswer;
    const ID: &'static str = "db.import.dbeaver";
    const TITLE: &'static str = "Import Connections from DBeaver";
    const DESCRIPTION: &'static str = "Imports connections from DBeaver's `data-sources.json` (`path`: its workspace folder, a project folder in it or the file; DBeaver's usual workspace when absent) into `scope` (`global` by default): names, folders, drivers we have, hosts, ports, databases, user names, connection types (`dev`, `test`, `prod`) and SSH hosts. DBeaver's credentials are never read and its files never changed; people type their passwords at the first connect. `keys` picks entries, `preview` only lists them. Entries of drivers the IDE does not have are skipped with the reason, and the same connection is not added twice. Connects nowhere.";

    fn run(&self, session: &mut Session, params: ImportParams) -> Result<ImportAnswer, CommandError> {
        let path = params
            .path
            .clone()
            .or_else(dbeaver_workspace)
            .ok_or_else(|| refused(t!("database-error-import-no-path")))?;
        let files = dbeaver_files(&path);
        if files.is_empty() {
            return Err(refused(t!(
                "database-error-import-none",
                path = crate::display(&path),
                file = "data-sources.json"
            )));
        }
        guard(session, &files)?;
        let mut found = Vec::new();
        for file in &files {
            let text = read(file)?;
            let entries = import::dbeaver(&text).map_err(|error| {
                refused(t!("database-error-import-read", file = crate::display(file), error = error))
            })?;
            found.extend(entries.into_iter().map(|entry| (file.clone(), entry)));
        }
        run_import(session, params, OriginKind::Dbeaver, files, found)
    }
}

pub struct ImportJetbrains;

impl Command for ImportJetbrains {
    type Params = ImportParams;
    type Output = ImportAnswer;
    const ID: &'static str = "db.import.jetbrains";
    const TITLE: &'static str = "Import Connections from JetBrains Project";
    const DESCRIPTION: &'static str = "Imports connections from a JetBrains project's `.idea/dataSources.xml` and the user names of its `dataSources.local.xml` (`path`: the project folder or its `.idea`; the open project when absent) into `scope` (`global` by default): names, drivers we have, hosts, ports, databases and user names. Passwords stay in the JetBrains credential store, never read; `.idea` is never written. `keys` picks entries, `preview` only lists them. Entries of drivers the IDE does not have are skipped with the reason, and the same connection is not added twice. Connects nowhere.";

    fn run(&self, session: &mut Session, params: ImportParams) -> Result<ImportAnswer, CommandError> {
        let path = params
            .path
            .clone()
            .or_else(|| session.state::<Workbench>().and_then(Workbench::project).map(Path::to_path_buf))
            .ok_or_else(|| refused(t!("database-error-import-no-path")))?;
        let idea = if path.file_name().is_some_and(|name| name == ".idea") { path.clone() } else { path.join(".idea") };
        let shared = idea.join("dataSources.xml");
        if !shared.is_file() {
            return Err(refused(t!(
                "database-error-import-none",
                path = crate::display(&path),
                file = ".idea/dataSources.xml"
            )));
        }
        let local = idea.join("dataSources.local.xml");
        let mut files = vec![shared.clone()];
        if local.is_file() {
            files.push(local.clone());
        }
        guard(session, &files)?;
        let text = read(&shared)?;
        let local_text = if local.is_file() { Some(read(&local)?) } else { None };
        let entries = import::jetbrains(&text, local_text.as_deref()).map_err(|error| {
            refused(t!("database-error-import-read", file = crate::display(&shared), error = error))
        })?;
        let found = entries.into_iter().map(|entry| (shared.clone(), entry)).collect();
        run_import(session, params, OriginKind::Jetbrains, files, found)
    }
}

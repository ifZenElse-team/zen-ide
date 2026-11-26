use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::client::{ClientCommand, ClientError, PASSWORD_VARIABLES, PasswordWay, Tool, tools_for};
use zen_db::engine::{OperationKind, OperationStatus};
use zen_db::registry::Entry;
use zen_db::types::AgentAccess;
use zen_i18n::t;
use zen_secrets::Secret;
use zen_secrets::database::Kind as SecretKind;
use zen_terminal::Options;

use super::safety::{AGENT_NOT_ALLOWED, type_of};
use super::sources::{Database, find, has_password, secret_id, secret_store};
use crate::terminal::{Terminals, default_cwd};

pub const CLIENT_NOT_FOUND: &str = "clientNotFound";

pub const NO_CLIENT: &str = "noClient";

pub const KNOWN_FOLDERS: [&str; 12] = [
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/homebrew/opt/libpq/bin",
    "/usr/local/opt/libpq/bin",
    "/opt/homebrew/opt/mysql-client/bin",
    "/usr/local/opt/mysql-client/bin",
    "/opt/homebrew/opt/mariadb/bin",
    "/Applications/Postgres.app/Contents/Versions/latest/bin",
    "/usr/local/mysql/bin",
    "/opt/local/bin",
    "/usr/bin",
    "/bin",
];

#[derive(Clone, Debug, Default)]
pub struct ClientFolders(pub Option<Vec<PathBuf>>);

pub fn search_folders(session: &Session) -> Vec<PathBuf> {
    if let Some(folders) = session.state::<ClientFolders>().and_then(|folders| folders.0.clone()) {
        return folders;
    }
    let mut folders = absolute_folders(std::env::var_os("PATH").as_deref());
    for known in KNOWN_FOLDERS {
        let known = PathBuf::from(known);
        if !folders.contains(&known) {
            folders.push(known);
        }
    }
    folders
}

fn absolute_folders(path: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    path.iter().flat_map(std::env::split_paths).filter(|folder| folder.is_absolute()).collect()
}

fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file() || path.with_extension("exe").is_file()
    }
}

pub fn find_client(tools: &[Tool], folders: &[PathBuf]) -> Option<(Tool, PathBuf)> {
    tools.iter().find_map(|tool| {
        tool.names()
            .iter()
            .find_map(|name| folders.iter().map(|folder| folder.join(name)).find(|path| executable(path)))
            .map(|path| (*tool, path))
    })
}

fn client_error(error: &ClientError, entry: &Entry) -> CommandError {
    let name = entry.source.name.as_str();
    let text = match error {
        ClientError::Unsupported => t!("database-client-unsupported", name = name),
        ClientError::TunnelClosed => t!("database-client-tunnel-closed", name = name),
        ClientError::TunnelUnsupported => t!("database-client-tunnel-unsupported", name = name),
        ClientError::Sentinel => t!("database-client-sentinel", name = name),
        ClientError::NoFile => t!("database-client-no-file", name = name),
    };
    CommandError::with_code(NO_CLIENT, text)
}

pub fn client_of(session: &mut Session, entry: &Entry) -> Result<(ClientCommand, Option<PathBuf>), CommandError> {
    let tools = tools_for(&entry.source.driver);
    if tools.is_empty() {
        return Err(client_error(&ClientError::Unsupported, entry));
    }
    let found = find_client(tools, &search_folders(session));
    let tool = found.as_ref().map_or(tools[0], |(tool, _)| *tool);
    let via =
        session.state::<Database>().and_then(Database::started).and_then(|engine| engine.tunnel_open(&entry.source.id));
    let mut source = entry.source.clone();
    source.read_only = super::safety::read_only(session, entry);
    source.database = source.database.map(|database| super::files::resolve(session, &source.driver, database));
    let command = zen_db::client::build(&source, tool, via).map_err(|error| client_error(&error, entry))?;
    Ok((command, found.map(|(_, program)| program)))
}

/// Names a connection.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientParams {
    /// The connection's id.
    pub connection: String,
}

/// A connection's client command, without secrets.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientAnswer {
    /// The client and its arguments.
    #[serde(flatten)]
    pub command: ClientCommand,
    /// The client's program, when it is installed.
    pub program: Option<PathBuf>,
    /// The command as a line of a POSIX shell, without the password.
    pub command_line: String,
}

pub struct ClientCommandLine;

impl Command for ClientCommandLine {
    type Params = ClientParams;
    type Output = ClientAnswer;
    const ID: &'static str = "db.connection.clientCommand";
    const TITLE: &'static str = "Copy Client Command";
    const DESCRIPTION: &'static str = "Answers the command line of database connection `connection`'s command-line client (psql, mysql or mariadb, sqlite3, redis-cli or valkey-cli, clickhouse client): `tool`, `args`, the non-secret `env` (TLS settings), `passwordVia` (how the client gets its password: PGPASSFILE, --defaults-extra-file, --config-file or --askpass; the password itself is never answered), `program` when the client is installed, and `commandLine` for a POSIX shell. A connection through SSH uses its open tunnel; refused with code `noClient` when there is no client for it or its tunnel is closed. Never connects.";

    fn run(&self, session: &mut Session, params: ClientParams) -> Result<ClientAnswer, CommandError> {
        let entry = find(session, &params.connection)?;
        let (command, program) = client_of(session, &entry)?;
        let shown = program
            .as_ref()
            .map_or_else(|| command.tool.names()[0].to_owned(), |program| program.to_string_lossy().into_owned());
        let command_line = command.command_line(&shown);
        Ok(ClientAnswer { command, program, command_line })
    }
}

/// Opens a connection's client, with a password for this time only.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct OpenTerminalParams {
    /// The connection's id.
    pub connection: String,
    /// The password for this client only, when none is saved; put in the client's environment and never stored or answered.
    pub password: Option<String>,
}

/// Where the password of a started client came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PasswordFrom {
    /// The call's `password`.
    Given,
    /// The connected connection's.
    Connection,
    /// The saved one, read in the background.
    Saved,
    /// None: the client asks for it when the server wants one.
    Asked,
}

/// A client started, or about to start once its saved password is read.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOpened {
    /// The client.
    pub tool: Tool,
    /// Its program.
    pub program: PathBuf,
    /// The terminal session, once started; `terminal.session.read` reads it.
    pub session: Option<u64>,
    /// The operation reading the saved password, when the client starts after it (`db.operation.state`).
    pub operation: Option<u64>,
    /// Where the password comes from.
    pub password_from: PasswordFrom,
}

struct Launch {
    command: ClientCommand,
    program: PathBuf,
    cwd: PathBuf,
    name: String,
}

struct Waiting {
    operation: u64,
    password: Arc<Mutex<Option<Secret>>>,
    launch: Launch,
}

#[derive(Default)]
pub struct WaitingClients(Vec<Waiting>);

const SCRIPT: &str = "shell=\"$1\"; secret=\"$2\"; shift 2; \
    if [ -n \"$secret\" ]; then trap 'rm -f -- \"$secret\"' EXIT HUP TERM; fi; \
    \"$@\"; \
    if [ -n \"$secret\" ]; then rm -f -- \"$secret\"; fi; trap - EXIT HUP TERM; \
    unset PGPASSFILE PGPASSWORD MYSQL_PWD REDISCLI_AUTH CLICKHOUSE_PASSWORD; exec \"$shell\" -l";

fn options(launch: &Launch, shell: &Path, secret: Option<&Path>, asks: bool) -> Options {
    let file_name = launch.program.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let tool = launch.command.tool;
    let mut client: Vec<String> = vec![launch.program.to_string_lossy().into_owned()];
    client.extend(tool.leading_args(&file_name).iter().map(|arg| (*arg).to_owned()));
    let mut env = launch.command.env.clone();
    env.retain(|(name, _)| !PASSWORD_VARIABLES.contains(&name.as_str()) && name != "PGPASSFILE");
    match (tool.password_way(), secret) {
        (Some(PasswordWay::FileInEnv(name)), Some(secret)) => {
            env.push((name.to_owned(), secret.to_string_lossy().into_owned()));
        }
        (Some(PasswordWay::FileArg(option)), Some(secret)) => client.push(format!("{option}{}", secret.display())),
        (Some(PasswordWay::Asks(option)), _) if asks => client.push(option.to_owned()),
        _ => {}
    }
    client.extend(launch.command.args.iter().cloned());
    let mut options = if cfg!(unix) {
        let shell = shell.to_string_lossy().into_owned();
        let secret = secret.map(|secret| secret.to_string_lossy().into_owned()).unwrap_or_default();
        let mut args = vec!["-c".to_owned(), SCRIPT.to_owned(), "zen-db-client".to_owned(), shell, secret];
        args.extend(client);
        Options { program: Some(PathBuf::from("/bin/sh")), args, ..Options::shell(&launch.cwd) }
    } else {
        let program = PathBuf::from(client.remove(0));
        Options { program: Some(program), args: client, ..Options::shell(&launch.cwd) }
    };
    options.env = env;
    options
}

fn secret_file(tool: Tool, password: &Secret) -> Result<Option<PathBuf>, CommandError> {
    #[cfg(unix)]
    {
        let Some(text) = tool.password_file(password.expose()) else { return Ok(None) };
        super::private_file("zen-db-client", &text)
            .map(Some)
            .map_err(|error| CommandError::new(t!("database-client-secret-file", error = error.to_string())))
    }
    #[cfg(not(unix))]
    {
        let _ = (tool, password);
        Ok(None)
    }
}

fn start(session: &mut Session, launch: &Launch, password: Option<&Secret>, asks: bool) -> Result<u64, CommandError> {
    crate::terminal::restore_pending(session)?;
    let shell = session.state_mut::<Terminals>().shell_program(&launch.cwd);
    let secret = match password {
        Some(password) => secret_file(launch.command.tool, password)?,
        None => None,
    };
    let options = options(launch, &shell, secret.as_deref(), asks);
    let started = session.state_mut::<Terminals>().run_in_new_tab(options, launch.name.clone());
    if started.is_err()
        && let Some(secret) = &secret
    {
        let _ = std::fs::remove_file(secret);
    }
    started
}

pub struct OpenTerminal;

impl Command for OpenTerminal {
    type Params = OpenTerminalParams;
    type Output = TerminalOpened;
    const ID: &'static str = "db.connection.openTerminal";
    const TITLE: &'static str = "Open in Terminal";
    const DESCRIPTION: &'static str = "Starts database connection `connection`'s command-line client (psql, mysql or mariadb, sqlite3, redis-cli or valkey-cli, clickhouse client, found on PATH or where installers put it) in a new tab of the Terminal tool window, in the project's folder, with the connection's host, port, user, database and TLS settings; a connection through SSH uses its open tunnel (connect first). The password is never on the command line, in the client's environment or on the screen: `password` (this time only), else the connected connection's, else the saved one read in the background (the client starts when `operation` ends), goes into a new file only the user can read that the client is pointed at (PGPASSFILE, --defaults-extra-file, --config-file), removed when the client ends or its tab closes; redis-cli asks for it (--askpass), as every client does on Windows and without a password. Agents may open it only where their access is full and the connection is neither read-only nor asks before changes (code `agentNotAllowed`), and never with a saved password. Refused with code `clientNotFound` when the client is not installed, `noClient` when there is none for the connection. Never connects the connection itself.";

    fn run(&self, session: &mut Session, params: OpenTerminalParams) -> Result<TerminalOpened, CommandError> {
        let entry = find(session, &params.connection)?;
        let by_agent = zen_core::macros::by_agent(session);
        if by_agent {
            let kind = type_of(session, &entry);
            let guarded = kind.confirm_execute || super::safety::read_only(session, &entry);
            if kind.agent_access() == AgentAccess::Full && guarded {
                return Err(CommandError::with_code(
                    AGENT_NOT_ALLOWED,
                    t!("database-client-agent-guarded", name = entry.source.name.as_str()),
                ));
            }
            if kind.agent_access() != AgentAccess::Full {
                return Err(CommandError::with_code(
                    AGENT_NOT_ALLOWED,
                    t!(
                        "database-error-agent-not-allowed",
                        name = entry.source.name.as_str(),
                        kind = kind.name.as_str()
                    ),
                ));
            }
        }
        let (command, program) = client_of(session, &entry)?;
        let program = program.ok_or_else(|| {
            CommandError::with_code(
                CLIENT_NOT_FOUND,
                t!(
                    "database-client-not-found",
                    tool = command.tool.names().join(", "),
                    name = entry.source.name.as_str()
                ),
            )
        })?;
        let tool = command.tool;
        let name = t!("database-client-tab", tool = command.tool.names()[0], name = entry.source.name.as_str());
        let launch = Launch { command, program: program.clone(), cwd: default_cwd(session), name };
        let way = launch.command.tool.password_way();
        let wants = cfg!(unix) && matches!(way, Some(PasswordWay::FileInEnv(_) | PasswordWay::FileArg(_)));
        let given = params.password.filter(|password| !password.is_empty()).map(Secret::new);
        let engine = session.state::<Database>().and_then(Database::started).cloned();
        let connected = engine.as_ref().and_then(|engine| engine.password(&entry.source.id)).filter(|_| !by_agent);
        let asks = matches!(way, Some(PasswordWay::Asks(_)))
            && (given.is_some() || connected.is_some() || has_password(&entry));
        let (password, from) = match (given, connected) {
            _ if !wants => (None, PasswordFrom::Asked),
            (Some(given), _) => (Some(given), PasswordFrom::Given),
            (None, Some(connected)) => (Some(connected), PasswordFrom::Connection),
            (None, None) if has_password(&entry) && !by_agent => {
                let operation = read_saved(session, &entry, launch)?;
                return Ok(TerminalOpened {
                    tool,
                    program,
                    session: None,
                    operation: Some(operation),
                    password_from: PasswordFrom::Saved,
                });
            }
            (None, None) => (None, PasswordFrom::Asked),
        };
        let id = start(session, &launch, password.as_ref(), asks)?;
        Ok(TerminalOpened { tool, program, session: Some(id), operation: None, password_from: from })
    }
}

fn read_saved(session: &mut Session, entry: &Entry, launch: Launch) -> Result<u64, CommandError> {
    let store = secret_store(session)?;
    let id = secret_id(session, entry, SecretKind::Password)?;
    let password = Arc::new(Mutex::new(None));
    let slot = Arc::clone(&password);
    let connection = entry.source.id.clone();
    let operation = super::sources::secret_work(session, OperationKind::ReadSecret, &entry.source.id, move || {
        let secret = store.get(&id).map_err(|error| super::sources::secret_text(&error, &connection))?;
        *slot.lock().unwrap_or_else(PoisonError::into_inner) = secret;
        Ok(())
    });
    session.state_mut::<WaitingClients>().0.push(Waiting { operation, password, launch });
    Ok(operation)
}

pub fn step(session: &mut Session) {
    if session.state::<WaitingClients>().is_none_or(|waiting| waiting.0.is_empty()) {
        return;
    }
    let Some(engine) = session.state::<Database>().and_then(Database::started).cloned() else { return };
    let waiting = std::mem::take(&mut session.state_mut::<WaitingClients>().0);
    let mut still = Vec::new();
    for client in waiting {
        let status = engine.operation(client.operation).map(|state| state.status);
        match status {
            Some(OperationStatus::Running) => still.push(client),
            Some(OperationStatus::Done) => {
                let password = client.password.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Err(error) = start(session, &client.launch, password.as_ref(), false) {
                    let body = error.message;
                    crate::notifications::post(
                        session,
                        crate::notifications::NewNotification::message(
                            crate::notifications::Group::General,
                            crate::notifications::Kind::Warning,
                            body,
                        ),
                    );
                }
            }
            _ => {}
        }
    }
    session.state_mut::<WaitingClients>().0.extend(still);
}

#[cfg(test)]
mod tests;

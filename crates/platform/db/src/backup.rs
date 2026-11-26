use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{TlsMode, TlsSettings};

pub mod find;

pub use find::{
    Container, Detection, DockerState, Found, Origin, Probe, VersionProblem, choose, detect, for_server, outside,
};

pub const NO_TOOL: &str = "noTool";
pub const UNSUPPORTED: &str = "unsupported";
pub const NOT_IN_CONTAINER: &str = "notInContainer";

/// The kind of server a connection is, as its tools see it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Family {
    /// PostgreSQL.
    Postgres,
    /// MySQL.
    Mysql,
    /// MariaDB.
    Mariadb,
}

impl Family {
    pub fn of(driver: &str) -> Option<Family> {
        match driver {
            crate::postgres::ID => Some(Family::Postgres),
            crate::mysql::ID => Some(Family::Mysql),
            "mariadb" => Some(Family::Mariadb),
            _ => None,
        }
    }

    pub fn tools(self) -> &'static [Tool] {
        match self {
            Family::Postgres => &[Tool::PgDump, Tool::PgRestore, Tool::Psql],
            Family::Mysql | Family::Mariadb => &[Tool::Mysqldump, Tool::Mysql],
        }
    }

    pub fn backup_tool(self) -> Tool {
        match self {
            Family::Postgres => Tool::PgDump,
            Family::Mysql | Family::Mariadb => Tool::Mysqldump,
        }
    }

    pub fn default_port(self) -> u16 {
        match self {
            Family::Postgres => 5432,
            Family::Mysql | Family::Mariadb => 3306,
        }
    }
}

/// A tool of a family.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    /// `pg_dump`: writes a PostgreSQL backup.
    PgDump,
    /// `pg_restore`: restores a custom, directory or tar backup.
    PgRestore,
    /// `psql`: restores a plain SQL backup.
    Psql,
    /// `mysqldump` or `mariadb-dump`: writes a MySQL or MariaDB backup.
    Mysqldump,
    /// `mysql` or `mariadb`: restores a MySQL or MariaDB backup.
    Mysql,
}

impl Tool {
    pub fn programs(self, family: Family) -> &'static [&'static str] {
        match (self, family) {
            (Tool::PgDump, _) => &["pg_dump"],
            (Tool::PgRestore, _) => &["pg_restore"],
            (Tool::Psql, _) => &["psql"],
            (Tool::Mysqldump, Family::Mariadb) => &["mariadb-dump", "mysqldump"],
            (Tool::Mysqldump, _) => &["mysqldump", "mariadb-dump"],
            (Tool::Mysql, Family::Mariadb) => &["mariadb", "mysql"],
            (Tool::Mysql, _) => &["mysql", "mariadb"],
        }
    }

    pub fn password_way(self) -> crate::client::PasswordWay {
        match self {
            Tool::PgDump | Tool::PgRestore | Tool::Psql => crate::client::PasswordWay::FileInEnv("PGPASSFILE"),
            Tool::Mysqldump | Tool::Mysql => crate::client::PasswordWay::FileArg("--defaults-extra-file="),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Tool::PgDump => "pg_dump",
            Tool::PgRestore => "pg_restore",
            Tool::Psql => "psql",
            Tool::Mysqldump => "mysqldump",
            Tool::Mysql => "mysql",
        }
    }
}

/// What a backup or restore holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Content {
    /// The structure and the data.
    #[default]
    All,
    /// The structure only.
    SchemaOnly,
    /// The data only.
    DataOnly,
}

/// The format of a PostgreSQL backup.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PgFormat {
    /// A plain SQL script, restored with `psql`.
    Plain,
    /// pg_dump's compressed archive, restored with `pg_restore`.
    #[default]
    Custom,
    /// A folder with a file per table, written and restored in parallel.
    Directory,
    /// A tar archive.
    Tar,
}

impl PgFormat {
    pub fn extension(self) -> &'static str {
        match self {
            PgFormat::Plain => "sql",
            PgFormat::Custom => "dump",
            PgFormat::Directory => "",
            PgFormat::Tar => "tar",
        }
    }

    fn letter(self) -> &'static str {
        match self {
            PgFormat::Plain => "p",
            PgFormat::Custom => "c",
            PgFormat::Directory => "d",
            PgFormat::Tar => "t",
        }
    }
}

/// How `mysqldump` keeps the backup consistent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DumpMethod {
    /// One transaction (`--single-transaction`): consistent for InnoDB without locking.
    #[default]
    SingleTransaction,
    /// Tables locked while they are read (`--lock-tables`).
    LockTables,
    /// Neither.
    None,
}

/// The options of `pg_dump`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PgDumpOptions {
    /// The format.
    pub format: PgFormat,
    /// The compression level, 0 to 9; the tool's default when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression: Option<u8>,
    /// What it holds.
    pub content: Content,
    /// Only these schemas (`--schema`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub schemas: Vec<String>,
    /// Only these tables (`--table`), as `schema.table` or patterns.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<String>,
    /// Not these tables (`--exclude-table`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_tables: Vec<String>,
    /// Rows as `INSERT` statements instead of `COPY` (`--inserts`).
    pub inserts: bool,
    /// `DROP` statements before each object (`--clean`).
    pub clean: bool,
    /// `IF EXISTS` on the `DROP` statements (`--if-exists`, with `clean`).
    pub if_exists: bool,
    /// A `CREATE DATABASE` statement first (`--create`).
    pub create: bool,
    /// No ownership statements (`--no-owner`).
    pub no_owner: bool,
    /// No `GRANT` or `REVOKE` statements (`--no-privileges`).
    pub no_privileges: bool,
    /// The encoding of the backup (`--encoding`); the database's when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
    /// Tables read in parallel, for the directory format (`--jobs`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jobs: Option<u8>,
    /// Progress written to the console (`--verbose`).
    pub verbose: bool,
    /// More arguments, one per item, added before the database.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

impl Default for PgDumpOptions {
    fn default() -> Self {
        Self {
            format: PgFormat::Custom,
            compression: None,
            content: Content::All,
            schemas: Vec::new(),
            tables: Vec::new(),
            exclude_tables: Vec::new(),
            inserts: false,
            clean: false,
            if_exists: false,
            create: false,
            no_owner: false,
            no_privileges: false,
            encoding: None,
            jobs: None,
            verbose: true,
            extra_args: Vec::new(),
        }
    }
}

/// The options of `mysqldump`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MysqldumpOptions {
    /// What it holds.
    pub content: Content,
    /// Only these tables of the database.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tables: Vec<String>,
    /// How the backup stays consistent.
    pub method: DumpMethod,
    /// `DROP TABLE` before each `CREATE TABLE` (`--add-drop-table`).
    pub add_drop_table: bool,
    /// Stored procedures and functions (`--routines`).
    pub routines: bool,
    /// Events (`--events`).
    pub events: bool,
    /// Triggers (`--triggers`).
    pub triggers: bool,
    /// Several rows in each `INSERT` (`--extended-insert`).
    pub extended_insert: bool,
    /// Column names in each `INSERT` (`--complete-insert`).
    pub complete_insert: bool,
    /// Binary columns written in hexadecimal (`--hex-blob`).
    pub hex_blob: bool,
    /// Comments in the backup (`--comments`).
    pub comments: bool,
    /// Progress written to the console (`--verbose`).
    pub verbose: bool,
    /// More arguments, one per item, added before the database.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

impl Default for MysqldumpOptions {
    fn default() -> Self {
        Self {
            content: Content::All,
            tables: Vec::new(),
            method: DumpMethod::SingleTransaction,
            add_drop_table: true,
            routines: true,
            events: false,
            triggers: true,
            extended_insert: true,
            complete_insert: false,
            hex_blob: false,
            comments: true,
            verbose: true,
            extra_args: Vec::new(),
        }
    }
}

/// The options of `pg_restore`; `psql` takes `singleTransaction`, `exitOnError` and `extraArgs`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PgRestoreOptions {
    /// What is restored.
    pub content: Content,
    /// Objects dropped before they are created again (`--clean`).
    pub clean: bool,
    /// `IF EXISTS` on the drops (`--if-exists`, with `clean`).
    pub if_exists: bool,
    /// The database created first and restored into (`--create`); the connection goes to `postgres` then.
    pub create: bool,
    /// No ownership statements (`--no-owner`).
    pub no_owner: bool,
    /// No `GRANT` or `REVOKE` statements (`--no-privileges`).
    pub no_privileges: bool,
    /// Everything in one transaction (`--single-transaction`).
    pub single_transaction: bool,
    /// Stop at the first error (`--exit-on-error`, psql's `ON_ERROR_STOP`).
    pub exit_on_error: bool,
    /// Objects restored in parallel (`--jobs`), not with one transaction or from a container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jobs: Option<u8>,
    /// Progress written to the console (`--verbose`).
    pub verbose: bool,
    /// More arguments, one per item.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

impl Default for PgRestoreOptions {
    fn default() -> Self {
        Self {
            content: Content::All,
            clean: false,
            if_exists: false,
            create: false,
            no_owner: false,
            no_privileges: false,
            single_transaction: false,
            exit_on_error: true,
            jobs: None,
            verbose: true,
            extra_args: Vec::new(),
        }
    }
}

/// The options of `mysql` restoring a backup.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MysqlRestoreOptions {
    /// Go on after an error (`--force`).
    pub force: bool,
    /// More arguments, one per item.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Target {
    pub family: Option<Family>,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: Option<String>,
    pub tls: TlsSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub password_way: crate::client::PasswordWay,
    pub tool_at: usize,
    pub container: Option<String>,
    pub stdin: Option<PathBuf>,
    pub stdout: Option<PathBuf>,
}

impl Invocation {
    pub fn display(&self) -> String {
        let mut words: Vec<String> = self.env.iter().map(|(name, value)| quoted(&format!("{name}={value}"))).collect();
        words.push(quoted(&self.program.display().to_string()));
        words.extend(self.args.iter().map(|arg| quoted(arg)));
        if let Some(file) = &self.stdin {
            words.push(format!("< {}", quoted(&file.display().to_string())));
        }
        if let Some(file) = &self.stdout {
            words.push(format!("> {}", quoted(&file.display().to_string())));
        }
        words.join(" ")
    }

    pub fn shell(&self) -> (PathBuf, Vec<String>, Vec<(String, String)>) {
        if self.stdin.is_none() && self.stdout.is_none() {
            return (self.program.clone(), self.args.clone(), self.env.clone());
        }
        let mut script = String::from("exec \"$@\"");
        let mut env = self.env.clone();
        if let Some(file) = &self.stdin {
            script.push_str(" < \"$ZEN_BACKUP_IN\"");
            env.push(("ZEN_BACKUP_IN".to_owned(), file.display().to_string()));
        }
        if let Some(file) = &self.stdout {
            script.push_str(" > \"$ZEN_BACKUP_OUT\"");
            env.push(("ZEN_BACKUP_OUT".to_owned(), file.display().to_string()));
        }
        let mut args = vec!["-c".to_owned(), script, "zen-backup".to_owned(), self.program.display().to_string()];
        args.extend(self.args.iter().cloned());
        (PathBuf::from("/bin/sh"), args, env)
    }

    pub fn with_password_file(&mut self, file: &str) {
        use crate::client::PasswordWay;
        match self.password_way {
            PasswordWay::FileInEnv(name) => match &self.container {
                Some(_) => {
                    let at = self.tool_at.saturating_sub(2);
                    self.args.insert(at, format!("{name}={file}"));
                    self.args.insert(at, "-e".to_owned());
                    self.tool_at += 2;
                }
                None => self.env.push((name.to_owned(), file.to_owned())),
            },
            PasswordWay::FileArg(option) => self.args.insert(self.tool_at, format!("{option}{file}")),
            PasswordWay::Asks(_) => {}
        }
    }

    pub fn password_file(&self, password: &str) -> Option<String> {
        use crate::client::{PasswordWay, Tool as Client};
        let client = match self.password_way {
            PasswordWay::FileInEnv(_) => Client::Psql,
            PasswordWay::FileArg(_) => Client::Mysql,
            PasswordWay::Asks(_) => return None,
        };
        client.password_file(password)
    }
}

pub fn quoted(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | ',' | '+' | '@' | '%'));
    if plain { word.to_owned() } else { format!("'{}'", word.replace('\'', r"'\''")) }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanError {
    pub code: &'static str,
    pub detail: String,
}

impl PlanError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }
}

fn tls_mode_name(mode: TlsMode) -> &'static str {
    match mode {
        TlsMode::Disable => "disable",
        TlsMode::Prefer => "prefer",
        TlsMode::Require => "require",
        TlsMode::VerifyCa => "verify-ca",
        TlsMode::VerifyFull => "verify-full",
    }
}

fn host_of(target: &Target) -> String {
    let host = target.host.trim();
    if host.is_empty() { "localhost".to_owned() } else { host.to_owned() }
}

struct Connection {
    host: String,
    port: u16,
    env: Vec<(String, String)>,
    docker: Option<Container>,
}

fn connection(found: &Found, target: &Target) -> Connection {
    match &found.container {
        Some(container) => Connection {
            host: "127.0.0.1".to_owned(),
            port: container.port,
            env: Vec::new(),
            docker: Some(container.clone()),
        },
        None => {
            let mut env = Vec::new();
            if matches!(target.family, Some(Family::Postgres)) {
                env.push(("PGSSLMODE".to_owned(), tls_mode_name(target.tls.mode).to_owned()));
                let files = [
                    ("PGSSLROOTCERT", &target.tls.ca_file),
                    ("PGSSLCERT", &target.tls.client_cert),
                    ("PGSSLKEY", &target.tls.client_key),
                ];
                for (name, file) in files {
                    if let Some(file) = file {
                        env.push((name.to_owned(), file.display().to_string()));
                    }
                }
            }
            Connection { host: host_of(target), port: target.port, env, docker: None }
        }
    }
}

fn pg_connection_args(connection: &Connection, target: &Target, database: &str) -> Vec<String> {
    let mut args = vec![format!("--host={}", connection.host), format!("--port={}", connection.port)];
    if let Some(user) = target.user.as_deref().filter(|user| !user.is_empty()) {
        args.push(format!("--username={user}"));
    }
    args.push("--no-password".to_owned());
    args.push(format!("--dbname=dbname='{}'", database.replace('\\', "\\\\").replace('\'', "\\'")));
    args
}

fn mariadb_client(found: &Found) -> bool {
    found.version.as_deref().is_some_and(|version| version.contains("MariaDB"))
        || found.program.file_name().is_some_and(|name| name.to_string_lossy().starts_with("mariadb"))
}

fn mysql_connection_args(found: &Found, connection: &Connection, target: &Target) -> Vec<String> {
    let mut args =
        vec![format!("--host={}", connection.host), format!("--port={}", connection.port), "--protocol=TCP".to_owned()];
    if let Some(user) = target.user.as_deref().filter(|user| !user.is_empty()) {
        args.push(format!("--user={user}"));
    }
    if connection.docker.is_none() {
        let mode = target.tls.mode;
        if mariadb_client(found) {
            match mode {
                TlsMode::Disable => args.push("--skip-ssl".to_owned()),
                TlsMode::Prefer => {}
                TlsMode::Require => args.push("--ssl".to_owned()),
                TlsMode::VerifyCa | TlsMode::VerifyFull => {
                    args.push("--ssl".to_owned());
                    args.push("--ssl-verify-server-cert".to_owned());
                }
            }
        } else {
            let named = match mode {
                TlsMode::Disable => "DISABLED",
                TlsMode::Prefer => "PREFERRED",
                TlsMode::Require => "REQUIRED",
                TlsMode::VerifyCa => "VERIFY_CA",
                TlsMode::VerifyFull => "VERIFY_IDENTITY",
            };
            args.push(format!("--ssl-mode={named}"));
        }
        let files = [
            ("--ssl-ca", &target.tls.ca_file),
            ("--ssl-cert", &target.tls.client_cert),
            ("--ssl-key", &target.tls.client_key),
        ];
        for (flag, file) in files {
            if let Some(file) = file {
                args.push(format!("{flag}={}", file.display()));
            }
        }
    }
    args
}

fn in_docker(
    found: &Found,
    connection: &Connection,
    tool_args: Vec<String>,
    stdin: bool,
) -> (PathBuf, Vec<String>, usize, Option<String>) {
    match &connection.docker {
        Some(container) => {
            let mut args = vec!["exec".to_owned()];
            if stdin {
                args.push("-i".to_owned());
            }
            args.push(container.id.clone());
            args.push(found.program.display().to_string());
            let at = args.len();
            args.extend(tool_args);
            (found.docker.clone().unwrap_or_else(|| PathBuf::from("docker")), args, at, Some(container.id.clone()))
        }
        None => (found.program.clone(), tool_args, 0, None),
    }
}

pub const BAD_NAME: &str = "badName";

fn plain_names<'a>(names: impl IntoIterator<Item = &'a String>) -> Result<(), PlanError> {
    match names.into_iter().find(|name| name.starts_with('-')) {
        Some(name) => Err(PlanError::new(BAD_NAME, format!("{name} would be read as an option"))),
        None => Ok(()),
    }
}

fn content_flags(content: Content) -> Option<&'static str> {
    match content {
        Content::All => None,
        Content::SchemaOnly => Some("--schema-only"),
        Content::DataOnly => Some("--data-only"),
    }
}

pub fn backup(
    found: &Found,
    target: &Target,
    pg: &PgDumpOptions,
    my: &MysqldumpOptions,
    file: &Path,
) -> Result<Invocation, PlanError> {
    let connection = connection(found, target);
    let docker = connection.docker.is_some();
    let (tool_args, stdout) = match found.tool {
        Tool::PgDump => {
            if docker && pg.format == PgFormat::Directory {
                return Err(PlanError::new(
                    NOT_IN_CONTAINER,
                    "the directory format is written by a pg_dump on this computer",
                ));
            }
            let mut args = pg_connection_args(&connection, target, &target.database);
            args.push(format!("--format={}", pg.format.letter()));
            if let Some(level) = pg.compression {
                args.push(format!("--compress={}", level.min(9)));
            }
            args.extend(content_flags(pg.content).map(str::to_owned));
            for schema in &pg.schemas {
                args.push(format!("--schema={schema}"));
            }
            for table in &pg.tables {
                args.push(format!("--table={table}"));
            }
            for table in &pg.exclude_tables {
                args.push(format!("--exclude-table={table}"));
            }
            let flags = [
                (pg.inserts, "--inserts"),
                (pg.clean, "--clean"),
                (pg.clean && pg.if_exists, "--if-exists"),
                (pg.create, "--create"),
                (pg.no_owner, "--no-owner"),
                (pg.no_privileges, "--no-privileges"),
                (pg.verbose, "--verbose"),
            ];
            args.extend(flags.iter().filter(|(on, _)| *on).map(|(_, flag)| (*flag).to_owned()));
            if let Some(encoding) = pg.encoding.as_deref().filter(|encoding| !encoding.trim().is_empty()) {
                args.push(format!("--encoding={}", encoding.trim()));
            }
            if pg.format == PgFormat::Directory
                && let Some(jobs) = pg.jobs.filter(|jobs| *jobs > 1)
            {
                args.push(format!("--jobs={jobs}"));
            }
            args.extend(pg.extra_args.iter().cloned());
            if docker {
                (args, Some(file.to_owned()))
            } else {
                args.push(format!("--file={}", file.display()));
                (args, None)
            }
        }
        Tool::Mysqldump => {
            let mut args = mysql_connection_args(found, &connection, target);
            match my.content {
                Content::All => {}
                Content::SchemaOnly => args.push("--no-data".to_owned()),
                Content::DataOnly => args.push("--no-create-info".to_owned()),
            }
            match my.method {
                DumpMethod::SingleTransaction => args.push("--single-transaction".to_owned()),
                DumpMethod::LockTables => args.push("--lock-tables".to_owned()),
                DumpMethod::None => args.push("--skip-lock-tables".to_owned()),
            }
            let switches = [
                (my.add_drop_table, "--add-drop-table", "--skip-add-drop-table"),
                (my.triggers, "--triggers", "--skip-triggers"),
                (my.extended_insert, "--extended-insert", "--skip-extended-insert"),
                (my.comments, "--comments", "--skip-comments"),
            ];
            for (on, yes, no) in switches {
                args.push(if on { yes } else { no }.to_owned());
            }
            let flags = [
                (my.routines, "--routines"),
                (my.events, "--events"),
                (my.complete_insert, "--complete-insert"),
                (my.hex_blob, "--hex-blob"),
                (my.verbose, "--verbose"),
            ];
            args.extend(flags.iter().filter(|(on, _)| *on).map(|(_, flag)| (*flag).to_owned()));
            if matches!(target.family, Some(Family::Mariadb)) && !mariadb_client(found) && found.major.unwrap_or(0) >= 8
            {
                args.push("--column-statistics=0".to_owned());
            }
            args.extend(my.extra_args.iter().cloned());
            if !docker {
                args.push(format!("--result-file={}", file.display()));
            }
            plain_names(std::iter::once(&target.database).chain(&my.tables))?;
            args.push(target.database.clone());
            args.extend(my.tables.iter().cloned());
            (args, docker.then(|| file.to_owned()))
        }
        other => return Err(PlanError::new(NO_TOOL, format!("{} does not write backups", other.name()))),
    };
    let (program, args, tool_at, container) = in_docker(found, &connection, tool_args, false);
    Ok(Invocation {
        program,
        args,
        env: connection.env,
        password_way: found.tool.password_way(),
        tool_at,
        container,
        stdin: None,
        stdout,
    })
}

pub fn restore(
    found: &Found,
    target: &Target,
    format: PgFormat,
    pg: &PgRestoreOptions,
    my: &MysqlRestoreOptions,
    file: &Path,
) -> Result<Invocation, PlanError> {
    let connection = connection(found, target);
    let docker = connection.docker.is_some();
    let (tool_args, stdin) = match found.tool {
        Tool::PgRestore => {
            if format == PgFormat::Plain {
                return Err(PlanError::new(NO_TOOL, "a plain SQL backup is restored with psql"));
            }
            if docker && format == PgFormat::Directory {
                return Err(PlanError::new(
                    NOT_IN_CONTAINER,
                    "a directory backup is restored by a pg_restore on this computer",
                ));
            }
            let database = if pg.create { "postgres" } else { target.database.as_str() };
            let mut args = pg_connection_args(&connection, target, database);
            args.extend(content_flags(pg.content).map(str::to_owned));
            let flags = [
                (pg.clean, "--clean"),
                (pg.clean && pg.if_exists, "--if-exists"),
                (pg.create, "--create"),
                (pg.no_owner, "--no-owner"),
                (pg.no_privileges, "--no-privileges"),
                (pg.single_transaction, "--single-transaction"),
                (pg.exit_on_error, "--exit-on-error"),
                (pg.verbose, "--verbose"),
            ];
            args.extend(flags.iter().filter(|(on, _)| *on).map(|(_, flag)| (*flag).to_owned()));
            if !docker
                && !pg.single_transaction
                && let Some(jobs) = pg.jobs.filter(|jobs| *jobs > 1)
            {
                args.push(format!("--jobs={jobs}"));
            }
            args.extend(pg.extra_args.iter().cloned());
            if docker {
                (args, Some(file.to_owned()))
            } else {
                args.push(file.display().to_string());
                (args, None)
            }
        }
        Tool::Psql => {
            let mut args = pg_connection_args(&connection, target, &target.database);
            if pg.exit_on_error {
                args.push("--set=ON_ERROR_STOP=1".to_owned());
            }
            if pg.single_transaction {
                args.push("--single-transaction".to_owned());
            }
            args.push("--quiet".to_owned());
            args.extend(pg.extra_args.iter().cloned());
            if docker {
                (args, Some(file.to_owned()))
            } else {
                args.push(format!("--file={}", file.display()));
                (args, None)
            }
        }
        Tool::Mysql => {
            let mut args = mysql_connection_args(found, &connection, target);
            if my.force {
                args.push("--force".to_owned());
            }
            args.extend(my.extra_args.iter().cloned());
            plain_names(std::iter::once(&target.database))?;
            args.push(target.database.clone());
            (args, Some(file.to_owned()))
        }
        other => return Err(PlanError::new(NO_TOOL, format!("{} does not restore backups", other.name()))),
    };
    let (program, args, tool_at, container) = in_docker(found, &connection, tool_args, stdin.is_some() && docker);
    Ok(Invocation {
        program,
        args,
        env: connection.env,
        password_way: found.tool.password_way(),
        tool_at,
        container,
        stdin,
        stdout: None,
    })
}

pub fn server_version(found: &Found, target: &Target) -> Result<Invocation, PlanError> {
    if found.tool != Tool::Psql {
        return Err(PlanError::new(NO_TOOL, format!("{} does not ask the server", found.tool.name())));
    }
    let connection = connection(found, target);
    let mut args = pg_connection_args(&connection, target, &target.database);
    args.extend(["-X", "-A", "-t", "-c", "SHOW server_version_num"].map(str::to_owned));
    let (program, args, tool_at, container) = in_docker(found, &connection, args, false);
    let mut env = connection.env;
    env.push(("PGCONNECT_TIMEOUT".to_owned(), "10".to_owned()));
    Ok(Invocation {
        program,
        args,
        env,
        password_way: found.tool.password_way(),
        tool_at,
        container,
        stdin: None,
        stdout: None,
    })
}

pub fn server_major(answer: &str) -> Option<u32> {
    let number: u32 = answer.trim().parse().ok()?;
    (number >= 10_000).then_some(number / 10_000)
}

pub fn pg_format(path: &Path) -> std::io::Result<PgFormat> {
    use std::io::Read;
    if path.is_dir() {
        return Ok(PgFormat::Directory);
    }
    let mut head = [0_u8; 512];
    let mut file = std::fs::File::open(path)?;
    let mut read = 0;
    while read < head.len() {
        let count = file.read(&mut head[read..])?;
        if count == 0 {
            break;
        }
        read += count;
    }
    if head[..read].starts_with(b"PGDMP") {
        return Ok(PgFormat::Custom);
    }
    if read >= 262 && &head[257..262] == b"ustar" {
        return Ok(PgFormat::Tar);
    }
    Ok(PgFormat::Plain)
}

pub fn restore_tool(family: Family, format: PgFormat) -> Tool {
    match family {
        Family::Postgres if format == PgFormat::Plain => Tool::Psql,
        Family::Postgres => Tool::PgRestore,
        Family::Mysql | Family::Mariadb => Tool::Mysql,
    }
}

pub fn file_name(database: &str, stamp: &str, family: Family, format: PgFormat) -> String {
    let base: String =
        database.chars().map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' }).collect();
    let base = base.trim_start_matches('.');
    let base = if base.is_empty() { "backup" } else { base };
    let extension = match family {
        Family::Postgres => format.extension(),
        Family::Mysql | Family::Mariadb => "sql",
    };
    if extension.is_empty() { format!("{base}_{stamp}") } else { format!("{base}_{stamp}.{extension}") }
}

#[cfg(test)]
mod tests;

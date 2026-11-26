use std::net::SocketAddr;

use schemars::JsonSchema;
use serde::Serialize;

use crate::registry::{DataSource, TlsMode};

/// A command-line client of a database.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Tool {
    /// PostgreSQL's `psql`.
    Psql,
    /// MySQL's `mysql`.
    Mysql,
    /// MariaDB's `mariadb`.
    Mariadb,
    /// SQLite's `sqlite3`.
    Sqlite3,
    /// Redis's and Valkey's `redis-cli`.
    RedisCli,
    /// ClickHouse's `clickhouse client`.
    ClickHouse,
}

impl Tool {
    pub fn names(self) -> &'static [&'static str] {
        match self {
            Tool::Psql => &["psql"],
            Tool::Mysql => &["mysql"],
            Tool::Mariadb => &["mariadb"],
            Tool::Sqlite3 => &["sqlite3"],
            Tool::RedisCli => &["redis-cli", "valkey-cli"],
            Tool::ClickHouse => &["clickhouse-client", "clickhouse"],
        }
    }

    pub fn password_way(self) -> Option<PasswordWay> {
        match self {
            Tool::Psql => Some(PasswordWay::FileInEnv("PGPASSFILE")),
            Tool::Mysql | Tool::Mariadb => Some(PasswordWay::FileArg("--defaults-extra-file=")),
            Tool::ClickHouse => Some(PasswordWay::FileArg("--config-file=")),
            Tool::RedisCli => Some(PasswordWay::Asks("--askpass")),
            Tool::Sqlite3 => None,
        }
    }

    pub fn password_file(self, password: &str) -> Option<String> {
        match self {
            Tool::Psql => {
                if password.contains(['\n', '\r']) {
                    return None;
                }
                Some(format!("*:*:*:*:{}\n", password.replace('\\', "\\\\").replace(':', "\\:")))
            }
            Tool::Mysql | Tool::Mariadb => {
                let escaped =
                    password.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t");
                Some(format!("[client]\npassword=\"{escaped}\"\n"))
            }
            Tool::ClickHouse => {
                let escaped = password
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
                    .replace('"', "&quot;")
                    .replace('\'', "&apos;");
                Some(format!("<config>\n    <password>{escaped}</password>\n</config>\n"))
            }
            Tool::Sqlite3 | Tool::RedisCli => None,
        }
    }

    pub fn leading_args(self, file_name: &str) -> &'static [&'static str] {
        if self == Tool::ClickHouse && file_name == "clickhouse" { &["client"] } else { &[] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordWay {
    FileInEnv(&'static str),
    FileArg(&'static str),
    Asks(&'static str),
}

impl Default for PasswordWay {
    fn default() -> Self {
        PasswordWay::FileInEnv("PGPASSFILE")
    }
}

impl PasswordWay {
    pub fn name(self) -> &'static str {
        match self {
            PasswordWay::FileInEnv(name) | PasswordWay::Asks(name) => name,
            PasswordWay::FileArg(option) => option.trim_end_matches('='),
        }
    }
}

pub const PASSWORD_VARIABLES: [&str; 4] = ["PGPASSWORD", "MYSQL_PWD", "REDISCLI_AUTH", "CLICKHOUSE_PASSWORD"];

pub fn tools_for(driver: &str) -> &'static [Tool] {
    match driver {
        "postgresql" => &[Tool::Psql],
        "mysql" => &[Tool::Mysql, Tool::Mariadb],
        "mariadb" => &[Tool::Mariadb, Tool::Mysql],
        "sqlite" => &[Tool::Sqlite3],
        "redis" => &[Tool::RedisCli],
        "clickhouse" => &[Tool::ClickHouse],
        _ => &[],
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    Unsupported,
    TunnelClosed,
    TunnelUnsupported,
    Sentinel,
    NoFile,
}

/// A client started for a connection: arguments and environment, never the password.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientCommand {
    /// The client.
    pub tool: Tool,
    /// Its arguments after the program.
    pub args: Vec<String>,
    /// Environment variables it needs that are not secret (TLS settings).
    pub env: Vec<(String, String)>,
    /// How the client gets its password, when it takes one: `PGPASSFILE` (a file named by that
    /// variable), `--defaults-extra-file` or `--config-file` (a file named by that option), or
    /// `--askpass` (it asks). Never the password itself.
    pub password_via: Option<String>,
}

impl ClientCommand {
    pub fn command_line(&self, program: &str) -> String {
        let mut words: Vec<String> =
            self.env.iter().map(|(name, value)| format!("{name}={}", shell_quote(value))).collect();
        words.push(shell_quote(program));
        let file_name = program.rsplit('/').next().unwrap_or(program);
        words.extend(self.tool.leading_args(file_name).iter().map(|arg| (*arg).to_owned()));
        words.extend(self.args.iter().map(|arg| shell_quote(arg)));
        words.join(" ")
    }
}

pub fn shell_quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '=' | ',' | '@' | '+'));
    if plain { text.to_owned() } else { format!("'{}'", text.replace('\'', "'\\''")) }
}

pub fn build(source: &DataSource, tool: Tool, via: Option<SocketAddr>) -> Result<ClientCommand, ClientError> {
    let tunnelled = source.ssh.as_ref().is_some_and(|ssh| !ssh.host.trim().is_empty());
    let via = if tunnelled {
        match tool {
            Tool::Sqlite3 => None,
            Tool::ClickHouse => return Err(ClientError::TunnelUnsupported),
            _ => Some(via.ok_or(ClientError::TunnelClosed)?),
        }
    } else {
        None
    };
    let mut command = ClientCommand {
        tool,
        args: Vec::new(),
        env: Vec::new(),
        password_via: tool.password_way().map(|way| way.name().to_owned()),
    };
    let text =
        |value: &Option<String>| value.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned);
    let host = via.map(|via| via.ip().to_string()).or_else(|| text(&source.host));
    let port = via.map(|via| via.port()).or(source.port);
    let user = text(&source.user);
    let database = text(&source.database);
    let args = &mut command.args;
    match tool {
        Tool::Psql => {
            args.extend(host.map(|host| format!("--host={host}")));
            args.extend(port.map(|port| format!("--port={port}")));
            args.extend(user.map(|user| format!("--username={user}")));
            args.extend(database.map(|database| format!("--dbname={database}")));
            let env = &mut command.env;
            env.push(("PGSSLMODE".to_owned(), libpq_mode(source.tls.mode).to_owned()));
            let files = [
                ("PGSSLROOTCERT", &source.tls.ca_file),
                ("PGSSLCERT", &source.tls.client_cert),
                ("PGSSLKEY", &source.tls.client_key),
            ];
            for (name, file) in files {
                if let Some(file) = file {
                    env.push((name.to_owned(), file.to_string_lossy().into_owned()));
                }
            }
            if source.tls.direct {
                env.push(("PGSSLNEGOTIATION".to_owned(), "direct".to_owned()));
            }
            if source.read_only {
                env.push(("PGOPTIONS".to_owned(), "-c default_transaction_read_only=on".to_owned()));
            }
        }
        Tool::Mysql | Tool::Mariadb => {
            args.push("--protocol=TCP".to_owned());
            args.push(format!("--host={}", host.unwrap_or_else(|| "localhost".to_owned())));
            args.extend(port.map(|port| format!("--port={port}")));
            args.extend(user.map(|user| format!("--user={user}")));
            args.extend(database.map(|database| format!("--database={database}")));
            let mode = source.tls.mode;
            if tool == Tool::Mysql {
                let named = match mode {
                    TlsMode::Disable => "DISABLED",
                    TlsMode::Prefer => "PREFERRED",
                    TlsMode::Require => "REQUIRED",
                    TlsMode::VerifyCa => "VERIFY_CA",
                    TlsMode::VerifyFull => "VERIFY_IDENTITY",
                };
                args.push(format!("--ssl-mode={named}"));
            } else {
                match mode {
                    TlsMode::Disable => args.push("--skip-ssl".to_owned()),
                    TlsMode::Prefer => {}
                    TlsMode::Require => args.push("--ssl".to_owned()),
                    TlsMode::VerifyCa | TlsMode::VerifyFull => {
                        args.push("--ssl".to_owned());
                        args.push("--ssl-verify-server-cert".to_owned());
                    }
                }
            }
            tls_files(args, source, ("--ssl-ca", "--ssl-cert", "--ssl-key"), true);
            if source.read_only {
                args.push("--init-command=SET SESSION TRANSACTION READ ONLY".to_owned());
            }
        }
        Tool::Sqlite3 => {
            let file = database.ok_or(ClientError::NoFile)?;
            if source.read_only {
                args.push("-readonly".to_owned());
            }
            args.push(if file.starts_with('-') { format!("./{file}") } else { file });
        }
        Tool::RedisCli => {
            let target = crate::redis::target(source).map_err(|_| ClientError::Unsupported)?;
            if target.mode == crate::redis::Mode::Sentinel {
                return Err(ClientError::Sentinel);
            }
            let (node_host, node_port) = target.nodes.first().cloned().unwrap_or(("localhost".to_owned(), 6379));
            let (host, port) = via.map_or((node_host, node_port), |via| (via.ip().to_string(), via.port()));
            args.extend(["-h".to_owned(), host, "-p".to_owned(), port.to_string()]);
            if target.mode == crate::redis::Mode::Cluster {
                args.push("-c".to_owned());
            }
            if let Some(user) = user {
                args.extend(["--user".to_owned(), user]);
            }
            if target.db != 0 {
                args.extend(["-n".to_owned(), target.db.to_string()]);
            }
            if target.tls {
                args.push("--tls".to_owned());
                if source.tls.mode == TlsMode::Require {
                    args.push("--insecure".to_owned());
                }
                tls_files(args, source, ("--cacert", "--cert", "--key"), false);
            }
        }
        Tool::ClickHouse => {
            args.extend(host.map(|host| format!("--host={host}")));
            args.extend(user.map(|user| format!("--user={user}")));
            args.extend(database.map(|database| format!("--database={database}")));
            if clickhouse_secure(source) {
                args.push("--secure".to_owned());
            }
            if source.read_only {
                args.push("--readonly=1".to_owned());
            }
        }
    }
    Ok(command)
}

fn libpq_mode(mode: TlsMode) -> &'static str {
    match mode {
        TlsMode::Disable => "disable",
        TlsMode::Prefer => "prefer",
        TlsMode::Require => "require",
        TlsMode::VerifyCa => "verify-ca",
        TlsMode::VerifyFull => "verify-full",
    }
}

fn tls_files(args: &mut Vec<String>, source: &DataSource, (ca, cert, key): (&str, &str, &str), joined: bool) {
    let files = [(ca, &source.tls.ca_file), (cert, &source.tls.client_cert), (key, &source.tls.client_key)];
    for (option, file) in files {
        let Some(file) = file else { continue };
        let file = file.to_string_lossy().into_owned();
        if joined {
            args.push(format!("{option}={file}"));
        } else {
            args.extend([option.to_owned(), file]);
        }
    }
}

fn clickhouse_secure(source: &DataSource) -> bool {
    let scheme =
        source.url.as_deref().and_then(|url| url.split_once("://")).map(|(scheme, _)| scheme).unwrap_or_default();
    scheme.ends_with("https")
        || source.settings.get("protocol").and_then(serde_json::Value::as_str) == Some("https")
        || matches!(source.tls.mode, TlsMode::Require | TlsMode::VerifyCa | TlsMode::VerifyFull)
}

#[cfg(test)]
mod tests;

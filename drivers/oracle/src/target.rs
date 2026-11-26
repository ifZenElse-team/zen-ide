use std::path::PathBuf;

use serde_json::Value as Json;
use zen_db_driver_kit::Failure;
use zen_db_driver_kit::tls::Mode;

use crate::program;

pub(crate) const DEFAULT_PORT: u64 = 1521;

pub(crate) const ROLES: &[&str] = &["normal", "SYSDBA", "SYSOPER", "SYSDG", "SYSBACKUP", "SYSKM"];

pub(crate) const CONNECTION_TYPES: &[&str] = &["serviceName", "sid", "tns", "custom"];

#[derive(Clone, Debug)]
pub(crate) struct Target {
    pub(crate) program: PathBuf,
    pub(crate) java_home: Option<PathBuf>,
    pub(crate) tns_admin: Option<PathBuf>,
    pub(crate) cloud_wallet: Option<String>,
    pub(crate) user: String,
    pub(crate) password: String,
    pub(crate) identifier: String,
    pub(crate) role: String,
    pub(crate) application: String,
}

fn setting<'a>(params: &'a Json, key: &str) -> Option<&'a str> {
    params.pointer(&format!("/settings/{key}")).and_then(Json::as_str).map(str::trim).filter(|value| !value.is_empty())
}

fn field<'a>(params: &'a Json, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Json::as_str).map(str::trim).filter(|value| !value.is_empty())
}

fn plain(value: &str, what: &str) -> Result<String, Failure> {
    let fits = |c: char| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '$' | '#' | ':' | '%');
    if value.chars().all(fits) {
        Ok(value.to_owned())
    } else {
        Err(Failure::params(format!("the {what} `{value}` holds characters a connect identifier cannot")))
    }
}

fn one_line(value: &str, what: &str) -> Result<String, Failure> {
    if value.contains(['\n', '\r', '"']) {
        Err(Failure::params(format!("the {what} cannot hold a line break or a double quote")))
    } else {
        Ok(value.to_owned())
    }
}

fn user_name(user: &str) -> Result<String, Failure> {
    let bare = user.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && user.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '#'));
    if bare {
        return Ok(user.to_owned());
    }
    Ok(format!("\"{}\"", one_line(user, "user name")?))
}

fn host_of(params: &Json) -> Result<String, Failure> {
    let host = plain(field(params, "host").unwrap_or("localhost"), "host")?;
    Ok(if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host })
}

fn descriptor(params: &Json, data: &str, verify_host: bool, wallet: Option<&str>) -> Result<String, Failure> {
    let host = host_of(params)?;
    let port = params.get("port").and_then(Json::as_u64).unwrap_or(DEFAULT_PORT);
    let mut security = format!("(SSL_SERVER_DN_MATCH={})", if verify_host { "ON" } else { "OFF" });
    if let Some(wallet) = wallet {
        if wallet.contains(['(', ')', ' ', '\t']) {
            return Err(Failure::params("the wallet folder's path cannot hold spaces or parentheses"));
        }
        security.push_str(&format!("(MY_WALLET_DIRECTORY={wallet})"));
    }
    Ok(format!(
        "(DESCRIPTION=(ADDRESS=(PROTOCOL=TCPS)(HOST={host})(PORT={port}))(CONNECT_DATA=({data}))(SECURITY={security}))"
    ))
}

impl Target {
    pub(crate) fn of(params: &Json) -> Result<Self, Failure> {
        let program = program::find_sqlcl(setting(params, "program"))?;
        let java_home = program::java_home(setting(params, "javaHome"))?;
        let user = field(params, "user").ok_or_else(|| Failure::database("authentication", "a user name is needed"))?;
        let password = params.get("password").and_then(Json::as_str).unwrap_or_default();
        if password.contains(['"', '\n', '\r']) {
            return Err(Failure::database(
                "authentication",
                "an Oracle password cannot hold a double quote or a line break",
            ));
        }
        let wallet = setting(params, "wallet").map(|wallet| one_line(wallet, "wallet")).transpose()?;
        let (cloud_wallet, wallet_folder) = match wallet {
            Some(wallet) if wallet.to_ascii_lowercase().ends_with(".zip") => (Some(wallet), None),
            other => (None, other),
        };
        let mode = Mode::of(params.get("tls").unwrap_or(&Json::Null));
        let database = field(params, "database");
        let kind = setting(params, "connectionType").unwrap_or("serviceName");
        let identifier = match kind {
            "sid" => {
                let sid = plain(database.ok_or_else(|| Failure::params("a SID is needed"))?, "SID")?;
                if mode.required() {
                    descriptor(params, &format!("SID={sid}"), mode == Mode::VerifyFull, wallet_folder.as_deref())?
                } else {
                    let port = params.get("port").and_then(Json::as_u64).unwrap_or(DEFAULT_PORT);
                    format!("{}:{port}:{sid}", host_of(params)?)
                }
            }
            "tns" => plain(
                setting(params, "tnsAlias").or(database).ok_or_else(|| Failure::params("a TNS alias is needed"))?,
                "TNS alias",
            )?,
            "custom" => one_line(
                setting(params, "connectString").ok_or_else(|| Failure::params("a connect string is needed"))?,
                "connect string",
            )?,
            _ => {
                let service = plain(database.unwrap_or("FREEPDB1"), "service name")?;
                if mode.required() {
                    descriptor(
                        params,
                        &format!("SERVICE_NAME={service}"),
                        mode == Mode::VerifyFull,
                        wallet_folder.as_deref(),
                    )?
                } else {
                    let port = params.get("port").and_then(Json::as_u64).unwrap_or(DEFAULT_PORT);
                    format!("//{}:{port}/{service}", host_of(params)?)
                }
            }
        };
        if identifier.contains(char::is_whitespace) {
            return Err(Failure::params("the connect string cannot hold spaces: write it without them"));
        }
        let role = match setting(params, "role") {
            None | Some("normal") => String::new(),
            Some(role) if ROLES.contains(&role) => format!(" AS {role}"),
            Some(role) => return Err(Failure::params(format!("`{role}` is not a role SQLcl connects as"))),
        };
        let application = field(params, "applicationName").unwrap_or("Zen IDE").replace('\'', "''");
        Ok(Self {
            program,
            java_home,
            tns_admin: setting(params, "tnsAdmin").map(PathBuf::from),
            cloud_wallet,
            user: user_name(user)?,
            password: password.to_owned(),
            identifier,
            role,
            application,
        })
    }

    pub(crate) fn connect_lines(&self) -> String {
        let mut lines = String::new();
        if let Some(wallet) = &self.cloud_wallet {
            lines.push_str(&format!("SET CLOUDCONFIG \"{wallet}\"\n"));
        }
        lines.push_str(&format!("CONNECT {}/\"{}\"@{}{}\n", self.user, self.password, self.identifier, self.role));
        lines
    }

    pub(crate) fn redact(&self, text: &str) -> String {
        if self.password.is_empty() { text.to_owned() } else { text.replace(&self.password, "********") }
    }
}

#[cfg(test)]
mod tests;

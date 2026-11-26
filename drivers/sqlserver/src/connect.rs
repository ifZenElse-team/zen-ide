use serde_json::Value as Json;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};
use zen_db_driver_kit::Failure;
use zen_db_driver_kit::tls::{self, Mode};

use crate::link::{Link, Wanted};

pub(crate) type Sql = Client<Compat<Link<TcpStream>>>;

pub(crate) const DEFAULT_PORT: u16 = 1433;

const SESSION_OPTIONS: &str = "SET ANSI_NULLS ON; SET ANSI_PADDING ON; SET ANSI_WARNINGS ON; \
SET CONCAT_NULL_YIELDS_NULL ON; SET QUOTED_IDENTIFIER ON; SET ARITHABORT ON; SET TEXTSIZE 2147483647;";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Login {
    Password,
    AccessToken,
}

pub(crate) struct Target {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) database: Option<String>,
    pub(crate) user: String,
    pub(crate) password: String,
    pub(crate) login: Login,
    pub(crate) tls: Json,
    pub(crate) strict: bool,
    pub(crate) application: String,
}

impl Target {
    pub(crate) fn of(params: &Json) -> Result<Self, Failure> {
        let text = |key: &str| params.get(key).and_then(Json::as_str).filter(|text| !text.is_empty());
        let setting = |key: &str| params.pointer(&format!("/settings/{key}"));
        let flag = |key: &str| {
            setting(key).is_some_and(|value| value.as_bool() == Some(true) || value.as_str() == Some("true"))
        };
        let host = text("host").unwrap_or("localhost").to_owned();
        let port =
            params.get("port").and_then(Json::as_u64).and_then(|port| u16::try_from(port).ok()).unwrap_or(DEFAULT_PORT);
        let login = match setting("authentication").and_then(Json::as_str) {
            Some("accessToken") => Login::AccessToken,
            _ => Login::Password,
        };
        let tls = params.get("tls").cloned().unwrap_or(Json::Null);
        Ok(Self {
            strict: tls::direct(&tls) || flag("strict"),
            host,
            port,
            database: text("database").map(str::to_owned),
            user: text("user").unwrap_or_default().to_owned(),
            password: text("password").unwrap_or_default().to_owned(),
            login,
            tls,
            application: text("applicationName").unwrap_or("Zen IDE").to_owned(),
        })
    }

    fn config(&self, host: &str, port: u16) -> Config {
        let mut config = Config::new();
        config.host(host);
        config.port(port);
        if let Some(database) = &self.database {
            config.database(database);
        }
        config.application_name(&self.application);
        config.authentication(match self.login {
            Login::Password => AuthMethod::sql_server(&self.user, &self.password),
            Login::AccessToken => AuthMethod::aad_token(&self.password),
        });
        config.encryption(EncryptionLevel::NotSupported);
        config
    }
}

pub(crate) async fn open(target: &Target) -> Result<Sql, Failure> {
    let mut host = target.host.clone();
    let mut port = target.port;
    for _ in 0..3 {
        match login(target, &host, port).await {
            Err(Redirect::To(next_host, next_port)) => {
                host = next_host;
                port = next_port;
            }
            Err(Redirect::Failed(failure)) => return Err(failure),
            Ok(client) => return Ok(client),
        }
    }
    Err(Failure::database("connection", "the server redirected the connection too many times"))
}

enum Redirect {
    To(String, u16),
    Failed(Failure),
}

async fn login(target: &Target, host: &str, port: u16) -> Result<Sql, Redirect> {
    let tcp = TcpStream::connect((host, port))
        .await
        .map_err(|error| Redirect::Failed(Failure::database("connection", format!("{host}:{port}: {error}"))))?;
    let _ = tcp.set_nodelay(true);
    let mode = Mode::of(&target.tls);
    let failed = |failure: Failure| Redirect::Failed(failure);
    let link = if target.strict {
        let config = tls::client_config(&target.tls, &[b"tds/8.0"]).map_err(failed)?;
        let name = tls::server_name(host).map_err(failed)?;
        Link::strict(tcp, config, name)
            .await
            .map_err(|error| failed(Failure::database("tls", format!("TLS (TDS 8.0): {error}"))))?
    } else {
        let config = tls::client_config_tls12(&target.tls).map_err(failed)?;
        let name = tls::server_name(host).map_err(failed)?;
        let wanted = match mode {
            Mode::Disable => Wanted::Never,
            Mode::Prefer => Wanted::IfOffered,
            Mode::Require | Mode::VerifyCa | Mode::VerifyFull => Wanted::Always,
        };
        Link::negotiated(tcp, config, name, wanted)
    };
    let mut client = match Client::connect(target.config(host, port), link.compat()).await {
        Ok(client) => client,
        Err(tiberius::error::Error::Routing { host, port }) => return Err(Redirect::To(host, port)),
        Err(error) => return Err(Redirect::Failed(crate::session::failure(&error, None))),
    };
    crate::session::select(&mut client, SESSION_OPTIONS, &[])
        .await
        .map_err(|error| Redirect::Failed(crate::session::failure(&error, None)))?;
    Ok(client)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn connect_parameters_become_a_target() {
        let target = Target::of(&json!({
            "host": "db.example.org", "port": 14330, "database": "shop", "user": "ann", "password": "secret",
            "tls": { "mode": "verify-full" }, "settings": { "authentication": "accessToken" },
            "applicationName": "Zen RustRonin"
        }))
        .unwrap();
        assert_eq!((target.host.as_str(), target.port), ("db.example.org", 14330));
        assert_eq!(target.login, Login::AccessToken);
        assert!(!target.strict);
        assert_eq!(target.application, "Zen RustRonin");
        let defaults = Target::of(&json!({ "tls": { "mode": "require", "direct": true } })).unwrap();
        assert_eq!((defaults.host.as_str(), defaults.port), ("localhost", DEFAULT_PORT));
        assert!(defaults.strict, "direct TLS is TDS 8.0");
        assert!(Target::of(&json!({ "settings": { "strict": "true" } })).unwrap().strict);
    }
}

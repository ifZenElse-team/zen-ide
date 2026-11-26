use serde_json::{Value as Json, json};
use zen_db_driver_kit::Failure;
use zen_db_driver_kit::tls::Mode;

use crate::auth::Mechanism;

pub(crate) const DEFAULT_PORT: u16 = 27017;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Target {
    pub(crate) hosts: Vec<(String, u16)>,
    pub(crate) srv: Option<String>,
    pub(crate) database: String,
    pub(crate) user: Option<String>,
    pub(crate) password: String,
    pub(crate) auth_source: Option<String>,
    pub(crate) mechanism: Option<Mechanism>,
    pub(crate) x509: bool,
    pub(crate) tls: Json,
    pub(crate) replica_set: Option<String>,
    pub(crate) direct: bool,
    pub(crate) application: String,
}

impl Target {
    pub(crate) fn of(params: &Json) -> Result<Self, Failure> {
        let text = |key: &str| params.get(key).and_then(Json::as_str).filter(|text| !text.is_empty());
        let setting = |key: &str| {
            params.pointer(&format!("/settings/{key}")).and_then(|value| match value {
                Json::String(text) if !text.is_empty() => Some(text.clone()),
                Json::Bool(flag) => Some(flag.to_string()),
                _ => None,
            })
        };
        let mut target = match text("url").filter(|url| url.starts_with("mongodb")) {
            Some(url) => parse_url(url)?,
            None => Target {
                hosts: Vec::new(),
                srv: None,
                database: String::new(),
                user: None,
                password: String::new(),
                auth_source: None,
                mechanism: None,
                x509: false,
                tls: Json::Null,
                replica_set: None,
                direct: false,
                application: String::new(),
            },
        };
        if let Some(host) = text("host") {
            let port = params.get("port").and_then(Json::as_u64).and_then(|port| u16::try_from(port).ok());
            if target.srv.is_none() || port.is_some() {
                target.srv = None;
                target.hosts = vec![(host.to_owned(), port.unwrap_or(DEFAULT_PORT))];
            }
        }
        if target.hosts.is_empty() && target.srv.is_none() {
            target.hosts = vec![("localhost".to_owned(), DEFAULT_PORT)];
        }
        if let Some(database) = text("database") {
            database.clone_into(&mut target.database);
        }
        if target.database.is_empty() {
            "test".clone_into(&mut target.database);
        }
        if let Some(user) = text("user") {
            target.user = Some(user.to_owned());
        }
        if let Some(password) = text("password") {
            password.clone_into(&mut target.password);
        }
        if let Some(source) = setting("authSource") {
            target.auth_source = Some(source);
        }
        if let Some(mechanism) = setting("authMechanism") {
            target.set_mechanism(&mechanism)?;
        }
        if let Some(set) = setting("replicaSet") {
            target.replica_set = Some(set);
        }
        if let Some(direct) = setting("directConnection") {
            target.direct = direct == "true";
        }
        target.application = text("applicationName").unwrap_or("Zen IDE").to_owned();
        target.tls = merge_tls(params.get("tls"), &target.tls);
        let certified = target.tls.get("clientCert").and_then(Json::as_str).is_some_and(|cert| !cert.is_empty());
        if target.x509 && (Mode::of(&target.tls) == Mode::Disable || !certified) {
            return Err(Failure::params(
                "MONGODB-X509 logs in with the TLS client certificate: turn TLS on and set a client certificate and key",
            ));
        }
        Ok(target)
    }

    pub(crate) fn set_mechanism(&mut self, name: &str) -> Result<(), Failure> {
        self.x509 = name.trim().eq_ignore_ascii_case("MONGODB-X509");
        self.mechanism = if self.x509 { None } else { Mechanism::named(name)? };
        Ok(())
    }

    pub(crate) fn source(&self) -> &str {
        self.auth_source.as_deref().unwrap_or("admin")
    }
}

fn merge_tls(asked: Option<&Json>, from_url: &Json) -> Json {
    let mut tls = asked.cloned().filter(Json::is_object).unwrap_or_else(|| json!({}));
    let explicit = tls.get("mode").and_then(Json::as_str).is_some_and(|mode| mode != "prefer");
    if !explicit && let Some(mode) = from_url.get("mode") {
        tls["mode"] = mode.clone();
    }
    for key in ["caFile", "clientCert", "clientKey"] {
        if tls.get(key).is_none()
            && let Some(value) = from_url.get(key)
        {
            tls[key] = value.clone();
        }
    }
    tls
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%'
            && let Some(byte) = text.get(at + 1..at + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            at += 3;
            continue;
        }
        out.push(bytes[at]);
        at += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(crate) fn parse_url(url: &str) -> Result<Target, Failure> {
    let invalid = |what: &str| Failure::params(format!("the MongoDB address is not valid: {what}"));
    let (srv, rest) = if let Some(rest) = url.strip_prefix("mongodb+srv://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("mongodb://") {
        (false, rest)
    } else {
        return Err(invalid("it must start with mongodb:// or mongodb+srv://"));
    };
    let (authority, tail) = match rest.find(['/', '?']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    let (login, hosts) = match authority.rfind('@') {
        Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
        None => (None, authority),
    };
    let (path, query) = match tail.find('?') {
        Some(at) => (&tail[..at], &tail[at + 1..]),
        None => (tail, ""),
    };
    let mut target = Target {
        hosts: Vec::new(),
        srv: None,
        database: decode(path.trim_start_matches('/')),
        user: None,
        password: String::new(),
        auth_source: None,
        mechanism: None,
        x509: false,
        tls: json!({}),
        replica_set: None,
        direct: false,
        application: String::new(),
    };
    if let Some(login) = login {
        let (user, password) = login.split_once(':').unwrap_or((login, ""));
        target.user = Some(decode(user)).filter(|user| !user.is_empty());
        target.password = decode(password);
    }
    for host in hosts.split(',').filter(|host| !host.is_empty()) {
        let (name, port) = match host.rsplit_once(':') {
            Some((name, port)) if !name.ends_with(':') && !port.contains(']') => {
                (name, port.parse::<u16>().map_err(|_| invalid("a port is not a number"))?)
            }
            _ => (host, DEFAULT_PORT),
        };
        target.hosts.push((name.trim_start_matches('[').trim_end_matches(']').to_owned(), port));
    }
    if target.hosts.is_empty() {
        return Err(invalid("no host"));
    }
    if srv {
        if target.hosts.len() != 1 || hosts.contains(':') {
            return Err(invalid("a mongodb+srv:// address names one host without a port"));
        }
        target.srv = target.hosts.pop().map(|(name, _)| name);
        target.tls = json!({ "mode": "verify-full" });
    }
    apply_options(&mut target, query)?;
    Ok(target)
}

pub(crate) fn apply_options(target: &mut Target, query: &str) -> Result<(), Failure> {
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value);
        let on = value.eq_ignore_ascii_case("true");
        match key.to_ascii_lowercase().as_str() {
            "authsource" => target.auth_source = Some(value),
            "authmechanism" => target.set_mechanism(&value)?,
            "replicaset" => target.replica_set = Some(value),
            "directconnection" => target.direct = on,
            "appname" => target.application = value,
            "tls" | "ssl" => {
                let relaxed = matches!(target.tls.get("mode").and_then(Json::as_str), Some("require" | "verify-ca"));
                if !on {
                    target.tls["mode"] = json!("disable");
                } else if !relaxed {
                    target.tls["mode"] = json!("verify-full");
                }
            }
            "tlsallowinvalidcertificates" | "tlsinsecure" if on => target.tls["mode"] = json!("require"),
            "tlsallowinvalidhostnames" if on => {
                if target.tls.get("mode").and_then(Json::as_str) != Some("require") {
                    target.tls["mode"] = json!("verify-ca");
                }
            }
            "tlscafile" => {
                target.tls["caFile"] = json!(value);
                if Mode::of(&target.tls) == Mode::Prefer {
                    target.tls["mode"] = json!("verify-full");
                }
            }
            "tlscertificatekeyfile" => {
                target.tls["clientCert"] = json!(value);
                target.tls["clientKey"] = json!(value);
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

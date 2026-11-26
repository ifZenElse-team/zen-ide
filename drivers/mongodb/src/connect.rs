use std::time::Duration;

use bson::{Bson, Document, doc};
use tokio::net::TcpStream;
use zen_db_driver_kit::Failure;
use zen_db_driver_kit::tls::{self, Mode};

use crate::auth::{self, Mechanism};
use crate::dns;
use crate::target::{Target, apply_options};
use crate::wire::{Stream, Wire};

pub(crate) struct Link {
    pub(crate) wire: Wire,
    pub(crate) connection_id: Option<i64>,
}

pub(crate) async fn resolve(target: &Target) -> Result<Target, Failure> {
    if target.srv.is_none() {
        return Ok(target.clone());
    }
    resolve_with(&dns::name_servers(), target).await
}

pub(crate) async fn resolve_with(servers: &[std::net::SocketAddr], target: &Target) -> Result<Target, Failure> {
    let Some(name) = &target.srv else { return Ok(target.clone()) };
    let found = dns::srv(servers, &format!("_mongodb._tcp.{name}")).await?;
    let parent = name.split_once('.').map(|(_, parent)| parent).unwrap_or_default();
    let hosts: Vec<(String, u16)> = found
        .into_iter()
        .map(|(host, port)| (host.trim_end_matches('.').to_owned(), port))
        .filter(|(host, _)| host.ends_with(&format!(".{parent}")))
        .collect();
    if hosts.is_empty() {
        return Err(Failure::database("connection", format!("DNS lists no MongoDB servers for {name}")));
    }
    let mut resolved = target.clone();
    resolved.hosts = hosts;
    resolved.srv = None;
    if let Ok(records) = dns::txt(servers, name).await {
        let user_options = (resolved.auth_source.clone(), resolved.replica_set.clone());
        for record in records {
            let allowed: Vec<&str> = record
                .split('&')
                .filter(|pair| {
                    let key = pair.split('=').next().unwrap_or_default().to_ascii_lowercase();
                    matches!(key.as_str(), "authsource" | "replicaset" | "loadbalanced")
                })
                .collect();
            apply_options(&mut resolved, &allowed.join("&"))?;
        }
        if user_options.0.is_some() {
            resolved.auth_source = user_options.0;
        }
        if user_options.1.is_some() {
            resolved.replica_set = user_options.1;
        }
    }
    Ok(resolved)
}

pub(crate) async fn open(target: &Target, timeout: Duration) -> Result<Link, Failure> {
    let target = resolve(target).await?;
    let mut last = Failure::database("connection", "no server to connect to");
    let mut queue: Vec<(String, u16)> = target.hosts.clone();
    let mut tried = Vec::new();
    while let Some(address) = (!queue.is_empty()).then(|| queue.remove(0)) {
        if tried.contains(&address) || tried.len() > 16 {
            continue;
        }
        tried.push(address.clone());
        match handshake(&target, &address, timeout).await {
            Ok((wire, hello)) => {
                let primary =
                    hello.get_bool("isWritablePrimary").or_else(|_| hello.get_bool("ismaster")).unwrap_or(false);
                let standalone =
                    !hello.contains_key("setName") && !hello.get_str("msg").is_ok_and(|msg| msg == "isdbgrid");
                if target.direct || primary || standalone || hello.get_str("msg").is_ok_and(|msg| msg == "isdbgrid") {
                    return login(&target, wire, hello).await;
                }
                if let Some(set) = &target.replica_set
                    && hello.get_str("setName").is_ok_and(|name| name != set)
                {
                    last = Failure::database(
                        "connection",
                        format!("{}:{} is not in replica set {set}", address.0, address.1),
                    );
                    continue;
                }
                if let Ok(primary) = hello.get_str("primary") {
                    queue.insert(0, split_address(primary));
                }
                for host in hello.get_array("hosts").map(Vec::as_slice).unwrap_or_default() {
                    if let Some(host) = host.as_str() {
                        queue.push(split_address(host));
                    }
                }
                last = Failure::database("connection", "no primary of the replica set answered");
            }
            Err(failure) => last = failure,
        }
    }
    Err(last)
}

fn split_address(text: &str) -> (String, u16) {
    match text.rsplit_once(':') {
        Some((host, port)) => (host.to_owned(), port.parse().unwrap_or(crate::target::DEFAULT_PORT)),
        None => (text.to_owned(), crate::target::DEFAULT_PORT),
    }
}

async fn stream(target: &Target, host: &str, port: u16, encrypted: bool) -> Result<Stream, Failure> {
    let tcp = TcpStream::connect((host, port))
        .await
        .map_err(|error| Failure::database("connection", format!("{host}:{port}: {error}")))?;
    let _ = tcp.set_nodelay(true);
    if !encrypted {
        return Ok(Stream::Plain(tcp));
    }
    let config = tls::client_config(&target.tls, &[])?;
    let name = tls::server_name(host)?;
    let stream = tokio_rustls::TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .map_err(|error| Failure::database("tls", format!("TLS: {error}")))?;
    Ok(Stream::Tls(Box::new(stream)))
}

fn hello_command(target: &Target) -> Document {
    let mut hello = doc! {
        "hello": 1,
        "helloOk": true,
        "client": {
            "driver": { "name": "Zen IDE", "version": env!("CARGO_PKG_VERSION") },
            "os": { "type": std::env::consts::OS, "architecture": std::env::consts::ARCH },
            "application": { "name": target.application.clone() },
        },
    };
    if let Some(user) = &target.user
        && target.mechanism.is_none()
        && !target.x509
    {
        hello.insert("saslSupportedMechs", format!("{}.{user}", target.source()));
    }
    hello
}

async fn handshake(target: &Target, address: &(String, u16), timeout: Duration) -> Result<(Wire, Document), Failure> {
    let (host, port) = (address.0.as_str(), address.1);
    let attempt = |encrypted: bool| async move {
        let work = async {
            let mut wire = Wire::new(stream(target, host, port, encrypted).await?);
            let hello = wire.command("admin", hello_command(target)).await.map_err(|error| error.failure())?;
            Ok::<_, Failure>((wire, hello))
        };
        tokio::time::timeout(timeout, work)
            .await
            .unwrap_or_else(|_| Err(Failure::database("timeout", format!("{host}:{port} did not answer in time"))))
    };
    match Mode::of(&target.tls) {
        Mode::Disable => attempt(false).await,
        Mode::Prefer => match attempt(true).await {
            Ok(done) => Ok(done),
            Err(_) => attempt(false).await,
        },
        Mode::Require | Mode::VerifyCa | Mode::VerifyFull => attempt(true).await,
    }
}

async fn login(target: &Target, mut wire: Wire, hello: Document) -> Result<Link, Failure> {
    if let Some(size) = hello.get("maxMessageSizeBytes").and_then(number) {
        wire.max_message = usize::try_from(size).unwrap_or(crate::wire::MAX_MESSAGE);
    }
    if target.x509 {
        auth::x509(&mut wire, target.user.as_deref()).await?;
    } else if let Some(user) = &target.user {
        let mechanism = target.mechanism.unwrap_or_else(|| Mechanism::offered(&hello));
        auth::authenticate(&mut wire, mechanism, target.source(), user, &target.password).await?;
    }
    let connection_id = hello.get("connectionId").and_then(number);
    Ok(Link { wire, connection_id })
}

pub(crate) fn number(value: &Bson) -> Option<i64> {
    match value {
        Bson::Int32(number) => Some(i64::from(*number)),
        Bson::Int64(number) => Some(*number),
        Bson::Double(number) if number.fract() == 0.0 => Some(*number as i64),
        _ => None,
    }
}

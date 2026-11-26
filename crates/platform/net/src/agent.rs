use std::sync::{Mutex, OnceLock, PoisonError, RwLock};
use std::time::{Duration, Instant};

use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{ConnectProxyConnector, Connector, SocksConnector, TcpConnector};

use crate::connector::TrustConnector;
use crate::proxy::{Route, route};
use crate::system::{self, SystemProxy};
use crate::{ProxyMode, ProxySettings};

const SYSTEM_LIFETIME: Duration = Duration::from_secs(30);

pub fn install_tls() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::CryptoProvider::install_default(rustls_rustcrypto::provider());
    });
}

#[derive(Clone, Default)]
struct Current {
    settings: ProxySettings,
    password: Option<(String, String)>,
}

impl Current {
    fn password(&self) -> Option<&str> {
        self.password
            .as_ref()
            .filter(|(owner, _)| *owner == password_owner(&self.settings))
            .map(|(_, password)| password.as_str())
    }
}

pub fn password_owner(settings: &ProxySettings) -> String {
    format!("{:?} {}@{}:{}", settings.kind, settings.login, settings.host.trim().to_ascii_lowercase(), settings.port)
}

fn current() -> &'static RwLock<Current> {
    static CURRENT: OnceLock<RwLock<Current>> = OnceLock::new();
    CURRENT.get_or_init(RwLock::default)
}

pub fn configure(settings: ProxySettings) {
    current().write().unwrap_or_else(PoisonError::into_inner).settings = settings;
}

pub fn set_password(password: Option<String>) {
    let mut current = current().write().unwrap_or_else(PoisonError::into_inner);
    let owner = password_owner(&current.settings);
    current.password = password.map(|password| (owner, password));
}

pub fn set_password_for(settings: &ProxySettings, password: String) {
    current().write().unwrap_or_else(PoisonError::into_inner).password = Some((password_owner(settings), password));
}

pub fn password() -> Option<String> {
    current().read().unwrap_or_else(PoisonError::into_inner).password().map(str::to_owned)
}

pub fn password_for(settings: &ProxySettings) -> Option<String> {
    let current = current().read().unwrap_or_else(PoisonError::into_inner);
    let owner = password_owner(settings);
    current.password.as_ref().filter(|(known, _)| *known == owner).map(|(_, password)| password.clone())
}

pub fn has_password() -> bool {
    current().read().unwrap_or_else(PoisonError::into_inner).password().is_some()
}

pub fn settings() -> ProxySettings {
    current().read().unwrap_or_else(PoisonError::into_inner).settings.clone()
}

fn system_proxy() -> SystemProxy {
    static CACHE: Mutex<Option<(Instant, SystemProxy)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    match &*cache {
        Some((read, system)) if read.elapsed() < SYSTEM_LIFETIME => system.clone(),
        _ => {
            let system = system::read();
            *cache = Some((Instant::now(), system.clone()));
            system
        }
    }
}

pub fn route_for(url: &str) -> Route {
    let current = current().read().unwrap_or_else(PoisonError::into_inner).clone();
    route_with(&current.settings, current.password(), url)
}

fn route_with(settings: &ProxySettings, password: Option<&str>, url: &str) -> Route {
    let detected = (settings.mode == ProxyMode::AutoDetect).then(system_proxy).unwrap_or_default();
    let environment = (settings.mode == ProxyMode::AutoDetect).then(ureq::Proxy::try_from_env).flatten();
    route(settings, password, environment, &detected, url)
}

pub type Builder = ureq::config::ConfigBuilder<ureq::typestate::AgentScope>;

pub fn config_for(url: &str) -> Builder {
    install_tls();
    ureq::Agent::config_builder().proxy(route_for(url).proxy())
}

pub fn direct() -> Builder {
    install_tls();
    ureq::Agent::config_builder().proxy(None)
}

pub fn agent(builder: Builder) -> ureq::Agent {
    let chain =
        ().chain(SocksConnector::default())
            .chain(ConnectProxyConnector::default())
            .chain(TcpConnector::default())
            .chain(TrustConnector);
    ureq::Agent::with_parts(builder.build(), chain, DefaultResolver::default())
}

pub fn agent_for(url: &str) -> ureq::Agent {
    agent(config_for(url))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckError {
    AutomaticConfiguration(String),
    UnusableProxy,
    Failed(String),
}

pub fn check(
    settings: &ProxySettings,
    password: Option<&str>,
    url: &str,
    timeout: Duration,
) -> Result<u16, CheckError> {
    install_tls();
    let route = route_with(settings, password, url);
    if route.is_unusable() {
        return Err(CheckError::UnusableProxy);
    }
    if let Some(script) = route.unsupported_script() {
        return Err(CheckError::AutomaticConfiguration(script.to_owned()));
    }
    let agent = agent(
        ureq::Agent::config_builder().proxy(route.proxy()).http_status_as_error(false).timeout_global(Some(timeout)),
    );
    agent.get(url).call().map(|answer| answer.status().as_u16()).map_err(|error| CheckError::Failed(error.to_string()))
}

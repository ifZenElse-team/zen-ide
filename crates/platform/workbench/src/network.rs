pub mod certificates;

use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_net::{CheckError, ProxyMode, ProxySettings};
use zen_secrets::{Secret, SecretId, SecretStore};

pub const SERVICE: &str = "space.ifzenelse.zen.proxy";

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);

fn password_id(settings: &ProxySettings) -> Result<SecretId, zen_secrets::SecretError> {
    SecretId::new(SERVICE, zen_net::password_owner(settings))
}

fn person_only(session: &Session) -> Result<(), CommandError> {
    if zen_core::macros::by_agent(session) {
        return Err(CommandError::with_code(crate::trust::AGENT_NOT_ALLOWED, t!("network-agent-refused")));
    }
    Ok(())
}

fn in_background(job: impl FnOnce() + Send + 'static) {
    let _ = std::thread::Builder::new().name("zen-proxy-password".to_owned()).spawn(job);
}

pub fn load_password(store: &dyn SecretStore) {
    if let Ok(id) = password_id(&zen_net::settings())
        && let Ok(Some(password)) = store.get(&id)
    {
        zen_net::set_password(Some(password.expose().to_owned()));
    }
}

pub fn apply(settings: &ProxySettings) {
    zen_net::configure(settings.clone());
    if settings.authentication && settings.remember && !zen_net::has_password() {
        in_background(|| load_password(zen_secrets::system().as_ref()));
    }
}

pub fn store_password(store: &dyn SecretStore, remember: bool, password: &str) -> Result<(), zen_secrets::SecretError> {
    store_password_for(store, &zen_net::settings(), remember, password)
}

fn store_password_for(
    store: &dyn SecretStore,
    settings: &ProxySettings,
    remember: bool,
    password: &str,
) -> Result<(), zen_secrets::SecretError> {
    zen_net::set_password_for(settings, password.to_owned());
    let id = password_id(settings)?;
    if remember { store.set(&id, &Secret::new(password.to_owned())) } else { store.delete(&id).map(drop) }
}

pub fn keep_password(settings: &ProxySettings, password: String) {
    zen_net::set_password_for(settings, password.clone());
    let settings = settings.clone();
    in_background(move || {
        let _ = store_password_for(zen_secrets::system().as_ref(), &settings, settings.remember, &password);
    });
}

pub fn clear_passwords(store: &dyn SecretStore) -> Result<(), zen_secrets::SecretError> {
    zen_net::set_password(None);
    store.delete(&password_id(&zen_net::settings())?).map(drop)
}

pub fn check_error(error: &CheckError) -> String {
    match error {
        CheckError::AutomaticConfiguration(script) => t!("network-check-script", script = script.as_str()),
        CheckError::UnusableProxy => t!("network-check-unusable"),
        CheckError::Failed(reason) => t!("network-check-failed", reason = reason.as_str()),
    }
}

pub fn check_settings(settings: &ProxySettings, password: Option<&str>, url: &str) -> Result<u16, CheckError> {
    zen_net::check(settings, password, url, CHECK_TIMEOUT)
}

pub fn check_settings_asking(settings: &ProxySettings, password: Option<&str>, url: &str) -> Result<u16, CheckError> {
    zen_net::trust::asking_the_person(|| check_settings(settings, password, url))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusParams {
    /// The address to resolve; the release server's when omitted.
    #[serde(default)]
    pub url: Option<String>,
}

/// What the IDE does with one address under the current proxy setting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    /// The mode of Settings → HTTP Proxy.
    pub mode: ProxyMode,
    /// `direct`, or the proxy's protocol, host and port.
    pub route: String,
    /// A proxy configuration script the IDE cannot evaluate, so it connects as `route` says.
    pub unsupported_script: Option<String>,
    /// Whether a password is known for this session.
    pub has_password: bool,
}

pub struct ProxyStatusCommand;

impl Command for ProxyStatusCommand {
    type Params = StatusParams;
    type Output = ProxyStatus;
    const ID: &'static str = "settings.proxy.status";
    const TITLE: &'static str = "HTTP Proxy Status";
    const DESCRIPTION: &'static str = "Says how the IDE reaches an address under Settings → HTTP Proxy: directly or \
        through which proxy, and whether a proxy configuration script (PAC) is set that the IDE cannot evaluate. The \
        setting itself is `proxy` in get_settings and set_settings.";

    fn run(&self, _: &mut Session, params: StatusParams) -> Result<ProxyStatus, CommandError> {
        let url = params.url.unwrap_or_else(|| zen_update::OFFICIAL_MANIFEST_URL.to_owned());
        let route = zen_net::route_for(&url);
        Ok(ProxyStatus {
            mode: zen_net::settings().mode,
            route: route.describe(),
            unsupported_script: route.unsupported_script().map(str::to_owned),
            has_password: zen_net::has_password(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckParams {
    /// The http or https address to fetch.
    pub url: String,
}

/// The answer of Check Connection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CheckAnswer {
    /// The HTTP status of the answer.
    pub status: u16,
}

pub struct CheckConnection;

impl Command for CheckConnection {
    type Params = CheckParams;
    type Output = CheckAnswer;
    const ID: &'static str = "settings.proxy.check";
    const TITLE: &'static str = "Check Connection";
    const DESCRIPTION: &'static str = "Fetches an http or https address under the current proxy setting and returns \
        the HTTP status, or fails with the reason. Fails when a proxy configuration script is set, because the IDE \
        cannot evaluate one. Waits up to 15 seconds.";

    fn run(&self, session: &mut Session, params: CheckParams) -> Result<CheckAnswer, CommandError> {
        person_only(session)?;
        let lower = params.url.to_ascii_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            return Err(CommandError::new(t!("network-check-url")));
        }
        let password = zen_net::password();
        check_settings(&zen_net::settings(), password.as_deref(), &params.url)
            .map(|status| CheckAnswer { status })
            .map_err(|error| CommandError::new(check_error(&error)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasswordParams {
    /// The proxy password. Kept in the system keychain when Remember is on, otherwise for this session.
    pub password: String,
}

pub struct SetProxyPassword(pub Arc<dyn SecretStore>);

impl Command for SetProxyPassword {
    type Params = PasswordParams;
    type Output = ();
    const ID: &'static str = "settings.proxy.password";
    const TITLE: &'static str = "Set Proxy Password";
    const DESCRIPTION: &'static str = "Sets the password of the manual proxy: in the system keychain when `remember` \
        is on in the proxy setting, otherwise for this session. It never reaches a settings file.";

    fn run(&self, session: &mut Session, params: PasswordParams) -> Result<(), CommandError> {
        person_only(session)?;
        store_password(self.0.as_ref(), zen_net::settings().remember, &params.password)
            .map_err(|error| CommandError::new(error.to_string()))
    }
}

pub struct ClearProxyPasswords(pub Arc<dyn SecretStore>);

impl Command for ClearProxyPasswords {
    type Params = zen_core::NoParams;
    type Output = ();
    const ID: &'static str = "settings.proxy.forget";
    const TITLE: &'static str = "Clear Proxy Passwords";
    const DESCRIPTION: &'static str = "Clear Passwords of Settings → HTTP Proxy: forgets the proxy password of this \
        session and removes it from the system keychain.";

    fn run(&self, session: &mut Session, _: zen_core::NoParams) -> Result<(), CommandError> {
        person_only(session)?;
        clear_passwords(self.0.as_ref()).map_err(|error| CommandError::new(error.to_string()))
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ProxyStatusCommand)?;
    registry.register(CheckConnection)?;
    registry.register(SetProxyPassword(zen_secrets::system()))?;
    registry.register(ClearProxyPasswords(zen_secrets::system()))?;
    certificates::register(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    static ONE_PROXY: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn the_password_lives_in_the_store_only_when_it_is_remembered() {
        let _globals = ONE_PROXY.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let store = zen_secrets::MemoryStore::new();
        let id = password_id(&zen_net::settings()).unwrap();
        store_password(&store, true, "pw").unwrap();
        assert_eq!(store.get(&id).unwrap().unwrap().expose(), "pw");
        assert_eq!(zen_net::password().as_deref(), Some("pw"));

        store_password(&store, false, "session").unwrap();
        assert!(store.get(&id).unwrap().is_none(), "Remember is off: the keychain forgets it");
        assert_eq!(zen_net::password().as_deref(), Some("session"));

        store_password(&store, true, "kept").unwrap();
        zen_net::set_password(None);
        load_password(&store);
        assert_eq!(zen_net::password().as_deref(), Some("kept"), "read back at start");

        clear_passwords(&store).unwrap();
        assert!(store.get(&id).unwrap().is_none());
        assert_eq!(zen_net::password(), None);
    }

    #[test]
    fn a_stored_password_is_sent_only_to_the_host_and_port_it_was_stored_for() {
        let _globals = ONE_PROXY.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let store = zen_secrets::MemoryStore::new();
        let at = |host: &str, port: u16| ProxySettings {
            mode: ProxyMode::Manual,
            host: host.to_owned(),
            port,
            authentication: true,
            login: "me".to_owned(),
            remember: true,
            ..ProxySettings::default()
        };
        zen_net::configure(at("p.corp", 3128));
        store_password(&store, true, "pw").unwrap();
        for moved in [at("evil.example", 3128), at("p.corp", 9999)] {
            zen_net::configure(moved.clone());
            zen_net::set_password(None);
            load_password(&store);
            assert_eq!(zen_net::password(), None, "{moved:?}");
        }
        zen_net::configure(at("p.corp", 3128));
        load_password(&store);
        assert_eq!(zen_net::password().as_deref(), Some("pw"));
        zen_net::configure(at("evil.example", 3128));
        assert_eq!(zen_net::password(), None, "in memory too");
        zen_net::configure(at("p.corp", 3128));
        assert_eq!(zen_net::password_for(&at("p.corp", 3128)).as_deref(), Some("pw"));
        assert_eq!(zen_net::password_for(&at("evil.example", 3128)), None, "what the page checks before OK");
        zen_net::configure(at("evil.example", 3128));
        let route = zen_net::route_for("https://a.example/").proxy().unwrap();
        assert_eq!(route.password(), None);
        zen_net::configure(ProxySettings::default());
    }

    #[test]
    fn network_and_trust_commands_are_a_person_s() {
        zen_i18n::register(crate::LOCALES);
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        let attempts = [
            ("settings.proxy.check", serde_json::json!({ "url": "https://a.example/" })),
            ("settings.proxy.password", serde_json::json!({ "password": "x" })),
            ("settings.proxy.forget", serde_json::json!({})),
            ("settings.certificates.add", serde_json::json!({ "path": "/nonexistent.pem" })),
            ("settings.certificates.answer", serde_json::json!({ "fingerprint": "00", "accept": true })),
        ];
        for (command, params) in attempts {
            let answer =
                zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, command, params));
            assert!(format!("{answer:?}").contains(crate::trust::AGENT_NOT_ALLOWED), "{command}: {answer:?}");
        }
        for (command, params) in [
            ("settings.proxy.status", serde_json::json!({})),
            ("settings.certificates.list", serde_json::json!({})),
            ("settings.certificates.pending", serde_json::json!({})),
        ] {
            let answer =
                zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, command, params));
            assert!(answer.is_ok(), "reading stays open: {command}: {answer:?}");
        }
    }

    #[test]
    fn a_proxy_script_reads_as_not_supported() {
        let text = check_error(&CheckError::AutomaticConfiguration("https://a/b.pac".to_owned()));
        assert!(text.contains("https://a/b.pac") && text.contains("not supported"), "{text}");
    }
}

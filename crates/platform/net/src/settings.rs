use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How the IDE reaches the network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProxyMode {
    /// Connect directly.
    NoProxy,
    /// Use the proxy the environment or the system names.
    #[default]
    AutoDetect,
    /// Use the host and port of these settings.
    Manual,
}

/// The protocol of a manually configured proxy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ProxyKind {
    /// An HTTP proxy, through CONNECT.
    #[default]
    Http,
    /// A SOCKS5 proxy.
    Socks,
}

/// Appearance & Behavior → System Settings → HTTP Proxy. The password is never part of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ProxySettings {
    /// No proxy, Auto-detect proxy settings or Manual proxy configuration.
    pub mode: ProxyMode,
    /// Automatic proxy configuration URL is ticked. Such scripts are reported as not supported.
    pub automatic_configuration: bool,
    /// The address of the proxy configuration script.
    pub automatic_configuration_url: String,
    /// HTTP or SOCKS.
    pub kind: ProxyKind,
    /// Host name of the manual proxy.
    pub host: String,
    /// Port number of the manual proxy.
    pub port: u16,
    /// Hosts reached directly, separated by commas; `*` matches any text.
    pub no_proxy_for: String,
    /// Proxy authentication is ticked.
    pub authentication: bool,
    /// Login of the proxy.
    pub login: String,
    /// Keep the password in the system keychain; otherwise it lasts for this session.
    pub remember: bool,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            mode: ProxyMode::default(),
            automatic_configuration: false,
            automatic_configuration_url: String::new(),
            kind: ProxyKind::default(),
            host: String::new(),
            port: 80,
            no_proxy_for: String::new(),
            authentication: false,
            login: String::new(),
            remember: false,
        }
    }
}

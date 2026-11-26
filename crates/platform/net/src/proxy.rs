use crate::{ProxyKind, ProxyMode, ProxySettings, SystemProxy};

#[derive(Clone, Debug, Default)]
pub struct Route {
    proxy: Option<ureq::Proxy>,
    script: Option<String>,
    unusable: bool,
}

impl Route {
    pub fn proxy(&self) -> Option<ureq::Proxy> {
        self.proxy.clone()
    }

    pub fn unsupported_script(&self) -> Option<&str> {
        self.script.as_deref()
    }

    pub fn is_unusable(&self) -> bool {
        self.unusable
    }

    pub fn describe(&self) -> String {
        if self.unusable {
            return "unusable".to_owned();
        }
        match &self.proxy {
            Some(proxy) => format!("{:?} {}:{}", proxy.protocol(), proxy.host(), proxy.port()),
            None => "direct".to_owned(),
        }
    }
}

fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    }
}

fn wildcard(pattern: &[u8], text: &[u8]) -> bool {
    match (pattern.first(), text.first()) {
        (None, None) => true,
        (Some(b'*'), _) => wildcard(&pattern[1..], text) || (!text.is_empty() && wildcard(pattern, &text[1..])),
        (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => wildcard(&pattern[1..], &text[1..]),
        _ => false,
    }
}

pub fn is_excepted(list: &str, url: &str) -> bool {
    let host = host_of(url);
    list.split(',')
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .any(|pattern| wildcard(pattern.as_bytes(), host.as_bytes()))
}

fn is_local(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host == "::1" || host.starts_with("127.")
}

fn usable_host(host: &str) -> bool {
    let bracketed = host.strip_prefix('[').and_then(|rest| rest.strip_suffix(']'));
    match bracketed {
        Some(address) => address.parse::<std::net::Ipv6Addr>().is_ok(),
        None => {
            !host.is_empty() && !host.chars().any(|c| c.is_whitespace() || c.is_control() || "/?#@:\\[]".contains(c))
        }
    }
}

fn nowhere() -> Route {
    let proxy = proxy_at(ureq::ProxyProtocol::Http, "127.0.0.1", 1);
    Route { proxy, script: None, unusable: true }
}

fn proxy_at(protocol: ureq::ProxyProtocol, host: &str, port: u16) -> Option<ureq::Proxy> {
    ureq::Proxy::builder(protocol).host(host).port(port).build().ok()
}

fn manual(settings: &ProxySettings, password: Option<&str>) -> Option<ureq::Proxy> {
    let protocol = match settings.kind {
        ProxyKind::Http => ureq::ProxyProtocol::Http,
        ProxyKind::Socks => ureq::ProxyProtocol::Socks5,
    };
    let mut builder = ureq::Proxy::builder(protocol).host(settings.host.trim()).port(settings.port);
    if settings.authentication && !settings.login.is_empty() {
        builder = builder.username(&settings.login);
        if let Some(password) = password {
            builder = builder.password(password);
        }
    }
    builder.build().ok()
}

pub fn route(
    settings: &ProxySettings,
    password: Option<&str>,
    environment: Option<ureq::Proxy>,
    system: &SystemProxy,
    url: &str,
) -> Route {
    match settings.mode {
        ProxyMode::NoProxy => Route::default(),
        ProxyMode::Manual if is_excepted(&settings.no_proxy_for, url) || is_local(host_of(url)) => Route::default(),
        ProxyMode::Manual if !usable_host(settings.host.trim()) || settings.port == 0 => nowhere(),
        ProxyMode::Manual => match manual(settings, password) {
            Some(proxy) => Route { proxy: Some(proxy), script: None, unusable: false },
            None => nowhere(),
        },
        ProxyMode::AutoDetect => {
            let script = settings
                .automatic_configuration
                .then(|| settings.automatic_configuration_url.trim().to_owned())
                .filter(|url| !url.is_empty())
                .or_else(|| system.script.clone());
            if is_local(host_of(url)) {
                return Route { proxy: None, script, unusable: false };
            }
            if environment.is_some() {
                return Route { proxy: environment, script: None, unusable: false };
            }
            let secure = url.get(..6).is_some_and(|scheme| scheme.eq_ignore_ascii_case("https:"));
            if system.exceptions.iter().any(|exception| is_excepted(exception, url)) {
                return Route { proxy: None, script, unusable: false };
            }
            let web = if secure { system.https.as_ref() } else { system.http.as_ref() };
            let proxy = match (web, &system.socks) {
                (Some((host, port)), _) => proxy_at(ureq::ProxyProtocol::Http, host, *port),
                (None, Some((host, port))) => proxy_at(ureq::ProxyProtocol::Socks5, host, *port),
                (None, None) => None,
            };
            Route { proxy, script, unusable: false }
        }
    }
}

#[cfg(test)]
mod tests;

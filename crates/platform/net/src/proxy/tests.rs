use std::net::TcpListener;
use std::time::Duration;

use super::*;
use crate::parse_scutil;
use crate::test_proxy::{Proxy, target};
use crate::{ProxyKind, ProxyMode, ProxySettings, SystemProxy};

const SCUTIL: &str = "<dictionary> {
  ExceptionsList : <array> {
    0 : *.local
    1 : 169.254/16
  }
  FTPPassive : 1
  HTTPEnable : 1
  HTTPPort : 8080
  HTTPProxy : web.corp
  HTTPSEnable : 1
  HTTPSPort : 8443
  HTTPSProxy : secure.corp
  SOCKSEnable : 0
  SOCKSPort : 1080
  SOCKSProxy : socks.corp
}
";

fn manual_settings(host: &str, port: u16) -> ProxySettings {
    ProxySettings { mode: ProxyMode::Manual, host: host.to_owned(), port, ..ProxySettings::default() }
}

fn none() -> SystemProxy {
    SystemProxy::default()
}

#[test]
fn the_system_proxy_is_read_from_scutil() {
    let system = parse_scutil(SCUTIL);
    assert_eq!(system.http, Some(("web.corp".to_owned(), 8080)));
    assert_eq!(system.https, Some(("secure.corp".to_owned(), 8443)));
    assert_eq!(system.socks, None, "switched off");
    assert_eq!(system.exceptions, ["*.local", "169.254/16"]);
    assert_eq!(system.script, None);
}

#[test]
fn a_proxy_script_of_the_system_is_reported_and_wpad_too() {
    let script = parse_scutil(
        "<dictionary> {\n  ProxyAutoConfigEnable : 1\n  ProxyAutoConfigURLString : http://wpad/x.pac\n}\n",
    );
    assert_eq!(script.script.as_deref(), Some("http://wpad/x.pac"));
    let discovery = parse_scutil("<dictionary> {\n  ProxyAutoDiscoveryEnable : 1\n}\n");
    assert_eq!(discovery.script.as_deref(), Some("WPAD"));
    let settings = ProxySettings::default();
    let route = route(&settings, None, None, &script, "https://example.com/");
    assert_eq!(route.unsupported_script(), Some("http://wpad/x.pac"));
    assert!(route.proxy().is_none());
}

#[test]
fn a_proxy_configuration_url_is_reported_as_not_supported() {
    let settings = ProxySettings {
        automatic_configuration: true,
        automatic_configuration_url: " https://example.com/wpad.pac ".to_owned(),
        ..ProxySettings::default()
    };
    let route = route(&settings, None, None, &none(), "https://example.com/");
    assert_eq!(route.unsupported_script(), Some("https://example.com/wpad.pac"));
}

#[test]
fn auto_detect_takes_the_web_proxy_of_the_scheme_and_the_exceptions() {
    let system = parse_scutil(SCUTIL);
    let settings = ProxySettings::default();
    let of = |url: &str| route(&settings, None, None, &system, url).describe();
    assert!(of("https://example.com/").ends_with("secure.corp:8443"), "{}", of("https://example.com/"));
    assert!(of("http://example.com/").ends_with("web.corp:8080"));
    assert_eq!(of("https://printer.local/"), "direct");
    assert_eq!(of("http://localhost:8080/"), "direct");
    assert_eq!(of("http://127.0.0.1:8080/"), "direct");
}

#[test]
fn auto_detect_prefers_the_environment_of_the_process() {
    let environment = ureq::Proxy::new("http://env.corp:3128").unwrap();
    let route = route(&ProxySettings::default(), None, Some(environment), &parse_scutil(SCUTIL), "https://a.b/");
    assert!(route.describe().ends_with("env.corp:3128"));
}

#[test]
fn no_proxy_ignores_the_environment_and_the_system() {
    let environment = ureq::Proxy::new("http://env.corp:3128").unwrap();
    let settings = ProxySettings { mode: ProxyMode::NoProxy, ..ProxySettings::default() };
    let route = route(&settings, None, Some(environment), &parse_scutil(SCUTIL), "https://a.b/");
    assert_eq!(route.describe(), "direct");
}

#[test]
fn manual_uses_the_list_of_hosts_reached_directly() {
    let settings = ProxySettings { no_proxy_for: "*.example.com, 192.168.*".to_owned(), ..manual_settings("p", 3128) };
    let of = |url: &str| route(&settings, None, None, &none(), url).describe();
    assert!(of("https://ifzenelse.space/").ends_with("p:3128"));
    assert_eq!(of("https://a.EXAMPLE.com:8443/x"), "direct");
    assert_eq!(of("http://192.168.1.5/"), "direct");
    assert!(of("https://example.org/").ends_with("p:3128"));
}

#[test]
fn a_manual_proxy_without_a_host_refuses_instead_of_connecting_directly() {
    assert_eq!(route(&manual_settings(" ", 80), None, None, &none(), "https://a.b/").describe(), "unusable");
}

#[test]
fn socks_is_socks5_and_the_login_is_used_only_when_ticked() {
    let settings = ProxySettings { kind: ProxyKind::Socks, login: "me".to_owned(), ..manual_settings("s", 1080) };
    let found = route(&settings, Some("pw"), None, &none(), "https://a.b/").proxy().unwrap();
    assert_eq!(found.protocol(), ureq::ProxyProtocol::Socks5);
    assert_eq!(found.username(), None);
    let settings = ProxySettings { authentication: true, ..settings };
    let found = route(&settings, Some("pw"), None, &none(), "https://a.b/").proxy().unwrap();
    assert_eq!((found.username(), found.password()), (Some("me"), Some("pw")));
}

#[test]
fn the_settings_never_serialise_a_password() {
    let settings = ProxySettings { login: "me".to_owned(), authentication: true, ..ProxySettings::default() };
    let text = serde_json::to_string(&settings).unwrap();
    assert!(!text.to_lowercase().contains("password"), "{text}");
}

#[test]
fn check_connection_goes_through_the_proxy_with_its_login() {
    let proxy = Proxy::start();
    let settings =
        ProxySettings { authentication: true, login: "me".to_owned(), ..manual_settings("127.0.0.1", proxy.port()) };
    let url = format!("http://{}:{}/", crate::test_proxy::TARGET_HOST, target());
    assert_eq!(crate::check(&settings, Some("pw"), &url, Duration::from_secs(10)), Ok(200));
    let head = proxy.requests().join("\n");
    assert!(head.starts_with("CONNECT zen-target.test"), "{head}");
    assert!(head.to_lowercase().contains("proxy-authorization: basic bWU6cHc=".to_lowercase().as_str()), "{head}");
}

#[test]
fn check_connection_reports_a_proxy_that_cannot_be_reached() {
    let closed = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let settings = manual_settings("127.0.0.1", closed);
    let answer = crate::check(&settings, None, "http://example.invalid/", Duration::from_secs(10));
    assert!(matches!(answer, Err(crate::CheckError::Failed(_))), "{answer:?}");
}

#[test]
fn check_connection_refuses_a_proxy_script() {
    let settings = ProxySettings {
        automatic_configuration: true,
        automatic_configuration_url: "https://example.com/wpad.pac".to_owned(),
        ..ProxySettings::default()
    };
    let answer = crate::check(&settings, None, "http://127.0.0.1:9/", Duration::from_secs(1));
    assert_eq!(answer, Err(crate::CheckError::AutomaticConfiguration("https://example.com/wpad.pac".to_owned())));
}

fn manual_at(host: &str, port: u16) -> ProxySettings {
    ProxySettings { mode: ProxyMode::Manual, host: host.to_owned(), port, ..ProxySettings::default() }
}

#[test]
fn a_manual_proxy_that_cannot_be_used_never_goes_direct() {
    for (host, port) in [("", 8080), ("   ", 8080), ("a b", 8080), ("p.corp/x", 8080), ("p.corp", 0)] {
        let route = route(&manual_at(host, port), None, None, &SystemProxy::default(), "https://a.example/");
        assert_ne!(route.describe(), "direct", "{host:?}:{port}");
        assert!(route.proxy().is_some(), "{host:?}:{port}");
    }
}

#[test]
fn a_manual_proxy_never_sees_this_computer_s_own_addresses() {
    let settings = manual_at("p.corp", 8080);
    for url in ["http://localhost:8080/", "http://127.0.0.1/", "http://[::1]:3000/", "http://LOCALHOST/"] {
        assert_eq!(route(&settings, None, None, &SystemProxy::default(), url).describe(), "direct", "{url}");
    }
    assert_ne!(route(&settings, None, None, &SystemProxy::default(), "http://a.example/").describe(), "direct");
}

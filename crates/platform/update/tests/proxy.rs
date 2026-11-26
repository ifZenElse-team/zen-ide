use std::time::Duration;

use zen_net::test_proxy::{Proxy, target};
use zen_net::{ProxyMode, ProxySettings};

#[test]
fn every_download_goes_through_the_shared_proxy_setting() {
    let proxy = Proxy::start();
    let port = target();
    let url = format!("http://{}:{port}/file", zen_net::test_proxy::TARGET_HOST);
    let manual = ProxySettings {
        mode: ProxyMode::Manual,
        host: "127.0.0.1".to_owned(),
        port: proxy.port(),
        ..ProxySettings::default()
    };
    zen_net::configure(manual.clone());
    let timeout = Duration::from_secs(10);

    assert_eq!(zen_update::fetch::download(&url, 16, |_, _| {}).unwrap(), b"ok");
    assert_eq!(zen_update::fetch::document(&url, 16, timeout).unwrap().unwrap(), b"ok");
    assert!(zen_update::fetch::conditional_json(&url, None, None, 16, timeout).is_ok());
    assert_eq!(zen_update::fetch::content_length(&url, timeout).unwrap(), Some(2));
    assert_eq!(zen_update::fetch::post_json(&url, b"{}", 16, timeout).unwrap(), b"ok");
    assert!(zen_update::fetch::conditional(&url, 16, timeout, &Default::default(), None).is_ok());
    assert_eq!(proxy.requests().len(), 6, "{:?}", proxy.requests());

    let own = format!("http://127.0.0.1:{port}/file");
    assert_eq!(zen_update::fetch::download(&own, 16, |_, _| {}).unwrap(), b"ok");
    assert_eq!(proxy.requests().len(), 6, "this computer's own address goes direct");

    zen_net::configure(ProxySettings { mode: ProxyMode::NoProxy, ..ProxySettings::default() });
    assert_eq!(zen_update::fetch::download(&own, 16, |_, _| {}).unwrap(), b"ok");
    assert_eq!(proxy.requests().len(), 6);
}

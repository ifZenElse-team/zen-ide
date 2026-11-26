use std::time::Duration;

use zen_http::Prepared;
use zen_net::test_proxy::{Proxy, target};
use zen_net::{ProxyMode, ProxySettings};

#[test]
fn the_http_client_uses_the_proxy_unless_the_request_disables_it() {
    let proxy = Proxy::start();
    zen_net::configure(ProxySettings {
        mode: ProxyMode::Manual,
        host: "127.0.0.1".to_owned(),
        port: proxy.port(),
        ..ProxySettings::default()
    });
    let request = Prepared {
        method: "GET".to_owned(),
        url: format!("http://{}:{}/", zen_net::test_proxy::TARGET_HOST, target()),
        follow_redirects: true,
        use_proxy: true,
        ..Prepared::default()
    };
    assert_eq!(zen_http::send(&request, Duration::from_secs(10)).unwrap().status, 200);
    assert_eq!(proxy.requests().len(), 1);

    let direct = Prepared { use_proxy: false, url: format!("http://127.0.0.1:{}/", target()), ..request };
    assert_eq!(zen_http::send(&direct, Duration::from_secs(10)).unwrap().status, 200);
    assert_eq!(proxy.requests().len(), 1, "# @no-proxy connects directly");
}

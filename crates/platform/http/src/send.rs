use std::time::{Duration, Instant};

use crate::{Prepared, Response, SendError};

const BODY_LIMIT: u64 = 16 << 20;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub fn send(prepared: &Prepared, timeout: Duration) -> Result<Response, SendError> {
    send_limited(prepared, timeout, BODY_LIMIT)
}

fn send_limited(prepared: &Prepared, timeout: Duration, limit: u64) -> Result<Response, SendError> {
    let started = Instant::now();
    let mut method = ureq::http::Method::from_bytes(prepared.method.as_bytes())
        .map_err(|error| SendError::Failed(error.to_string()))?;
    let mut url = prepared.url.clone();
    let mut headers = prepared.headers.clone();
    let mut body = prepared.body.clone().unwrap_or_default();
    let mut redirects = 0;
    loop {
        let left = timeout.checked_sub(started.elapsed()).filter(|left| !left.is_zero()).ok_or(SendError::Timeout)?;
        let mut response = exchange(&method, &url, &headers, &body, left, prepared.use_proxy)?;
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(ureq::http::header::LOCATION)
            .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned());
        if let Some(location) = location.filter(|_| prepared.follow_redirects && is_redirect(status)) {
            if redirects == MAX_REDIRECTS {
                return Err(SendError::TooManyRedirects);
            }
            redirects += 1;
            let _ = response.body_mut().with_config().limit(limit).read_to_vec();
            let next = redirect_target(&url, &location)?;
            if origin(&next) != origin(&url) {
                headers.retain(|(name, _)| !is_credential(name));
            }
            if status == 303 && method != ureq::http::Method::HEAD
                || matches!(status, 301 | 302) && method == ureq::http::Method::POST
            {
                method = ureq::http::Method::GET;
                body.clear();
                headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
            }
            url = next;
            continue;
        }
        let read = response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(|error| SendError::Failed(error.to_string()))?;
        let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let status = response.status();
        return Ok(Response {
            status: status.as_u16(),
            reason: status.canonical_reason().unwrap_or_default().to_owned(),
            version: format!("{:?}", response.version()),
            headers: response
                .headers()
                .iter()
                .map(|(name, value)| (name.as_str().to_owned(), String::from_utf8_lossy(value.as_bytes()).into_owned()))
                .collect(),
            body: read,
            duration_ms,
        });
    }
}

const MAX_REDIRECTS: usize = 10;

fn exchange(
    method: &ureq::http::Method,
    url: &str,
    headers: &[(String, String)],
    body: &[u8],
    timeout: Duration,
    use_proxy: bool,
) -> Result<ureq::http::Response<ureq::Body>, SendError> {
    let builder = if use_proxy { zen_net::config_for(url) } else { zen_net::direct() };
    let agent = zen_net::agent(
        builder
            .http_status_as_error(false)
            .allow_non_standard_methods(true)
            .https_only(false)
            .max_redirects(0)
            .timeout_global(Some(timeout))
            .timeout_connect(Some(CONNECT_TIMEOUT.min(timeout)))
            .user_agent(concat!("ZenIDE-HttpClient/", env!("CARGO_PKG_VERSION"))),
    );
    let mut builder = ureq::http::Request::builder().method(method.clone()).uri(url);
    for (name, value) in headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let request = builder.body(body.to_vec()).map_err(|error| SendError::Failed(error.to_string()))?;
    agent.run(request).map_err(|error| SendError::Failed(error.to_string()))
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

fn is_credential(name: &str) -> bool {
    ["authorization", "cookie", "proxy-authorization"].iter().any(|known| name.eq_ignore_ascii_case(known))
}

fn origin(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    format!("{}://{}", scheme.to_ascii_lowercase(), authority.to_ascii_lowercase())
}

fn redirect_target(current: &str, location: &str) -> Result<String, SendError> {
    let location = location.trim();
    let scheme = location.split_once(':').map(|(scheme, _)| scheme).filter(|scheme| {
        scheme.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    });
    if let Some(scheme) = scheme {
        if scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https") {
            return Ok(location.to_owned());
        }
        return Err(SendError::RedirectScheme(scheme.to_owned()));
    }
    let (scheme, _) = current.split_once("://").unwrap_or(("http", current));
    if let Some(rest) = location.strip_prefix("//") {
        return Ok(format!("{scheme}://{rest}"));
    }
    let base = origin(current);
    if location.starts_with('/') {
        return Ok(format!("{base}{location}"));
    }
    let path = current[base.len().min(current.len())..].split(['?', '#']).next().unwrap_or_default();
    let folder = path.rfind('/').map_or("/", |slash| &path[..=slash]);
    Ok(format!("{base}{folder}{location}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_server::TestServer;

    fn prepared(method: &str, url: String, headers: &[(&str, &str)], body: Option<&str>) -> Prepared {
        Prepared {
            method: method.to_owned(),
            url,
            headers: headers.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect(),
            body: body.map(|body| body.as_bytes().to_vec()),
            follow_redirects: true,
            use_proxy: true,
            shown: crate::Shown::default(),
            files: Vec::new(),
        }
    }

    #[test]
    fn get_post_headers_and_body_against_a_local_server() {
        let server = TestServer::start(vec![
            TestServer::json(200, "{\"items\":[]}"),
            TestServer::reply(201, "Created", &[("Location", "/items/1"), ("Content-Type", "text/plain")], "made"),
            TestServer::reply(404, "Not Found", &[], ""),
        ]);
        let got = send(
            &prepared("GET", server.url("/items?q=1"), &[("Accept", "application/json")], None),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!((got.status, got.reason.as_str(), got.version.as_str()), (200, "OK", "HTTP/1.1"));
        assert_eq!(got.body, b"{\"items\":[]}");
        assert!(got.headers.iter().any(|(name, value)| name == "content-type" && value == "application/json"));
        let posted = send(
            &prepared(
                "POST",
                server.url("/items"),
                &[("Content-Type", "application/json"), ("X-Trace", "abc")],
                Some("{\"name\":\"tea\"}"),
            ),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(posted.status, 201);
        assert_eq!(posted.body, b"made");
        let missing = send(&prepared("DELETE", server.url("/items/9"), &[], None), Duration::from_secs(5)).unwrap();
        assert_eq!(missing.status, 404, "an error status is an answer, not a failure");
        let seen = server.finish();
        assert_eq!(seen.len(), 3);
        assert_eq!((seen[0].method.as_str(), seen[0].target.as_str()), ("GET", "/items?q=1"));
        assert_eq!(seen[0].header("accept"), Some("application/json"));
        assert!(seen[0].header("user-agent").is_some_and(|agent| agent.starts_with("ZenIDE-HttpClient/")));
        assert_eq!((seen[1].method.as_str(), seen[1].body.as_slice()), ("POST", &b"{\"name\":\"tea\"}"[..]));
        assert_eq!(seen[1].header("x-trace"), Some("abc"));
        assert_eq!(seen[1].header("content-length"), Some("14"));
        assert_eq!(seen[2].method, "DELETE");
    }

    #[test]
    fn a_closed_port_is_an_error() {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/", free.local_addr().unwrap().port());
        drop(free);
        assert!(send(&prepared("GET", url, &[], None), Duration::from_secs(5)).is_err());
    }

    #[test]
    fn redirects_stay_on_http_and_drop_credentials() {
        let target = TestServer::start(vec![TestServer::reply(200, "OK", &[], "moved here")]);
        let first = TestServer::start(vec![
            TestServer::reply(302, "Found", &[("Location", &target.url("/new"))], ""),
            TestServer::reply(302, "Found", &[("Location", "file:///etc/hosts")], ""),
            TestServer::reply(301, "Moved", &[("Location", "/elsewhere")], ""),
        ]);
        let got =
            send(&prepared("GET", first.url("/old"), &[("Authorization", "Bearer t0k")], None), Duration::from_secs(5))
                .unwrap();
        assert_eq!((got.status, got.body.as_slice()), (200, &b"moved here"[..]));
        assert_eq!(target.finish()[0].header("authorization"), None, "credentials do not follow a redirect");
        let error = send(&prepared("GET", first.url("/file"), &[], None), Duration::from_secs(5)).unwrap_err();
        assert_eq!(error, SendError::RedirectScheme("file".to_owned()));
        assert_eq!(first.finish().len(), 2, "nothing is asked for the file");
        let mut kept = prepared("GET", first.url("/stay"), &[], None);
        kept.follow_redirects = false;
        let got = send(&kept, Duration::from_secs(5)).unwrap();
        assert_eq!(got.status, 301, "# @no-redirect gives the redirect itself");
    }

    #[test]
    fn a_post_redirected_with_302_becomes_a_get_on_the_same_server() {
        let server = TestServer::start(vec![
            TestServer::reply(302, "Found", &[("Location", "done")], ""),
            TestServer::reply(200, "OK", &[], "ok"),
        ]);
        let got = send(
            &prepared(
                "POST",
                server.url("/a/form"),
                &[("Authorization", "Basic x"), ("Content-Type", "text/plain")],
                Some("body"),
            ),
            Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(got.status, 200);
        let seen = server.finish();
        assert_eq!((seen[1].method.as_str(), seen[1].target.as_str()), ("GET", "/a/done"));
        assert!(seen[1].body.is_empty());
        assert_eq!(seen[1].header("authorization"), Some("Basic x"), "the same server keeps the credentials");
        assert_eq!(seen[1].header("content-type"), None);
    }

    #[test]
    fn redirect_targets() {
        assert_eq!(redirect_target("http://h:1/a/b?c", "d").unwrap(), "http://h:1/a/d");
        assert_eq!(redirect_target("https://h/a/b", "/x?y").unwrap(), "https://h/x?y");
        assert_eq!(redirect_target("https://h/a", "//g/z").unwrap(), "https://g/z");
        assert_eq!(redirect_target("http://h", "next").unwrap(), "http://h/next");
        assert_eq!(redirect_target("http://h/", "HTTPS://g/").unwrap(), "HTTPS://g/");
        for refused in ["file:///etc/hosts", "javascript:alert(1)", "ftp://g/", "data:text/plain,x"] {
            assert!(redirect_target("http://h/", refused).is_err(), "{refused}");
        }
        assert_eq!(origin("HTTP://H:1/a?b"), "http://h:1");
    }

    #[test]
    fn a_body_over_the_limit_is_refused() {
        let server = TestServer::start(vec![
            TestServer::reply(200, "OK", &[], &"x".repeat(2048)),
            TestServer::reply(200, "OK", &[], &"x".repeat(1024)),
        ]);
        assert!(send_limited(&prepared("GET", server.url("/big"), &[], None), Duration::from_secs(5), 1024).is_err());
        let fits =
            send_limited(&prepared("GET", server.url("/fits"), &[], None), Duration::from_secs(5), 2048).unwrap();
        assert_eq!(fits.body.len(), 1024);
        assert_eq!(BODY_LIMIT, 16 << 20);
    }
}

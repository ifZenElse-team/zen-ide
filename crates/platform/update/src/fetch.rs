use std::io::Read;

use crate::{Artifact, Manifest, UpdateError, check_artifact, verify_manifest};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchError(pub String);

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FetchError {}

impl From<UpdateError> for FetchError {
    fn from(error: UpdateError) -> Self {
        FetchError(error.to_string())
    }
}

pub fn manifest(manifest_url: &str, public_key: &str) -> Result<Manifest, FetchError> {
    let bytes = get(manifest_url, 64 << 20, |_, _| {})?;
    let signature = get(&format!("{manifest_url}.sig"), 1 << 10, |_, _| {})?;
    let signature =
        String::from_utf8(signature).map_err(|_| FetchError("the manifest signature is not text".to_owned()))?;
    Ok(verify_manifest(&bytes, signature.trim(), public_key)?)
}

pub fn signed(url: &str, public_key: &str) -> Result<Vec<u8>, FetchError> {
    let bytes = get(url, 16 << 20, |_, _| {})?;
    let signature = get(&format!("{url}.sig"), 1 << 10, |_, _| {})?;
    let signature = String::from_utf8(signature).map_err(|_| FetchError("the signature is not text".to_owned()))?;
    crate::verify_signature(&bytes, signature.trim(), public_key)?;
    Ok(bytes)
}

pub fn checked(url: &str, size: u64, sha256: &str) -> Result<Vec<u8>, FetchError> {
    let artifact = Artifact { platform: String::new(), url: url.to_owned(), sha256: sha256.to_owned(), size };
    let bytes = get(url, size.saturating_add(1), |_, _| {})?;
    check_artifact(&artifact, &bytes)?;
    Ok(bytes)
}

pub fn checked_with_progress(
    url: &str,
    size: u64,
    sha256: &str,
    progress: impl FnMut(u64, u64),
) -> Result<Vec<u8>, FetchError> {
    let artifact = Artifact { platform: String::new(), url: url.to_owned(), sha256: sha256.to_owned(), size };
    let bytes = get(url, size.saturating_add(1), progress)?;
    check_artifact(&artifact, &bytes)?;
    Ok(bytes)
}

const MAX_UNSIZED_ARTIFACT: u64 = 4 << 30;

pub fn content_length(url: &str, timeout: std::time::Duration) -> Result<Option<u64>, FetchError> {
    let agent = zen_net::agent(zen_net::config_for(url).timeout_global(Some(timeout)));
    let response = agent.head(url).call().map_err(|error| FetchError(format!("cannot reach {url}: {error}")))?;
    Ok(content_length_of(response.headers()))
}

fn content_length_of(headers: &ureq::http::HeaderMap) -> Option<u64> {
    headers
        .get(ureq::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse().ok())
        .filter(|&size| size > 0)
}

pub fn artifact(
    manifest_url: &str,
    artifact: &Artifact,
    progress: impl FnMut(u64, u64),
) -> Result<Vec<u8>, FetchError> {
    let url = resolve(manifest_url, &artifact.url);
    let limit = if artifact.size == 0 { MAX_UNSIZED_ARTIFACT } else { artifact.size.saturating_add(1) };
    let bytes = get(&url, limit, progress)?;
    check_artifact(artifact, &bytes)?;
    Ok(bytes)
}

pub fn download(url: &str, limit: u64, progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, FetchError> {
    get(url, limit, progress)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebAnswer {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub bytes: Vec<u8>,
}

impl WebAnswer {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(key, _)| key.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }
}

pub fn with_headers(
    url: &str,
    headers: &[(String, String)],
    limit: u64,
    timeout: std::time::Duration,
) -> Result<WebAnswer, FetchError> {
    let agent = zen_net::agent(
        zen_net::config_for(url).timeout_global(Some(timeout)).http_status_as_error(false).https_only(true),
    );
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(name, value);
    }
    let response = request.call().map_err(|error| FetchError(format!("cannot download {url}: {error}")))?;
    let status = response.status().as_u16();
    let answered = response
        .headers()
        .iter()
        .filter_map(|(name, value)| Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned())))
        .collect();
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_with_config()
        .limit(limit)
        .reader()
        .read_to_end(&mut bytes)
        .map_err(|error| FetchError(format!("the download of {url} failed: {error}")))?;
    Ok(WebAnswer { status, headers: answered, bytes })
}

pub fn document(url: &str, limit: u64, timeout: std::time::Duration) -> Result<Option<Vec<u8>>, FetchError> {
    let agent = zen_net::agent(zen_net::config_for(url).timeout_global(Some(timeout)));
    let response = match agent.get(url).call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404 | 410)) => return Ok(None),
        Err(error) => return Err(FetchError(format!("cannot download {url}: {error}"))),
    };
    read_body(response, url, limit).map(Some)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validators {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Conditional {
    Unchanged,
    Missing,
    Changed { body: Vec<u8>, validators: Validators },
}

pub fn conditional(
    url: &str,
    limit: u64,
    timeout: std::time::Duration,
    validators: &Validators,
    authorization: Option<&str>,
) -> Result<Conditional, FetchError> {
    let agent = zen_net::agent(zen_net::config_for(url).timeout_global(Some(timeout)));
    let mut request = agent.get(url);
    if let Some(etag) = &validators.etag {
        request = request.header(ureq::http::header::IF_NONE_MATCH, etag);
    }
    if let Some(modified) = &validators.last_modified {
        request = request.header(ureq::http::header::IF_MODIFIED_SINCE, modified);
    }
    if let Some(authorization) = authorization {
        request = request.header(ureq::http::header::AUTHORIZATION, authorization);
    }
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(404 | 410)) => return Ok(Conditional::Missing),
        Err(error) => return Err(FetchError(format!("cannot download {url}: {error}"))),
    };
    if response.status() == ureq::http::StatusCode::NOT_MODIFIED {
        return Ok(Conditional::Unchanged);
    }
    let header = |name: ureq::http::header::HeaderName| {
        response.headers().get(name).and_then(|value| value.to_str().ok()).map(str::to_owned)
    };
    let validators =
        Validators { etag: header(ureq::http::header::ETAG), last_modified: header(ureq::http::header::LAST_MODIFIED) };
    let body = read_body(response, url, limit)?;
    Ok(Conditional::Changed { body, validators })
}

pub fn post_json(url: &str, body: &[u8], limit: u64, timeout: std::time::Duration) -> Result<Vec<u8>, FetchError> {
    let agent = zen_net::agent(zen_net::config_for(url).timeout_global(Some(timeout)));
    let response = agent
        .post(url)
        .header(ureq::http::header::CONTENT_TYPE, "application/json")
        .send(body)
        .map_err(|error| FetchError(format!("cannot ask {url}: {error}")))?;
    read_body(response, url, limit)
}

fn read_body(response: ureq::http::Response<ureq::Body>, url: &str, limit: u64) -> Result<Vec<u8>, FetchError> {
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_with_config()
        .limit(limit)
        .reader()
        .read_to_end(&mut bytes)
        .map_err(|error| FetchError(format!("the download of {url} failed: {error}")))?;
    Ok(bytes)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonAnswer {
    NotModified,
    Fresh { bytes: Vec<u8>, etag: Option<String>, last_modified: Option<String> },
}

pub fn conditional_json(
    url: &str,
    etag: Option<&str>,
    last_modified: Option<&str>,
    limit: u64,
    timeout: std::time::Duration,
) -> Result<JsonAnswer, FetchError> {
    let agent = zen_net::agent(zen_net::config_for(url).timeout_global(Some(timeout)));
    let mut request = agent.get(url).header("Accept", "application/json");
    if let Some(etag) = etag {
        request = request.header("If-None-Match", etag);
    }
    if let Some(last_modified) = last_modified {
        request = request.header("If-Modified-Since", last_modified);
    }
    let response = request.call().map_err(|error| FetchError(format!("cannot download {url}: {error}")))?;
    if response.status() == 304 {
        return Ok(JsonAnswer::NotModified);
    }
    let header = |name: &str| response.headers().get(name).and_then(|value| value.to_str().ok()).map(str::to_owned);
    let (etag, last_modified) = (header("etag"), header("last-modified"));
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_with_config()
        .limit(limit)
        .reader()
        .read_to_end(&mut bytes)
        .map_err(|error| FetchError(format!("the download of {url} failed: {error}")))?;
    Ok(JsonAnswer::Fresh { bytes, etag, last_modified })
}

pub fn resolve(manifest_url: &str, relative: &str) -> String {
    if is_absolute(relative) && allowed(relative) {
        return relative.to_owned();
    }
    match manifest_url.rfind('/') {
        Some(index) => format!("{}/{relative}", &manifest_url[..index]),
        None => relative.to_owned(),
    }
}

pub fn allowed(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        return true;
    }
    let Some(rest) = lower.strip_prefix("http://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
    let host = if host.starts_with('[') {
        host.split(']').next().map(|host| host.trim_start_matches('[')).unwrap_or_default()
    } else {
        host.split(':').next().unwrap_or_default()
    };
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn is_absolute(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, _)| {
        !scheme.is_empty()
            && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
    })
}

fn get(url: &str, limit: u64, mut progress: impl FnMut(u64, u64)) -> Result<Vec<u8>, FetchError> {
    let response = zen_net::agent_for(url)
        .get(url)
        .call()
        .map_err(|error| FetchError(format!("cannot download {url}: {error}")))?;
    let total = response.body().content_length().unwrap_or(0);
    let mut reader = response.into_body().into_with_config().limit(limit).reader();
    let mut bytes = Vec::with_capacity(total.min(limit) as usize);
    let mut chunk = vec![0u8; 64 << 10];
    loop {
        let read =
            reader.read(&mut chunk).map_err(|error| FetchError(format!("the download of {url} failed: {error}")))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        progress(bytes.len() as u64, total);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::net::TcpListener;
    use std::time::Duration;

    use super::*;

    fn serve(answers: Vec<&'static str>) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/file", listener.local_addr().unwrap());
        let thread = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                let mut seen = Vec::new();
                let mut chunk = [0; 2048];
                while !String::from_utf8_lossy(&seen).contains("\r\n\r\n") {
                    let read = stream.read(&mut chunk).unwrap();
                    seen.extend_from_slice(&chunk[..read]);
                }
                let text = String::from_utf8_lossy(&seen).into_owned();
                if let Some(length) = text
                    .lines()
                    .find_map(|line| line.to_ascii_lowercase().strip_prefix("content-length: ").map(str::to_owned))
                {
                    let length: usize = length.trim().parse().unwrap();
                    let head = text.find("\r\n\r\n").unwrap() + 4;
                    while seen.len() < head + length {
                        let read = stream.read(&mut chunk).unwrap();
                        seen.extend_from_slice(&chunk[..read]);
                    }
                }
                requests.push(String::from_utf8_lossy(&seen).to_ascii_lowercase());
                stream.write_all(answer.as_bytes()).unwrap();
            }
            requests
        });
        (url, thread)
    }

    #[test]
    fn a_conditional_request_sends_its_validators_and_reads_the_new_ones() {
        let changed =
            "HTTP/1.1 200 OK\r\nETag: \"b\"\r\nLast-Modified: Tue\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
        let same = "HTTP/1.1 304 Not Modified\r\nConnection: close\r\n\r\n";
        let gone = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
        let (url, server) = serve(vec![changed, same, gone]);
        let wait = Duration::from_secs(10);
        let known = Validators { etag: Some("\"a\"".to_owned()), last_modified: Some("Mon".to_owned()) };
        let answer = conditional(&url, 100, wait, &known, Some("Basic abc")).unwrap();
        let Conditional::Changed { body, validators } = answer else { panic!("not changed") };
        assert_eq!(body, b"ok");
        assert_eq!(validators, Validators { etag: Some("\"b\"".to_owned()), last_modified: Some("Tue".to_owned()) });
        assert_eq!(conditional(&url, 100, wait, &known, None).unwrap(), Conditional::Unchanged);
        assert_eq!(conditional(&url, 100, wait, &Validators::default(), None).unwrap(), Conditional::Missing);
        let requests = server.join().unwrap();
        assert!(requests[0].contains("if-none-match: \"a\"") && requests[0].contains("if-modified-since: mon"));
        assert!(requests[0].contains("authorization: basic abc"));
        assert!(!requests[1].contains("authorization"));
    }

    #[test]
    fn a_password_is_not_sent_after_a_redirect() {
        let ok = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
        let (other, other_server) = serve(vec![ok]);
        let moved: &'static str = Box::leak(
            format!("HTTP/1.1 302 Found\r\nLocation: {other}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .into_boxed_str(),
        );
        let (url, server) = serve(vec![moved]);
        let answer =
            conditional(&url, 100, Duration::from_secs(10), &Validators::default(), Some("Basic secret")).unwrap();
        assert!(matches!(answer, Conditional::Changed { .. }), "the redirect is followed: {answer:?}");
        assert!(server.join().unwrap()[0].contains("authorization: basic secret"));
        assert!(!other_server.join().unwrap()[0].contains("authorization"), "another host gets no password");
    }

    #[test]
    fn json_is_posted_and_the_answer_read() {
        let ok = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}";
        let (url, server) = serve(vec![ok]);
        assert_eq!(post_json(&url, b"{\"a\":1}", 100, Duration::from_secs(10)).unwrap(), b"{}");
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("post ") && requests[0].contains("content-type: application/json"));
        assert!(requests[0].ends_with("{\"a\":1}"));
    }

    #[test]
    fn content_lengths_are_read_from_headers() {
        let mut headers = ureq::http::HeaderMap::new();
        assert_eq!(content_length_of(&headers), None);
        headers.insert(ureq::http::header::CONTENT_LENGTH, "41234567".parse().unwrap());
        assert_eq!(content_length_of(&headers), Some(41_234_567));
        headers.insert(ureq::http::header::CONTENT_LENGTH, "0".parse().unwrap());
        assert_eq!(content_length_of(&headers), None, "an empty answer says nothing about the file");
        headers.insert(ureq::http::header::CONTENT_LENGTH, "many".parse().unwrap());
        assert_eq!(content_length_of(&headers), None);
    }

    #[test]
    fn relative_urls() {
        assert_eq!(
            resolve("http://h/releases/manifest.json", "artifacts/a/1/a.tar.gz"),
            "http://h/releases/artifacts/a/1/a.tar.gz"
        );
    }

    #[test]
    fn absolute_urls_are_kept() {
        let catalog = "https://ide.ifzenelse.space/releases/plugins.json";
        assert_eq!(
            resolve(catalog, "https://example.org/zen-duckdb-macos-aarch64"),
            "https://example.org/zen-duckdb-macos-aarch64"
        );
        assert_eq!(resolve(catalog, "http://127.0.0.1:8787/x"), "http://127.0.0.1:8787/x");
        assert_eq!(
            resolve(catalog, "artifacts/plugin-duckdb/0.1.0/zen-db-duckdb"),
            "https://ide.ifzenelse.space/releases/artifacts/plugin-duckdb/0.1.0/zen-db-duckdb"
        );
        assert_eq!(
            resolve(catalog, "http://example.org/x"),
            "https://ide.ifzenelse.space/releases/http://example.org/x",
            "no plain HTTP but to this computer"
        );
        assert_eq!(
            resolve(catalog, "http://127.0.0.1.example.org/x"),
            "https://ide.ifzenelse.space/releases/http://127.0.0.1.example.org/x"
        );
        assert_eq!(resolve(catalog, "file:///etc/passwd"), "https://ide.ifzenelse.space/releases/file:///etc/passwd");
        assert_eq!(resolve(catalog, "http://[::1]:8787/x"), "http://[::1]:8787/x");
        assert_eq!(
            resolve(catalog, "drivers/a://b"),
            "https://ide.ifzenelse.space/releases/drivers/a://b",
            "a path is not a scheme"
        );
    }

    #[test]
    #[ignore = "needs the network"]
    fn the_official_manifest_comes_over_https() {
        let manifest = manifest(crate::OFFICIAL_MANIFEST_URL, crate::OFFICIAL_PUBLIC_KEY).unwrap();
        assert!(!manifest.products.is_empty());
        assert!(signed(crate::OFFICIAL_PLUGINS_URL, crate::OFFICIAL_PUBLIC_KEY).is_ok());
        assert!(get("https://expired.badssl.com/", 1 << 20, |_, _| {}).is_err(), "an expired certificate is refused");
    }
}

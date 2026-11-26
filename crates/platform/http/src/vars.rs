use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::parse::Request;

const DEPTH: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Variables {
    pub file: BTreeMap<String, String>,
    pub environment: BTreeMap<String, String>,
    pub private: BTreeSet<String>,
}

impl Variables {
    pub fn new(file: &[(String, String)], environment: BTreeMap<String, String>) -> Self {
        Self { file: file.iter().cloned().collect(), environment, private: BTreeSet::new() }
    }

    #[must_use]
    pub fn with_private(mut self, private: BTreeSet<String>) -> Self {
        self.private = private;
        self
    }

    pub fn resolve(&self, text: &str) -> Result<String, Vec<String>> {
        self.resolve_shown(text).map(|(sent, _)| sent)
    }

    pub fn resolve_shown(&self, text: &str) -> Result<(String, String), Vec<String>> {
        let mut missing = Vec::new();
        let mut resolved = (String::with_capacity(text.len()), String::with_capacity(text.len()));
        self.resolve_at(text, 0, &mut missing, &mut resolved, false);
        if missing.is_empty() { Ok(resolved) } else { Err(missing) }
    }

    fn resolve_at(
        &self,
        text: &str,
        depth: usize,
        missing: &mut Vec<String>,
        out: &mut (String, String),
        hidden: bool,
    ) {
        let mut rest = text;
        while let Some(open) = rest.find("{{") {
            push(out, &rest[..open], hidden);
            let after = &rest[open + 2..];
            let Some(close) = after.find("}}") else {
                push(out, &rest[open..], hidden);
                return;
            };
            let name = after[..close].trim();
            let written = &rest[open..open + 2 + close + 2];
            match self.value(name) {
                Some(value) if depth < DEPTH => {
                    let private = !self.file.contains_key(name) && self.private.contains(name);
                    if private && !hidden {
                        out.1.push_str(&format!("{{{{{name}}}}}"));
                    }
                    self.resolve_at(&value, depth + 1, missing, out, hidden || private);
                }
                _ => {
                    if !missing.iter().any(|known| known == name) {
                        missing.push(name.to_owned());
                    }
                    push(out, written, hidden);
                }
            }
            rest = &after[close + 2..];
        }
        push(out, rest, hidden);
    }

    fn value(&self, name: &str) -> Option<String> {
        if let Some(value) = self.file.get(name).or_else(|| self.environment.get(name)) {
            return Some(value.clone());
        }
        match name {
            "$uuid" | "$random.uuid" => Some(uuid()),
            "$timestamp" => Some(now_seconds().to_string()),
            "$randomInt" => Some((random() % 1000).to_string()),
            _ => None,
        }
    }
}

fn push(out: &mut (String, String), text: &str, hidden: bool) {
    out.0.push_str(text);
    if !hidden {
        out.1.push_str(text);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prepared {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub follow_redirects: bool,
    pub use_proxy: bool,
    pub shown: Shown,
    pub files: Vec<PathBuf>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrepareError {
    Unresolved(Vec<String>),
    BodyFile { path: String, error: String },
    BodyOutside { path: String },
    NoUrl,
    Scheme(String),
}

pub fn prepare(
    request: &Request,
    variables: &Variables,
    folder: &Path,
    root: Option<&Path>,
) -> Result<Prepared, PrepareError> {
    let mut missing = Vec::new();
    let mut resolve = |text: &str| match variables.resolve_shown(text) {
        Ok(resolved) => resolved,
        Err(names) => {
            for name in names {
                if !missing.contains(&name) {
                    missing.push(name);
                }
            }
            (text.to_owned(), text.to_owned())
        }
    };
    let url = resolve(&request.url);
    let (headers, shown_headers): (Vec<_>, Vec<_>) = request
        .headers
        .iter()
        .filter(|header| !header.name.eq_ignore_ascii_case("content-length"))
        .map(|header| {
            let (sent, shown) = resolve(&header.value);
            ((header.name.clone(), sent), (header.name.clone(), shown))
        })
        .unzip();
    let body = request.body.as_deref().map(&mut resolve);
    if !missing.is_empty() {
        return Err(PrepareError::Unresolved(missing));
    }
    if url.0.trim().is_empty() {
        return Err(PrepareError::NoUrl);
    }
    let full = full_url(&url.0, &headers);
    if let Some((scheme, _)) = full.split_once("://")
        && !scheme.eq_ignore_ascii_case("http")
        && !scheme.eq_ignore_ascii_case("https")
    {
        return Err(PrepareError::Scheme(scheme.to_owned()));
    }
    let root = root.unwrap_or(folder);
    let mut files = Vec::new();
    let (body, shown_body) = match body {
        Some((sent, shown)) => {
            let shown_bytes = if shown == sent { None } else { Some(with_files(&shown, folder, root, &mut files)?) };
            let sent = with_files(&sent, folder, root, &mut files)?;
            let shown_bytes = shown_bytes.unwrap_or_else(|| sent.clone());
            (Some(sent), Some(shown_bytes))
        }
        None => (None, None),
    };
    Ok(Prepared {
        method: request.method.clone(),
        shown: Shown { url: full_url(&url.1, &shown_headers), headers: shown_headers, body: shown_body },
        url: full,
        headers,
        body,
        follow_redirects: !request.no_redirect,
        use_proxy: !request.no_proxy,
        files,
    })
}

fn full_url(url: &str, headers: &[(String, String)]) -> String {
    if url.contains("://") {
        return url.to_owned();
    }
    if url.starts_with('/')
        && let Some((_, host)) = headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("host"))
    {
        return format!("http://{host}{url}");
    }
    format!("http://{url}")
}

fn with_files(body: &str, folder: &Path, root: &Path, files: &mut Vec<PathBuf>) -> Result<Vec<u8>, PrepareError> {
    let mut out = Vec::with_capacity(body.len());
    for (number, line) in body.split('\n').enumerate() {
        if number > 0 {
            out.push(b'\n');
        }
        match line.strip_prefix("< ") {
            Some(file) => {
                let file = file.trim();
                let failed =
                    |error: std::io::Error| PrepareError::BodyFile { path: file.to_owned(), error: error.to_string() };
                let path = folder.join(file).canonicalize().map_err(failed)?;
                let root = root.canonicalize().map_err(failed)?;
                if !path.starts_with(&root) {
                    return Err(PrepareError::BodyOutside { path: file.to_owned() });
                }
                let bytes = std::fs::read(&path).map_err(failed)?;
                out.extend_from_slice(&bytes);
                if !files.contains(&path) {
                    files.push(path);
                }
            }
            None => out.extend_from_slice(line.as_bytes()),
        }
    }
    Ok(out)
}

fn now_seconds() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

fn random() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos()));
    hasher.finish()
}

fn uuid() -> String {
    let (high, low) = (random(), random());
    let bytes = ((u128::from(high) << 64) | u128::from(low)) & !(0xF000u128 << 64) & !(0xC000u128 << 48);
    let bytes = bytes | (0x4000u128 << 64) | (0x8000u128 << 48);
    let hex = format!("{bytes:032x}");
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    fn variables(file: &[(&str, &str)], environment: &[(&str, &str)]) -> Variables {
        Variables {
            file: file.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect(),
            environment: environment.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect(),
            private: BTreeSet::new(),
        }
    }

    #[test]
    fn file_variables_win_and_may_use_the_environment() {
        let vars = variables(&[("base", "http://{{host}}/v1"), ("host", "file")], &[("host", "env"), ("id", "7")]);
        assert_eq!(vars.resolve("{{base}}/items/{{ id }}").unwrap(), "http://file/v1/items/7");
        assert_eq!(vars.resolve("no variables {{ unterminated").unwrap(), "no variables {{ unterminated");
        assert_eq!(vars.resolve("{{a}} {{b}} {{a}}").unwrap_err(), ["a", "b"]);
    }

    #[test]
    fn a_loop_of_variables_stops() {
        let vars = variables(&[("a", "{{b}}"), ("b", "{{a}}")], &[]);
        assert!(vars.resolve("{{a}}").is_err());
    }

    #[test]
    fn dynamic_variables() {
        let vars = Variables::default();
        let uuid = vars.resolve("{{$uuid}}").unwrap();
        assert_eq!(uuid.len(), 36);
        assert_eq!(&uuid[14..15], "4");
        assert!(matches!(&uuid[19..20], "8" | "9" | "a" | "b"), "{uuid}");
        assert!(vars.resolve("{{$timestamp}}").unwrap().parse::<u64>().unwrap() > 1_700_000_000);
        assert!(vars.resolve("{{$randomInt}}").unwrap().parse::<u64>().unwrap() < 1000);
    }

    #[test]
    fn prepares_url_headers_and_body() {
        let (requests, file) = parse(
            "@host = 127.0.0.1:9\nPOST {{host}}/items\nContent-Type: application/json\nContent-Length: 999\nAuthorization: Bearer {{token}}\n\n{\"name\": \"{{name}}\"}\n",
        );
        let mut environment = BTreeMap::new();
        environment.insert("token".to_owned(), "t0k".to_owned());
        environment.insert("name".to_owned(), "tea".to_owned());
        let prepared = prepare(&requests[0], &Variables::new(&file, environment), Path::new("/"), None).unwrap();
        assert_eq!(prepared.method, "POST");
        assert_eq!(prepared.url, "http://127.0.0.1:9/items");
        assert_eq!(
            prepared.headers,
            [
                ("Content-Type".to_owned(), "application/json".to_owned()),
                ("Authorization".to_owned(), "Bearer t0k".to_owned())
            ]
        );
        assert_eq!(prepared.body.as_deref(), Some(&b"{\"name\": \"tea\"}"[..]));
        assert!(prepared.follow_redirects);
    }

    #[test]
    fn unresolved_variables_are_listed_and_nothing_is_prepared() {
        let (requests, _) = parse("GET http://{{host}}/a\nX: {{token}}\n");
        assert_eq!(
            prepare(&requests[0], &Variables::default(), Path::new("/"), None),
            Err(PrepareError::Unresolved(vec!["host".to_owned(), "token".to_owned()]))
        );
    }

    #[test]
    fn relative_urls_take_the_host_header() {
        let (requests, _) = parse("GET /a?b=1\nHost: example.test:81\n");
        assert_eq!(
            prepare(&requests[0], &Variables::default(), Path::new("/"), None).unwrap().url,
            "http://example.test:81/a?b=1"
        );
        let (requests, _) = parse("GET https://example.test/\n");
        assert_eq!(
            prepare(&requests[0], &Variables::default(), Path::new("/"), None).unwrap().url,
            "https://example.test/"
        );
    }

    #[test]
    fn body_files_are_read_from_the_request_files_folder() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("payload.json"), "{\"from\": \"file\"}").unwrap();
        let (requests, _) = parse("POST http://h/\n\n< payload.json\n");
        let prepared = prepare(&requests[0], &Variables::default(), folder.path(), None).unwrap();
        assert_eq!(prepared.body.as_deref(), Some(&b"{\"from\": \"file\"}"[..]));
        let (requests, _) = parse("POST http://h/\n\n< missing.json\n");
        assert!(matches!(
            prepare(&requests[0], &Variables::default(), folder.path(), None),
            Err(PrepareError::BodyFile { path, .. }) if path == "missing.json"
        ));
    }

    #[test]
    fn private_values_are_sent_but_never_shown() {
        let (requests, file) = parse(
            "@auth = Bearer {{token}}\nPOST http://{{host}}/login?key={{token}}\nAuthorization: {{auth}}\n\n{\"password\": \"{{password}}\"}\n",
        );
        let environment: BTreeMap<String, String> =
            [("host", "127.0.0.1:9"), ("token", "t0k"), ("password", "p{{host}}")]
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect();
        let private = ["token".to_owned(), "password".to_owned()].into_iter().collect();
        let variables = Variables::new(&file, environment).with_private(private);
        let prepared = prepare(&requests[0], &variables, Path::new("/"), None).unwrap();
        assert_eq!(prepared.url, "http://127.0.0.1:9/login?key=t0k");
        assert_eq!(prepared.headers[0].1, "Bearer t0k");
        assert_eq!(prepared.body.as_deref(), Some(&b"{\"password\": \"p127.0.0.1:9\"}"[..]));
        assert_eq!(prepared.shown.url, "http://127.0.0.1:9/login?key={{token}}");
        assert_eq!(prepared.shown.headers[0].1, "Bearer {{token}}");
        assert_eq!(prepared.shown.body.as_deref(), Some(&b"{\"password\": \"{{password}}\"}"[..]));
        let (requests, file) = parse("@token = mine\nGET http://h/?t={{token}}\n");
        let private = ["token".to_owned()].into_iter().collect();
        let shadowed = Variables::new(&file, BTreeMap::new()).with_private(private);
        assert_eq!(prepare(&requests[0], &shadowed, Path::new("/"), None).unwrap().shown.url, "http://h/?t=mine");
    }

    #[test]
    fn only_http_and_https_are_sent() {
        for url in ["file:///etc/hosts", "ftp://h/a", "gopher://h/"] {
            let (requests, _) = parse(&format!("GET {url}\n"));
            assert!(
                matches!(
                    prepare(&requests[0], &Variables::default(), Path::new("/"), None),
                    Err(PrepareError::Scheme(_))
                ),
                "{url}"
            );
        }
        let (requests, _) = parse("GET HTTPS://h/\n");
        assert!(prepare(&requests[0], &Variables::default(), Path::new("/"), None).is_ok());
    }

    #[test]
    fn body_files_stay_inside_the_project() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
        let project = tempfile::tempdir().unwrap();
        let api = project.path().join("api");
        std::fs::create_dir_all(&api).unwrap();
        std::fs::write(project.path().join("shared.json"), "{}").unwrap();
        let (requests, _) = parse("POST http://h/\n\n< ../shared.json\n");
        let prepared = prepare(&requests[0], &Variables::default(), &api, Some(project.path())).unwrap();
        assert_eq!(prepared.body.as_deref(), Some(&b"{}"[..]));
        assert!(matches!(
            prepare(&requests[0], &Variables::default(), &api, None),
            Err(PrepareError::BodyOutside { .. })
        ));
        let escape = format!("POST http://h/\n\n< {}\n", outside.path().join("secret.txt").display());
        let (requests, _) = parse(&escape);
        assert!(matches!(
            prepare(&requests[0], &Variables::default(), &api, Some(project.path())),
            Err(PrepareError::BodyOutside { .. })
        ));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path().join("secret.txt"), api.join("link.txt")).unwrap();
            let (requests, _) = parse("POST http://h/\n\n< link.txt\n");
            assert!(matches!(
                prepare(&requests[0], &Variables::default(), &api, Some(project.path())),
                Err(PrepareError::BodyOutside { .. })
            ));
        }
    }
}

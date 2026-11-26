use std::path::Path;

use zen_update::{Artifact, Manifest, Release, SCHEMA};

use crate::is_safe_segment;

#[derive(Clone, Debug)]
pub struct Publication<'a> {
    pub server: &'a str,
    pub token: &'a str,
    pub secret_key: &'a str,
    pub product: &'a str,
    pub name: &'a str,
    pub version: &'a str,
    pub channel: &'a str,
    pub platform: &'a str,
    pub notes: &'a str,
    pub file: &'a Path,
    pub stage: Option<&'a str>,
}

pub fn publish(publication: &Publication<'_>) -> Result<Manifest, String> {
    let p = publication;
    if !is_safe_segment(p.product) || !is_safe_segment(p.version) {
        return Err("the product and version may contain only letters, digits, '.', '_' and '-'".to_owned());
    }
    if p.channel != "stable" && p.channel != "nightly" {
        return Err(format!("unknown channel `{}`; use stable or nightly", p.channel));
    }
    semver::Version::parse(p.version).map_err(|error| format!("`{}` is not a semantic version: {error}", p.version))?;
    let file_name = p
        .file
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_safe_segment(name))
        .ok_or("the artifact's file name may contain only letters, digits, '.', '_' and '-'")?;
    let bytes = std::fs::read(p.file).map_err(|error| format!("cannot read {}: {error}", p.file.display()))?;
    let public_key = zen_update::public_key_of(p.secret_key).map_err(|error| error.to_string())?;
    let base = p.server.trim_end_matches('/');

    let live = current_document(base, "releases/manifest.json")?;
    let staged = match p.stage {
        Some(stage) => current_document(base, &format!("releases/stage/{stage}/manifest.json"))?,
        None => None,
    };
    let mut manifest = match staged.as_ref().or(live.as_ref()) {
        Some((bytes, signature)) => zen_update::verify_manifest(bytes, signature, &public_key)
            .map_err(|error| format!("{error}; was it signed with a different key?"))?,
        None => Manifest { schema: SCHEMA, products: Vec::new() },
    };

    let artifact = Artifact {
        platform: p.platform.to_owned(),
        url: format!("artifacts/{}/{}/{file_name}", p.product, p.version),
        sha256: zen_update::sha256_hex(&bytes),
        size: bytes.len() as u64,
    };
    let release = Release {
        version: p.version.to_owned(),
        channel: p.channel.to_owned(),
        date: crate::today(),
        notes: p.notes.to_owned(),
        artifacts: vec![artifact],
    };
    manifest.add(p.product, p.name, release);

    let manifest_bytes = manifest.to_bytes();
    let signature = zen_update::sign_manifest(&manifest_bytes, p.secret_key).map_err(|error| error.to_string())?;
    check(base, p.token, "manifest", &manifest_bytes, &signature)?;
    upload_bytes(base, p.token, p.product, p.version, file_name, &bytes)?;
    send_signed(base, p.token, "manifest", p.stage, &base_of(live.as_ref()), &manifest_bytes, &signature)?;
    Ok(manifest)
}

pub fn set_notes(
    server: &str,
    token: &str,
    secret_key: &str,
    product: &str,
    version: &str,
    notes: &str,
) -> Result<bool, String> {
    let base = server.trim_end_matches('/');
    let public_key = zen_update::public_key_of(secret_key).map_err(|error| error.to_string())?;
    let mut manifest = current(base, &public_key)?.ok_or("nothing is published yet")?;
    let mut changed = false;
    for release in manifest
        .products
        .iter_mut()
        .filter(|entry| entry.id == product)
        .flat_map(|entry| entry.releases.iter_mut())
        .filter(|release| release.version == version)
    {
        if release.notes != notes {
            release.notes = notes.to_owned();
            changed = true;
        }
    }
    if !changed {
        return Ok(false);
    }
    let bytes = manifest.to_bytes();
    let signature = zen_update::sign_manifest(&bytes, secret_key).map_err(|error| error.to_string())?;
    check(base, token, "manifest", &bytes, &signature)?;
    ureq::put(format!("{base}/api/v1/manifest"))
        .header("Authorization", format!("Bearer {token}"))
        .header("X-Zen-Signature", &signature)
        .header("Content-Type", "application/json")
        .send(&bytes[..])
        .map_err(|error| format!("the server refused the manifest: {error}"))?;
    Ok(true)
}

pub fn withdraw(
    server: &str,
    token: &str,
    secret_key: &str,
    product: &str,
    version: Option<&str>,
) -> Result<usize, String> {
    let base = server.trim_end_matches('/');
    let public_key = zen_update::public_key_of(secret_key).map_err(|error| error.to_string())?;
    let mut manifest = current(base, &public_key)?.ok_or("nothing is published yet")?;
    let withdrawn = manifest.withdraw(product, version);
    if withdrawn == 0 {
        return Err(format!("no matching release of `{product}` is published"));
    }
    let bytes = manifest.to_bytes();
    let signature = zen_update::sign_manifest(&bytes, secret_key).map_err(|error| error.to_string())?;
    check(base, token, "manifest", &bytes, &signature)?;
    ureq::put(format!("{base}/api/v1/manifest"))
        .header("Authorization", format!("Bearer {token}"))
        .header("X-Zen-Signature", &signature)
        .header("Content-Type", "application/json")
        .send(&bytes[..])
        .map_err(|error| format!("the server refused the manifest: {error}"))?;
    Ok(withdrawn)
}

pub fn check(base: &str, token: &str, kind: &str, bytes: &[u8], signature: &str) -> Result<(), String> {
    let deploy =
        "deploy the release server first (deploy/releases/README.md), then publish again; nothing was published";
    let answer = ureq::put(format!("{base}/api/v1/check/{kind}"))
        .config()
        .http_status_as_error(false)
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .header("X-Zen-Signature", signature)
        .header("Content-Type", "application/json")
        .send(bytes);
    let mut answer = answer.map_err(|error| format!("cannot ask the release server to check the {kind}: {error}"))?;
    let status = answer.status().as_u16();
    let why = answer.body_mut().read_to_string().unwrap_or_default();
    match status {
        200..=299 => Ok(()),
        404 | 405 => {
            Err(format!("the release server is older than this publisher: it cannot check the {kind}; {deploy}"))
        }
        422 => Err(format!("the release server cannot read this {kind}: {why}; {deploy}")),
        other => Err(format!("the release server answered {other} to the check of the {kind}: {why}")),
    }
}

pub fn current(base: &str, public_key: &str) -> Result<Option<Manifest>, String> {
    let manifest = match ureq::get(format!("{base}/releases/manifest.json")).call() {
        Ok(mut response) => {
            response.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|error| error.to_string())?
        }
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(error) => return Err(format!("cannot fetch the manifest: {error}")),
    };
    let signature = ureq::get(format!("{base}/releases/manifest.json.sig"))
        .call()
        .map_err(|error| format!("cannot fetch the signature: {error}"))?
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    zen_update::verify_manifest(&manifest, &signature, public_key)
        .map(Some)
        .map_err(|error| format!("{error}; was it signed with a different key?"))
}

#[derive(Clone, Debug)]
pub struct PluginPublication<'a> {
    pub server: &'a str,
    pub token: &'a str,
    pub secret_key: &'a str,
    pub plugin: zen_plugins::CatalogPlugin,
    pub version: &'a str,
    pub file: &'a Path,
    pub notes: &'a str,
    pub stage: Option<&'a str>,
}

pub fn publish_plugin(publication: &PluginPublication<'_>) -> Result<zen_plugins::Catalog, String> {
    let p = publication;
    let id = &p.plugin.id;
    if !is_safe_segment(id) || !is_safe_segment(p.version) {
        return Err("the plug-in id and version may contain only letters, digits, '.', '_' and '-'".to_owned());
    }
    semver::Version::parse(p.version).map_err(|error| format!("`{}` is not a semantic version: {error}", p.version))?;
    let bytes = std::fs::read(p.file).map_err(|error| format!("cannot read {}: {error}", p.file.display()))?;
    let loaded =
        zen_plugins::WasmHighlighter::load(id, &bytes, &p.plugin.extensions).map_err(|error| error.to_string())?;
    let public_key = zen_update::public_key_of(p.secret_key).map_err(|error| error.to_string())?;
    let base = p.server.trim_end_matches('/');
    let live = current_document(base, "releases/plugins.json")?;
    let staged = match p.stage {
        Some(stage) => current_document(base, &format!("releases/stage/{stage}/plugins.json"))?,
        None => None,
    };
    let mut catalog = match staged.as_ref().or(live.as_ref()) {
        Some((bytes, signature)) => zen_plugins::Catalog::verify(bytes, signature, &public_key)
            .map_err(|error| format!("{error}; was it signed with a different key?"))?,
        None => zen_plugins::Catalog { schema: zen_plugins::CATALOG_SCHEMA, plugins: Vec::new() },
    };

    let product = format!("plugin-{id}");
    let file_name = format!("{id}.wasm");
    let version = zen_plugins::CatalogVersion {
        version: p.version.to_owned(),
        date: crate::today(),
        abi: loaded.abi(),
        url: format!("artifacts/{product}/{}/{file_name}", p.version),
        sha256: zen_update::sha256_hex(&bytes),
        size: bytes.len() as u64,
        notes: p.notes.trim().to_owned(),
    };
    catalog.add(p.plugin.clone(), version);

    let catalog_bytes = catalog.to_bytes();
    let signature = zen_update::sign_manifest(&catalog_bytes, p.secret_key).map_err(|error| error.to_string())?;
    check(base, p.token, "plugins", &catalog_bytes, &signature)?;
    upload_bytes(base, p.token, &product, p.version, &file_name, &bytes)?;
    send_signed(base, p.token, "plugins", p.stage, &base_of(live.as_ref()), &catalog_bytes, &signature)?;
    Ok(catalog)
}

pub fn current_plugins(base: &str, public_key: &str) -> Result<Option<zen_plugins::Catalog>, String> {
    let bytes = match ureq::get(format!("{base}/releases/plugins.json")).call() {
        Ok(mut response) => {
            response.body_mut().with_config().limit(16 << 20).read_to_vec().map_err(|error| error.to_string())?
        }
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(error) => return Err(format!("cannot fetch the plug-in catalog: {error}")),
    };
    let signature = ureq::get(format!("{base}/releases/plugins.json.sig"))
        .call()
        .map_err(|error| format!("cannot fetch the signature: {error}"))?
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    zen_plugins::Catalog::verify(&bytes, signature.trim(), public_key)
        .map(Some)
        .map_err(|error| format!("{error}; was it signed with a different key?"))
}

pub fn publish_changelog(base: &str, token: &str, json: &[u8]) -> Result<(), String> {
    stage_changelog(base, token, None, json)
}

pub fn stage_changelog(base: &str, token: &str, stage: Option<&str>, json: &[u8]) -> Result<(), String> {
    let base = base.trim_end_matches('/');
    let path = match stage {
        Some(stage) => format!("api/v1/stage/{stage}/changelog"),
        None => "api/v1/changelog".to_owned(),
    };
    ureq::put(format!("{base}/{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send(json)
        .map_err(|error| format!("the server refused the changelog: {error}"))?;
    Ok(())
}

fn current_document(base: &str, path: &str) -> Result<Option<(Vec<u8>, String)>, String> {
    let bytes = match ureq::get(format!("{base}/{path}")).call() {
        Ok(mut response) => {
            response.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|error| error.to_string())?
        }
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(error) => return Err(format!("cannot fetch {path}: {error}")),
    };
    let signature = ureq::get(format!("{base}/{path}.sig"))
        .call()
        .map_err(|error| format!("cannot fetch the signature of {path}: {error}"))?
        .body_mut()
        .read_to_string()
        .map_err(|error| error.to_string())?;
    Ok(Some((bytes, signature.trim().to_owned())))
}

fn base_of(document: Option<&(Vec<u8>, String)>) -> String {
    document.map_or_else(|| "none".to_owned(), |(bytes, _)| zen_update::sha256_hex(bytes))
}

fn send_signed(
    base: &str,
    token: &str,
    kind: &str,
    stage: Option<&str>,
    live_base: &str,
    bytes: &[u8],
    signature: &str,
) -> Result<(), String> {
    let (url, what) = match stage {
        Some(stage) => (format!("{base}/api/v1/stage/{stage}/{kind}"), format!("the staged {kind}")),
        None => (format!("{base}/api/v1/{kind}"), format!("the {kind}")),
    };
    let mut answer = ureq::put(url)
        .config()
        .http_status_as_error(false)
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .header("X-Zen-Signature", signature)
        .header("X-Zen-Base", live_base)
        .header("Content-Type", "application/json")
        .send(bytes)
        .map_err(|error| format!("cannot send {what}: {error}"))?;
    let status = answer.status().as_u16();
    if (200..300).contains(&status) {
        return Ok(());
    }
    let why = answer.body_mut().read_to_string().unwrap_or_default();
    if stage.is_some() && matches!(status, 404 | 405) {
        return Err(format!(
            "the release server cannot stage releases yet (ADR-0171): deploy it first (deploy/releases/README.md); {why}"
        ));
    }
    Err(format!("the server refused {what} ({status}): {why}"))
}

pub fn stage_site(base: &str, token: &str, stage: &str, dir: &Path) -> Result<usize, String> {
    let base = base.trim_end_matches('/');
    let mut files = Vec::new();
    site_files(dir, dir, &mut files)?;
    if !files.iter().any(|(relative, _)| relative == "index.html") {
        return Err(format!("{}: a website needs index.html", dir.display()));
    }
    for (relative, path) in &files {
        let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let url = format!("{base}/api/v1/stage/{stage}/site/{relative}");
        let what = format!("the website's {relative}");
        put_retrying(&url, token, &bytes, &Patience::from_environment(), &what, &|| false)?;
    }
    Ok(files.len())
}

fn site_files(root: &Path, dir: &Path, files: &mut Vec<(String, std::path::PathBuf)>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            site_files(root, &path, files)?;
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| error.to_string())?
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if !relative.split('/').all(is_safe_segment) {
            return Err(format!(
                "{relative}: a website file's name may contain only letters, digits, '.', '_' and '-'"
            ));
        }
        files.push((relative, path));
    }
    Ok(())
}

pub fn stage_summary(base: &str, token: &str, stage: &str) -> Result<serde_json::Value, String> {
    let base = base.trim_end_matches('/');
    let mut answer = ureq::get(format!("{base}/api/v1/stage/{stage}"))
        .header("Authorization", format!("Bearer {token}"))
        .call()
        .map_err(|error| format!("cannot read the stage {stage}: {error}"))?;
    serde_json::from_slice(&answer.body_mut().read_to_vec().map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())
}

pub fn release_stage(base: &str, token: &str, stage: &str) -> Result<serde_json::Value, String> {
    let base = base.trim_end_matches('/');
    let mut answer = ureq::post(format!("{base}/api/v1/stage/{stage}/release"))
        .config()
        .http_status_as_error(false)
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .send_empty()
        .map_err(|error| format!("cannot reach the release server: {error}"))?;
    let status = answer.status().as_u16();
    let body = answer.body_mut().read_to_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!("the release server did not release {stage} ({status}): {body}"));
    }
    serde_json::from_str(&body).map_err(|error| error.to_string())
}

pub fn check_credentials(base: &str, token: &str, secret_key: &str) -> Result<(), String> {
    let base = base.trim_end_matches('/');
    let probe = Manifest { schema: SCHEMA, products: Vec::new() }.to_bytes();
    let signature = zen_update::sign_manifest(&probe, secret_key).map_err(|error| error.to_string())?;
    let mut answer = ureq::put(format!("{base}/api/v1/check/manifest"))
        .config()
        .http_status_as_error(false)
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .header("X-Zen-Signature", &signature)
        .header("Content-Type", "application/json")
        .send(&probe[..])
        .map_err(|error| format!("cannot reach {base}: {error}"))?;
    let status = answer.status().as_u16();
    let _ = answer.body_mut().read_to_vec();
    match status {
        200..=299 => Ok(()),
        401 => Err("the server refused the token (401)".to_owned()),
        403 => Err("the server takes no uploads: it has no token (403)".to_owned()),
        422 => Err("the server does not trust this signing key: its signature does not verify (422)".to_owned()),
        404 | 405 => Err(format!("the server has no check endpoint ({status}): deploy it first")),
        other => Err(format!("the server answered {other}")),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patience {
    pub retries: u32,
    pub pause: std::time::Duration,
    pub connect: std::time::Duration,
    pub answer: std::time::Duration,
    pub least_rate: u64,
    pub send_floor: std::time::Duration,
}

pub const MIN_RATE_ENV: &str = "ZEN_RELEASES_MIN_RATE";

pub const PARALLEL_ENV: &str = "ZEN_RELEASES_UPLOAD_PARALLEL";

pub const LARGE_FILE: u64 = 20 * 1024 * 1024;

impl Default for Patience {
    fn default() -> Self {
        Self {
            retries: 3,
            pause: std::time::Duration::from_secs(5),
            connect: std::time::Duration::from_secs(30),
            answer: std::time::Duration::from_secs(300),
            least_rate: 16 * 1024,
            send_floor: std::time::Duration::from_secs(600),
        }
    }
}

impl Patience {
    pub fn from_environment() -> Self {
        let rate =
            std::env::var(MIN_RATE_ENV).ok().and_then(|value| value.trim().parse::<u64>().ok()).filter(|kib| *kib > 0);
        let mut patience = Self::default();
        if let Some(kib) = rate {
            patience.least_rate = kib * 1024;
        }
        patience
    }
}

pub const BUFFERED: u64 = 16 * 1024 * 1024;

impl Patience {
    fn for_body(&self, size: usize) -> std::time::Duration {
        self.send_floor + std::time::Duration::from_secs(size as u64 / self.least_rate.max(1))
    }

    pub fn for_answer(&self, size: usize) -> std::time::Duration {
        let unsent = (size as u64).min(BUFFERED);
        self.answer + std::time::Duration::from_secs(unsent / self.least_rate.max(1))
    }
}

const EINVAL: i32 = 22;

pub fn dropped(error: &ureq::Error) -> bool {
    use std::io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, TimedOut, UnexpectedEof};
    match error {
        ureq::Error::Io(io) => {
            matches!(io.kind(), BrokenPipe | ConnectionReset | ConnectionAborted | UnexpectedEof | TimedOut)
                || io.raw_os_error() == Some(EINVAL)
        }
        ureq::Error::Timeout(_)
        | ureq::Error::ConnectionFailed
        | ureq::Error::Protocol(_)
        | ureq::Error::BodyStalled => true,
        _ => false,
    }
}

fn stored_sha(url: &str, token: &str, patience: &Patience) -> Option<(u64, String)> {
    let mut answer = ureq::get(url)
        .config()
        .http_status_as_error(false)
        .timeout_connect(Some(patience.connect))
        .timeout_recv_response(Some(patience.answer))
        .timeout_recv_body(Some(patience.answer))
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .call()
        .ok()
        .filter(|answer| answer.status().as_u16() == 200)?;
    let stored: serde_json::Value = serde_json::from_slice(&answer.body_mut().read_to_vec().ok()?).ok()?;
    Some((stored["size"].as_u64()?, stored["sha256"].as_str()?.to_owned()))
}

pub fn upload_bytes(
    base: &str,
    token: &str,
    product: &str,
    version: &str,
    file: &str,
    bytes: &[u8],
) -> Result<bool, String> {
    upload_bytes_with(base, token, product, version, file, bytes, &Patience::from_environment())
}

#[allow(clippy::print_stderr, reason = "the release job's log shows each upload that is tried again")]
fn tell_retry(failure: &str, attempt: u32, retries: u32) {
    eprintln!("{failure}; trying again ({attempt} of {retries})");
}

fn put_once(url: &str, token: &str, bytes: &[u8], patience: &Patience, what: &str) -> Result<bool, (String, bool)> {
    let sent = ureq::put(url)
        .config()
        .http_status_as_error(false)
        .timeout_connect(Some(patience.connect))
        .timeout_send_body(Some(patience.for_body(bytes.len())))
        .timeout_recv_response(Some(patience.for_answer(bytes.len())))
        .timeout_recv_body(Some(patience.answer))
        .build()
        .header("Authorization", format!("Bearer {token}"))
        .send(bytes);
    match sent {
        Ok(mut answer) => {
            let status = answer.status().as_u16();
            if (200..300).contains(&status) {
                return Ok(true);
            }
            let why = answer.body_mut().read_to_string().unwrap_or_default();
            Err((format!("the server refused {what} ({status}): {why}"), matches!(status, 502..=504)))
        }
        Err(error) => {
            let again = dropped(&error);
            Err((format!("cannot upload {what}: {error}"), again))
        }
    }
}

fn put_retrying(
    url: &str,
    token: &str,
    bytes: &[u8],
    patience: &Patience,
    what: &str,
    done: &dyn Fn() -> bool,
) -> Result<bool, String> {
    let mut attempt = 0;
    loop {
        if done() {
            return Ok(false);
        }
        let (failure, again) = match put_once(url, token, bytes, patience, what) {
            Ok(sent) => return Ok(sent),
            Err(failure) => failure,
        };
        if !again {
            return Err(failure);
        }
        if attempt >= patience.retries {
            return Err(format!("{failure} (after {} tries)", attempt + 1));
        }
        attempt += 1;
        tell_retry(&failure, attempt, patience.retries);
        std::thread::sleep(patience.pause * attempt);
    }
}

pub fn upload_bytes_with(
    base: &str,
    token: &str,
    product: &str,
    version: &str,
    file: &str,
    bytes: &[u8],
    patience: &Patience,
) -> Result<bool, String> {
    let base = base.trim_end_matches('/');
    if [product, version, file].iter().any(|segment| !is_safe_segment(segment)) {
        return Err(format!("{product}/{version}/{file}: only letters, digits, '.', '_' and '-'"));
    }
    let url = format!("{base}/api/v1/artifacts/{product}/{version}/{file}");
    let sha256 = zen_update::sha256_hex(bytes);
    let same = || stored_sha(&url, token, patience) == Some((bytes.len() as u64, sha256.clone()));
    put_retrying(&url, token, bytes, patience, file, &same)
}

#[derive(Clone, Debug)]
pub struct Upload {
    pub product: String,
    pub version: String,
    pub name: String,
    pub path: std::path::PathBuf,
}

pub fn parallel(jobs: usize) -> usize {
    std::env::var(PARALLEL_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(jobs)
}

pub fn upload_all(base: &str, token: &str, uploads: &[Upload], jobs: usize) -> Result<(usize, usize), String> {
    let large = |upload: &Upload| std::fs::metadata(&upload.path).is_ok_and(|meta| meta.len() > LARGE_FILE);
    let (big, small): (Vec<Upload>, Vec<Upload>) = uploads.iter().cloned().partition(|upload| large(upload));
    let (sent_small, kept_small) = upload_some(base, token, &small, parallel(jobs))?;
    let (sent_big, kept_big) = upload_some(base, token, &big, 1)?;
    Ok((sent_small + sent_big, kept_small + kept_big))
}

fn upload_some(base: &str, token: &str, uploads: &[Upload], jobs: usize) -> Result<(usize, usize), String> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let sent = std::sync::atomic::AtomicUsize::new(0);
    let failure = std::sync::Mutex::new(None::<String>);
    std::thread::scope(|scope| {
        for _ in 0..jobs.clamp(1, 8) {
            scope.spawn(|| {
                loop {
                    if failure.lock().map(|failure| failure.is_some()).unwrap_or(true) {
                        return;
                    }
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(upload) = uploads.get(index) else { return };
                    let result = std::fs::read(&upload.path)
                        .map_err(|error| format!("{}: {error}", upload.path.display()))
                        .and_then(|bytes| {
                            upload_bytes(base, token, &upload.product, &upload.version, &upload.name, &bytes)
                        });
                    match result {
                        Ok(true) => {
                            sent.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        }
                        Ok(false) => {}
                        Err(error) => {
                            if let Ok(mut failure) = failure.lock() {
                                failure.get_or_insert(error);
                            }
                            return;
                        }
                    }
                }
            });
        }
    });
    if let Some(error) = failure.into_inner().ok().flatten() {
        return Err(error);
    }
    let sent = sent.into_inner();
    Ok((sent, uploads.len() - sent))
}

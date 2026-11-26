use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError};
use std::time::SystemTime;

use axum::Router;
use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use tokio::sync::RwLock;
use tower_http::services::ServeDir;

use crate::is_safe_segment;

mod stage;

const MAX_ARTIFACT: u64 = 4 << 30;

const MAX_DOCUMENT: usize = 32 << 20;

const MAX_SITE_FILE: usize = 64 << 20;

#[derive(Clone, Debug)]
pub struct Config {
    pub data: PathBuf,
    pub public_key: String,
    pub token: Option<String>,
}

#[derive(Default)]
pub(crate) struct Published {
    manifest: Option<(Vec<u8>, String)>,
    plugins: Option<(Vec<u8>, String)>,
}

pub(crate) struct AppState {
    config: Config,
    published: RwLock<Published>,
    hashes: Hashes,
}

#[derive(Default)]
pub(crate) struct Hashes(std::sync::Mutex<HashMap<PathBuf, Hashed>>);

#[derive(Clone)]
struct Hashed {
    modified: Option<SystemTime>,
    size: u64,
    sha256: String,
}

impl Hashes {
    async fn of(&self, path: &Path) -> std::io::Result<(u64, String)> {
        let meta = tokio::fs::metadata(path).await?;
        let (size, modified) = (meta.len(), meta.modified().ok());
        let known = self.0.lock().unwrap_or_else(PoisonError::into_inner).get(path).cloned();
        if let Some(known) =
            known.filter(|known| known.size == size && modified.is_some() && known.modified == modified)
        {
            return Ok((known.size, known.sha256));
        }
        let file = path.to_owned();
        let sha256 = tokio::task::spawn_blocking(move || hash_file(&file)).await.map_err(std::io::Error::other)??;
        let entry = Hashed { modified, size, sha256: sha256.clone() };
        self.0.lock().unwrap_or_else(PoisonError::into_inner).insert(path.to_owned(), entry);
        Ok((size, sha256))
    }
}

fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        match std::io::Read::read(&mut file, &mut buffer)? {
            0 => return Ok(hex::encode(hasher.finalize())),
            read => hasher.update(&buffer[..read]),
        }
    }
}

pub fn router(config: Config) -> Result<Router, String> {
    zen_update::check_public_key(&config.public_key).map_err(|error| error.to_string())?;
    writable_data(&config.data)?;
    let published = load(&config.data, &config.public_key)?;
    let state =
        Arc::new(AppState { published: RwLock::new(published), config: config.clone(), hashes: Hashes::default() });
    let authorised = Router::new()
        .route("/api/v1/artifacts/{product}/{version}/{file}", put(upload_artifact).get(artifact_hash))
        .route("/api/v1/manifest", put(upload_manifest))
        .route("/api/v1/plugins", put(upload_plugins))
        .route("/api/v1/check/manifest", put(check_manifest))
        .route("/api/v1/check/plugins", put(check_plugins))
        .route("/api/v1/changelog", put(upload_changelog))
        .route("/api/v1/stage/{stage}", get(stage::summary))
        .route("/api/v1/stage/{stage}/manifest", put(stage::put_manifest))
        .route("/api/v1/stage/{stage}/plugins", put(stage::put_plugins))
        .route("/api/v1/stage/{stage}/changelog", put(stage::put_changelog))
        .route(
            "/api/v1/stage/{stage}/site/{*path}",
            put(stage::put_site_file).layer(DefaultBodyLimit::max(MAX_SITE_FILE)),
        )
        .route("/api/v1/stage/{stage}/release", post(stage::release))
        .route_layer(middleware::from_fn_with_state(Arc::clone(&state), require_token));
    Ok(Router::new()
        .route("/releases/manifest.json", get(manifest))
        .route("/releases/manifest.json.sig", get(signature))
        .route("/releases/plugins.json", get(plugins))
        .route("/releases/plugins.json.sig", get(plugins_signature))
        .route("/releases/changelog.json", get(changelog))
        .route("/releases/stage/{stage}/{file}", get(stage::document))
        .nest_service("/releases/artifacts", ServeDir::new(config.data.join("artifacts")))
        .nest_service("/releases/staged-site", ServeDir::new(config.data.join("site")))
        .route("/api/v1/health", get(|| async { "ok" }))
        .merge(authorised)
        .layer(DefaultBodyLimit::max(MAX_DOCUMENT))
        .with_state(state))
}

async fn require_token(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    match refusal(&state, request.headers()) {
        Some(response) => response,
        None => next.run(request).await,
    }
}

fn writable_data(data: &Path) -> Result<(), String> {
    for folder in [data.to_owned(), data.join("artifacts"), data.join("stages"), data.join("site")] {
        let created = std::fs::create_dir_all(&folder);
        let probe = folder.join(".write-probe");
        let written = created.and_then(|()| std::fs::write(&probe, b"")).and_then(|()| std::fs::remove_file(&probe));
        if let Err(error) = written {
            return Err(format!(
                "the data folder {} is not writable by this server: {error}; give it to the server's user \
                 (in the container, uid 10001: chown -R 10001:10001 /srv/zen-releases/data)",
                folder.display()
            ));
        }
    }
    Ok(())
}

pub(crate) fn listed(published: &Published) -> std::collections::HashSet<(String, String, u64)> {
    let mut known = std::collections::HashSet::new();
    if let Some((bytes, _)) = &published.manifest
        && let Ok(manifest) = serde_json::from_slice::<zen_update::Manifest>(bytes)
    {
        known.extend(
            manifest.artifacts().map(|artifact| (artifact.url.clone(), artifact.sha256.clone(), artifact.size)),
        );
    }
    if let Some((bytes, _)) = &published.plugins
        && let Ok(catalog) = serde_json::from_slice::<zen_plugins::Catalog>(bytes)
    {
        known.extend(
            catalog_artifacts(&catalog)
                .unwrap_or_default()
                .into_iter()
                .map(|artifact| (artifact.url, artifact.sha256, artifact.size)),
        );
    }
    known
}

pub(crate) fn catalog_artifacts(catalog: &zen_plugins::Catalog) -> Result<Vec<zen_update::Artifact>, String> {
    let mut artifacts = Vec::new();
    for plugin in &catalog.plugins {
        artifacts.extend(plugin.versions.iter().map(|version| zen_update::Artifact {
            platform: String::new(),
            url: version.url.clone(),
            sha256: version.sha256.clone(),
            size: version.size,
        }));
        for driver in &plugin.database_drivers {
            let Some(platforms) = driver["platforms"].as_object() else { continue };
            for (platform, program) in platforms {
                let url = program["url"].as_str().unwrap_or_default();
                if url.contains("://") {
                    continue;
                }
                let (Some(sha256), Some(size)) = (program["sha256"].as_str(), program["size"].as_u64()) else {
                    return Err(format!(
                        "{}: the driver program {url} ({platform}) is listed without its SHA-256 or size",
                        plugin.id
                    ));
                };
                artifacts.push(zen_update::Artifact {
                    platform: platform.clone(),
                    url: url.to_owned(),
                    sha256: sha256.to_owned(),
                    size,
                });
            }
        }
    }
    Ok(artifacts)
}

pub(crate) async fn check_listed(data: &Path, hashes: &Hashes, artifact: &zen_update::Artifact) -> Result<(), String> {
    let Some(relative) = artifact.url.strip_prefix("artifacts/") else {
        return Err(format!("{}: artifact URLs must start with artifacts/", artifact.url));
    };
    if !relative.split('/').all(is_safe_segment) {
        return Err(format!("{}: unsafe artifact path", artifact.url));
    }
    let not_stored = |_| format!("{}: not stored on this server; upload it first", artifact.url);
    let path = data.join("artifacts").join(relative);
    let stored = tokio::fs::metadata(&path).await.map_err(not_stored)?.len();
    if artifact.size != 0 && stored != artifact.size {
        return Err(format!("{}: stored with {stored} bytes, listed with {}", artifact.url, artifact.size));
    }
    let (_, sha256) = hashes.of(&path).await.map_err(not_stored)?;
    if sha256 == artifact.sha256 {
        Ok(())
    } else {
        Err(format!("{}: stored with the SHA-256 {sha256}, listed with {}", artifact.url, artifact.sha256))
    }
}

async fn artifact_hash(
    State(state): State<Arc<AppState>>,
    UrlPath((product, version, file)): UrlPath<(String, String, String)>,
) -> Response {
    if ![&product, &version, &file].iter().all(|segment| is_safe_segment(segment)) {
        return (StatusCode::BAD_REQUEST, "unsafe artifact path").into_response();
    }
    let path = state.config.data.join("artifacts").join(&product).join(&version).join(&file);
    match state.hashes.of(&path).await {
        Ok((size, sha256)) => axum::Json(serde_json::json!({ "size": size, "sha256": sha256 })).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not stored").into_response(),
    }
}

fn load(data: &Path, public_key: &str) -> Result<Published, String> {
    let read = |name: &str| {
        let Ok(bytes) = std::fs::read(data.join(name)) else { return Ok::<_, String>(None) };
        let stored = std::fs::read_to_string(data.join(format!("{name}.sig"))).ok();
        let interrupted = data.join(format!(".{name}.sig.partial"));
        let verifies = |signature: &str| zen_update::verify_signature(&bytes, signature.trim(), public_key);
        if let Some(signature) = stored.as_deref().filter(|signature| verifies(signature).is_ok()) {
            let signature = signature.trim().to_owned();
            return Ok(Some((bytes, signature)));
        }
        if let Some(signature) =
            std::fs::read_to_string(&interrupted).ok().filter(|signature| verifies(signature).is_ok())
        {
            std::fs::rename(&interrupted, data.join(format!("{name}.sig")))
                .map_err(|error| format!("cannot finish storing {name}: {error}"))?;
            let signature = signature.trim().to_owned();
            return Ok(Some((bytes, signature)));
        }
        match stored.map(|signature| verifies(&signature)) {
            Some(Err(error)) => Err(format!("the stored {name} does not verify with the trusted key: {error}")),
            _ => Ok(None),
        }
    };
    Ok(Published { manifest: read("manifest.json")?, plugins: read("plugins.json")? })
}

pub(crate) async fn store_signed(data: &Path, name: &str, bytes: &[u8], signature: &str) -> std::io::Result<()> {
    let document = data.join(format!(".{name}.partial"));
    let signed = data.join(format!(".{name}.sig.partial"));
    write_synced(&document, bytes).await?;
    write_synced(&signed, signature.as_bytes()).await?;
    tokio::fs::rename(&document, data.join(name)).await?;
    tokio::fs::rename(&signed, data.join(format!("{name}.sig"))).await?;
    tokio::fs::File::open(data).await?.sync_all().await
}

async fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = tokio::fs::File::create(path).await?;
    file.write_all(bytes).await?;
    file.sync_all().await
}

async fn plugins(State(state): State<Arc<AppState>>) -> Response {
    match &state.published.read().await.plugins {
        Some((bytes, _)) => ([(header::CONTENT_TYPE, "application/json")], bytes.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "no plug-ins have been published yet").into_response(),
    }
}

async fn plugins_signature(State(state): State<Arc<AppState>>) -> Response {
    match &state.published.read().await.plugins {
        Some((_, signature)) => signature.clone().into_response(),
        None => (StatusCode::NOT_FOUND, "no plug-ins have been published yet").into_response(),
    }
}

async fn check_manifest(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(signature) = headers.get("x-zen-signature").and_then(|value| value.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "the X-Zen-Signature header is missing").into_response();
    };
    match zen_update::verify_manifest(&body, signature, &state.config.public_key) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response(),
    }
}

async fn check_plugins(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(signature) = headers.get("x-zen-signature").and_then(|value| value.to_str().ok()) else {
        return (StatusCode::BAD_REQUEST, "the X-Zen-Signature header is missing").into_response();
    };
    match zen_plugins::Catalog::verify(&body, signature, &state.config.public_key) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response(),
    }
}

async fn upload_plugins(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(signature) = headers.get("x-zen-signature").and_then(|value| value.to_str().ok()).map(str::to_owned)
    else {
        return (StatusCode::BAD_REQUEST, "the X-Zen-Signature header is missing").into_response();
    };
    let catalog = match zen_plugins::Catalog::verify(&body, &signature, &state.config.public_key) {
        Ok(catalog) => catalog,
        Err(error) => return (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response(),
    };
    let artifacts = match catalog_artifacts(&catalog) {
        Ok(artifacts) => artifacts,
        Err(problem) => return (StatusCode::UNPROCESSABLE_ENTITY, problem).into_response(),
    };
    for artifact in &artifacts {
        if let Err(problem) = check_listed(&state.config.data, &state.hashes, artifact).await {
            return (StatusCode::UNPROCESSABLE_ENTITY, problem).into_response();
        }
    }
    let mut published = state.published.write().await;
    if let Err(error) = store_signed(&state.config.data, "plugins.json", &body, &signature).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot store the catalog: {error}")).into_response();
    }
    published.plugins = Some((body.to_vec(), signature));
    (StatusCode::OK, "published").into_response()
}

async fn manifest(State(state): State<Arc<AppState>>) -> Response {
    match &state.published.read().await.manifest {
        Some((bytes, _)) => ([(header::CONTENT_TYPE, "application/json")], bytes.clone()).into_response(),
        None => (StatusCode::NOT_FOUND, "nothing has been published yet").into_response(),
    }
}

async fn signature(State(state): State<Arc<AppState>>) -> Response {
    match &state.published.read().await.manifest {
        Some((_, signature)) => signature.clone().into_response(),
        None => (StatusCode::NOT_FOUND, "nothing has been published yet").into_response(),
    }
}

async fn changelog(State(state): State<Arc<AppState>>) -> Response {
    match tokio::fs::read(state.config.data.join("changelog.json")).await {
        Ok(bytes) => {
            ([(header::CONTENT_TYPE, "application/json"), (header::CACHE_CONTROL, "no-cache")], bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "no changelog has been published yet").into_response(),
    }
}

async fn upload_changelog(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    let shape = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .is_some_and(|value| value["schema"] == 1 && value["changelogs"].is_array());
    if !shape {
        return (StatusCode::UNPROCESSABLE_ENTITY, "expected {\"schema\": 1, \"changelogs\": [...]}").into_response();
    }
    let data = &state.config.data;
    let written = async {
        tokio::fs::write(data.join(".changelog.json.partial"), &body).await?;
        tokio::fs::rename(data.join(".changelog.json.partial"), data.join("changelog.json")).await
    };
    match written.await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

pub(crate) fn refusal(state: &AppState, headers: &HeaderMap) -> Option<Response> {
    let Some(token) = &state.config.token else {
        return Some((StatusCode::FORBIDDEN, "uploads are disabled: the server has no token").into_response());
    };
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    match given {
        Some(given) if constant_time_eq(given.as_bytes(), token.as_bytes()) => None,
        _ => Some((StatusCode::UNAUTHORIZED, "a valid bearer token is required").into_response()),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

enum Receipt {
    TooLarge,
    Broken(String),
    Failed(std::io::Error),
}

async fn receive(body: Body, path: &Path) -> Result<(u64, String), Receipt> {
    let mut file = tokio::fs::File::create(path).await.map_err(Receipt::Failed)?;
    let mut hasher = Sha256::new();
    let mut size = 0u64;
    let mut stream = body.into_data_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| Receipt::Broken(error.to_string()))?;
        size += chunk.len() as u64;
        if size > MAX_ARTIFACT {
            return Err(Receipt::TooLarge);
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(Receipt::Failed)?;
    }
    file.sync_all().await.map_err(Receipt::Failed)?;
    Ok((size, hex::encode(hasher.finalize())))
}

fn too_large() -> Response {
    (StatusCode::PAYLOAD_TOO_LARGE, "an artifact may not exceed 4 GiB").into_response()
}

async fn upload_artifact(
    State(state): State<Arc<AppState>>,
    UrlPath((product, version, file)): UrlPath<(String, String, String)>,
    headers: HeaderMap,
    body: Body,
) -> Response {
    static UPLOADS: AtomicU64 = AtomicU64::new(0);
    if ![&product, &version, &file].iter().all(|segment| is_safe_segment(segment)) {
        return (
            StatusCode::BAD_REQUEST,
            "product, version and file name may contain only letters, digits, '.', '_' and '-'",
        )
            .into_response();
    }
    let announced = headers.get(header::CONTENT_LENGTH).and_then(|value| value.to_str().ok()?.parse::<u64>().ok());
    if announced.is_some_and(|size| size > MAX_ARTIFACT) {
        return too_large();
    }
    let directory = state.config.data.join("artifacts").join(&product).join(&version);
    if let Err(error) = tokio::fs::create_dir_all(&directory).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot store the artifact: {error}")).into_response();
    }
    let temporary = directory.join(format!(".{file}.{}.partial", UPLOADS.fetch_add(1, Ordering::Relaxed)));
    let received = receive(body, &temporary).await;
    let (size, sha256) = match received {
        Ok(received) => received,
        Err(receipt) => {
            let _ = tokio::fs::remove_file(&temporary).await;
            return match receipt {
                Receipt::TooLarge => too_large(),
                Receipt::Broken(error) => {
                    (StatusCode::BAD_REQUEST, format!("the upload broke off: {error}")).into_response()
                }
                Receipt::Failed(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot store the artifact: {error}")).into_response()
                }
            };
        }
    };
    let url = format!("artifacts/{product}/{version}/{file}");
    let published = state.published.read().await;
    let changes_live = listed(&published).iter().any(|(listed, hash, _)| *listed == url && *hash != sha256);
    if changes_live {
        let _ = tokio::fs::remove_file(&temporary).await;
        return (
            StatusCode::CONFLICT,
            format!("{url}: the live release lists other bytes under this name; publish a new version instead"),
        )
            .into_response();
    }
    match tokio::fs::rename(&temporary, directory.join(&file)).await {
        Ok(()) => axum::Json(serde_json::json!({ "size": size, "sha256": sha256 })).into_response(),
        Err(error) => {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot store the artifact: {error}")).into_response()
        }
    }
}

async fn upload_manifest(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(signature) = headers.get("x-zen-signature").and_then(|value| value.to_str().ok()).map(str::to_owned)
    else {
        return (StatusCode::BAD_REQUEST, "the X-Zen-Signature header is missing").into_response();
    };
    let manifest = match zen_update::verify_manifest(&body, &signature, &state.config.public_key) {
        Ok(manifest) => manifest,
        Err(error) => return (StatusCode::UNPROCESSABLE_ENTITY, error.to_string()).into_response(),
    };
    for artifact in manifest.artifacts() {
        if let Err(problem) = check_listed(&state.config.data, &state.hashes, artifact).await {
            return (StatusCode::UNPROCESSABLE_ENTITY, problem).into_response();
        }
    }

    let mut published = state.published.write().await;
    if let Err(error) = store_signed(&state.config.data, "manifest.json", &body, &signature).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot store the manifest: {error}")).into_response();
    }
    published.manifest = Some((body.to_vec(), signature));
    (StatusCode::OK, "published").into_response()
}

pub async fn serve(config: Config, listen: std::net::SocketAddr) -> Result<(), String> {
    let app = router(config)?;
    let listener =
        tokio::net::TcpListener::bind(listen).await.map_err(|error| format!("cannot listen on {listen}: {error}"))?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn an_artifact_is_checked_by_its_hash_and_hashed_once_while_it_is_unchanged() {
        let data = tempfile::tempdir().unwrap();
        let folder = data.path().join("artifacts/gogarden/26.10.2");
        std::fs::create_dir_all(&folder).unwrap();
        let stored = folder.join("g.tar.gz");
        std::fs::write(&stored, b"stored bytes").unwrap();
        let artifact = zen_update::Artifact {
            platform: "macos-aarch64".to_owned(),
            url: "artifacts/gogarden/26.10.2/g.tar.gz".to_owned(),
            sha256: zen_update::sha256_hex(b"stored bytes"),
            size: 12,
        };
        let runtime = runtime();
        let hashes = Hashes::default();
        assert_eq!(runtime.block_on(check_listed(data.path(), &hashes, &artifact)), Ok(()));
        assert_eq!(hashes.0.lock().unwrap().len(), 1, "the hash is kept");

        let changed = zen_update::Artifact { sha256: "0".repeat(64), ..artifact.clone() };
        let refused = runtime.block_on(check_listed(data.path(), &hashes, &changed)).unwrap_err();
        assert!(refused.contains("SHA-256"), "{refused}");

        let shorter = zen_update::Artifact { size: 5, ..artifact.clone() };
        assert!(runtime.block_on(check_listed(data.path(), &hashes, &shorter)).unwrap_err().contains("12 bytes"));
        let missing = zen_update::Artifact { url: "artifacts/gogarden/26.10.2/none.tar.gz".to_owned(), ..artifact };
        assert!(runtime.block_on(check_listed(data.path(), &hashes, &missing)).unwrap_err().contains("not stored"));
    }

    #[test]
    fn a_stored_hash_does_not_outlive_a_change_of_the_file() {
        let data = tempfile::tempdir().unwrap();
        let path = data.path().join("file");
        std::fs::write(&path, b"first").unwrap();
        let runtime = runtime();
        let hashes = Hashes::default();
        let (_, first) = runtime.block_on(hashes.of(&path)).unwrap();
        assert_eq!(first, zen_update::sha256_hex(b"first"));
        std::fs::write(&path, b"second, longer").unwrap();
        let (size, second) = runtime.block_on(hashes.of(&path)).unwrap();
        assert_eq!((size, second), (14, zen_update::sha256_hex(b"second, longer")));
    }

    #[test]
    fn a_crash_between_the_two_renames_does_not_stop_the_server_and_is_finished_at_start() {
        let data = tempfile::tempdir().unwrap();
        let (secret, public) = zen_update::key_pair_from_seed([3; 32]);
        let sign = |bytes: &[u8]| zen_update::sign_manifest(bytes, &secret).unwrap();
        std::fs::write(data.path().join("manifest.json"), b"new").unwrap();
        std::fs::write(data.path().join("manifest.json.sig"), sign(b"old")).unwrap();
        std::fs::write(data.path().join(".manifest.json.sig.partial"), sign(b"new")).unwrap();

        let published = load(data.path(), &public).unwrap();

        assert_eq!(published.manifest.unwrap(), (b"new".to_vec(), sign(b"new")));
        assert_eq!(std::fs::read_to_string(data.path().join("manifest.json.sig")).unwrap(), sign(b"new"));
        assert!(!data.path().join(".manifest.json.sig.partial").exists());
    }

    #[test]
    fn a_crash_before_the_first_signature_is_stored_is_finished_at_start() {
        let data = tempfile::tempdir().unwrap();
        let (secret, public) = zen_update::key_pair_from_seed([4; 32]);
        let signature = zen_update::sign_manifest(b"new", &secret).unwrap();
        std::fs::write(data.path().join("plugins.json"), b"new").unwrap();
        std::fs::write(data.path().join(".plugins.json.sig.partial"), &signature).unwrap();
        assert_eq!(load(data.path(), &public).unwrap().plugins.unwrap().1, signature);
    }

    #[test]
    fn a_document_no_signature_verifies_still_stops_the_server() {
        let data = tempfile::tempdir().unwrap();
        let (secret, public) = zen_update::key_pair_from_seed([5; 32]);
        std::fs::write(data.path().join("manifest.json"), b"changed").unwrap();
        std::fs::write(data.path().join("manifest.json.sig"), zen_update::sign_manifest(b"original", &secret).unwrap())
            .unwrap();
        assert!(load(data.path(), &public).err().unwrap().contains("does not verify"));
    }

    #[test]
    fn storing_a_signed_document_leaves_a_pair_that_verifies_and_nothing_else() {
        let data = tempfile::tempdir().unwrap();
        let (secret, public) = zen_update::key_pair_from_seed([6; 32]);
        let signature = zen_update::sign_manifest(b"doc", &secret).unwrap();
        runtime().block_on(store_signed(data.path(), "manifest.json", b"doc", &signature)).unwrap();
        assert_eq!(load(data.path(), &public).unwrap().manifest.unwrap(), (b"doc".to_vec(), signature));
        let names: Vec<_> = std::fs::read_dir(data.path()).unwrap().map(|entry| entry.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
    }
}

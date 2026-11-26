use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as UrlPath, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};

use super::{AppState, check_listed};
use crate::is_safe_segment;

pub(crate) const CURRENT: &str = "current";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Signed {
    Manifest,
    Plugins,
}

impl Signed {
    fn file(self) -> &'static str {
        match self {
            Signed::Manifest => "manifest.json",
            Signed::Plugins => "plugins.json",
        }
    }
}

pub(crate) fn valid_stage(stage: &str) -> bool {
    is_safe_segment(stage) && stage != CURRENT
}

fn stage_dir(data: &Path, stage: &str) -> PathBuf {
    data.join("stages").join(stage)
}

pub(crate) fn base_of(document: Option<&(Vec<u8>, String)>) -> String {
    document.map_or_else(|| "none".to_owned(), |(bytes, _)| zen_update::sha256_hex(bytes))
}

fn bad_stage() -> Response {
    (StatusCode::BAD_REQUEST, "a stage name may contain only letters, digits, '.', '_' and '-'").into_response()
}

async fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("file");
    let temporary = path.with_file_name(format!(".{name}.partial"));
    tokio::fs::write(&temporary, bytes).await?;
    tokio::fs::rename(&temporary, path).await
}

pub(super) async fn document(
    State(state): State<Arc<AppState>>,
    UrlPath((stage, file)): UrlPath<(String, String)>,
) -> Response {
    const FILES: [&str; 5] =
        ["manifest.json", "manifest.json.sig", "plugins.json", "plugins.json.sig", "changelog.json"];
    if !valid_stage(&stage) || !FILES.contains(&file.as_str()) {
        return (StatusCode::NOT_FOUND, "no such staged document").into_response();
    }
    match tokio::fs::read(stage_dir(&state.config.data, &stage).join(&file)).await {
        Ok(bytes) => {
            let kind = if file.ends_with(".sig") { "text/plain" } else { "application/json" };
            ([(header::CONTENT_TYPE, kind), (header::CACHE_CONTROL, "no-cache")], bytes).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "not staged").into_response(),
    }
}

fn artifacts_of(
    kind: Signed,
    bytes: &[u8],
    signature: &str,
    public_key: &str,
) -> Result<Vec<zen_update::Artifact>, String> {
    match kind {
        Signed::Manifest => zen_update::verify_manifest(bytes, signature, public_key)
            .map(|manifest| manifest.artifacts().cloned().collect())
            .map_err(|error| error.to_string()),
        Signed::Plugins => zen_plugins::Catalog::verify(bytes, signature, public_key)
            .map_err(|error| error.to_string())
            .and_then(|catalog| super::catalog_artifacts(&catalog)),
    }
}

async fn put_signed(state: Arc<AppState>, stage: String, headers: HeaderMap, body: Bytes, kind: Signed) -> Response {
    if !valid_stage(&stage) {
        return bad_stage();
    }
    let header_value = |name: &str| headers.get(name).and_then(|value| value.to_str().ok()).map(str::to_owned);
    let Some(signature) = header_value("x-zen-signature") else {
        return (StatusCode::BAD_REQUEST, "the X-Zen-Signature header is missing").into_response();
    };
    let Some(base) = header_value("x-zen-base") else {
        return (StatusCode::BAD_REQUEST, "the X-Zen-Base header is missing (the live document's SHA-256, or none)")
            .into_response();
    };
    let artifacts = match artifacts_of(kind, &body, &signature, &state.config.public_key) {
        Ok(artifacts) => artifacts,
        Err(error) => return (StatusCode::UNPROCESSABLE_ENTITY, error).into_response(),
    };
    for artifact in &artifacts {
        if let Err(problem) = check_listed(&state.config.data, &state.hashes, artifact).await {
            return (StatusCode::UNPROCESSABLE_ENTITY, problem).into_response();
        }
    }
    let _published = state.published.write().await;
    let dir = stage_dir(&state.config.data, &stage);
    if dir.join("released").exists() {
        return (StatusCode::CONFLICT, "this stage is released already; stage under a new name").into_response();
    }
    let base_file = dir.join(format!("{}.base", kind.file().trim_end_matches(".json")));
    match tokio::fs::read_to_string(&base_file).await {
        Ok(recorded) if recorded.trim() != base => {
            return (
                StatusCode::CONFLICT,
                format!(
                    "the stage was made from the live {} {}, this one from {base}: the live one changed meanwhile; \
                     stage again under a new name",
                    kind.file(),
                    recorded.trim()
                ),
            )
                .into_response();
        }
        Ok(_) => {}
        Err(_) => {
            if let Err(error) = write_atomically(&base_file, base.as_bytes()).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
            }
        }
    }
    let written = async {
        write_atomically(&dir.join(format!("{}.sig", kind.file())), signature.as_bytes()).await?;
        write_atomically(&dir.join(kind.file()), &body).await
    };
    match written.await {
        Ok(()) => (StatusCode::OK, "staged").into_response(),
        Err(error) => {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("cannot stage the {}: {error}", kind.file())).into_response()
        }
    }
}

pub(super) async fn put_manifest(
    State(state): State<Arc<AppState>>,
    UrlPath(stage): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    put_signed(state, stage, headers, body, Signed::Manifest).await
}

pub(super) async fn put_plugins(
    State(state): State<Arc<AppState>>,
    UrlPath(stage): UrlPath<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    put_signed(state, stage, headers, body, Signed::Plugins).await
}

pub(super) async fn put_changelog(
    State(state): State<Arc<AppState>>,
    UrlPath(stage): UrlPath<String>,
    body: Bytes,
) -> Response {
    if !valid_stage(&stage) {
        return bad_stage();
    }
    let shape = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .is_some_and(|value| value["schema"] == 1 && value["changelogs"].is_array());
    if !shape {
        return (StatusCode::UNPROCESSABLE_ENTITY, "expected {\"schema\": 1, \"changelogs\": [...]}").into_response();
    }
    match write_atomically(&stage_dir(&state.config.data, &stage).join("changelog.json"), &body).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

pub(crate) fn valid_site_path(path: &str) -> bool {
    !path.is_empty() && path.split('/').count() <= 8 && path.split('/').all(is_safe_segment)
}

pub(super) async fn put_site_file(
    State(state): State<Arc<AppState>>,
    UrlPath((stage, path)): UrlPath<(String, String)>,
    body: Bytes,
) -> Response {
    if !valid_stage(&stage) {
        return bad_stage();
    }
    if !valid_site_path(&path) {
        return (
            StatusCode::BAD_REQUEST,
            "each part of a website path may contain only letters, digits, '.', '_' and '-'",
        )
            .into_response();
    }
    let _published = state.published.read().await;
    if stage_dir(&state.config.data, &stage).join("released").exists() {
        return (StatusCode::CONFLICT, "this stage is released already; stage under a new name").into_response();
    }
    match write_atomically(&state.config.data.join("site").join(&stage).join(&path), &body).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn describe(state: &AppState, stage: &str) -> serde_json::Value {
    let dir = stage_dir(&state.config.data, stage);
    let read = |name: &str| {
        let path = dir.join(name);
        async move { tokio::fs::read(path).await.ok() }
    };
    let products = match read("manifest.json").await {
        Some(bytes) => serde_json::from_slice::<zen_update::Manifest>(&bytes)
            .map(|manifest| {
                manifest
                    .products
                    .iter()
                    .map(|product| {
                        let versions: Vec<&str> =
                            product.releases.iter().map(|release| release.version.as_str()).collect();
                        serde_json::json!({ "id": product.id, "versions": versions })
                    })
                    .collect::<Vec<_>>()
            })
            .ok(),
        None => None,
    };
    let site = state.config.data.join("site").join(stage).join("index.html");
    let text = |bytes: Option<Vec<u8>>| bytes.map(|bytes| String::from_utf8_lossy(&bytes).trim().to_owned());
    serde_json::json!({
        "stage": stage,
        "manifest": products,
        "manifestBase": text(read("manifest.base").await),
        "plugins": read("plugins.json").await.is_some(),
        "pluginsBase": text(read("plugins.base").await),
        "changelog": read("changelog.json").await.is_some(),
        "site": tokio::fs::metadata(site).await.is_ok(),
        "released": text(read("released").await),
    })
}

pub(super) async fn summary(State(state): State<Arc<AppState>>, UrlPath(stage): UrlPath<String>) -> Response {
    if !valid_stage(&stage) {
        return bad_stage();
    }
    axum::Json(describe(&state, &stage).await).into_response()
}

async fn check_staged(state: &AppState, dir: &Path) -> Result<(), String> {
    for kind in [Signed::Manifest, Signed::Plugins] {
        let Ok(bytes) = tokio::fs::read(dir.join(kind.file())).await else { continue };
        let signature = tokio::fs::read_to_string(dir.join(format!("{}.sig", kind.file()))).await.unwrap_or_default();
        let Ok(artifacts) = artifacts_of(kind, &bytes, signature.trim(), &state.config.public_key) else { continue };
        for artifact in &artifacts {
            check_listed(&state.config.data, &state.hashes, artifact).await?;
        }
    }
    Ok(())
}

pub(super) async fn release(State(state): State<Arc<AppState>>, UrlPath(stage): UrlPath<String>) -> Response {
    if !valid_stage(&stage) {
        return bad_stage();
    }
    let data = state.config.data.clone();
    let dir = stage_dir(&data, &stage);
    if !dir.join("released").exists()
        && let Err(problem) = check_staged(&state, &dir).await
    {
        return (StatusCode::UNPROCESSABLE_ENTITY, format!("{problem}; nothing was released")).into_response();
    }
    let mut published = state.published.write().await;
    if let Ok(when) = tokio::fs::read_to_string(dir.join("released")).await {
        let mut answer = describe(&state, &stage).await;
        answer["already"] = serde_json::Value::Bool(true);
        answer["released"] = serde_json::Value::String(when.trim().to_owned());
        return axum::Json(answer).into_response();
    }
    let site = data.join("site").join(&stage);
    let has_site = tokio::fs::metadata(site.join("index.html")).await.is_ok();
    let mut documents = Vec::new();
    for kind in [Signed::Manifest, Signed::Plugins] {
        let Ok(bytes) = tokio::fs::read(dir.join(kind.file())).await else { continue };
        let signature = tokio::fs::read_to_string(dir.join(format!("{}.sig", kind.file()))).await.unwrap_or_default();
        let verified = match kind {
            Signed::Manifest => zen_update::verify_manifest(&bytes, signature.trim(), &state.config.public_key)
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Signed::Plugins => zen_plugins::Catalog::verify(&bytes, signature.trim(), &state.config.public_key)
                .map(|_| ())
                .map_err(|error| error.to_string()),
        };
        if let Err(error) = verified {
            return (StatusCode::UNPROCESSABLE_ENTITY, format!("the staged {} does not verify: {error}", kind.file()))
                .into_response();
        }
        let base_name = format!("{}.base", kind.file().trim_end_matches(".json"));
        let base = tokio::fs::read_to_string(dir.join(base_name)).await.unwrap_or_default();
        let live = match kind {
            Signed::Manifest => base_of(published.manifest.as_ref()),
            Signed::Plugins => base_of(published.plugins.as_ref()),
        };
        if base.trim() != live && live != zen_update::sha256_hex(&bytes) {
            return (
                StatusCode::CONFLICT,
                format!(
                    "the live {} changed since this stage was made (it was {}, it is {live}): nothing was released; \
                     stage the release again",
                    kind.file(),
                    base.trim()
                ),
            )
                .into_response();
        }
        documents.push((kind, bytes, signature.trim().to_owned()));
    }
    let changelog = tokio::fs::read(dir.join("changelog.json")).await.ok();
    if documents.is_empty() && changelog.is_none() && !has_site {
        return (StatusCode::NOT_FOUND, "nothing is staged under this name").into_response();
    }
    let switched = async {
        for (kind, bytes, signature) in &documents {
            super::store_signed(&data, kind.file(), bytes, signature).await?;
        }
        if let Some(changelog) = &changelog {
            write_atomically(&data.join("changelog.json"), changelog).await?;
        }
        if has_site {
            switch_site(&data.join("site"), &stage)?;
        }
        write_atomically(&dir.join("released"), crate::today().as_bytes()).await
    };
    if let Err(error) = switched.await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("the release stopped midway: {error}; release again"))
            .into_response();
    }
    for (kind, bytes, signature) in documents {
        match kind {
            Signed::Manifest => published.manifest = Some((bytes, signature)),
            Signed::Plugins => published.plugins = Some((bytes, signature)),
        }
    }
    drop(published);
    axum::Json(describe(&state, &stage).await).into_response()
}

pub(crate) fn switch_site(site: &Path, stage: &str) -> std::io::Result<()> {
    let temporary = site.join(format!(".{CURRENT}.partial"));
    let _ = std::fs::remove_file(&temporary);
    #[cfg(unix)]
    std::os::unix::fs::symlink(stage, &temporary)?;
    #[cfg(not(unix))]
    return Err(std::io::Error::other(format!("the live website's link to {stage} needs a Unix server")));
    #[cfg(unix)]
    std::fs::rename(&temporary, site.join(CURRENT))
}

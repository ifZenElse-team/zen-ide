use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_lsp::connection::{CONTENT_MODIFIED, SERVER_CANCELLED};
use zen_lsp::{Client, DocumentRequest, LspError};

use super::Synced;

pub(crate) const RETRY: Duration = Duration::from_millis(300);

#[derive(Debug, Default)]
pub(crate) struct Pull {
    request: Option<DocumentRequest>,
    done: Option<i32>,
    retry_at: Option<Instant>,
}

pub(crate) fn supported(client: &Client) -> bool {
    client.capabilities().is_some_and(|capabilities| capabilities.diagnostic_provider.is_some())
}

fn asks_again(error: &LspError) -> bool {
    matches!(error, LspError::Server { code, .. } if *code == SERVER_CANCELLED || *code == CONTENT_MODIFIED)
}

pub(crate) fn step(
    pulls: &mut BTreeMap<PathBuf, Pull>,
    client: &mut Client,
    files: &HashMap<PathBuf, Synced>,
) -> Vec<(PathBuf, Vec<Value>)> {
    pulls.retain(|path, _| files.contains_key(path));
    let mut answered = Vec::new();
    let now = Instant::now();
    for (path, synced) in files {
        let pull = pulls.entry(path.clone()).or_default();
        if let Some(request) = &pull.request {
            let Some(answer) = request.pending.try_answer() else { continue };
            let stale = client.is_stale(request);
            let version = request.version;
            pull.request = None;
            match answer {
                Ok(report) if !stale => {
                    pull.done = Some(version);
                    if report["kind"] == "full" {
                        let items = report["items"].as_array().cloned().unwrap_or_default();
                        answered.push((path.clone(), items));
                    }
                }
                Ok(_) => {}
                Err(error) if asks_again(&error) => pull.retry_at = Some(now + RETRY),
                Err(_) => pull.done = Some(version),
            }
        }
        let version = client.version(&synced.uri);
        let due = pull.retry_at.is_none_or(|at| now >= at);
        if pull.request.is_none() && version.is_some() && pull.done != version && due {
            let params = json!({ "textDocument": { "uri": synced.uri } });
            pull.request = client.document_request(&synced.uri, "textDocument/diagnostic", params).ok();
            pull.retry_at = None;
        }
    }
    answered
}

pub(crate) fn answered(pulls: &BTreeMap<PathBuf, Pull>, path: &Path, version: Option<i32>) -> bool {
    pulls.get(path).is_some_and(|pull| pull.done.is_some() && pull.done == version)
}

pub(crate) fn refresh(pulls: &mut BTreeMap<PathBuf, Pull>) {
    for pull in pulls.values_mut() {
        pull.done = None;
        pull.retry_at = None;
    }
}

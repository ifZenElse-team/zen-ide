use std::collections::BTreeMap;

use serde_json::{Value as Json, json};

pub const SECTION: &str = "state:resources";

fn text(value: &Json) -> String {
    value.as_str().unwrap_or_default().lines().next().unwrap_or_default().to_owned()
}

fn group_of(api_version: &str, kind: &str) -> Option<&'static str> {
    let group = api_version.split('/').next().unwrap_or("");
    match (group, kind) {
        ("source.toolkit.fluxcd.io" | "kustomize.toolkit.fluxcd.io" | "helm.toolkit.fluxcd.io", _) => Some("flux"),
        ("argoproj.io", "Application") => Some("argocd"),
        _ => None,
    }
}

struct State {
    ready: String,
    synced: String,
    health: String,
    revision: String,
    message: String,
    state: &'static str,
}

fn flux(item: &Json) -> State {
    let status = &item["status"];
    let ready = status["conditions"].as_array().into_iter().flatten().find(|condition| condition["type"] == "Ready");
    let revision = ["lastAppliedRevision", "lastAttemptedRevision"]
        .iter()
        .map(|key| text(&status[key]))
        .chain([text(&status["artifact"]["revision"]), text(&status["history"][0]["chartVersion"])])
        .find(|revision| !revision.is_empty())
        .unwrap_or_default();
    let suspended = item["spec"]["suspend"] == true;
    let (shown, state) = match ready.map(|condition| text(&condition["status"])).as_deref() {
        _ if suspended => ("Suspended".to_owned(), "unknown"),
        Some("True") => ("True".to_owned(), "ok"),
        Some("False") => {
            let reason = ready.map(|condition| text(&condition["reason"])).unwrap_or_default();
            let state = if reason.contains("Progress") { "progressing" } else { "error" };
            (format!("False ({reason})"), state)
        }
        Some(other) => (other.to_owned(), "progressing"),
        None => (String::new(), "unknown"),
    };
    State {
        ready: shown,
        synced: String::new(),
        health: String::new(),
        revision,
        message: ready.map(|condition| text(&condition["message"])).unwrap_or_default(),
        state,
    }
}

fn argo(item: &Json) -> State {
    let status = &item["status"];
    let synced = text(&status["sync"]["status"]);
    let health = text(&status["health"]["status"]);
    let revision = [text(&status["sync"]["revision"]), text(&status["sync"]["revisions"][0])]
        .into_iter()
        .find(|revision| !revision.is_empty())
        .unwrap_or_default();
    let message = [
        text(&status["conditions"][0]["message"]),
        text(&status["operationState"]["message"]),
        text(&status["health"]["message"]),
    ]
    .into_iter()
    .find(|message| !message.is_empty())
    .unwrap_or_default();
    let state = match (synced.as_str(), health.as_str()) {
        (_, "Degraded" | "Missing") => "error",
        ("Synced", "Healthy") => "ok",
        ("OutOfSync", _) | (_, "Progressing" | "Suspended") => "progressing",
        _ => "unknown",
    };
    State { ready: String::new(), synced, health, revision, message, state }
}

pub fn rows(output: &str) -> Json {
    let Ok(listed) = serde_json::from_str::<Json>(output) else { return json!([]) };
    let items: Vec<&Json> = match listed["items"].as_array() {
        Some(items) => items.iter().collect(),
        None if listed["kind"].is_string() => vec![&listed],
        None => Vec::new(),
    };
    let mut rows: Vec<(String, Json)> = Vec::new();
    let mut namespaces: BTreeMap<(String, String, String), usize> = BTreeMap::new();
    for item in &items {
        let kind = text(&item["kind"]);
        let Some(group) = group_of(&text(&item["apiVersion"]), &kind) else { continue };
        let name = text(&item["metadata"]["name"]);
        let namespace = text(&item["metadata"]["namespace"]);
        if name.is_empty() {
            continue;
        }
        *namespaces.entry((group.to_owned(), kind.clone(), name.clone())).or_default() += 1;
        let found = if group == "flux" { flux(item) } else { argo(item) };
        let selector = format!("involvedObject.kind={kind},involvedObject.name={name}");
        let row = json!({
            "id": format!("{group}:{kind}/{}/{name}", if namespace.is_empty() { "-" } else { &namespace }),
            "columns": [found.ready, found.synced, found.health, found.revision, found.message, name, namespace, selector],
            "state": found.state,
        });
        rows.push(((group.to_owned() + ":" + &kind + "/-/" + &name), row));
    }
    let mut out: Vec<Json> = Vec::new();
    for (fallback, row) in rows {
        let (group, rest) = fallback.split_once(':').unwrap_or_default();
        let (kind, name) = rest.split_once("/-/").unwrap_or_default();
        let unique = namespaces.get(&(group.to_owned(), kind.to_owned(), name.to_owned())) == Some(&1);
        if unique && row["id"] != fallback.as_str() {
            let mut copy = row.clone();
            copy["id"] = json!(fallback);
            out.push(copy);
        }
        out.push(row);
    }
    Json::Array(out)
}

pub fn tool_rows(section: &str, output: &str) -> Json {
    if section == SECTION { rows(output) } else { json!([]) }
}

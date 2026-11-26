use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub columns: Vec<String>,
    pub state: Option<String>,
}

fn text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => String::new(),
    }
}

fn count(value: &Value) -> i64 {
    value.as_i64().unwrap_or(0)
}

fn items(output: &str) -> Vec<Value> {
    let Ok(document) = serde_json::from_str::<Value>(output) else { return Vec::new() };
    match document.get("items") {
        Some(Value::Array(items)) => items.clone(),
        _ if document.get("metadata").is_some() => vec![document],
        _ => Vec::new(),
    }
}

fn name(item: &Value) -> String {
    text(&item["metadata"]["name"])
}

fn created(item: &Value) -> String {
    text(&item["metadata"]["creationTimestamp"])
}

pub fn contexts(output: &str) -> Vec<Row> {
    let Ok(config) = serde_json::from_str::<Value>(output) else { return Vec::new() };
    let current = text(&config["current-context"]);
    config["contexts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let name = text(&entry["name"]);
            if name.is_empty() {
                return None;
            }
            let context = &entry["context"];
            let state = (name == current).then(|| "current".to_owned());
            let columns =
                vec![name.clone(), text(&context["cluster"]), text(&context["user"]), text(&context["namespace"])];
            Some(Row { id: name, columns, state })
        })
        .collect()
}

pub fn namespaces(output: &str, chosen: bool) -> Vec<Row> {
    items(output)
        .iter()
        .filter_map(|item| {
            let name = name(item);
            if name.is_empty() {
                return None;
            }
            let state = if chosen { (name == "default").then(|| "current".to_owned()) } else { None };
            Some(Row { id: name.clone(), columns: vec![name, text(&item["status"]["phase"]), created(item)], state })
        })
        .collect()
}

fn pod_status(item: &Value) -> (String, &'static str) {
    if !item["metadata"]["deletionTimestamp"].is_null() {
        return ("Terminating".to_owned(), "restarting");
    }
    let statuses = item["status"]["containerStatuses"].as_array().cloned().unwrap_or_default();
    for status in &statuses {
        let waiting = text(&status["state"]["waiting"]["reason"]);
        if !waiting.is_empty() {
            let state = if waiting.contains("BackOff") || waiting.contains("Err") { "dead" } else { "starting" };
            return (waiting, state);
        }
        let terminated = text(&status["state"]["terminated"]["reason"]);
        if !terminated.is_empty() && terminated != "Completed" {
            return (terminated, "dead");
        }
    }
    let phase = text(&item["status"]["phase"]);
    let ready = !statuses.is_empty() && statuses.iter().all(|status| status["ready"] == true);
    let state = match phase.as_str() {
        "Running" if ready => "running",
        "Running" | "Pending" => "starting",
        "Succeeded" => "exited",
        "Failed" => "dead",
        _ => "unknown",
    };
    (phase, state)
}

pub fn pods(output: &str) -> Vec<Row> {
    items(output)
        .iter()
        .filter_map(|item| {
            let name = name(item);
            if name.is_empty() {
                return None;
            }
            let statuses = item["status"]["containerStatuses"].as_array().cloned().unwrap_or_default();
            let containers = item["spec"]["containers"].as_array().map_or(statuses.len(), Vec::len);
            let ready = statuses.iter().filter(|status| status["ready"] == true).count();
            let restarts: i64 = statuses.iter().map(|status| count(&status["restartCount"])).sum();
            let (status, state) = pod_status(item);
            let columns = vec![
                name.clone(),
                format!("{ready}/{containers}"),
                status,
                restarts.to_string(),
                text(&item["spec"]["nodeName"]),
                created(item),
            ];
            Some(Row { id: format!("pod/{name}"), columns, state: Some(state.to_owned()) })
        })
        .collect()
}

pub fn deployments(output: &str) -> Vec<Row> {
    items(output)
        .iter()
        .filter_map(|item| {
            let name = name(item);
            if name.is_empty() {
                return None;
            }
            let wanted = item["spec"]["replicas"].as_i64().unwrap_or(1);
            let ready = count(&item["status"]["readyReplicas"]);
            let state = match (wanted, ready) {
                (0, _) => "exited",
                (wanted, ready) if ready >= wanted => "running",
                _ => "starting",
            };
            let columns = vec![
                name.clone(),
                format!("{ready}/{wanted}"),
                count(&item["status"]["updatedReplicas"]).to_string(),
                count(&item["status"]["availableReplicas"]).to_string(),
                created(item),
            ];
            Some(Row { id: format!("deployment/{name}"), columns, state: Some(state.to_owned()) })
        })
        .collect()
}

fn ports(item: &Value) -> String {
    item["spec"]["ports"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|port| {
            let number = text(&port["port"]);
            let protocol = text(&port["protocol"]);
            let node = text(&port["nodePort"]);
            let shown = if node.is_empty() { number } else { format!("{number}:{node}") };
            if protocol.is_empty() { shown } else { format!("{shown}/{protocol}") }
        })
        .collect::<Vec<_>>()
        .join(",")
}

pub fn services(output: &str) -> Vec<Row> {
    items(output)
        .iter()
        .filter_map(|item| {
            let name = name(item);
            if name.is_empty() {
                return None;
            }
            let columns = vec![
                name.clone(),
                text(&item["spec"]["type"]),
                text(&item["spec"]["clusterIP"]),
                ports(item),
                created(item),
            ];
            Some(Row { id: format!("service/{name}"), columns, state: None })
        })
        .collect()
}

pub fn config_maps(output: &str) -> Vec<Row> {
    items(output)
        .iter()
        .filter_map(|item| {
            let name = name(item);
            if name.is_empty() {
                return None;
            }
            let keys = ["data", "binaryData"]
                .iter()
                .map(|field| item[*field].as_object().map_or(0, serde_json::Map::len))
                .sum::<usize>();
            Some(Row {
                id: format!("configmap/{name}"),
                columns: vec![name, keys.to_string(), created(item)],
                state: None,
            })
        })
        .collect()
}

pub fn secrets(output: &str) -> Vec<Row> {
    output
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            let name = line.strip_prefix("secret/").or_else(|| line.strip_prefix("secret.v1/"))?;
            let valid = !name.is_empty()
                && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
            valid.then(|| Row { id: format!("secret/{name}"), columns: vec![name.to_owned()], state: None })
        })
        .collect()
}

pub fn events(output: &str) -> Vec<Row> {
    let mut rows: Vec<(String, Row)> = items(output)
        .iter()
        .filter_map(|item| {
            let name = name(item);
            if name.is_empty() {
                return None;
            }
            let seen = [&item["lastTimestamp"], &item["eventTime"], &item["metadata"]["creationTimestamp"]]
                .into_iter()
                .map(text)
                .find(|value| !value.is_empty())
                .unwrap_or_default();
            let kind = text(&item["type"]);
            let object = &item["involvedObject"];
            let about = format!("{}/{}", text(&object["kind"]).to_ascii_lowercase(), text(&object["name"]));
            let state = (kind == "Warning").then(|| "unhealthy".to_owned());
            let columns = vec![seen.clone(), kind, text(&item["reason"]), about, text(&item["message"])];
            Some((seen, Row { id: name, columns, state }))
        })
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    rows.into_iter().map(|(_, row)| row).collect()
}

pub fn rows(section: &str, output: &str) -> Vec<Row> {
    match section {
        "choice:context" | "contexts" => contexts(output),
        "choice:namespace" => namespaces(output, true),
        "namespaces" => namespaces(output, false),
        "pods" => pods(output),
        "deployments" => deployments(output),
        "services" => services(output),
        "configmaps" => config_maps(output),
        "secrets" => secrets(output),
        "events" => events(output),
        _ => Vec::new(),
    }
}

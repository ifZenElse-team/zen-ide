use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub pid: u32,
    pub url: String,
    pub product: String,
    pub projects: Vec<PathBuf>,
    pub started: u64,
}

pub fn folder(data: &Path) -> PathBuf {
    data.join("headless")
}

pub fn file(data: &Path, pid: u32) -> PathBuf {
    folder(data).join(format!("{pid}.json"))
}

pub fn record(data: &Path, instance: &Instance) -> std::io::Result<PathBuf> {
    let path = file(data, instance.pid);
    std::fs::create_dir_all(folder(data))?;
    let text = serde_json::to_string_pretty(instance).map_err(std::io::Error::other)?;
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, text)?;
    std::fs::rename(&partial, &path)?;
    Ok(path)
}

pub fn forget(data: &Path, pid: u32) {
    let _ = std::fs::remove_file(file(data, pid));
    let _ = std::fs::remove_dir_all(clients_folder(data, pid));
}

pub fn clients_folder(data: &Path, pid: u32) -> PathBuf {
    folder(data).join(format!("{pid}.clients"))
}

#[derive(Debug)]
pub struct Lease {
    file: std::fs::File,
    path: PathBuf,
}

impl Lease {
    pub fn take(data: &Path, pid: u32) -> std::io::Result<Self> {
        let folder = clients_folder(data, pid);
        std::fs::create_dir_all(&folder)?;
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = folder.join(format!("{}-{serial}.lock", std::process::id()));
        let file = std::fs::OpenOptions::new().create(true).truncate(true).write(true).open(&path)?;
        file.lock()?;
        Ok(Self { file, path })
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = self.file.unlock();
    }
}

pub fn leases(data: &Path, pid: u32) -> Option<usize> {
    clients_folder(data, pid).is_dir().then(|| clients(data, pid))
}

pub fn clients(data: &Path, pid: u32) -> usize {
    let Ok(entries) = std::fs::read_dir(clients_folder(data, pid)) else { return 0 };
    let mut live = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "lock") {
            continue;
        }
        let Ok(file) = std::fs::OpenOptions::new().write(true).open(&path) else { continue };
        match file.try_lock() {
            Ok(()) => {
                let _ = std::fs::remove_file(&path);
                let _ = file.unlock();
            }
            Err(_) => live += 1,
        }
    }
    live
}

pub fn list(data: &Path) -> Vec<Instance> {
    let Ok(entries) = std::fs::read_dir(folder(data)) else { return Vec::new() };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let instance = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice::<Instance>(&bytes).ok());
        match instance {
            Some(instance) if alive(&instance.url) => found.push(instance),
            _ => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    found.sort_by_key(|instance| (instance.started, instance.pid));
    found
}

pub fn with_project(data: &Path, project: &Path) -> Option<Instance> {
    let project = std::fs::canonicalize(project).unwrap_or_else(|_| project.to_owned());
    list(data).into_iter().find(|instance| instance.projects.contains(&project))
}

pub fn alive(url: &str) -> bool {
    let ping = json!({ "jsonrpc": "2.0", "id": "alive", "method": "ping" });
    ureq::post(url)
        .header("Content-Type", "application/json")
        .config()
        .timeout_global(Some(Duration::from_secs(2)))
        .build()
        .send(ping.to_string())
        .is_ok()
}

pub fn call(url: &str, tool: &str, arguments: Value) -> Result<Value, String> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": { "name": tool, "arguments": arguments },
    });
    let mut response = ureq::post(url)
        .header("Content-Type", "application/json")
        .config()
        .http_status_as_error(false)
        .build()
        .send(request.to_string())
        .map_err(|error| error.to_string())?;
    let text = response.body_mut().read_to_string().map_err(|error| error.to_string())?;
    let answer: Value = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    if let Some(error) = answer.get("error") {
        return Err(error["message"].as_str().unwrap_or("error").to_owned());
    }
    let result = &answer["result"];
    let text = || result["content"][0]["text"].as_str().unwrap_or_default().to_owned();
    if result["isError"] == true {
        return Err(text());
    }
    Ok(serde_json::from_str(&text()).unwrap_or_else(|_| result["structuredContent"].clone()))
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |duration| duration.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ServerInfo, Tool};

    #[test]
    fn records_list_live_instances_and_drop_stale_ones() {
        let data = tempfile::tempdir().unwrap();
        let project = data.path().canonicalize().unwrap();
        let tools = vec![Tool {
            name: "echo".to_owned(),
            description: "Echoes.".to_owned(),
            input_schema: json!({ "type": "object" }),
            output_schema: None,
        }];
        let info = ServerInfo { name: "Test".to_owned(), version: "1".to_owned(), instructions: String::new() };
        let (server, calls) = crate::Server::start(0, info, tools, || {}).unwrap();
        std::thread::spawn(move || {
            for call in calls {
                let arguments = call.arguments.clone();
                call.reply(Ok(arguments));
            }
        });
        let live = Instance {
            pid: 1,
            url: server.url(),
            product: "test".to_owned(),
            projects: vec![project.clone()],
            started: 10,
        };
        let stale = Instance { pid: 2, url: "http://127.0.0.1:1/mcp".to_owned(), started: 5, ..live.clone() };
        record(data.path(), &live).unwrap();
        record(data.path(), &stale).unwrap();

        assert_eq!(list(data.path()), vec![live.clone()]);
        assert!(!file(data.path(), 2).exists(), "the stale record is gone");
        assert_eq!(with_project(data.path(), &project), Some(live.clone()));
        assert_eq!(with_project(data.path(), &project.join("nope")), None);
        assert_eq!(call(&live.url, "echo", json!({ "a": 1 })), Ok(json!({ "a": 1 })));
        assert!(call(&live.url, "nope", json!({})).is_err());
        forget(data.path(), 1);
        assert_eq!(list(data.path()), Vec::new());
    }

    #[test]
    fn leases_count_while_held_and_go_with_their_client() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(clients(data.path(), 7), 0);
        assert_eq!(leases(data.path(), 7), None, "no client yet");
        let first = Lease::take(data.path(), 7).unwrap();
        let second = Lease::take(data.path(), 7).unwrap();
        assert_eq!(clients(data.path(), 7), 2);
        assert_eq!(clients(data.path(), 8), 0, "leases are per IDE");
        drop(first);
        assert_eq!(clients(data.path(), 7), 1);

        let stale = clients_folder(data.path(), 7).join("999999-0.lock");
        std::fs::write(&stale, "").unwrap();
        assert_eq!(clients(data.path(), 7), 1);
        assert!(!stale.exists(), "the stale lease is removed");

        drop(second);
        assert_eq!(clients(data.path(), 7), 0);
        assert_eq!(leases(data.path(), 7), Some(0), "the clients have been and gone");
        forget(data.path(), 7);
        assert!(!clients_folder(data.path(), 7).exists());
        assert_eq!(leases(data.path(), 7), None);
    }
}

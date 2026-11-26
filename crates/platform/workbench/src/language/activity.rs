use std::collections::BTreeMap;
use std::time::Instant;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::Value;

/// A piece of work a server reported with `$/progress`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ServerProgress {
    /// What it does, e.g. `Indexing`.
    pub title: String,
    /// Its current step, e.g. `12/40 (core)`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// How far it is, from 0 to 100, when the server says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentage: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Import {
    Running,
    Done,
    Failed(String),
}

#[derive(Clone, Debug)]
pub(crate) struct Activity {
    progress: BTreeMap<String, (u64, ServerProgress)>,
    begun: u64,
    quiescent: Option<bool>,
    status: Option<String>,
    import: Option<Import>,
    started: Instant,
    ready_after: Option<u64>,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            progress: BTreeMap::new(),
            begun: 0,
            quiescent: None,
            status: None,
            import: None,
            started: Instant::now(),
            ready_after: None,
        }
    }
}

impl Activity {
    pub(crate) fn notification(&mut self, method: &str, params: &Value) -> bool {
        match method {
            "$/progress" => {
                let token = match &params["token"] {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                };
                let value = &params["value"];
                let text = |key: &str| value[key].as_str().map(str::to_owned);
                let percentage = value["percentage"].as_u64().map(|value| value.min(100) as u32);
                match value["kind"].as_str() {
                    Some("begin") => {
                        self.begun += 1;
                        let progress = ServerProgress {
                            title: text("title").unwrap_or_default(),
                            message: text("message"),
                            percentage,
                        };
                        self.progress.insert(token, (self.begun, progress));
                    }
                    Some("report") => {
                        if let Some((_, progress)) = self.progress.get_mut(&token) {
                            if let Some(message) = text("message") {
                                progress.message = Some(message);
                            }
                            progress.percentage = percentage.or(progress.percentage);
                        }
                    }
                    Some("end") => {
                        self.progress.remove(&token);
                    }
                    _ => return false,
                }
            }
            "experimental/serverStatus" => {
                self.quiescent = params["quiescent"].as_bool();
                let healthy = params["health"].as_str().is_none_or(|health| health == "ok");
                self.status = params["message"]
                    .as_str()
                    .map(str::trim)
                    .filter(|message| !healthy && !message.is_empty())
                    .map(str::to_owned);
            }
            "language/status" => match params["type"].as_str() {
                Some("Starting") if self.import.is_none() => self.import = Some(Import::Running),
                Some("Starting") => {}
                Some("Started") => self.import = Some(Import::Done),
                Some("Error") => {
                    self.import = Some(Import::Failed(params["message"].as_str().unwrap_or_default().to_owned()));
                }
                _ => return false,
            },
            _ => return false,
        }
        if !self.busy() && self.ready_after.is_none() {
            self.ready_after = Some(u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX));
        }
        true
    }

    pub(crate) fn busy(&self) -> bool {
        !self.progress.is_empty() || self.quiescent == Some(false) || self.import == Some(Import::Running)
    }

    pub(crate) fn import(&self) -> Option<&Import> {
        self.import.as_ref()
    }

    pub(crate) fn progress(&self) -> Vec<ServerProgress> {
        let mut found: Vec<&(u64, ServerProgress)> = self.progress.values().collect();
        found.sort_by_key(|(begun, _)| *begun);
        found.into_iter().map(|(_, progress)| progress.clone()).collect()
    }

    pub(crate) fn status(&self) -> Option<&str> {
        self.status.as_deref()
    }

    pub(crate) fn ready_after(&self) -> Option<u64> {
        self.ready_after
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_java_server_says_when_its_import_ends() {
        let mut activity = Activity::default();
        assert_eq!(activity.import(), None);
        let status = |kind: &str, message: &str| json!({ "type": kind, "message": message });
        assert!(activity.notification("language/status", &status("Starting", "Init...")));
        assert_eq!(activity.import(), Some(&Import::Running));
        assert!(activity.busy());
        assert!(!activity.notification("language/status", &status("ProjectStatus", "OK")));
        assert!(activity.notification("language/status", &status("Started", "Ready")));
        assert_eq!(activity.import(), Some(&Import::Done));
        assert!(activity.notification("language/status", &status("Starting", "1100% Starting Java Language Server")));
        assert_eq!(activity.import(), Some(&Import::Done), "a Gradle import reports progress after its end");
        assert!(!activity.busy());
        assert!(activity.ready_after().is_some());
        assert!(activity.notification("language/status", &status("Error", "the import failed")));
        assert_eq!(activity.import(), Some(&Import::Failed("the import failed".to_owned())));
    }

    #[test]
    fn progress_begins_reports_and_ends() {
        let mut activity = Activity::default();
        assert!(!activity.busy());
        let progress = |token: Value, value: Value| json!({ "token": token, "value": value });
        assert!(activity.notification(
            "$/progress",
            &progress(json!("rustAnalyzer/Indexing"), json!({ "kind": "begin", "title": "Indexing", "percentage": 0 }))
        ));
        assert!(
            activity
                .notification("$/progress", &progress(json!(7), json!({ "kind": "begin", "title": "cargo check" })))
        );
        assert!(activity.busy());
        activity.notification(
            "$/progress",
            &progress(
                json!("rustAnalyzer/Indexing"),
                json!({ "kind": "report", "message": "12/40 (core)", "percentage": 30 }),
            ),
        );
        assert_eq!(
            activity.progress(),
            [
                ServerProgress { title: "Indexing".into(), message: Some("12/40 (core)".into()), percentage: Some(30) },
                ServerProgress { title: "cargo check".into(), message: None, percentage: None },
            ]
        );
        activity.notification("$/progress", &progress(json!("rustAnalyzer/Indexing"), json!({ "kind": "end" })));
        activity.notification("$/progress", &progress(json!(7), json!({ "kind": "end" })));
        assert!(!activity.busy());
        assert!(activity.ready_after().is_some());
        assert!(!activity.notification("textDocument/publishDiagnostics", &json!({})));
    }

    #[test]
    fn rust_analyzers_status_says_when_it_is_loading() {
        let mut activity = Activity::default();
        activity.notification("experimental/serverStatus", &json!({ "health": "ok", "quiescent": false }));
        assert!(activity.busy());
        assert_eq!(activity.ready_after(), None);
        activity.notification(
            "experimental/serverStatus",
            &json!({ "health": "warning", "quiescent": true, "message": "Failed to load sysroot\n" }),
        );
        assert!(!activity.busy());
        assert_eq!(activity.status(), Some("Failed to load sysroot"));
        activity
            .notification("experimental/serverStatus", &json!({ "health": "ok", "quiescent": true, "message": "ok" }));
        assert_eq!(activity.status(), None);
        assert!(activity.ready_after().is_some());
    }
}

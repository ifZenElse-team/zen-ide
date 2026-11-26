use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::Session;
use zen_i18n::t;

use crate::settings::{NOTIFICATIONS, NOTIFIED};
use crate::{SelfUpdate, Snapshot, TOOLBOX_ID, ToolState, Tools};

/// A system notification about a new version.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// The product id, `toolbox` for Zen Toolbox itself.
    pub id: String,
    /// The new version.
    pub version: String,
    /// The notification's title.
    pub title: String,
    /// The notification's text.
    pub body: String,
}

fn notice(id: &str, name: &str, version: &str) -> Notice {
    Notice {
        id: id.to_owned(),
        version: version.to_owned(),
        title: t!("toolbox-notice-title", tool = name, version = version),
        body: t!("toolbox-notice-body"),
    }
}

pub fn due(snapshot: &Snapshot, told: &BTreeMap<String, String>) -> Vec<Notice> {
    let mut notices: Vec<Notice> = snapshot
        .tools
        .iter()
        .filter_map(|tool| match &tool.state {
            ToolState::Installed { update: Some(version), .. } => Some(notice(&tool.id, &tool.name, version)),
            _ => None,
        })
        .collect();
    if let SelfUpdate::Available(version) = &snapshot.self_update {
        notices.push(notice(TOOLBOX_ID, "Zen Toolbox", version));
    }
    notices.retain(|notice| told.get(&notice.id) != Some(&notice.version));
    notices
}

pub fn follow(session: &mut Session) -> Vec<Notice> {
    let Some(tools) = session.state::<Tools>().cloned() else { return Vec::new() };
    if !session.settings().get_or(NOTIFICATIONS, true) {
        return Vec::new();
    }
    let mut told: BTreeMap<String, String> = session.settings().get(NOTIFIED).unwrap_or_default();
    let notices = due(&tools.snapshot(), &told);
    if notices.is_empty() {
        return notices;
    }
    for notice in &notices {
        tools.notify(notice);
        told.insert(notice.id.clone(), notice.version.clone());
    }
    let _ = session.settings_mut().set(NOTIFIED, &told);
    notices
}

#[cfg(target_os = "macos")]
pub fn system(notice: &Notice) {
    const SCRIPT: [&str; 6] = [
        "-e",
        "on run argv",
        "-e",
        "display notification (item 2 of argv) with title (item 1 of argv)",
        "-e",
        "end run",
    ];
    let started = std::process::Command::new("/usr/bin/osascript")
        .args(SCRIPT)
        .args([&notice.title, &notice.body])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    if let Ok(mut child) = started {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

#[cfg(not(target_os = "macos"))]
pub fn system(_: &Notice) {}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::{Config, Tool};

    fn tool(id: &str, version: &str, update: Option<&str>) -> Tool {
        Tool {
            id: id.to_owned(),
            name: id.to_owned(),
            subject: String::new(),
            bundle_id: String::new(),
            launcher: id.to_owned(),
            state: ToolState::Installed {
                version: version.to_owned(),
                update: update.map(str::to_owned),
                running: false,
            },
            download_size: None,
        }
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            tools: vec![tool("gogarden", "1.0.0", Some("1.1.0")), tool("rustronin", "2.0.0", None)],
            self_update: SelfUpdate::Available("0.9.0".to_owned()),
            ..Snapshot::default()
        }
    }

    #[test]
    fn installed_tools_with_an_update_and_toolbox_itself_are_announced() {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let notices = due(&snapshot(), &BTreeMap::new());
        let ids: Vec<(&str, &str)> = notices.iter().map(|n| (n.id.as_str(), n.version.as_str())).collect();
        assert_eq!(ids, [("gogarden", "1.1.0"), (TOOLBOX_ID, "0.9.0")]);
        assert_eq!(notices[0].title, "gogarden 1.1.0 is available");
    }

    #[test]
    fn a_version_is_announced_once_and_a_newer_one_again() {
        let told = BTreeMap::from([("gogarden".to_owned(), "1.1.0".to_owned())]);
        let notices = due(&snapshot(), &told);
        assert_eq!(notices.iter().map(|n| n.id.as_str()).collect::<Vec<_>>(), [TOOLBOX_ID]);
        let mut newer = snapshot();
        newer.tools[0] = tool("gogarden", "1.0.0", Some("1.2.0"));
        assert_eq!(due(&newer, &told).len(), 2);
    }

    fn session(sent: &Arc<Mutex<Vec<Notice>>>) -> Session {
        let sent = Arc::clone(sent);
        let tools = Tools::new(Config::default()).with_notifier(Arc::new(move |notice: &Notice| {
            sent.lock().unwrap().push(notice.clone());
        }));
        tools.set_update_for_test("0.9.0");
        let mut session = Session::new();
        session.set_state(tools);
        session
    }

    #[test]
    fn following_twice_sends_once_and_remembers_it_in_the_settings() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut session = session(&sent);
        assert_eq!(follow(&mut session).len(), 1);
        assert!(follow(&mut session).is_empty(), "once per version");
        assert_eq!(sent.lock().unwrap().len(), 1);
        let told: BTreeMap<String, String> = session.settings().get(NOTIFIED).unwrap();
        assert_eq!(told.get(TOOLBOX_ID).map(String::as_str), Some("0.9.0"));
    }

    #[test]
    fn switched_off_nothing_is_sent_or_remembered() {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let mut session = session(&sent);
        session.settings_mut().set(NOTIFICATIONS, false).unwrap();
        assert!(follow(&mut session).is_empty());
        assert!(sent.lock().unwrap().is_empty());
        assert_eq!(session.settings().get::<BTreeMap<String, String>>(NOTIFIED), None);
    }
}

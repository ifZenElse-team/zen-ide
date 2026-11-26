use std::path::Path;

use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::notifications::{Action, Group, Kind, NewNotification};

use super::IdeView;

impl IdeView {
    pub(super) fn follow_plugin_projects(&mut self, registry: &Registry, session: &mut Session) {
        let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) else {
            return;
        };
        let seen = (root, self.plugins.generation());
        if self.plugin_projects_seen.as_ref() == Some(&seen) {
            return;
        }
        let root = seen.0.clone();
        self.plugin_projects_seen = Some(seen);
        for plugin in self.plugins.project_offers(&root) {
            let requires = self.plugins.missing_requirements(&plugin);
            let body = if requires.is_empty() {
                t!("ide-plugin-project-suggestion", name = plugin.name.as_str())
            } else {
                t!("ide-plugin-project-suggestion-with", name = plugin.name.as_str(), requires = requires.join(", "))
            };
            let offer = NewNotification::message(Group::Plugins, Kind::Info, body)
                .action(Action::new(
                    "install",
                    t!("ide-plugin-install", name = plugin.name.as_str()),
                    "view.plugin.install",
                    json!({ "id": plugin.id }),
                ))
                .action(Action::new(
                    "ignore",
                    t!("ide-plugin-project-ignore"),
                    "view.plugin.ignoreForProjects",
                    json!({ "id": plugin.id }),
                ));
            self.notifications.queue_once(format!("plugin-project:{}:{}", plugin.id, root.display()), offer);
        }
        let Ok(applied) = registry.dispatch(session, "plugins.project.apply", json!({})) else { return };
        let added = applied["added"].as_array().cloned().unwrap_or_default();
        let names: Vec<&str> = added.iter().filter_map(|added| added["name"].as_str()).collect();
        if !names.is_empty() {
            let message = t!("ide-plugin-configurations-added", names = names.join(", "));
            self.notify(NewNotification::message(Group::Plugins, Kind::Info, message).silent());
        }
        for offer in applied["offers"].as_array().cloned().unwrap_or_default() {
            let text = |key: &str| offer[key].as_str().unwrap_or_default().to_owned();
            let (plugin, name, creates) = (text("pluginName"), text("name"), text("creates"));
            let body = t!(
                "ide-plugin-setup-offer",
                plugin = plugin.as_str(),
                creates = creates.as_str(),
                name = name.as_str()
            );
            self.notify(NewNotification::message(Group::Plugins, Kind::Info, body).action(Action::new(
                "setup",
                t!("ide-plugin-setup-run", name = name.as_str()),
                "plugins.project.setup",
                json!({ "plugin": offer["plugin"] }),
            )));
        }
    }
}

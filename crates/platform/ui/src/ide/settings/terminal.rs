use egui::Ui;
use zen_i18n::t;
use zen_terminal::links::LinkKind;
use zen_workbench::terminal::LinkAction;
use zen_workbench::terminal::agents::AgentAdapter;

use super::{SettingsDialog, widgets};
use crate::ide::IdePalette;
use crate::ide::terminal_link_dialog::action_label;

fn choice_name(choice: Option<LinkAction>) -> String {
    choice.map_or_else(|| t!("settings-terminal-links-ask"), action_label)
}

fn slug(name: &str) -> String {
    let mut slug = String::new();
    for ch in name.trim().to_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').chars().take(40).collect()
}

pub(super) fn adapter_from_form([name, program, resume]: &[String; 3]) -> Option<AgentAdapter> {
    let adapter = AgentAdapter {
        id: slug(name),
        name: name.trim().to_owned(),
        programs: vec![program.trim().to_owned()],
        resume: resume.split_whitespace().map(str::to_owned).collect(),
        session_field: "/session_id".to_owned(),
        docs: None,
    };
    adapter.is_valid().then_some(adapter)
}

impl SettingsDialog {
    fn terminal_agents(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::group_title(ui, p, &t!("settings-terminal-agents"), query);
        ui.add(
            egui::Label::new(egui::RichText::new(t!("settings-terminal-agents-about")).color(p.text_secondary)).wrap(),
        );
        ui.add_space(6.0);
        let mut remove = None;
        for (index, adapter) in draft.terminal_agents.iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(&adapter.name).color(p.text));
                ui.label(egui::RichText::new(adapter.resume.join(" ")).monospace().color(p.text_secondary));
                if ui.button(t!("settings-terminal-agents-remove")).clicked() {
                    remove = Some(index);
                }
            });
            ui.label(egui::RichText::new(adapter.hook()).monospace().small().color(p.text_secondary));
        }
        if let Some(index) = remove {
            draft.terminal_agents.remove(index);
        }
        let [name, program, resume] = &mut self.agent_form;
        widgets::text_field(ui, p, &t!("settings-terminal-agents-name"), query, name);
        widgets::text_field(ui, p, &t!("settings-terminal-agents-program"), query, program);
        widgets::text_field(ui, p, &t!("settings-terminal-agents-resume"), query, resume);
        let adapter = adapter_from_form(&self.agent_form);
        let taken =
            adapter.as_ref().is_some_and(|adapter| draft.terminal_agents.iter().any(|known| known.id == adapter.id));
        if ui.add_enabled(adapter.is_some() && !taken, egui::Button::new(t!("settings-terminal-agents-add"))).clicked()
            && let Some(adapter) = adapter
        {
            draft.terminal_agents.push(adapter);
            self.agent_form = Default::default();
        }
    }

    pub(super) fn terminal_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        widgets::group_title(ui, p, &t!("settings-terminal-sessions"), query);
        widgets::checkbox(ui, p, &mut draft.terminal_restore, &t!("settings-terminal-restore"), query);
        ui.add(
            egui::Label::new(egui::RichText::new(t!("settings-terminal-restore-about")).color(p.text_secondary)).wrap(),
        );
        ui.add_space(12.0);
        widgets::group_title(ui, p, &t!("settings-terminal-environment"), query);
        widgets::checkbox(ui, p, &mut draft.login_shell_environment, &t!("settings-terminal-login-shell"), query);
        ui.add(
            egui::Label::new(egui::RichText::new(t!("settings-terminal-login-shell-about")).color(p.text_secondary))
                .wrap(),
        );
        ui.add_space(12.0);
        self.terminal_agents(ui, p, query);
        let Some(draft) = self.draft.as_mut() else { return };
        ui.add_space(12.0);
        widgets::group_title(ui, p, &t!("settings-terminal-links"), query);
        ui.add(
            egui::Label::new(egui::RichText::new(t!("settings-terminal-links-about")).color(p.text_secondary)).wrap(),
        );
        ui.add_space(8.0);
        for (kind, label, salt) in [
            (LinkKind::Url, t!("settings-terminal-links-url"), "settings-terminal-links-url"),
            (LinkKind::File, t!("settings-terminal-links-file"), "settings-terminal-links-file"),
        ] {
            let mut choice = draft.terminal_links.get(kind);
            ui.horizontal(|ui| {
                ui.label(widgets::label_job(ui, p, &label, query));
                let combo = egui::ComboBox::from_id_salt(salt)
                    .selected_text(choice_name(choice))
                    .width(widgets::field_width(ui, 160.0, 220.0))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut choice, None, choice_name(None));
                        for action in LinkAction::offered(kind) {
                            ui.selectable_value(&mut choice, Some(*action), choice_name(Some(*action)));
                        }
                    });
                crate::accessibility::name(&combo.response, egui::Role::ComboBox, &label);
            });
            draft.terminal_links.set(kind, choice);
            ui.add_space(4.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_form_makes_an_adapter_only_with_one_place_for_the_conversation_id() {
        let form = ["My Agent 2".to_owned(), "mine".to_owned(), "mine -r {session}".to_owned()];
        let adapter = adapter_from_form(&form).unwrap();
        assert_eq!((adapter.id.as_str(), adapter.resume.len()), ("my-agent-2", 3));
        assert_eq!(adapter.resume_line("c-1").as_deref(), Some("mine -r c-1"));
        assert!(adapter_from_form(&["X".to_owned(), "x".to_owned(), "x --last".to_owned()]).is_none());
        assert!(adapter_from_form(&["".to_owned(), "x".to_owned(), "x {session}".to_owned()]).is_none());
    }
}

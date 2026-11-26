use std::path::{Path, PathBuf};

use egui::{Align2, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_toolbox::{ShellScriptsView, Tools};

use super::{ToolboxView, metrics};
use crate::components::{self, IconButton};
use crate::{icons, theme};

#[derive(Debug, Default)]
pub(super) struct SettingsPage {
    location: Option<String>,
    added: Option<Result<Vec<PathBuf>, String>>,
}

pub(super) fn tilde(path: &Path) -> String {
    match zen_update::scripts::home().and_then(|home| path.strip_prefix(home).ok().map(Path::to_owned)) {
        Some(relative) if relative.as_os_str().is_empty() => "~".to_owned(),
        Some(relative) => format!("~/{}", relative.display()),
        None => path.display().to_string(),
    }
}

fn files(paths: &[PathBuf]) -> String {
    let names: Vec<String> = paths.iter().map(|path| tilde(path)).collect();
    match names.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => t!("toolbox-settings-and", first = rest.join(", "), last = last.as_str()),
    }
}

impl ToolboxView {
    pub fn open_settings(&mut self, session: &Session) {
        self.settings = Some(SettingsPage::default());
        if let Some(tools) = session.state::<Tools>() {
            tools.check_path();
        }
    }

    pub(super) fn settings_page(&mut self, ui: &mut Ui, body: Rect, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let row = Rect::from_min_size(body.min, vec2(body.width(), metrics::TABS));
        let back = Rect::from_center_size(pos2(row.left() + 15.0, row.center().y), Vec2::splat(30.0));
        let response = components::icon_button_named(
            ui,
            &p,
            back,
            ui.scope_id().with("settings-back"),
            IconButton { icon: icons::chevron_left, icon_size: 16.0, color: p.text_secondary, radius: 8 },
            || t!("toolbox-settings-back"),
        );
        ui.painter().text(
            pos2(back.right() + 6.0, row.center().y),
            Align2::LEFT_CENTER,
            t!("toolbox-settings-title"),
            theme::semibold(15.0),
            p.text,
        );
        if response.clicked() || (self.licences.is_none() && ui.input(|input| input.key_pressed(egui::Key::Escape))) {
            self.settings = None;
            return;
        }
        let view = session.state::<Tools>().map(Tools::shell_scripts).unwrap_or_default();
        if view.status.is_none() && view.location.is_some() && !view.demo {
            if !view.checking {
                self.recheck(session);
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        }
        let rest = Rect::from_min_max(pos2(body.left(), row.bottom() + 4.0), body.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rest));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| {
            self.panel(ui, |page, ui| page.shell_scripts(ui, registry, session, &view));
            self.panel(ui, |page, ui| page.notifications(ui, registry, session, view.demo));
            self.panel(ui, |page, ui| page.licences_section(ui, registry, session));
        });
        let frame = egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(16));
        if let Some(view) = &mut self.licences
            && view.show(ui, frame, "Zen Toolbox")
        {
            self.licences = None;
        }
    }

    pub fn open_licences(&mut self, registry: &Registry, session: &mut Session) {
        let listed = registry.dispatch(session, "toolbox.licences.list", serde_json::json!({}));
        let folder = listed.ok().and_then(|listed| listed["folder"].as_str().map(PathBuf::from));
        self.licences = Some(crate::licences::LicencesView::new(folder));
    }

    pub fn licences_open(&self) -> bool {
        self.licences.is_some()
    }

    fn licences_section(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let _ = self.section_header(ui, &t!("toolbox-settings-licences"), None);
        let mut open = false;
        egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 0, bottom: 10 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            let hint = t!("toolbox-settings-licences-hint");
            ui.add(egui::Label::new(RichText::new(hint).font(theme::regular(12.5)).color(p.text_secondary)).wrap());
            let label = RichText::new(t!("toolbox-settings-licences-show")).font(theme::regular(13.0)).color(p.text);
            open = ui.button(label).clicked();
        });
        if open {
            self.open_licences(registry, session);
        }
    }

    fn notifications(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, demo: bool) {
        let p = self.palette;
        let _ = self.section_header(ui, &t!("toolbox-settings-notifications"), None);
        let enabled = session.settings().get_or(zen_toolbox::settings::NOTIFICATIONS, true);
        let mut on = enabled;
        egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 0, bottom: 10 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let label = t!("toolbox-settings-notifications-new-versions");
            ui.horizontal(|ui| {
                ui.label(RichText::new(label.as_str()).font(theme::regular(13.5)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(!demo, |ui| components::toggle(ui, &p, &mut on, &label));
                });
            });
            let hint = t!("toolbox-settings-notifications-hint");
            ui.add(egui::Label::new(RichText::new(hint).font(theme::regular(12.5)).color(p.text_secondary)).wrap());
        });
        if on != enabled {
            self.dispatch(ui, registry, session, "toolbox.settings.set", serde_json::json!({ "notifications": on }));
        }
    }

    fn shell_scripts(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, view: &ShellScriptsView) {
        let p = self.palette;
        let _ = self.section_header(ui, &t!("toolbox-settings-shell-scripts"), None);
        egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 0, bottom: 10 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let secondary = |text: String| RichText::new(text).font(theme::regular(12.5)).color(p.text_secondary);

            let label = t!("toolbox-settings-shell-scripts-generate");
            let mut on = view.enabled;
            ui.horizontal(|ui| {
                ui.label(RichText::new(label.as_str()).font(theme::regular(13.5)).color(p.text));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(!view.demo, |ui| components::toggle(ui, &p, &mut on, &label));
                });
            });
            if on != view.enabled {
                let params = serde_json::json!({ "shellScriptsEnabled": on });
                self.dispatch(ui, registry, session, "toolbox.settings.set", params);
                self.recheck(session);
            }
            ui.add(egui::Label::new(secondary(t!("toolbox-settings-shell-scripts-hint"))).wrap());
            ui.add_space(6.0);

            ui.label(
                RichText::new(t!("toolbox-settings-shell-scripts-location")).font(theme::regular(13.5)).color(p.text),
            );
            let shown = view.location.as_deref().map(tilde).unwrap_or_default();
            let page = self.settings.get_or_insert_default();
            let mut text = page.location.clone().unwrap_or_else(|| shown.clone());
            let mut apply = false;
            let row = vec2(ui.available_width(), components::metrics::TEXT_FIELD);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let edited = text != shown;
                apply = ui.add_enabled(edited && !view.demo, egui::Button::new(t!("toolbox-settings-apply"))).clicked();
                let (field, _) = ui.allocate_exact_size(vec2(ui.available_width(), row.y), Sense::hover());
                let hint = t!("toolbox-settings-shell-scripts-location");
                let response = components::text_field(ui, &p, field, &mut text, &hint, None);
                apply |= response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            });
            page.location = (text != shown).then_some(text.clone());
            if apply && text != shown && !view.demo {
                let location = zen_toolbox::launchers::expand_home(Path::new(text.trim()));
                let params = serde_json::json!({ "shellScriptsLocation": location.display().to_string() });
                self.dispatch(ui, registry, session, "toolbox.settings.set", params);
                if let Some(page) = &mut self.settings {
                    page.location = None;
                    page.added = None;
                }
                self.recheck(session);
            }

            if view.demo {
                ui.label(secondary(t!("toolbox-settings-demo")));
                return;
            }
            let names: Vec<String> = view
                .launchers
                .iter()
                .filter_map(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
                .collect();
            if view.status.is_some() {
                let text = if names.is_empty() {
                    t!("toolbox-settings-no-scripts")
                } else {
                    t!("toolbox-settings-scripts", names = names.join(", "))
                };
                ui.add(egui::Label::new(secondary(text)).wrap());
            }
            ui.add_space(4.0);
            self.path_hint(ui, registry, session, view, names.first().map(String::as_str));
            ui.add_space(4.0);
            self.completions(ui, registry, session, view);
        });
    }

    fn path_hint(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        view: &ShellScriptsView,
        example: Option<&str>,
    ) {
        let p = self.palette;
        let Some(status) = &view.status else {
            ui.label(
                RichText::new(t!("toolbox-settings-path-checking")).font(theme::regular(12.5)).color(p.text_muted),
            );
            return;
        };
        let added = self.settings.as_ref().and_then(|page| page.added.clone());
        let text =
            |text: String, color| egui::Label::new(RichText::new(text).font(theme::regular(13.0)).color(color)).wrap();
        if status.on_path && added.is_none() {
            let example = format!("{} .", example.unwrap_or("gogarden"));
            ui.add(text(t!("toolbox-settings-path-ok", example = example.as_str()), p.running));
            return;
        }
        if let Some(added) = added {
            match added {
                Ok(changed) if changed.is_empty() => ui.add(text(t!("toolbox-settings-path-already"), p.text)),
                Ok(changed) => ui.add(text(t!("toolbox-settings-path-added", files = files(&changed)), p.running)),
                Err(error) => ui.add(text(error, p.attention)),
            };
            return;
        }
        if status.in_profile {
            ui.add(text(t!("toolbox-settings-path-new-window"), p.text));
            return;
        }
        ui.add(text(t!("toolbox-settings-path-missing"), p.attention));
        egui::Frame::new()
            .fill(p.hover)
            .stroke(Stroke::new(1.0, p.panel_edge))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(RichText::new(status.line.as_str()).monospace().size(12.0).color(p.text))
                        .selectable(true)
                        .wrap(),
                );
            });
        let button = egui::Button::new(
            RichText::new(t!("toolbox-settings-path-add")).font(theme::semibold(13.0)).color(p.on_accent),
        )
        .fill(p.accent[1])
        .corner_radius(8)
        .min_size(vec2(0.0, 28.0));
        if ui.add(button).clicked() {
            let result = registry
                .dispatch(session, "toolbox.shellScripts.addToPath", serde_json::json!({}))
                .map_err(super::error_message)
                .and_then(|value| {
                    serde_json::from_value::<zen_toolbox::AddedToPath>(value).map_err(|error| error.to_string())
                })
                .map(|added| added.changed);
            if let Some(page) = &mut self.settings {
                page.added = Some(result);
            }
        }
        ui.add(
            egui::Label::new(
                RichText::new(t!("toolbox-settings-path-add-detail", files = files(&status.profiles)))
                    .font(theme::regular(12.0))
                    .color(p.text_secondary),
            )
            .wrap(),
        );
    }

    fn completions(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, view: &ShellScriptsView) {
        let p = self.palette;
        let Some(status) = &view.status else { return };
        let label =
            |text: String, color| egui::Label::new(RichText::new(text).font(theme::regular(12.5)).color(color)).wrap();
        if status.completions {
            ui.add(label(t!("toolbox-settings-completions-on"), p.text_secondary));
            return;
        }
        ui.add(label(t!("toolbox-settings-completions-hint"), p.text_secondary));
        let button = egui::Button::new(
            RichText::new(t!("toolbox-settings-completions-add")).font(theme::semibold(13.0)).color(p.text),
        )
        .fill(p.hover)
        .corner_radius(8)
        .min_size(vec2(0.0, 28.0));
        if ui.add_enabled(!view.demo, button).clicked() {
            self.dispatch(ui, registry, session, "toolbox.shellScripts.addCompletions", serde_json::json!({}));
            self.recheck(session);
        }
    }

    fn recheck(&mut self, session: &Session) {
        if let Some(tools) = session.state::<Tools>() {
            tools.check_path();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_named_as_people_type_them() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let Some(home) = zen_update::scripts::home() else { return };
        assert_eq!(tilde(&home.join(".local/bin")), "~/.local/bin");
        assert_eq!(tilde(Path::new("/opt/bin")), "/opt/bin");
        assert_eq!(files(&[home.join(".zprofile")]), "~/.zprofile");
        assert_eq!(files(&[home.join(".zprofile"), home.join(".bash_profile")]), "~/.zprofile and ~/.bash_profile");
    }
}

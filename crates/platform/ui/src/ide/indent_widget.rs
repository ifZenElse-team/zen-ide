use egui::{Response, Ui};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_project::settings::CodeStyle;
use zen_workbench::Workbench;
use zen_workbench::file_indent::{self, FileIndent, IndentSource};

use super::settings::PageId;
use super::{IdeView, accessibility};

#[derive(Default)]
pub(super) struct IndentUi {
    pub(super) open_menu: bool,
    pub(super) menu_open: bool,
}

enum Choice {
    Detected,
    Configured,
    Settings,
}

pub(super) fn describe(style: CodeStyle) -> String {
    if style.use_tabs { t!("ide-status-tab") } else { t!("ide-status-spaces", count = style.indent) }
}

fn hint_of(indent: &FileIndent) -> String {
    match indent.source {
        IndentSource::Detected => t!("ide-indent-hint-detected"),
        IndentSource::Configured => t!("ide-indent-hint-configured"),
    }
}

impl IdeView {
    pub(super) fn indent_widget(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        response: &Response,
    ) {
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return };
        let indent = file_indent::of(session, document);
        let language = zen_workbench::code_style::language_of(document.path());
        let shown = describe(indent.effective);
        accessibility::button(response, &shown);
        accessibility::describe(response, &hint_of(&indent));
        let response = response.clone().on_hover_text(hint_of(&indent));
        if std::mem::take(&mut self.indent_ui.open_menu) {
            egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&response));
        }
        let mut choice = None;
        let popup = crate::components::popup_menu(&response, |ui| {
            if let Some(detected) = indent.detected.filter(|_| indent.can_switch) {
                let style = file_indent::with_detected(indent.configured, detected);
                let label = t!("ide-indent-use-detected", indent = describe(style));
                if ui.add(egui::Button::selectable(indent.source == IndentSource::Detected, label)).clicked() {
                    choice = Some(Choice::Detected);
                    ui.close();
                }
                let label = t!("ide-indent-use-configured", indent = describe(indent.configured));
                if ui.add(egui::Button::selectable(indent.source == IndentSource::Configured, label)).clicked() {
                    choice = Some(Choice::Configured);
                    ui.close();
                }
                ui.separator();
            }
            if crate::components::menu_item(ui, t!("ide-indent-configure")) {
                choice = Some(Choice::Settings);
                ui.close();
            }
        });
        self.indent_ui.menu_open = popup.is_some();
        match choice {
            Some(Choice::Detected) => self.dispatch(ui, registry, session, "editor.indent.useDetected", json!({})),
            Some(Choice::Configured) => self.dispatch(ui, registry, session, "editor.indent.useConfigured", json!({})),
            Some(Choice::Settings) => {
                let page = self
                    .settings_sections()
                    .into_iter()
                    .map(|section| section.page)
                    .find(|page| matches!(page, PageId::CodeStyleOf(id) if *id == language))
                    .unwrap_or(PageId::CodeStyleOther);
                self.settings.open_at(page);
            }
            None => {}
        }
    }
}

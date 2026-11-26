use egui::{Align2, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_workbench::notifications::{Action, Group, Kind, NewNotification};

use super::glyphs::glyph_check;
use super::settings::IdeSettings;
use super::{IdePalette, IdeView, Request, Side, hint};
use crate::{accessibility, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StatusWidget {
    LineColumn,
    LanguageServices,
    LineSeparator,
    FileEncoding,
    PowerSave,
    Indentation,
    McpServer,
    ReadOnly,
    MemoryIndicator,
}

impl StatusWidget {
    pub(super) const ALL: [StatusWidget; 9] = [
        StatusWidget::LineColumn,
        StatusWidget::LanguageServices,
        StatusWidget::LineSeparator,
        StatusWidget::FileEncoding,
        StatusWidget::PowerSave,
        StatusWidget::Indentation,
        StatusWidget::McpServer,
        StatusWidget::ReadOnly,
        StatusWidget::MemoryIndicator,
    ];

    pub(super) fn id(self) -> &'static str {
        match self {
            StatusWidget::LineColumn => "lineColumn",
            StatusWidget::LanguageServices => "languageServices",
            StatusWidget::LineSeparator => "lineSeparator",
            StatusWidget::FileEncoding => "fileEncoding",
            StatusWidget::PowerSave => "powerSaveMode",
            StatusWidget::Indentation => "indentation",
            StatusWidget::McpServer => "mcpServer",
            StatusWidget::ReadOnly => "readOnly",
            StatusWidget::MemoryIndicator => "memoryIndicator",
        }
    }

    pub(super) fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|widget| widget.id() == id)
    }

    pub(super) fn on_by_default(self) -> bool {
        !matches!(self, StatusWidget::PowerSave | StatusWidget::MemoryIndicator)
    }

    pub(super) fn title(self) -> String {
        match self {
            StatusWidget::LineColumn => t!("ide-status-widget-line-column"),
            StatusWidget::LanguageServices => t!("ide-status-widget-language-services"),
            StatusWidget::LineSeparator => t!("ide-status-widget-line-separator"),
            StatusWidget::FileEncoding => t!("ide-status-widget-file-encoding"),
            StatusWidget::PowerSave => t!("ide-status-widget-power-save"),
            StatusWidget::Indentation => t!("ide-status-widget-indentation"),
            StatusWidget::McpServer => t!("ide-status-widget-mcp-server"),
            StatusWidget::ReadOnly => t!("ide-status-widget-read-only"),
            StatusWidget::MemoryIndicator => t!("ide-status-widget-memory"),
        }
    }

    pub(super) fn shown_in(self, settings: &IdeSettings) -> bool {
        settings.status_bar_widgets.get(self.id()).copied().unwrap_or_else(|| self.on_by_default())
    }
}

pub(super) fn widgets_state(settings: &IdeSettings) -> Value {
    Value::Object(
        StatusWidget::ALL
            .into_iter()
            .map(|widget| (widget.id().to_owned(), json!(widget.shown_in(settings))))
            .collect(),
    )
}

pub(super) fn megabytes(bytes: u64) -> String {
    ((bytes + (1 << 19)) >> 20).to_string()
}

pub(super) fn power_save_notice() -> NewNotification {
    NewNotification::worded(Group::PowerSave, Kind::Warning, zen_i18n::Message::new("ide-power-save-notice-body"))
        .titled_by(zen_i18n::Message::new("ide-power-save-notice-title"))
        .action(Action::new(
            "hide",
            t!("ide-power-save-notice-hide"),
            "view.notifications.balloonsOff",
            json!({ "group": Group::PowerSave.id() }),
        ))
        .action(Action::new(
            "disable",
            t!("ide-power-save-notice-disable"),
            "view.powerSaveMode.toggle",
            json!({ "on": false }),
        ))
}

pub(super) fn battery_icon(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let body = Rect::from_center_size(rect.center() - vec2(1.0, 0.0), vec2(rect.width() - 3.0, rect.height() * 0.55));
    painter.rect_stroke(body, 2, Stroke::new(1.2, color), egui::StrokeKind::Inside);
    let nub = Rect::from_min_size(pos2(body.right(), body.center().y - 1.5), vec2(1.5, 3.0));
    painter.rect_filled(nub, 0.5, color);
    let charge = Rect::from_min_size(body.min + vec2(2.0, 2.0), vec2((body.width() - 4.0) * 0.35, body.height() - 4.0));
    painter.rect_filled(charge, 0.5, color);
}

impl IdeView {
    pub(super) fn widget_shown(&self, widget: StatusWidget) -> bool {
        widget.shown_in(&self.overview.settings)
    }

    pub(super) fn set_widget_shown(&mut self, widget: StatusWidget, shown: bool) {
        let mut settings = self.overview.settings.clone();
        if shown == widget.on_by_default() {
            settings.status_bar_widgets.remove(widget.id());
        } else {
            settings.status_bar_widgets.insert(widget.id().to_owned(), shown);
        }
        self.overview.settings = settings.clone();
        self.requests.push(Request::Settings(Box::new(settings)));
    }

    pub(super) fn power_save_on(&self) -> bool {
        self.overview.settings.power_save_mode
    }

    pub(super) fn toggle_power_save(&mut self) -> bool {
        let mut settings = self.overview.settings.clone();
        settings.power_save_mode = !settings.power_save_mode;
        let on = settings.power_save_mode;
        self.overview.settings = settings.clone();
        self.requests.push(Request::Settings(Box::new(settings)));
        if on {
            self.notify(power_save_notice());
        }
        on
    }

    pub(super) fn open_status_menu(&mut self, at: Pos2) {
        self.status_menu_at = Some(at);
    }

    pub(super) fn status_menu_open(&self) -> bool {
        self.status_menu_at.is_some()
    }

    pub(super) fn status_bar_context(&mut self, ui: &Ui, rect: Rect) {
        let clicked = ui.input(|input| input.pointer.secondary_clicked());
        if clicked
            && let Some(at) = ui.input(|input| input.pointer.interact_pos())
            && rect.contains(at)
        {
            self.open_status_menu(at);
        }
    }

    pub(super) fn status_menu(&mut self, ui: &Ui) {
        let Some(at) = self.status_menu_at else { return };
        let p = self.palette;
        let mut open = true;
        let titles: Vec<(StatusWidget, String)> =
            StatusWidget::ALL.into_iter().map(|widget| (widget, widget.title())).collect();
        let widest = titles
            .iter()
            .map(|(_, title)| ui.painter().layout_no_wrap(title.clone(), theme::regular(13.0), p.text).size().x)
            .fold(0.0, f32::max);
        let width = (widest + 52.0).max(200.0);
        let mut toggled = None;
        egui::Popup::new(ui.scope_id().with("status-widgets-menu"), ui.ctx().clone(), at, ui.layer_id())
            .kind(egui::PopupKind::Menu)
            .align(egui::RectAlign::TOP_START)
            .layout(egui::Layout::top_down_justified(egui::Align::Min))
            .open_bool(&mut open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.set_width(width);
                for (widget, title) in &titles {
                    let shown = self.widget_shown(*widget);
                    let response = menu_row(ui, &p, width, title, shown);
                    accessibility::name(&response, egui::accesskit::Role::MenuItemCheckBox, title);
                    accessibility::pressed(&response, shown);
                    if response.clicked() {
                        toggled = Some((*widget, !shown));
                    }
                }
            });
        if let Some((widget, shown)) = toggled {
            self.set_widget_shown(widget, shown);
        }
        if !open {
            self.status_menu_at = None;
        }
    }

    pub(super) fn power_save_widget(&mut self, ui: &mut Ui, right: f32, y: f32) -> f32 {
        if !self.power_save_on() || !self.widget_shown(StatusWidget::PowerSave) {
            return right;
        }
        let p = self.palette;
        let rect = Rect::from_center_size(pos2(right - 11.0, y), Vec2::splat(22.0));
        let response = ui.interact(rect, ui.scope_id().with("status-power-save"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        battery_icon(ui.painter(), rect.shrink(4.0), p.text_secondary);
        let text = t!("ide-status-power-save-hint");
        hint(ui, &response, &text, None, Side::Left);
        accessibility::button(&response, &text);
        accessibility::pressed(&response, true);
        if response.clicked() {
            self.toggle_power_save();
        }
        rect.left()
    }

    pub(super) fn memory_widget(&mut self, ui: &mut Ui, right: f32, y: f32) -> f32 {
        if !self.widget_shown(StatusWidget::MemoryIndicator) {
            return right;
        }
        ui.ctx().request_repaint_after(zen_workbench::activity_monitor::MEMORY_INTERVAL);
        let Some(bytes) = zen_workbench::activity_monitor::current_memory() else { return right };
        let p = self.palette;
        let megabytes = megabytes(bytes);
        let label = t!("ide-status-memory", megabytes = megabytes.as_str());
        let galley = ui.painter().layout_no_wrap(label, theme::regular(12.5), p.text_secondary);
        let text = Align2::RIGHT_CENTER.anchor_size(pos2(right - 4.0, y), galley.size());
        let rect = text.expand2(vec2(4.0, 3.0));
        let response = ui.interact(rect, ui.scope_id().with("status-memory"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 4, p.hover);
        }
        let shown = galley.text().to_owned();
        ui.painter().galley(text.min, galley, p.text_secondary);
        let detail = t!("ide-status-memory-hint", megabytes = megabytes.as_str());
        hint(ui, &response, &detail, None, Side::Left);
        accessibility::button(&response, &shown);
        accessibility::describe(&response, &detail);
        if response.clicked() {
            self.open_activity_monitor();
        }
        rect.left()
    }
}

fn menu_row(ui: &mut Ui, p: &IdePalette, width: f32, label: &str, checked: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 5, p.selection);
    }
    if checked {
        glyph_check(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(11.0)),
            p.text,
        );
    }
    ui.painter().text(
        pos2(rect.left() + 30.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(13.0),
        p.text,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widgets_have_ids_and_defaults() {
        let ids: Vec<&str> = StatusWidget::ALL.iter().map(|widget| widget.id()).collect();
        assert_eq!(
            ids,
            [
                "lineColumn",
                "languageServices",
                "lineSeparator",
                "fileEncoding",
                "powerSaveMode",
                "indentation",
                "mcpServer",
                "readOnly",
                "memoryIndicator"
            ]
        );
        for widget in StatusWidget::ALL {
            assert_eq!(StatusWidget::from_id(widget.id()), Some(widget));
        }
        let mut settings = IdeSettings::default();
        assert!(!StatusWidget::MemoryIndicator.shown_in(&settings), "off by default");
        assert!(!StatusWidget::PowerSave.shown_in(&settings), "off by default");
        assert!(StatusWidget::McpServer.shown_in(&settings));
        settings.status_bar_widgets.insert("memoryIndicator".to_owned(), true);
        settings.status_bar_widgets.insert("lineSeparator".to_owned(), false);
        let state = widgets_state(&settings);
        assert_eq!((state["memoryIndicator"].as_bool(), state["lineSeparator"].as_bool()), (Some(true), Some(false)));
        assert_eq!(megabytes(312 << 20), "312");
        assert_eq!(megabytes((1 << 20) - 1), "1");
    }

    #[test]
    fn the_power_save_notice_offers_to_hide_it_and_to_turn_the_mode_off() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let notice = power_save_notice();
        assert_eq!((notice.group, notice.kind, notice.balloon), (Group::PowerSave, Kind::Warning, true));
        assert_eq!(notice.title, "Power saving is on");
        let actions: Vec<(&str, &str)> =
            notice.actions.iter().map(|action| (action.label.as_str(), action.command.as_str())).collect();
        assert_eq!(
            actions,
            [
                ("Do not show again", "view.notifications.balloonsOff"),
                ("Disable power save mode", "view.powerSaveMode.toggle")
            ]
        );
        assert_eq!(notice.actions[1].params, json!({ "on": false }));
    }
}

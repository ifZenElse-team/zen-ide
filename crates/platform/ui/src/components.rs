use egui::{Align2, Color32, Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::accessibility;
use crate::icons;
use crate::theme::{self, Palette};

pub mod metrics {
    pub const BUTTON: f32 = 26.0;
    pub const BUTTON_PADDING: f32 = 28.0;
    pub const BUTTON_RADIUS: f32 = 7.0;
    pub const ROW_RADIUS: u8 = 10;
    pub const TAB_INSET: f32 = 8.0;
    pub const TAB_GAP: f32 = 20.0;
    pub const TAB_FONT: f32 = 14.5;
    pub const TAB_BADGE_RADIUS: f32 = 8.0;
    pub const TEXT_FIELD: f32 = 32.0;
    pub const TEXT_FIELD_RADIUS: u8 = 9;
    pub const TOGGLE_WIDTH: f32 = 30.0;
    pub const TOGGLE_HEIGHT: f32 = 18.0;
}

pub type Icon = fn(&egui::Painter, Rect, Color32);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab {
    pub label: String,
    pub badge: Option<usize>,
}

pub fn primary_button_size(ui: &Ui, label: &str) -> Vec2 {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(13.0), Color32::WHITE);
    vec2(galley.size().x + metrics::BUTTON_PADDING, metrics::BUTTON)
}

pub fn primary_button(ui: &mut Ui, p: &Palette, anchor: Pos2, label: &str, id: Id) -> Response {
    let size = primary_button_size(ui, label);
    let rect =
        Rect::from_min_max(pos2(anchor.x - size.x, anchor.y - size.y / 2.0), pos2(anchor.x, anchor.y + size.y / 2.0));
    let response = ui.interact(rect, id, Sense::click());
    let [top, bottom] = if response.hovered() { p.accent_hover } else { p.accent };
    icons::fill_rounded(ui.painter(), rect, metrics::BUTTON_RADIUS, |at| lerp(top, bottom, at.y));
    let galley = ui.painter().layout_no_wrap(label.to_owned(), theme::semibold(13.0), p.on_accent);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, p.on_accent);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    accessibility::button(&response, label);
    response
}

#[derive(Clone, Copy, Debug)]
pub struct IconButton {
    pub icon: Icon,
    pub icon_size: f32,
    pub color: Color32,
    pub radius: u8,
}

pub fn icon_button(ui: &mut Ui, p: &Palette, rect: Rect, id: Id, button: IconButton) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    hover_fill(ui, &response, p, button.radius);
    (button.icon)(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(button.icon_size)), button.color);
    response
}

pub fn icon_button_named(
    ui: &mut Ui,
    p: &Palette,
    rect: Rect,
    id: Id,
    button: IconButton,
    name: impl Fn() -> String,
) -> Response {
    let response = icon_button(ui, p, rect, id, button);
    accessibility::button_with(&response, name);
    response
}

pub fn hover_fill(ui: &Ui, response: &Response, p: &Palette, radius: u8) {
    if response.hovered() {
        ui.painter().rect_filled(response.rect, radius, p.hover);
    }
}

pub fn list_row(ui: &mut Ui, p: &Palette, height: f32, sense: Sense) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
    if response.hovered() {
        ui.painter().rect_filled(rect, metrics::ROW_RADIUS, p.hover);
    }
    response
}

pub fn list_row_named(ui: &mut Ui, p: &Palette, height: f32, sense: Sense, name: impl Fn() -> String) -> Response {
    let response = list_row(ui, p, height, sense);
    accessibility::name_with(&response, egui::Role::ListItem, name);
    response
}

pub fn tabs(ui: &mut Ui, p: &Palette, rect: Rect, items: &[Tab], active: usize) -> Option<usize> {
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect).accessibility_role(egui::Role::TabList));
    tab_row(&mut row, p, rect, items, active)
}

fn tab_row(ui: &mut Ui, p: &Palette, rect: Rect, items: &[Tab], active: usize) -> Option<usize> {
    let mut clicked = None;
    let mut x = rect.left() + metrics::TAB_INSET;
    for (index, tab) in items.iter().enumerate() {
        let is_active = index == active;
        let font = if is_active { theme::semibold(metrics::TAB_FONT) } else { theme::regular(metrics::TAB_FONT) };
        let color = if is_active { p.text } else { p.text_secondary };
        let text = ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, &tab.label, font, color);
        let mut hit = text.expand2(vec2(6.0, 10.0));
        if let Some(count) = tab.badge.filter(|count| *count > 0) {
            let center = text.right_center() + vec2(12.0, -1.0);
            ui.painter().circle_filled(center, metrics::TAB_BADGE_RADIUS, p.accent[1]);
            ui.painter().text(center, Align2::CENTER_CENTER, count.to_string(), theme::bold(10.0), p.on_accent);
            hit = hit.union(Rect::from_center_size(center, Vec2::splat(2.0 * metrics::TAB_BADGE_RADIUS)));
        }
        if is_active {
            let underline =
                Rect::from_min_max(pos2(text.left(), rect.bottom() - 4.0), pos2(text.right(), rect.bottom() - 2.0));
            ui.painter().rect_filled(underline, 1, p.link);
        }
        let response = ui.interact(hit, ui.scope_id().with(("tab", index)), Sense::click());
        if response.hovered() && !is_active {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        accessibility::item(&response, egui::Role::Tab, is_active, &tab.label);
        if let Some(count) = tab.badge.filter(|count| *count > 0) {
            accessibility::edit(&response, |node| node.set_description(count.to_string()));
        }
        if response.clicked() {
            clicked = Some(index);
        }
        x = hit.right() + metrics::TAB_GAP;
    }
    clicked
}

pub fn popup_menu<R>(response: &Response, add_contents: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    egui::Popup::menu(response).show(add_contents).map(|inner| inner.inner)
}

pub fn menu_item(ui: &mut Ui, label: impl Into<egui::WidgetText>) -> bool {
    ui.button(label).clicked()
}

pub fn menu_item_disabled(ui: &mut Ui, label: impl Into<egui::WidgetText>, hint: Option<&str>) {
    let response = ui.add_enabled(false, egui::Button::new(label));
    if let Some(hint) = hint {
        let _ = response.on_disabled_hover_text(hint);
    }
}

pub fn toggle(ui: &mut Ui, p: &Palette, on: &mut bool, label: &str) -> Response {
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(metrics::TOGGLE_WIDTH, metrics::TOGGLE_HEIGHT), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(response.id, *on);
    let radius = rect.height() / 2.0;
    let off_track = if p.dark { p.text_muted } else { p.text_secondary };
    let track = lerp(off_track, p.accent[1], t);
    let track = if response.hovered() { lerp(track, p.text, 0.08) } else { track };
    ui.painter().rect_filled(rect, radius, track);
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    ui.painter().circle_filled(pos2(knob_x, rect.center().y), radius - 2.5, p.on_accent);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Switch, ui.is_enabled(), *on, label));
    response
}

pub fn text_field(ui: &mut Ui, p: &Palette, rect: Rect, text: &mut String, hint: &str, icon: Option<Icon>) -> Response {
    ui.painter().rect_filled(rect, metrics::TEXT_FIELD_RADIUS, p.panel);
    ui.painter().rect_stroke(rect, metrics::TEXT_FIELD_RADIUS, Stroke::new(1.0, p.panel_edge), StrokeKind::Inside);
    let mut left = rect.left() + 10.0;
    if let Some(icon) = icon {
        let mark = Rect::from_center_size(pos2(rect.left() + 17.0, rect.center().y), Vec2::splat(14.0));
        icon(ui.painter(), mark, p.text_muted);
        left = rect.left() + 32.0;
    }
    let field = Rect::from_min_max(pos2(left, rect.top() + 5.0), pos2(rect.right() - 8.0, rect.bottom() - 5.0));
    let edit = egui::TextEdit::singleline(text)
        .frame(egui::Frame::NONE)
        .hint_text(hint)
        .font(theme::regular(13.5))
        .text_color(p.text);
    let response = ui.put(field, edit);
    accessibility::labelled(&response, hint);
    response
}

pub fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
        channel(a.a(), b.a()),
    )
}

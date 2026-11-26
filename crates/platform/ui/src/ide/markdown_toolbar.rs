use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_workbench::markdown::format::ListKind;

use super::{IdePalette, Side, hint};
use crate::theme;

const HEIGHT: f32 = 30.0;
const BUTTON: f32 = 24.0;
const GAP: f32 = 6.0;
const STYLE_WIDTH: f32 = 96.0;

pub(super) const HEADING: &str = "markdown.format.heading";
const LIST: &str = "markdown.format.list";

type Asked = (&'static str, Value);

enum Face {
    Letter(&'static str, Style),
    List(ListKind),
    Link,
    Code,
}

#[derive(Clone, Copy)]
enum Style {
    Bold,
    Italic,
    Strike,
}

const STYLES: [(&str, Face, &str); 3] = [
    ("markdown.format.bold", Face::Letter("B", Style::Bold), "ide-markdown-toolbar-bold"),
    ("markdown.format.italic", Face::Letter("I", Style::Italic), "ide-markdown-toolbar-italic"),
    ("markdown.format.strikethrough", Face::Letter("S", Style::Strike), "ide-markdown-toolbar-strikethrough"),
];

const LISTS: [(ListKind, &str); 3] = [
    (ListKind::Bullet, "ide-markdown-toolbar-bullet-list"),
    (ListKind::Numbered, "ide-markdown-toolbar-numbered-list"),
    (ListKind::Check, "ide-markdown-toolbar-check-list"),
];

fn level_name(level: u8) -> String {
    if level == 0 { t!("ide-markdown-toolbar-text") } else { format!("H{level}") }
}

fn width() -> f32 {
    let buttons = (STYLES.len() + 2 + LISTS.len()) as f32;
    STYLE_WIDTH + buttons * (BUTTON + 2.0) + 2.0 * GAP + 16.0
}

pub(super) fn place(selection: Rect, area: Rect) -> egui::Pos2 {
    let x = selection.left().clamp(area.left() + 4.0, (area.right() - width() - 4.0).max(area.left() + 4.0));
    let above = selection.top() - HEIGHT - 6.0;
    let y = if above >= area.top() { above } else { selection.bottom() + 6.0 };
    pos2(x, y)
}

fn button(ui: &mut Ui, p: &IdePalette, name: &str, face: &Face) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let painter = ui.painter();
    let colour = p.text;
    match face {
        Face::Letter(letter, style) => {
            let font = match style {
                Style::Bold => theme::bold(14.0),
                Style::Italic | Style::Strike => theme::regular(14.0),
            };
            let mut format = TextFormat::simple(font, colour);
            format.italics = matches!(style, Style::Italic);
            if matches!(style, Style::Strike) {
                format.strikethrough = Stroke::new(1.2, colour);
            }
            let shown = painter.layout_job(LayoutJob::single_section((*letter).to_owned(), format));
            painter.galley(rect.center() - shown.size() / 2.0, shown, colour);
        }
        Face::Code => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "</>",
                FontId::new(11.5, egui::FontFamily::Monospace),
                colour,
            );
        }
        Face::Link => glyph_link(painter, rect.shrink(6.0), colour),
        Face::List(kind) => glyph_list(painter, rect.shrink(6.0), *kind, colour),
    }
    hint(ui, &response, name, None, Side::Below);
    response
}

fn glyph_link(painter: &egui::Painter, rect: Rect, colour: Color32) {
    let s = Stroke::new(1.3, colour);
    let height = rect.height() * 0.55;
    let width = rect.width() * 0.6;
    let left = Rect::from_center_size(pos2(rect.left() + width / 2.0, rect.center().y), vec2(width, height));
    let right = Rect::from_center_size(pos2(rect.right() - width / 2.0, rect.center().y), vec2(width, height));
    painter.rect_stroke(left, CornerRadius::same(4), s, egui::StrokeKind::Inside);
    painter.rect_stroke(right, CornerRadius::same(4), s, egui::StrokeKind::Inside);
}

fn glyph_list(painter: &egui::Painter, rect: Rect, kind: ListKind, colour: Color32) {
    let s = Stroke::new(1.3, colour);
    for row in 0..3 {
        let y = rect.top() + rect.height() * (0.15 + row as f32 * 0.35);
        let marker = pos2(rect.left() + 1.5, y);
        match kind {
            ListKind::Bullet => {
                painter.circle_filled(marker, 1.3, colour);
            }
            ListKind::Numbered => {
                painter.line_segment([marker + vec2(0.0, -2.0), marker + vec2(0.0, 2.0)], s);
            }
            ListKind::Check => {
                let tick = Rect::from_center_size(marker, Vec2::splat(3.0));
                painter.rect_stroke(tick, CornerRadius::ZERO, Stroke::new(1.0, colour), egui::StrokeKind::Outside);
            }
        }
        painter.line_segment([pos2(rect.left() + 6.0, y), pos2(rect.right(), y)], s);
    }
}

fn separator(ui: &mut Ui, p: &IdePalette) {
    let (rect, _) = ui.allocate_exact_size(vec2(GAP, HEIGHT - 12.0), Sense::hover());
    ui.painter().vline(rect.center().x, rect.y_range(), Stroke::new(1.0, p.tab_border));
}

pub(super) fn show(ui: &Ui, p: &IdePalette, at: egui::Pos2, level: u8) -> (Option<Asked>, Rect) {
    let mut asked = None;
    let area = egui::Area::new(ui.make_persistent_id("markdown-floating-toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(at)
        .show(ui.ctx(), |ui| {
            let frame = egui::Frame::new()
                .fill(p.popup)
                .stroke(Stroke::new(1.0, p.popup_border))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::symmetric(4, 2));
            frame.show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.horizontal_centered(|ui| {
                    ui.set_height(HEIGHT - 4.0);
                    let combo = egui::ComboBox::from_id_salt("markdown-toolbar-style")
                        .selected_text(level_name(level))
                        .width(STYLE_WIDTH - 8.0)
                        .show_ui(ui, |ui| {
                            for choice in 0..=6 {
                                if ui.selectable_label(choice == level, level_name(choice)).clicked() {
                                    asked = Some((HEADING, json!({ "level": choice })));
                                }
                            }
                        });
                    crate::accessibility::name(
                        &combo.response,
                        egui::Role::ComboBox,
                        &t!("ide-markdown-toolbar-style"),
                    );
                    separator(ui, p);
                    for (command, face, key) in &STYLES {
                        if button(ui, p, &t!(*key), face).clicked() {
                            asked = Some((*command, json!({})));
                        }
                    }
                    if button(ui, p, &t!("ide-markdown-toolbar-code"), &Face::Code).clicked() {
                        asked = Some(("markdown.format.code", json!({})));
                    }
                    if button(ui, p, &t!("ide-markdown-toolbar-link"), &Face::Link).clicked() {
                        asked = Some(("markdown.format.link", json!({})));
                    }
                    separator(ui, p);
                    for (kind, key) in LISTS {
                        if button(ui, p, &t!(key), &Face::List(kind)).clicked() {
                            asked = Some((LIST, json!({ "kind": kind })));
                        }
                    }
                });
            });
        });
    (asked, area.response.rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toolbar_sits_above_the_selection_and_below_it_at_the_top() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
        let middle = Rect::from_min_size(pos2(100.0, 200.0), vec2(50.0, 18.0));
        assert_eq!(place(middle, area), pos2(100.0, 200.0 - HEIGHT - 6.0));
        let top = Rect::from_min_size(pos2(100.0, 4.0), vec2(50.0, 18.0));
        assert_eq!(place(top, area).y, 28.0);
        let right = Rect::from_min_size(pos2(790.0, 200.0), vec2(5.0, 18.0));
        assert!(place(right, area).x + width() <= 800.0, "stays in the editor");
    }
}

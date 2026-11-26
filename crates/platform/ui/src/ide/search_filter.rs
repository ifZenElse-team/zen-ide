use egui::{Align2, Color32, Rect, Sense, Ui, Vec2, pos2, vec2};
use zen_editor::SearchContext;
use zen_i18n::t;

use super::glyphs::{glyph_check, stroke};
use super::{IdePalette, Side, hint};
use crate::{accessibility, theme};

pub(super) fn message(context: SearchContext) -> &'static str {
    match context {
        SearchContext::Anywhere => "ide-search-filter-anywhere",
        SearchContext::InComments => "ide-search-filter-in-comments",
        SearchContext::InStringLiterals => "ide-search-filter-in-strings",
        SearchContext::ExceptComments => "ide-search-filter-except-comments",
        SearchContext::ExceptStringLiterals => "ide-search-filter-except-strings",
        SearchContext::ExceptCommentsAndStringLiterals => "ide-search-filter-except-both",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FilterChoice {
    InSelection,
    Context(SearchContext),
}

pub(super) fn filter_button(
    ui: &mut Ui,
    rect: Rect,
    palette: &IdePalette,
    id: &str,
    current: SearchContext,
) -> Option<SearchContext> {
    match filter_menu(ui, rect, palette, id, current, None) {
        Some(FilterChoice::Context(context)) => Some(context),
        _ => None,
    }
}

pub(super) fn filter_menu(
    ui: &mut Ui,
    rect: Rect,
    palette: &IdePalette,
    id: &str,
    current: SearchContext,
    in_selection: Option<bool>,
) -> Option<FilterChoice> {
    let response = ui.interact(rect, ui.scope_id().with(id), Sense::click());
    let on = current != SearchContext::Anywhere || in_selection == Some(true);
    if on {
        ui.painter().rect_filled(rect, 4, palette.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4, palette.hover);
    }
    glyph_funnel(ui.painter(), rect.shrink(5.0), if on { palette.text } else { palette.text_secondary });
    hint(ui, &response, &t!("ide-search-filter"), Some(&t!(message(current))), Side::Below);
    let mut chosen = None;
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(260.0);
        if let Some(in_selection) = in_selection {
            let label = t!("keymap-editor-find-searchInSelection");
            let keys = crate::keymap::shortcut("editor.find.searchInSelection").unwrap_or_default();
            let row = menu_row_with(ui, palette, &label, in_selection, &keys);
            accessibility::item(&row, egui::Role::MenuItemCheckBox, in_selection, &label);
            if row.clicked() {
                chosen = Some(FilterChoice::InSelection);
                ui.close();
            }
            ui.separator();
            menu_heading(ui, palette, &t!("ide-search-filter-scope"));
        }
        for context in SearchContext::ALL {
            let label = t!(message(context));
            let row = menu_row(ui, palette, &label, context == current);
            accessibility::item(&row, egui::Role::MenuItemRadio, context == current, &label);
            if row.clicked() {
                chosen = Some(FilterChoice::Context(context));
                ui.close();
            }
        }
    });
    chosen
}

pub(super) fn menu_heading(ui: &mut Ui, palette: &IdePalette, label: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().max(260.0), 22.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(12.0),
        palette.text_secondary,
    );
}

fn menu_row(ui: &mut Ui, palette: &IdePalette, label: &str, checked: bool) -> egui::Response {
    menu_row_with(ui, palette, label, checked, "")
}

pub(super) fn menu_row_with(
    ui: &mut Ui,
    palette: &IdePalette,
    label: &str,
    checked: bool,
    keys: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().max(260.0), 26.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 5, palette.selection);
    }
    if checked {
        let tick = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(11.0));
        glyph_check(ui.painter(), tick, palette.text);
    }
    ui.painter().text(
        pos2(rect.left() + 30.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(13.0),
        palette.text,
    );
    if !keys.is_empty() {
        ui.painter().text(
            pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            keys,
            theme::regular(12.5),
            palette.text_secondary,
        );
    }
    response
}

pub(super) fn glyph_funnel(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (left, right, top) = (rect.left() + rect.width() * 0.1, rect.right() - rect.width() * 0.1, rect.top() + 2.0);
    let neck = rect.top() + rect.height() * 0.5;
    let (neck_left, neck_right) = (rect.center().x - rect.width() * 0.1, rect.center().x + rect.width() * 0.1);
    painter.line_segment([pos2(left, top), pos2(right, top)], s);
    painter.line_segment([pos2(left, top), pos2(neck_left, neck)], s);
    painter.line_segment([pos2(right, top), pos2(neck_right, neck)], s);
    painter.line_segment([pos2(neck_left, neck), pos2(neck_left, rect.bottom() - 2.0)], s);
    painter.line_segment([pos2(neck_right, neck), pos2(neck_right, rect.bottom() - 4.0)], s);
    painter.line_segment([pos2(neck_left, rect.bottom() - 2.0), pos2(neck_right, rect.bottom() - 4.0)], s);
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers, PointerButton};

    use super::*;

    fn frame(
        ctx: &egui::Context,
        events: Vec<Event>,
        current: SearchContext,
    ) -> (Option<SearchContext>, Vec<(String, egui::Pos2)>, Vec<Rect>) {
        let input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
            ..Default::default()
        };
        let mut chosen = None;
        let mut output = ctx.run_ui(input, |ui| {
            let rect = Rect::from_min_size(pos2(10.0, 10.0), Vec2::splat(24.0));
            chosen = filter_button(ui, rect, &IdePalette::zen_dark(), "filter", current);
        });
        output.textures_delta.clear();
        let texts = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.pos)),
                _ => None,
            })
            .collect();
        let lines = output
            .shapes
            .iter()
            .filter(|clipped| matches!(clipped.shape, egui::Shape::Path(_)))
            .map(|clipped| clipped.shape.visual_bounding_rect())
            .collect();
        (chosen, texts, lines)
    }

    fn click(ctx: &egui::Context, at: egui::Pos2, current: SearchContext) -> Option<SearchContext> {
        let press = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let (first, ..) = frame(ctx, vec![Event::PointerMoved(at), press(true)], current);
        let (second, ..) = frame(ctx, vec![press(false)], current);
        first.or(second)
    }

    #[test]
    fn the_menu_lists_the_six_filters_in_order_and_a_click_chooses_one() {
        let ctx = egui::Context::default();
        let current = SearchContext::ExceptComments;
        frame(&ctx, Vec::new(), current);
        assert_eq!(click(&ctx, pos2(22.0, 22.0), current), None, "the button opens the menu");
        let (_, texts, lines) = frame(&ctx, vec![Event::PointerMoved(pos2(700.0, 500.0))], current);
        let names: Vec<String> = SearchContext::ALL.into_iter().map(|context| t!(message(context))).collect();
        let shown: Vec<&(String, egui::Pos2)> = texts.iter().filter(|(text, _)| names.contains(text)).collect();
        assert_eq!(shown.iter().map(|(text, _)| text).collect::<Vec<_>>(), names.iter().collect::<Vec<_>>());
        assert!(shown.windows(2).all(|pair| pair[0].1.y < pair[1].1.y), "one under another, in menu order");
        let ticked: Vec<&String> = shown
            .iter()
            .filter(|(_, at)| {
                lines
                    .iter()
                    .any(|line| line.right() < at.x && line.center().y > at.y - 6.0 && line.center().y < at.y + 18.0)
            })
            .map(|(text, _)| text)
            .collect();
        assert_eq!(ticked, [&t!(message(current))], "only the current filter is ticked");
        let in_comments = shown[1].1 + vec2(4.0, 4.0);
        assert_eq!(click(&ctx, in_comments, current), Some(SearchContext::InComments));
    }

    #[test]
    fn every_filter_has_its_own_message_in_menu_order() {
        let messages: Vec<&str> = SearchContext::ALL.into_iter().map(message).collect();
        assert_eq!(
            messages,
            [
                "ide-search-filter-anywhere",
                "ide-search-filter-in-comments",
                "ide-search-filter-in-strings",
                "ide-search-filter-except-comments",
                "ide-search-filter-except-strings",
                "ide-search-filter-except-both",
            ]
        );
        let english = include_str!("../../locales/en/ui.ftl");
        let russian = include_str!("../../locales/ru/ui.ftl");
        for name in messages {
            let line = format!("\n{name} = ");
            assert!(english.contains(&line) && russian.contains(&line), "{name} is a message in en and ru");
        }
    }
}

use egui::{Align2, Color32, FontId, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_vcs::filter::TextMatcher;
use zen_workbench::vcs::graph_search::{FocusKind, FocusTarget, GraphFocus, SearchState};
use zen_workbench::vcs::workspace::{RefKind, RefTarget};

use super::super::find_bar::{glyph_arrow_down, glyph_arrow_up, glyph_close};
use super::super::glyphs::{glyph_eye, glyph_eye_off, glyph_search, glyph_solo};
use super::super::{IdePalette, IdeView, Side, hint};
use super::{Ask, GitWorkspaceView, cell};
use crate::theme;

pub(super) const BAR: f32 = 36.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Found {
    No,
    Match,
    Current,
}

pub(super) struct Matcher {
    text: String,
    regex: bool,
    match_case: bool,
    matcher: Option<TextMatcher>,
}

impl GitWorkspaceView {
    pub(super) fn matcher(&mut self, search: &SearchState) -> Option<TextMatcher> {
        if !search.is_open() {
            return None;
        }
        let same = self.search_matcher.as_ref().is_some_and(|known| {
            known.text == search.text() && known.regex == search.regex() && known.match_case == search.match_case()
        });
        if !same {
            self.search_matcher = Some(Matcher {
                text: search.text().to_owned(),
                regex: search.regex(),
                match_case: search.match_case(),
                matcher: TextMatcher::new(search.text(), search.regex(), search.match_case()).ok().flatten(),
            });
        }
        self.search_matcher.as_ref().and_then(|known| known.matcher.clone())
    }
}

pub(super) fn row_tint(p: &IdePalette) -> Color32 {
    p.search.gamma_multiply(0.4)
}

pub(super) fn found_cell(
    painter: &egui::Painter,
    range: (f32, f32),
    middle: f32,
    text: &str,
    (font, color): (FontId, Color32),
    found: Option<std::ops::Range<usize>>,
    highlight: Color32,
) -> bool {
    let found = found.filter(|found| {
        found.start < found.end && text.is_char_boundary(found.start) && text.is_char_boundary(found.end)
    });
    let Some(found) = found else { return cell(painter, range, middle, text, font, color) };
    let width = (range.1 - range.0 - 18.0).max(0.0);
    let mut job = egui::text::LayoutJob::default();
    let format = |background| egui::TextFormat { font_id: font.clone(), color, background, ..Default::default() };
    job.append(&text[..found.start], 0.0, format(Color32::TRANSPARENT));
    job.append(&text[found.clone()], 0.0, format(highlight));
    job.append(&text[found.end..], 0.0, format(Color32::TRANSPARENT));
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    let galley = painter.layout_job(job);
    let elided = galley.elided;
    painter.galley(pos2(range.0 + 10.0, middle - galley.size().y / 2.0), galley, color);
    elided
}

impl IdeView {
    pub(super) fn workspace_search_bar(
        &mut self,
        ui: &mut Ui,
        bar: Rect,
        p: &IdePalette,
        search: &SearchState,
        asks: &mut Vec<Ask>,
    ) {
        let view = &mut self.git_workspace;
        ui.painter().rect_filled(bar, 0, p.frame);
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
        glyph_search(
            ui.painter(),
            Rect::from_center_size(pos2(bar.left() + 18.0, bar.center().y), Vec2::splat(14.0)),
            p.text_secondary,
        );
        let id = ui.scope_id().with("gw-search-field");
        if ui.memory(|memory| memory.has_focus(id)) {
            let (down, up) = ui.input_mut(|input| {
                (
                    input.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                    input.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                )
            });
            if down {
                asks.push(Ask::Search(json!({ "step": "next" })));
            }
            if up {
                asks.push(Ask::Search(json!({ "step": "previous" })));
            }
        }
        if view.search_text != search.text() && !ui.memory(|memory| memory.has_focus(id)) {
            view.search_text = search.text().to_owned();
        }
        let field_rect =
            Rect::from_min_max(pos2(bar.left() + 34.0, bar.top() + 6.0), pos2(bar.right() - 196.0, bar.bottom() - 6.0));
        let field = ui.put(
            field_rect,
            egui::TextEdit::singleline(&mut view.search_text)
                .id(id)
                .frame(egui::Frame::NONE)
                .align(Align2::LEFT_CENTER)
                .font(theme::regular(13.0))
                .hint_text(t!("vcs-workspace-search-hint"))
                .desired_width(field_rect.width() - 64.0),
        );
        if std::mem::take(&mut view.search_focus) {
            field.request_focus();
        }
        if field.changed() {
            asks.push(Ask::Search(json!({ "text": view.search_text })));
        }
        if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
            let step = if ui.input(|input| input.modifiers.shift) { "previous" } else { "next" };
            asks.push(Ask::Search(json!({ "step": step })));
            field.request_focus();
        }
        if (field.has_focus() || field.lost_focus()) && ui.input(|input| input.key_pressed(Key::Escape)) {
            asks.push(Ask::Search(json!({ "close": true })));
        }
        let toggles = [
            ("Cc", "ide-find-match-case", search.match_case(), "matchCase"),
            (".*", "ide-find-regex", search.regex(), "regex"),
        ];
        for (index, (label, name, on, param)) in toggles.into_iter().enumerate() {
            let rect = Rect::from_center_size(
                pos2(field_rect.right() - 48.0 + index as f32 * 26.0, bar.center().y),
                vec2(24.0, 22.0),
            );
            let response = ui.interact(rect, ui.scope_id().with(("gw-search-toggle", index)), Sense::click());
            if on {
                ui.painter().rect_filled(rect, 4, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                theme::semibold(11.5),
                if on { p.text } else { p.text_secondary },
            );
            hint(ui, &response, &t!(name), None, Side::Below);
            if response.clicked() {
                asks.push(Ask::Search(json!({ param: !on })));
            }
        }
        let count = search.matches().len();
        let status = match (search.error(), count, search.current()) {
            (Some(_), _, _) => t!("ide-find-bad-regex"),
            (None, 0, _) if !search.text().trim().is_empty() => t!("ide-find-no-matches"),
            (None, 0, _) => String::new(),
            (None, count, Some(current)) => t!("ide-find-count", current = current + 1, count = count),
            (None, count, None) => t!("ide-find-results", count = count),
        };
        let bad = search.error().is_some() || (count == 0 && !search.text().trim().is_empty());
        ui.painter()
            .with_clip_rect(Rect::from_x_y_ranges(field_rect.right() + 8.0..=bar.right() - 92.0, bar.y_range()))
            .text(
                pos2(field_rect.right() + 14.0, bar.center().y),
                Align2::LEFT_CENTER,
                status,
                theme::regular(12.5),
                if bad { p.excluded } else { p.text_secondary },
            );
        let buttons: [(super::super::glyphs::Glyph, &str, serde_json::Value); 3] = [
            (glyph_arrow_up, "ide-find-previous", json!({ "step": "previous" })),
            (glyph_arrow_down, "ide-find-next", json!({ "step": "next" })),
            (glyph_close, "ide-find-close", json!({ "close": true })),
        ];
        for (index, (glyph, name, params)) in buttons.into_iter().enumerate() {
            let x = if index == 2 { bar.right() - 20.0 } else { bar.right() - 80.0 + index as f32 * 28.0 };
            let rect = Rect::from_center_size(pos2(x, bar.center().y), Vec2::splat(24.0));
            let response = ui.interact(rect, ui.scope_id().with(("gw-search-button", index)), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            glyph(ui.painter(), rect.shrink(6.0), p.text_secondary);
            hint(ui, &response, &t!(name), None, Side::Below);
            if response.clicked() {
                asks.push(Ask::Search(params));
            }
        }
    }
}

pub(super) fn filter_field(
    ui: &mut Ui,
    p: &IdePalette,
    view: &mut GitWorkspaceView,
    filter: &str,
    asks: &mut Vec<Ask>,
) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
    let frame =
        Rect::from_min_max(pos2(row.left() + 10.0, row.top() + 4.0), pos2(row.right() - 10.0, row.bottom() - 6.0));
    let id = ui.scope_id().with("gw-filter-field");
    if ui.input_mut(|input| input.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::ALT, Key::F)) {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    let focused = ui.memory(|memory| memory.has_focus(id));
    if view.filter_text != filter && !focused {
        view.filter_text = filter.to_owned();
    }
    ui.painter().rect_filled(frame, 6, p.text_field);
    ui.painter().rect_stroke(
        frame,
        6,
        Stroke::new(if focused { 1.5 } else { 1.0 }, if focused { p.accent } else { p.text_field_border }),
        egui::StrokeKind::Inside,
    );
    let clear = !view.filter_text.is_empty();
    if !clear {
        glyph_search(
            ui.painter(),
            Rect::from_center_size(pos2(frame.right() - 15.0, frame.center().y), Vec2::splat(12.0)),
            p.text_secondary,
        );
    }
    let field_rect = Rect::from_min_max(
        pos2(frame.left() + 10.0, frame.top() + 2.0),
        pos2(frame.right() - 26.0, frame.bottom() - 2.0),
    );
    let shortcut = zen_core::keymap::Keystroke::parse("⌥⌘F").map(|keys| keys.to_string()).unwrap_or_default();
    let field = ui.put(
        field_rect,
        egui::TextEdit::singleline(&mut view.filter_text)
            .id(id)
            .frame(egui::Frame::NONE)
            .align(Align2::LEFT_CENTER)
            .font(theme::regular(13.0))
            .hint_text(t!("vcs-workspace-filter-hint", shortcut = shortcut.as_str()))
            .desired_width(field_rect.width()),
    );
    let mut emptied =
        (field.has_focus() || field.lost_focus()) && clear && ui.input(|input| input.key_pressed(Key::Escape));
    if clear {
        let button = Rect::from_center_size(pos2(frame.right() - 14.0, frame.center().y), Vec2::splat(18.0));
        let response = ui.interact(button, id.with("clear"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(button, 4, p.hover);
        }
        glyph_close(ui.painter(), button.shrink(5.0), p.text_secondary);
        hint(ui, &response, &t!("vcs-workspace-filter-clear"), None, Side::Below);
        emptied |= response.clicked();
    }
    if emptied {
        view.filter_text.clear();
        asks.push(Ask::Filter(String::new()));
    } else if field.changed() {
        asks.push(Ask::Filter(view.filter_text.clone()));
    }
}

pub(super) fn focus_summary(ui: &mut Ui, p: &IdePalette, focus: &GraphFocus, asks: &mut Vec<Ask>) {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let link = ui.painter().layout_no_wrap(t!("vcs-workspace-show-all"), theme::regular(12.0), p.link);
    let button = Rect::from_min_size(
        pos2(row.right() - 14.0 - link.size().x - 8.0, row.center().y - 10.0),
        vec2(link.size().x + 8.0, 20.0),
    );
    let summary = t!("vcs-workspace-focus-summary", hidden = focus.hidden.len(), solo = focus.solo.len());
    ui.painter().with_clip_rect(Rect::from_x_y_ranges(row.left()..=button.left() - 4.0, row.y_range())).text(
        pos2(row.left() + 16.0, row.center().y),
        Align2::LEFT_CENTER,
        summary,
        theme::regular(12.0),
        p.text_secondary,
    );
    let response = ui.interact(button, ui.scope_id().with("gw-show-all"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(button, 4, p.hover);
    }
    ui.painter().galley(button.center() - link.size() / 2.0, link, p.link);
    hint(ui, &response, &t!("vcs-workspace-show-all-hint"), None, Side::Below);
    if response.clicked() {
        asks.push(Ask::Focus("vcs.graph.hide", json!({ "showAll": true })));
        asks.push(Ask::Focus("vcs.graph.solo", json!({ "clear": true })));
    }
}

pub(super) fn out_text(p: &IdePalette) -> Color32 {
    p.text_secondary.gamma_multiply(0.7)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FocusButton {
    Hide,
    Solo,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RowFocus {
    pub(super) hidden: bool,
    pub(super) soloed: bool,
    out: bool,
}

impl RowFocus {
    pub(super) fn of(focus: &GraphFocus, name: &str, kind: RefKind) -> Self {
        let kind_of = if kind == RefKind::Remote { FocusKind::Remote } else { FocusKind::Local };
        let target = FocusTarget { name: name.to_owned(), kind: kind_of };
        RowFocus {
            hidden: focus.hidden.contains(&target),
            soloed: focus.solo.contains(&target),
            out: !focus.tip(name, kind),
        }
    }

    pub(super) fn of_remote(focus: &GraphFocus, name: &str) -> Self {
        let target = FocusTarget { name: name.to_owned(), kind: FocusKind::WholeRemote };
        let (hidden, soloed) = (focus.hidden.contains(&target), focus.solo.contains(&target));
        RowFocus { hidden, soloed, out: hidden || (!focus.solo.is_empty() && !soloed) }
    }

    pub(super) fn out(self) -> bool {
        self.out
    }
}

pub(super) fn focus_buttons(
    ui: &mut Ui,
    p: &IdePalette,
    inner: Rect,
    focus: RowFocus,
    hovered: bool,
    id: egui::Id,
) -> (f32, Option<FocusButton>) {
    let mut right = inner.right() - 4.0;
    let mut clicked = None;
    for button in [FocusButton::Hide, FocusButton::Solo] {
        let on = match button {
            FocusButton::Hide => focus.hidden,
            FocusButton::Solo => focus.soloed,
        };
        if !(hovered || on) {
            continue;
        }
        let rect = Rect::from_center_size(pos2(right - 10.0, inner.center().y), Vec2::splat(20.0));
        right = rect.left() - 2.0;
        let response = ui.interact(rect, id.with(button == FocusButton::Solo), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 4, p.selection);
        }
        let (glyph, color, name): (super::super::glyphs::Glyph, Color32, &str) = match (button, on) {
            (FocusButton::Hide, false) => (glyph_eye, p.text_secondary, "vcs-workspace-hide-in-graph"),
            (FocusButton::Hide, true) => (glyph_eye_off, p.text_secondary, "vcs-workspace-show-in-graph"),
            (FocusButton::Solo, false) => (glyph_solo, p.text_secondary, "vcs-workspace-solo"),
            (FocusButton::Solo, true) => (glyph_solo, p.accent, "vcs-workspace-unsolo"),
        };
        glyph(ui.painter(), rect.shrink(4.0), color);
        hint(ui, &response, &t!(name), None, Side::Below);
        if response.clicked() {
            clicked = Some(button);
        }
    }
    (right - 4.0, clicked)
}

pub(super) fn remote_row(ui: &mut Ui, p: &IdePalette, name: &str, focus: RowFocus) -> Option<FocusButton> {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let inner = row.shrink2(vec2(6.0, 0.0));
    let hovered = ui.rect_contains_pointer(row);
    if hovered {
        ui.painter().rect_filled(inner, 5, p.hover);
    }
    super::super::glyphs::glyph_database(
        ui.painter(),
        Rect::from_center_size(pos2(row.left() + 26.0, row.center().y), Vec2::splat(12.0)),
        p.text_secondary,
    );
    let (right, button) = focus_buttons(ui, p, inner, focus, hovered, ui.scope_id().with(("gw-remote-focus", name)));
    ui.painter().with_clip_rect(Rect::from_min_max(inner.min, pos2(right, inner.bottom()))).text(
        pos2(row.left() + 38.0, row.center().y),
        Align2::LEFT_CENTER,
        name,
        theme::regular(13.0),
        if focus.out() { out_text(p) } else { p.text },
    );
    button
}

pub(super) fn focus_ask(button: FocusButton, name: &str, kind: FocusKind, focus: RowFocus) -> Ask {
    match button {
        FocusButton::Hide => {
            Ask::Focus("vcs.graph.hide", json!({ "name": name, "kind": kind, "hidden": !focus.hidden }))
        }
        FocusButton::Solo => Ask::Focus("vcs.graph.solo", json!({ "name": name, "kind": kind, "solo": !focus.soloed })),
    }
}

pub(super) fn focus_items(ui: &mut Ui, target: &RefTarget, focus: &GraphFocus, asks: &mut Vec<Ask>) {
    let kind = match target.kind {
        RefKind::Local => FocusKind::Local,
        RefKind::Remote => FocusKind::Remote,
        RefKind::Tag | RefKind::Head => return,
    };
    let row = RowFocus::of(focus, &target.name, target.kind);
    ui.separator();
    let hide = if row.hidden { t!("vcs-workspace-show-in-graph") } else { t!("vcs-workspace-hide-in-graph") };
    if ui.button(hide).clicked() {
        asks.push(focus_ask(FocusButton::Hide, &target.name, kind, row));
        ui.close();
    }
    let solo = if row.soloed { t!("vcs-workspace-unsolo") } else { t!("vcs-workspace-solo") };
    if ui.button(solo).clicked() {
        asks.push(focus_ask(FocusButton::Solo, &target.name, kind, row));
        ui.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_knows_what_hide_and_solo_hold() {
        let target = |name: &str, kind| FocusTarget { name: name.into(), kind };
        let mut focus = GraphFocus { hidden: vec![target("origin", FocusKind::WholeRemote)], solo: Vec::new() };
        let row = RowFocus::of(&focus, "origin/main", RefKind::Remote);
        assert_eq!((row.hidden, row.soloed, row.out()), (false, false, true), "out with its remote");
        assert!(RowFocus::of_remote(&focus, "origin").hidden);
        assert!(!RowFocus::of(&focus, "main", RefKind::Local).out());
        focus.solo.push(target("main", FocusKind::Local));
        let main = RowFocus::of(&focus, "main", RefKind::Local);
        assert_eq!((main.soloed, main.out()), (true, false));
        assert!(RowFocus::of(&focus, "topic", RefKind::Local).out(), "something else is soloed");
        assert!(RowFocus::of_remote(&focus, "upstream").out());
        let Ask::Focus(command, params) = focus_ask(FocusButton::Solo, "main", FocusKind::Local, main) else {
            panic!("a focus ask")
        };
        assert_eq!((command, params), ("vcs.graph.solo", json!({ "name": "main", "kind": "local", "solo": false })));
        let Ask::Focus(command, params) =
            focus_ask(FocusButton::Hide, "origin", FocusKind::WholeRemote, RowFocus::default())
        else {
            panic!("a focus ask")
        };
        assert_eq!(command, "vcs.graph.hide");
        assert_eq!(params, json!({ "name": "origin", "kind": "wholeRemote", "hidden": true }));
    }

    #[test]
    fn a_match_is_highlighted_inside_its_column() {
        let ctx = egui::Context::default();
        let mut seen = Vec::new();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            let painter = ui.painter();
            let font = (theme::regular(13.0), Color32::WHITE);
            let text = "Fix the parser";
            for found in [Some(4..7), None, Some(3..100), Some(9..9)] {
                seen.push(found_cell(painter, (0.0, 300.0), 14.0, text, font.clone(), found, Color32::RED));
            }
            let long = "A very long message that does not fit in the column of the graph at all, with parser";
            seen.push(found_cell(painter, (0.0, 120.0), 14.0, long, font, Some(2..6), Color32::RED));
        });
        output.textures_delta.clear();
        assert_eq!(seen, [false, false, false, false, true], "out-of-range matches are drawn plain");
    }
}

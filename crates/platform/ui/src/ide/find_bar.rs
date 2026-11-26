use egui::{Align2, Color32, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_editor::Query;
use zen_i18n::t;
use zen_workbench::Workbench;

use super::find_history::{HistoryPopup, glyph_history, glyph_new_line};
use super::glyphs::{Glyph, glyph_more, stroke};
use super::search_filter::{FilterChoice, filter_menu, menu_heading, menu_row_with};
use super::{IdePalette, Side, hint, keymap_page};
use crate::{accessibility, icons, theme};

type Toggle = (&'static str, &'static str, bool, fn(&mut Query) -> &mut bool);

const ROW: f32 = 36.0;
const LINE: f32 = 18.0;
const MAX_LINES: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Query,
    Replacement,
}

#[derive(Default)]
pub(super) struct FindBar {
    query: String,
    replacement: String,
    focus: bool,
    history: HistoryPopup,
    history_of: Option<Field>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Stay,
    Closed,
}

fn row_height(text: &str) -> f32 {
    let lines = text.split('\n').count().clamp(1, MAX_LINES);
    ROW + (lines - 1) as f32 * LINE
}

fn text_rect(field: Rect, text: &str) -> Rect {
    if text.contains('\n') { field.with_min_y(field.top() + (ROW - 12.0 - LINE) / 2.0) } else { field }
}

fn insert_new_line(ctx: &egui::Context, id: egui::Id, text: &mut String) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let count = text.chars().count();
    let at = state.cursor.char_range().map_or(count, |range| usize::from(range.primary.index).min(count));
    let byte = text.char_indices().nth(at).map_or(text.len(), |(byte, _)| byte);
    text.insert(byte, '\n');
    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at + 1))));
    state.store(ctx, id);
}

fn tool_button(ui: &Ui, rect: Rect, id: egui::Id, palette: &IdePalette, on: bool) -> egui::Response {
    let response = ui.interact(rect, id, Sense::click());
    if on {
        ui.painter().rect_filled(rect, 4, palette.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4, palette.hover);
    }
    response
}

fn field_keys(ui: &Ui, field: Field) -> (bool, Vec<&'static str>) {
    ui.input_mut(|input| {
        let new_line = input.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::Enter);
        let mut commands = Vec::new();
        if field == Field::Query && input.consume_key(egui::Modifiers::ALT, Key::F7) {
            commands.push("editor.find.openInFindWindow");
        }
        let enter = match field {
            Field::Query => [
                (egui::Modifiers::ALT, "editor.find.selectAll"),
                (egui::Modifiers::SHIFT, "editor.find.previous"),
                (egui::Modifiers::NONE, "editor.find.next"),
            ]
            .into_iter()
            .find(|(modifiers, _)| input.consume_key(*modifiers, Key::Enter)),
            Field::Replacement => input
                .consume_key(egui::Modifiers::NONE, Key::Enter)
                .then_some((egui::Modifiers::NONE, "editor.find.replace")),
        };
        commands.extend(enter.map(|(_, command)| command));
        (new_line, commands)
    })
}

impl FindBar {
    pub(super) fn focus(&mut self) {
        self.focus = true;
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
    ) -> Outcome {
        let Some(workbench) = session.state::<Workbench>() else { return Outcome::Stay };
        let state = workbench.find().clone();
        if !state.open {
            self.history.close();
            return Outcome::Stay;
        }
        let query_id = ui.scope_id().with("find-query");
        let replace_id = ui.scope_id().with("find-replace");
        if self.query != state.query.text && !ui.memory(|memory| memory.has_focus(query_id)) {
            self.query = state.query.text.clone();
        }
        if self.replacement != state.replacement && !ui.memory(|memory| memory.has_focus(replace_id)) {
            self.replacement = state.replacement.clone();
        }

        let mut commands: Vec<(&str, serde_json::Value)> = Vec::new();
        let mut query = state.query.clone();
        let mut outcome = Outcome::Stay;

        let focused = [(Field::Query, query_id), (Field::Replacement, replace_id)]
            .into_iter()
            .find(|(_, id)| ui.memory(|memory| memory.has_focus(*id)));
        if let Some((field, id)) = focused {
            if self.history_of.is_some_and(|of| of != field) {
                self.history.close();
            }
            let replacements = field == Field::Replacement;
            if let Some(entry) = self.history.keys(ui, registry, session, replacements) {
                self.take_history(field, entry, &mut query, &mut commands);
            }
            if self.history.is_open() {
                self.history_of = Some(field);
            } else {
                let (new_line, keys) = field_keys(ui, field);
                let text = if replacements { &mut self.replacement } else { &mut self.query };
                if new_line {
                    insert_new_line(ui.ctx(), id, text);
                    self.changed(field, &mut query, &mut commands);
                }
                for command in keys {
                    let params = if command == "editor.find.replace" {
                        json!({ "replacement": self.replacement })
                    } else {
                        json!({})
                    };
                    commands.push((command, params));
                }
            }
        }

        let query_height = row_height(&self.query);
        let replace_height = if state.replace_open { row_height(&self.replacement) } else { 0.0 };
        let (bar, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), query_height + replace_height), Sense::hover());
        ui.painter().rect_filled(bar, 0, palette.frame);
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, palette.tab_border));

        let info = registry.dispatch(session, "editor.find.info", json!({})).ok();
        let count = info.as_ref().and_then(|info| info["count"].as_u64()).unwrap_or(0);
        let current = info.as_ref().and_then(|info| info["current"].as_u64());
        let error = info.as_ref().and_then(|info| info["error"].as_str().map(str::to_owned));
        let current_excluded = info.as_ref().is_some_and(|info| info["currentExcluded"] == true);
        let in_selection = info.as_ref().is_some_and(|info| info["inSelection"] == true);

        let row = Rect::from_min_size(bar.min, vec2(bar.width(), query_height));
        let middle = row.top() + ROW / 2.0;
        let chevron = Rect::from_center_size(pos2(row.left() + 16.0, middle), Vec2::splat(20.0));
        let toggle = tool_button(ui, chevron, ui.scope_id().with("find-replace-toggle"), palette, false);
        let glyph = if state.replace_open { icons::chevron_down } else { icons::chevron_right };
        glyph(ui.painter(), chevron.shrink(2.0), palette.text_secondary);
        hint(ui, &toggle, &t!("ide-find-toggle-replace"), None, Side::Below);
        if toggle.clicked() {
            commands.push(("editor.find.open", json!({ "replace": !state.replace_open, "keepQuery": true })));
        }
        let field_rect =
            Rect::from_min_max(pos2(row.left() + 58.0, row.top() + 6.0), pos2(row.right() - 262.0, row.bottom() - 6.0));
        let history_rect = Rect::from_center_size(pos2(row.left() + 44.0, middle), Vec2::splat(22.0));
        if self.history_button(ui, history_rect, registry, session, palette, Field::Query) {
            ui.memory_mut(|memory| memory.request_focus(query_id));
        }
        let text_at = text_rect(field_rect, &self.query);
        let editor = if self.query.contains('\n') {
            let lines = self.query.split('\n').count().min(MAX_LINES);
            egui::TextEdit::multiline(&mut self.query).desired_rows(lines)
        } else {
            egui::TextEdit::singleline(&mut self.query).align(Align2::LEFT_CENTER)
        };
        let field = ui.put(
            text_at,
            editor
                .id(query_id)
                .frame(egui::Frame::NONE)
                .font(theme::regular(13.0))
                .hint_text(t!("ide-find-hint"))
                .desired_width(field_rect.width() - 112.0),
        );
        accessibility::labelled(&field, &t!("ide-find-hint"));
        if std::mem::take(&mut self.focus) {
            field.request_focus();
        }
        if field.changed() {
            query.text = self.query.clone();
            commands.push(("editor.find.set", json!({ "query": query })));
        }
        if (field.has_focus() || field.lost_focus()) && ui.input(|input| input.key_pressed(Key::Escape)) {
            commands.push(("editor.find.close", json!({})));
            outcome = Outcome::Closed;
        }
        if self.history_of == Some(Field::Query)
            && let Some(entry) = self.history.show(ui, field_rect, history_rect, palette)
        {
            self.take_history(Field::Query, entry, &mut query, &mut commands);
            field.request_focus();
        }

        let new_line = Rect::from_center_size(pos2(field_rect.right() - 100.0, middle), vec2(24.0, 22.0));
        let response = tool_button(ui, new_line, ui.scope_id().with("find-new-line"), palette, false);
        glyph_new_line(ui.painter(), new_line.shrink(5.0), palette.text_secondary);
        hint(ui, &response, &t!("ide-find-new-line"), Some("⇧⌘⏎"), Side::Below);
        if response.clicked() {
            insert_new_line(ui.ctx(), query_id, &mut self.query);
            self.changed(Field::Query, &mut query, &mut commands);
            field.request_focus();
        }
        let toggles: [Toggle; 3] = [
            ("Cc", "ide-find-match-case", query.match_case, |q| &mut q.match_case),
            ("W", "ide-find-words", query.whole_words, |q| &mut q.whole_words),
            (".*", "ide-find-regex", query.regex, |q| &mut q.regex),
        ];
        for (index, (label, name, on, flag)) in toggles.into_iter().enumerate() {
            let rect =
                Rect::from_center_size(pos2(field_rect.right() - 74.0 + index as f32 * 26.0, middle), vec2(24.0, 22.0));
            let response = tool_button(ui, rect, ui.scope_id().with(("find-toggle", index)), palette, on);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                theme::semibold(11.5),
                if on { palette.text } else { palette.text_secondary },
            );
            hint(ui, &response, &t!(name), None, Side::Below);
            accessibility::pressed(&response, on);
            if response.clicked() {
                let value = flag(&mut query);
                *value = !*value;
                if query.match_case {
                    query.preserve_case = false;
                }
                commands.push(("editor.find.set", json!({ "query": query })));
            }
        }

        let status = match (&error, count, current) {
            (Some(_), _, _) => t!("ide-find-bad-regex"),
            (None, 0, _) if !query.text.is_empty() => t!("ide-find-no-matches"),
            (None, 0, _) => String::new(),
            (None, count, Some(current)) => t!("ide-find-count", current = current, count = count),
            (None, count, None) => t!("ide-find-results", count = count),
        };
        let bad = error.is_some() || (count == 0 && !query.text.is_empty());
        ui.painter().text(
            pos2(field_rect.right() + 12.0, middle),
            Align2::LEFT_CENTER,
            status,
            theme::regular(12.5),
            if bad { palette.excluded } else { palette.text_secondary },
        );
        let buttons: [(Glyph, &str, &str, f32); 3] = [
            (glyph_arrow_up, "ide-find-previous", "editor.find.previous", 134.0),
            (glyph_arrow_down, "ide-find-next", "editor.find.next", 106.0),
            (glyph_close, "ide-find-close", "editor.find.close", 20.0),
        ];
        for (index, (glyph, name, command, from_right)) in buttons.into_iter().enumerate() {
            let rect = Rect::from_center_size(pos2(row.right() - from_right, middle), Vec2::splat(24.0));
            let response = tool_button(ui, rect, ui.scope_id().with(("find-button", index)), palette, false);
            glyph(ui.painter(), rect.shrink(6.0), palette.text_secondary);
            hint(ui, &response, &t!(name), None, Side::Below);
            if response.clicked() {
                commands.push((command, json!({})));
                if command == "editor.find.close" {
                    outcome = Outcome::Closed;
                }
            }
        }
        let filter = Rect::from_center_size(pos2(row.right() - 76.0, middle), Vec2::splat(24.0));
        match filter_menu(ui, filter, palette, "find-filter", query.context, Some(in_selection)) {
            Some(FilterChoice::InSelection) => {
                commands.push(("editor.find.searchInSelection", json!({ "on": !in_selection })));
            }
            Some(FilterChoice::Context(context)) => {
                query.context = context;
                commands.push(("editor.find.set", json!({ "query": query })));
            }
            None => {}
        }
        let more = Rect::from_center_size(pos2(row.right() - 48.0, middle), Vec2::splat(24.0));
        commands.extend(more_menu(ui, more, palette, state.scroll_to_results));

        if state.replace_open {
            let row =
                Rect::from_min_size(pos2(bar.left(), bar.top() + query_height), vec2(bar.width(), replace_height));
            let middle = row.top() + ROW / 2.0;
            let field_rect = Rect::from_min_max(
                pos2(row.left() + 58.0, row.top() + 6.0),
                pos2(row.right() - 300.0, row.bottom() - 6.0),
            );
            let history_rect = Rect::from_center_size(pos2(row.left() + 44.0, middle), Vec2::splat(22.0));
            if self.history_button(ui, history_rect, registry, session, palette, Field::Replacement) {
                ui.memory_mut(|memory| memory.request_focus(replace_id));
            }
            let text_at = text_rect(field_rect, &self.replacement);
            let editor = if self.replacement.contains('\n') {
                let lines = self.replacement.split('\n').count().min(MAX_LINES);
                egui::TextEdit::multiline(&mut self.replacement).desired_rows(lines)
            } else {
                egui::TextEdit::singleline(&mut self.replacement).align(Align2::LEFT_CENTER)
            };
            let field = ui.put(
                text_at,
                editor
                    .id(replace_id)
                    .frame(egui::Frame::NONE)
                    .font(theme::regular(13.0))
                    .hint_text(t!("ide-find-replace-hint"))
                    .desired_width(field_rect.width() - 60.0),
            );
            accessibility::labelled(&field, &t!("ide-find-replace-hint"));
            if self.history_of == Some(Field::Replacement)
                && let Some(entry) = self.history.show(ui, field_rect, history_rect, palette)
            {
                self.take_history(Field::Replacement, entry, &mut query, &mut commands);
                field.request_focus();
            }
            let new_line = Rect::from_center_size(pos2(field_rect.right() - 48.0, middle), vec2(24.0, 22.0));
            let response = tool_button(ui, new_line, ui.scope_id().with("find-replace-new-line"), palette, false);
            glyph_new_line(ui.painter(), new_line.shrink(5.0), palette.text_secondary);
            hint(ui, &response, &t!("ide-find-new-line"), Some("⇧⌘⏎"), Side::Below);
            if response.clicked() {
                insert_new_line(ui.ctx(), replace_id, &mut self.replacement);
                self.changed(Field::Replacement, &mut query, &mut commands);
                field.request_focus();
            }
            let rect = Rect::from_center_size(pos2(field_rect.right() - 22.0, middle), vec2(24.0, 22.0));
            let response =
                tool_button(ui, rect, ui.scope_id().with("find-preserve-case"), palette, query.preserve_case);
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Aa",
                theme::semibold(11.5),
                if query.preserve_case { palette.text } else { palette.text_secondary },
            );
            hint(ui, &response, &t!("ide-find-preserve-case"), None, Side::Below);
            accessibility::pressed(&response, query.preserve_case);
            if response.clicked() {
                query.preserve_case = !query.preserve_case;
                if query.preserve_case {
                    query.match_case = false;
                }
                commands.push(("editor.find.set", json!({ "query": query })));
            }
            if field.changed() {
                commands.push(("editor.find.set", json!({ "query": query, "replacement": self.replacement })));
            }
            if (field.has_focus() || field.lost_focus()) && ui.input(|input| input.key_pressed(Key::Escape)) {
                commands.push(("editor.find.close", json!({})));
                outcome = Outcome::Closed;
            }
            let labels = [
                (t!("ide-find-replace"), "editor.find.replace"),
                (t!("ide-find-replace-all"), "editor.find.replaceAll"),
                (t!(if current_excluded { "ide-find-include" } else { "ide-find-exclude" }), "editor.find.exclude"),
            ];
            let mut x = field_rect.right() + 14.0;
            for (label, command) in labels {
                let galley = ui.painter().layout_no_wrap(label, theme::regular(12.5), palette.text);
                let rect = Rect::from_min_size(pos2(x, middle - 12.0), vec2(galley.size().x + 18.0, 24.0));
                let response = ui.interact(rect, ui.scope_id().with(command), Sense::click());
                ui.painter().rect_stroke(
                    rect,
                    5,
                    Stroke::new(1.0, if response.hovered() { palette.text_secondary } else { palette.tab_border }),
                    egui::StrokeKind::Inside,
                );
                accessibility::button(&response, galley.text());
                ui.painter().galley(rect.center() - galley.size() / 2.0, galley, palette.text);
                if response.clicked() {
                    let params = if command == "editor.find.exclude" {
                        json!({})
                    } else {
                        json!({ "replacement": self.replacement })
                    };
                    commands.push((command, params));
                }
                x = rect.right() + 8.0;
            }
        }

        if outcome == Outcome::Closed {
            self.history.close();
        }
        for (command, params) in commands {
            let _ = registry.dispatch(session, command, params);
        }
        outcome
    }

    fn history_button(
        &mut self,
        ui: &Ui,
        rect: Rect,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        field: Field,
    ) -> bool {
        let id = ui.scope_id().with(("find-history", field == Field::Query));
        let open = self.history.is_open() && self.history_of == Some(field);
        let response = tool_button(ui, rect, id, palette, open);
        glyph_history(ui.painter(), rect.shrink(4.0), palette.text_secondary);
        hint(ui, &response, &t!("ide-find-history"), Some("⌥↓"), Side::Below);
        if response.clicked() {
            if open {
                self.history.close();
            } else {
                self.history.open(registry, session, field == Field::Replacement);
                self.history_of = Some(field);
            }
        }
        response.clicked()
    }

    fn take_history(
        &mut self,
        field: Field,
        entry: String,
        query: &mut Query,
        commands: &mut Vec<(&str, serde_json::Value)>,
    ) {
        match field {
            Field::Query => self.query = entry,
            Field::Replacement => self.replacement = entry,
        }
        self.changed(field, query, commands);
    }

    fn changed(&self, field: Field, query: &mut Query, commands: &mut Vec<(&str, serde_json::Value)>) {
        match field {
            Field::Query => {
                query.text = self.query.clone();
                commands.push(("editor.find.set", json!({ "query": query })));
            }
            Field::Replacement => {
                commands.push(("editor.find.set", json!({ "query": query, "replacement": self.replacement })));
            }
        }
    }
}

fn more_menu(ui: &mut Ui, rect: Rect, palette: &IdePalette, scroll: bool) -> Option<(&'static str, serde_json::Value)> {
    let response = tool_button(ui, rect, ui.scope_id().with("find-more"), palette, false);
    glyph_more(ui.painter(), rect.shrink(6.0), palette.text_secondary);
    hint(ui, &response, &t!("ide-find-more"), None, Side::Below);
    let mut chosen = None;
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(320.0);
        let mut row = |ui: &mut Ui, action: &'static str, keys: Option<String>, checked: Option<bool>| {
            let label = keymap_page::action_title(action);
            let keys = keys.or_else(|| crate::keymap::shortcut(action)).unwrap_or_default();
            let response = menu_row_with(ui, palette, &label, checked == Some(true), &keys);
            let role = if checked.is_some() { egui::Role::MenuItemCheckBox } else { egui::Role::MenuItem };
            accessibility::item(&response, role, checked == Some(true), &label);
            if response.clicked() {
                let params = match checked {
                    Some(on) => json!({ "on": !on }),
                    None => json!({}),
                };
                chosen = Some((action, params));
                ui.close();
            }
        };
        row(ui, "editor.find.openInFindWindow", Some("⌥F7".to_owned()), None);
        ui.separator();
        menu_heading(ui, palette, &t!("ide-find-multiple-cursors"));
        row(ui, "editor.selection.addNextOccurrence", None, None);
        row(ui, "editor.selection.unselectOccurrence", None, None);
        row(ui, "editor.selection.selectAllOccurrences", None, None);
        ui.separator();
        row(ui, "editor.find.scrollToResults", None, Some(scroll));
    });
    chosen
}

pub(super) fn glyph_arrow_up(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.line_segment([rect.center_bottom(), rect.center_top()], s);
    painter.line_segment(
        [rect.center_top(), pos2(rect.left() + rect.width() * 0.15, rect.top() + rect.height() * 0.4)],
        s,
    );
    painter.line_segment(
        [rect.center_top(), pos2(rect.right() - rect.width() * 0.15, rect.top() + rect.height() * 0.4)],
        s,
    );
}

pub(super) fn glyph_arrow_down(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.line_segment([rect.center_top(), rect.center_bottom()], s);
    painter.line_segment(
        [rect.center_bottom(), pos2(rect.left() + rect.width() * 0.15, rect.bottom() - rect.height() * 0.4)],
        s,
    );
    painter.line_segment(
        [rect.center_bottom(), pos2(rect.right() - rect.width() * 0.15, rect.bottom() - rect.height() * 0.4)],
        s,
    );
}

pub(super) fn glyph_close(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.12);
    painter.line_segment([r.left_top(), r.right_bottom()], s);
    painter.line_segment([r.right_top(), r.left_bottom()], s);
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers};

    use super::*;

    struct Fixture {
        registry: Registry,
        session: Session,
        bar: FindBar,
        ctx: egui::Context,
        _dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new(text: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let file = dir.path().join("main.go");
            std::fs::write(&file, text).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.set_state(Workbench::default());
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
            registry.dispatch(&mut session, "editor.find.open", json!({})).unwrap();
            let mut fixture =
                Self { _dir: dir, registry, session, bar: FindBar::default(), ctx: egui::Context::default() };
            fixture.bar.focus();
            fixture.frame(Vec::new());
            fixture
        }

        fn frame(&mut self, events: Vec<Event>) -> (Vec<(String, egui::Pos2)>, f32) {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1000.0, 600.0))),
                ..Default::default()
            };
            let mut height = 0.0;
            let mut output = self.ctx.run_ui(input, |ui| {
                let top = ui.cursor().top();
                self.bar.show(ui, &self.registry, &mut self.session, &IdePalette::zen_dark());
                height = ui.cursor().top() - top;
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
            (texts, height)
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) -> (Vec<(String, egui::Pos2)>, f32) {
            self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }])
        }

        fn info(&mut self) -> serde_json::Value {
            self.registry.dispatch(&mut self.session, "editor.find.info", json!({})).unwrap()
        }
    }

    #[test]
    fn the_search_row_reads_toggles_then_the_count() {
        let mut fixture = Fixture::new("one two one\n");
        fixture.frame(vec![Event::Text("one".to_owned())]);
        let (texts, _) = fixture.frame(Vec::new());
        let x = |label: &str| texts.iter().find(|(text, _)| text == label).map(|(_, at)| at.x).unwrap();
        let count = t!("ide-find-count", current = 1, count = 2);
        assert!(x("Cc") < x("W") && x("W") < x(".*") && x(".*") < x(&count), "{texts:?}");
    }

    #[test]
    fn new_line_makes_the_field_multi_line() {
        let mut fixture = Fixture::new("one\ntwo\none\ntwo\n");
        let (_, single) = fixture.frame(vec![Event::Text("one".to_owned())]);
        fixture.key(Key::Enter, Modifiers::COMMAND | Modifiers::SHIFT);
        fixture.frame(vec![Event::Text("two".to_owned())]);
        let (_, height) = fixture.frame(Vec::new());
        let info = fixture.info();
        assert_eq!(info["state"]["query"]["text"], "one\ntwo");
        assert_eq!(info["count"], 2);
        assert_eq!(height, single + LINE, "the field grows by a line");
    }

    #[test]
    fn alt_down_lists_the_history_and_enter_takes_an_entry() {
        let mut fixture = Fixture::new("alpha beta\n");
        let history = json!({ "query": "alpha" });
        fixture.registry.dispatch(&mut fixture.session, "search.findHistory.add", history).unwrap();
        let history = json!({ "query": "beta" });
        fixture.registry.dispatch(&mut fixture.session, "search.findHistory.add", history).unwrap();
        fixture.key(Key::ArrowDown, Modifiers::ALT);
        let (texts, _) = fixture.frame(Vec::new());
        let shown: Vec<&str> =
            texts.iter().map(|(text, _)| text.as_str()).filter(|text| ["alpha", "beta"].contains(text)).collect();
        assert_eq!(shown, ["beta", "alpha"], "newest first");
        fixture.key(Key::ArrowDown, Modifiers::NONE);
        fixture.key(Key::Enter, Modifiers::NONE);
        assert_eq!(fixture.info()["state"]["query"]["text"], "alpha");
        fixture.key(Key::Enter, Modifiers::NONE);
        let after = fixture.registry.dispatch(&mut fixture.session, "search.findHistory.list", json!({})).unwrap();
        assert_eq!(after["queries"], json!(["alpha", "beta"]), "Enter searched with it");
    }

    #[test]
    fn alt_f7_opens_the_results_in_the_find_window() {
        let mut fixture = Fixture::new("one two one\n");
        fixture.frame(vec![Event::Text("one".to_owned())]);
        fixture.key(Key::F7, Modifiers::ALT);
        let usages = fixture.registry.dispatch(&mut fixture.session, "navigate.usages.list", json!({})).unwrap();
        assert_eq!((usages["occurrences"].as_bool(), usages["total"].as_u64()), (Some(true), Some(2)));
    }
    fn click(fixture: &mut Fixture, at: egui::Pos2) -> Vec<(String, egui::Pos2)> {
        let press = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        fixture.frame(vec![Event::PointerMoved(at), press(true)]);
        fixture.frame(vec![press(false)]);
        fixture.frame(Vec::new()).0
    }

    #[test]
    fn the_filter_and_the_more_menu_offer_their_items() {
        let mut fixture = Fixture::new("one\ntwo one\n");
        let selection =
            json!({ "selections": [{ "anchor": { "line": 0, "column": 0 }, "head": { "line": 1, "column": 3 } }] });
        fixture.registry.dispatch(&mut fixture.session, "editor.selection.set", selection).unwrap();
        let (texts, _) = fixture.frame(Vec::new());
        let bar_top = texts.iter().find(|(text, _)| text == "Cc").map(|(_, at)| at.y).unwrap();
        let filter = pos2(1000.0 - 76.0, bar_top + 6.0);
        let menu = click(&mut fixture, filter);
        let names: Vec<&str> = menu.iter().map(|(text, _)| text.as_str()).collect();
        let in_selection = t!("keymap-editor-find-searchInSelection");
        let at = names.iter().position(|name| *name == in_selection).unwrap_or_else(|| panic!("{names:?}"));
        assert!(names.contains(&"⌃⌥E"), "{names:?}");
        let anywhere = names.iter().position(|name| *name == t!("ide-search-filter-anywhere")).unwrap();
        assert!(at < anywhere, "Search In Selection first, then the six filters");
        let row = menu[at].1 + vec2(4.0, 4.0);
        click(&mut fixture, row);
        assert_eq!(fixture.info()["inSelection"], true);

        let more = pos2(1000.0 - 48.0, bar_top + 6.0);
        let menu = click(&mut fixture, more);
        let names: Vec<String> = menu.into_iter().map(|(text, _)| text).collect();
        for action in [
            "editor.find.openInFindWindow",
            "editor.selection.addNextOccurrence",
            "editor.selection.unselectOccurrence",
            "editor.selection.selectAllOccurrences",
            "editor.find.scrollToResults",
        ] {
            assert!(names.contains(&keymap_page::action_title(action)), "{action}: {names:?}");
        }
        assert!(names.contains(&"⌥F7".to_owned()) && names.contains(&t!("ide-find-multiple-cursors")), "{names:?}");
    }

    #[test]
    fn the_bar_and_its_menus_draw() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let mut fixture = Fixture::new("one two one\nthree one\n");
        let Fixture { registry, session, bar, .. } = &mut fixture;
        for (command, params) in [
            ("search.findHistory.add", json!({ "query": "alpha", "replacement": "beta" })),
            ("search.findHistory.add", json!({ "query": "one\nthree" })),
            ("editor.find.open", json!({ "replace": true })),
            ("editor.find.set", json!({ "query": { "text": "one" }, "replacement": "two" })),
        ] {
            registry.dispatch(session, command, params).unwrap();
        }
        let palette = IdePalette::zen_dark();
        let width = 1000.0;
        let mut screen = crate::offscreen::Offscreen::new(vec2(width, 260.0), 2.0);
        screen.set_background(palette.island);
        let top = std::cell::Cell::new(0.0);
        let mut draw = |events: Vec<Event>, session: &mut Session, bar: &mut FindBar, paint: bool| {
            let show = |ui: &mut Ui| {
                top.set(ui.cursor().top());
                bar.show(ui, registry, session, &palette);
            };
            if paint {
                Some(screen.frame(events, show))
            } else {
                screen.step(events, show);
                None
            }
        };
        let mut settle = |session: &mut Session, bar: &mut FindBar, events: Vec<Vec<Event>>| {
            for events in events {
                draw(events, session, bar, false);
            }
            for _ in 0..20 {
                draw(Vec::new(), session, bar, false);
            }
            draw(Vec::new(), session, bar, true).unwrap()
        };
        let click = |at: egui::Pos2| {
            let press = |pressed| Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            vec![vec![Event::PointerMoved(at), press(true)], vec![press(false)]]
        };
        let mut shots = vec![("find-bar", settle(session, bar, Vec::new()))];
        let middle = top.get() + ROW / 2.0;
        for (name, x) in [("find-bar-filter", width - 76.0), ("find-bar-more", width - 48.0)] {
            let mut open = click(pos2(x, middle));
            open.push(vec![Event::PointerMoved(pos2(100.0, 240.0))]);
            shots.push((name, settle(session, bar, open)));
            settle(session, bar, click(pos2(100.0, 240.0)));
        }
        registry.dispatch(session, "editor.find.set", json!({ "query": { "text": "one\nthree" } })).unwrap();
        shots.push(("find-bar-multiline", settle(session, bar, Vec::new())));
        let info = registry.dispatch(session, "editor.find.info", json!({})).unwrap();
        assert_eq!(info["state"]["query"]["context"], "anywhere", "closing the menus chose nothing");
        let mut history = click(pos2(44.0, middle));
        history.push(vec![Event::PointerMoved(pos2(100.0, 240.0))]);
        shots.push(("find-bar-history", settle(session, bar, history)));
        assert!(bar.history.is_open(), "the lens lists the history");
        if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from) {
            for (name, image) in &shots {
                crate::offscreen::write_png(&dir.join(format!("{name}.png")), image).unwrap();
            }
        }
        assert!(shots.windows(2).all(|pair| pair[0].1.pixels != pair[1].1.pixels), "each state draws differently");
    }
}

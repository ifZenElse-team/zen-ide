use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat};
use egui::{Align2, Color32, FontFamily, FontId, Key, Modifiers, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::{Highlighter, Span};

use super::find_history::{HistoryPopup, glyph_history, glyph_new_line};
use super::popup_size::{MARGIN, PopupKind};
use super::scope_row::ScopeRow;
use super::{IdePalette, dialog_frame, metrics};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const CODE_SIZE: f32 = 12.5;
const FETCH_EVERY: Duration = Duration::from_millis(100);
const DEBOUNCE: Duration = Duration::from_millis(150);
const COMMIT_AFTER: Duration = Duration::from_millis(400);
const PREVIEW_WINDOW: usize = 201;
const PREVIEW_ROW: f32 = 18.0;
const QUERY_LINES: usize = 5;
const POPUP: PopupKind = PopupKind { name: "findInFiles", min: vec2(600.0, 520.0) };

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Request {
    OpenInFindWindow,
    Failed(zen_core::DispatchError),
}

#[derive(Default)]
pub(super) struct FindInFiles {
    open: bool,
    replace: bool,
    query: String,
    history: HistoryPopup,
    query_id: Option<egui::Id>,
    match_case: bool,
    whole_words: bool,
    regex: bool,
    context: zen_editor::SearchContext,
    preserve_case: bool,
    mask: String,
    mask_on: bool,
    masks: Vec<String>,
    scope: ScopeRow,
    replacement: String,
    selected: usize,
    started: Option<Value>,
    replacing: bool,
    pending: Option<(Value, Instant)>,
    focus: bool,
    shown: Option<Rect>,
    list: Option<Rect>,
    offset: f32,
    results: Results,
    preview: Option<PreviewEditor>,
    pinned: bool,
    active: bool,
    new_tab: bool,
    too_many: bool,
    open_when_ready: bool,
}

#[derive(Default)]
struct Results {
    matches: Vec<Value>,
    rows: Vec<Row>,
    files: usize,
    more: bool,
    done: bool,
    error: Option<String>,
    fetched: Option<Instant>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Row {
    path: Value,
    relative: String,
    line: u64,
    text: String,
    ranges: Vec<(usize, usize)>,
    place: String,
    test: bool,
}

struct PreviewEditor {
    key: (Value, u64),
    relative: String,
    first: usize,
    original: String,
    text: String,
    marks: Vec<Range<usize>>,
    spans: Arc<Vec<Span>>,
    spans_of: String,
    edited: Option<Instant>,
    stale: bool,
    scroll: bool,
    id: Option<egui::Id>,
}

impl Results {
    fn fetch(&mut self, registry: &Registry, session: &mut Session, now: Instant, limit: usize) {
        let mut results = registry.dispatch(session, "search.text.results", json!({})).unwrap_or_default();
        let mut matches = match results["matches"].take() {
            Value::Array(matches) => matches,
            _ => Vec::new(),
        };
        self.more = matches.len() > limit;
        matches.truncate(limit);
        self.files = matches.iter().filter_map(|m| m["path"].as_str()).collect::<BTreeSet<_>>().len();
        self.rows = rows(&matches);
        self.matches = matches;
        self.done = results["done"] == true;
        self.error = results["error"].as_str().map(str::to_owned);
        self.fetched = Some(now);
    }
}

fn rows(matches: &[Value]) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for found in matches {
        let relative = found["path"].as_str().unwrap_or_default();
        let line = found["line"].as_u64().unwrap_or(1).saturating_sub(1);
        let column = found["column"].as_u64().unwrap_or(1).saturating_sub(1) as usize;
        let range = (column, found["length"].as_u64().unwrap_or(0) as usize);
        match rows.last_mut() {
            Some(row) if row.relative == relative && row.line == line => row.ranges.push(range),
            _ => rows.push(Row {
                path: found["path"].clone(),
                relative: relative.to_owned(),
                line,
                text: found["text"].as_str().unwrap_or_default().to_owned(),
                ranges: vec![range],
                place: String::new(),
                test: crate::file_icons::is_test_file(file_name(relative)),
            }),
        }
    }
    let mut folders: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for row in &rows {
        folders.entry(file_name(&row.relative)).or_default().insert(&row.relative);
    }
    let ambiguous: BTreeSet<String> =
        folders.into_iter().filter(|(_, paths)| paths.len() > 1).map(|(name, _)| name.to_owned()).collect();
    for row in &mut rows {
        let name = file_name(&row.relative);
        let parent = row.relative.rsplit('/').nth(1).filter(|_| ambiguous.contains(name));
        row.place = match parent {
            Some(parent) => format!("{parent}/{name} {}", row.line + 1),
            None => format!("{name} {}", row.line + 1),
        };
    }
    rows
}

fn file_name(relative: &str) -> &str {
    relative.rsplit('/').next().unwrap_or(relative)
}

impl FindInFiles {
    #[cfg(test)]
    pub(super) fn typed(&self) -> &str {
        &self.query
    }

    #[cfg(test)]
    pub(super) fn previewed(&self) -> Option<&str> {
        self.preview.as_ref().map(|preview| preview.relative.as_str())
    }

    pub(super) fn state(&self) -> Option<bool> {
        self.open.then_some(self.replace)
    }

    pub(super) fn holds_keyboard(&self) -> bool {
        self.open && (self.active || !self.pinned)
    }

    pub(super) fn pinned(&self) -> bool {
        self.pinned
    }

    pub(super) fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }

    pub(super) fn new_tab(&self) -> bool {
        self.new_tab
    }

    pub(super) fn set_new_tab(&mut self, on: bool) {
        self.new_tab = on;
    }

    pub(super) fn set_too_many(&mut self, asking: bool) {
        self.too_many = asking;
    }

    pub(super) fn reopen_with(&mut self, request: &Value) {
        let replacement = request["replacement"].as_str();
        self.open(replacement.is_some(), None, None);
        let query = &request["query"];
        self.query = query["text"].as_str().unwrap_or_default().to_owned();
        self.match_case = query["matchCase"] == true;
        self.whole_words = query["wholeWords"] == true;
        self.regex = query["regex"] == true;
        self.preserve_case = query["preserveCase"] == true;
        self.context = serde_json::from_value(query["context"].clone()).unwrap_or_default();
        let mask = request["mask"].as_str().unwrap_or_default();
        self.mask_on = !mask.is_empty();
        if self.mask_on {
            self.mask = mask.to_owned();
        }
        if let Some(replacement) = replacement {
            self.replacement = replacement.to_owned();
        }
    }

    pub(super) fn replacement(&self) -> Option<String> {
        (self.open && self.replace).then(|| self.replacement.clone())
    }

    pub(super) fn open(&mut self, replace: bool, selected: Option<String>, folder: Option<String>) {
        self.open = true;
        self.active = true;
        self.replace = replace;
        self.focus = true;
        self.selected = 0;
        if let Some(text) = selected.filter(|text| !text.is_empty() && !text.contains('\n')) {
            self.query = if self.regex { zen_editor::escape_regex(&text) } else { text };
        }
        self.history.close();
        self.scope.reopen(folder);
        self.started = None;
        self.pending = None;
        self.open_when_ready = false;
    }

    pub(super) fn search_params(&mut self, registry: &Registry, session: &mut Session) -> Option<Value> {
        if !self.open || self.query.is_empty() {
            return None;
        }
        let mut params = self.params(registry, session);
        if let Some(params) = params.as_object_mut() {
            params.remove("limit");
        }
        Some(params)
    }

    pub(super) fn handed_over(&mut self, registry: &Registry, session: &mut Session) {
        self.remember(registry, session, false);
        if !self.pinned {
            self.close(registry, session);
        }
    }

    fn params(&mut self, registry: &Registry, session: &mut Session) -> Value {
        let limit = session
            .settings()
            .get_or(zen_workbench::search::POPUP_LIMIT, zen_workbench::search::DEFAULT_POPUP_LIMIT)
            .max(1);
        let mut params = json!({
            "query": {
                "text": self.query,
                "matchCase": self.match_case,
                "wholeWords": self.whole_words,
                "regex": self.regex,
                "context": self.context,
                "preserveCase": self.replace && self.preserve_case,
            },
            "mask": if self.mask_on { self.mask.trim() } else { "" },
            "limit": limit + 1,
        });
        if let (Some(params), Value::Object(scope)) = (params.as_object_mut(), self.scope.params(registry, session)) {
            params.extend(scope);
        }
        params
    }

    fn limit(&self) -> usize {
        self.started
            .as_ref()
            .and_then(|params| params["limit"].as_u64())
            .map_or(usize::MAX, |limit| (limit as usize).saturating_sub(1).max(1))
    }

    fn remember(&self, registry: &Registry, session: &mut Session, replaced: bool) {
        let replacement = (replaced && self.replace).then(|| self.replacement.clone());
        let mask = self.mask_on.then(|| self.mask.clone());
        let entry = json!({ "query": self.query, "replacement": replacement, "mask": mask });
        let _ = registry.dispatch(session, "search.findHistory.add", entry);
    }

    fn close(&mut self, registry: &Registry, session: &mut Session) {
        if let Some(preview) = &mut self.preview {
            preview.commit(registry, session);
        }
        let _ = registry.dispatch(session, "search.text.cancel", json!({}));
        self.open = false;
        self.active = false;
        self.history.close();
    }

    fn search(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, now_please: bool) {
        let params = self.params(registry, session);
        if self.started.as_ref() == Some(&params) {
            self.pending = None;
            return;
        }
        let now = Instant::now();
        let waited = match &self.pending {
            Some((pending, at)) if *pending == params => now.saturating_duration_since(*at),
            _ => {
                self.pending = Some((params.clone(), now));
                Duration::ZERO
            }
        };
        if !(self.started.is_none() || now_please || waited >= DEBOUNCE) {
            ui.ctx().request_repaint_after(DEBOUNCE - waited);
            return;
        }
        let _ = registry.dispatch(session, "search.text.start", params.clone());
        self.results = Results::default();
        self.started = Some(params);
        self.pending = None;
        self.selected = 0;
        self.offset = 0.0;
    }

    fn keys(&mut self, ui: &Ui, busy: bool, rows: usize) -> (bool, bool, bool, bool) {
        let (mut open, mut find_window, mut close, mut reveal) = (false, false, false, false);
        let preview_focused =
            self.preview.as_ref().and_then(|preview| preview.id).is_some_and(|id| ui.memory(|m| m.has_focus(id)));
        let query_focused = self.query_id.is_some_and(|id| ui.memory(|m| m.has_focus(id)));
        if self.active && preview_focused && !self.too_many {
            find_window = ui.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Enter));
        }
        if !self.active || self.history.is_open() || preview_focused || self.too_many {
            return (open, find_window, close, reveal);
        }
        let options = Modifiers::CTRL | Modifiers::ALT;
        let enter_opens = query_focused || ui.memory(|m| m.focused().is_none());
        let mut new_line = false;
        ui.input_mut(|input| {
            if input.consume_key(Modifiers::NONE, Key::ArrowDown) && self.selected + 1 < rows {
                self.selected += 1;
                reveal = true;
            }
            if input.consume_key(Modifiers::NONE, Key::ArrowUp) {
                self.selected = self.selected.saturating_sub(1);
                reveal = true;
            }
            if input.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Enter) {
                new_line = true;
            }
            if input.consume_key(Modifiers::COMMAND, Key::Enter) {
                find_window = true;
            }
            if enter_opens && input.consume_key(Modifiers::NONE, Key::Enter) {
                open = true;
            }
            if input.consume_key(options, Key::C) {
                self.match_case = !self.match_case;
            }
            if input.consume_key(options, Key::W) {
                self.whole_words = !self.whole_words;
            }
            if input.consume_key(options, Key::X) {
                self.regex = !self.regex;
            }
            if !busy && input.consume_key(Modifiers::NONE, Key::Escape) {
                close = true;
            }
        });
        if new_line {
            self.insert_new_line(ui.ctx());
        }
        (open, find_window, close, reveal)
    }

    fn insert_new_line(&mut self, ctx: &egui::Context) {
        let chars = self.query.chars().count();
        let state = self.query_id.and_then(|id| egui::text_edit::TextEditState::load(ctx, id).map(|state| (id, state)));
        let at = state
            .as_ref()
            .and_then(|(_, state)| state.cursor.char_range())
            .map_or(chars, |range| range.primary.index.0.min(chars));
        let byte = self.query.char_indices().nth(at).map_or(self.query.len(), |(byte, _)| byte);
        self.query.insert(byte, '\n');
        if let Some((id, mut state)) = state {
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(at + 1))));
            state.store(ctx, id);
            ctx.memory_mut(|memory| memory.request_focus(id));
        }
    }

    pub(super) fn finish_replace(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
    ) -> Result<(), zen_core::DispatchError> {
        if !self.replacing {
            return Ok(());
        }
        match registry.dispatch(session, "search.replace.finish", json!({})) {
            Ok(Value::Null) => {
                ctx.request_repaint_after(Duration::from_millis(50));
                Ok(())
            }
            Ok(_) => {
                self.replacing = false;
                Ok(())
            }
            Err(error) => {
                self.replacing = false;
                Err(error)
            }
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        folder: Option<String>,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn Highlighter>>,
    ) -> Option<Request> {
        let mut request = None;
        if !self.open {
            return request;
        }
        if self.query_id.is_some_and(|id| ui.memory(|memory| memory.has_focus(id)))
            && self.active
            && let Some(entry) = self.history.keys(ui, registry, session, false)
        {
            self.query = entry;
        }
        let busy = self.scope.busy(ui.ctx());
        let (mut open_row, find_window, mut close, reveal) = self.keys(ui, busy, self.results.rows.len());
        if find_window {
            request = Some(Request::OpenInFindWindow);
        }
        self.search(ui, registry, session, open_row);
        let now = Instant::now();
        let limit = self.limit();
        let due = self.results.fetched.is_none_or(|at| now.saturating_duration_since(at) >= FETCH_EVERY);
        if !self.results.done && (due || open_row) {
            self.results.fetch(registry, session, now, limit);
        }
        let results = std::mem::take(&mut self.results);
        let rows = &results.rows;
        if !results.done {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        self.selected = self.selected.min(rows.len().saturating_sub(1));
        let shown = rows.get(self.selected).filter(|_| !self.query.is_empty());
        self.follow_preview(registry, session, shown, highlighter);

        let mut replace_all = false;
        let screen = ui.ctx().content_rect();
        let default = vec2((screen.width() * 0.62).clamp(680.0, 1000.0), (screen.height() * 0.78).clamp(520.0, 820.0));
        let max = screen.size() - Vec2::splat(2.0 * MARGIN);
        let size = POPUP.size(ui.ctx(), default, max);
        let place = move |size: Vec2| {
            let top = screen.top() + ((screen.height() - size.y) * 0.35).max(MARGIN);
            Rect::from_min_size(pos2(screen.center().x - size.x / 2.0, top), size)
        };
        let area = egui::Area::new(ui.scope_id().with("find-in-files"))
            .order(egui::Order::Foreground)
            .fixed_pos(place(size).min);
        let response = area.show(ui.ctx(), |ui| {
            let frame = dialog_frame(palette).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_width(size.x - 24.0);
                ui.set_height(size.y - 24.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                self.header(ui, registry, session, palette, &results);
                ui.add_space(10.0);
                let query = self.query_row(ui, registry, session, palette);
                if query.lost_focus() && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
                    open_row = true;
                }
                ui.add_space(6.0);
                self.scope.show(ui, registry, session, palette, folder.clone(), self.active);
                if self.replace {
                    ui.add_space(6.0);
                    replace_all = self.replace_row(ui, palette, !rows.is_empty());
                }
                if let Some(error) = &results.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(error).font(theme::regular(12.0)).color(palette.excluded));
                }
                ui.add_space(8.0);

                let footer = 34.0;
                let preview_height = ((ui.available_height() - footer) * 0.45).clamp(120.0, 420.0);
                let list_height = (ui.available_height() - preview_height - footer - 8.0).max(ROW * 3.0);
                if self.query.is_empty() {
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), list_height), Sense::hover());
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        t!("ide-fif-empty"),
                        theme::regular(13.0),
                        palette.text_secondary,
                    );
                    self.list = Some(rect);
                } else if let Some(index) = self.results_list(ui, palette, rows, list_height, reveal) {
                    open_row = true;
                    self.selected = index;
                }
                ui.add_space(8.0);
                let (preview, _) = ui.allocate_exact_size(vec2(ui.available_width(), preview_height), Sense::hover());
                self.draw_preview(ui, preview, palette);
                if self.footer(ui, palette, !rows.is_empty() && !self.query.is_empty()) {
                    request = Some(Request::OpenInFindWindow);
                }
            });
            self.shown = Some(frame.response.rect);
            POPUP.grips(ui, frame.response.rect, size, max, place);
        });
        if ui.input(|input| input.pointer.any_pressed())
            && !self.scope.busy(ui.ctx())
            && !self.too_many
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
        {
            let inside = response.response.rect.contains(pointer);
            if inside {
                self.active = true;
            } else if self.pinned {
                self.active = false;
            } else {
                close = true;
            }
        }
        self.results = results;
        if replace_all {
            if let Some(preview) = &mut self.preview {
                preview.commit(registry, session);
            }
            let mut params = self.params(registry, session);
            if let Some(params) = params.as_object_mut() {
                params.remove("limit");
            }
            params["replacement"] = json!(self.replacement);
            match registry.dispatch(session, "search.replace.start", params) {
                Ok(_) => self.replacing = true,
                Err(error) => request = Some(Request::Failed(error)),
            }
            self.remember(registry, session, true);
            self.started = None;
            self.preview = None;
        }
        if open_row && self.results.rows.is_empty() && !self.results.done {
            self.open_when_ready = true;
            open_row = false;
        } else if self.open_when_ready && !self.results.rows.is_empty() {
            self.open_when_ready = false;
            open_row = true;
        } else if self.results.done {
            self.open_when_ready = false;
        }
        let chosen = open_row.then(|| self.results.rows.get(self.selected).cloned()).flatten();
        if let Some(row) = chosen {
            let (line, (column, length)) = (row.line, row.ranges.first().copied().unwrap_or_default());
            let _ = registry.dispatch(session, "file.document.open", json!({ "path": row.path }));
            let _ = registry.dispatch(
                session,
                "editor.selection.set",
                json!({ "selections": [{ "anchor": { "line": line, "column": column }, "head": { "line": line, "column": column + length } }] }),
            );
            if self.pinned {
                self.active = false;
            } else {
                close = true;
            }
        }
        if close {
            self.remember(registry, session, false);
            self.close(registry, session);
        }
        request
    }

    fn header(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        results: &Results,
    ) {
        ui.horizontal(|ui| {
            let title = if self.replace { t!("ide-fif-replace-title") } else { t!("ide-fif-title") };
            ui.label(egui::RichText::new(title).font(theme::semibold(14.0)).color(palette.text));
            let count = if self.query.is_empty() {
                String::new()
            } else if results.matches.is_empty() && !results.done {
                t!("ide-fif-searching")
            } else if results.more {
                t!("ide-fif-count-more", matches = results.matches.len(), files = results.files)
            } else {
                t!("ide-fif-count", matches = results.matches.len(), files = results.files)
            };
            if !count.is_empty() {
                ui.add_space(10.0);
                ui.label(egui::RichText::new(count).font(theme::regular(12.5)).color(palette.text_secondary));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let (pin, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                let response = ui.interact(pin, ui.scope_id().with("fif-pin"), Sense::click());
                if self.pinned {
                    ui.painter().rect_filled(pin, 4, palette.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(pin, 4, palette.popup_hover);
                }
                glyph_pin(
                    ui.painter(),
                    pin.shrink(5.0),
                    if self.pinned { palette.text } else { palette.text_secondary },
                );
                super::hint(ui, &response, &t!("ide-fif-pin"), None, super::Side::Below);
                accessibility::pressed(&response, self.pinned);
                if response.clicked() {
                    self.pinned = !self.pinned;
                }
                let (filter, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                if let Some(context) =
                    super::search_filter::filter_button(ui, filter, palette, "fif-filter", self.context)
                {
                    self.context = context;
                }
                ui.add_space(6.0);
                let (chevron, _) = ui.allocate_exact_size(vec2(16.0, 24.0), Sense::hover());
                let list = ui.interact(chevron, ui.scope_id().with("fif-mask-list"), Sense::click());
                if list.hovered() {
                    ui.painter().rect_filled(chevron, 4, palette.popup_hover);
                }
                let (c, w) = (chevron.center(), 3.5);
                let stroke = Stroke::new(1.2, palette.text_secondary);
                ui.painter().line_segment([pos2(c.x - w, c.y - w / 2.0), pos2(c.x, c.y + w / 2.0)], stroke);
                ui.painter().line_segment([pos2(c.x, c.y + w / 2.0), pos2(c.x + w, c.y - w / 2.0)], stroke);
                accessibility::button(&list, &t!("ide-fif-mask-history"));
                if list.clicked() {
                    let data = registry.dispatch(session, "search.findHistory.list", json!({})).unwrap_or_default();
                    self.masks = data["masks"]
                        .as_array()
                        .map(|masks| masks.iter().filter_map(|mask| mask.as_str().map(str::to_owned)).collect())
                        .unwrap_or_default();
                }
                egui::Popup::menu(&list).show(|ui| {
                    ui.set_min_width(180.0);
                    if self.masks.is_empty() {
                        ui.label(egui::RichText::new(t!("ide-fif-mask-history-empty")).color(palette.text_secondary));
                    }
                    for mask in self.masks.clone() {
                        if ui.button(egui::RichText::new(&mask).font(theme::regular(12.5))).clicked() {
                            self.mask = mask;
                            self.mask_on = true;
                            ui.close();
                        }
                    }
                });
                let field = ui.add_enabled(
                    self.mask_on,
                    egui::TextEdit::singleline(&mut self.mask)
                        .font(theme::regular(12.5))
                        .hint_text("*.go, !*_test.go")
                        .desired_width(150.0),
                );
                accessibility::labelled(&field, &t!("ide-fif-mask"));
                let check = ui.checkbox(
                    &mut self.mask_on,
                    egui::RichText::new(t!("ide-fif-mask")).font(theme::regular(12.5)).color(palette.text_secondary),
                );
                if check.changed() && self.mask_on {
                    field.request_focus();
                }
            });
        });
    }

    fn query_row(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
    ) -> egui::Response {
        let lines = self.query.split('\n').count().clamp(1, QUERY_LINES);
        let height = if lines == 1 { 32.0 } else { 12.0 + 20.0 * lines as f32 };
        let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
        ui.painter().rect_filled(row, 6, palette.frame);
        let tools = 5.0 * 26.0 + 6.0;
        let field_rect =
            Rect::from_min_max(pos2(row.left() + 32.0, row.top()), pos2(row.right() - tools, row.bottom()));
        let hint = t!("ide-find-hint");
        let edit = if lines == 1 && !self.query.contains('\n') {
            egui::TextEdit::singleline(&mut self.query).align(Align2::LEFT_CENTER)
        } else {
            egui::TextEdit::multiline(&mut self.query).desired_rows(lines).margin(egui::Margin::symmetric(0, 6))
        };
        let field = ui.put(
            field_rect,
            edit.id(ui.scope_id().with("fif-query"))
                .frame(egui::Frame::NONE)
                .font(theme::regular(14.0))
                .hint_text(hint.as_str())
                .desired_width(field_rect.width()),
        );
        accessibility::labelled(&field, &hint);
        self.query_id = Some(field.id);
        if std::mem::take(&mut self.focus) {
            super::filter_popups::focus_selected(ui, &field, &self.query);
        }
        let history_rect = Rect::from_center_size(pos2(row.left() + 16.0, row.top() + 16.0), Vec2::splat(22.0));
        let history = ui.interact(history_rect, ui.scope_id().with("fif-history"), Sense::click());
        if self.history.is_open() {
            ui.painter().rect_filled(history_rect, 4, palette.selection);
        } else if history.hovered() {
            ui.painter().rect_filled(history_rect, 4, palette.popup_hover);
        }
        glyph_history(ui.painter(), history_rect.shrink(4.0), palette.text_secondary);
        super::hint(ui, &history, &t!("ide-find-history"), Some("⌥↓"), super::Side::Below);
        if history.clicked() {
            if self.history.is_open() {
                self.history.close();
            } else {
                self.history.open(registry, session, false);
            }
            field.request_focus();
        }
        if let Some(entry) = self.history.show(ui, row, history_rect, palette) {
            self.query = entry;
            field.request_focus();
        }
        let centre = |index: usize| pos2(row.right() - tools + 13.0 + index as f32 * 26.0, row.top() + 16.0);
        let clear_rect = Rect::from_center_size(centre(0), vec2(22.0, 22.0));
        if !self.query.is_empty() {
            let clear = ui.interact(clear_rect, ui.scope_id().with("fif-clear"), Sense::click());
            if clear.hovered() {
                ui.painter().rect_filled(clear_rect, 4, palette.popup_hover);
            }
            ui.painter().text(
                clear_rect.center(),
                Align2::CENTER_CENTER,
                "×",
                theme::regular(15.0),
                palette.text_secondary,
            );
            super::hint(ui, &clear, &t!("ide-fif-clear"), None, super::Side::Below);
            if clear.clicked() {
                self.query.clear();
                field.request_focus();
            }
        }
        let new_line_rect = Rect::from_center_size(centre(1), vec2(22.0, 22.0));
        let new_line = ui.interact(new_line_rect, ui.scope_id().with("fif-new-line"), Sense::click());
        if new_line.hovered() {
            ui.painter().rect_filled(new_line_rect, 4, palette.popup_hover);
        }
        glyph_new_line(ui.painter(), new_line_rect.shrink(5.0), palette.text_secondary);
        super::hint(ui, &new_line, &t!("ide-find-new-line"), Some("⇧⌘↵"), super::Side::Below);
        if new_line.clicked() {
            self.insert_new_line(ui.ctx());
        }
        let toggles: [(&str, &str, &str, &mut bool); 3] = [
            ("Cc", "ide-find-match-case", "⌃⌥C", &mut self.match_case),
            ("W", "ide-find-words", "⌃⌥W", &mut self.whole_words),
            (".*", "ide-find-regex", "⌃⌥X", &mut self.regex),
        ];
        for (index, (label, name, keys, flag)) in toggles.into_iter().enumerate() {
            let rect = Rect::from_center_size(centre(index + 2), vec2(24.0, 22.0));
            let response = ui.interact(rect, ui.scope_id().with(("fif-toggle", index)), Sense::click());
            if *flag {
                ui.painter().rect_filled(rect, 4, palette.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 4, palette.popup_hover);
            }
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                label,
                theme::semibold(11.5),
                if *flag { palette.text } else { palette.text_secondary },
            );
            super::hint(ui, &response, &t!(name), Some(keys), super::Side::Below);
            accessibility::pressed(&response, *flag);
            if response.clicked() {
                *flag = !*flag;
            }
        }
        if self.match_case {
            self.preserve_case = false;
        }
        field
    }

    fn replace_row(&mut self, ui: &mut Ui, palette: &IdePalette, any: bool) -> bool {
        let mut replace_all = false;
        ui.horizontal(|ui| {
            let hint = t!("ide-find-replace-hint");
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.replacement)
                    .font(theme::regular(13.5))
                    .hint_text(hint.as_str())
                    .desired_width(ui.available_width() - 172.0),
            );
            accessibility::labelled(&field, &hint);
            let (rect, response) = ui.allocate_exact_size(vec2(24.0, 22.0), Sense::click());
            if self.preserve_case {
                ui.painter().rect_filled(rect, 4, palette.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 4, palette.popup_hover);
            }
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "Aa",
                theme::semibold(11.5),
                if self.preserve_case { palette.text } else { palette.text_secondary },
            );
            let name = t!("ide-find-preserve-case");
            super::hint(ui, &response, &name, None, super::Side::Below);
            accessibility::pressed(&response, self.preserve_case);
            if response.clicked() {
                self.preserve_case = !self.preserve_case;
                if self.preserve_case {
                    self.match_case = false;
                }
            }
            let button = egui::Button::new(egui::RichText::new(t!("ide-find-replace-all")).color(palette.accent_text))
                .fill(palette.accent)
                .corner_radius(6);
            replace_all = ui.add_enabled(any, button).clicked();
        });
        replace_all
    }

    fn results_list(
        &mut self,
        ui: &mut Ui,
        palette: &IdePalette,
        rows: &[Row],
        height: f32,
        reveal: bool,
    ) -> Option<usize> {
        let mut opened = None;
        let list_top = ui.cursor().top();
        let mut list =
            egui::ScrollArea::vertical().id_salt("fif-results").max_height(height).auto_shrink([false, false]);
        let selected_top = self.selected as f32 * ROW;
        if reveal && selected_top < self.offset {
            list = list.vertical_scroll_offset(selected_top);
        } else if reveal && selected_top + ROW > self.offset + height {
            list = list.vertical_scroll_offset(selected_top + ROW - height);
        }
        let family = metrics::editor_family(ui.ctx());
        let scrolled = list.show_rows(ui, ROW, rows.len(), |ui, shown| {
            for (index, row) in shown.map(|index| (index, &rows[index])) {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                if index == self.selected {
                    ui.painter().rect_filled(rect, 4, palette.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 4, palette.popup_hover);
                } else if row.test {
                    ui.painter().rect_filled(rect, 0, palette.test_row);
                }
                accessibility::name_with(&response, egui::Role::ListBoxOption, || {
                    format!("{}, {}", row.text.trim(), row.place)
                });
                accessibility::edit(&response, |node| node.set_selected(index == self.selected));
                let right = ui.painter().text(
                    pos2(rect.right() - 8.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    &row.place,
                    theme::regular(12.0),
                    palette.text_secondary,
                );
                let lead = row.text.chars().count() - row.text.trim_start().chars().count();
                let ranges: Vec<Range<usize>> = row
                    .ranges
                    .iter()
                    .map(|(column, length)| column.saturating_sub(lead)..column.saturating_sub(lead) + length)
                    .collect();
                let job = highlighted(
                    row.text.trim_start(),
                    &ranges,
                    palette,
                    (right.left() - rect.left() - 20.0).max(40.0),
                    family.clone(),
                );
                let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                ui.painter().galley(
                    pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
                    galley,
                    palette.text,
                );
                if response.clicked() {
                    self.selected = index;
                    self.active = true;
                }
                if response.double_clicked() {
                    opened = Some(index);
                }
            }
        });
        self.offset = scrolled.state.clamped_offset().y;
        self.list = Some(Rect::from_x_y_ranges(ui.max_rect().x_range(), list_top..=ui.min_rect().bottom()));
        opened
    }

    fn footer(&mut self, ui: &mut Ui, palette: &IdePalette, any: bool) -> bool {
        let mut pressed = false;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.checkbox(
                &mut self.new_tab,
                egui::RichText::new(t!("ide-fif-new-tab")).font(theme::regular(12.5)).color(palette.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut job = LayoutJob::default();
                job.append("⌘↵  ", 0.0, TextFormat::simple(theme::regular(12.5), palette.text_secondary));
                job.append(
                    &t!("ide-fif-open-in-find-window"),
                    0.0,
                    TextFormat::simple(theme::regular(12.5), palette.text),
                );
                let button = egui::Button::new(job).corner_radius(6).stroke(Stroke::new(1.0, palette.button_border));
                pressed = ui.add_enabled(any, button).clicked();
            });
        });
        pressed
    }

    fn follow_preview(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        row: Option<&Row>,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn Highlighter>>,
    ) {
        let key = row.map(|row| (row.path.clone(), row.line));
        let stale = self.preview.as_ref().is_some_and(|preview| preview.stale);
        if stale || self.preview.as_ref().map(|preview| &preview.key) != key.as_ref() {
            if let Some(preview) = &mut self.preview {
                preview.commit(registry, session);
            }
            self.preview = row.and_then(|row| PreviewEditor::load(registry, session, row));
        }
        let Some(preview) = &mut self.preview else { return };
        if preview.edited.is_some_and(|at| at.elapsed() >= COMMIT_AFTER) {
            preview.commit(registry, session);
        }
        if preview.spans_of != preview.text {
            let name = file_name(&preview.relative).to_owned();
            preview.spans =
                Arc::new(highlighter(&name).map(|found| found.highlight(&preview.text)).unwrap_or_default());
            preview.spans_of = preview.text.clone();
        }
    }

    fn draw_preview(&mut self, ui: &mut Ui, rect: Rect, palette: &IdePalette) {
        ui.painter().rect_filled(rect, 6, palette.editor_background);
        ui.painter().rect_stroke(rect, 6, Stroke::new(1.0, palette.tab_border), egui::StrokeKind::Inside);
        let Some(preview) = &mut self.preview else { return };
        let header = Rect::from_min_size(rect.min, vec2(rect.width(), 24.0));
        let name = file_name(&preview.relative);
        let folder = preview.relative.strip_suffix(name).unwrap_or_default().trim_end_matches('/');
        crate::file_icons::file_icon(
            ui.painter(),
            Rect::from_center_size(pos2(header.left() + 16.0, header.center().y), Vec2::splat(14.0)),
            name,
        );
        let mut job = LayoutJob::default();
        job.append(name, 0.0, TextFormat::simple(theme::regular(12.5), palette.text));
        if !folder.is_empty() {
            job.append(&format!(" · {folder}"), 0.0, TextFormat::simple(theme::regular(12.5), palette.text_secondary));
        }
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().with_clip_rect(header).galley(
            pos2(header.left() + 30.0, header.center().y - galley.size().y / 2.0),
            galley,
            palette.text,
        );
        ui.painter().hline(header.x_range(), header.bottom(), Stroke::new(1.0, palette.tab_border));
        let body = Rect::from_min_max(pos2(rect.left() + 1.0, header.bottom() + 1.0), rect.max - vec2(1.0, 1.0));
        let family = metrics::editor_family(ui.ctx());
        let count = preview.text.split('\n').count();
        let gutter = (count + preview.first).to_string().len() as f32 * 8.0 + 18.0;
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
        child.set_clip_rect(body);
        let mut area = egui::ScrollArea::vertical()
            .id_salt(("fif-preview", preview.key.0.to_string()))
            .auto_shrink([false, false]);
        if std::mem::take(&mut preview.scroll) {
            let line = preview
                .marks
                .first()
                .map_or(0, |mark| preview.text.chars().take(mark.start).filter(|c| *c == '\n').count());
            area = area.vertical_scroll_offset((line as f32 * PREVIEW_ROW - body.height() / 3.0).max(0.0));
        }
        let (spans, marks, text_color) = (preview.spans.clone(), preview.marks.clone(), palette.code);
        let unchanged = preview.text == preview.original;
        let first = preview.first;
        let mut edited = false;
        area.show(&mut child, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let (numbers, _) =
                    ui.allocate_exact_size(vec2(gutter, count as f32 * PREVIEW_ROW + 8.0), Sense::hover());
                let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                    let marks: &[Range<usize>] = if unchanged { &marks } else { &[] };
                    let job = code_job(
                        text.as_str(),
                        &spans,
                        marks,
                        palette,
                        FontId::new(CODE_SIZE, family.clone()),
                        text_color,
                    );
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                let response = ui.add(
                    egui::TextEdit::multiline(&mut preview.text)
                        .id_salt("fif-preview-text")
                        .frame(egui::Frame::NONE)
                        .margin(egui::Margin::symmetric(4, 4))
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .layouter(&mut layouter),
                );
                accessibility::labelled(&response, &t!("ide-fif-title"));
                preview.id = Some(response.id);
                edited = response.changed();
                if response.has_focus() {
                    self.active = true;
                }
                let painter = ui.painter();
                for row in 0..count {
                    let y = response.rect.top() + 1.0 + row as f32 * PREVIEW_ROW + PREVIEW_ROW / 2.0;
                    painter.text(
                        pos2(numbers.right() - 8.0, y),
                        Align2::RIGHT_CENTER,
                        (first + row + 1).to_string(),
                        FontId::new(CODE_SIZE - 0.5, family.clone()),
                        palette.gutter_text,
                    );
                }
            });
        });
        if edited {
            preview.edited = Some(Instant::now());
            ui.ctx().request_repaint_after(COMMIT_AFTER);
        }
    }
}

impl PreviewEditor {
    fn load(registry: &Registry, session: &mut Session, row: &Row) -> Option<Self> {
        let asked = json!({ "path": row.path, "line": row.line + 1, "lines": PREVIEW_WINDOW });
        let shown = registry.dispatch(session, "search.text.preview", asked).ok()?;
        let first = shown["first"].as_u64().unwrap_or(1).saturating_sub(1) as usize;
        let lines: Vec<&str> =
            shown["lines"].as_array()?.iter().map(|line| line.as_str().unwrap_or_default()).collect();
        let at = (row.line as usize).saturating_sub(first);
        let start: usize = lines.iter().take(at).map(|line| line.chars().count() + 1).sum();
        let marks = row.ranges.iter().map(|(column, length)| start + column..start + column + length).collect();
        let text = lines.join("\n");
        Some(Self {
            key: (row.path.clone(), row.line),
            relative: row.relative.clone(),
            first,
            original: text.clone(),
            text,
            marks,
            spans: Arc::default(),
            spans_of: String::from("\u{0}"),
            edited: None,
            stale: false,
            scroll: true,
            id: None,
        })
    }

    fn commit(&mut self, registry: &Registry, session: &mut Session) {
        self.edited = None;
        let Some((line, column, end_line, end_column, text)) = change(&self.original, &self.text) else { return };
        let params = json!({
            "path": self.key.0,
            "line": self.first + line + 1,
            "column": column + 1,
            "endLine": self.first + end_line + 1,
            "endColumn": end_column + 1,
            "text": text,
            "expected": self.original.split('\n').collect::<Vec<_>>(),
            "expectedFrom": self.first + 1,
        });
        if registry.dispatch(session, "search.text.edit", params).is_ok() {
            self.marks = shifted(&self.marks, &self.original, &self.text);
            self.original = self.text.clone();
        } else {
            self.text = self.original.clone();
            self.stale = true;
        }
    }
}

fn shifted(marks: &[Range<usize>], old: &str, new: &str) -> Vec<Range<usize>> {
    let (old_chars, new_chars): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = old_chars.iter().zip(&new_chars).take_while(|(a, b)| a == b).count();
    let room = old_chars.len().min(new_chars.len()) - prefix;
    let suffix = old_chars.iter().rev().zip(new_chars.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let old_end = old_chars.len() - suffix;
    marks
        .iter()
        .filter_map(|mark| {
            if mark.end <= prefix {
                Some(mark.clone())
            } else if mark.start >= old_end {
                let at = |offset: usize| offset - old_end + new_chars.len() - suffix;
                Some(at(mark.start)..at(mark.end))
            } else {
                None
            }
        })
        .collect()
}

fn change(old: &str, new: &str) -> Option<(usize, usize, usize, usize, String)> {
    if old == new {
        return None;
    }
    let (old_chars, new_chars): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let prefix = old_chars.iter().zip(&new_chars).take_while(|(a, b)| a == b).count();
    let room = old_chars.len().min(new_chars.len()) - prefix;
    let suffix = old_chars.iter().rev().zip(new_chars.iter().rev()).take(room).take_while(|(a, b)| a == b).count();
    let place = |end: usize| {
        let before = &old_chars[..end];
        let line = before.iter().filter(|c| **c == '\n').count();
        let column = before.iter().rev().take_while(|c| **c != '\n').count();
        (line, column)
    };
    let (line, column) = place(prefix);
    let (end_line, end_column) = place(old_chars.len() - suffix);
    let text = new_chars[prefix..new_chars.len() - suffix].iter().collect();
    Some((line, column, end_line, end_column, text))
}

fn code_job(
    text: &str,
    spans: &[Span],
    marks: &[Range<usize>],
    palette: &IdePalette,
    font: FontId,
    plain: Color32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut piece = String::new();
    let mut piece_format: Option<TextFormat> = None;
    for (index, (byte, character)) in text.char_indices().enumerate() {
        let color = spans
            .get(spans.partition_point(|span| span.range.end <= byte))
            .filter(|span| span.range.contains(&byte))
            .map_or(plain, |span| palette.syntax(span.kind).color);
        let mut format = TextFormat {
            font_id: font.clone(),
            color,
            line_height: Some(PREVIEW_ROW),
            valign: egui::Align::Center,
            ..TextFormat::default()
        };
        if marks.iter().any(|mark| mark.contains(&index)) {
            format.background = palette.search;
        }
        if piece_format.as_ref().is_some_and(|current| *current != format) {
            job.append(&piece, 0.0, piece_format.take().unwrap_or_default());
            piece.clear();
        }
        piece_format = Some(format);
        piece.push(character);
    }
    match piece_format {
        Some(format) => job.append(&piece, 0.0, format),
        None => job.append(
            "",
            0.0,
            TextFormat {
                font_id: font,
                color: plain,
                line_height: Some(PREVIEW_ROW),
                valign: egui::Align::Center,
                ..TextFormat::default()
            },
        ),
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

fn highlighted(text: &str, ranges: &[Range<usize>], palette: &IdePalette, width: f32, family: FontFamily) -> LayoutJob {
    let font = FontId::new(CODE_SIZE, family);
    let plain = TextFormat { font_id: font.clone(), color: palette.code, ..TextFormat::default() };
    let marked = TextFormat { font_id: font, color: palette.text, background: palette.search, ..TextFormat::default() };
    let chars: Vec<char> = text.chars().collect();
    let mut job = LayoutJob::default();
    let mut at = 0;
    for range in ranges {
        let (start, end) = (range.start.clamp(at, chars.len()), range.end.min(chars.len()));
        job.append(&chars[at..start].iter().collect::<String>(), 0.0, plain.clone());
        if end > start {
            job.append(&chars[start..end].iter().collect::<String>(), 0.0, marked.clone());
        }
        at = end.max(start);
    }
    job.append(&chars[at..].iter().collect::<String>(), 0.0, plain);
    job.wrap =
        egui::text::TextWrapping {
            max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…')
        };
    job
}

pub(super) fn glyph_pin(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = super::glyphs::stroke(rect, color);
    let (x, top) = (rect.center().x, rect.top());
    let w = rect.width();
    painter.line_segment([pos2(x - w * 0.22, top + w * 0.1), pos2(x + w * 0.22, top + w * 0.1)], s);
    painter.line_segment([pos2(x - w * 0.15, top + w * 0.1), pos2(x - w * 0.15, top + w * 0.5)], s);
    painter.line_segment([pos2(x + w * 0.15, top + w * 0.1), pos2(x + w * 0.15, top + w * 0.5)], s);
    painter.line_segment([pos2(x - w * 0.35, top + w * 0.58), pos2(x + w * 0.35, top + w * 0.58)], s);
    painter.line_segment([pos2(x, top + w * 0.58), pos2(x, rect.bottom())], s);
}

#[cfg(test)]
mod tests {
    use egui::{Event, Pos2, pos2};

    use super::*;

    const SCREEN: Vec2 = vec2(1200.0, 800.0);

    struct Harness {
        ctx: egui::Context,
        registry: Registry,
        session: Session,
        window: FindInFiles,
        time: f64,
        requests: Vec<Request>,
        dir: tempfile::TempDir,
    }

    impl Harness {
        fn new(lines: usize) -> Self {
            let text: String = (0..lines).map(|at| format!("needle {at}\n")).collect();
            Self::with(&[("hay.go", &text)], "needle", 5000, lines)
        }

        fn with(files: &[(&str, &str)], query: &str, limit: usize, expected: usize) -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let dir = tempfile::tempdir().unwrap();
            for (name, text) in files {
                let path = dir.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            session.settings_mut().set(zen_workbench::search::POPUP_LIMIT, limit).unwrap();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            let ctx = egui::Context::default();
            theme::install(&ctx);
            POPUP.forget(&ctx);
            let mut window = FindInFiles::default();
            window.open(false, Some(query.to_owned()), None);
            let mut harness = Self { ctx, registry, session, window, time: 0.0, requests: Vec::new(), dir };
            harness.settle(expected.min(limit));
            harness
        }

        fn frame(&mut self, events: Vec<Event>) -> egui::FullOutput {
            self.time += 1.0 / 60.0;
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)),
                time: Some(self.time),
                ..Default::default()
            };
            let Self { ctx, registry, session, window, requests, .. } = self;
            let mut output = ctx.run_ui(input, |ui| {
                let request = window.show(ui, registry, session, &IdePalette::zen_dark(), None, &|_| None);
                requests.extend(request);
            });
            output.textures_delta.clear();
            output
        }

        fn settle(&mut self, matches: usize) {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                self.frame(Vec::new());
                let results = self.registry.dispatch(&mut self.session, "search.text.results", json!({})).unwrap();
                let found = results["matches"].as_array().map_or(0, Vec::len);
                if results["done"] == true && found >= matches {
                    break;
                }
                if results["done"] == true {
                    self.window.started = None;
                }
                assert!(Instant::now() < deadline, "timed out: {results}");
                std::thread::sleep(Duration::from_millis(10));
            }
            while !self.window.results.done || self.window.results.matches.len() != matches {
                assert!(Instant::now() < deadline, "the window did not show the results");
                std::thread::sleep(Duration::from_millis(10));
                self.frame(Vec::new());
            }
        }

        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(pointer(from, None));
            self.frame(pointer(from, Some(true)));
            for step in 1..=4 {
                self.frame(pointer(from + (to - from) * step as f32 / 4.0, None));
            }
            self.frame(pointer(to, Some(false)));
            self.frame(Vec::new());
        }

        fn click(&mut self, at: Pos2) {
            self.frame(pointer(at, None));
            self.frame(pointer(at, Some(true)));
            self.frame(pointer(at, Some(false)));
        }

        fn shown(&self) -> Rect {
            self.window.shown.expect("the window is drawn")
        }
    }

    fn pointer(pos: Pos2, pressed: Option<bool>) -> Vec<Event> {
        let mut events = vec![Event::PointerMoved(pos)];
        if let Some(pressed) = pressed {
            events.push(Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        events
    }

    fn key(key: Key, modifiers: Modifiers) -> Vec<Event> {
        vec![
            Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers },
            Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers },
        ]
    }

    #[test]
    fn the_window_is_resized_by_its_corner_and_keeps_its_size() {
        let mut harness = Harness::new(3);
        let before = harness.shown();
        let corner = before.right_bottom() - vec2(2.0, 2.0);
        harness.drag(corner, corner + vec2(50.0, 40.0));
        let bigger = harness.shown();
        assert!((bigger.right() - (corner.x + 50.0)).abs() < 2.0, "{before:?} → {bigger:?}");
        assert!((bigger.bottom() - (corner.y + 40.0)).abs() < 2.0, "{before:?} → {bigger:?}");
        assert!((bigger.center().x - before.center().x).abs() < 1.0, "it stays centred");
        harness.window.open = false;
        harness.frame(Vec::new());
        harness.window.open(false, None, None);
        harness.frame(Vec::new());
        assert_eq!(harness.shown(), bigger, "the size is remembered");
        let corner = bigger.right_bottom() - vec2(2.0, 2.0);
        harness.drag(corner, corner - vec2(1000.0, 1000.0));
        let smallest = harness.shown().size();
        assert!((smallest - POPUP.min).max_elem() <= 2.0, "no smaller than its minimum: {smallest:?}");
        let corner = harness.shown().right_bottom() - vec2(2.0, 2.0);
        harness.drag(corner, corner + vec2(3000.0, 3000.0));
        let largest = harness.shown();
        let window = Rect::from_min_size(Pos2::ZERO, SCREEN).shrink(MARGIN - 3.0);
        assert!(window.contains_rect(largest), "within the window: {largest:?}");
        POPUP.forget(&harness.ctx);
    }

    #[test]
    fn dragging_the_scroll_bar_keeps_the_results_where_they_are_dragged() {
        let mut harness = Harness::new(200);
        let list = harness.window.list.expect("the results are drawn");
        let thumb = pos2(list.right() - 3.0, list.top() + 6.0);
        harness.drag(thumb, thumb + vec2(0.0, 150.0));
        for _ in 0..60 {
            harness.frame(Vec::new());
        }
        harness.click(pos2(list.center().x, list.top() + ROW / 2.0));
        assert!(harness.window.selected > 10, "the list went back to the top: {}", harness.window.selected);
    }

    #[test]
    fn a_frame_lays_out_only_the_results_in_view_and_fetches_them_once() {
        let mut harness = Harness::new(1500);
        let fetched = harness.window.results.fetched;
        harness.ctx.enable_accesskit();
        let mut output = harness.frame(Vec::new());
        let rows = output.platform_output.accesskit_update.take().expect("the accessibility tree").nodes;
        let rows = rows.iter().filter(|(_, node)| node.role() == egui::accesskit::Role::ListBoxOption).count();
        let list = harness.window.list.expect("the results are drawn");
        assert!(rows <= (list.height() / ROW) as usize + 2, "{rows} rows laid out for {list:?}");
        for _ in 0..5 {
            harness.frame(Vec::new());
        }
        assert_eq!(harness.window.results.fetched, fetched, "the finished results are not asked for again");
        assert_eq!(harness.window.results.matches.len(), 1500);
    }

    #[test]
    fn arrow_keys_bring_the_selected_result_into_view() {
        let mut harness = Harness::new(200);
        for _ in 0..40 {
            harness.frame(key(Key::ArrowDown, Modifiers::NONE));
        }
        harness.frame(Vec::new());
        let list = harness.window.list.expect("the results are drawn");
        let top = harness.window.selected as f32 * ROW - harness.window.offset;
        assert_eq!(harness.window.selected, 40);
        assert!(top >= 0.0 && top + ROW <= list.height() + 0.5, "the selected row is at {top} in {list:?}");
    }

    #[test]
    fn matches_of_a_line_share_a_row_ambiguous_names_show_their_folder_and_tests_are_marked() {
        let harness = Harness::with(
            &[
                ("kucoin/client.go", "Version Version\n"),
                ("binance/client.go", "Version\n"),
                ("sol.go", "x Version\n"),
                ("sol_test.go", "Version\n"),
            ],
            "Version",
            100,
            5,
        );
        let rows = &harness.window.results.rows;
        let summary: Vec<(&str, usize, bool)> =
            rows.iter().map(|row| (row.place.as_str(), row.ranges.len(), row.test)).collect();
        assert_eq!(
            summary,
            [
                ("binance/client.go 1", 1, false),
                ("kucoin/client.go 1", 2, false),
                ("sol.go 1", 1, false),
                ("sol_test.go 1", 1, true)
            ]
        );
        assert_eq!(rows[1].ranges, [(0, 7), (8, 7)]);
        let job = highlighted(
            "x Version",
            &[Range { start: 2, end: 9 }],
            &IdePalette::zen_dark(),
            400.0,
            FontFamily::Monospace,
        );
        assert_eq!(job.sections.len(), 3);
    }

    #[test]
    fn at_the_limit_the_count_says_plus_and_only_the_limit_is_listed() {
        let harness =
            Harness::with(&[("a.go", &"needle\n".repeat(30)), ("b.go", &"needle\n".repeat(30))], "needle", 40, 41);
        let results = &harness.window.results;
        assert!(results.more);
        assert_eq!((results.matches.len(), results.rows.len(), results.files), (40, 40, 2));
        zen_i18n::set_language(Some("en"));
        assert_eq!(t!("ide-fif-count-more", matches = 100, files = 29), "100+ matches in 29+ files");
    }

    #[test]
    fn typing_starts_the_search_once_the_query_rests() {
        let mut harness = Harness::new(3);
        let started = harness.window.started.clone();
        harness.window.query = "needle 1".to_owned();
        harness.frame(Vec::new());
        assert_eq!(harness.window.started, started, "not on the keystroke itself");
        std::thread::sleep(DEBOUNCE + Duration::from_millis(20));
        harness.frame(Vec::new());
        assert_eq!(harness.window.started.as_ref().unwrap()["query"]["text"], "needle 1");
        harness.window.query.clear();
        harness.frame(Vec::new());
        std::thread::sleep(DEBOUNCE + Duration::from_millis(20));
        harness.frame(Vec::new());
        for _ in 0..50 {
            if harness.window.results.done {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
            harness.frame(Vec::new());
        }
        assert!(harness.window.results.rows.is_empty() && harness.window.results.done, "nothing is searched for");
    }

    #[test]
    fn a_selection_becomes_the_query_escaped_under_regex() {
        let mut window = FindInFiles { regex: true, ..FindInFiles::default() };
        window.open(false, Some("a.b(c)".to_owned()), None);
        assert_eq!(window.query, r"a\.b\(c\)");
        window.regex = false;
        window.open(false, Some("x.y".to_owned()), None);
        assert_eq!(window.query, "x.y");
    }

    #[test]
    fn a_pinned_window_stays_open_when_the_editor_is_clicked_and_lets_the_keys_go() {
        let mut harness = Harness::new(3);
        harness.click(pos2(5.0, 5.0));
        assert!(harness.window.state().is_none(), "a click outside closes the window that is not pinned");
        harness.window.open(false, None, None);
        harness.window.set_pinned(true);
        harness.settle(3);
        harness.click(pos2(5.0, 5.0));
        assert_eq!(harness.window.state(), Some(false), "pinned, it stays open");
        assert!(!harness.window.holds_keyboard());
        assert_eq!(harness.window.results.rows.len(), 3, "with its results");
        harness.frame(key(Key::ArrowDown, Modifiers::NONE));
        assert_eq!(harness.window.selected, 0, "the arrow keys are the editor's");
        harness.frame(key(Key::Escape, Modifiers::NONE));
        assert!(harness.window.state().is_some(), "and so is Escape");
        let list = harness.window.list.unwrap();
        harness.click(pos2(list.center().x, list.top() + ROW * 1.5));
        assert!(harness.window.holds_keyboard());
        harness.frame(key(Key::ArrowDown, Modifiers::NONE));
        assert_eq!(harness.window.selected, 2);
    }

    #[test]
    fn the_option_keys_toggle_new_line_breaks_the_query_and_command_enter_asks_for_the_find_window() {
        let mut harness = Harness::new(3);
        let options = Modifiers::CTRL | Modifiers::ALT;
        harness.frame(key(Key::C, options));
        harness.frame(key(Key::W, options));
        harness.frame(key(Key::X, options));
        assert!(harness.window.match_case && harness.window.whole_words && harness.window.regex);
        harness.frame(key(Key::Enter, Modifiers::COMMAND | Modifiers::SHIFT));
        assert!(harness.window.query.contains('\n'), "{:?}", harness.window.query);
        harness.frame(key(Key::Enter, Modifiers::COMMAND));
        assert_eq!(harness.requests, [Request::OpenInFindWindow]);
    }

    #[test]
    fn typing_in_the_preview_writes_the_file_once_typing_pauses() {
        let mut harness = Harness::with(&[("a.go", "package a\n\nvar Version = 1\n")], "Version", 100, 1);
        harness.frame(Vec::new());
        let preview = harness.window.preview.as_mut().expect("the selected row is previewed");
        assert_eq!((preview.first, preview.marks.clone()), (0, vec![Range { start: 15, end: 22 }]));
        preview.text = preview.text.replace("Version", "Release");
        preview.edited = Some(Instant::now() - COMMIT_AFTER);
        harness.frame(Vec::new());
        let file = harness.dir.path().join("a.go");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package a\n\nvar Release = 1\n");
        std::fs::write(&file, "package a\n\n// changed\nvar Release = 1\n").unwrap();
        let preview = harness.window.preview.as_mut().unwrap();
        preview.text = preview.text.replace("Release", "Stale");
        preview.edited = Some(Instant::now() - COMMIT_AFTER);
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package a\n\n// changed\nvar Release = 1\n");
        let preview = harness.window.preview.as_ref().unwrap();
        assert_eq!(preview.text, "package a\n\n// changed\nvar Release = 1", "read again from the file");
        assert_eq!(
            shifted(&[Range { start: 4, end: 11 }], "var Version", "// e\nvar Version"),
            [Range { start: 9, end: 16 }]
        );
        assert!(shifted(&[Range { start: 4, end: 11 }], "var Version", "var Vers").is_empty(), "an edited match");
        assert_eq!(change("ab\ncd", "ab\nxd"), Some((1, 0, 1, 1, "x".to_owned())));
        assert_eq!(change("ab\ncd", "abcd"), Some((0, 2, 1, 0, String::new())));
    }

    struct Keywords;

    impl Highlighter for Keywords {
        fn handles(&self, _: &str) -> bool {
            true
        }

        fn highlight(&self, text: &str) -> Vec<Span> {
            let mut spans = Vec::new();
            for word in ["package", "func", "var", "return"] {
                spans.extend(
                    text.match_indices(word)
                        .map(|(at, _)| Span { range: at..at + word.len(), kind: zen_syntax::HighlightKind::Keyword }),
                );
            }
            spans.sort_by_key(|span| span.range.start);
            spans
        }
    }

    #[test]
    fn the_preview_colours_the_code_and_marks_the_matches() {
        let palette = IdePalette::zen_dark();
        let text = "var Version = 1";
        let spans = Keywords.highlight(text);
        let job =
            code_job(text, &spans, &[Range { start: 4, end: 11 }], &palette, FontId::monospace(12.0), palette.code);
        assert_eq!(job.text, text, "the editor's text stays as typed");
        let at = |byte: usize| {
            job.sections
                .iter()
                .find(|section| (section.byte_range.start.0..section.byte_range.end.0).contains(&byte))
                .unwrap()
        };
        assert_eq!(at(0).format.color, palette.syntax(zen_syntax::HighlightKind::Keyword).color);
        assert_eq!(at(5).format.background, palette.search);
        assert_eq!(at(13).format.color, palette.code);
    }

    #[test]
    fn snapshots_of_the_window() {
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
        let files: &[(&str, &str)] = &[
            ("kucoin/client.go", "package kucoin\n\n// Version of the client.\nvar Version = \"1\" // Version\n"),
            ("binance/client.go", "package binance\n\nvar Version = \"2\"\n"),
            ("sol.go", "package main\n\nfunc main() { println(Version) }\n"),
            ("sol_test.go", "package main\n\nvar _ = Version\n"),
        ];
        for language in ["en", "ru"] {
            let mut harness = Harness::with(files, "Version", 100, 6);
            zen_i18n::set_language(Some(language));
            harness.window.mask_on = true;
            harness.window.mask = "*.go".to_owned();
            harness.window.pinned = true;
            harness.window.started = None;
            let deadline = Instant::now() + Duration::from_secs(60);
            harness.frame(Vec::new());
            while !harness.window.results.done || harness.window.results.matches.len() != 6 {
                assert!(Instant::now() < deadline, "the masked search did not finish");
                std::thread::sleep(Duration::from_millis(20));
                harness.frame(Vec::new());
            }
            harness.window.preview = None;
            let mut screen = crate::offscreen::Offscreen::new(SCREEN, 2.0);
            theme::install(screen.context());
            POPUP.forget(screen.context());
            let Harness { registry, session, window, .. } = &mut harness;
            let image = screen.render(|ui| {
                let _ =
                    window.show(ui, registry, session, &IdePalette::zen_dark(), None, &|_| Some(Arc::new(Keywords)));
            });
            let path = std::path::Path::new(&dir).join(format!("find-in-files-{language}.png"));
            crate::offscreen::write_png(&path, &image).unwrap();
            window.query.clear();
            let image = screen.render(|ui| {
                let _ = window.show(ui, registry, session, &IdePalette::zen_dark(), None, &|_| None);
            });
            let path = std::path::Path::new(&dir).join(format!("find-in-files-empty-{language}.png"));
            crate::offscreen::write_png(&path, &image).unwrap();
        }
        zen_i18n::set_language(Some("en"));
    }
}

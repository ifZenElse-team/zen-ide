use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::Highlighter;
use zen_workbench::vcs::conflict::{ConflictChange, ConflictEditor, ConflictKind, ConflictView};

use super::super::diff_view::{arrow, line_job, scroll_bars, sideways};
use super::super::{IdePalette, IdeView, Side, hint, hover};
use super::look;
use crate::theme;
use zen_workbench::vcs::workspace::{CONFLICT_TAB, WORKSPACE as GIT};
use zen_workbench::workspaces::Workspaces;

const ROW: f32 = 22.0;
const NUMBERS: f32 = 40.0;
const GUTTER: f32 = 34.0;
const MARKS: f32 = 12.0;
const TOOLBAR: f32 = 54.0;
const TITLES: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Pane {
    Ours,
    Result,
    Theirs,
}

#[derive(Default)]
struct Text {
    lines: Vec<(usize, String)>,
    raw: Vec<String>,
    spans: Vec<zen_syntax::Span>,
}

impl Text {
    fn new(text: &str, highlighter: Option<&Arc<dyn Highlighter>>) -> Self {
        let mut offset = 0;
        let mut lines = Vec::new();
        let mut raw = Vec::new();
        for line in text.split_inclusive('\n') {
            lines.push((offset, line.trim_end_matches(['\n', '\r']).to_owned()));
            raw.push(line.to_owned());
            offset += line.len();
        }
        let spans = highlighter.map(|highlighter| highlighter.highlight(text)).unwrap_or_default();
        Text { lines, raw, spans }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Segment {
    pub(super) change: Option<usize>,
    pub(super) ours: std::ops::Range<usize>,
    pub(super) result: std::ops::Range<usize>,
    pub(super) theirs: std::ops::Range<usize>,
}

impl Segment {
    fn rows(&self, result_lines: Option<usize>) -> usize {
        let result = result_lines.unwrap_or(self.result.len());
        let most = self.ours.len().max(self.theirs.len()).max(result);
        if self.change.is_some() { most.max(1) } else { most }
    }
}

fn segments(view: &ConflictView, ours: usize, theirs: usize, result: usize) -> Vec<Segment> {
    let mut segments = Vec::new();
    let (mut o, mut t, mut r) = (0, 0, 0);
    let same = |segments: &mut Vec<Segment>, to: (usize, usize, usize), at: &mut (usize, usize, usize)| {
        let length = to.0.saturating_sub(at.0).max(to.1.saturating_sub(at.1)).max(to.2.saturating_sub(at.2));
        if length > 0 {
            segments.push(Segment {
                change: None,
                ours: at.0..to.0.max(at.0),
                theirs: at.1..to.1.max(at.1),
                result: at.2..to.2.max(at.2),
            });
        }
        *at = to;
    };
    let mut at = (o, t, r);
    for change in &view.changes {
        same(&mut segments, (change.ours[0], change.theirs[0], change.result[0]), &mut at);
        segments.push(Segment {
            change: Some(change.index),
            ours: change.ours[0]..change.ours[1],
            theirs: change.theirs[0]..change.theirs[1],
            result: change.result[0]..change.result[1],
        });
        at = (change.ours[1], change.theirs[1], change.result[1]);
    }
    (o, t, r) = at;
    if o < ours || t < theirs || r < result {
        same(&mut segments, (ours, theirs, result), &mut (o, t, r));
    }
    segments
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    pub(super) segment: usize,
    pub(super) offset: usize,
}

fn rows(segments: &[Segment], editing: Option<(usize, usize)>) -> Vec<Row> {
    let mut rows = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let draft = editing.filter(|(edited, _)| *edited == index).map(|(_, lines)| lines);
        for offset in 0..segment.rows(draft) {
            rows.push(Row { segment: index, offset });
        }
    }
    rows
}

struct Editing {
    segment: usize,
    draft: String,
    focus: bool,
}

#[derive(Default)]
pub(in crate::ide) struct ConflictTab {
    seen_raised: u64,
    seen_revision: u64,
    view: Option<ConflictView>,
    ours: Text,
    theirs: Text,
    result: Text,
    current: Option<usize>,
    scroll_to: Option<usize>,
    scroll_x: f32,
    editing: Option<Editing>,
    confirm: bool,
}

enum Ask {
    Resolve(serde_json::Value),
    Text(String),
}

impl IdeView {
    pub(in crate::ide) fn conflict_in_front(&self, session: &Session) -> bool {
        self.git_tab_in_front(session, CONFLICT_TAB)
            && session.state::<ConflictEditor>().is_some_and(ConflictEditor::is_open)
    }

    pub(in crate::ide) fn conflict_title(&self, session: &Session) -> String {
        let name = session
            .state::<ConflictEditor>()
            .and_then(ConflictEditor::relative)
            .map(|relative| relative.rsplit('/').next().unwrap_or(relative).to_owned())
            .unwrap_or_default();
        t!("vcs-conflict-tab", name = name.as_str())
    }

    pub(in crate::ide) fn conflict_frame(&mut self, session: &mut Session) {
        let Some(editor) = session.state::<ConflictEditor>() else { return };
        let (open, raised) = (editor.is_open(), editor.raised());
        if !open {
            session.state_mut::<Workspaces>().remove_tab(GIT, CONFLICT_TAB);
            self.conflict_tab.view = None;
            self.conflict_tab.editing = None;
            return;
        }
        Self::put_git_tab(session, CONFLICT_TAB, self.conflict_title(session));
        if raised != self.conflict_tab.seen_raised {
            self.conflict_tab.seen_raised = raised;
            self.conflict_tab.current = None;
            self.conflict_tab.scroll_x = 0.0;
            self.conflict_tab.editing = None;
            self.conflict_tab.view = None;
            zen_workbench::workspaces::bring(session, GIT, CONFLICT_TAB);
            self.close_text_dialogs();
        }
    }

    fn read_conflict(&mut self, registry: &Registry, session: &mut Session) {
        let revision = session.state::<ConflictEditor>().map_or(0, ConflictEditor::revision);
        if self.conflict_tab.view.is_some() && revision == self.conflict_tab.seen_revision {
            return;
        }
        let Some(view) = registry
            .dispatch(session, "vcs.conflict.get", json!({}))
            .ok()
            .and_then(|view| serde_json::from_value::<ConflictView>(view).ok())
        else {
            return;
        };
        let highlighter = self.highlighter_at(&view.path);
        let tab = &mut self.conflict_tab;
        tab.seen_revision = revision;
        tab.ours = Text::new(view.ours.text.as_deref().unwrap_or_default(), highlighter.as_ref());
        tab.theirs = Text::new(view.theirs.text.as_deref().unwrap_or_default(), highlighter.as_ref());
        tab.result = Text::new(view.result.as_deref().unwrap_or_default(), highlighter.as_ref());
        if tab.current.is_none() {
            tab.current = first_conflict(&view, 0, true);
            tab.scroll_to = tab.current;
        }
        tab.view = Some(view);
    }

    pub(in crate::ide) fn conflict_editor(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        self.caret = None;
        let full = ui.available_rect_before_wrap();
        ui.allocate_rect(full, Sense::hover());
        ui.painter().rect_filled(full, 0, p.editor_background);
        self.read_conflict(registry, session);
        let Some(view) = self.conflict_tab.view.clone() else { return };
        let operation = self.git_workspace.operation_title();
        let mut asks = Vec::new();
        let toolbar = Rect::from_min_size(full.min, vec2(full.width(), TOOLBAR));
        conflict_toolbar(ui, toolbar, &p, &view, operation.as_deref(), &mut self.conflict_tab, &mut asks);
        ui.painter().hline(full.x_range(), toolbar.bottom() - 0.5, Stroke::new(1.0, p.frame));
        let body = Rect::from_min_max(pos2(full.left(), toolbar.bottom()), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).id_salt("cf-body"));
        if view.mergeable {
            conflict_panes(&mut child, &p, &view, &mut self.conflict_tab, &mut asks);
        } else {
            whole_file(&mut child, &p, &view, &mut asks);
        }
        confirm_dialog(ui, &p, &view, &mut self.conflict_tab, &mut asks);
        for ask in asks {
            let params = match ask {
                Ask::Resolve(params) => params,
                Ask::Text(text) => json!({ "text": text }),
            };
            let marks = params["markResolved"] == json!(true)
                || params.get("change").is_none() && params.get("take").is_some() && params.get("conflicts").is_none();
            self.dispatch(ui, registry, session, "vcs.conflict.resolve", params);
            if marks {
                self.git_workspace.conflicts_changed();
                let _ = registry.dispatch(session, "vcs.status.refresh", json!({}));
            }
        }
    }
}

fn first_conflict(view: &ConflictView, from: usize, inclusive: bool) -> Option<usize> {
    let unresolved = |change: &&ConflictChange| change.state == "unresolved";
    view.changes
        .iter()
        .filter(unresolved)
        .find(|change| if inclusive { change.index >= from } else { change.index > from })
        .or_else(|| view.changes.iter().find(unresolved))
        .map(|change| change.index)
}

fn conflict_toolbar(
    ui: &mut Ui,
    rect: Rect,
    p: &IdePalette,
    view: &ConflictView,
    operation: Option<&str>,
    tab: &mut ConflictTab,
    asks: &mut Vec<Ask>,
) {
    let painter = ui.painter().clone();
    let (folder, name) = look::split_path(&view.relative);
    let x = rect.left() + 16.0;
    let title = painter.text(pos2(x, rect.center().y - 9.0), Align2::LEFT_CENTER, name, theme::semibold(15.0), p.text);
    if !folder.is_empty() {
        painter.text(
            pos2(title.right() + 8.0, rect.center().y - 9.0),
            Align2::LEFT_CENTER,
            folder.trim_end_matches('/'),
            theme::regular(12.5),
            p.text_secondary,
        );
    }
    let caption = operation.map(str::to_owned).unwrap_or_else(|| t!("vcs-conflict-caption"));
    painter.text(pos2(x, rect.center().y + 10.0), Align2::LEFT_CENTER, caption, theme::regular(12.5), p.text_secondary);
    let mut right = rect.right() - 14.0;
    let mut button = |ui: &mut Ui, label: String, enabled: bool, primary: bool, help: String| {
        let galley = ui.painter().layout_no_wrap(label.clone(), theme::regular(13.0), p.text);
        let width = galley.size().x + 28.0;
        let area = Rect::from_min_max(pos2(right - width, rect.center().y - 15.0), pos2(right, rect.center().y + 15.0));
        right = area.left() - 8.0;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area));
        let color = match (enabled, primary) {
            (false, _) => p.text_secondary,
            (true, true) => p.accent_text,
            (true, false) => p.text,
        };
        let fill = match (enabled, primary) {
            (true, true) => p.accent,
            (false, true) => p.hover,
            _ => Color32::TRANSPARENT,
        };
        let mut widget = egui::Button::new(egui::RichText::new(label).font(theme::regular(13.0)).color(color))
            .fill(fill)
            .corner_radius(6)
            .min_size(area.size());
        if !primary {
            widget = widget.stroke(Stroke::new(1.0, p.button_border));
        }
        child.add_enabled(enabled, widget).on_hover_text(help).clicked()
    };
    if view.mergeable && button(ui, t!("vcs-conflict-mark-resolved"), true, true, t!("vcs-conflict-mark-resolved-help"))
    {
        if view.unresolved > 0 {
            tab.confirm = true;
        } else {
            asks.push(Ask::Resolve(json!({ "markResolved": true })));
        }
    }
    let theirs = t!("vcs-conflict-accept-theirs");
    if button(ui, theirs, view.theirs.exists || view.ours.exists, false, t!("vcs-conflict-accept-theirs-help")) {
        asks.push(Ask::Resolve(json!({ "take": "theirs" })));
    }
    if button(ui, t!("vcs-conflict-accept-ours"), true, false, t!("vcs-conflict-accept-ours-help")) {
        asks.push(Ask::Resolve(json!({ "take": "ours" })));
    }
    if !view.mergeable {
        return;
    }
    let (next, previous) = ui.input_mut(|input| {
        (
            input.consume_key(egui::Modifiers::NONE, egui::Key::F7),
            input.consume_key(egui::Modifiers::SHIFT, egui::Key::F7),
        )
    });
    let mut x = right - 8.0;
    let count = if view.unresolved == 0 {
        t!("vcs-conflict-all-resolved")
    } else {
        t!("vcs-conflict-left", left = view.unresolved, count = view.conflicts)
    };
    let color = if view.unresolved == 0 { p.vcs_added } else { p.text_secondary };
    let galley = painter.layout_no_wrap(count, theme::regular(12.5), color);
    x -= galley.size().x;
    let min_x = title.right().max(rect.left() + 200.0) + 16.0;
    if x - 60.0 < min_x {
        return;
    }
    painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    x -= 10.0;
    let mut step = |forward: bool| {
        let from = tab.current.unwrap_or(0);
        let found = if forward {
            first_conflict(view, from, false)
        } else {
            view.changes.iter().rev().filter(|c| c.state == "unresolved").find(|c| c.index < from).map(|c| c.index)
        };
        let found = found.or_else(|| {
            let count = view.changes.len();
            (count > 0).then(|| if forward { (from + 1).min(count - 1) } else { from.saturating_sub(1) })
        });
        if let Some(found) = found {
            tab.current = Some(found);
            tab.scroll_to = Some(found);
        }
    };
    for (forward, shortcut) in [(true, "F7"), (false, "⇧F7")] {
        let button = Rect::from_min_size(pos2(x - 24.0, rect.center().y - 12.0), Vec2::splat(24.0));
        let response = ui.interact(button, ui.scope_id().with(("cf-step", forward)), Sense::click());
        hover(ui, &response, p);
        let name = if forward { t!("vcs-conflict-next") } else { t!("vcs-conflict-previous") };
        hint(ui, &response, &name, Some(shortcut), Side::Below);
        arrow(ui.painter(), button.shrink(7.0), forward, p.text_secondary);
        if response.clicked() {
            step(forward);
        }
        x = button.left() - 2.0;
    }
    if next || previous {
        step(next);
    }
}

fn fill(p: &IdePalette, change: &ConflictChange, pane: Pane) -> Option<Color32> {
    let conflict = look::mix(p.editor_background, p.vcs_conflict, 0.2);
    if change.state == "unresolved" {
        return Some(conflict);
    }
    match pane {
        Pane::Ours => matches!(change.kind.as_str(), "ours" | "both" | "conflict").then_some(p.diff_modified),
        Pane::Theirs => matches!(change.kind.as_str(), "theirs" | "both" | "conflict").then_some(p.diff_modified),
        Pane::Result => Some(match change.state.as_str() {
            "ours" | "theirs" | "both" => p.diff_inserted,
            _ => p.diff_modified,
        }),
    }
}

fn conflict_panes(ui: &mut Ui, p: &IdePalette, view: &ConflictView, tab: &mut ConflictTab, asks: &mut Vec<Ask>) {
    let segments = segments(view, tab.ours.lines.len(), tab.theirs.lines.len(), tab.result.lines.len());
    let draft_lines =
        tab.editing.as_ref().map(|editing| (editing.segment, editing.draft.split_inclusive('\n').count().max(1)));
    let rows = rows(&segments, draft_lines);
    let frame = ui.available_rect_before_wrap();
    let inner = Rect::from_min_max(frame.min, pos2(frame.right() - MARKS, frame.bottom()));
    let pane = (inner.width() - 2.0 * GUTTER) / 3.0;
    let left = |which: Pane| -> f32 {
        inner.left()
            + match which {
                Pane::Ours => 0.0,
                Pane::Result => pane + GUTTER,
                Pane::Theirs => 2.0 * (pane + GUTTER),
            }
    };
    let titles = Rect::from_min_size(inner.min, vec2(inner.width(), TITLES));
    let short = |commit: &Option<String>| commit.as_deref().map(|id| id.chars().take(7).collect::<String>());
    for (which, caption, label, commit) in [
        (Pane::Ours, t!("vcs-conflict-ours"), view.ours.label.clone(), short(&view.ours.commit)),
        (Pane::Result, t!("vcs-conflict-result"), String::new(), None),
        (Pane::Theirs, t!("vcs-conflict-theirs"), view.theirs.label.clone(), short(&view.theirs.commit)),
    ] {
        let x = left(which) + 12.0;
        let painter =
            ui.painter().with_clip_rect(Rect::from_min_size(pos2(left(which), titles.top()), vec2(pane, TITLES)));
        let (galley, shortened) =
            look::small_caps_fitted(&painter, &caption, 10.5, p.text_secondary, (pane - 24.0).max(0.0));
        let width = galley.size().x;
        let at = pos2(x, titles.center().y - galley.size().y / 2.0);
        if shortened {
            let span = Rect::from_min_size(at, galley.size());
            let response = ui.interact(span, ui.scope_id().with(("conflict-pane-title", which)), Sense::hover());
            hint(ui, &response, &caption.to_uppercase(), None, Side::Below);
        }
        painter.galley(at, galley, p.text_secondary);
        let mut x = x + width + 8.0;
        if !label.is_empty() {
            let rect =
                painter.text(pos2(x, titles.center().y), Align2::LEFT_CENTER, &label, theme::regular(12.5), p.text);
            x = rect.right() + 6.0;
        }
        if let Some(commit) = commit.filter(|commit| !label.starts_with(commit.as_str())) {
            painter.text(
                pos2(x, titles.center().y),
                Align2::LEFT_CENTER,
                commit,
                FontId::monospace(11.5),
                p.text_secondary,
            );
        }
    }
    ui.painter().hline(inner.x_range(), titles.bottom() - 0.5, Stroke::new(1.0, p.frame));
    let area_rect = Rect::from_min_max(pos2(inner.left(), titles.bottom()), inner.max);
    let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
    let widest = tab
        .ours
        .lines
        .iter()
        .chain(&tab.theirs.lines)
        .chain(&tab.result.lines)
        .map(|(_, line)| line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>())
        .max()
        .unwrap_or(0) as f32
        * char_width;
    let room = pane - NUMBERS - 4.0;
    let max_x = (widest + 2.0 * char_width - room).max(0.0);
    sideways(ui, area_rect, &mut tab.scroll_x, max_x);
    let scroll_row = tab.scroll_to.take().and_then(|change| {
        let segment = segments.iter().position(|segment| segment.change == Some(change))?;
        rows.iter().position(|row| row.segment == segment)
    });
    let mut area = egui::ScrollArea::vertical().id_salt("cf-rows").auto_shrink([false, false]);
    if let Some(row) = scroll_row {
        area = area.vertical_scroll_offset((row as f32 * ROW - 3.0 * ROW).max(0.0));
    }
    let mut start_editing: Option<usize> = None;
    let mut finish_editing = false;
    let mut cancel_editing = false;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(area_rect).id_salt("cf-scroll"));
    area.show_rows(&mut child, ROW, rows.len(), |ui, range| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let top = ui.cursor().top() - range.start as f32 * ROW;
        let width = ui.available_width();
        for index in range.clone() {
            let row = rows[index];
            let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
            let segment = &segments[row.segment];
            let change = segment.change.and_then(|change| view.changes.get(change));
            let editing_here = tab.editing.as_ref().is_some_and(|editing| editing.segment == row.segment);
            for (which, text, lines) in [
                (Pane::Ours, &tab.ours, &segment.ours),
                (Pane::Result, &tab.result, &segment.result),
                (Pane::Theirs, &tab.theirs, &segment.theirs),
            ] {
                let cell = Rect::from_min_size(pos2(left(which), rect.top()), vec2(pane, ROW));
                let line = (row.offset < lines.len()).then(|| lines.start + row.offset);
                if let Some(change) = change {
                    let color = if line.is_some() || which == Pane::Result && editing_here {
                        fill(p, change, which)
                    } else {
                        Some(p.diff_filler)
                    };
                    if let Some(color) = color {
                        ui.painter().rect_filled(cell, 0, color);
                    }
                    if Some(change.index) == tab.current && which == Pane::Result {
                        ui.painter().rect_filled(Rect::from_min_size(cell.min, vec2(3.0, ROW)), 0, p.accent);
                    }
                }
                if which == Pane::Result && editing_here {
                    continue;
                }
                let Some(line) = line else { continue };
                ui.painter().with_clip_rect(cell).text(
                    pos2(cell.left() + NUMBERS - 8.0, cell.center().y),
                    Align2::RIGHT_CENTER,
                    (line + 1).to_string(),
                    FontId::monospace(12.0),
                    p.gutter_text,
                );
                if let Some((start, content)) = text.lines.get(line) {
                    let job = line_job(content, *start, &text.spans, &[], p, Color32::TRANSPARENT);
                    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                    let text_rect = Rect::from_min_max(pos2(cell.left() + NUMBERS + 4.0, cell.top()), cell.max);
                    ui.painter().with_clip_rect(text_rect.intersect(ui.clip_rect())).galley(
                        pos2(text_rect.left() - tab.scroll_x, text_rect.center().y - galley.size().y / 2.0),
                        galley,
                        p.code,
                    );
                }
            }
            let result_cell = Rect::from_min_size(pos2(left(Pane::Result), rect.top()), vec2(pane, ROW));
            if !editing_here {
                let response = ui.interact(result_cell, ui.scope_id().with(("cf-result", index)), Sense::click());
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                }
                if response.clicked() {
                    start_editing = Some(row.segment);
                }
            }
            if let Some(change) = change {
                for (gutter_left, pane_of) in
                    [(left(Pane::Ours) + pane, Pane::Ours), (left(Pane::Result) + pane, Pane::Theirs)]
                {
                    let band =
                        Rect::from_min_max(pos2(gutter_left, rect.top()), pos2(gutter_left + GUTTER, rect.bottom()));
                    if let Some(color) =
                        fill(p, change, if change.state == "unresolved" { pane_of } else { Pane::Result })
                    {
                        ui.painter().rect_filled(band.shrink2(vec2(8.0, 0.0)), 0, color.gamma_multiply(0.8));
                    }
                }
                if row.offset == 0 {
                    take_buttons(ui, p, rect, left(Pane::Ours) + pane, left(Pane::Result) + pane, change, asks, tab);
                }
            }
        }
        if let Some(editing) = &mut tab.editing {
            let Some(first) = rows.iter().position(|row| row.segment == editing.segment) else { return };
            let count = rows.iter().filter(|row| row.segment == editing.segment).count().max(1);
            let rect = Rect::from_min_size(
                pos2(left(Pane::Result) + NUMBERS + 4.0, top + first as f32 * ROW),
                vec2(pane - NUMBERS - 6.0, count as f32 * ROW),
            );
            ui.painter().rect_stroke(rect.expand(1.0), 3, Stroke::new(1.0, p.accent), egui::StrokeKind::Outside);
            let code = p.code;
            let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, _wrap: f32| {
                let mut job = LayoutJob::simple(text.as_str().to_owned(), FontId::monospace(13.0), code, f32::INFINITY);
                for section in &mut job.sections {
                    section.format.line_height = Some(ROW);
                    section.format.valign = egui::Align::Center;
                }
                ui.fonts_mut(|fonts| fonts.layout_job(job))
            };
            let response = ui.put(
                rect,
                egui::TextEdit::multiline(&mut editing.draft)
                    .id_salt("cf-draft")
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(rect.width())
                    .layouter(&mut layouter),
            );
            if std::mem::take(&mut editing.focus) {
                response.request_focus();
            }
            if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                cancel_editing = true;
            } else if response.lost_focus() {
                finish_editing = true;
            }
        }
    });
    if cancel_editing {
        tab.editing = None;
    } else if (finish_editing || start_editing.is_some() && tab.editing.is_some())
        && let Some(editing) = tab.editing.take()
    {
        let text = edited_result(&segments, &tab.result, editing.segment, &editing.draft);
        if text != view.result.clone().unwrap_or_default() {
            asks.push(Ask::Text(text));
        }
    }
    if let Some(segment) = start_editing.filter(|_| tab.editing.is_none() && asks.is_empty()) {
        let draft = tab.result.raw[segments[segment].result.clone()].concat();
        tab.editing = Some(Editing { segment, draft, focus: true });
    }
    if max_x > 0.0 {
        let texts: Vec<Rect> = [Pane::Ours, Pane::Result, Pane::Theirs]
            .into_iter()
            .map(|which| {
                Rect::from_min_max(
                    pos2(left(which) + NUMBERS + 4.0, area_rect.top()),
                    pos2(left(which) + pane, area_rect.bottom()),
                )
            })
            .collect();
        scroll_bars(ui, p, &texts, &mut tab.scroll_x, room, max_x, "cf-hbar");
    }
    change_marks(ui, p, frame, view, &segments, &rows, tab);
}

fn edited_result(segments: &[Segment], result: &Text, segment: usize, draft: &str) -> String {
    let mut text = String::new();
    for (index, each) in segments.iter().enumerate() {
        let lines = &result.raw[each.result.start.min(result.raw.len())..each.result.end.min(result.raw.len())];
        if index == segment {
            let mut draft = draft.to_owned();
            let had_break = lines.last().is_some_and(|line| line.ends_with('\n'));
            let more = segments[index + 1..].iter().any(|later| !later.result.is_empty());
            if !draft.is_empty() && !draft.ends_with('\n') && (had_break || more) {
                draft.push('\n');
            }
            text.push_str(&draft);
        } else {
            text.push_str(&lines.concat());
        }
    }
    text
}

#[allow(clippy::too_many_arguments, reason = "one row's buttons: where both gutters are, and the change")]
fn take_buttons(
    ui: &Ui,
    p: &IdePalette,
    rect: Rect,
    left_gutter: f32,
    right_gutter: f32,
    change: &ConflictChange,
    asks: &mut Vec<Ask>,
    tab: &mut ConflictTab,
) {
    for (gutter, ours) in [(left_gutter, true), (right_gutter, false)] {
        let button = Rect::from_center_size(pos2(gutter + GUTTER / 2.0, rect.center().y), vec2(22.0, 18.0));
        let response = ui.interact(button, ui.scope_id().with(("cf-take", change.index, ours)), Sense::click());
        let taken = change.state == if ours { "ours" } else { "theirs" } || change.state == "both";
        let other = change.state == if ours { "theirs" } else { "ours" };
        let take = if other {
            "both"
        } else if ours {
            "ours"
        } else {
            "theirs"
        };
        let conflict = change.state == "unresolved";
        let color = if taken {
            p.text_secondary.gamma_multiply(0.5)
        } else if conflict {
            p.text
        } else {
            p.text_secondary
        };
        ui.painter().rect_filled(button, 4, if response.hovered() { p.hover } else { p.editor_background });
        ui.painter().rect_stroke(button, 4, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
        ui.painter().text(
            button.center(),
            Align2::CENTER_CENTER,
            if ours { "»" } else { "«" },
            theme::semibold(14.0),
            color,
        );
        let name = match (take, ours) {
            ("both", _) => t!("vcs-conflict-take-both"),
            (_, true) => t!("vcs-conflict-take-ours"),
            _ => t!("vcs-conflict-take-theirs"),
        };
        hint(ui, &response, &name, None, Side::Below);
        if response.clicked() && !taken {
            tab.current = Some(change.index);
            asks.push(Ask::Resolve(json!({ "change": change.index, "take": take })));
        }
    }
}

fn change_marks(
    ui: &Ui,
    p: &IdePalette,
    frame: Rect,
    view: &ConflictView,
    segments: &[Segment],
    rows: &[Row],
    tab: &mut ConflictTab,
) {
    let track = Rect::from_min_max(
        pos2(frame.right() - MARKS + 3.0, frame.top() + TITLES + 2.0),
        pos2(frame.right() - 2.0, frame.bottom() - 2.0),
    );
    let total = rows.len().max(1) as f32;
    for (index, segment) in segments.iter().enumerate() {
        let Some(change) = segment.change.and_then(|change| view.changes.get(change)) else { continue };
        let (Some(first), Some(last)) =
            (rows.iter().position(|row| row.segment == index), rows.iter().rposition(|row| row.segment == index))
        else {
            continue;
        };
        let top = track.top() + track.height() * first as f32 / total;
        let bottom = (track.top() + track.height() * (last + 1) as f32 / total).max(top + 3.0);
        let rect = Rect::from_min_max(pos2(track.left(), top), pos2(track.right(), bottom));
        let color = if change.state == "unresolved" { p.vcs_conflict } else { p.gutter_modified };
        ui.painter().rect_filled(rect, 1, color);
        let response =
            ui.interact(rect.expand2(vec2(1.0, 2.0)), ui.scope_id().with(("cf-mark", index)), Sense::click());
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() {
            tab.current = Some(change.index);
            tab.scroll_to = Some(change.index);
        }
    }
}

fn whole_file(ui: &mut Ui, p: &IdePalette, view: &ConflictView, asks: &mut Vec<Ask>) {
    let rect = ui.available_rect_before_wrap();
    let card =
        Rect::from_center_size(pos2(rect.center().x, rect.top() + 150.0), vec2(rect.width().min(520.0) - 32.0, 180.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card).id_salt("cf-whole"));
    child.spacing_mut().item_spacing.y = 0.0;
    let what = match view.kind {
        ConflictKind::DeletedByThem => t!("vcs-conflict-deleted-by-them", theirs = view.theirs.label.as_str()),
        ConflictKind::DeletedByUs => t!("vcs-conflict-deleted-by-us", ours = view.ours.label.as_str()),
        ConflictKind::AddedByUs | ConflictKind::AddedByThem | ConflictKind::BothDeleted => t!("vcs-conflict-one-side"),
        _ => t!("vcs-conflict-binary"),
    };
    child.label(egui::RichText::new(t!("vcs-conflict-whole-title")).font(theme::semibold(15.0)).color(p.text));
    child.add_space(10.0);
    child.add(egui::Label::new(egui::RichText::new(what).font(theme::regular(13.0)).color(p.text)).wrap());
    child.add_space(8.0);
    child.add(
        egui::Label::new(
            egui::RichText::new(t!("vcs-conflict-whole-help")).font(theme::regular(12.5)).color(p.text_secondary),
        )
        .wrap(),
    );
    child.add_space(16.0);
    child.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (take, exists, label) in [
            ("ours", view.ours.exists, t!("vcs-conflict-keep-ours", ours = view.ours.label.as_str())),
            ("theirs", view.theirs.exists, t!("vcs-conflict-keep-theirs", theirs = view.theirs.label.as_str())),
        ] {
            let label = if exists { label } else { t!("vcs-conflict-delete-file") };
            let button = egui::Button::new(egui::RichText::new(label).font(theme::regular(13.0)).color(p.text))
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border))
                .corner_radius(6)
                .min_size(vec2(0.0, 30.0));
            if ui.add(button).clicked() {
                asks.push(Ask::Resolve(json!({ "take": take })));
            }
        }
    });
}

fn confirm_dialog(ui: &Ui, p: &IdePalette, view: &ConflictView, tab: &mut ConflictTab, asks: &mut Vec<Ask>) {
    if !tab.confirm {
        return;
    }
    let (mut yes, mut no) = (false, false);
    let modal =
        egui::Modal::new(ui.scope_id().with("cf-confirm")).frame(super::super::dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("vcs-conflict-confirm-title")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(12.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t!("vcs-conflict-confirm-text", count = view.unresolved))
                        .font(theme::regular(13.0))
                        .color(p.text),
                )
                .wrap(),
            );
            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let ok = egui::Button::new(egui::RichText::new(t!("vcs-conflict-confirm-ok")).color(p.accent_text))
                    .fill(p.accent)
                    .corner_radius(6)
                    .min_size(vec2(0.0, 28.0));
                yes = ui.add(ok).clicked() || ui.input(|input| input.key_pressed(egui::Key::Enter));
                let cancel = egui::Button::new(t!("settings-cancel"))
                    .stroke(Stroke::new(1.0, p.button_border))
                    .corner_radius(6)
                    .min_size(vec2(0.0, 28.0));
                no = ui.add(cancel).clicked();
            });
        });
    if modal.should_close() || no {
        tab.confirm = false;
    }
    if yes {
        tab.confirm = false;
        asks.push(Ask::Resolve(json!({ "markResolved": true, "force": true })));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(index: usize, ours: [usize; 2], theirs: [usize; 2], result: [usize; 2]) -> ConflictChange {
        ConflictChange {
            index,
            kind: "conflict".into(),
            state: "unresolved".into(),
            ours,
            theirs,
            result,
            ..ConflictChange::default()
        }
    }

    #[test]
    fn segments_line_the_panes_up_and_take_every_line_once() {
        let view = ConflictView {
            changes: vec![change(0, [1, 3], [1, 2], [1, 2]), change(1, [5, 6], [4, 4], [4, 5])],
            ..ConflictView::default()
        };
        let segments = segments(&view, 6, 4, 5);
        let ranges: Vec<_> =
            segments.iter().map(|s| (s.change, s.ours.clone(), s.theirs.clone(), s.result.clone())).collect();
        assert_eq!(
            ranges,
            [
                (None, 0..1, 0..1, 0..1),
                (Some(0), 1..3, 1..2, 1..2),
                (None, 3..5, 2..4, 2..4),
                (Some(1), 5..6, 4..4, 4..5)
            ]
        );
        let rows = rows(&segments, None);
        assert_eq!(rows.len(), 1 + 2 + 2 + 1, "a change takes the most lines a pane has of it");
        assert_eq!(super::rows(&segments, Some((1, 4))).len(), 1 + 4 + 2 + 1);
        let empty = ConflictView { changes: vec![change(0, [1, 1], [1, 1], [1, 1])], ..ConflictView::default() };
        let segments = super::segments(&empty, 2, 2, 2);
        assert_eq!(super::rows(&segments, None).len(), 3);
    }

    #[test]
    fn the_result_is_edited_in_place_and_a_side_taken_with_its_button() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let view = ConflictView {
            mergeable: true,
            result: Some("a\nb\nc\n".into()),
            changes: vec![change(0, [1, 2], [1, 2], [1, 2])],
            conflicts: 1,
            unresolved: 1,
            ..ConflictView::default()
        };
        let mut tab = ConflictTab {
            ours: Text::new("a\nO\nc\n", None),
            theirs: Text::new("a\nT\nc\n", None),
            result: Text::new("a\nb\nc\n", None),
            ..ConflictTab::default()
        };
        let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let ctx = egui::Context::default();
        let frame = |events: Vec<egui::Event>, tab: &mut ConflictTab| -> Vec<Ask> {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut asks = Vec::new();
            let mut output = ctx.run_ui(input, |ui| conflict_panes(ui, &palette, &view, tab, &mut asks));
            output.textures_delta.clear();
            asks
        };
        let click = |at: egui::Pos2| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        frame(Vec::new(), &mut tab);
        let row = TITLES + ROW * 1.5;
        assert!(frame(click(pos2(560.0, row)), &mut tab).is_empty());
        assert_eq!(tab.editing.as_ref().map(|editing| (editing.segment, editing.draft.as_str())), Some((1, "b\n")));
        frame(Vec::new(), &mut tab);
        frame(vec![egui::Event::Text("x".into())], &mut tab);
        assert_eq!(
            tab.editing.as_ref().map(|editing| editing.draft.as_str()),
            Some("b\nx"),
            "typed where the caret is: at the end"
        );
        let asks = frame(click(pos2(560.0, TITLES + ROW * 0.5)), &mut tab);
        let texts: Vec<String> =
            asks.iter().filter_map(|ask| if let Ask::Text(text) = ask { Some(text.clone()) } else { None }).collect();
        assert_eq!(texts, ["a\nb\nx\nc\n"], "its last line gets its line break");
        assert!(tab.editing.is_none());
        let pane = (1200.0 - MARKS - 2.0 * GUTTER) / 3.0;
        let asks = frame(click(pos2(pane + GUTTER / 2.0, row)), &mut tab);
        let resolves: Vec<serde_json::Value> = asks
            .iter()
            .filter_map(|ask| if let Ask::Resolve(params) = ask { Some(params.clone()) } else { None })
            .collect();
        assert_eq!(resolves, [json!({ "change": 0, "take": "ours" })]);
    }

    #[test]
    fn a_segment_typed_over_keeps_the_lines_after_it_apart() {
        let view = ConflictView { changes: vec![change(0, [1, 2], [1, 2], [1, 2])], ..ConflictView::default() };
        let result = Text::new("a\nb\nc\n", None);
        let segments = segments(&view, 3, 3, 3);
        assert_eq!(edited_result(&segments, &result, 1, "B1\nB2"), "a\nB1\nB2\nc\n");
        assert_eq!(edited_result(&segments, &result, 1, ""), "a\nc\n", "the lines taken out");
        assert_eq!(edited_result(&segments, &result, 2, "C"), "a\nb\nC\n");
        assert_eq!(first_conflict(&view, 0, true), Some(0));
        let resolved = ConflictView {
            changes: vec![ConflictChange { state: "ours".into(), ..change(0, [1, 2], [1, 2], [1, 2]) }],
            ..ConflictView::default()
        };
        assert_eq!(first_conflict(&resolved, 0, true), None);
    }
}

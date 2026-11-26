use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_syntax::Highlighter;
use zen_workbench::compare::{ThreeWayView, Whitespace};
use zen_workbench::vcs::commit::{ChangePart, DiffHunk, DiffView};

use super::{IdePalette, Side, hint, hover};
use crate::theme;

const MARKS: f32 = 12.0;
const ROW: f32 = 22.0;
const NUMBERS: f32 = 44.0;
const DIVIDER: f32 = 28.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Row {
    pub(super) before: Option<usize>,
    pub(super) after: Option<usize>,
    pub(super) kind: RowKind,
    pub(super) hunk: Option<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowKind {
    Same,
    Inserted,
    Deleted,
    Modified,
}

pub(super) fn side_by_side_rows(view: &DiffView) -> Vec<Row> {
    let (before_count, after_count) = (line_count(&view.before), line_count(&view.after));
    let mut rows = Vec::new();
    let (mut before, mut after) = (0, 0);
    for (index, hunk) in view.hunks.iter().enumerate() {
        while before < hunk.before_start && after < hunk.after_start {
            rows.push(Row { before: Some(before), after: Some(after), kind: RowKind::Same, hunk: None });
            before += 1;
            after += 1;
        }
        let kind = kind_of(hunk);
        let length = (hunk.before_end - hunk.before_start).max(hunk.after_end - hunk.after_start);
        for offset in 0..length {
            let left = hunk.before_start + offset;
            let right = hunk.after_start + offset;
            rows.push(Row {
                before: (left < hunk.before_end).then_some(left),
                after: (right < hunk.after_end).then_some(right),
                kind,
                hunk: Some(index),
            });
        }
        before = hunk.before_end;
        after = hunk.after_end;
    }
    while before < before_count || after < after_count {
        rows.push(Row {
            before: (before < before_count).then_some(before),
            after: (after < after_count).then_some(after),
            kind: RowKind::Same,
            hunk: None,
        });
        before += 1;
        after += 1;
    }
    rows
}

pub(super) fn unified_rows(view: &DiffView) -> Vec<Row> {
    let after_count = line_count(&view.after);
    let mut rows = Vec::new();
    let (mut before, mut after) = (0, 0);
    for (index, hunk) in view.hunks.iter().enumerate() {
        while after < hunk.after_start {
            rows.push(Row { before: Some(before), after: Some(after), kind: RowKind::Same, hunk: None });
            before += 1;
            after += 1;
        }
        for line in hunk.before_start..hunk.before_end {
            rows.push(Row { before: Some(line), after: None, kind: RowKind::Deleted, hunk: Some(index) });
        }
        for line in hunk.after_start..hunk.after_end {
            rows.push(Row { before: None, after: Some(line), kind: RowKind::Inserted, hunk: Some(index) });
        }
        before = hunk.before_end;
        after = hunk.after_end;
    }
    while after < after_count {
        rows.push(Row { before: Some(before), after: Some(after), kind: RowKind::Same, hunk: None });
        before += 1;
        after += 1;
    }
    rows
}

fn kind_of(hunk: &DiffHunk) -> RowKind {
    match hunk.kind.as_str() {
        "added" => RowKind::Inserted,
        "deleted" => RowKind::Deleted,
        _ => RowKind::Modified,
    }
}

fn line_count(text: &str) -> usize {
    text.split_inclusive('\n').count()
}

pub(super) struct DiffTab {
    pub(super) view: DiffView,
    pub(super) unified: bool,
    pub(super) swapped: bool,
    pub(super) whitespace: Whitespace,
    pub(super) three: Option<ThreeWayView>,
    current: usize,
    scroll_to: Option<usize>,
    before_lines: Vec<(usize, String)>,
    after_lines: Vec<(usize, String)>,
    before_spans: Vec<zen_syntax::Span>,
    after_spans: Vec<zen_syntax::Span>,
    three_lines: Vec<Vec<(usize, String)>>,
    three_spans: Vec<Vec<zen_syntax::Span>>,
    highlighter: Option<Arc<dyn Highlighter>>,
    file: Option<(PathBuf, Option<[usize; 2]>, bool)>,
    synced: Option<(u64, zen_text::Text)>,
    layouts: [Option<PaneLayout>; 2],
    marks: u64,
    typed_at: Option<f64>,
    three_widest: usize,
    tops: [Vec<(f32, f32)>; 2],
    scroll_x: f32,
    chosen_added: BTreeSet<usize>,
    chosen_removed: BTreeSet<usize>,
    anchor: Option<(bool, usize)>,
    caret: Option<(bool, usize)>,
    focus: Option<egui::Id>,
    pub(super) revision: Option<u64>,
}

pub(super) type SideWrite = (PathBuf, Option<[usize; 2]>, String);

const RETYPED_AFTER: f64 = 0.25;

struct PaneLayout {
    text: String,
    wrap: f32,
    marks: u64,
    palette: IdePalette,
    job: LayoutJob,
}

#[derive(Default)]
pub(super) struct DiffOutcome {
    pub(super) switched: bool,
    pub(super) ask: Option<(&'static str, serde_json::Value)>,
    pub(super) rehunk: bool,
    pub(super) write: Option<SideWrite>,
}

fn other_side_line(hunks: &[DiffHunk], after: bool, line: usize) -> usize {
    let mut shift: isize = 0;
    for hunk in hunks {
        let (start, end, other_start, other_end) = if after {
            (hunk.after_start, hunk.after_end, hunk.before_start, hunk.before_end)
        } else {
            (hunk.before_start, hunk.before_end, hunk.after_start, hunk.after_end)
        };
        if line < start {
            break;
        }
        if line < end {
            let into = line - start;
            return other_start + into.min(other_end.saturating_sub(other_start + 1));
        }
        shift = other_end as isize - end as isize;
    }
    line.saturating_add_signed(shift)
}

fn caret_side(caret: Option<(bool, usize)>, row: Row, unified: bool) -> Option<bool> {
    let (after, line) = caret?;
    let on = if after { row.after == Some(line) } else { row.before == Some(line) };
    let shown = !unified || after || row.after.is_none();
    (on && shown).then_some(after)
}

fn caret_rect(rect: Rect, after: bool, unified: bool) -> Rect {
    if unified {
        return rect;
    }
    let half = (rect.width() - DIVIDER) / 2.0;
    if after {
        Rect::from_min_size(pos2(rect.left() + half + DIVIDER, rect.top()), vec2(half, ROW))
    } else {
        Rect::from_min_size(rect.min, vec2(half, ROW))
    }
}

fn split_lines(text: &str) -> Vec<(usize, String)> {
    let mut offset = 0;
    text.split_inclusive('\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, line.trim_end_matches(['\n', '\r']).to_owned())
        })
        .collect()
}

fn swapped_hunk(hunk: &DiffHunk) -> DiffHunk {
    let kind = match hunk.kind.as_str() {
        "added" => "deleted",
        "deleted" => "added",
        other => other,
    };
    DiffHunk {
        kind: kind.to_owned(),
        before_start: hunk.after_start,
        before_end: hunk.after_end,
        after_start: hunk.before_start,
        after_end: hunk.before_end,
        before_words: hunk.after_words.clone(),
        after_words: hunk.before_words.clone(),
    }
}

impl DiffTab {
    pub(super) fn new(view: DiffView, highlighter: Option<Arc<dyn Highlighter>>, unified: bool) -> Self {
        let file = (view.after_editable && !view.path.as_os_str().is_empty())
            .then(|| (view.path.clone(), view.after_range, true));
        let mut tab = Self {
            before_lines: Vec::new(),
            after_lines: Vec::new(),
            before_spans: Vec::new(),
            after_spans: Vec::new(),
            three_lines: Vec::new(),
            three_spans: Vec::new(),
            highlighter,
            file,
            synced: None,
            layouts: [None, None],
            marks: 0,
            typed_at: None,
            three_widest: 0,
            tops: [Vec::new(), Vec::new()],
            current: 0,
            scroll_to: (!view.hunks.is_empty()).then_some(0),
            scroll_x: 0.0,
            unified,
            swapped: false,
            whitespace: Whitespace::None,
            three: None,
            chosen_added: BTreeSet::new(),
            chosen_removed: BTreeSet::new(),
            anchor: None,
            caret: None,
            focus: None,
            revision: None,
            view,
        };
        tab.refresh_texts();
        tab
    }

    pub(super) fn three(three: ThreeWayView, highlighter: Option<Arc<dyn Highlighter>>) -> Self {
        let middle = three.paths.get(1).cloned().unwrap_or_default();
        let view = DiffView {
            relative: middle.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
            path: middle,
            binary: three.binary,
            ..DiffView::default()
        };
        let mut tab = Self::new(view, highlighter, false);
        tab.three = Some(three);
        tab.refresh_texts();
        tab.scroll_to = (tab.change_count() > 0).then_some(0);
        tab
    }

    fn refresh_texts(&mut self) {
        let spans =
            |text: &str| self.highlighter.as_ref().map(|highlighter| highlighter.highlight(text)).unwrap_or_default();
        self.marks += 1;
        if let Some(three) = &self.three {
            self.three_lines = three.texts.iter().map(|text| split_lines(text)).collect();
            self.three_spans = three.texts.iter().map(|text| spans(text)).collect();
            self.three_widest = self
                .three_lines
                .iter()
                .flatten()
                .map(|(_, line)| line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>())
                .max()
                .unwrap_or(0);
            return;
        }
        self.before_spans = spans(&self.view.before);
        self.after_spans = spans(&self.view.after);
        self.before_lines = split_lines(&self.view.before);
        self.after_lines = split_lines(&self.view.after);
    }

    pub(super) fn replace(&mut self, view: DiffView, highlighter: Option<Arc<dyn Highlighter>>) {
        let (unified, current, scroll_x, revision) = (self.unified, self.current, self.scroll_x, self.revision);
        let (swapped, whitespace) = (self.swapped, self.whitespace);
        *self = DiffTab::new(view, highlighter, unified);
        if swapped {
            self.swap();
        }
        self.whitespace = whitespace;
        self.current = current.min(self.change_count().saturating_sub(1));
        self.scroll_to = None;
        self.scroll_x = scroll_x;
        self.revision = revision;
    }

    pub(super) fn set_hunks(&mut self, hunks: Vec<DiffHunk>, right: Option<Vec<DiffHunk>>) {
        match (&mut self.three, right) {
            (Some(three), Some(right)) => {
                three.left_hunks = hunks;
                three.right_hunks = right;
            }
            _ => self.view.hunks = hunks,
        }
        self.marks += 1;
        self.current = self.current.min(self.change_count().saturating_sub(1));
    }

    pub(super) fn swap(&mut self) {
        self.swapped = !self.swapped;
        if let Some(three) = &mut self.three {
            three.paths.reverse();
            three.titles.reverse();
            three.texts.reverse();
            let left = std::mem::take(&mut three.left_hunks);
            three.left_hunks = three.right_hunks.iter().map(swapped_hunk).collect();
            three.right_hunks = left.iter().map(swapped_hunk).collect();
        } else {
            let view = &mut self.view;
            std::mem::swap(&mut view.before, &mut view.after);
            std::mem::swap(&mut view.before_title, &mut view.after_title);
            std::mem::swap(&mut view.before_editable, &mut view.after_editable);
            view.hunks = view.hunks.iter().map(swapped_hunk).collect();
            std::mem::swap(&mut self.chosen_added, &mut self.chosen_removed);
            self.caret = self.caret.map(|(after, line)| (!after, line));
            if let Some(file) = &mut self.file {
                file.2 = !file.2;
            }
        }
        self.tops = [Vec::new(), Vec::new()];
        self.refresh_texts();
        self.scroll_to = (self.change_count() > 0).then_some(self.current);
    }

    pub(super) fn editable(&self, after: bool) -> bool {
        self.three.is_none()
            && !self.view.binary
            && if after { self.view.after_editable } else { self.view.before_editable }
    }

    pub(super) fn typed(&self) -> bool {
        !self.unified && (self.editable(false) || self.editable(true))
    }

    pub(super) fn set_side(&mut self, after: bool, text: String, title: Option<String>) -> Option<Option<SideWrite>> {
        if !self.editable(after) {
            return None;
        }
        let write = self
            .file
            .as_ref()
            .filter(|file| file.2 == after)
            .map(|(path, range, _)| (path.clone(), *range, text.clone()));
        if let Some(title) = title.filter(|_| write.is_none()) {
            if after { self.view.after_title = title } else { self.view.before_title = title }
        }
        if after {
            self.view.after = text
        } else {
            self.view.before = text
        }
        self.refresh_texts();
        Some(write)
    }

    pub(super) fn written(&mut self, range: [usize; 2], version: u64, text: zen_text::Text) {
        if let Some(file) = &mut self.file
            && file.1.is_some()
        {
            file.1 = Some(range);
        }
        self.synced = Some((version, text));
    }

    pub(super) fn file_path(&self) -> Option<&Path> {
        self.file.as_ref().map(|(path, ..)| path.as_path())
    }

    pub(super) fn follow(&mut self, document: &zen_workbench::Document) -> bool {
        let Some((path, range, after)) = self.file.clone() else { return false };
        if document.path() != path {
            return false;
        }
        let Some((version, before)) = self.synced.take() else {
            self.synced = Some((document.version(), document.snapshot().clone()));
            return false;
        };
        if version == document.version() {
            self.synced = Some((version, before));
            return false;
        }
        self.synced = Some((document.version(), document.snapshot().clone()));
        let range = range.map(|range| moved(&before.to_string(), document.text(), range));
        if let Some(file) = &mut self.file {
            file.1 = range;
        }
        let text = zen_workbench::compare::side_text(document.text(), range);
        let side = if after { &mut self.view.after } else { &mut self.view.before };
        if *side == text {
            return false;
        }
        *side = text;
        self.refresh_texts();
        true
    }

    pub(super) fn texts(&self) -> Vec<String> {
        match &self.three {
            Some(three) => three.texts.clone(),
            None => vec![self.view.before.clone(), self.view.after.clone()],
        }
    }

    pub(super) fn change_count(&self) -> usize {
        match &self.three {
            Some(three) => three.left_hunks.len() + three.right_hunks.len(),
            None => self.view.hunks.len(),
        }
    }

    pub(super) fn state(&self) -> serde_json::Value {
        let common = json!({
            "title": self.title(),
            "unified": self.unified,
            "swapped": self.swapped,
            "whitespace": self.whitespace,
            "current": self.current,
            "differences": self.change_count(),
        });
        let mut state = match &self.three {
            Some(three) => json!({
                "sides": 3,
                "paths": three.paths,
                "titles": three.titles,
                "texts": three.texts,
                "leftHunks": three.left_hunks,
                "rightHunks": three.right_hunks,
            }),
            None => json!({
                "sides": 2,
                "path": self.view.path,
                "titles": [self.view.before_title, self.view.after_title],
                "texts": [self.view.before, self.view.after],
                "editable": [self.editable(false), self.editable(true)],
                "hunks": self.view.hunks,
                "part": self.view.part,
                "range": self.file.as_ref().and_then(|file| file.1),
            }),
        };
        if let (Some(state), Some(common)) = (state.as_object_mut(), common.as_object()) {
            state.extend(common.clone());
        }
        state
    }

    pub(super) fn summary(&self) -> serde_json::Value {
        let mut state = self.state();
        if let Some(state) = state.as_object_mut() {
            for key in ["texts", "hunks", "leftHunks", "rightHunks"] {
                state.remove(key);
            }
        }
        state
    }

    fn stages(&self) -> bool {
        self.view.part.is_some() && !self.swapped && self.whitespace == Whitespace::None && self.three.is_none()
    }

    fn is_changed(&self, after: bool, line: usize) -> bool {
        self.view.hunks.iter().any(|hunk| {
            if after {
                (hunk.after_start..hunk.after_end).contains(&line)
            } else {
                (hunk.before_start..hunk.before_end).contains(&line)
            }
        })
    }

    pub(super) fn choose(&mut self, after: bool, line: usize, range: bool) {
        if !self.is_changed(after, line) {
            return;
        }
        let lines: Vec<usize> = match self.anchor {
            Some((side, from)) if range && side == after => {
                (from.min(line)..=from.max(line)).filter(|line| self.is_changed(after, *line)).collect()
            }
            _ => vec![line],
        };
        let set = if after { &mut self.chosen_added } else { &mut self.chosen_removed };
        if lines.len() == 1 && !range && set.contains(&line) {
            set.remove(&line);
        } else {
            set.extend(lines);
        }
        self.anchor = Some((after, line));
    }

    fn chosen(&self) -> usize {
        self.chosen_added.len() + self.chosen_removed.len()
    }

    fn ask(&self, hunk: Option<usize>, discard: bool) -> Option<(&'static str, serde_json::Value)> {
        let part = self.view.part?;
        let command = match (part, discard) {
            (ChangePart::Unstaged, true) => "vcs.workspace.discard",
            (ChangePart::Unstaged, false) => "vcs.index.stage",
            (ChangePart::Staged, false) => "vcs.index.unstage",
            (ChangePart::Staged, true) => return None,
        };
        let params = match hunk {
            Some(hunk) => json!({ "paths": [self.view.path], "hunks": [hunk] }),
            None => json!({
                "paths": [self.view.path],
                "lines": self.chosen_added,
                "removedLines": self.chosen_removed,
            }),
        };
        Some((command, params))
    }

    pub(super) fn title(&self) -> String {
        let name = self.view.relative.rsplit('/').next().unwrap_or_default();
        if name.is_empty() { t!("vcs-diff-blank") } else { name.to_owned() }
    }

    fn rows(&self) -> Vec<Row> {
        if self.unified { unified_rows(&self.view) } else { side_by_side_rows(&self.view) }
    }

    pub(super) fn has_keyboard(&self, ctx: &egui::Context) -> bool {
        ctx.memory(egui::Memory::focused).is_none_or(|focused| Some(focused) == self.focus)
    }

    pub(super) fn set_caret(&mut self, after: bool, line: usize) {
        self.caret = Some((after, line));
    }

    pub(super) fn jump_target(&self) -> Option<(PathBuf, usize)> {
        if self.three.is_some() || self.view.binary || self.view.path.as_os_str().is_empty() {
            return None;
        }
        let file_after = !self.swapped;
        let line = match self.caret {
            Some((after, line)) if after == file_after => line,
            Some((after, line)) => other_side_line(&self.view.hunks, after, line),
            None => self
                .view
                .hunks
                .get(self.current)
                .map_or(0, |hunk| if file_after { hunk.after_start } else { hunk.before_start }),
        };
        Some((self.view.path.clone(), line))
    }

    pub(super) fn step(&mut self, forward: bool) {
        let count = self.change_count();
        if count == 0 {
            return;
        }
        self.current = if forward { (self.current + 1).min(count - 1) } else { self.current.saturating_sub(1) };
        self.scroll_to = Some(self.current);
    }

    fn chooser(ui: &Ui, p: &IdePalette, x: f32, y: f32, salt: &str, items: &[(String, bool)]) -> (Rect, Option<usize>) {
        let label = items.iter().find(|(_, on)| *on).map(|(label, _)| label.clone()).unwrap_or_default();
        let galley = ui.painter().layout_no_wrap(label, theme::regular(13.0), p.text);
        let chooser = Rect::from_min_size(pos2(x, y - 13.0), vec2(galley.size().x + 30.0, 26.0));
        let response = ui.interact(chooser, ui.scope_id().with(salt), Sense::click());
        crate::accessibility::name(&response, egui::Role::ComboBox, galley.text());
        hover(ui, &response, p);
        ui.painter().galley(pos2(chooser.left() + 8.0, y - galley.size().y / 2.0), galley, p.text);
        crate::icons::chevron_down(
            ui.painter(),
            Rect::from_center_size(pos2(chooser.right() - 11.0, y), Vec2::splat(10.0)),
            p.text_secondary,
        );
        let mut chosen = None;
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(200.0);
            for (index, (label, on)) in items.iter().enumerate() {
                if ui.selectable_label(*on, label).clicked() {
                    chosen = Some(index);
                    ui.close();
                }
            }
        });
        (chooser, chosen)
    }

    pub(super) fn set_whitespace(&mut self, whitespace: Whitespace) -> bool {
        let changed = self.whitespace != whitespace;
        self.whitespace = whitespace;
        changed
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> DiffOutcome {
        let mut outcome = DiffOutcome::default();
        if let Some(at) = self.typed_at {
            let waited = ui.input(|input| input.time) - at;
            if waited >= RETYPED_AFTER || !self.typed() {
                self.typed_at = None;
                self.refresh_texts();
                outcome.rehunk = true;
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(RETYPED_AFTER - waited));
            }
        }
        let (previous, next) = ui.input_mut(|input| {
            let previous = input.consume_key(egui::Modifiers::SHIFT, egui::Key::F7);
            (previous, input.consume_key(egui::Modifiers::NONE, egui::Key::F7))
        });
        if next || previous {
            self.step(next);
        }
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        let y = bar.center().y;
        let mut x = bar.left() + 10.0;
        let shortcuts = [("⇧F7", false), ("F7", true)];
        for (index, (shortcut, forward)) in shortcuts.into_iter().enumerate() {
            let button = Rect::from_min_size(pos2(x, y - 12.0), Vec2::splat(24.0));
            let response = ui.interact(button, ui.scope_id().with(("diff-step", index)), Sense::click());
            hover(ui, &response, p);
            let name = if forward { t!("vcs-diff-next") } else { t!("vcs-diff-previous") };
            hint(ui, &response, &name, Some(shortcut), Side::Below);
            arrow(ui.painter(), button.shrink(7.0), forward, p.text_secondary);
            if response.clicked() {
                self.step(forward);
            }
            x = button.right() + 2.0;
        }
        x += 10.0;
        let mut chooser = Rect::from_min_size(pos2(x, y), Vec2::ZERO);
        if self.three.is_none() {
            let items = [(t!("vcs-diff-side-by-side"), !self.unified), (t!("vcs-diff-unified"), self.unified)];
            let (rect, chosen) = Self::chooser(ui, p, x, y, "diff-viewer", &items);
            if let Some(index) = chosen {
                outcome.switched = self.unified != (index == 1);
                self.unified = index == 1;
            }
            chooser = rect;
            x = rect.right() + 6.0;
        }
        let items: Vec<(String, bool)> =
            Whitespace::ALL.iter().map(|mode| (whitespace_label(*mode), *mode == self.whitespace)).collect();
        let (rect, chosen) = Self::chooser(ui, p, x, y, "diff-whitespace", &items);
        if let Some(index) = chosen {
            outcome.rehunk |= self.set_whitespace(Whitespace::ALL[index]);
        }
        chooser = chooser.union(rect);
        let swap = Rect::from_min_size(pos2(rect.right() + 6.0, y - 12.0), Vec2::splat(24.0));
        let response = ui.interact(swap, ui.scope_id().with("diff-swap"), Sense::click());
        crate::accessibility::name(&response, egui::Role::Button, &t!("vcs-diff-swap"));
        hover(ui, &response, p);
        hint(ui, &response, &t!("vcs-diff-swap"), None, Side::Below);
        swap_icon(ui.painter(), swap.shrink(6.0), p.text_secondary);
        if response.clicked() {
            self.swap();
        }
        chooser = chooser.union(swap);
        if outcome.switched {
            self.scroll_to = Some(self.current);
        }
        let count = ui.painter().text(
            pos2(bar.right() - 12.0, y),
            Align2::RIGHT_CENTER,
            t!("vcs-diff-count", count = self.change_count()),
            theme::regular(12.5),
            p.text_secondary,
        );
        if let Some(part) = self.view.part.filter(|_| self.stages())
            && !self.view.binary
            && !self.view.hunks.is_empty()
        {
            let chosen = self.chosen();
            let mut right = count.left() - 14.0;
            if chosen > 0 {
                let clear = Rect::from_min_size(pos2(right - 24.0, y - 12.0), Vec2::splat(24.0));
                let response = ui.interact(clear, ui.scope_id().with("diff-clear-lines"), Sense::click());
                hover(ui, &response, p);
                hint(ui, &response, &t!("vcs-diff-clear-lines"), Some("Esc"), Side::Below);
                cross(ui.painter(), clear.shrink(8.0), p.text_secondary);
                if response.clicked() || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                    self.chosen_added.clear();
                    self.chosen_removed.clear();
                }
                right = clear.left() - 6.0;
            }
            let labels: Vec<(String, bool)> = match (part, chosen) {
                (ChangePart::Unstaged, 0) => {
                    vec![(t!("vcs-diff-discard-hunk"), true), (t!("vcs-diff-stage-hunk"), false)]
                }
                (ChangePart::Unstaged, n) => {
                    vec![
                        (t!("vcs-diff-discard-lines", count = n), true),
                        (t!("vcs-diff-stage-lines", count = n), false),
                    ]
                }
                (ChangePart::Staged, 0) => vec![(t!("vcs-diff-unstage-hunk"), false)],
                (ChangePart::Staged, n) => vec![(t!("vcs-diff-unstage-lines", count = n), false)],
            };
            for (index, (label, discard)) in labels.into_iter().enumerate().rev() {
                let galley = ui.painter().layout_no_wrap(label, theme::regular(12.5), p.text);
                let button = Rect::from_min_max(pos2(right - galley.size().x - 20.0, y - 12.0), pos2(right, y + 12.0));
                if button.left() < chooser.right() + 8.0 {
                    break;
                }
                let response = ui.interact(button, ui.scope_id().with(("diff-part", index)), Sense::click());
                hover(ui, &response, p);
                ui.painter().rect_stroke(button, 6, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
                ui.painter().galley(button.center() - galley.size() / 2.0, galley, p.text);
                if chosen == 0 {
                    hint(ui, &response, &t!("vcs-diff-choose-lines"), None, Side::Below);
                }
                if response.clicked() {
                    let hunk = (chosen == 0).then_some(self.current);
                    outcome.ask = self.ask(hunk, discard);
                    self.chosen_added.clear();
                    self.chosen_removed.clear();
                }
                right = button.left() - 8.0;
            }
        }
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.frame));

        if self.view.binary {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(t!("vcs-diff-binary")).color(p.text_secondary));
            });
            return outcome;
        }
        if self.three.is_some() {
            self.show_three(ui, p);
            return outcome;
        }
        if self.typed() {
            self.show_typed(ui, p, &mut outcome);
            return outcome;
        }
        let (titles, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        let half = (titles.width() - DIVIDER) / 2.0;
        let font = theme::regular(12.5);
        if self.unified {
            let text = format!("{} → {}", self.view.before_title, self.view.after_title);
            ui.painter().text(
                pos2(titles.left() + 12.0, titles.center().y),
                Align2::LEFT_CENTER,
                text,
                font,
                p.text_secondary,
            );
        } else {
            pane_title(ui, p, titles, 0, half, &self.view.before_title);
            pane_title(ui, p, titles, 1, half, &self.view.after_title);
        }

        let rows = self.rows();
        let scroll_row = self.scroll_to.take().and_then(|hunk| rows.iter().position(|row| row.hunk == Some(hunk)));
        let frame = ui.available_rect_before_wrap();
        let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let widest = self
            .before_lines
            .iter()
            .chain(&self.after_lines)
            .map(|(_, line)| line.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>())
            .max()
            .unwrap_or(0) as f32
            * char_width;
        let room = if self.unified {
            frame.width() - 2.0 * NUMBERS - 8.0
        } else {
            (frame.width() - DIVIDER) / 2.0 - NUMBERS - 4.0
        };
        let max_x = (widest + 2.0 * char_width - room).max(0.0);
        sideways(ui, frame, &mut self.scroll_x, max_x);
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if let Some(row) = scroll_row {
            area = area.vertical_scroll_offset((row as f32 * ROW - 3.0 * ROW).max(0.0));
        }
        let parted = self.stages();
        let mut clicked: Option<(Row, f32, bool)> = None;
        let mut pressed: Option<(Row, f32)> = None;
        let focus = ui.make_persistent_id("diff-rows");
        self.focus = Some(focus);
        ui.interact(frame, focus, Sense::focusable_noninteractive());
        let caret = self.caret;
        let inner = Rect::from_min_max(frame.min, pos2(frame.right() - MARKS, frame.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            area.show_rows(ui, ROW, rows.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let width = ui.available_width().max(400.0);
                for index in range {
                    let row = rows[index];
                    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                    if let Some(side) = caret_side(caret, row, self.unified) {
                        ui.painter().rect_filled(caret_rect(rect, side, self.unified), 0, p.current_line);
                    }
                    if self.unified {
                        self.unified_row(ui, p, rect, row);
                    } else {
                        self.side_row(ui, p, rect, row);
                    }
                    if parted && row.hunk.is_some() {
                        self.part_marks(ui, p, rect, row);
                        if response.hovered() && self.number_side(rect, row, response.hover_pos()).is_some() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if response.clicked()
                            && let Some(pointer) = response.interact_pointer_pos()
                        {
                            clicked = Some((row, pointer.x - rect.left(), ui.input(|input| input.modifiers.shift)));
                        }
                    }
                    if response.clicked()
                        && let Some(pointer) = response.interact_pointer_pos()
                    {
                        pressed = Some((row, pointer.x - rect.left()));
                    }
                }
            });
        });
        if let Some((row, x)) = pressed {
            ui.memory_mut(|memory| memory.request_focus(focus));
            if let Some(caret) = self.caret_at(row, x, frame.width().max(400.0)) {
                self.caret = Some(caret);
            }
        }
        if let Some((row, x, shift)) = clicked {
            match self.number_side_at(row, x, frame.width().max(400.0)) {
                Some((after, line)) => self.choose(after, line, shift),
                None => self.current = row.hunk.unwrap_or(self.current),
            }
        }
        if max_x > 0.0 {
            self.horizontal_bars(ui, p, inner, room, max_x);
        }
        self.change_marks(ui, p, frame, &rows);
        outcome
    }

    fn caret_at(&self, row: Row, x: f32, width: f32) -> Option<(bool, usize)> {
        if self.unified {
            return row.after.map(|line| (true, line)).or(row.before.map(|line| (false, line)));
        }
        let half = (width - DIVIDER) / 2.0;
        if x < half + DIVIDER / 2.0 { row.before.map(|line| (false, line)) } else { row.after.map(|line| (true, line)) }
    }

    fn number_side(&self, rect: Rect, row: Row, pointer: Option<egui::Pos2>) -> Option<(bool, usize)> {
        self.number_side_at(row, pointer?.x - rect.left(), rect.width())
    }

    fn number_side_at(&self, row: Row, x: f32, width: f32) -> Option<(bool, usize)> {
        let (after, line) = if self.unified {
            if x > 2.0 * NUMBERS {
                return None;
            }
            match (row.before, row.after, row.kind) {
                (Some(line), None, _) => (false, line),
                (_, Some(line), RowKind::Inserted | RowKind::Modified) => (true, line),
                _ => return None,
            }
        } else {
            let half = (width - DIVIDER) / 2.0;
            if x <= NUMBERS {
                (false, row.before?)
            } else if (half + DIVIDER..=half + DIVIDER + NUMBERS).contains(&x) {
                (true, row.after?)
            } else {
                return None;
            }
        };
        self.is_changed(after, line).then_some((after, line))
    }

    fn part_marks(&self, ui: &Ui, p: &IdePalette, rect: Rect, row: Row) {
        if row.hunk == Some(self.current) && self.chosen() == 0 {
            ui.painter().rect_filled(Rect::from_min_size(rect.min, vec2(3.0, rect.height())), 0, p.accent);
        }
        let half = (rect.width() - DIVIDER) / 2.0;
        let mark = |left: f32| {
            let number =
                Rect::from_min_size(pos2(rect.left() + left + 3.0, rect.top() + 2.0), vec2(NUMBERS - 6.0, ROW - 4.0));
            ui.painter().rect_stroke(number, 4, Stroke::new(1.5, p.accent), egui::StrokeKind::Inside);
        };
        if row.before.is_some_and(|line| self.chosen_removed.contains(&line)) {
            mark(0.0);
        }
        if let Some(line) = row.after
            && self.chosen_added.contains(&line)
            && row.kind != RowKind::Same
        {
            mark(if self.unified { NUMBERS } else { half + DIVIDER });
        }
    }

    fn change_marks(&mut self, ui: &Ui, p: &IdePalette, frame: Rect, rows: &[Row]) {
        let marks = change_marks(rows);
        if marks.is_empty() {
            return;
        }
        let track = Rect::from_min_max(
            pos2(frame.right() - MARKS + 3.0, frame.top() + 2.0),
            pos2(frame.right() - 2.0, frame.bottom() - 2.0),
        );
        let total = rows.len().max(1) as f32;
        for (index, mark) in marks.iter().enumerate() {
            let top = track.top() + track.height() * mark.start as f32 / total;
            let bottom = (track.top() + track.height() * mark.end as f32 / total).max(top + 3.0);
            let rect = Rect::from_min_max(pos2(track.left(), top), pos2(track.right(), bottom));
            let color = match mark.kind {
                RowKind::Inserted => p.gutter_added,
                RowKind::Deleted => p.gutter_deleted,
                _ => p.gutter_modified,
            };
            ui.painter().rect_filled(rect, 1, color);
            let response =
                ui.interact(rect.expand2(vec2(1.0, 2.0)), ui.scope_id().with(("diff-mark", index)), Sense::click());
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let response = response.on_hover_text(mark_hint(mark, rows));
            if response.clicked()
                && let Some(hunk) = mark.hunk
            {
                self.current = hunk;
                self.scroll_to = Some(hunk);
            }
        }
    }

    fn horizontal_bars(&mut self, ui: &Ui, p: &IdePalette, frame: Rect, room: f32, max_x: f32) {
        let texts: Vec<Rect> = if self.unified {
            vec![Rect::from_min_max(pos2(frame.left() + 2.0 * NUMBERS + 8.0, frame.top()), frame.max)]
        } else {
            let half = (frame.width() - DIVIDER) / 2.0;
            vec![
                Rect::from_min_max(
                    pos2(frame.left() + NUMBERS + 4.0, frame.top()),
                    pos2(frame.left() + half, frame.bottom()),
                ),
                Rect::from_min_max(
                    pos2(frame.left() + half + DIVIDER + NUMBERS + 4.0, frame.top()),
                    pos2(frame.right(), frame.bottom()),
                ),
            ]
        };
        scroll_bars(ui, p, &texts, &mut self.scroll_x, room, max_x, "diff-hbar");
    }

    fn side_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, row: Row) {
        let half = (rect.width() - DIVIDER) / 2.0;
        let left = Rect::from_min_size(rect.min, vec2(half, ROW));
        let right = Rect::from_min_size(pos2(rect.left() + half + DIVIDER, rect.top()), vec2(half, ROW));
        let (left_fill, right_fill) = match row.kind {
            RowKind::Same => (None, None),
            RowKind::Inserted => (Some(p.diff_filler), Some(p.diff_inserted)),
            RowKind::Deleted => (Some(p.diff_deleted), Some(p.diff_filler)),
            RowKind::Modified => (
                Some(if row.before.is_some() { p.diff_modified } else { p.diff_filler }),
                Some(if row.after.is_some() { p.diff_modified } else { p.diff_filler }),
            ),
        };
        self.cell(ui, p, left, row.before, left_fill, false, row.kind);
        self.cell(ui, p, right, row.after, right_fill, true, row.kind);
        if row.kind != RowKind::Same {
            let band = Rect::from_min_max(pos2(left.right(), rect.top()), pos2(right.left(), rect.bottom()));
            let color = match row.kind {
                RowKind::Inserted => p.diff_inserted,
                RowKind::Deleted => p.diff_deleted,
                _ => p.diff_modified,
            };
            ui.painter().rect_filled(band.shrink2(vec2(6.0, 0.0)), 0, color);
        }
    }

    fn unified_row(&self, ui: &Ui, p: &IdePalette, rect: Rect, row: Row) {
        let fill = match row.kind {
            RowKind::Same => None,
            RowKind::Deleted => Some(p.diff_deleted),
            _ => Some(p.diff_inserted),
        };
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, 0, fill);
        }
        let painter = ui.painter().with_clip_rect(rect);
        let number_font = FontId::monospace(12.0);
        for (column, line) in [(0.0, row.before), (NUMBERS, row.after)] {
            if let Some(line) = line {
                painter.text(
                    pos2(rect.left() + column + NUMBERS - 8.0, rect.center().y),
                    Align2::RIGHT_CENTER,
                    (line + 1).to_string(),
                    number_font.clone(),
                    p.gutter_text,
                );
            }
        }
        let text_rect = Rect::from_min_max(pos2(rect.left() + 2.0 * NUMBERS + 8.0, rect.top()), rect.max);
        match (row.before, row.after) {
            (_, Some(line)) => self.text(ui, p, text_rect, line, true, row.kind),
            (Some(line), None) => self.text(ui, p, text_rect, line, false, row.kind),
            (None, None) => {}
        }
    }

    #[allow(clippy::too_many_arguments, reason = "one cell of a row: its side, line and colours")]
    fn cell(
        &self,
        ui: &Ui,
        p: &IdePalette,
        rect: Rect,
        line: Option<usize>,
        fill: Option<Color32>,
        after: bool,
        kind: RowKind,
    ) {
        if let Some(fill) = fill {
            ui.painter().rect_filled(rect, 0, fill);
        }
        let Some(line) = line else { return };
        ui.painter().with_clip_rect(rect).text(
            pos2(rect.left() + NUMBERS - 8.0, rect.center().y),
            Align2::RIGHT_CENTER,
            (line + 1).to_string(),
            FontId::monospace(12.0),
            p.gutter_text,
        );
        let text_rect = Rect::from_min_max(pos2(rect.left() + NUMBERS + 4.0, rect.top()), rect.max);
        self.text(ui, p, text_rect, line, after, kind);
    }

    fn text(&self, ui: &Ui, p: &IdePalette, rect: Rect, line: usize, after: bool, kind: RowKind) {
        let (lines, spans, words) = if after {
            (&self.after_lines, &self.after_spans, &self.view.hunks)
        } else {
            (&self.before_lines, &self.before_spans, &self.view.hunks)
        };
        let Some((start, text)) = lines.get(line) else { return };
        let word_color = match kind {
            RowKind::Modified => p.diff_modified_word,
            RowKind::Inserted => p.diff_inserted_word,
            _ => p.diff_deleted_word,
        };
        let changed: Vec<(usize, usize)> = words
            .iter()
            .flat_map(|hunk| if after { &hunk.after_words } else { &hunk.before_words })
            .filter(|word| word[0] == line)
            .map(|word| (word[1], word[2]))
            .collect();
        let job = line_job(text, *start, spans, &changed, p, word_color);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())).galley(
            pos2(rect.left() - self.scroll_x, rect.center().y - galley.size().y / 2.0),
            galley,
            p.code,
        );
    }
}

impl DiffTab {
    fn show_typed(&mut self, ui: &mut Ui, p: &IdePalette, outcome: &mut DiffOutcome) {
        let titles_rect = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover()).0;
        let half = (titles_rect.width() - DIVIDER) / 2.0;
        pane_title(ui, p, titles_rect, 0, half, &self.view.before_title);
        pane_title(ui, p, titles_rect, 1, half, &self.view.after_title);
        let frame = ui.available_rect_before_wrap();
        let width = frame.width() - MARKS;
        let half = (width - DIVIDER) / 2.0;
        let wrap = (half - NUMBERS - 8.0).max(40.0);
        let height = [&self.view.before, &self.view.after]
            .iter()
            .map(|text| text.split_inclusive('\n').count() as f32 + 1.0)
            .fold(0.0_f32, f32::max)
            * ROW;
        let mut area = egui::ScrollArea::vertical().id_salt("diff-typed").auto_shrink([false, false]);
        if let Some(hunk) = self.scroll_to.take().and_then(|current| self.view.hunks.get(current)) {
            let (after, line) = if self.editable(true) { (1, hunk.after_start) } else { (0, hunk.before_start) };
            if let Some((top, _)) = self.tops[after].get(line) {
                area = area.vertical_scroll_offset((top - 3.0 * ROW).max(0.0));
            }
        }
        let hovered_files = ui.input(|input| !input.raw.hovered_files.is_empty());
        let dropped: Vec<PathBuf> = ui.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_owned())
                .filter(|path| !path.as_os_str().is_empty())
                .collect()
        });
        let pointer = ui.input(|input| input.pointer.hover_pos());
        let mut drop_side = None;
        let inner = Rect::from_min_max(frame.min, pos2(frame.right() - MARKS, frame.bottom()));
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            area.show(ui, |ui| {
                let (content, _) =
                    ui.allocate_exact_size(vec2(width, height.max(frame.height() - 4.0)), Sense::hover());
                let mut tops: [Vec<(f32, f32)>; 2] = [Vec::new(), Vec::new()];
                for after in [false, true] {
                    let left = content.left() + if after { half + DIVIDER } else { 0.0 };
                    let pane = Rect::from_min_size(pos2(left, content.top()), vec2(half, content.height()));
                    if hovered_files && self.editable(after) && pointer.is_some_and(|pointer| pane.contains(pointer)) {
                        ui.painter().rect_stroke(
                            pane.shrink(1.0),
                            4,
                            Stroke::new(2.0, p.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if !dropped.is_empty() && self.editable(after) {
                        let over = pointer.is_some_and(|pointer| pane.contains(pointer));
                        if over || drop_side.is_none() && pointer.is_none() {
                            drop_side = Some(after);
                        }
                    }
                    let backgrounds = ui.painter().add(egui::Shape::Noop);
                    let editable = self.editable(after);
                    let spans = if after { &mut self.after_spans } else { &mut self.before_spans };
                    let hunks = &self.view.hunks;
                    let (marks, palette) = (self.marks, *p);
                    let layout = &mut self.layouts[usize::from(after)];
                    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                        let text = text.as_str();
                        if let Some(kept) = layout.as_ref() {
                            if kept.text == text
                                && kept.wrap == wrap_width
                                && kept.marks == marks
                                && kept.palette == palette
                            {
                                return ui.fonts_mut(|fonts| fonts.layout_job(kept.job.clone()));
                            }
                            if kept.marks == marks && kept.text != text {
                                shifted_spans(spans, &kept.text, text);
                            }
                        }
                        let words: Vec<([usize; 3], Color32)> = hunks
                            .iter()
                            .flat_map(|hunk| {
                                let color = word_color(&palette, kind_of(hunk));
                                let words = if after { &hunk.after_words } else { &hunk.before_words };
                                words.iter().map(move |word| (*word, color))
                            })
                            .collect();
                        let job = pane_job(text, spans, &words, &palette, palette.code, wrap_width);
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job.clone()));
                        *layout = Some(PaneLayout { text: text.to_owned(), wrap: wrap_width, marks, palette, job });
                        galley
                    };
                    let text_rect =
                        Rect::from_min_size(pos2(left + NUMBERS + 4.0, content.top()), vec2(wrap, content.height()));
                    let mut text = std::mem::take(if after { &mut self.view.after } else { &mut self.view.before });
                    let before_edit = text.clone();
                    let mut shown = before_edit.as_str();
                    let hint = if editable && text.is_empty() { t!("vcs-diff-paste-here") } else { String::new() };
                    let output = {
                        let edit = if editable {
                            egui::TextEdit::multiline(&mut text)
                        } else {
                            egui::TextEdit::multiline(&mut shown)
                        };
                        let edit = edit
                            .id_salt(("diff-side", after))
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .desired_width(wrap)
                            .desired_rows(1)
                            .hint_text(hint)
                            .layouter(&mut layouter);
                        ui.scope_builder(egui::UiBuilder::new().max_rect(text_rect), |ui| edit.show(ui)).inner
                    };
                    let changed = editable && text != before_edit;
                    *(if after { &mut self.view.after } else { &mut self.view.before }) = text;
                    let lines = line_tops(&output.galley, output.galley_pos);
                    let mut shapes = Vec::new();
                    for (index, hunk) in self.view.hunks.iter().enumerate() {
                        let kind = kind_of(hunk);
                        let (start, end) = if after {
                            (hunk.after_start, hunk.after_end)
                        } else {
                            (hunk.before_start, hunk.before_end)
                        };
                        let (top, bottom) = span_of(&lines, start, end);
                        let color = match (kind, after) {
                            (RowKind::Inserted, _) => p.diff_inserted,
                            (RowKind::Deleted, _) => p.diff_deleted,
                            _ => p.diff_modified,
                        };
                        let fill = Rect::from_min_max(pos2(left, top), pos2(left + half, bottom.max(top + 1.5)));
                        shapes.push(egui::Shape::rect_filled(
                            fill,
                            0,
                            if start == end { color.gamma_multiply(2.0) } else { color },
                        ));
                        if index == self.current {
                            shapes.push(egui::Shape::rect_filled(
                                Rect::from_min_size(pos2(left, top), vec2(3.0, (bottom - top).max(2.0))),
                                0,
                                p.accent,
                            ));
                        }
                    }
                    ui.painter().set(backgrounds, egui::Shape::Vec(shapes));
                    let visible = ui.clip_rect();
                    for (line, (top, _)) in lines.iter().enumerate() {
                        if *top > visible.bottom() || top + ROW < visible.top() {
                            continue;
                        }
                        ui.painter().with_clip_rect(pane.intersect(ui.clip_rect())).text(
                            pos2(left + NUMBERS - 8.0, top + ROW / 2.0),
                            Align2::RIGHT_CENTER,
                            (line + 1).to_string(),
                            FontId::monospace(12.0),
                            p.gutter_text,
                        );
                    }
                    tops[usize::from(after)] =
                        lines.iter().map(|(top, bottom)| (top - content.top(), bottom - content.top())).collect();
                    if changed {
                        self.typed_at = Some(ui.input(|input| input.time));
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(RETYPED_AFTER));
                        if let Some((path, range, side)) = &self.file
                            && *side == after
                        {
                            let text = if after { &self.view.after } else { &self.view.before };
                            outcome.write = Some((path.clone(), *range, text.clone()));
                        }
                    }
                }
                let absolute = |side: &Vec<(f32, f32)>| {
                    side.iter().map(|(top, bottom)| (top + content.top(), bottom + content.top())).collect::<Vec<_>>()
                };
                let (left_lines, right_lines) = (absolute(&tops[0]), absolute(&tops[1]));
                let (from, to) = (content.left() + half, content.left() + half + DIVIDER);
                for hunk in &self.view.hunks {
                    let (a, b) = span_of(&left_lines, hunk.before_start, hunk.before_end);
                    let (c, d) = span_of(&right_lines, hunk.after_start, hunk.after_end);
                    let color = match kind_of(hunk) {
                        RowKind::Inserted => p.diff_inserted,
                        RowKind::Deleted => p.diff_deleted,
                        _ => p.diff_modified,
                    };
                    let points = vec![pos2(from, a), pos2(to, c), pos2(to, d.max(c + 1.5)), pos2(from, b.max(a + 1.5))];
                    ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
                }
                self.tops = tops;
            });
        });
        if let Some(after) = drop_side
            && let Some(path) = dropped.first()
            && let Some(text) = dropped_text(path)
            && let Some(write) = self.set_side(after, text, Some(path.display().to_string()))
        {
            outcome.rehunk = true;
            outcome.write = write.or(outcome.write.take());
        }
        let rows = side_by_side_rows(&self.view);
        self.change_marks(ui, p, frame, &rows);
    }

    fn show_three(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(three) = self.three.take() else { return };
        let titles_rect = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover()).0;
        let pane = (titles_rect.width() - MARKS - 2.0 * DIVIDER) / 3.0;
        let left_of = |index: usize, origin: f32| origin + index as f32 * (pane + DIVIDER);
        for (index, title) in three.titles.iter().enumerate() {
            pane_title(ui, p, titles_rect, index, pane, title);
        }
        let frame = ui.available_rect_before_wrap();
        let char_width = ui.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(13.0), 'M'));
        let widest = self.three_widest as f32 * char_width;
        let room = pane - NUMBERS - 4.0;
        let max_x = (widest + 2.0 * char_width - room).max(0.0);
        sideways(ui, frame, &mut self.scroll_x, max_x);
        let changes = three_changes(&three);
        let rows = self.three_lines.iter().map(Vec::len).max().unwrap_or(0) + 1;
        let mut area = egui::ScrollArea::vertical().id_salt("diff-three").auto_shrink([false, false]);
        if let Some(change) = self.scroll_to.take().and_then(|current| changes.get(current)) {
            area = area.vertical_scroll_offset((change.0 as f32 * ROW - 3.0 * ROW).max(0.0));
        }
        let inner = Rect::from_min_max(frame.min, pos2(frame.right() - MARKS, frame.bottom()));
        let current = changes.get(self.current).copied();
        ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
            area.show(ui, |ui| {
                let (content, _) = ui.allocate_exact_size(
                    vec2(inner.width(), (rows as f32 * ROW).max(frame.height() - 4.0)),
                    Sense::hover(),
                );
                let y = |line: usize| content.top() + line as f32 * ROW;
                for (index, lines) in self.three_lines.iter().enumerate() {
                    let left = left_of(index, content.left());
                    let pane_rect = Rect::from_min_size(pos2(left, content.top()), vec2(pane, content.height()));
                    let mut marked: Vec<(usize, usize, RowKind, bool)> = Vec::new();
                    for (hunk_index, hunk) in three.left_hunks.iter().enumerate() {
                        let is_current = current == Some((hunk.after_start, false, hunk_index));
                        match index {
                            0 => marked.push((hunk.before_start, hunk.before_end, kind_of(hunk), is_current)),
                            1 => marked.push((hunk.after_start, hunk.after_end, kind_of(hunk), is_current)),
                            _ => {}
                        }
                    }
                    for (hunk_index, hunk) in three.right_hunks.iter().enumerate() {
                        let is_current = current == Some((hunk.before_start, true, hunk_index));
                        match index {
                            1 => marked.push((hunk.before_start, hunk.before_end, kind_of(hunk), is_current)),
                            2 => marked.push((hunk.after_start, hunk.after_end, kind_of(hunk), is_current)),
                            _ => {}
                        }
                    }
                    for (start, end, kind, is_current) in marked {
                        let color = match kind {
                            RowKind::Inserted => p.diff_inserted,
                            RowKind::Deleted => p.diff_deleted,
                            _ => p.diff_modified,
                        };
                        let fill =
                            Rect::from_min_max(pos2(left, y(start)), pos2(left + pane, y(end).max(y(start) + 1.5)));
                        ui.painter().rect_filled(fill, 0, if start == end { color.gamma_multiply(2.0) } else { color });
                        if is_current {
                            ui.painter().rect_filled(
                                Rect::from_min_size(fill.min, vec2(3.0, fill.height().max(2.0))),
                                0,
                                p.accent,
                            );
                        }
                    }
                    let visible = ui.clip_rect();
                    let first = ((visible.top() - content.top()) / ROW).floor().max(0.0) as usize;
                    let last = (((visible.bottom() - content.top()) / ROW).ceil().max(0.0) as usize).min(lines.len());
                    let spans = self.three_spans.get(index).map(Vec::as_slice).unwrap_or_default();
                    let mut words: Vec<([usize; 3], Color32)> = Vec::new();
                    let mut add = |hunks: &[DiffHunk], after: bool| {
                        for hunk in hunks {
                            let color = word_color(p, kind_of(hunk));
                            let side = if after { &hunk.after_words } else { &hunk.before_words };
                            words.extend(side.iter().map(|word| (*word, color)));
                        }
                    };
                    match index {
                        0 => add(&three.left_hunks, false),
                        1 => {
                            add(&three.left_hunks, true);
                            add(&three.right_hunks, false);
                        }
                        _ => add(&three.right_hunks, true),
                    }
                    for (line, (start, text)) in lines.iter().enumerate().take(last).skip(first) {
                        let cell = Rect::from_min_size(pos2(left, y(line)), vec2(pane, ROW));
                        ui.painter().with_clip_rect(cell.intersect(pane_rect)).text(
                            pos2(left + NUMBERS - 8.0, cell.center().y),
                            Align2::RIGHT_CENTER,
                            (line + 1).to_string(),
                            FontId::monospace(12.0),
                            p.gutter_text,
                        );
                        let changed: Vec<(usize, usize)> = words
                            .iter()
                            .filter(|(word, _)| word[0] == line)
                            .map(|(word, _)| (word[1], word[2]))
                            .collect();
                        let color = words
                            .iter()
                            .find(|(word, _)| word[0] == line)
                            .map_or(Color32::TRANSPARENT, |(_, color)| *color);
                        let job = line_job(text, *start, spans, &changed, p, color);
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                        let text_rect = Rect::from_min_max(pos2(left + NUMBERS + 4.0, cell.top()), cell.max);
                        ui.painter().with_clip_rect(text_rect.intersect(visible)).galley(
                            pos2(text_rect.left() - self.scroll_x, text_rect.center().y - galley.size().y / 2.0),
                            galley,
                            p.code,
                        );
                    }
                }
                for (gap, hunks) in [(0, &three.left_hunks), (1, &three.right_hunks)] {
                    let from = left_of(gap, content.left()) + pane;
                    let to = from + DIVIDER;
                    for hunk in hunks.iter() {
                        let color = match kind_of(hunk) {
                            RowKind::Inserted => p.diff_inserted,
                            RowKind::Deleted => p.diff_deleted,
                            _ => p.diff_modified,
                        };
                        let (a, b, c, d) =
                            (y(hunk.before_start), y(hunk.before_end), y(hunk.after_start), y(hunk.after_end));
                        let points =
                            vec![pos2(from, a), pos2(to, c), pos2(to, d.max(c + 1.5)), pos2(from, b.max(a + 1.5))];
                        ui.painter().add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
                    }
                }
            });
        });
        if max_x > 0.0 {
            let texts: Vec<Rect> = (0..3)
                .map(|index| {
                    let left = left_of(index, inner.left());
                    Rect::from_min_max(pos2(left + NUMBERS + 4.0, inner.top()), pos2(left + pane, inner.bottom()))
                })
                .collect();
            scroll_bars(ui, p, &texts, &mut self.scroll_x, room, max_x, "diff-three-hbar");
        }
        self.three = Some(three);
    }
}

pub(super) fn three_changes(three: &ThreeWayView) -> Vec<(usize, bool, usize)> {
    let mut changes: Vec<(usize, bool, usize)> = three
        .left_hunks
        .iter()
        .enumerate()
        .map(|(index, hunk)| (hunk.after_start, false, index))
        .chain(three.right_hunks.iter().enumerate().map(|(index, hunk)| (hunk.before_start, true, index)))
        .collect();
    changes.sort();
    changes
}

fn moved(old: &str, new: &str, [start, end]: [usize; 2]) -> [usize; 2] {
    let prefix = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).count();
    let (old_length, new_length) = (old.chars().count(), new.chars().count());
    let room = old_length.min(new_length) - prefix;
    let suffix = old.chars().rev().zip(new.chars().rev()).take(room).take_while(|(a, b)| a == b).count();
    let (old_end, new_end) = (old_length - suffix, new_length - suffix);
    let map = |offset: usize, inside: usize| {
        if offset <= prefix {
            offset
        } else if offset >= old_end {
            offset - old_end + new_end
        } else {
            inside
        }
    };
    let start = map(start, prefix);
    [start, map(end, new_end).max(start)]
}

fn shifted_spans(spans: &mut Vec<zen_syntax::Span>, old: &str, new: &str) {
    let prefix = old.bytes().zip(new.bytes()).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old.bytes().rev().zip(new.bytes().rev()).take(room).take_while(|(a, b)| a == b).count();
    let old_end = old.len() - suffix;
    let new_end = new.len() - suffix;
    spans.retain_mut(|span| {
        if span.range.end <= prefix {
            true
        } else if span.range.start >= old_end {
            span.range = span.range.start - old_end + new_end..span.range.end - old_end + new_end;
            true
        } else {
            false
        }
    });
}

fn line_tops(galley: &egui::Galley, origin: egui::Pos2) -> Vec<(f32, f32)> {
    let mut lines = Vec::new();
    let mut start: Option<f32> = None;
    for row in &galley.rows {
        let rect = row.rect().translate(origin.to_vec2());
        let top = *start.get_or_insert(rect.top());
        if row.ends_with_newline {
            lines.push((top, rect.bottom()));
            start = None;
        }
    }
    if let Some(top) = start {
        let bottom = galley.rows.last().map_or(top + ROW, |row| row.rect().translate(origin.to_vec2()).bottom());
        lines.push((top, bottom));
    }
    lines
}

fn span_of(lines: &[(f32, f32)], start: usize, end: usize) -> (f32, f32) {
    let at = |line: usize| {
        lines.get(line).map(|(top, _)| *top).or_else(|| lines.last().map(|(_, bottom)| *bottom)).unwrap_or(0.0)
    };
    if end > start {
        let bottom = lines.get(end - 1).map(|(_, bottom)| *bottom).unwrap_or_else(|| at(end));
        (at(start), bottom)
    } else {
        (at(start), at(start))
    }
}

fn word_color(p: &IdePalette, kind: RowKind) -> Color32 {
    match kind {
        RowKind::Modified => p.diff_modified_word,
        RowKind::Inserted => p.diff_inserted_word,
        _ => p.diff_deleted_word,
    }
}

fn pane_job(
    text: &str,
    spans: &[zen_syntax::Span],
    words: &[([usize; 3], Color32)],
    p: &IdePalette,
    code: Color32,
    wrap: f32,
) -> LayoutJob {
    let mut by_line: std::collections::HashMap<usize, Vec<(usize, usize, Color32)>> = std::collections::HashMap::new();
    for (word, color) in words {
        by_line.entry(word[0]).or_default().push((word[1], word[2], *color));
    }
    let format = |color: Color32, background: Color32| {
        let mut format = TextFormat::simple(FontId::monospace(13.0), color);
        format.line_height = Some(ROW);
        format.valign = egui::Align::Center;
        format.background = background;
        format
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut piece_start = 0;
    let mut piece_look: Option<(Color32, Color32)> = None;
    let (mut line, mut column) = (0, 0);
    let mut line_words = by_line.get(&0).map_or(&[][..], Vec::as_slice);
    let mut span_at = 0;
    for (byte, character) in text.char_indices() {
        while spans.get(span_at).is_some_and(|span| span.range.end <= byte) {
            span_at += 1;
        }
        let color =
            spans.get(span_at).filter(|span| span.range.contains(&byte)).map_or(code, |span| p.syntax(span.kind).color);
        let background = if character == '\n' {
            Color32::TRANSPARENT
        } else {
            line_words
                .iter()
                .find(|(start, end, _)| (*start..*end).contains(&column))
                .map_or(Color32::TRANSPARENT, |(.., background)| *background)
        };
        if piece_look.is_some_and(|look| look != (color, background)) {
            if let Some((color, background)) = piece_look {
                job.append(&text[piece_start..byte], 0.0, format(color, background));
            }
            piece_start = byte;
        }
        piece_look = Some((color, background));
        if character == '\n' {
            line += 1;
            column = 0;
            line_words = by_line.get(&line).map_or(&[][..], Vec::as_slice);
        } else {
            column += 1;
        }
    }
    let (color, background) = piece_look.unwrap_or((code, Color32::TRANSPARENT));
    job.append(&text[piece_start..], 0.0, format(color, background));
    job
}

fn pane_title(ui: &Ui, p: &IdePalette, titles: Rect, index: usize, width: f32, title: &str) {
    let left = titles.left() + index as f32 * (width + DIVIDER);
    let rect = Rect::from_min_size(pos2(left, titles.top()), vec2(width, titles.height()));
    let painter = ui.painter().with_clip_rect(rect.shrink2(vec2(4.0, 0.0)).intersect(ui.clip_rect()));
    let shown = painter.text(
        pos2(left + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        theme::regular(12.5),
        p.text_secondary,
    );
    if shown.right() > rect.right() {
        let _ = ui.interact(rect, ui.scope_id().with(("diff-title", index)), Sense::hover()).on_hover_text(title);
    }
}

pub(super) fn whitespace_label(mode: Whitespace) -> String {
    match mode {
        Whitespace::None => t!("vcs-diff-whitespace-none"),
        Whitespace::Trim => t!("vcs-diff-whitespace-trim"),
        Whitespace::Ignore => t!("vcs-diff-whitespace-ignore"),
    }
}

fn swap_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let (upper, lower) = (rect.top() + rect.height() * 0.3, rect.top() + rect.height() * 0.7);
    let wing = rect.width() * 0.25;
    painter.line_segment([pos2(rect.left(), upper), pos2(rect.right(), upper)], stroke);
    painter.line_segment([pos2(rect.right(), upper), pos2(rect.right() - wing, upper - wing)], stroke);
    painter.line_segment([pos2(rect.left(), lower), pos2(rect.right(), lower)], stroke);
    painter.line_segment([pos2(rect.left(), lower), pos2(rect.left() + wing, lower + wing)], stroke);
}

pub(super) fn sideways(ui: &Ui, frame: Rect, scroll_x: &mut f32, max_x: f32) {
    if ui.rect_contains_pointer(frame) {
        let (delta, shift) = ui.input(|input| (input.smooth_scroll_delta, input.modifiers.shift));
        let sideways = if delta.x != 0.0 {
            delta.x
        } else if shift {
            delta.y
        } else {
            0.0
        };
        if sideways != 0.0 {
            *scroll_x -= sideways;
            ui.input_mut(|input| {
                input.smooth_scroll_delta.x = 0.0;
                if shift {
                    input.smooth_scroll_delta.y = 0.0;
                }
            });
        }
    }
    *scroll_x = scroll_x.clamp(0.0, max_x);
}

pub(super) fn scroll_bars(
    ui: &Ui,
    p: &IdePalette,
    texts: &[Rect],
    scroll_x: &mut f32,
    room: f32,
    max_x: f32,
    salt: &str,
) {
    for (index, text) in texts.iter().enumerate() {
        let track =
            Rect::from_min_max(pos2(text.left(), text.bottom() - 8.0), pos2(text.right() - 10.0, text.bottom() - 2.0));
        let length = (track.width() * room / (room + max_x)).max(24.0).min(track.width());
        let left = track.left() + (track.width() - length) * (*scroll_x / max_x);
        let thumb = Rect::from_min_size(pos2(left, track.top()), vec2(length, track.height()));
        let response = ui.interact(track, ui.scope_id().with((salt, index)), Sense::click_and_drag());
        if response.dragged() {
            let travel = (track.width() - length).max(1.0);
            *scroll_x = (*scroll_x + response.drag_delta().x * max_x / travel).clamp(0.0, max_x);
        }
        let color = if response.hovered() || response.dragged() { p.text_secondary } else { p.gutter_text };
        ui.painter().rect_filled(thumb, 3, color.gamma_multiply(0.6));
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ChangeMark {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) kind: RowKind,
    pub(super) hunk: Option<usize>,
}

fn mark_hint(mark: &ChangeMark, rows: &[Row]) -> String {
    let span = |lines: Vec<usize>| -> String {
        match (lines.iter().min(), lines.iter().max()) {
            (Some(first), Some(last)) if first == last => (first + 1).to_string(),
            (Some(first), Some(last)) => format!("{}–{}", first + 1, last + 1),
            _ => String::new(),
        }
    };
    let part = &rows[mark.start..mark.end];
    let before = span(part.iter().filter_map(|row| row.before).collect());
    let after = span(part.iter().filter_map(|row| row.after).collect());
    match mark.kind {
        RowKind::Inserted => t!("vcs-diff-mark-added", lines = after.as_str()),
        RowKind::Deleted => t!("vcs-diff-mark-deleted", lines = before.as_str()),
        _ => t!("vcs-diff-mark-modified", before = before.as_str(), after = after.as_str()),
    }
}

pub(super) fn change_marks(rows: &[Row]) -> Vec<ChangeMark> {
    let mut marks: Vec<ChangeMark> = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        if row.kind == RowKind::Same {
            continue;
        }
        match marks.last_mut() {
            Some(last) if last.end == index && last.hunk == row.hunk => last.end = index + 1,
            _ => marks.push(ChangeMark { start: index, end: index + 1, kind: row.kind, hunk: row.hunk }),
        }
    }
    marks
}

pub(super) fn line_job(
    text: &str,
    start: usize,
    spans: &[zen_syntax::Span],
    changed: &[(usize, usize)],
    p: &IdePalette,
    word_color: Color32,
) -> LayoutJob {
    let font = FontId::monospace(13.0);
    let mut job = LayoutJob::default();
    let tabbed = |piece: &str| piece.replace('\t', "    ");
    let in_word = |char_index: usize| changed.iter().any(|(from, to)| (*from..*to).contains(&char_index));
    let mut piece = String::new();
    let mut piece_format: Option<TextFormat> = None;
    for (char_index, (byte, character)) in text.char_indices().enumerate() {
        let absolute = start + byte;
        let color = spans
            .get(spans.partition_point(|span| span.range.end <= absolute))
            .filter(|span| span.range.contains(&absolute))
            .map_or(p.code, |span| p.syntax(span.kind).color);
        let mut format = TextFormat::simple(font.clone(), color);
        if in_word(char_index) {
            format.background = word_color;
        }
        if piece_format.as_ref().is_some_and(|current| *current != format) {
            job.append(&tabbed(&piece), 0.0, piece_format.take().unwrap_or_default());
            piece.clear();
        }
        piece_format = Some(format);
        piece.push(character);
    }
    if let Some(format) = piece_format {
        job.append(&tabbed(&piece), 0.0, format);
    }
    job
}

pub(super) fn diff_icon(painter: &egui::Painter, rect: Rect, p: &IdePalette) {
    let half = rect.width() * 0.42;
    let left = Rect::from_min_size(rect.min, vec2(half, rect.height()));
    let right = Rect::from_min_size(pos2(rect.right() - half, rect.top()), vec2(half, rect.height()));
    painter.rect_stroke(left, 2, Stroke::new(1.1, p.text_secondary), egui::StrokeKind::Inside);
    painter.rect_filled(right, 2, p.gutter_modified);
}

fn cross(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.4, color);
    painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_top(), rect.left_bottom()], stroke);
}

pub(super) fn arrow(painter: &egui::Painter, rect: Rect, down: bool, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    let (from, to) =
        if down { (rect.center_top(), rect.center_bottom()) } else { (rect.center_bottom(), rect.center_top()) };
    painter.line_segment([from, to], stroke);
    let head = if down { -1.0 } else { 1.0 };
    let wing = rect.width() * 0.35;
    painter.line_segment([to, to + vec2(-wing, wing * head)], stroke);
    painter.line_segment([to, to + vec2(wing, wing * head)], stroke);
}

const DROPPED_LIMIT: u64 = 10 << 20;

fn dropped_text(path: &Path) -> Option<String> {
    let small = std::fs::metadata(path).is_ok_and(|metadata| metadata.len() <= DROPPED_LIMIT);
    small.then(|| std::fs::read_to_string(path).ok()).flatten()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_dropped_file_too_big_to_compare_is_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let (big, small) = (dir.path().join("big.txt"), dir.path().join("small.txt"));
        std::fs::File::create(&big).unwrap().set_len(DROPPED_LIMIT + 1).unwrap();
        std::fs::write(&small, "text").unwrap();
        assert_eq!(dropped_text(&big), None);
        assert_eq!(dropped_text(&small).as_deref(), Some("text"));
    }

    use super::*;

    fn view(before: &str, after: &str, hunks: Vec<DiffHunk>) -> DiffView {
        DiffView {
            before: before.into(),
            after: after.into(),
            hunks,
            relative: "src/a.go".into(),
            ..DiffView::default()
        }
    }

    fn hunk(kind: &str, before: (usize, usize), after: (usize, usize)) -> DiffHunk {
        DiffHunk {
            kind: kind.into(),
            before_start: before.0,
            before_end: before.1,
            after_start: after.0,
            after_end: after.1,
            ..DiffHunk::default()
        }
    }

    #[test]
    fn every_difference_has_one_mark_over_its_rows() {
        let before = "a\nb\nc\nd\ne\n";
        let after = "a\nB\nc\nd\ne\nf\n";
        let tab_view = view(before, after, vec![hunk("modified", (1, 2), (1, 2)), hunk("added", (5, 5), (5, 6))]);
        let marks = change_marks(&side_by_side_rows(&tab_view));
        assert_eq!(marks.len(), 2);
        assert_eq!((marks[0].start, marks[0].end, marks[0].hunk), (1, 2, Some(0)));
        assert_eq!((marks[1].kind, marks[1].hunk), (RowKind::Inserted, Some(1)));
        assert!(change_marks(&side_by_side_rows(&view(before, before, Vec::new()))).is_empty());
    }

    #[test]
    fn both_sides_scroll_sideways_together() {
        let long = format!("x := \"{}\"\n", "a".repeat(400));
        let mut tab =
            DiffTab::new(view(&long, &format!("y{long}"), vec![hunk("modified", (0, 1), (0, 1))]), None, false);
        let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let ctx = egui::Context::default();
        let frame = |events: Vec<egui::Event>, tab: &mut DiffTab| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| {
                let _ = tab.show(ui, &palette);
            });
            output.textures_delta.clear();
        };
        let wheel = |delta: Vec2, modifiers: egui::Modifiers| egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers,
        };
        let over = egui::Event::PointerMoved(pos2(500.0, 300.0));
        frame(Vec::new(), &mut tab);
        assert_eq!(tab.scroll_x, 0.0);
        frame(vec![over.clone(), wheel(vec2(-300.0, 0.0), egui::Modifiers::NONE)], &mut tab);
        for _ in 0..10 {
            frame(Vec::new(), &mut tab);
        }
        assert!(tab.scroll_x > 100.0, "a sideways swipe scrolls: {}", tab.scroll_x);
        let swiped = tab.scroll_x;
        frame(vec![over.clone(), wheel(vec2(0.0, -200.0), egui::Modifiers::SHIFT)], &mut tab);
        for _ in 0..10 {
            frame(Vec::new(), &mut tab);
        }
        assert!(tab.scroll_x > swiped, "⇧ and the wheel scroll sideways too");
        frame(vec![over, wheel(vec2(100_000.0, 0.0), egui::Modifiers::NONE)], &mut tab);
        for _ in 0..10 {
            frame(Vec::new(), &mut tab);
        }
        assert_eq!(tab.scroll_x, 0.0, "it stops at the start");
    }

    #[test]
    fn side_by_side_rows_pair_lines_and_fill_the_shorter_side() {
        let view = view(
            "a\nb\nc\nd\n",
            "a\nB\nB2\nc\n",
            vec![hunk("modified", (1, 2), (1, 3)), hunk("deleted", (3, 4), (4, 4))],
        );
        let rows: Vec<(Option<usize>, Option<usize>, RowKind)> =
            side_by_side_rows(&view).iter().map(|row| (row.before, row.after, row.kind)).collect();
        assert_eq!(
            rows,
            [
                (Some(0), Some(0), RowKind::Same),
                (Some(1), Some(1), RowKind::Modified),
                (None, Some(2), RowKind::Modified),
                (Some(2), Some(3), RowKind::Same),
                (Some(3), None, RowKind::Deleted),
            ]
        );
    }

    #[test]
    fn unified_rows_put_old_lines_before_new_ones() {
        let view =
            view("a\nb\nc\n", "a\nB\nc\nd\n", vec![hunk("modified", (1, 2), (1, 2)), hunk("added", (3, 3), (3, 4))]);
        let rows: Vec<(Option<usize>, Option<usize>, RowKind)> =
            unified_rows(&view).iter().map(|row| (row.before, row.after, row.kind)).collect();
        assert_eq!(
            rows,
            [
                (Some(0), Some(0), RowKind::Same),
                (Some(1), None, RowKind::Deleted),
                (None, Some(1), RowKind::Inserted),
                (Some(2), Some(2), RowKind::Same),
                (None, Some(3), RowKind::Inserted),
            ]
        );
    }

    #[test]
    fn stepping_stays_within_the_differences() {
        let mut tab = DiffTab::new(
            view("a\nb\n", "A\nB\n", vec![hunk("modified", (0, 1), (0, 1)), hunk("modified", (1, 2), (1, 2))]),
            None,
            false,
        );
        assert_eq!(tab.title(), "a.go");
        tab.step(false);
        assert_eq!(tab.current, 0);
        tab.step(true);
        tab.step(true);
        assert_eq!(tab.current, 1);
    }

    fn parted(part: ChangePart) -> DiffTab {
        let mut view =
            view("a\nb\nc\n", "A\nb\nc\nd\n", vec![hunk("modified", (0, 1), (0, 1)), hunk("added", (3, 3), (3, 4))]);
        view.path = "/r/src/a.go".into();
        view.part = Some(part);
        DiffTab::new(view, None, false)
    }

    #[test]
    fn changed_lines_are_chosen_by_their_numbers_and_ranges() {
        let mut tab = parted(ChangePart::Unstaged);
        tab.choose(true, 1, false);
        assert_eq!(tab.chosen(), 0, "an unchanged line is not chosen");
        tab.choose(true, 0, false);
        tab.choose(false, 0, false);
        assert_eq!((tab.chosen_added.len(), tab.chosen_removed.len()), (1, 1));
        tab.choose(true, 0, false);
        assert!(tab.chosen_added.is_empty(), "a second click leaves it");
        tab.choose(true, 0, false);
        tab.choose(true, 3, true);
        assert_eq!(tab.chosen_added.iter().copied().collect::<Vec<_>>(), [0, 3]);
        let (command, params) = tab.ask(None, false).unwrap();
        assert_eq!(command, "vcs.index.stage");
        assert_eq!(params, json!({ "paths": ["/r/src/a.go"], "lines": [0, 3], "removedLines": [0] }));
        let (command, params) = tab.ask(Some(1), true).unwrap();
        assert_eq!((command, params), ("vcs.workspace.discard", json!({ "paths": ["/r/src/a.go"], "hunks": [1] })));
        let staged = parted(ChangePart::Staged);
        assert_eq!(staged.ask(Some(0), false).unwrap().0, "vcs.index.unstage");
        assert!(staged.ask(Some(0), true).is_none(), "staged changes are not discarded here");
        assert!(DiffTab::new(view("a\n", "b\n", Vec::new()), None, false).ask(Some(0), false).is_none());
    }

    #[test]
    fn a_click_on_a_line_number_names_the_line_of_that_side() {
        let mut tab = parted(ChangePart::Unstaged);
        let rows = side_by_side_rows(&tab.view);
        let half = (1000.0 - DIVIDER) / 2.0;
        assert_eq!(tab.number_side_at(rows[0], 10.0, 1000.0), Some((false, 0)));
        assert_eq!(tab.number_side_at(rows[0], half + DIVIDER + 10.0, 1000.0), Some((true, 0)));
        assert_eq!(tab.number_side_at(rows[0], 200.0, 1000.0), None, "the text, not a number");
        assert_eq!(tab.number_side_at(rows[1], 10.0, 1000.0), None, "an unchanged line");
        assert_eq!(tab.number_side_at(rows[3], 10.0, 1000.0), None, "the filler of an added line");
        tab.unified = true;
        let rows = unified_rows(&tab.view);
        assert_eq!(tab.number_side_at(rows[0], 10.0, 1000.0), Some((false, 0)));
        assert_eq!(tab.number_side_at(rows[1], NUMBERS + 10.0, 1000.0), Some((true, 0)));
        assert_eq!(tab.number_side_at(rows[1], 2.0 * NUMBERS + 10.0, 1000.0), None);
        tab.choose(true, 0, false);
        tab.current = 1;
        let fresh = parted(ChangePart::Unstaged).view;
        tab.replace(fresh, None);
        assert_eq!((tab.chosen(), tab.unified, tab.current), (0, true, 1));
    }

    #[test]
    fn jump_to_source_goes_to_the_carets_line_of_the_file() {
        let before = "a\nb\nc\nd\ne\n";
        let after = "a\nB\nB2\nc\ne\nf\n";
        let hunks =
            vec![hunk("modified", (1, 2), (1, 3)), hunk("deleted", (3, 4), (4, 4)), hunk("added", (5, 5), (5, 6))];
        let mut view = view(before, after, hunks);
        view.path = PathBuf::from("/project/src/a.go");
        let mut tab = DiffTab::new(view, None, false);
        let file = PathBuf::from("/project/src/a.go");
        assert_eq!(tab.jump_target(), Some((file.clone(), 1)), "the current difference's first line");
        tab.step(true);
        assert_eq!(tab.jump_target(), Some((file.clone(), 4)));
        tab.set_caret(true, 3);
        assert_eq!(tab.jump_target(), Some((file.clone(), 3)), "a line of the file's side");
        for (line, expected) in [(0, 0), (1, 1), (2, 3), (3, 4), (4, 4)] {
            tab.set_caret(false, line);
            assert_eq!(tab.jump_target(), Some((file.clone(), expected)), "HEAD's line {line}");
        }
        tab.swap();
        tab.set_caret(false, 3);
        assert_eq!(tab.jump_target(), Some((file.clone(), 3)), "swapped, the file is on the left");
        tab.set_caret(true, 2);
        assert_eq!(tab.jump_target(), Some((file.clone(), 3)), "swapped, HEAD is on the right");

        let width = 1000.0;
        let half = (width - DIVIDER) / 2.0;
        let rows = side_by_side_rows(&tab.view);
        assert_eq!(tab.caret_at(rows[0], 10.0, width), Some((false, 0)));
        assert_eq!(tab.caret_at(rows[0], half + DIVIDER + 10.0, width), Some((true, 0)));
        tab.unified = true;
        let rows = unified_rows(&tab.view);
        assert!(rows.iter().any(|row| row.after.is_none() && tab.caret_at(*row, 300.0, width).is_some_and(|c| !c.0)));

        let blank = DiffTab::new(DiffView::default(), None, false);
        assert_eq!(blank.jump_target(), None, "a blank diff has no file");
    }

    #[derive(Debug)]
    struct Dropped(PathBuf);

    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &Path {
            &self.0
        }

        fn bytes(&self) -> Result<Vec<u8>, String> {
            std::fs::read(&self.0).map_err(|error| error.to_string())
        }
    }

    #[test]
    fn blank_panes_take_typing_paste_and_drop_and_the_file_side_asks_to_be_written() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let mut blank = DiffView { before_editable: true, after_editable: true, ..DiffView::default() };
        blank.relative.clear();
        let mut tab = DiffTab::new(blank, None, false);
        assert_eq!(tab.title(), t!("vcs-diff-blank"));
        let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
        let ctx = egui::Context::default();
        let size = vec2(1000.0, 600.0);
        let clock = std::cell::Cell::new(0.0);
        let frame = |events: Vec<egui::Event>, dropped: Vec<egui::DroppedFileHandle>, tab: &mut DiffTab| {
            clock.set(clock.get() + 0.01);
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
                time: Some(clock.get()),
                events,
                dropped_files: dropped,
                ..Default::default()
            };
            let mut outcome = DiffOutcome::default();
            let mut output = ctx.run_ui(input, |ui| outcome = tab.show(ui, &palette));
            output.textures_delta.clear();
            outcome
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
        frame(Vec::new(), Vec::new(), &mut tab);
        let left_pane = pos2(NUMBERS + 40.0, 34.0 + 26.0 + 8.0);
        frame(click(left_pane), Vec::new(), &mut tab);
        let pasted = frame(vec![egui::Event::Paste("one\ntwo\n".to_owned())], Vec::new(), &mut tab);
        assert_eq!(tab.view.before, "one\ntwo\n", "a paste goes into the pane with the caret");
        assert!(!pasted.rehunk && pasted.write.is_none(), "a blank pane has no file");
        let typed = frame(vec![egui::Event::Text("x".to_owned())], Vec::new(), &mut tab);
        assert_eq!(tab.view.before, "one\ntwo\nx");
        assert!(!typed.rehunk, "not compared again while typing");
        assert!(!frame(Vec::new(), Vec::new(), &mut tab).rehunk);
        clock.set(clock.get() + RETYPED_AFTER);
        assert!(frame(Vec::new(), Vec::new(), &mut tab).rehunk, "compared again once typing pauses");
        assert!(!frame(Vec::new(), Vec::new(), &mut tab).rehunk);

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("other.txt");
        std::fs::write(&file, "one\n2\n").unwrap();
        let right_pane = pos2(500.0 + DIVIDER + NUMBERS + 40.0, 200.0);
        let dropped =
            frame(vec![egui::Event::PointerMoved(right_pane)], vec![Arc::new(Dropped(file.clone()))], &mut tab);
        assert_eq!(tab.view.after, "one\n2\n", "a file dropped on a pane gives it its text");
        assert_eq!(tab.view.after_title, file.display().to_string());
        assert!(dropped.rehunk);

        let view = DiffView {
            path: file.clone(),
            relative: "other.txt".into(),
            before: "clip\n".into(),
            after: "one\n".into(),
            after_editable: true,
            after_range: Some([4, 8]),
            hunks: vec![hunk("modified", (0, 1), (0, 1))],
            ..DiffView::default()
        };
        let mut tab = DiffTab::new(view, None, false);
        assert!(!tab.editable(false) && tab.editable(true));
        assert_eq!(tab.set_side(false, "no".into(), None), None, "the clipboard's side is read-only");
        let write = tab.set_side(true, "ONE\n".into(), None).unwrap().unwrap();
        assert_eq!(write, (file.clone(), Some([4, 8]), "ONE\n".to_owned()));
        tab.written([4, 8], 1, zen_text::Text::new());
        tab.swap();
        assert!(tab.editable(false) && !tab.editable(true));
        assert_eq!(tab.view.before_title, "");
        assert_eq!(tab.view.hunks[0].before_start, 0);
        let write = tab.set_side(false, "one\n".into(), None).unwrap().unwrap();
        assert_eq!(write.0, file);
        assert!(!tab.stages());
    }

    #[test]
    fn swapping_trades_the_sides_and_their_changes() {
        let mut view =
            view("a\nb\n", "a\nB\nc\n", vec![hunk("modified", (1, 2), (1, 2)), hunk("added", (2, 2), (2, 3))]);
        view.before_title = "old".into();
        view.after_title = "new".into();
        let mut tab = DiffTab::new(view, None, false);
        tab.swap();
        assert_eq!((tab.view.before.as_str(), tab.view.after.as_str()), ("a\nB\nc\n", "a\nb\n"));
        assert_eq!((tab.view.before_title.as_str(), tab.view.after_title.as_str()), ("new", "old"));
        assert_eq!(tab.view.hunks[1].kind, "deleted");
        assert_eq!((tab.view.hunks[1].before_start, tab.view.hunks[1].before_end), (2, 3));
        assert!(tab.swapped);
        assert_eq!(tab.state()["titles"], json!(["new", "old"]));
        tab.swap();
        assert_eq!(tab.view.hunks[1].kind, "added");
    }

    #[test]
    fn three_files_step_through_both_sides_changes_in_the_middle_ones_order() {
        let three = ThreeWayView {
            paths: vec!["/a".into(), "/b".into(), "/c".into()],
            titles: vec!["a".into(), "b".into(), "c".into()],
            texts: vec!["1\n2\n".into(), "1\nB\n".into(), "X\nB\n".into()],
            left_hunks: vec![hunk("modified", (1, 2), (1, 2))],
            right_hunks: vec![hunk("modified", (0, 1), (0, 1))],
            binary: false,
        };
        let mut tab = DiffTab::three(three, None);
        assert_eq!(tab.change_count(), 2);
        assert_eq!(three_changes(tab.three.as_ref().unwrap()), [(0, true, 0), (1, false, 0)]);
        tab.step(true);
        tab.step(true);
        assert_eq!(tab.current, 1);
        assert_eq!(tab.state()["sides"], 3);
        tab.swap();
        let three = tab.three.as_ref().unwrap();
        assert_eq!(three.titles, ["c", "b", "a"]);
        assert_eq!((three.left_hunks[0].before_start, three.right_hunks[0].after_start), (0, 1));
    }
}

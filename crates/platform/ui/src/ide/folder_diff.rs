use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};

use egui::{Align2, Color32, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_workbench::compare_folders::{
    self, EntryKind, EntrySide, EntryState, FolderComparison, FolderEntry, NameFilter, Progress, SyncAction, SyncItem,
};

use super::{IdePalette, Side, hint, hover};
use crate::theme;

const ROW: f32 = 22.0;
const ACTION: f32 = 40.0;
const SIZE: f32 = 80.0;
const DATE: f32 = 128.0;
const ANSWERED_ROWS: usize = 500;

type Job = (mpsc::Receiver<Result<FolderComparison, String>>, Arc<Progress>);

pub(super) struct FolderDiffTab {
    pub(super) left: PathBuf,
    pub(super) right: PathBuf,
    comparison: Option<FolderComparison>,
    job: Option<Job>,
    error: Option<String>,
    shown: [bool; 4],
    filter: String,
    parsed: NameFilter,
    rows: Vec<usize>,
    actions: HashMap<String, Option<SyncAction>>,
    selected: BTreeSet<String>,
    lead: Option<String>,
    anchor: Option<String>,
    scroll_to: Option<usize>,
    offset: i32,
    pub(super) confirm: Option<(Vec<SyncItem>, Vec<SyncItem>)>,
    pub(super) drawn_at: Option<u64>,
    pub(super) hits: Hits,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Hits {
    pub(super) body: Rect,
    pub(super) toggles: [Rect; 4],
    pub(super) filter: Rect,
    pub(super) sync: [Rect; 2],
}

#[derive(Default)]
pub(super) struct FolderOutcome {
    pub(super) open: Option<String>,
    pub(super) synchronize: Option<bool>,
    pub(super) refresh: bool,
}

fn state_index(state: EntryState) -> usize {
    EntryState::ALL.iter().position(|each| *each == state).unwrap_or(0)
}

pub(super) fn state_name(state: EntryState) -> &'static str {
    match state {
        EntryState::OnlyLeft => "onlyLeft",
        EntryState::OnlyRight => "onlyRight",
        EntryState::Different => "different",
        EntryState::Equal => "equal",
    }
}

pub(super) fn action_name(action: Option<SyncAction>) -> &'static str {
    match action {
        Some(SyncAction::ToRight) => "toRight",
        Some(SyncAction::ToLeft) => "toLeft",
        Some(SyncAction::Delete) => "delete",
        None => "none",
    }
}

fn state_color(state: EntryState, p: &IdePalette) -> Color32 {
    match state {
        EntryState::OnlyLeft | EntryState::OnlyRight => p.vcs_added,
        EntryState::Different => p.vcs_modified,
        EntryState::Equal => p.text,
    }
}

fn state_label(state: EntryState, count: usize) -> String {
    match state {
        EntryState::OnlyLeft => t!("folder-diff-only-left", count = count),
        EntryState::OnlyRight => t!("folder-diff-only-right", count = count),
        EntryState::Different => t!("folder-diff-different", count = count),
        EntryState::Equal => t!("folder-diff-equal", count = count),
    }
}

fn state_hint(state: EntryState) -> String {
    match state {
        EntryState::OnlyLeft => t!("folder-diff-show-only-left"),
        EntryState::OnlyRight => t!("folder-diff-show-only-right"),
        EntryState::Different => t!("folder-diff-show-different"),
        EntryState::Equal => t!("folder-diff-show-equal"),
    }
}

fn action_hint(action: Option<SyncAction>) -> String {
    match action {
        Some(SyncAction::ToRight) => t!("folder-diff-action-right"),
        Some(SyncAction::ToLeft) => t!("folder-diff-action-left"),
        Some(SyncAction::Delete) => t!("folder-diff-action-delete"),
        None => t!("folder-diff-action-none"),
    }
}

fn action_glyph(painter: &egui::Painter, rect: Rect, action: Option<SyncAction>, p: &IdePalette) {
    let c = rect.center();
    match action {
        Some(SyncAction::ToRight | SyncAction::ToLeft) => {
            let sign = if action == Some(SyncAction::ToRight) { 1.0 } else { -1.0 };
            let (from, to) = (c - vec2(6.0 * sign, 0.0), c + vec2(6.0 * sign, 0.0));
            painter.line_segment([from, to], Stroke::new(1.5, p.accent));
            painter.line_segment([to, to - vec2(4.0 * sign, 4.0)], Stroke::new(1.5, p.accent));
            painter.line_segment([to, to - vec2(4.0 * sign, -4.0)], Stroke::new(1.5, p.accent));
        }
        Some(SyncAction::Delete) => {
            let s = Stroke::new(1.5, p.vcs_deleted);
            painter.line_segment([c - vec2(4.5, 4.5), c + vec2(4.5, 4.5)], s);
            painter.line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], s);
        }
        None => {
            let faint = Stroke::new(1.0, p.text_secondary.gamma_multiply(0.5));
            painter.hline(c.x - 4.0..=c.x + 4.0, c.y - 2.0, faint);
            painter.hline(c.x - 4.0..=c.x + 4.0, c.y + 2.0, faint);
        }
    }
}

impl FolderDiffTab {
    pub(super) fn new(left: PathBuf, right: PathBuf) -> Self {
        Self {
            left,
            right,
            comparison: None,
            job: None,
            error: None,
            shown: [true, true, true, false],
            filter: String::new(),
            parsed: NameFilter::default(),
            rows: Vec::new(),
            actions: HashMap::new(),
            selected: BTreeSet::new(),
            lead: None,
            anchor: None,
            scroll_to: None,
            offset: zen_vcs::local_offset(),
            confirm: None,
            drawn_at: None,
            hits: Hits {
                body: Rect::NOTHING,
                toggles: [Rect::NOTHING; 4],
                filter: Rect::NOTHING,
                sync: [Rect::NOTHING; 2],
            },
        }
    }

    pub(super) fn start(&mut self, ctx: &egui::Context) {
        if let Some((_, progress)) = self.job.take() {
            progress.cancel();
        }
        let (sender, receiver) = mpsc::channel();
        let progress = Arc::new(Progress::default());
        let (left, right, shared, ctx) = (self.left.clone(), self.right.clone(), progress.clone(), ctx.clone());
        let spawned = std::thread::Builder::new().name("zen-compare-folders".to_owned()).spawn(move || {
            let _ = sender.send(compare_folders::compare(&left, &right, &shared));
            ctx.request_repaint();
        });
        match spawned {
            Ok(_) => self.job = Some((receiver, progress)),
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub(super) fn compare_now(&mut self) {
        if let Some((_, progress)) = self.job.take() {
            progress.cancel();
        }
        let result = compare_folders::compare(&self.left, &self.right, &Progress::default());
        self.take(result);
    }

    pub(super) fn poll(&mut self) -> bool {
        let Some((receiver, _)) = &self.job else { return false };
        match receiver.try_recv() {
            Ok(result) => {
                self.job = None;
                self.take(result);
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.job = None;
                false
            }
        }
    }

    pub(super) fn cancel(&self) {
        if let Some((_, progress)) = &self.job {
            progress.cancel();
        }
    }

    pub(super) fn comparing(&self) -> bool {
        self.job.is_some()
    }

    pub(super) fn wait(&mut self, limit: std::time::Duration) -> bool {
        let Some((receiver, _)) = &self.job else { return false };
        match receiver.recv_timeout(limit) {
            Ok(result) => {
                self.job = None;
                self.take(result);
                true
            }
            Err(_) => false,
        }
    }

    fn take(&mut self, result: Result<FolderComparison, String>) {
        match result {
            Ok(comparison) => {
                self.error = None;
                let known: BTreeSet<&str> = comparison.entries.iter().map(|entry| entry.relative.as_str()).collect();
                self.selected.retain(|relative| known.contains(relative.as_str()));
                self.actions.retain(|relative, _| known.contains(relative.as_str()));
                if self.lead.as_ref().is_some_and(|lead| !known.contains(lead.as_str())) {
                    self.lead = None;
                }
                self.comparison = Some(comparison);
            }
            Err(error) => self.error = Some(error),
        }
        self.refilter();
    }

    fn refilter(&mut self) {
        let Some(comparison) = &self.comparison else {
            self.rows.clear();
            return;
        };
        self.rows = comparison
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| self.shown[state_index(entry.state)] && self.parsed.matches(&entry.relative))
            .map(|(index, _)| index)
            .collect();
    }

    pub(super) fn title(&self) -> String {
        let name = |path: &Path| {
            path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned())
        };
        t!("folder-diff-title", left = name(&self.left), right = name(&self.right))
    }

    pub(super) fn set_shown(&mut self, state: EntryState, shown: bool) {
        self.shown[state_index(state)] = shown;
        self.refilter();
    }

    pub(super) fn set_filter(&mut self, filter: &str) {
        filter.clone_into(&mut self.filter);
        self.parsed = NameFilter::parse(filter);
        self.refilter();
    }

    pub(super) fn entry(&self, relative: &str) -> Option<&FolderEntry> {
        self.comparison.as_ref()?.entries.iter().find(|entry| entry.relative == relative)
    }

    pub(super) fn action_of(&self, entry: &FolderEntry) -> Option<SyncAction> {
        self.actions.get(&entry.relative).copied().unwrap_or_else(|| entry.default_action())
    }

    pub(super) fn set_action(&mut self, relative: &str, action: Option<SyncAction>) -> bool {
        let Some(entry) = self.entry(relative) else { return false };
        if action.is_some_and(|action| !entry.actions().contains(&action)) {
            return false;
        }
        self.actions.insert(relative.to_owned(), action);
        true
    }

    fn cycle(&mut self, index: usize) {
        let Some(entry) = self.comparison.as_ref().and_then(|comparison| comparison.entries.get(index)) else { return };
        let choices: Vec<Option<SyncAction>> =
            entry.actions().iter().copied().map(Some).chain(std::iter::once(None)).collect();
        let current = self.action_of(entry);
        let at = choices.iter().position(|choice| *choice == current).unwrap_or(0);
        let next = choices[(at + 1) % choices.len()];
        self.actions.insert(entry.relative.clone(), next);
    }

    pub(super) fn select(&mut self, relatives: &[String]) {
        self.selected = relatives.iter().filter(|relative| self.entry(relative).is_some()).cloned().collect();
        self.lead = relatives.first().filter(|relative| self.selected.contains(*relative)).cloned();
        self.anchor = self.lead.clone();
        self.scroll_to = self.lead.as_ref().and_then(|lead| self.row_of(lead));
    }

    pub(super) fn lead(&self) -> Option<&str> {
        self.lead.as_deref()
    }

    fn row_of(&self, relative: &str) -> Option<usize> {
        let entries = &self.comparison.as_ref()?.entries;
        self.rows.iter().position(|index| entries[*index].relative == relative)
    }

    pub(super) fn items(&self, all: bool) -> Vec<SyncItem> {
        let Some(comparison) = &self.comparison else { return Vec::new() };
        self.rows
            .iter()
            .map(|index| &comparison.entries[*index])
            .filter(|entry| all || self.selected.contains(&entry.relative))
            .filter_map(|entry| {
                self.action_of(entry).map(|action| SyncItem { relative: entry.relative.clone(), action })
            })
            .collect()
    }

    pub(super) fn state(&self) -> Value {
        self.state_hiding(&BTreeSet::new())
    }

    pub(super) fn agent_state(&self, session: &zen_core::Session) -> Value {
        let patterns = session
            .state::<zen_workbench::hidden_files::HiddenFromAgents>()
            .map(|hidden| hidden.patterns().to_vec())
            .unwrap_or_default();
        let Some(comparison) = self.comparison.as_ref().filter(|_| !patterns.is_empty()) else {
            return self.state();
        };
        let root = session.state::<zen_workbench::Workbench>().and_then(zen_workbench::Workbench::project);
        let hidden: BTreeSet<String> = comparison
            .entries
            .iter()
            .filter(|entry| compare_folders::is_hidden_entry(&self.left, &self.right, &entry.relative, &patterns, root))
            .map(|entry| entry.relative.clone())
            .collect();
        let mut state = self.state_hiding(&hidden);
        state["hiddenFiles"] = json!(hidden.len());
        state
    }

    fn state_hiding(&self, hidden: &BTreeSet<String>) -> Value {
        let visible = |relative: &String| !hidden.contains(relative);
        let rows: Vec<&FolderEntry> = self
            .comparison
            .iter()
            .flat_map(|comparison| self.rows.iter().map(|index| &comparison.entries[*index]))
            .filter(|entry| visible(&entry.relative))
            .collect();
        let entries: Vec<Value> = rows
            .iter()
            .take(ANSWERED_ROWS)
            .map(|entry| {
                json!({
                    "relative": entry.relative,
                    "state": state_name(entry.state),
                    "action": action_name(self.action_of(entry)),
                    "selected": self.selected.contains(&entry.relative),
                })
            })
            .collect();
        let shown: Vec<&str> = EntryState::ALL
            .iter()
            .filter(|state| self.shown[state_index(**state)])
            .map(|state| state_name(*state))
            .collect();
        let counts = self.comparison.as_ref().map(|comparison| {
            if hidden.is_empty() {
                return comparison.counts;
            }
            let mut counts = compare_folders::StateCounts::default();
            for entry in comparison.entries.iter().filter(|entry| visible(&entry.relative)) {
                match entry.state {
                    EntryState::OnlyLeft => counts.only_left += 1,
                    EntryState::OnlyRight => counts.only_right += 1,
                    EntryState::Different => counts.different += 1,
                    EntryState::Equal => counts.equal += 1,
                }
            }
            counts
        });
        let selected: Vec<&String> = self.selected.iter().filter(|relative| visible(relative)).collect();
        let confirming = self
            .confirm
            .as_ref()
            .map(|(_, risky)| risky.iter().filter(|item| visible(&item.relative)).cloned().collect::<Vec<SyncItem>>());
        json!({
            "left": self.left,
            "right": self.right,
            "title": self.title(),
            "comparing": self.comparing(),
            "error": self.error,
            "counts": counts,
            "contentsRead": self.comparison.as_ref().map(|comparison| comparison.contents_read),
            "shown": shown,
            "filter": self.filter,
            "rows": rows.len(),
            "entries": entries,
            "truncated": rows.len() > ANSWERED_ROWS,
            "selected": selected,
            "lead": self.lead.as_ref().filter(|lead| visible(lead)),
            "confirming": confirming,
        })
    }

    fn select_row(&mut self, row: usize, modifiers: egui::Modifiers) {
        let Some(comparison) = &self.comparison else { return };
        let Some(relative) = self.rows.get(row).map(|index| comparison.entries[*index].relative.clone()) else {
            return;
        };
        if modifiers.shift
            && let Some(anchor) = self.anchor.clone().and_then(|anchor| self.row_of(&anchor))
        {
            let (from, to) = (anchor.min(row), anchor.max(row));
            self.selected =
                self.rows[from..=to].iter().map(|index| comparison.entries[*index].relative.clone()).collect();
        } else if modifiers.command {
            if !self.selected.remove(&relative) {
                self.selected.insert(relative.clone());
            }
            self.anchor = Some(relative.clone());
        } else {
            self.selected = BTreeSet::from([relative.clone()]);
            self.anchor = Some(relative.clone());
        }
        self.lead = Some(relative);
    }

    fn keys(&mut self, ui: &Ui, outcome: &mut FolderOutcome) {
        let (down, up, enter, all) = ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
                    || input.consume_key(egui::Modifiers::SHIFT, Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, Key::ArrowUp)
                    || input.consume_key(egui::Modifiers::SHIFT, Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, Key::Enter),
                input.consume_key(egui::Modifiers::COMMAND, Key::A),
            )
        });
        let shift = ui.input(|input| input.modifiers.shift);
        if (down || up) && !self.rows.is_empty() {
            let current = self.lead.clone().and_then(|lead| self.row_of(&lead));
            let next = match current {
                Some(row) if down => (row + 1).min(self.rows.len() - 1),
                Some(row) => row.saturating_sub(1),
                None => 0,
            };
            let modifiers = if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
            self.select_row(next, modifiers);
            self.scroll_to = Some(next);
        }
        if all && let Some(comparison) = &self.comparison {
            self.selected = self.rows.iter().map(|index| comparison.entries[*index].relative.clone()).collect();
        }
        if enter {
            outcome.open = self.lead.clone();
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, keys: bool) -> FolderOutcome {
        let mut outcome = FolderOutcome::default();
        self.poll();
        if self.comparing() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if keys {
            self.keys(ui, &mut outcome);
        }
        self.toolbar(ui, p, &mut outcome);
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW + 4.0), Sense::hover());
        let half = (header.width() - ACTION) / 2.0;
        ui.painter().rect_filled(header, 0, p.frame);
        for (index, root) in [&self.left, &self.right].into_iter().enumerate() {
            let x = header.left() + index as f32 * (half + ACTION) + 10.0;
            let text = root.display().to_string();
            let clip = ui
                .painter()
                .with_clip_rect(Rect::from_min_size(pos2(x, header.top()), vec2(half - 14.0, header.height())));
            clip.text(pos2(x, header.center().y), Align2::LEFT_CENTER, text, theme::regular(12.0), p.text_secondary);
        }
        ui.painter().hline(header.x_range(), header.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
        let body = ui.available_rect_before_wrap();
        self.hits.body = body;
        if let Some(error) = &self.error {
            ui.painter().text(body.center(), Align2::CENTER_CENTER, error, theme::regular(13.0), p.error_text);
            return outcome;
        }
        if self.rows.is_empty() {
            let message = match (&self.comparison, &self.job) {
                (None, _) | (_, Some(_)) => {
                    t!(
                        "folder-diff-comparing",
                        count = self.job.as_ref().map_or(0, |(_, progress)| progress.examined())
                    )
                }
                (Some(comparison), None) if comparison.entries.is_empty() => t!("folder-diff-empty"),
                _ => t!("folder-diff-nothing-shown"),
            };
            ui.painter().text(body.center(), Align2::CENTER_CENTER, message, theme::regular(13.0), p.text_secondary);
            return outcome;
        }
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]).id_salt("folder-diff-rows");
        if let Some(row) = self.scroll_to.take() {
            let top = row as f32 * ROW;
            let current =
                ui.ctx().data(|data| data.get_temp::<f32>(ui.scope_id().with("folder-diff-offset"))).unwrap_or(0.0);
            let height = body.height();
            let offset = if top < current {
                top
            } else if top + ROW > current + height {
                top + ROW - height
            } else {
                current
            };
            area = area.vertical_scroll_offset(offset.max(0.0));
        }
        let count = self.rows.len();
        let mut clicked = None;
        let mut cycled = None;
        let mut opened = None;
        let output = area.show_rows(ui, ROW, count, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let Some(comparison) = &self.comparison else { return };
            for row in range {
                let entry = &comparison.entries[self.rows[row]];
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                let selected = self.selected.contains(&entry.relative);
                if selected {
                    ui.painter().rect_filled(rect, 0, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 0, p.hover);
                }
                let half = (rect.width() - ACTION) / 2.0;
                let color = state_color(entry.state, p);
                self.side(ui, rect.left(), half, rect, entry, entry.left.as_ref(), color, p);
                self.side(ui, rect.left() + half + ACTION, half, rect, entry, entry.right.as_ref(), color, p);
                let cell = Rect::from_center_size(
                    pos2(rect.left() + half + ACTION / 2.0, rect.center().y),
                    vec2(26.0, ROW - 2.0),
                );
                let action = self.action_of(entry);
                if !entry.actions().is_empty() {
                    let button =
                        ui.interact(cell, ui.scope_id().with(("folder-diff-action", &entry.relative)), Sense::click());
                    hover(ui, &button, p);
                    hint(ui, &button, &action_hint(action), None, Side::Below);
                    if button.clicked() {
                        cycled = Some(self.rows[row]);
                    }
                    action_glyph(ui.painter(), cell, action, p);
                } else {
                    action_glyph(ui.painter(), cell, None, p);
                }
                ui.painter().vline(rect.left() + half, rect.y_range(), Stroke::new(1.0, p.tab_border));
                ui.painter().vline(rect.left() + half + ACTION, rect.y_range(), Stroke::new(1.0, p.tab_border));
                if response.double_clicked() {
                    opened = Some(entry.relative.clone());
                } else if response.clicked() {
                    clicked = Some((row, ui.input(|input| input.modifiers)));
                }
            }
        });
        ui.ctx().data_mut(|data| {
            data.insert_temp(ui.scope_id().with("folder-diff-offset"), output.state.clamped_offset().y)
        });
        if let Some(index) = cycled {
            self.cycle(index);
        } else if let Some((row, modifiers)) = clicked {
            self.select_row(row, modifiers);
        }
        if let Some(relative) = opened {
            self.select_row(self.row_of(&relative).unwrap_or(0), egui::Modifiers::NONE);
            outcome.open = Some(relative);
        }
        outcome
    }

    #[allow(clippy::too_many_arguments)]
    fn side(
        &self,
        ui: &Ui,
        x: f32,
        width: f32,
        rect: Rect,
        entry: &FolderEntry,
        side: Option<&EntrySide>,
        color: Color32,
        p: &IdePalette,
    ) {
        let Some(side) = side else { return };
        let y = rect.center().y;
        let name_width = (width - SIZE - DATE - 20.0).max(40.0);
        let icon = Rect::from_center_size(pos2(x + 18.0, y), Vec2::splat(14.0));
        if side.kind == EntryKind::Folder {
            crate::file_icons::folder_icon(ui.painter(), icon, entry.name(), false, false);
        } else {
            crate::file_icons::file_icon(ui.painter(), icon, entry.name());
        }
        let clip = ui
            .painter()
            .with_clip_rect(Rect::from_min_max(pos2(x + 28.0, rect.top()), pos2(x + 28.0 + name_width, rect.bottom())));
        clip.text(pos2(x + 28.0, y), Align2::LEFT_CENTER, &entry.relative, theme::regular(13.0), color);
        if side.kind != EntryKind::Folder {
            let size = super::image_view::file_size_text(side.size);
            ui.painter().text(
                pos2(x + width - DATE - 14.0, y),
                Align2::RIGHT_CENTER,
                size,
                theme::regular(12.0),
                p.text_secondary,
            );
        }
        if let Some(modified) = side.modified {
            let seconds = i64::try_from(modified / 1000).unwrap_or(0) + i64::from(self.offset);
            let date = super::vcs::format_date(seconds);
            ui.painter().text(
                pos2(x + width - 8.0, y),
                Align2::RIGHT_CENTER,
                date,
                theme::regular(12.0),
                p.text_secondary,
            );
        }
    }

    fn toolbar(&mut self, ui: &mut Ui, p: &IdePalette, outcome: &mut FolderOutcome) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        let y = bar.center().y;
        let mut limit = bar.right() - 10.0;
        for (all, label) in [(true, t!("folder-diff-sync-all")), (false, t!("folder-diff-sync-selected"))] {
            let galley = ui.painter().layout_no_wrap(label, theme::regular(12.5), p.text);
            let button = Rect::from_min_max(pos2(limit - galley.size().x - 20.0, y - 12.0), pos2(limit, y + 12.0));
            self.hits.sync[usize::from(!all)] = button;
            let enabled = !self.items(all).is_empty() && !self.comparing();
            let response = ui.interact(button, ui.scope_id().with(("folder-diff-sync", all)), Sense::click());
            crate::accessibility::name(&response, egui::Role::Button, galley.text());
            if enabled {
                hover(ui, &response, p);
            }
            ui.painter().rect_stroke(button, 6, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
            let color = if enabled { p.text } else { p.text_secondary.gamma_multiply(0.6) };
            ui.painter().galley_with_override_text_color(button.center() - galley.size() / 2.0, galley, color);
            if response.clicked() && enabled {
                outcome.synchronize = Some(all);
            }
            limit = button.left() - 6.0;
        }
        if let Some((_, progress)) = &self.job {
            let status = ui.painter().text(
                pos2(limit - 6.0, y),
                Align2::RIGHT_CENTER,
                t!("folder-diff-comparing", count = progress.examined()),
                theme::regular(12.0),
                p.text_secondary,
            );
            limit = status.left() - 8.0;
        }
        let mut x = bar.left() + 10.0;
        let refresh = Rect::from_min_size(pos2(x, y - 12.0), Vec2::splat(24.0));
        let response = ui.interact(refresh, ui.scope_id().with("folder-diff-refresh"), Sense::click());
        crate::accessibility::name(&response, egui::Role::Button, &t!("folder-diff-refresh"));
        hover(ui, &response, p);
        hint(ui, &response, &t!("folder-diff-refresh"), None, Side::Below);
        super::glyphs::glyph_refresh(ui.painter(), refresh.shrink(5.0), p.text_secondary);
        if response.clicked() {
            outcome.refresh = true;
        }
        x = refresh.right() + 8.0;
        let counts = self.comparison.as_ref().map(|comparison| comparison.counts).unwrap_or_default();
        let galleys = |compact: bool| -> Vec<Arc<egui::Galley>> {
            EntryState::ALL
                .iter()
                .map(|state| {
                    let count = counts.of(*state);
                    let label = if compact { count.to_string() } else { state_label(*state, count) };
                    ui.painter().layout_no_wrap(label, theme::regular(12.5), p.text)
                })
                .collect()
        };
        let mut labels = galleys(false);
        let wide: f32 = labels.iter().map(|galley| galley.size().x + 30.0).sum();
        if x + wide + 160.0 > limit {
            labels = galleys(true);
        }
        for (state, galley) in EntryState::ALL.into_iter().zip(labels) {
            let on = self.shown[state_index(state)];
            let toggle = Rect::from_min_size(pos2(x, y - 12.0), vec2(galley.size().x + 26.0, 24.0));
            self.hits.toggles[state_index(state)] = toggle;
            let response =
                ui.interact(toggle, ui.scope_id().with(("folder-diff-state", state_index(state))), Sense::click());
            crate::accessibility::item(&response, egui::Role::CheckBox, on, &state_hint(state));
            if on {
                ui.painter().rect_filled(toggle, 6, p.active_tab);
                ui.painter().rect_stroke(toggle, 6, Stroke::new(1.0, p.active_tab_border), egui::StrokeKind::Inside);
            } else {
                hover(ui, &response, p);
            }
            hint(ui, &response, &state_hint(state), None, Side::Below);
            ui.painter().circle_filled(pos2(toggle.left() + 11.0, y), 4.0, state_color(state, p));
            ui.painter().galley(pos2(toggle.left() + 20.0, y - galley.size().y / 2.0), galley, p.text);
            if response.clicked() {
                self.set_shown(state, !on);
            }
            x = toggle.right() + 4.0;
        }
        x += 8.0;
        let field_rect = Rect::from_min_size(pos2(x, y - 12.0), vec2((limit - x).clamp(80.0, 260.0), 24.0));
        self.hits.filter = field_rect;
        ui.painter().rect(field_rect, 6, p.text_field, Stroke::new(1.0, p.text_field_border), egui::StrokeKind::Inside);
        let mut text = self.filter.clone();
        let field = ui.put(
            field_rect.shrink2(vec2(8.0, 2.0)),
            egui::TextEdit::singleline(&mut text)
                .id(ui.scope_id().with("folder-diff-filter"))
                .frame(egui::Frame::NONE)
                .font(theme::regular(12.5))
                .hint_text(t!("folder-diff-filter-hint"))
                .desired_width(field_rect.width() - 16.0),
        );
        hint(ui, &field, &t!("folder-diff-filter-tip"), None, Side::Below);
        if field.changed() {
            self.set_filter(&text);
        }
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
    }
}

pub(super) fn confirm_dialog(ui: &mut Ui, p: &IdePalette, items: &[SyncItem]) -> Option<bool> {
    let mut choice = None;
    let overwritten = items.iter().filter(|item| item.action != SyncAction::Delete).count();
    let deleted = items.len() - overwritten;
    let modal = egui::Modal::new(ui.scope_id().with("folder-diff-confirm")).frame(super::dialog_frame(p)).show(
        ui.ctx(),
        |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("folder-diff-confirm-title")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(12.0);
            let question = t!("folder-diff-confirm-question", overwritten = overwritten, deleted = deleted);
            ui.add(egui::Label::new(egui::RichText::new(question).font(theme::regular(13.5)).color(p.text)).wrap());
            ui.add_space(8.0);
            for item in items.iter().take(8) {
                let line = match item.action {
                    SyncAction::ToRight => format!("→ {}", item.relative),
                    SyncAction::ToLeft => format!("← {}", item.relative),
                    SyncAction::Delete => format!("× {}", item.relative),
                };
                ui.label(egui::RichText::new(line).font(theme::regular(12.5)).color(p.text_secondary));
                ui.add_space(2.0);
            }
            if items.len() > 8 {
                ui.label(
                    egui::RichText::new(t!("folder-diff-confirm-more", count = items.len() - 8))
                        .font(theme::regular(12.5))
                        .color(p.text_secondary),
                );
            }
            ui.add_space(18.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let go = egui::Button::new(
                    egui::RichText::new(t!("folder-diff-confirm-go")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(96.0, 30.0))
                .corner_radius(6)
                .fill(p.accent);
                if ui.add(go).clicked() {
                    choice = Some(true);
                }
                if super::vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    choice = Some(false);
                }
            });
            if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
                choice = Some(true);
            }
        },
    );
    choice.or_else(|| modal.should_close().then_some(false))
}

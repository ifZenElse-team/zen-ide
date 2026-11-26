use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_syntax::Highlighter;
use zen_workbench::Workbench;
use zen_workbench::local_history::{ChangedFile, FileStatus, VersionDiff, VersionInfo, VersionList};

use super::diff_view::DiffTab;
use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

const ROW: f32 = 44.0;
const FILE_ROW: f32 = 24.0;
const LIST_WIDTH: f32 = 290.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum HistoryAction {
    Select(usize),
    SelectFile(usize),
    Revert,
    Close,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum HistoryOutcome {
    Notice(String),
    Closed,
}

#[derive(Default)]
pub(super) struct LocalHistoryDialog {
    window: Option<HistoryWindow>,
    label: Option<LabelForm>,
}

impl std::fmt::Debug for LocalHistoryDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHistoryDialog")
            .field("window", &self.window.as_ref().map(|window| &window.path))
            .field("label", &self.label.is_some())
            .finish()
    }
}

struct HistoryWindow {
    path: PathBuf,
    loaded: bool,
    folder: bool,
    versions: Vec<VersionInfo>,
    selected: Option<usize>,
    files: Vec<ChangedFile>,
    selected_file: Option<usize>,
    diff: Option<DiffTab>,
    message: Option<String>,
    offset: i64,
}

struct LabelForm {
    text: String,
    focused: bool,
    error: Option<String>,
}

impl LocalHistoryDialog {
    pub(super) fn open(&mut self, path: PathBuf) {
        self.window = Some(HistoryWindow {
            path,
            loaded: false,
            folder: false,
            versions: Vec::new(),
            selected: None,
            files: Vec::new(),
            selected_file: None,
            diff: None,
            message: None,
            offset: i64::from(zen_vcs::local_offset()),
        });
    }

    pub(super) fn open_label(&mut self) {
        self.label = Some(LabelForm { text: String::new(), focused: false, error: None });
    }

    pub(super) fn is_open(&self) -> bool {
        self.window.is_some() || self.label.is_some()
    }

    pub(super) fn is_history_open(&self) -> bool {
        self.window.is_some()
    }

    pub(super) fn is_label_open(&self) -> bool {
        self.label.is_some()
    }

    pub(super) fn update(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn Highlighter>>,
    ) -> Vec<HistoryOutcome> {
        let mut outcomes = Vec::new();
        if let Some(window) = &mut self.window
            && !window.loaded
            && let Err(message) = window.load(registry, session, highlighter)
        {
            outcomes.extend(message.map(HistoryOutcome::Notice));
            self.window = None;
        }
        if let Some(window) = &mut self.window
            && let Some(action) = window.show(ui, p)
            && window.act(&action, registry, session, highlighter, &mut outcomes)
        {
            self.window = None;
            outcomes.push(HistoryOutcome::Closed);
        }
        if let Some(form) = &mut self.label {
            match form.show(ui, p) {
                Some(Some(label)) => {
                    match run(registry, session, "localHistory.label.put", json!({ "label": label })) {
                        Ok(_) => {
                            self.label = None;
                            outcomes.push(HistoryOutcome::Closed);
                        }
                        Err(message) => form.error = message,
                    }
                }
                Some(None) => {
                    self.label = None;
                    outcomes.push(HistoryOutcome::Closed);
                }
                None => {}
            }
        }
        outcomes
    }

    #[cfg(test)]
    pub(super) fn act(
        &mut self,
        action: &HistoryAction,
        registry: &Registry,
        session: &mut Session,
    ) -> Vec<HistoryOutcome> {
        let mut outcomes = Vec::new();
        if let Some(window) = &mut self.window
            && window.act(action, registry, session, &|_| None, &mut outcomes)
        {
            self.window = None;
            outcomes.push(HistoryOutcome::Closed);
        }
        outcomes
    }
}

impl HistoryWindow {
    fn load(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn Highlighter>>,
    ) -> Result<(), Option<String>> {
        let value = run(registry, session, "localHistory.versions.list", json!({ "path": self.path }))?;
        let list: VersionList = serde_json::from_value(value).map_err(|error| Some(error.to_string()))?;
        self.loaded = true;
        self.folder = list.folder;
        self.versions = list.versions;
        self.files.clear();
        self.selected_file = None;
        self.diff = None;
        self.selected = None;
        if !self.versions.is_empty() {
            self.select(0, registry, session, highlighter);
        }
        Ok(())
    }

    fn select(
        &mut self,
        index: usize,
        registry: &Registry,
        session: &mut Session,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn Highlighter>>,
    ) {
        let Some(version) = self.versions.get(index) else { return };
        self.selected = Some(index);
        self.message = None;
        self.diff = None;
        let id = version.id;
        if self.folder {
            match self.compare(registry, session, &self.path.clone(), id) {
                Ok(diff) => self.files = diff.files,
                Err(message) => self.message = message,
            }
            self.selected_file = None;
            if !self.files.is_empty() {
                self.select_file(0, registry, session, highlighter);
            }
        } else {
            self.show_diff(&self.path.clone(), id, registry, session, highlighter);
        }
    }

    fn select_file(
        &mut self,
        index: usize,
        registry: &Registry,
        session: &mut Session,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn Highlighter>>,
    ) {
        let (Some(file), Some(version)) =
            (self.files.get(index), self.selected.and_then(|selected| self.versions.get(selected)))
        else {
            return;
        };
        let (path, id) = (file.path.clone(), version.id);
        self.selected_file = Some(index);
        self.show_diff(&path, id, registry, session, highlighter);
    }

    fn compare(
        &self,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        id: u64,
    ) -> Result<VersionDiff, Option<String>> {
        let value = run(registry, session, "localHistory.version.diff", json!({ "path": path, "id": id }))?;
        serde_json::from_value(value).map_err(|error| Some(error.to_string()))
    }

    fn show_diff(
        &mut self,
        path: &Path,
        id: u64,
        registry: &Registry,
        session: &mut Session,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn Highlighter>>,
    ) {
        match self.compare(registry, session, path, id) {
            Ok(VersionDiff { diff: Some(mut view), .. }) => {
                if let Some(version) = self.selected.and_then(|selected| self.versions.get(selected)) {
                    view.before_title = format!("{} · {}", when(version.time, self.offset), view.before_title);
                }
                let name = zen_vfs::file_types::known_name_at(&view.path, None).unwrap_or_default();
                self.diff = Some(DiffTab::new(view, highlighter(&name), false));
            }
            Ok(_) => self.diff = None,
            Err(message) => self.message = message,
        }
    }

    fn act(
        &mut self,
        action: &HistoryAction,
        registry: &Registry,
        session: &mut Session,
        highlighter: &dyn Fn(&str) -> Option<Arc<dyn Highlighter>>,
        outcomes: &mut Vec<HistoryOutcome>,
    ) -> bool {
        match action {
            HistoryAction::Close => return true,
            HistoryAction::Select(index) => self.select(*index, registry, session, highlighter),
            HistoryAction::SelectFile(index) => self.select_file(*index, registry, session, highlighter),
            HistoryAction::Revert => {
                let Some(version) = self.selected.and_then(|selected| self.versions.get(selected)) else {
                    return false;
                };
                let params = json!({ "path": self.path, "id": version.id });
                match run(registry, session, "localHistory.version.revert", params) {
                    Ok(value) => {
                        let count = value["files"].as_array().map_or(0, Vec::len);
                        outcomes.push(HistoryOutcome::Notice(t!("ide-local-history-reverted", count = count)));
                        return true;
                    }
                    Err(message) => self.message = message,
                }
            }
        }
        false
    }

    fn title(&self) -> String {
        let name = self
            .path
            .file_name()
            .map_or_else(|| self.path.display().to_string(), |name| name.to_string_lossy().into_owned());
        t!("ide-local-history-title", name = name.as_str())
    }

    fn show(&mut self, ui: &Ui, p: &IdePalette) -> Option<HistoryAction> {
        let mut action = None;
        let screen = ui.ctx().content_rect();
        let size = vec2((screen.width() * 0.86).clamp(560.0, 1240.0), (screen.height() * 0.82).clamp(360.0, 860.0));
        let modal = egui::Modal::new(ui.scope_id().with("local-history")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(size.x);
            ui.set_height(size.y);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(self.title()).font(theme::semibold(15.0)).color(p.text));
            });
            ui.add_space(12.0);
            let (body, _) = ui.allocate_exact_size(vec2(size.x, size.y - 84.0), Sense::hover());
            let list = Rect::from_min_size(body.min, vec2(LIST_WIDTH, body.height()));
            let right = Rect::from_min_max(pos2(list.right() + 12.0, body.top()), body.max);
            ui.painter().rect_stroke(list, 6, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
            let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list.shrink(1.0)));
            if let Some(index) = self.versions_list(&mut list_ui, p) {
                action = Some(HistoryAction::Select(index));
            }
            let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(right));
            if let Some(found) = self.details(&mut right_ui, p) {
                action = Some(found);
            }
            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let close = egui::Button::new(
                    egui::RichText::new(t!("ide-local-history-close")).font(theme::regular(13.0)).color(p.text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border));
                if ui.add(close).clicked() {
                    action = Some(HistoryAction::Close);
                }
                let can_revert = self.selected.is_some_and(|selected| {
                    self.versions.get(selected).is_some_and(|version| self.folder || has_text(version))
                });
                let revert = egui::Button::new(
                    egui::RichText::new(t!("ide-local-history-revert")).font(theme::regular(13.0)).color(p.accent_text),
                )
                .min_size(vec2(84.0, 30.0))
                .corner_radius(6)
                .fill(if can_revert { p.accent } else { p.accent.gamma_multiply(0.4) });
                if ui.add_enabled(can_revert, revert).clicked() {
                    action = Some(HistoryAction::Revert);
                }
                if let Some(message) = &self.message {
                    ui.label(egui::RichText::new(message).font(theme::regular(12.5)).color(p.excluded));
                }
            });
        });
        if action.is_none() && modal.should_close() {
            action = Some(HistoryAction::Close);
        }
        action
    }

    fn versions_list(&self, ui: &mut Ui, p: &IdePalette) -> Option<usize> {
        if self.versions.is_empty() {
            let rect = ui.max_rect();
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                t!("ide-local-history-empty"),
                theme::regular(13.0),
                p.text_secondary,
            );
            return None;
        }
        let mut chosen = None;
        egui::ScrollArea::vertical().id_salt("local-history-versions").auto_shrink([false, false]).show(ui, |ui| {
            for (index, version) in self.versions.iter().enumerate() {
                let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                let selected = self.selected == Some(index);
                if selected {
                    ui.painter().rect_filled(row.shrink2(vec2(4.0, 1.0)), 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(row.shrink2(vec2(4.0, 1.0)), 5, p.hover);
                }
                let label = version.kind == zen_workbench::local_history::VersionKind::Label;
                ui.painter().text(
                    pos2(row.left() + 14.0, row.top() + 13.0),
                    Align2::LEFT_CENTER,
                    when(version.time, self.offset),
                    theme::regular(13.0),
                    p.text,
                );
                let detail = match (&version.relative, self.folder) {
                    (Some(relative), true) => format!("{} · {}", version.title, relative),
                    _ => version.title.clone(),
                };
                let font = if label { theme::semibold(12.5) } else { theme::regular(12.5) };
                let color = if label { p.accent } else { p.text_secondary };
                let galley = ui.painter().layout_job(super::vcs::truncated(&detail, font, color, row.width() - 28.0));
                ui.painter().galley(pos2(row.left() + 14.0, row.top() + 24.0), galley, color);
                if response.clicked() && !selected {
                    chosen = Some(index);
                }
            }
        });
        let (up, down) =
            ui.input(|input| (input.key_pressed(egui::Key::ArrowUp), input.key_pressed(egui::Key::ArrowDown)));
        match self.selected {
            Some(selected) if up && selected > 0 => chosen = Some(selected - 1),
            Some(selected) if down && selected + 1 < self.versions.len() => chosen = Some(selected + 1),
            _ => {}
        }
        chosen
    }

    fn details(&mut self, ui: &mut Ui, p: &IdePalette) -> Option<HistoryAction> {
        let mut action = None;
        let area = ui.max_rect();
        let mut diff_area = area;
        if self.folder {
            let height = (self.files.len() as f32 * FILE_ROW + 8.0).clamp(FILE_ROW + 8.0, area.height() * 0.35);
            let files = Rect::from_min_size(area.min, vec2(area.width(), height));
            diff_area = Rect::from_min_max(pos2(area.left(), files.bottom() + 10.0), area.max);
            ui.painter().rect_stroke(files, 6, Stroke::new(1.0, p.button_border), egui::StrokeKind::Inside);
            let mut files_ui = ui.new_child(egui::UiBuilder::new().max_rect(files.shrink(4.0)));
            if self.files.is_empty() {
                files_ui.painter().text(
                    files.center(),
                    Align2::CENTER_CENTER,
                    t!("ide-local-history-no-changes"),
                    theme::regular(12.5),
                    p.text_secondary,
                );
            }
            egui::ScrollArea::vertical().id_salt("local-history-files").auto_shrink([false, false]).show(
                &mut files_ui,
                |ui| {
                    for (index, file) in self.files.iter().enumerate() {
                        let (row, response) =
                            ui.allocate_exact_size(vec2(ui.available_width(), FILE_ROW), Sense::click());
                        if self.selected_file == Some(index) {
                            ui.painter().rect_filled(row, 4, p.selection);
                        } else if response.hovered() {
                            ui.painter().rect_filled(row, 4, p.hover);
                        }
                        let (status, color) = match file.status {
                            FileStatus::Modified => (t!("ide-local-history-modified"), p.vcs_modified),
                            FileStatus::Deleted => (t!("ide-local-history-deleted"), p.vcs_deleted),
                            FileStatus::Added => (t!("ide-local-history-added"), p.vcs_added),
                        };
                        let name = ui.painter().text(
                            pos2(row.left() + 8.0, row.center().y),
                            Align2::LEFT_CENTER,
                            &file.relative,
                            theme::regular(13.0),
                            color,
                        );
                        ui.painter().text(
                            pos2(name.right() + 10.0, row.center().y),
                            Align2::LEFT_CENTER,
                            status,
                            theme::regular(12.0),
                            p.text_secondary,
                        );
                        if response.clicked() {
                            action = Some(HistoryAction::SelectFile(index));
                        }
                    }
                },
            );
        }
        let mut diff_ui = ui.new_child(egui::UiBuilder::new().max_rect(diff_area));
        diff_ui.set_clip_rect(diff_area);
        match &mut self.diff {
            Some(diff) => {
                diff.show(&mut diff_ui, p);
            }
            None => {
                let text = if self.versions.is_empty() || self.folder {
                    String::new()
                } else {
                    t!("ide-local-history-no-diff")
                };
                diff_ui.painter().text(
                    diff_area.center(),
                    Align2::CENTER_CENTER,
                    text,
                    theme::regular(13.0),
                    p.text_secondary,
                );
            }
        }
        action
    }
}

fn has_text(version: &VersionInfo) -> bool {
    version.has_content || version.kind == zen_workbench::local_history::VersionKind::Label
}

impl LabelForm {
    fn show(&mut self, ui: &Ui, p: &IdePalette) -> Option<Option<String>> {
        let mut answer = None;
        let valid = !self.text.trim().is_empty();
        let modal =
            egui::Modal::new(ui.scope_id().with("local-history-label")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(380.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    egui::RichText::new(t!("ide-local-history-label-title")).font(theme::semibold(15.0)).color(p.text),
                );
                ui.add_space(14.0);
                ui.label(
                    egui::RichText::new(t!("ide-local-history-label-name"))
                        .font(theme::regular(12.5))
                        .color(p.text_secondary),
                );
                ui.add_space(6.0);
                let field = egui::Frame::new()
                    .fill(p.text_field)
                    .stroke(Stroke::new(1.5, if self.error.is_some() { p.excluded } else { p.accent }))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        egui::TextEdit::singleline(&mut self.text)
                            .id_salt("local-history-label-text")
                            .frame(egui::Frame::NONE)
                            .font(theme::regular(14.0))
                            .desired_width(f32::INFINITY)
                            .show(ui)
                    })
                    .inner;
                if !self.focused {
                    field.response.request_focus();
                    self.focused = true;
                }
                if field.response.changed() {
                    self.error = None;
                }
                if field.response.lost_focus()
                    && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                    && valid
                {
                    answer = Some(Some(self.text.trim().to_owned()));
                }
                if let Some(error) = &self.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(error).font(theme::regular(12.5)).color(p.excluded));
                }
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let ok = egui::Button::new(
                        egui::RichText::new(t!("ide-local-history-label-ok"))
                            .font(theme::regular(13.0))
                            .color(p.accent_text),
                    )
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(if valid { p.accent } else { p.accent.gamma_multiply(0.4) });
                    if ui.add_enabled(valid, ok).clicked() {
                        answer = Some(Some(self.text.trim().to_owned()));
                    }
                    let cancel = egui::Button::new(
                        egui::RichText::new(t!("ide-local-history-label-cancel"))
                            .font(theme::regular(13.0))
                            .color(p.text),
                    )
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, p.button_border));
                    if ui.add(cancel).clicked() {
                        answer = Some(None);
                    }
                });
            });
        if answer.is_none() && modal.should_close() {
            answer = Some(None);
        }
        answer
    }
}

pub(super) fn when(time: u64, offset: i64) -> String {
    let now = i64::try_from(zen_workbench::local_history::now() / 1000).unwrap_or(i64::MAX);
    let local = i64::try_from(time / 1000).unwrap_or(i64::MAX).saturating_add(offset);
    let (day, today) = (local.div_euclid(86_400), now.saturating_add(offset).div_euclid(86_400));
    let minutes = local.rem_euclid(86_400) / 60;
    let clock = format!("{:02}:{:02}", minutes / 60, minutes % 60);
    if day == today {
        return t!("ide-local-history-today", time = clock.as_str());
    }
    if day == today - 1 {
        return t!("ide-local-history-yesterday", time = clock.as_str());
    }
    let (year, month, date) = civil(day);
    format!("{date:02}.{month:02}.{year:04} {clock}")
}

fn civil(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    (year_of_era + era * 400 + i64::from(month <= 2), month, day)
}

fn run(registry: &Registry, session: &mut Session, command: &str, params: Value) -> Result<Value, Option<String>> {
    registry.dispatch(session, command, params).map_err(|error| match error {
        DispatchError::Failed { error, .. } => Some(error.message),
        DispatchError::Unavailable { .. } => None,
        other => Some(other.to_string()),
    })
}

impl IdeView {
    pub(super) fn history_context(&self, session: &Session) -> Option<PathBuf> {
        let workbench = session.state::<Workbench>()?;
        if !self.diff_in_front(session)
            && !self.git_in_front(session)
            && let Some(document) = workbench.active_document()
        {
            return Some(document.path().to_owned());
        }
        self.selection.items(&self.visible_rows).into_iter().next().or_else(|| workbench.project().map(Path::to_owned))
    }

    pub(super) fn local_history_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if !self.local_history.is_open() {
            return;
        }
        let palette = self.palette;
        let product = Arc::clone(&self.product);
        let plugins = Arc::clone(&self.plugins);
        let highlighter = move |name: &str| plugins.highlighter(&product.highlighters, name);
        for outcome in self.local_history.update(ui, registry, session, &palette, &highlighter) {
            match outcome {
                HistoryOutcome::Notice(message) => self.post(ui, message),
                HistoryOutcome::Closed => self.editor_view.focus(),
            }
        }
    }
}

#[cfg(test)]
mod tests;

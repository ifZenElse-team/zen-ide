use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use egui::text::CCursorRange;
use egui::{Color32, Key, Rect, Stroke, Ui, vec2};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_workbench::language::refactoring_dialog::{ColumnKind, FieldKind, RefactoringField};

use super::refactoring_preview::{PreviewChange, PreviewFile, PreviewOperation, PreviewTab, RenamePreview};
use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

const POLL: Duration = Duration::from_millis(50);

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct FormSpec {
    pub pending: bool,
    pub title: String,
    pub message: Option<String>,
    pub fields: Vec<RefactoringField>,
    pub immediate: bool,
    pub conflicts_title: String,
    pub anyway_label: String,
    pub view_label: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ConflictPlace {
    pub path: PathBuf,
    pub start: usize,
    pub line: u32,
    pub preview: Option<String>,
    pub highlight: Option<[usize; 2]>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Conflict {
    pub message: String,
    pub place: Option<ConflictPlace>,
    pub relative: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Plan {
    pub pending: bool,
    pub files: Vec<PreviewFile>,
    pub operations: Vec<PreviewOperation>,
    pub usage_count: usize,
    pub conflicts: Vec<Conflict>,
    pub conflicts_message: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Then {
    Refactor,
    Preview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Refactor,
    Preview,
    Cancel,
    Anyway,
    View,
}

#[derive(Debug)]
pub(super) enum RefactoringOutcome {
    Notice(String),
    Preview(Box<PreviewTab>),
    Closed,
}

#[derive(Debug, Default)]
pub(super) struct RefactoringDialog {
    pub(super) state: Option<State>,
}

#[derive(Debug)]
pub(super) enum State {
    Loading { provider: String, context: Value },
    Open(Form),
    Conflicts(Form, Plan),
}

#[derive(Debug)]
pub(super) struct Form {
    pub provider: String,
    pub context: Value,
    pub spec: FormSpec,
    pub values: Map<String, Value>,
    pub shown: bool,
    pub waiting: Option<Then>,
    pub error: Option<String>,
    pub focused: bool,
    pub buttons: BTreeMap<&'static str, Rect>,
    pub checks: BTreeMap<(String, usize, String), Rect>,
    pub selected: BTreeMap<String, usize>,
}

impl Form {
    fn params(&self) -> Value {
        json!({ "provider": self.provider, "context": self.context, "values": self.values })
    }

    fn command(&self, force: bool) -> (String, Value) {
        let mut params = self.params();
        params["force"] = json!(force);
        ("refactor.dialog.apply".to_owned(), params)
    }

    fn cancel(&self) -> (String, Value) {
        ("refactor.dialog.cancel".to_owned(), json!({ "provider": self.provider }))
    }

    fn valid(&self) -> bool {
        self.spec.fields.iter().all(|field| {
            field.kind != FieldKind::Text
                || self.values.get(&field.id).and_then(Value::as_str).is_some_and(|text| !text.trim().is_empty())
        })
    }

    fn table(&mut self, ui: &mut Ui, p: &IdePalette, field: &RefactoringField) {
        ui.label(egui::RichText::new(&field.label).font(theme::regular(12.5)).color(p.text_secondary));
        ui.add_space(6.0);
        let value = self.values.entry(field.id.clone()).or_insert_with(|| field.value.clone());
        let mut rows: Vec<Value> = value.as_array().cloned().unwrap_or_default();
        let mut selected = self.selected.get(&field.id).copied().filter(|row| *row < rows.len());
        let mut changed = false;
        let columns = field.columns.len().max(1);
        egui::Frame::new().stroke(Stroke::new(1.0, p.button_border)).corner_radius(6).inner_margin(6).show(ui, |ui| {
            ui.set_width(ui.available_width());
            let width = (ui.available_width() - 8.0 * (columns as f32 - 1.0)) / columns as f32;
            let mut headers = Vec::new();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                for column in &field.columns {
                    let header = ui.add_sized(
                        [width, 18.0],
                        egui::Label::new(
                            egui::RichText::new(&column.label).font(theme::semibold(12.0)).color(p.text_secondary),
                        ),
                    );
                    headers.push(header.id);
                }
            });
            egui::ScrollArea::vertical().id_salt(("refactoring-table", &field.id)).max_height(150.0).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for (index, row) in rows.iter_mut().enumerate() {
                    let fill = if selected == Some(index) { p.selection } else { Color32::TRANSPARENT };
                    egui::Frame::new().fill(fill).corner_radius(4).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            for (column, header) in field.columns.iter().zip(&headers) {
                                match column.kind {
                                    ColumnKind::Checkbox => {
                                        let cell = ui.allocate_ui(vec2(width, 20.0), |ui| {
                                            ui.set_width(width);
                                            let mut on = row.get(&column.id).and_then(Value::as_bool)?;
                                            let check = ui.add(egui::Checkbox::without_text(&mut on));
                                            crate::accessibility::edit(&check, |node| {
                                                node.push_labelled_by(header.accesskit_id());
                                            });
                                            Some((check, on))
                                        });
                                        if let Some((check, on)) = cell.inner {
                                            self.checks
                                                .insert((field.id.clone(), index, column.id.clone()), check.rect);
                                            if check.clicked() {
                                                selected = Some(index);
                                            }
                                            if check.changed() {
                                                row[column.id.as_str()] = json!(on);
                                                changed = true;
                                            }
                                        }
                                    }
                                    ColumnKind::Label => {
                                        let text = row.get(&column.id).and_then(Value::as_str).unwrap_or_default();
                                        let label = ui.add_sized(
                                            [width, 20.0],
                                            egui::Label::new(
                                                egui::RichText::new(text).font(theme::regular(13.0)).color(p.text),
                                            )
                                            .truncate()
                                            .sense(egui::Sense::click()),
                                        );
                                        if label.clicked() {
                                            selected = Some(index);
                                        }
                                    }
                                    ColumnKind::Text => {
                                        let mut text =
                                            row.get(&column.id).and_then(Value::as_str).unwrap_or_default().to_owned();
                                        let edit = ui.add(
                                            egui::TextEdit::singleline(&mut text)
                                                .id_salt(("refactoring-cell", &field.id, index, &column.id))
                                                .font(theme::regular(13.0))
                                                .desired_width(width),
                                        );
                                        crate::accessibility::edit(&edit, |node| {
                                            node.push_labelled_by(header.accesskit_id());
                                        });
                                        if edit.has_focus() || edit.clicked() {
                                            selected = Some(index);
                                        }
                                        if edit.changed() {
                                            row[column.id.as_str()] = json!(text);
                                            changed = true;
                                        }
                                    }
                                }
                            }
                        });
                    });
                }
            });
        });
        if field.fixed {
            self.store_table(field, rows, selected, changed);
            return;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let small = |text: String| {
                egui::Button::new(egui::RichText::new(text).font(theme::regular(12.5)).color(p.text))
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, p.button_border))
                    .corner_radius(4)
                    .min_size(vec2(0.0, 24.0))
            };
            let add = ui.add(small(t!("ide-refactoring-table-add")));
            self.buttons.insert("tableAdd", add.rect);
            if add.clicked() {
                let new_row = if field.new_row.is_object() { field.new_row.clone() } else { json!({}) };
                let at = selected.map_or(rows.len(), |row| row + 1);
                rows.insert(at, new_row);
                selected = Some(at);
                changed = true;
            }
            let remove = ui.add_enabled(selected.is_some(), small(t!("ide-refactoring-table-remove")));
            self.buttons.insert("tableRemove", remove.rect);
            if remove.clicked()
                && let Some(row) = selected
            {
                rows.remove(row);
                selected = (!rows.is_empty()).then(|| row.min(rows.len() - 1));
                changed = true;
            }
            let up = selected.is_some_and(|row| row > 0);
            let moved_up = ui.add_enabled(up, small(t!("ide-refactoring-table-up")));
            self.buttons.insert("tableUp", moved_up.rect);
            if moved_up.clicked()
                && let Some(row) = selected
            {
                rows.swap(row, row - 1);
                selected = Some(row - 1);
                changed = true;
            }
            let down = selected.is_some_and(|row| row + 1 < rows.len());
            let moved_down = ui.add_enabled(down, small(t!("ide-refactoring-table-down")));
            self.buttons.insert("tableDown", moved_down.rect);
            if moved_down.clicked()
                && let Some(row) = selected
            {
                rows.swap(row, row + 1);
                selected = Some(row + 1);
                changed = true;
            }
        });
        self.store_table(field, rows, selected, changed);
    }

    fn store_table(&mut self, field: &RefactoringField, rows: Vec<Value>, selected: Option<usize>, changed: bool) {
        match selected {
            Some(row) => self.selected.insert(field.id.clone(), row),
            None => self.selected.remove(&field.id),
        };
        if changed {
            self.values.insert(field.id.clone(), Value::Array(rows));
            self.error = None;
        }
    }
}

impl RefactoringDialog {
    pub(super) fn open(&mut self, provider: String, context: Value) {
        self.state = Some(State::Loading { provider, context });
    }

    pub(super) fn is_open(&self) -> bool {
        self.state.is_some()
    }

    pub(super) fn kind(&self) -> Option<&'static str> {
        match self.state.as_ref()? {
            State::Loading { .. } => Some("refactoringLoading"),
            State::Open(form) if form.shown => Some("refactoringDialog"),
            State::Open(_) => Some("refactoringLoading"),
            State::Conflicts(..) => Some("refactoringConflicts"),
        }
    }

    pub(super) fn update(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
    ) -> Vec<RefactoringOutcome> {
        let mut outcomes = Vec::new();
        match self.state.take() {
            None => {}
            Some(State::Loading { provider, context }) => {
                let params = json!({ "provider": provider, "context": context });
                match run(registry, session, "refactor.dialog.form", params) {
                    Err(message) => {
                        outcomes.extend(message.map(RefactoringOutcome::Notice));
                        outcomes.push(RefactoringOutcome::Closed);
                    }
                    Ok(value) => {
                        let spec: FormSpec = serde_json::from_value(value).unwrap_or_default();
                        if let Some(error) = spec.error {
                            outcomes.push(RefactoringOutcome::Notice(error));
                            outcomes.push(RefactoringOutcome::Closed);
                        } else if spec.pending {
                            ui.ctx().request_repaint_after(POLL);
                            self.state = Some(State::Loading { provider, context });
                        } else {
                            let values =
                                spec.fields.iter().map(|field| (field.id.clone(), field.value.clone())).collect();
                            let shown = !(spec.immediate && spec.fields.is_empty());
                            let waiting = (!shown).then_some(Then::Refactor);
                            let form = Form {
                                provider,
                                context,
                                spec,
                                values,
                                shown,
                                waiting,
                                error: None,
                                focused: false,
                                buttons: BTreeMap::new(),
                                checks: BTreeMap::new(),
                                selected: BTreeMap::new(),
                            };
                            self.state = Some(State::Open(form));
                            ui.ctx().request_repaint();
                        }
                    }
                }
            }
            Some(State::Open(mut form)) => {
                let choice = if form.shown { form.show(ui, p) } else { None };
                match choice {
                    Some(Choice::Cancel) => {
                        let (command, params) = form.cancel();
                        let _ = run(registry, session, &command, params);
                        outcomes.push(RefactoringOutcome::Closed);
                        return outcomes;
                    }
                    Some(Choice::Refactor) => form.waiting = Some(Then::Refactor),
                    Some(Choice::Preview) => form.waiting = Some(Then::Preview),
                    _ => {}
                }
                match form.waiting {
                    None => self.state = Some(State::Open(form)),
                    Some(then) => self.state = form.advance(ui, registry, session, then, &mut outcomes),
                }
            }
            Some(State::Conflicts(mut form, plan)) => match conflicts(ui, p, &mut form, &plan) {
                None => self.state = Some(State::Conflicts(form, plan)),
                Some(Choice::Anyway) => {
                    let (command, params) = form.command(true);
                    if let Err(Some(message)) = run(registry, session, &command, params) {
                        outcomes.push(RefactoringOutcome::Notice(message));
                    }
                    outcomes.push(RefactoringOutcome::Closed);
                }
                Some(Choice::View) => {
                    outcomes.push(RefactoringOutcome::Preview(Box::new(conflicts_tab(&form, &plan))));
                }
                Some(_) => {
                    let (command, params) = form.cancel();
                    let _ = run(registry, session, &command, params);
                    outcomes.push(RefactoringOutcome::Closed);
                }
            },
        }
        outcomes
    }
}

impl Form {
    fn advance(
        mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        then: Then,
        outcomes: &mut Vec<RefactoringOutcome>,
    ) -> Option<State> {
        let plan = match run(registry, session, "refactor.dialog.preview", self.params()) {
            Ok(value) => serde_json::from_value::<Plan>(value).unwrap_or_default(),
            Err(message) => Plan { error: Some(message.unwrap_or_default()), ..Plan::default() },
        };
        if plan.pending {
            ui.ctx().request_repaint_after(POLL);
            return Some(State::Open(self));
        }
        if let Some(error) = plan.error {
            self.waiting = None;
            if !self.shown {
                outcomes.push(RefactoringOutcome::Notice(error));
                outcomes.push(RefactoringOutcome::Closed);
                return None;
            }
            self.error = Some(error);
            self.focused = false;
            ui.ctx().request_repaint();
            return Some(State::Open(self));
        }
        if then == Then::Preview {
            outcomes.push(RefactoringOutcome::Preview(Box::new(preview_tab(&self, plan))));
            return None;
        }
        if !plan.conflicts.is_empty() {
            self.waiting = None;
            ui.ctx().request_repaint();
            return Some(State::Conflicts(self, plan));
        }
        let (command, params) = self.command(false);
        if let Err(Some(message)) = run(registry, session, &command, params) {
            outcomes.push(RefactoringOutcome::Notice(message));
        }
        outcomes.push(RefactoringOutcome::Closed);
        None
    }

    fn show(&mut self, ui: &Ui, p: &IdePalette) -> Option<Choice> {
        let mut choice = None;
        let busy = self.waiting.is_some();
        let valid = self.valid() && !busy;
        let modal =
            egui::Modal::new(ui.scope_id().with("refactoring-dialog")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(440.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(&self.spec.title).font(theme::semibold(15.0)).color(p.text));
                if let Some(message) = &self.spec.message {
                    ui.add_space(12.0);
                    ui.add(
                        egui::Label::new(egui::RichText::new(message).font(theme::regular(13.0)).color(p.text)).wrap(),
                    );
                }
                let fields = self.spec.fields.clone();
                for field in &fields {
                    ui.add_space(12.0);
                    if self.field(ui, p, field) && valid {
                        choice = Some(Choice::Refactor);
                    }
                }
                if let Some(error) = &self.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(error).font(theme::regular(12.5)).color(p.excluded));
                }
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if self.button(ui, p, "refactor", t!("ide-rename-refactor"), true, valid) {
                        choice = Some(Choice::Refactor);
                    }
                    if self.button(ui, p, "cancel", t!("ide-rename-cancel"), false, true) {
                        choice = Some(Choice::Cancel);
                    }
                    if self.button(ui, p, "preview", t!("ide-rename-preview"), false, valid) {
                        choice = Some(Choice::Preview);
                    }
                });
            });
        if choice.is_none() && modal.should_close() {
            choice = Some(Choice::Cancel);
        }
        choice
    }

    fn field(&mut self, ui: &mut Ui, p: &IdePalette, field: &RefactoringField) -> bool {
        let value = self.values.entry(field.id.clone()).or_insert_with(|| field.value.clone());
        match field.kind {
            FieldKind::Checkbox => {
                let mut on = value.as_bool().unwrap_or(false);
                let label = egui::RichText::new(&field.label).font(theme::regular(13.0)).color(p.text);
                if ui.add(egui::Checkbox::new(&mut on, label)).changed() {
                    *value = json!(on);
                    self.error = None;
                }
                false
            }
            FieldKind::Table => {
                self.table(ui, p, field);
                false
            }
            FieldKind::Choice => {
                let label =
                    ui.label(egui::RichText::new(&field.label).font(theme::regular(12.5)).color(p.text_secondary));
                ui.add_space(6.0);
                let current = value.as_str().unwrap_or_default().to_owned();
                let shown = field
                    .options
                    .iter()
                    .find(|option| option.value == current)
                    .map_or(current.clone(), |option| option.label.clone());
                let mut picked = None;
                let combo = egui::ComboBox::from_id_salt(("refactoring-choice", &field.id))
                    .selected_text(shown)
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for option in &field.options {
                            if ui.selectable_label(option.value == current, &option.label).clicked() {
                                picked = Some(option.value.clone());
                            }
                        }
                    });
                crate::accessibility::edit(&combo.response, |node| node.push_labelled_by(label.id.accesskit_id()));
                if let Some(picked) = picked {
                    *value = json!(picked);
                    self.error = None;
                }
                false
            }
            FieldKind::Text => {
                let label =
                    ui.label(egui::RichText::new(&field.label).font(theme::regular(12.5)).color(p.text_secondary));
                ui.add_space(6.0);
                let mut text = value.as_str().unwrap_or_default().to_owned();
                let border = if self.error.is_some() { p.excluded } else { p.accent };
                let edit = egui::Frame::new()
                    .fill(p.text_field)
                    .stroke(Stroke::new(1.5, border))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        egui::TextEdit::singleline(&mut text)
                            .id_salt(("refactoring-field", &field.id))
                            .frame(egui::Frame::NONE)
                            .font(theme::regular(14.0))
                            .desired_width(f32::INFINITY)
                            .show(ui)
                    })
                    .inner;
                crate::accessibility::edit(&edit.response, |node| node.push_labelled_by(label.id.accesskit_id()));
                if !self.focused {
                    edit.response.request_focus();
                    let mut state = edit.state.clone();
                    state.cursor.set_char_range(Some(CCursorRange::select_all_str(&text)));
                    state.store(ui.ctx(), edit.response.id);
                    self.focused = true;
                }
                if edit.response.changed() {
                    *value = json!(text);
                    self.error = None;
                }
                edit.response.lost_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter))
            }
        }
    }

    fn button(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        key: &'static str,
        text: String,
        primary: bool,
        enabled: bool,
    ) -> bool {
        let response = ui.add_enabled(enabled, styled(p, text, primary, enabled));
        self.buttons.insert(key, response.rect);
        response.clicked()
    }
}

fn styled(p: &IdePalette, text: String, primary: bool, enabled: bool) -> egui::Button<'static> {
    let color = if primary { p.accent_text } else { p.text };
    let button = egui::Button::new(egui::RichText::new(text).font(theme::regular(13.0)).color(color))
        .min_size(vec2(84.0, 30.0))
        .corner_radius(6);
    if primary {
        button.fill(if enabled { p.accent } else { p.accent.gamma_multiply(0.4) })
    } else {
        button.fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, p.button_border))
    }
}

fn conflicts(ui: &Ui, p: &IdePalette, form: &mut Form, plan: &Plan) -> Option<Choice> {
    let mut choice = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("refactoring-conflicts")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(480.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(&form.spec.conflicts_title).font(theme::semibold(15.0)).color(p.text));
            if let Some(message) = &plan.conflicts_message {
                ui.add_space(12.0);
                ui.add(egui::Label::new(egui::RichText::new(message).font(theme::regular(13.0)).color(p.text)).wrap());
            }
            ui.add_space(10.0);
            egui::Frame::new().stroke(Stroke::new(1.0, p.button_border)).corner_radius(6).inner_margin(8).show(
                ui,
                |ui| {
                    egui::ScrollArea::vertical().id_salt("refactoring-conflicts-list").max_height(180.0).show(
                        ui,
                        |ui| {
                            ui.set_width(ui.available_width());
                            ui.spacing_mut().item_spacing.y = 4.0;
                            for conflict in &plan.conflicts {
                                ui.label(
                                    egui::RichText::new(&conflict.message).font(theme::regular(12.5)).color(p.text),
                                );
                                if let Some(preview) =
                                    conflict.place.as_ref().and_then(|place| place.preview.as_deref())
                                {
                                    ui.label(
                                        egui::RichText::new(preview)
                                            .font(egui::FontId::monospace(12.0))
                                            .color(p.text_secondary),
                                    );
                                }
                            }
                        },
                    );
                },
            );
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if !form.spec.view_label.is_empty()
                    && form.button(ui, p, "view", form.spec.view_label.clone(), false, true)
                {
                    choice = Some(Choice::View);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let anyway = if form.spec.anyway_label.is_empty() {
                        t!("ide-refactoring-continue")
                    } else {
                        form.spec.anyway_label.clone()
                    };
                    if form.button(ui, p, "anyway", anyway, true, true) {
                        choice = Some(Choice::Anyway);
                    }
                    if form.button(ui, p, "cancel", t!("ide-rename-cancel"), false, true) {
                        choice = Some(Choice::Cancel);
                    }
                });
            });
        });
    if choice.is_none() && modal.should_close() {
        choice = Some(Choice::Cancel);
    }
    choice
}

fn preview_tab(form: &Form, plan: Plan) -> PreviewTab {
    let preview = RenamePreview {
        pending: false,
        files: plan.files,
        operations: plan.operations,
        usage_count: plan.usage_count,
        error: None,
    };
    let mut tab = PreviewTab::with_commands(preview, form.command(true), form.cancel());
    tab.heading = Some(form.spec.title.clone());
    tab
}

fn conflicts_tab(form: &Form, plan: &Plan) -> PreviewTab {
    let mut files: Vec<PreviewFile> = Vec::new();
    for conflict in &plan.conflicts {
        let Some(place) = &conflict.place else { continue };
        let change = PreviewChange {
            path: place.path.clone(),
            start: place.start,
            line: place.line,
            preview: place.preview.clone(),
            highlight: place.highlight,
            new_text: String::new(),
        };
        match files.iter_mut().find(|file| file.path == place.path) {
            Some(file) => file.changes.push(change),
            None => files.push(PreviewFile {
                path: place.path.clone(),
                relative: conflict.relative.clone().unwrap_or_else(|| place.path.display().to_string()),
                changes: vec![change],
            }),
        }
    }
    let count = files.iter().map(|file| file.changes.len()).sum();
    let preview = RenamePreview { pending: false, files, operations: Vec::new(), usage_count: count, error: None };
    let mut tab = PreviewTab::with_commands(preview, form.command(true), form.cancel());
    tab.heading = Some(form.spec.conflicts_title.clone());
    tab.conflicts = true;
    tab
}

fn run(registry: &Registry, session: &mut Session, command: &str, params: Value) -> Result<Value, Option<String>> {
    registry.dispatch(session, command, params).map_err(|error| match error {
        DispatchError::Failed { error, .. } => Some(error.message),
        DispatchError::Unavailable { .. } => None,
        other => Some(other.to_string()),
    })
}

impl IdeView {
    pub(super) fn open_refactoring(&mut self, provider: String, context: Value) {
        if !self.refactoring.is_open() {
            self.refactoring.open(provider, context);
            if self.find_window.close_refactoring() && self.tool_visible("find") {
                self.hide_tool("find");
            }
        }
    }

    pub(super) fn refactoring_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let palette = self.palette;
        for outcome in self.refactoring.update(ui, registry, session, &palette) {
            match outcome {
                RefactoringOutcome::Notice(message) => self.post(ui, message),
                RefactoringOutcome::Preview(tab) => {
                    self.find_window.show_refactoring(*tab);
                    self.show_tool("find");
                }
                RefactoringOutcome::Closed => self.editor_view.focus(),
            }
        }
    }
}

#[cfg(test)]
mod tests;

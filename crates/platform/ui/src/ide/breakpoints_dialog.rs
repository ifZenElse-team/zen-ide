use std::path::{Path, PathBuf};

use egui::{Key, Pos2, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;
use zen_workbench::debug::{
    AfterHit, BreakpointInfo, DataBreakpointInfo, ExceptionBreakpointInfo, FunctionBreakpointInfo, Suspend,
};

use super::vcs::{primary_button, secondary_button};
use super::{IdePalette, dialog_frame};
use crate::{accessibility, theme};

pub(super) const VIEW: &str = "view.breakpoints.open";
pub(super) const PROPERTIES: &str = "view.breakpoint.properties";
pub(super) const ADD_CONDITIONAL: &str = "view.breakpoint.addConditional";
pub(super) const ADD_LOGGING: &str = "view.breakpoint.addLogging";

const PREVIEW_LINES: u32 = 4;
const SUGGESTIONS: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Support {
    conditions: bool,
    hit_conditions: bool,
    thread_suspend: bool,
    functions: bool,
    data: bool,
    exception_conditions: bool,
}

impl Support {
    fn read(value: &Value) -> Self {
        let on = |key: &str| value.is_null() || value[key] == true;
        Self {
            conditions: on("conditions"),
            hit_conditions: on("hitConditions"),
            thread_suspend: !value.is_null() && value["threadSuspend"] == true,
            functions: on("functions"),
            data: !value.is_null() && value["data"] == true,
            exception_conditions: on("exceptionConditions"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Offered {
    filter: String,
    label: String,
    supports_condition: bool,
}

#[derive(Clone, Debug)]
struct Listed {
    lines: Vec<BreakpointInfo>,
    functions: Vec<FunctionBreakpointInfo>,
    exceptions: Vec<ExceptionBreakpointInfo>,
    offered: Vec<Offered>,
    data: Vec<DataBreakpointInfo>,
    muted: bool,
    support: Support,
}

impl Listed {
    fn read(registry: &Registry, session: &mut Session) -> Self {
        let answer = registry.dispatch(session, "debug.breakpoint.list", json!({})).unwrap_or_default();
        let read = |key: &str| answer.get(key).cloned().unwrap_or(Value::Array(Vec::new()));
        let offered = answer["offered"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|filter| {
                Some(Offered {
                    filter: filter["filter"].as_str()?.to_owned(),
                    label: filter["label"].as_str().unwrap_or_default().to_owned(),
                    supports_condition: filter["supportsCondition"] == true,
                })
            })
            .collect();
        Self {
            lines: serde_json::from_value(read("breakpoints")).unwrap_or_default(),
            functions: serde_json::from_value(read("functions")).unwrap_or_default(),
            exceptions: serde_json::from_value(read("exceptions")).unwrap_or_default(),
            offered,
            data: serde_json::from_value(read("data")).unwrap_or_default(),
            muted: answer["muted"] == true,
            support: Support::read(answer.get("support").unwrap_or(&Value::Null)),
        }
    }

    fn line(&self, path: &Path, line: u32) -> Option<&BreakpointInfo> {
        self.lines.iter().find(|info| info.path == path && info.line == line)
    }

    fn groups(&self) -> Vec<String> {
        let mut groups: Vec<String> = self.lines.iter().filter_map(|info| info.group.clone()).collect();
        groups.sort();
        groups.dedup();
        groups
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Selected {
    Line(PathBuf, u32),
    Function(String),
    Exception(String),
    Data(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Condition,
    Expression,
    Prompt,
    Evaluate,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Suggest {
    field: Option<Field>,
    answer: Value,
    selected: usize,
    ask: Option<(Field, String)>,
}

impl Suggest {
    pub(super) fn items(&self, field: Field) -> &[Value] {
        if self.field != Some(field) {
            return &[];
        }
        self.answer["items"].as_array().map_or(&[], |items| &items[..items.len().min(SUGGESTIONS)])
    }

    pub(super) fn run(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, (path, line): (&Path, u32)) {
        let ask = self.ask.take().or_else(|| {
            let field = self.field?;
            (self.answer["pending"] == true)
                .then(|| (field, self.answer["text"].as_str().unwrap_or_default().to_owned()))
        });
        let Some((field, text)) = ask else { return };
        let params = json!({ "path": path, "line": line, "text": text });
        let mut answer = registry.dispatch(session, "debug.expression.complete", params).unwrap_or_default();
        answer["text"] = json!(text);
        if answer["pending"] == true {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        self.field = Some(field);
        self.answer = answer;
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Texts {
    of: Option<Selected>,
    condition: String,
    expression: String,
    evaluate: bool,
    hit: String,
    suggest: Suggest,
}

impl Texts {
    fn of_line(info: &BreakpointInfo) -> Self {
        Self {
            of: Some(Selected::Line(info.path.clone(), info.line)),
            condition: info.condition.clone().unwrap_or_default(),
            expression: info.log_expression.clone().unwrap_or_default(),
            evaluate: info.log_expression.is_some(),
            hit: info.hit_condition.clone().unwrap_or_default(),
            suggest: Suggest::default(),
        }
    }
}

pub(super) fn place_name(path: &Path, line: u32) -> String {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into_owned());
    format!("{name}:{}", line + 1)
}

fn line_properties(
    ui: &mut Ui,
    p: &IdePalette,
    info: &BreakpointInfo,
    texts: &mut Texts,
    (support, full): (Support, bool),
    others: &[BreakpointInfo],
    focus: &mut Option<Field>,
) -> serde_json::Map<String, Value> {
    let mut change = serde_json::Map::new();
    let mut enabled = info.enabled;
    if ui.checkbox(&mut enabled, t!("ide-breakpoint-enabled")).changed() {
        change.insert("enabled".to_owned(), json!(enabled));
    }
    ui.add_space(6.0);
    let mut suspend = info.suspend != Suspend::None;
    ui.horizontal(|ui| {
        if ui.checkbox(&mut suspend, t!("ide-breakpoint-suspend")).changed() {
            let to = if suspend { Suspend::All } else { Suspend::None };
            change.insert("suspend".to_owned(), json!(to));
            if !suspend && !info.log_hit && !info.log_stack && info.log_expression.is_none() {
                change.insert("logHit".to_owned(), json!(true));
            }
        }
        if suspend && support.thread_suspend {
            let mut policy = if info.suspend == Suspend::Thread { Suspend::Thread } else { Suspend::All };
            let before = policy;
            ui.radio_value(&mut policy, Suspend::All, t!("ide-breakpoint-suspend-all"));
            ui.radio_value(&mut policy, Suspend::Thread, t!("ide-breakpoint-suspend-thread"));
            if policy != before {
                change.insert("suspend".to_owned(), json!(policy));
            }
        }
    });
    if support.conditions {
        ui.add_space(8.0);
        let label = ui.label(egui::RichText::new(t!("ide-breakpoint-condition")).color(p.text_secondary));
        let field = suggested_field(
            ui,
            p,
            (&mut texts.condition, &mut texts.suggest),
            (label.id, Field::Condition),
            focus.take_if(|field| *field == Field::Condition),
        );
        if field.changed() {
            change.insert("condition".to_owned(), json!(texts.condition));
        }
    }
    if full || !suspend || info.log_hit || info.log_stack || info.log_expression.is_some() {
        ui.add_space(10.0);
        ui.label(egui::RichText::new(t!("ide-breakpoint-log")).color(p.text_secondary));
        let mut hit = info.log_hit;
        if ui.checkbox(&mut hit, t!("ide-breakpoint-log-hit")).changed() {
            change.insert("logHit".to_owned(), json!(hit));
        }
        let mut stack = info.log_stack;
        if ui.checkbox(&mut stack, t!("ide-breakpoint-log-stack")).changed() {
            change.insert("logStack".to_owned(), json!(stack));
        }
        let evaluate = ui.checkbox(&mut texts.evaluate, t!("ide-breakpoint-log-evaluate"));
        if evaluate.changed() {
            if texts.evaluate {
                *focus = Some(Field::Expression);
            } else {
                change.insert("logExpression".to_owned(), json!(""));
            }
        }
        if texts.evaluate {
            let field = suggested_field(
                ui,
                p,
                (&mut texts.expression, &mut texts.suggest),
                (evaluate.id, Field::Expression),
                focus.take_if(|field| *field == Field::Expression),
            );
            if field.changed() {
                change.insert("logExpression".to_owned(), json!(texts.expression));
            }
        }
        ui.add_space(6.0);
        let mut remove = info.remove_once_hit;
        if ui.checkbox(&mut remove, t!("ide-breakpoint-remove-once-hit")).changed() {
            change.insert("removeOnceHit".to_owned(), json!(remove));
        }
    }
    if full {
        ui.add_space(10.0);
        let label = ui.label(egui::RichText::new(t!("ide-breakpoint-depends-on")).color(p.text_secondary));
        let current = info.depends_on.as_ref().map(|master| (master.path.clone(), master.line));
        let shown =
            current.as_ref().map_or_else(|| t!("ide-breakpoint-depends-none"), |(path, line)| place_name(path, *line));
        let mut chosen = current.clone();
        let combo = egui::ComboBox::from_id_salt(("breakpoint-depends", &info.path, info.line))
            .selected_text(shown)
            .width(260.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut chosen, None, t!("ide-breakpoint-depends-none"));
                for other in others.iter().filter(|other| !(other.path == info.path && other.line == info.line)) {
                    let place = Some((other.path.clone(), other.line));
                    ui.selectable_value(&mut chosen, place, place_name(&other.path, other.line));
                }
            });
        accessibility::name(&combo.response, egui::Role::ComboBox, &t!("ide-breakpoint-depends-on"));
        combo.response.labelled_by(label.id);
        if chosen != current {
            let master = chosen.map(|(path, line)| json!({ "path": path, "line": line }));
            change.insert("dependsOn".to_owned(), master.unwrap_or(Value::Null));
        }
        if info.depends_on.is_some() {
            ui.horizontal(|ui| {
                ui.label(t!("ide-breakpoint-after-hit"));
                let mut after = info.after_hit;
                ui.radio_value(&mut after, AfterHit::DisableAgain, t!("ide-breakpoint-disable-again"));
                ui.radio_value(&mut after, AfterHit::LeaveEnabled, t!("ide-breakpoint-leave-enabled"));
                if after != info.after_hit {
                    change.insert("afterHit".to_owned(), json!(after));
                }
            });
        }
        if support.hit_conditions {
            ui.add_space(8.0);
            let label = ui.label(egui::RichText::new(t!("ide-breakpoint-pass-count")).color(p.text_secondary));
            if code_field(ui, p, &mut texts.hit, label.id, None).changed() {
                change.insert("hitCondition".to_owned(), json!(texts.hit));
            }
        }
    }
    change
}

pub(super) fn suggested_field(
    ui: &mut Ui,
    p: &IdePalette,
    (text, suggest): (&mut String, &mut Suggest),
    (label, field): (egui::Id, Field),
    focus: Option<Field>,
) -> egui::Response {
    let shown = suggest.items(field).len();
    let mut accept = None;
    if shown > 0 {
        ui.input_mut(|input| {
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                suggest.selected = (suggest.selected + 1) % shown;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                suggest.selected = (suggest.selected + shown - 1) % shown;
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Enter)
                || input.consume_key(egui::Modifiers::NONE, Key::Tab)
            {
                accept = Some(suggest.selected.min(shown - 1));
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Escape) {
                suggest.field = None;
            }
        });
    }
    let mut response = code_field(ui, p, text, label, focus);
    if response.changed() {
        suggest.ask = Some((field, text.clone()));
        suggest.selected = 0;
    }
    let items = suggest.items(field).to_vec();
    if !items.is_empty() && (response.has_focus() || accept.is_some()) {
        let at = response.rect.left_bottom() + vec2(0.0, 2.0);
        egui::Area::new(response.id.with("suggestions")).order(egui::Order::Tooltip).fixed_pos(at).show(
            ui.ctx(),
            |ui| {
                super::doc_popups::popup_frame(p).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                    ui.set_min_width(response.rect.width().min(360.0));
                    for (index, item) in items.iter().enumerate() {
                        let label = item["label"].as_str().unwrap_or_default();
                        let detail = item["detail"].as_str().unwrap_or_default();
                        let row = ui.selectable_label(
                            index == suggest.selected,
                            egui::RichText::new(format!("{label}  {detail}")).font(egui::FontId::monospace(12.0)),
                        );
                        accessibility::name(&row, egui::Role::ListItem, label);
                        if row.clicked() {
                            accept = Some(index);
                        }
                    }
                });
            },
        );
    }
    if let Some(index) = accept
        && let Some(item) = items.get(index)
    {
        let start = suggest.answer["start"].as_u64().unwrap_or_default() as usize;
        let kept: String = text.chars().take(start).collect();
        *text = format!("{kept}{}", item["insert"].as_str().unwrap_or_default());
        suggest.field = None;
        suggest.answer = Value::Null;
        response.mark_changed();
        response.request_focus();
    }
    response
}

fn code_field(ui: &mut Ui, p: &IdePalette, text: &mut String, label: egui::Id, focus: Option<Field>) -> egui::Response {
    let field = egui::Frame::new()
        .fill(p.text_field)
        .stroke(Stroke::new(1.0, p.text_field_border))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(text)
                    .frame(egui::Frame::NONE)
                    .font(egui::FontId::monospace(12.5))
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label)
        })
        .inner;
    if focus.is_some() {
        field.request_focus();
    }
    field
}

struct Popup {
    path: PathBuf,
    line: u32,
    at: Pos2,
    listed: Listed,
    texts: Texts,
    focus: Option<Field>,
    opened: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum PromptKind {
    #[default]
    Function,
    Group,
    Description,
}

#[derive(Clone, Debug, Default)]
struct Prompt {
    kind: PromptKind,
    text: String,
}

struct View {
    listed: Listed,
    selected: Option<Selected>,
    texts: Texts,
    prompt: Option<Prompt>,
    focus: Option<Field>,
    preview: Option<(PathBuf, u32, Vec<String>)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Navigate { path: PathBuf, line: u32 },
    More { path: PathBuf, line: u32 },
}

#[derive(Default)]
pub(super) struct BreakpointDialogs {
    popup: Option<Popup>,
    view: Option<View>,
}

impl BreakpointDialogs {
    pub(super) fn names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.popup.is_some() {
            names.push("breakpointProperties");
        }
        if self.view.is_some() {
            names.push("breakpoints");
        }
        names
    }

    pub(super) fn is_open(&self) -> bool {
        self.popup.is_some() || self.view.is_some()
    }

    pub(super) fn open_popup(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        (path, line): (PathBuf, u32),
        at: Pos2,
        focus: Option<&str>,
    ) -> bool {
        let listed = Listed::read(registry, session);
        let Some(info) = listed.line(&path, line) else { return false };
        let texts = Texts::of_line(info);
        let focus = match focus {
            Some("condition") => Some(Field::Condition),
            Some("logExpression") => Some(Field::Expression),
            _ => None,
        };
        let mut texts = texts;
        if focus == Some(Field::Expression) {
            texts.evaluate = true;
        }
        self.popup = Some(Popup { path, line, at, listed, texts, focus, opened: true });
        true
    }

    pub(super) fn open_view(&mut self, registry: &Registry, session: &mut Session, select: Option<(PathBuf, u32)>) {
        self.popup = None;
        let listed = Listed::read(registry, session);
        let selected = select
            .filter(|(path, line)| listed.line(path, *line).is_some())
            .map(|(path, line)| Selected::Line(path, line))
            .or_else(|| listed.lines.first().map(|info| Selected::Line(info.path.clone(), info.line)));
        self.view = Some(View { listed, selected, texts: Texts::default(), prompt: None, focus: None, preview: None });
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
    ) -> Option<Outcome> {
        let popup = self.popup.take().and_then(|popup| show_popup(ui, registry, session, p, popup));
        let mut outcome = None;
        match popup {
            Some(Ok(popup)) => self.popup = Some(popup),
            Some(Err(more)) => outcome = Some(more),
            None => {}
        }
        if let Some(view) = self.view.take() {
            match show_view(ui, registry, session, p, view) {
                Ok(view) => self.view = Some(view),
                Err(done) => outcome = outcome.or(done),
            }
        }
        if let Some(Outcome::More { path, line }) = &outcome {
            self.open_view(registry, session, Some((path.clone(), *line)));
        }
        outcome
    }
}

fn apply(registry: &Registry, session: &mut Session, command: &str, params: Value) {
    let _ = registry.dispatch(session, command, params);
}

fn show_popup(
    ui: &mut Ui,
    registry: &Registry,
    session: &mut Session,
    p: &IdePalette,
    mut popup: Popup,
) -> Option<Result<Popup, Outcome>> {
    let info = popup.listed.line(&popup.path, popup.line).cloned()?;
    let mut done = ui.input(|input| input.key_pressed(Key::Escape))
        && popup.texts.suggest.items(Field::Condition).is_empty()
        && popup.texts.suggest.items(Field::Expression).is_empty();
    let mut more = false;
    let mut change = serde_json::Map::new();
    let area = egui::Area::new(ui.scope_id().with("breakpoint-properties"))
        .order(egui::Order::Foreground)
        .fixed_pos(popup.at)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            super::doc_popups::popup_frame(p).inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                ui.set_width(360.0);
                ui.label(
                    egui::RichText::new(place_name(&popup.path, popup.line)).font(theme::semibold(13.0)).color(p.text),
                );
                ui.add_space(8.0);
                change = line_properties(
                    ui,
                    p,
                    &info,
                    &mut popup.texts,
                    (popup.listed.support, false),
                    &popup.listed.lines,
                    &mut popup.focus,
                );
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    let shortcut = crate::keymap::shortcut(VIEW).map(|keys| format!(" ({keys})")).unwrap_or_default();
                    if ui.link(format!("{}{shortcut}", t!("ide-breakpoint-more"))).clicked() {
                        more = true;
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if primary_button(ui, p, &t!("ide-breakpoint-done")).clicked() {
                            done = true;
                        }
                    });
                });
            });
        });
    if !popup.opened && area.response.clicked_elsewhere() {
        done = true;
    }
    popup.opened = false;
    popup.texts.suggest.run(ui, registry, session, (&popup.path, popup.line));
    if !change.is_empty() {
        change.insert("path".to_owned(), json!(popup.path));
        change.insert("line".to_owned(), json!(popup.line));
        apply(registry, session, "debug.breakpoint.set", Value::Object(change));
        popup.listed = Listed::read(registry, session);
    }
    if more {
        return Some(Err(Outcome::More { path: popup.path, line: popup.line }));
    }
    (!done).then_some(Ok(popup))
}

fn row(
    ui: &mut Ui,
    p: &IdePalette,
    selected: bool,
    checked: Option<bool>,
    text: &str,
    detail: &str,
) -> (egui::Response, bool) {
    let height = 24.0;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 4, p.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    let mut toggled = false;
    let mut x = rect.left() + 6.0;
    if let Some(mut on) = checked {
        let check = egui::Rect::from_min_size(pos2(x, rect.top() + 4.0), vec2(16.0, 16.0));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(check));
        if child.checkbox(&mut on, "").changed() {
            toggled = true;
        }
        x += 22.0;
    }
    ui.painter().text(pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, text, theme::regular(13.0), p.text);
    if !detail.is_empty() {
        let width = ui.painter().layout_no_wrap(text.to_owned(), theme::regular(13.0), p.text).size().x;
        ui.painter().text(
            pos2(x + width + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            detail,
            theme::regular(12.5),
            p.text_secondary,
        );
    }
    accessibility::name(&response, egui::Role::TreeItem, text);
    (response, toggled)
}

fn header(ui: &mut Ui, p: &IdePalette, text: &str) {
    ui.add_space(4.0);
    ui.label(egui::RichText::new(text).font(theme::semibold(12.5)).color(p.text_secondary));
}

fn preview_of(session: &Session, path: &Path, line: u32) -> Vec<String> {
    let open = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| document.text().to_owned());
    let text = open.or_else(|| std::fs::read_to_string(path).ok()).unwrap_or_default();
    let first = line.saturating_sub(PREVIEW_LINES) as usize;
    text.lines().skip(first).take((PREVIEW_LINES * 2 + 1) as usize).map(str::to_owned).collect()
}

#[allow(clippy::too_many_lines, reason = "one dialog: the tree, the properties and the preview")]
fn show_view(
    ui: &mut Ui,
    registry: &Registry,
    session: &mut Session,
    p: &IdePalette,
    mut view: View,
) -> Result<View, Option<Outcome>> {
    let mut actions: Vec<(&'static str, Value)> = Vec::new();
    let mut outcome = None;
    let mut done = false;
    let escape = ui.input(|input| input.key_pressed(Key::Escape))
        && view.prompt.is_none()
        && view.texts.suggest.items(Field::Condition).is_empty()
        && view.texts.suggest.items(Field::Expression).is_empty();
    if let Some(Selected::Line(path, line)) = &view.selected
        && view.preview.as_ref().is_none_or(|(shown, at, _)| shown != path || at != line)
    {
        view.preview = Some((path.clone(), *line, preview_of(session, path, *line)));
    }
    if view.texts.of != view.selected
        && let Some(Selected::Line(path, line)) = &view.selected
        && let Some(info) = view.listed.line(path, *line)
    {
        view.texts = Texts::of_line(info);
    } else if view.texts.of != view.selected {
        let texts = match &view.selected {
            Some(Selected::Function(name)) => {
                view.listed.functions.iter().find(|f| f.name == *name).map(|function| Texts {
                    of: view.selected.clone(),
                    condition: function.condition.clone().unwrap_or_default(),
                    hit: function.hit_condition.clone().unwrap_or_default(),
                    ..Texts::default()
                })
            }
            Some(Selected::Exception(filter)) => {
                view.listed.exceptions.iter().find(|row| row.filter == *filter).map(|row| Texts {
                    of: view.selected.clone(),
                    condition: row.condition.clone().unwrap_or_default(),
                    ..Texts::default()
                })
            }
            _ => None,
        };
        view.texts = texts.unwrap_or(Texts { of: view.selected.clone(), ..Texts::default() });
    }
    let modal = egui::Modal::new(ui.scope_id().with("breakpoints")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(880.0);
        ui.label(egui::RichText::new(t!("ide-breakpoints-title")).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(10.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui(vec2(320.0, 460.0), |ui| {
                ui.set_min_size(vec2(320.0, 460.0));
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        let add = ui.button("+").on_hover_text(t!("ide-breakpoints-add"));
                        accessibility::name(&add, egui::Role::Button, &t!("ide-breakpoints-add"));
                        egui::Popup::menu(&add).show(|ui| {
                            if ui.button(t!("ide-breakpoints-add-function")).clicked() {
                                view.prompt = Some(Prompt { kind: PromptKind::Function, text: String::new() });
                                view.focus = Some(Field::Prompt);
                                ui.close();
                            }
                            for offered in &view.listed.offered {
                                if view.listed.exceptions.iter().any(|row| row.filter == offered.filter) {
                                    continue;
                                }
                                if ui.button(&offered.label).clicked() {
                                    actions
                                        .push(("debug.exceptionBreakpoint.add", json!({ "filter": offered.filter })));
                                    view.selected = Some(Selected::Exception(offered.filter.clone()));
                                    ui.close();
                                }
                            }
                        });
                        let remove = ui
                            .add_enabled(view.selected.is_some(), egui::Button::new("−"))
                            .on_hover_text(t!("ide-breakpoints-remove"));
                        accessibility::name(&remove, egui::Role::Button, &t!("ide-breakpoints-remove"));
                        if remove.clicked() {
                            actions.extend(remove_action(view.selected.take()));
                        }
                    });
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().id_salt("breakpoints-tree").max_height(400.0).show(ui, |ui| {
                        tree(ui, p, &mut view, &mut actions, &mut outcome);
                    });
                    if let Some(prompt) = &mut view.prompt {
                        ui.add_space(6.0);
                        let title = match prompt.kind {
                            PromptKind::Function => t!("ide-breakpoints-function-name"),
                            PromptKind::Group => t!("ide-breakpoints-group-name"),
                            PromptKind::Description => t!("ide-breakpoints-description"),
                        };
                        let label = ui.label(egui::RichText::new(title).color(p.text_secondary));
                        let field =
                            code_field(ui, p, &mut prompt.text, label.id, view.focus.take_if(|f| *f == Field::Prompt));
                        let enter = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                        let cancel = ui.input(|input| input.key_pressed(Key::Escape));
                        if enter {
                            let text = prompt.text.trim().to_owned();
                            match (&prompt.kind, &view.selected) {
                                (PromptKind::Function, _) if !text.is_empty() => {
                                    actions.push(("debug.functionBreakpoint.add", json!({ "name": text })));
                                    view.selected = Some(Selected::Function(text));
                                }
                                (PromptKind::Group, Some(Selected::Line(path, line))) => {
                                    actions.push((
                                        "debug.breakpoint.set",
                                        json!({ "path": path, "line": line, "group": text }),
                                    ));
                                }
                                (PromptKind::Description, Some(Selected::Line(path, line))) => {
                                    actions.push((
                                        "debug.breakpoint.set",
                                        json!({ "path": path, "line": line, "description": text }),
                                    ));
                                }
                                _ => {}
                            }
                        }
                        if enter || cancel {
                            view.prompt = None;
                        }
                    }
                });
            });
            let (line, _) = ui.allocate_exact_size(vec2(9.0, 460.0), Sense::hover());
            ui.painter().vline(line.center().x, line.y_range(), Stroke::new(1.0, p.tab_border));
            ui.vertical(|ui| {
                ui.set_width(520.0);
                ui.set_min_height(460.0);
                properties(ui, p, &mut view, &mut actions);
            });
        });
        ui.add_space(12.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary_button(ui, p, &t!("ide-breakpoint-done")).clicked() {
                done = true;
            }
            if let Some(Selected::Line(path, line)) = &view.selected
                && secondary_button(ui, p, &t!("ide-breakpoints-go-to")).clicked()
            {
                outcome = Some(Outcome::Navigate { path: path.clone(), line: *line });
            }
        });
    });
    if let Some(Selected::Line(path, line)) = view.selected.clone() {
        view.texts.suggest.run(ui, registry, session, (&path, line));
    }
    let changed = !actions.is_empty();
    for (command, params) in actions {
        apply(registry, session, command, params);
    }
    if changed {
        view.listed = Listed::read(registry, session);
        view.preview = None;
    }
    if done || escape || outcome.is_some() || (modal.should_close() && view.prompt.is_none()) {
        return Err(outcome);
    }
    Ok(view)
}

fn remove_action(selected: Option<Selected>) -> Option<(&'static str, Value)> {
    match selected? {
        Selected::Line(path, line) => Some(("debug.breakpoint.remove", json!({ "path": path, "line": line }))),
        Selected::Function(name) => Some(("debug.functionBreakpoint.remove", json!({ "name": name }))),
        Selected::Exception(filter) => Some(("debug.exceptionBreakpoint.remove", json!({ "filter": filter }))),
        Selected::Data(id) => Some(("debug.dataBreakpoint.remove", json!({ "dataId": id }))),
    }
}

fn tree(
    ui: &mut Ui,
    p: &IdePalette,
    view: &mut View,
    actions: &mut Vec<(&'static str, Value)>,
    outcome: &mut Option<Outcome>,
) {
    let listed = view.listed.clone();
    if !listed.lines.is_empty() {
        header(ui, p, &t!("ide-breakpoints-lines"));
        let groups = listed.groups();
        let mut sections: Vec<(Option<String>, Vec<&BreakpointInfo>)> =
            groups.iter().map(|group| (Some(group.clone()), Vec::new())).collect();
        sections.push((None, Vec::new()));
        for info in &listed.lines {
            let at = sections.iter().position(|(group, _)| *group == info.group).unwrap_or(sections.len() - 1);
            sections[at].1.push(info);
        }
        for (group, infos) in sections {
            if infos.is_empty() {
                continue;
            }
            if let Some(group) = &group {
                ui.label(egui::RichText::new(format!("▾ {group}")).color(p.text));
            }
            ui.indent(("breakpoint-group", group.clone()), |ui| {
                for info in infos {
                    let key = Selected::Line(info.path.clone(), info.line);
                    let detail = info.description.clone().unwrap_or_default();
                    let (response, toggled) = row(
                        ui,
                        p,
                        view.selected.as_ref() == Some(&key),
                        Some(info.enabled),
                        &place_name(&info.path, info.line),
                        &detail,
                    );
                    if toggled {
                        actions.push((
                            "debug.breakpoint.set",
                            json!({ "path": info.path, "line": info.line, "enabled": !info.enabled }),
                        ));
                    }
                    if response.clicked() {
                        view.selected = Some(key.clone());
                    }
                    if response.double_clicked() {
                        *outcome = Some(Outcome::Navigate { path: info.path.clone(), line: info.line });
                    }
                    response.context_menu(|ui| {
                        view.selected = Some(key.clone());
                        ui.menu_button(t!("ide-breakpoints-move-to-group"), |ui| {
                            for group in &listed.groups() {
                                if ui.button(group).clicked() {
                                    actions.push((
                                        "debug.breakpoint.set",
                                        json!({ "path": info.path, "line": info.line, "group": group }),
                                    ));
                                    ui.close();
                                }
                            }
                            if ui.button(t!("ide-breakpoints-new-group")).clicked() {
                                view.prompt = Some(Prompt { kind: PromptKind::Group, text: String::new() });
                                view.focus = Some(Field::Prompt);
                                ui.close();
                            }
                            if info.group.is_some() && ui.button(t!("ide-breakpoints-no-group")).clicked() {
                                actions.push((
                                    "debug.breakpoint.set",
                                    json!({ "path": info.path, "line": info.line, "group": "" }),
                                ));
                                ui.close();
                            }
                        });
                        if ui.button(t!("ide-breakpoints-edit-description")).clicked() {
                            let text = info.description.clone().unwrap_or_default();
                            view.prompt = Some(Prompt { kind: PromptKind::Description, text });
                            view.focus = Some(Field::Prompt);
                            ui.close();
                        }
                    });
                }
            });
        }
    }
    if !listed.functions.is_empty() {
        header(ui, p, &t!("ide-breakpoints-functions"));
        for function in &listed.functions {
            let key = Selected::Function(function.name.clone());
            let detail = function.message.clone().unwrap_or_default();
            let (response, toggled) =
                row(ui, p, view.selected.as_ref() == Some(&key), Some(function.enabled), &function.name, &detail);
            if toggled {
                actions.push((
                    "debug.functionBreakpoint.add",
                    json!({ "name": function.name, "enabled": !function.enabled }),
                ));
            }
            if response.clicked() {
                view.selected = Some(key);
            }
        }
    }
    if !listed.exceptions.is_empty() {
        header(ui, p, &t!("ide-breakpoints-exceptions"));
        for exception in &listed.exceptions {
            let key = Selected::Exception(exception.filter.clone());
            let (response, toggled) =
                row(ui, p, view.selected.as_ref() == Some(&key), Some(exception.enabled), &exception.label, "");
            if toggled {
                actions.push((
                    "debug.exceptionBreakpoint.add",
                    json!({ "filter": exception.filter, "enabled": !exception.enabled }),
                ));
            }
            if response.clicked() {
                view.selected = Some(key);
            }
        }
    }
    if !listed.data.is_empty() {
        header(ui, p, &t!("ide-breakpoints-data"));
        for data in &listed.data {
            let key = Selected::Data(data.data_id.clone());
            let detail = data.access_type.clone().unwrap_or_default();
            let (response, toggled) =
                row(ui, p, view.selected.as_ref() == Some(&key), Some(data.enabled), &data.description, &detail);
            if toggled {
                actions.push(("debug.dataBreakpoint.set", json!({ "dataId": data.data_id, "enabled": !data.enabled })));
            }
            if response.clicked() {
                view.selected = Some(key);
            }
        }
    }
    if listed.lines.is_empty() && listed.functions.is_empty() && listed.exceptions.is_empty() && listed.data.is_empty()
    {
        ui.label(egui::RichText::new(t!("ide-breakpoints-none")).color(p.text_secondary));
    }
}

fn properties(ui: &mut Ui, p: &IdePalette, view: &mut View, actions: &mut Vec<(&'static str, Value)>) {
    let listed = view.listed.clone();
    if listed.muted {
        ui.label(egui::RichText::new(t!("ide-breakpoints-muted")).color(p.warning_text));
        ui.add_space(6.0);
    }
    match view.selected.clone() {
        Some(Selected::Line(path, line)) => {
            let Some(info) = listed.line(&path, line).cloned() else { return };
            ui.label(egui::RichText::new(place_name(&path, line)).font(theme::semibold(13.0)).color(p.text));
            if info.waiting {
                ui.label(egui::RichText::new(t!("ide-breakpoint-waiting")).color(p.text_secondary));
            }
            ui.add_space(8.0);
            let mut change =
                line_properties(ui, p, &info, &mut view.texts, (listed.support, true), &listed.lines, &mut view.focus);
            if !change.is_empty() {
                change.insert("path".to_owned(), json!(path));
                change.insert("line".to_owned(), json!(line));
                actions.push(("debug.breakpoint.set", Value::Object(change)));
            }
            if let Some((_, at, lines)) = &view.preview {
                ui.add_space(12.0);
                egui::Frame::new().fill(p.editor_background).corner_radius(4).inner_margin(8).show(ui, |ui| {
                    ui.set_min_width(500.0);
                    let first = at.saturating_sub(PREVIEW_LINES);
                    for (index, text) in lines.iter().enumerate() {
                        let number = first + index as u32;
                        let color = if number == *at { p.text } else { p.text_secondary };
                        let mut label = egui::RichText::new(format!("{:>4}  {text}", number + 1))
                            .font(egui::FontId::monospace(12.0))
                            .color(color);
                        if number == *at {
                            label = label.background_color(p.execution_line);
                        }
                        ui.label(label);
                    }
                });
            }
        }
        Some(Selected::Function(name)) => {
            let Some(function) = listed.functions.iter().find(|function| function.name == name).cloned() else {
                return;
            };
            ui.label(egui::RichText::new(&name).font(theme::semibold(13.0)).color(p.text));
            if !listed.support.functions {
                ui.label(egui::RichText::new(t!("ide-breakpoints-functions-unsupported")).color(p.text_secondary));
            }
            ui.add_space(8.0);
            let mut enabled = function.enabled;
            let mut change = serde_json::Map::new();
            if ui.checkbox(&mut enabled, t!("ide-breakpoint-enabled")).changed() {
                change.insert("enabled".to_owned(), json!(enabled));
            }
            if listed.support.conditions {
                ui.add_space(8.0);
                let label = ui.label(egui::RichText::new(t!("ide-breakpoint-condition")).color(p.text_secondary));
                if code_field(ui, p, &mut view.texts.condition, label.id, None).changed() {
                    change.insert("condition".to_owned(), json!(view.texts.condition));
                }
            }
            if listed.support.hit_conditions {
                ui.add_space(8.0);
                let label = ui.label(egui::RichText::new(t!("ide-breakpoint-pass-count")).color(p.text_secondary));
                if code_field(ui, p, &mut view.texts.hit, label.id, None).changed() {
                    change.insert("hitCondition".to_owned(), json!(view.texts.hit));
                }
            }
            if !change.is_empty() {
                change.insert("name".to_owned(), json!(name));
                actions.push(("debug.functionBreakpoint.add", Value::Object(change)));
            }
        }
        Some(Selected::Exception(filter)) => {
            let Some(row) = listed.exceptions.iter().find(|row| row.filter == filter).cloned() else { return };
            ui.label(egui::RichText::new(&row.label).font(theme::semibold(13.0)).color(p.text));
            ui.add_space(8.0);
            let mut enabled = row.enabled;
            let mut change = serde_json::Map::new();
            if ui.checkbox(&mut enabled, t!("ide-breakpoint-enabled")).changed() {
                change.insert("enabled".to_owned(), json!(enabled));
            }
            let conditional = listed
                .offered
                .iter()
                .find(|offered| offered.filter == filter)
                .is_some_and(|offered| offered.supports_condition);
            if conditional && listed.support.exception_conditions {
                ui.add_space(8.0);
                let label = ui.label(egui::RichText::new(t!("ide-breakpoint-condition")).color(p.text_secondary));
                if code_field(ui, p, &mut view.texts.condition, label.id, None).changed() {
                    change.insert("condition".to_owned(), json!(view.texts.condition));
                }
            }
            if !change.is_empty() {
                change.insert("filter".to_owned(), json!(filter));
                actions.push(("debug.exceptionBreakpoint.add", Value::Object(change)));
            }
        }
        Some(Selected::Data(id)) => {
            let Some(data) = listed.data.iter().find(|data| data.data_id == id).cloned() else { return };
            ui.label(egui::RichText::new(&data.description).font(theme::semibold(13.0)).color(p.text));
            ui.add_space(8.0);
            let mut enabled = data.enabled;
            if ui.checkbox(&mut enabled, t!("ide-breakpoint-enabled")).changed() {
                actions.push(("debug.dataBreakpoint.set", json!({ "dataId": id, "enabled": enabled })));
            }
        }
        None => {
            ui.label(egui::RichText::new(t!("ide-breakpoints-select")).color(p.text_secondary));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use egui::Event;
    use egui::accesskit::Role;
    use zen_dap::fake::FakeAdapter;
    use zen_run::RunEnv;
    use zen_run::fake::FakeProvider;
    use zen_workbench::debug::{Debugger, FakeLauncher};
    use zen_workbench::run::Runs;

    use super::*;

    struct Harness {
        registry: Registry,
        session: Session,
        main: PathBuf,
        _dir: tempfile::TempDir,
        ctx: egui::Context,
        dialogs: BreakpointDialogs,
        outcome: Option<Outcome>,
        tree: Option<egui::accesskit::TreeUpdate>,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let main = dir.path().join("main.go");
            std::fs::write(&main, (1..=12).map(|line| format!("line {line}\n")).collect::<String>()).unwrap();
            let mut registry = Registry::new();
            zen_workbench::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            session.state_mut::<Runs>().set_providers(vec![Arc::new(FakeProvider::new())], RunEnv::default());
            session.state_mut::<Debugger>().set_launcher(Arc::new(FakeLauncher::new(FakeAdapter::new())));
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            Self {
                registry,
                session,
                main,
                _dir: dir,
                ctx,
                dialogs: BreakpointDialogs::default(),
                outcome: None,
                tree: None,
                time: 0.0,
            }
        }

        fn dispatch(&mut self, id: &str, params: Value) -> Value {
            self.registry.dispatch(&mut self.session, id, params).unwrap()
        }

        fn frame(&mut self, events: Vec<Event>) {
            let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
            self.time += 1.0 / 60.0;
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1400.0, 900.0))),
                events,
                time: Some(self.time),
                ..Default::default()
            };
            let (registry, session, dialogs) = (&self.registry, &mut self.session, &mut self.dialogs);
            let mut outcome = None;
            let mut output = self.ctx.run_ui(input, |ui| outcome = dialogs.show(ui, registry, session, &palette));
            output.textures_delta.clear();
            self.tree = output.platform_output.accesskit_update.or(self.tree.take());
            self.outcome = outcome.or(self.outcome.take());
        }

        fn listed(&mut self) -> Value {
            self.dispatch("debug.breakpoint.list", json!({}))
        }

        fn widget(&self, role: Role, name: &str) -> Pos2 {
            let tree = self.tree.as_ref().expect("a frame ran");
            let node = accessibility::testing::find(tree, role, name).unwrap_or_else(|| {
                let names: Vec<String> = tree
                    .nodes
                    .iter()
                    .filter_map(|(_, node)| {
                        accessibility::accessible_name(node, &tree.nodes).map(|n| format!("{:?}:{n}", node.role()))
                    })
                    .collect();
                panic!("no {name} in {names:?}")
            });
            let bounds = node.bounds().unwrap();
            pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32)
        }

        fn click(&mut self, at: Pos2) {
            let button = egui::PointerButton::Primary;
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![Event::PointerButton { pos: at, button, pressed: true, modifiers: Default::default() }]);
            self.frame(vec![Event::PointerButton { pos: at, button, pressed: false, modifiers: Default::default() }]);
            self.frame(Vec::new());
        }
    }

    #[test]
    fn the_popup_changes_the_breakpoint_at_once_and_more_opens_the_dialog() {
        let mut harness = Harness::new();
        let main = harness.main.clone();
        harness.dispatch("debug.breakpoint.set", json!({ "path": main, "line": 2 }));
        let opened = harness.dialogs.open_popup(
            &harness.registry,
            &mut harness.session,
            (main.clone(), 2),
            pos2(100.0, 100.0),
            Some("condition"),
        );
        assert!(opened);
        harness.frame(Vec::new());
        assert_eq!(harness.dialogs.names(), ["breakpointProperties"]);
        harness.frame(vec![Event::Text("x > 1".to_owned())]);
        assert_eq!(harness.listed()["breakpoints"][0]["condition"], "x > 1", "the condition applies as it is typed");
        let suspend = harness.widget(Role::CheckBox, &t!("ide-breakpoint-suspend"));
        harness.click(suspend);
        let breakpoint = harness.listed()["breakpoints"][0].clone();
        assert_eq!((breakpoint["suspend"].clone(), breakpoint["logHit"].clone()), (json!("none"), json!(true)));
        let more = harness.widget(Role::Link, &t!("ide-breakpoint-more"));
        harness.click(more);
        assert_eq!(harness.dialogs.names(), ["breakpoints"]);
        assert!(!harness.dialogs.open_popup(&harness.registry, &mut harness.session, (main, 7), Pos2::ZERO, None));
    }

    #[test]
    fn a_suggestion_of_the_language_server_completes_the_condition() {
        let mut harness = Harness::new();
        let main = harness.main.clone();
        harness.dispatch("debug.breakpoint.set", json!({ "path": main, "line": 2 }));
        let place = (main, 2);
        harness.dialogs.open_popup(
            &harness.registry,
            &mut harness.session,
            place,
            pos2(100.0, 100.0),
            Some("condition"),
        );
        harness.frame(Vec::new());
        harness.frame(vec![Event::Text("x > co".to_owned())]);
        let popup = harness.dialogs.popup.as_mut().unwrap();
        assert_eq!(popup.texts.suggest.field, Some(Field::Condition), "typing asks debug.expression.complete");
        popup.texts.suggest.answer = json!({ "pending": false, "start": 4, "items": [
            { "label": "count", "insert": "count", "kind": "variable", "detail": "int" },
            { "label": "copy", "insert": "copy(dst, src)", "kind": "function" },
        ] });
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        harness.widget(Role::ListItem, "count");
        let key =
            |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
        harness.frame(vec![key(Key::ArrowDown)]);
        harness.frame(vec![key(Key::Enter)]);
        assert_eq!(harness.listed()["breakpoints"][0]["condition"], "x > copy(dst, src)");
        assert!(harness.dialogs.popup.is_some(), "Enter takes the suggestion and leaves the popup open");
    }

    #[test]
    fn the_dialog_lists_by_kind_removes_adds_functions_and_navigates() {
        let mut harness = Harness::new();
        let main = harness.main.clone();
        harness.dispatch("debug.breakpoint.set", json!({ "path": main, "line": 2, "group": "Mine" }));
        harness.dispatch("debug.breakpoint.set", json!({ "path": main, "line": 5 }));
        harness.dispatch("debug.exceptionBreakpoint.add", json!({ "filter": "uncaught" }));
        harness.dialogs.open_view(&harness.registry, &mut harness.session, Some((main.clone(), 5)));
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        assert_eq!(harness.dialogs.names(), ["breakpoints"]);
        let view = harness.dialogs.view.as_ref().unwrap();
        assert_eq!(view.selected, Some(Selected::Line(main.clone(), 5)));
        assert_eq!(view.preview.as_ref().map(|(_, _, lines)| lines.len()), Some(9), "the code around the line");
        harness.widget(Role::TreeItem, "main.go:3");
        harness.widget(Role::TreeItem, "uncaught");
        let remove = harness.widget(Role::Button, &t!("ide-breakpoints-remove"));
        harness.click(remove);
        assert_eq!(harness.listed()["breakpoints"].as_array().unwrap().len(), 1, "− removes the selected one");
        harness.dialogs.view.as_mut().unwrap().prompt =
            Some(Prompt { kind: PromptKind::Function, text: String::new() });
        harness.dialogs.view.as_mut().unwrap().focus = Some(Field::Prompt);
        harness.frame(Vec::new());
        harness.frame(vec![Event::Text("main.run".to_owned())]);
        let enter = Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        harness.frame(vec![enter]);
        assert_eq!(harness.listed()["functions"][0]["name"], "main.run");
        let row = harness.widget(Role::TreeItem, "main.go:3");
        harness.click(row);
        let go = harness.widget(Role::Button, &t!("ide-breakpoints-go-to"));
        harness.click(go);
        assert_eq!(harness.outcome, Some(Outcome::Navigate { path: main, line: 2 }));
        assert!(!harness.dialogs.is_open(), "navigating closes the dialog");
    }
}

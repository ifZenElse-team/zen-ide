use std::collections::{BTreeMap, BTreeSet};

use egui::{Key, Modifiers, Rect, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_modules::generate::Chooser;

use super::IdePalette;
use super::caret_popup::{CaretPlace, PopupAnchor};
use super::vcs::{primary_button, secondary_button};
use crate::theme;

const ROW: f32 = 24.0;
const VISIBLE: usize = 14;
const TREE_ROWS: f32 = 12.0;
const WIDTH: f32 = 460.0;

#[derive(Clone, Debug, PartialEq)]
struct Generator {
    id: String,
    title: String,
    chooser: bool,
    action: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum GenerateOutcome {
    Apply { id: String, members: Option<Vec<String>>, options: BTreeMap<String, bool> },
    Notice(String),
}

#[derive(Default)]
enum State {
    #[default]
    Closed,
    Listing(PopupAnchor, Option<String>),
    Shown {
        at: PopupAnchor,
        generators: Vec<Generator>,
        selected: usize,
        reveal: bool,
    },
    Asking(Generator, bool),
    Choosing(Box<MemberChooser>),
}

#[derive(Default)]
pub(super) struct GeneratePopup {
    state: State,
}

fn generators(list: &Value) -> Vec<Generator> {
    list["generators"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|generator| {
            Some(Generator {
                id: generator["id"].as_str()?.to_owned(),
                title: generator["title"].as_str()?.to_owned(),
                chooser: generator["chooser"] == true,
                action: generator["action"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

fn nothing_for(action: Option<String>) -> String {
    match action.as_deref() {
        Some("override") => t!("ide-generate-nothing-to-override"),
        Some("implement") => t!("ide-generate-nothing-to-implement"),
        _ => t!("workbench-generate-none"),
    }
}

fn failed(error: &zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message.clone(),
        other => other.to_string(),
    }
}

impl GeneratePopup {
    pub(super) fn open(&mut self, at: PopupAnchor, action: Option<&str>) {
        self.state = State::Listing(at, action.map(str::to_owned));
    }

    pub(super) fn open_name(&self) -> Option<&'static str> {
        match self.state {
            State::Closed => None,
            State::Listing(_, None) | State::Shown { .. } => Some("generate"),
            State::Listing(_, Some(_)) | State::Asking(..) | State::Choosing(_) => Some("memberChooser"),
        }
    }

    pub(super) fn follow(&mut self, ui: &Ui, registry: &Registry, session: &mut Session) -> Option<GenerateOutcome> {
        let repaint = || ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        match std::mem::take(&mut self.state) {
            State::Listing(at, action) => match registry.dispatch(session, "editor.generate.list", json!({})) {
                Ok(list) if list["pending"] == true => {
                    self.state = State::Listing(at, action);
                    repaint();
                }
                Ok(list) if action.is_some() => {
                    let found = generators(&list).into_iter().find(|generator| generator.action == action);
                    let Some(generator) = found else { return Some(GenerateOutcome::Notice(nothing_for(action))) };
                    if generator.chooser {
                        self.state = State::Asking(generator, true);
                        repaint();
                    } else {
                        return Some(GenerateOutcome::Apply {
                            id: generator.id,
                            members: None,
                            options: BTreeMap::new(),
                        });
                    }
                }
                Ok(list) => {
                    let found = generators(&list);
                    if found.is_empty() {
                        let error = list["error"].as_str().map_or_else(|| t!("workbench-generate-none"), str::to_owned);
                        return Some(GenerateOutcome::Notice(error));
                    }
                    self.state = State::Shown { at, generators: found, selected: 0, reveal: true };
                }
                Err(error) => return Some(GenerateOutcome::Notice(failed(&error))),
            },
            State::Asking(generator, direct) => {
                match registry.dispatch(session, "editor.generate.members", json!({ "id": generator.id })) {
                    Ok(answer) if answer["pending"] == true => {
                        self.state = State::Asking(generator, direct);
                        repaint();
                    }
                    Ok(answer) => match serde_json::from_value::<Chooser>(answer["chooser"].clone()) {
                        Ok(chooser) if direct && chooser.members.is_empty() && !chooser.allow_empty => {
                            return Some(GenerateOutcome::Notice(nothing_for(generator.action)));
                        }
                        Ok(chooser) => {
                            self.state = State::Choosing(Box::new(MemberChooser::new(generator.id, chooser)))
                        }
                        Err(error) => return Some(GenerateOutcome::Notice(error.to_string())),
                    },
                    Err(error) => return Some(GenerateOutcome::Notice(failed(&error))),
                }
            }
            other => self.state = other,
        }
        None
    }

    pub(super) fn keys(&mut self, ui: &mut Ui) -> Option<GenerateOutcome> {
        if matches!(self.state, State::Listing(..) | State::Asking(..))
            && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape))
        {
            self.state = State::Closed;
            return None;
        }
        let State::Shown { generators, selected, reveal, .. } = &mut self.state else { return None };
        let count = generators.len();
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if up {
            *selected = selected.checked_sub(1).unwrap_or(count - 1);
        }
        if down {
            *selected = (*selected + 1) % count;
        }
        *reveal |= up || down;
        if enter {
            let index = *selected;
            return self.choose(index);
        }
        if escape {
            self.state = State::Closed;
        }
        None
    }

    fn choose(&mut self, index: usize) -> Option<GenerateOutcome> {
        let State::Shown { generators, .. } = &self.state else { return None };
        let generator = generators.get(index)?.clone();
        if generator.chooser {
            self.state = State::Asking(generator, false);
            return None;
        }
        self.state = State::Closed;
        Some(GenerateOutcome::Apply { id: generator.id, members: None, options: BTreeMap::new() })
    }

    pub(super) fn show(&mut self, ui: &Ui, palette: &IdePalette, caret: CaretPlace) -> Option<GenerateOutcome> {
        if let State::Choosing(chooser) = &mut self.state {
            let outcome = chooser.show(ui, palette);
            if outcome.is_some() || chooser.cancelled {
                self.state = State::Closed;
            }
            return outcome;
        }
        let State::Shown { at, generators, selected, reveal } = &mut self.state else { return None };
        let area = egui::Area::new(ui.scope_id().with("generate-popup")).order(egui::Order::Foreground);
        let area = at.area(ui.ctx(), area, caret)?;
        let mut chosen = None;
        let area = area.show(ui.ctx(), |ui| {
            palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                let widest = generators
                    .iter()
                    .map(|generator| {
                        ui.painter()
                            .layout_no_wrap(generator.title.clone(), theme::regular(13.0), palette.text)
                            .size()
                            .x
                    })
                    .fold(0.0_f32, f32::max);
                ui.set_width((widest + 32.0).clamp(200.0, 520.0));
                ui.spacing_mut().item_spacing.y = 0.0;
                let heading = egui::RichText::new(t!("ide-generate-title"))
                    .font(theme::semibold(12.0))
                    .color(palette.text_secondary);
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(heading);
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(ROW * VISIBLE as f32).auto_shrink([false, true]).show(
                    ui,
                    |ui| {
                        for (index, generator) in generators.iter().enumerate() {
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                            if index == *selected {
                                ui.painter().rect_filled(rect, 4, palette.selection);
                                if *reveal {
                                    response.scroll_to_me(None);
                                }
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 4, palette.popup_hover);
                            }
                            let label = if generator.chooser {
                                format!("{}…", generator.title)
                            } else {
                                generator.title.clone()
                            };
                            crate::accessibility::item(
                                &response,
                                egui::Role::ListBoxOption,
                                index == *selected,
                                &label,
                            );
                            let text = ui.painter().layout_no_wrap(label, theme::regular(13.0), palette.text);
                            let left = rect.left() + 10.0;
                            let clip =
                                Rect::from_min_max(pos2(left, rect.top()), pos2(rect.right() - 6.0, rect.bottom()));
                            ui.painter().with_clip_rect(clip).galley(
                                pos2(left, rect.center().y - text.size().y / 2.0),
                                text,
                                palette.text,
                            );
                            if response.clicked() {
                                chosen = Some(index);
                            }
                        }
                    },
                );
            });
        });
        *reveal = false;
        if let Some(index) = chosen {
            return self.choose(index);
        }
        let pressed_outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| !area.response.rect.contains(pos))
        });
        if pressed_outside {
            self.state = State::Closed;
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Tree,
    SelectAll,
    Sort,
    Option(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    Group(String),
    Member(usize),
}

pub(super) struct MemberChooser {
    id: String,
    chooser: Chooser,
    checked: BTreeSet<String>,
    options: Vec<bool>,
    sorted: bool,
    query: String,
    cursor: usize,
    focus: Focus,
    reveal: bool,
    cancelled: bool,
    error: Option<String>,
}

impl MemberChooser {
    fn new(id: String, chooser: Chooser) -> Self {
        let checked = chooser.members.iter().filter(|member| member.selected).map(|member| member.id.clone()).collect();
        let options = chooser.options.iter().map(|option| option.value).collect();
        let mut dialog = Self {
            id,
            chooser,
            checked,
            options,
            sorted: false,
            query: String::new(),
            cursor: 0,
            focus: Focus::Tree,
            reveal: true,
            cancelled: false,
            error: None,
        };
        dialog.cursor = dialog.rows().iter().position(|row| matches!(row, Row::Member(_))).unwrap_or(0);
        dialog
    }

    fn matches(&self, index: usize) -> bool {
        let member = &self.chooser.members[index];
        let text = format!("{} {}", member.name, member.detail.as_deref().unwrap_or_default()).to_lowercase();
        self.query.to_lowercase().split_whitespace().all(|word| text.contains(word))
    }

    fn rows(&self) -> Vec<Row> {
        if self.chooser.search {
            return (0..self.chooser.members.len()).filter(|&index| self.matches(index)).map(Row::Member).collect();
        }
        let mut groups: Vec<Option<String>> = Vec::new();
        for member in &self.chooser.members {
            if !groups.contains(&member.group) {
                groups.push(member.group.clone());
            }
        }
        let mut rows = Vec::new();
        for group in groups {
            let mut members: Vec<usize> =
                (0..self.chooser.members.len()).filter(|&index| self.chooser.members[index].group == group).collect();
            if self.sorted {
                members.sort_by_key(|&index| self.chooser.members[index].name.to_lowercase());
            }
            if let Some(name) = group {
                rows.push(Row::Group(name));
            }
            rows.extend(members.into_iter().map(Row::Member));
        }
        rows
    }

    fn members_of(&self, row: &Row) -> Vec<String> {
        match row {
            Row::Member(index) => vec![self.chooser.members[*index].id.clone()],
            Row::Group(name) => self
                .chooser
                .members
                .iter()
                .filter(|member| member.group.as_deref() == Some(name))
                .map(|member| member.id.clone())
                .collect(),
        }
    }

    fn row_checked(&self, row: &Row) -> Option<bool> {
        let ids = self.members_of(row);
        let on = ids.iter().filter(|id| self.checked.contains(*id)).count();
        if on == ids.len() && !ids.is_empty() {
            Some(true)
        } else if on == 0 {
            Some(false)
        } else {
            None
        }
    }

    fn toggle(&mut self, row: &Row) {
        let on = self.row_checked(row) != Some(true);
        for id in self.members_of(row) {
            if on {
                self.checked.insert(id);
            } else {
                self.checked.remove(&id);
            }
        }
    }

    fn all_checked(&self) -> bool {
        !self.chooser.members.is_empty() && self.chooser.members.iter().all(|member| self.checked.contains(&member.id))
    }

    fn toggle_all(&mut self) {
        if self.all_checked() {
            self.checked.clear();
        } else {
            self.checked = self.chooser.members.iter().map(|member| member.id.clone()).collect();
        }
    }

    fn chosen(&self) -> Vec<String> {
        if self.chooser.search {
            return match self.rows().get(self.cursor) {
                Some(Row::Member(index)) => vec![self.chooser.members[*index].id.clone()],
                _ => Vec::new(),
            };
        }
        self.chooser
            .members
            .iter()
            .filter(|member| self.checked.contains(&member.id))
            .map(|member| member.id.clone())
            .collect()
    }

    fn confirm(&mut self) -> Option<GenerateOutcome> {
        let members = self.chosen();
        if members.is_empty() && !self.chooser.allow_empty && !self.chooser.members.is_empty() {
            self.error = Some(t!("workbench-generate-nothing-chosen"));
            return None;
        }
        let options =
            self.chooser.options.iter().zip(&self.options).map(|(option, value)| (option.id.clone(), *value)).collect();
        Some(GenerateOutcome::Apply { id: self.id.clone(), members: Some(members), options })
    }

    fn focus_ring(&self) -> Vec<Focus> {
        if self.chooser.search {
            return vec![Focus::Tree];
        }
        let mut ring = vec![Focus::Tree, Focus::SelectAll, Focus::Sort];
        ring.extend((0..self.options.len()).map(Focus::Option));
        ring
    }

    fn keys(&mut self, ui: &mut Ui) -> Option<GenerateOutcome> {
        let pressed = |ui: &mut Ui, modifiers, key| ui.input_mut(|input| input.consume_key(modifiers, key));
        if pressed(ui, Modifiers::NONE, Key::Escape) {
            self.cancelled = true;
            return None;
        }
        if pressed(ui, Modifiers::NONE, Key::Enter) {
            return self.confirm();
        }
        let search = self.chooser.search;
        if !search && pressed(ui, Modifiers::COMMAND, Key::A) {
            self.toggle_all();
        }
        let ring = self.focus_ring();
        let at = ring.iter().position(|focus| *focus == self.focus).unwrap_or(0);
        if pressed(ui, Modifiers::SHIFT, Key::Tab) {
            self.focus = ring[(at + ring.len() - 1) % ring.len()];
        } else if pressed(ui, Modifiers::NONE, Key::Tab) {
            self.focus = ring[(at + 1) % ring.len()];
        }
        let rows = self.rows();
        let space = !search && pressed(ui, Modifiers::NONE, Key::Space);
        match self.focus {
            Focus::Tree if !rows.is_empty() => {
                let (up, down) = ui.input_mut(|input| {
                    (
                        input.count_and_consume_key(Modifiers::NONE, Key::ArrowUp),
                        input.count_and_consume_key(Modifiers::NONE, Key::ArrowDown),
                    )
                });
                let count = rows.len();
                self.cursor = (self.cursor % count + down % count + count - up % count) % count;
                self.reveal |= up + down > 0;
                if space && let Some(row) = rows.get(self.cursor.min(rows.len() - 1)) {
                    self.toggle(&row.clone());
                }
            }
            Focus::Tree => {}
            Focus::SelectAll if space => self.toggle_all(),
            Focus::Sort if space => self.toggle_sort(),
            Focus::Option(index) if space => {
                if let Some(value) = self.options.get_mut(index) {
                    *value = !*value;
                }
            }
            _ => {}
        }
        None
    }

    fn toggle_sort(&mut self) {
        let rows = self.rows();
        let current = rows.get(self.cursor).cloned();
        self.sorted = !self.sorted;
        if let Some(current) = current {
            self.cursor = self.rows().iter().position(|row| *row == current).unwrap_or(0);
        }
    }

    fn show(&mut self, ui: &Ui, p: &IdePalette) -> Option<GenerateOutcome> {
        let mut outcome = None;
        let modal = egui::Modal::new(ui.scope_id().with("generate-members")).frame(super::dialog_frame(p)).show(
            ui.ctx(),
            |ui| {
                if outcome.is_none() {
                    outcome = self.keys(ui);
                }
                ui.set_width(WIDTH);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(&self.chooser.title).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(10.0);
                if self.chooser.search {
                    let field = egui::TextEdit::singleline(&mut self.query)
                        .hint_text(t!("ide-generate-search"))
                        .desired_width(f32::INFINITY);
                    let response = ui.add(field);
                    response.request_focus();
                    if response.changed() {
                        self.cursor = 0;
                        self.reveal = true;
                    }
                } else {
                    ui.horizontal(|ui| {
                        let mut all = self.all_checked();
                        let select_all = ui.checkbox(
                            &mut all,
                            egui::RichText::new(t!("ide-generate-select-all")).font(theme::regular(12.5)),
                        );
                        focus_ring(ui, p, &select_all, self.focus == Focus::SelectAll);
                        if select_all.changed() {
                            self.toggle_all();
                            self.focus = Focus::SelectAll;
                        }
                        ui.add_space(12.0);
                        let mut sorted = self.sorted;
                        let sort = ui.checkbox(
                            &mut sorted,
                            egui::RichText::new(t!("ide-generate-sort")).font(theme::regular(12.5)),
                        );
                        focus_ring(ui, p, &sort, self.focus == Focus::Sort);
                        if sort.changed() {
                            self.toggle_sort();
                            self.focus = Focus::Sort;
                        }
                    });
                }
                ui.add_space(8.0);
                let rows = self.rows();
                let frame = egui::Frame::new()
                    .stroke(egui::Stroke::new(1.0, p.text_field_border))
                    .corner_radius(4)
                    .inner_margin(egui::Margin::same(2));
                frame.show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(ROW * TREE_ROWS)
                        .min_scrolled_height(ROW * 4.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if rows.is_empty() {
                                ui.add_space(6.0);
                                ui.label(
                                    egui::RichText::new(t!("ide-generate-no-members"))
                                        .font(theme::regular(13.0))
                                        .color(p.text_secondary),
                                );
                                ui.add_space(6.0);
                            }
                            for (index, row) in rows.iter().enumerate() {
                                let (rect, response) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                                let current = index == self.cursor;
                                if current {
                                    let fill = if self.focus == Focus::Tree { p.selection } else { p.popup_hover };
                                    ui.painter().rect_filled(rect, 3, fill);
                                    if self.reveal {
                                        response.scroll_to_me(None);
                                    }
                                }
                                let indent = match row {
                                    Row::Group(_) => 4.0,
                                    Row::Member(_)
                                        if self.chooser.members.iter().any(|member| member.group.is_some()) =>
                                    {
                                        24.0
                                    }
                                    Row::Member(_) => 4.0,
                                };
                                let check = Rect::from_min_size(
                                    pos2(rect.left() + indent, rect.center().y - 7.0),
                                    vec2(14.0, 14.0),
                                );
                                let state = self.row_checked(row);
                                if !self.chooser.search {
                                    draw_check(ui, p, check, state);
                                }
                                let (name, detail, font) = match row {
                                    Row::Group(name) => (name.clone(), None, theme::semibold(13.0)),
                                    Row::Member(member) => {
                                        let member = &self.chooser.members[*member];
                                        (member.name.clone(), member.detail.clone(), theme::regular(13.0))
                                    }
                                };
                                let label = match &detail {
                                    Some(detail) => format!("{name}: {detail}"),
                                    None => name.clone(),
                                };
                                crate::accessibility::item(
                                    &response,
                                    egui::Role::CheckBox,
                                    state == Some(true),
                                    &label,
                                );
                                let galley = ui.painter().layout_no_wrap(name, font, p.text);
                                let left = if self.chooser.search { rect.left() + 8.0 } else { check.right() + 8.0 };
                                let width = galley.size().x;
                                ui.painter().galley(
                                    pos2(left, rect.center().y - galley.size().y / 2.0),
                                    galley,
                                    p.text,
                                );
                                if let Some(detail) = detail {
                                    let galley =
                                        ui.painter().layout_no_wrap(detail, theme::regular(12.5), p.text_secondary);
                                    let at = pos2(left + width + 8.0, rect.center().y - galley.size().y / 2.0);
                                    ui.painter().with_clip_rect(rect).galley(at, galley, p.text_secondary);
                                }
                                if response.clicked() {
                                    self.cursor = index;
                                    self.focus = Focus::Tree;
                                    if let Some(pos) = response.interact_pointer_pos()
                                        && pos.x <= check.right() + 4.0
                                        && !self.chooser.search
                                    {
                                        self.toggle(row);
                                    }
                                }
                                if response.double_clicked() {
                                    if self.chooser.search {
                                        self.cursor = index;
                                        outcome = self.confirm();
                                    } else {
                                        self.toggle(row);
                                    }
                                }
                            }
                        });
                });
                self.reveal = false;
                if !self.chooser.options.is_empty() {
                    ui.add_space(10.0);
                    for index in 0..self.options.len() {
                        let label = egui::RichText::new(&self.chooser.options[index].label)
                            .font(theme::regular(13.0))
                            .color(p.text);
                        let response = ui.checkbox(&mut self.options[index], label);
                        focus_ring(ui, p, &response, self.focus == Focus::Option(index));
                        if response.changed() {
                            self.focus = Focus::Option(index);
                        }
                        ui.add_space(4.0);
                    }
                }
                if let Some(error) = &self.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(error).font(theme::regular(12.0)).color(p.error_text));
                }
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if primary_button(ui, p, &t!("ide-generate-ok")).clicked() {
                        outcome = self.confirm();
                    }
                    if secondary_button(ui, p, &t!("ide-generate-cancel")).clicked() {
                        self.cancelled = true;
                    }
                });
            },
        );
        if outcome.is_none() && modal.should_close() {
            self.cancelled = true;
        }
        outcome
    }
}

fn focus_ring(ui: &Ui, p: &IdePalette, response: &egui::Response, focused: bool) {
    if focused {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            3,
            egui::Stroke::new(1.5, p.accent),
            egui::StrokeKind::Outside,
        );
    }
}

fn draw_check(ui: &Ui, p: &IdePalette, rect: Rect, state: Option<bool>) {
    let painter = ui.painter();
    match state {
        Some(false) => {
            painter.rect_stroke(rect, 3, egui::Stroke::new(1.0, p.text_secondary), egui::StrokeKind::Inside);
        }
        Some(true) => {
            painter.rect_filled(rect, 3, p.accent);
            let points = vec![
                pos2(rect.left() + 3.0, rect.center().y),
                pos2(rect.left() + 6.0, rect.bottom() - 3.5),
                pos2(rect.right() - 3.0, rect.top() + 3.5),
            ];
            painter.add(egui::Shape::line(points, egui::Stroke::new(1.6, p.accent_text)));
        }
        None => {
            painter.rect_filled(rect, 3, p.accent);
            let y = rect.center().y;
            painter.line_segment(
                [pos2(rect.left() + 3.5, y), pos2(rect.right() - 3.5, y)],
                egui::Stroke::new(1.6, p.accent_text),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use egui::Pos2;
    use zen_modules::generate::{ChooserOption, Member};

    use super::*;

    fn key(key: Key, modifiers: Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn member(id: &str, name: &str, group: &str, selected: bool) -> Member {
        Member {
            id: id.to_owned(),
            name: name.to_owned(),
            detail: Some("int".to_owned()),
            group: Some(group.to_owned()),
            selected,
        }
    }

    fn chooser() -> Chooser {
        Chooser {
            title: "Choose Fields".to_owned(),
            members: vec![
                member("0", "y", "Point", true),
                member("1", "x", "Point", false),
                member("2", "z", "Base", false),
            ],
            options: vec![ChooserOption {
                id: "pointer".to_owned(),
                label: "Return a pointer".to_owned(),
                value: true,
            }],
            allow_empty: false,
            search: false,
        }
    }

    fn run(ctx: &egui::Context, dialog: &mut MemberChooser, events: Vec<egui::Event>) -> Option<GenerateOutcome> {
        let mut outcome = None;
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 800.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| outcome = dialog.show(ui, &IdePalette::zen_dark()));
        output.textures_delta.clear();
        outcome
    }

    #[test]
    fn the_member_chooser_works_from_the_keyboard_alone() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        let mut dialog = MemberChooser::new("1.0".to_owned(), chooser());
        assert_eq!(dialog.rows()[0], Row::Group("Point".to_owned()));
        assert_eq!(dialog.cursor, 1, "the first member, not its group");
        assert_eq!(run(&ctx, &mut dialog, Vec::new()), None);
        run(&ctx, &mut dialog, vec![key(Key::ArrowDown, Modifiers::NONE), key(Key::Space, Modifiers::NONE)]);
        assert_eq!(dialog.chosen(), ["0", "1"], "Space checks the member at the cursor");
        run(
            &ctx,
            &mut dialog,
            vec![
                key(Key::ArrowUp, Modifiers::NONE),
                key(Key::ArrowUp, Modifiers::NONE),
                key(Key::Space, Modifiers::NONE),
            ],
        );
        assert!(dialog.chosen().is_empty(), "Space on a fully checked group unchecks all of it");
        run(&ctx, &mut dialog, vec![key(Key::A, Modifiers::COMMAND)]);
        assert_eq!(dialog.chosen(), ["0", "1", "2"], "⌘A checks every member");
        run(&ctx, &mut dialog, vec![key(Key::Tab, Modifiers::NONE), key(Key::Space, Modifiers::NONE)]);
        assert!(dialog.chosen().is_empty(), "Select All unchecks everything when all is checked");
        assert_eq!(run(&ctx, &mut dialog, vec![key(Key::Enter, Modifiers::NONE)]), None, "nothing chosen");
        assert_eq!(dialog.error.as_deref(), Some(t!("workbench-generate-nothing-chosen").as_str()));

        run(&ctx, &mut dialog, vec![key(Key::Tab, Modifiers::NONE), key(Key::Space, Modifiers::NONE)]);
        assert!(dialog.sorted);
        let names: Vec<Row> = dialog.rows();
        assert_eq!(names[1..3], [Row::Member(1), Row::Member(0)], "sorted by name inside the group");
        run(&ctx, &mut dialog, vec![key(Key::Tab, Modifiers::NONE), key(Key::Space, Modifiers::NONE)]);
        assert_eq!(dialog.options, [false]);
        run(
            &ctx,
            &mut dialog,
            vec![
                key(Key::Tab, Modifiers::NONE),
                key(Key::ArrowDown, Modifiers::NONE),
                key(Key::Space, Modifiers::NONE),
            ],
        );
        let outcome = run(&ctx, &mut dialog, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(
            outcome,
            Some(GenerateOutcome::Apply {
                id: "1.0".to_owned(),
                members: Some(vec!["1".to_owned()]),
                options: [("pointer".to_owned(), false)].into(),
            })
        );
        let mut dialog = MemberChooser::new("1.0".to_owned(), chooser());
        run(&ctx, &mut dialog, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(dialog.cancelled);
    }

    #[test]
    fn a_search_chooser_narrows_the_list_and_picks_the_member_at_the_cursor() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let ctx = egui::Context::default();
        let member = |id: &str, name: &str, detail: &str| Member {
            id: id.to_owned(),
            name: name.to_owned(),
            detail: Some(detail.to_owned()),
            ..Member::default()
        };
        let chooser = Chooser {
            title: "Implement".to_owned(),
            members: vec![member("a", "Reader", "io"), member("b", "Writer", "io"), member("c", "Handler", "net/http")],
            search: true,
            ..Chooser::default()
        };
        let mut dialog = MemberChooser::new("1.4".to_owned(), chooser);
        assert_eq!(dialog.focus_ring(), [Focus::Tree], "keys stay with the search field");
        dialog.query = "IO wri".to_owned();
        assert_eq!(dialog.rows(), [Row::Member(1)], "by every word, in the name or the package");
        dialog.query = String::new();
        run(&ctx, &mut dialog, vec![key(Key::ArrowDown, Modifiers::NONE), key(Key::ArrowDown, Modifiers::NONE)]);
        assert_eq!(dialog.cursor, 2);
        let outcome = run(&ctx, &mut dialog, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(
            outcome,
            Some(GenerateOutcome::Apply {
                id: "1.4".to_owned(),
                members: Some(vec!["c".to_owned()]),
                options: BTreeMap::new(),
            })
        );
        dialog.query = "nothing like it".to_owned();
        dialog.cursor = 0;
        assert_eq!(dialog.confirm(), None, "no match, nothing to pick");
    }

    #[test]
    fn the_list_applies_a_plain_generator_and_asks_for_members_first() {
        let ctx = egui::Context::default();
        let list = json!({ "pending": false, "generators": [
            { "id": "1.0", "title": "Constructor", "kind": null, "chooser": true },
            { "id": "1.1", "title": "Add test", "kind": "source.addTest", "chooser": false }
        ] });
        let mut popup = GeneratePopup {
            state: State::Shown {
                at: PopupAnchor::At(Pos2::ZERO),
                generators: generators(&list),
                selected: 0,
                reveal: true,
            },
        };
        let frame = |events: Vec<egui::Event>, popup: &mut GeneratePopup| {
            let mut out = None;
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                out = popup.keys(ui);
                let _ = popup.show(ui, &IdePalette::zen_dark(), CaretPlace::default());
            });
            output.textures_delta.clear();
            out
        };
        assert_eq!(popup.open_name(), Some("generate"));
        assert_eq!(frame(vec![key(Key::Enter, Modifiers::NONE)], &mut popup), None);
        assert_eq!(popup.open_name(), Some("memberChooser"), "a generator with a chooser asks for members");
        popup.state =
            State::Shown { at: PopupAnchor::At(Pos2::ZERO), generators: generators(&list), selected: 0, reveal: true };
        let applied = frame(vec![key(Key::ArrowDown, Modifiers::NONE), key(Key::Enter, Modifiers::NONE)], &mut popup);
        assert_eq!(
            applied,
            Some(GenerateOutcome::Apply { id: "1.1".to_owned(), members: None, options: BTreeMap::new() })
        );
        assert_eq!(popup.open_name(), None);
    }

    struct FakeList(Value);

    impl zen_core::Command for FakeList {
        type Params = Value;
        type Output = Value;
        const ID: &'static str = "editor.generate.list";
        const TITLE: &'static str = "List";
        const DESCRIPTION: &'static str = "List";

        fn run(&self, _: &mut Session, _: Value) -> Result<Value, zen_core::CommandError> {
            Ok(self.0.clone())
        }
    }

    struct FakeMembers(Value);

    impl zen_core::Command for FakeMembers {
        type Params = Value;
        type Output = Value;
        const ID: &'static str = "editor.generate.members";
        const TITLE: &'static str = "Members";
        const DESCRIPTION: &'static str = "Members";

        fn run(&self, _: &mut Session, params: Value) -> Result<Value, zen_core::CommandError> {
            Ok(json!({ "pending": false, "chooser": self.0[params["id"].as_str().unwrap_or_default()] }))
        }
    }

    #[test]
    fn override_and_implement_open_their_generators_chooser_directly() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let ctx = egui::Context::default();
        let list = json!({ "pending": false, "generators": [
            { "id": "1.0", "title": "Constructor", "chooser": true },
            { "id": "1.1", "title": "Override Methods", "chooser": true, "action": "override" },
            { "id": "1.2", "title": "Implement Methods", "chooser": true, "action": "implement" }
        ] });
        let chooser = |members: Vec<Member>| {
            serde_json::to_value(Chooser {
                title: "Select".to_owned(),
                members,
                options: Vec::new(),
                allow_empty: false,
                search: false,
            })
            .unwrap()
        };
        let mut registry = Registry::new();
        registry.register(FakeList(list)).unwrap();
        registry
            .register(FakeMembers(json!({
                "1.1": chooser(vec![member("methods.0", "toString()", "Object", false)]),
                "1.2": chooser(Vec::new())
            })))
            .unwrap();
        let mut session = Session::new();
        let mut follow = |popup: &mut GeneratePopup| {
            let mut out = None;
            let mut output =
                ctx.run_ui(egui::RawInput::default(), |ui| out = popup.follow(ui, &registry, &mut session));
            output.textures_delta.clear();
            out
        };
        let mut popup = GeneratePopup::default();
        popup.open(PopupAnchor::At(Pos2::ZERO), Some("override"));
        assert_eq!(popup.open_name(), Some("memberChooser"), "no Generate list in between");
        assert_eq!(follow(&mut popup), None);
        assert_eq!(follow(&mut popup), None);
        let State::Choosing(dialog) = &popup.state else { panic!("the chooser is open") };
        assert_eq!(dialog.id, "1.1");

        popup.open(PopupAnchor::At(Pos2::ZERO), Some("implement"));
        assert_eq!(follow(&mut popup), None);
        assert_eq!(follow(&mut popup), Some(GenerateOutcome::Notice(t!("ide-generate-nothing-to-implement"))));

        popup.open(PopupAnchor::At(Pos2::ZERO), Some("other"));
        assert_eq!(follow(&mut popup), Some(GenerateOutcome::Notice(t!("workbench-generate-none"))));
    }
}

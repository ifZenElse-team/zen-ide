use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use egui::{Key, Modifiers, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_workbench::{CharEdit, Workbench};

use super::IdePalette;
use super::caret_popup::{CaretPlace, PopupAnchor};
use crate::theme;

const ROW: f32 = 22.0;
const POLL: Duration = Duration::from_millis(50);

pub(super) const ACTIONS: [(&str, &str); 6] = [
    ("view.extract.variable", "refactor.extract.variable"),
    ("view.extract.constant", "refactor.extract.constant"),
    ("view.extract.field", "refactor.extract.field"),
    ("view.extract.parameter", "refactor.extract.parameter"),
    ("view.extract.method", "refactor.extract.method"),
    ("view.inline.open", "refactor.inline.apply"),
];

pub(super) fn command_of(action: &str) -> Option<&'static str> {
    ACTIONS.iter().find(|(id, _)| *id == action).map(|(_, command)| *command)
}

#[derive(Clone, Debug, PartialEq)]
struct Choice {
    title: String,
    range: Option<Range<usize>>,
    variant: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Naming {
    path: PathBuf,
    version: u64,
    selection_version: u64,
    ranges: Vec<Range<usize>>,
    primary: usize,
    original: String,
    suggestions: Vec<String>,
    selected: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    Asking { command: &'static str, params: Value },
    Choosing { command: &'static str, params: Value, choices: Vec<Choice>, selected: usize, version: u64, header: bool },
    Naming(Naming),
}

#[derive(Default)]
pub(super) struct ExtractFlow {
    state: Option<State>,
}

fn active(session: &Session) -> Option<(PathBuf, u64, u64)> {
    let document = session.state::<Workbench>()?.active_document()?;
    Some((document.path().to_owned(), document.version(), document.selection_version()))
}

fn failure(error: DispatchError) -> String {
    match error {
        DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

fn ranges(list: &Value) -> Vec<Range<usize>> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|range| Some(range["start"].as_u64()? as usize..range["end"].as_u64()? as usize))
        .collect()
}

fn mapped(range: &Range<usize>, edit: &CharEdit) -> Option<Range<usize>> {
    let grow = |offset: usize| offset - edit.range.len() + edit.inserted;
    if edit.range.end <= range.start && edit.range.start < range.start {
        Some(grow(range.start)..grow(range.end))
    } else if edit.range.start >= range.start && edit.range.end <= range.end {
        Some(range.start..grow(range.end))
    } else if edit.range.start >= range.end {
        Some(range.clone())
    } else {
        None
    }
}

impl Naming {
    fn name(&self, session: &Session) -> String {
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else {
            return String::new();
        };
        let range = &self.ranges[self.primary];
        document.text().chars().skip(range.start).take(range.len()).collect()
    }

    fn shown(&self, session: &Session) -> Vec<String> {
        if self.suggestions.len() < 2 {
            return Vec::new();
        }
        let typed = self.name(session);
        if typed == self.original {
            return self.suggestions.clone();
        }
        let typed = typed.to_lowercase();
        self.suggestions
            .iter()
            .filter(|name| name.to_lowercase().starts_with(&typed) && **name != typed)
            .cloned()
            .collect()
    }

    fn follow(&mut self, session: &Session) -> bool {
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return false };
        if document.path() != self.path {
            return false;
        }
        if document.version() != self.version {
            let Some(edits) = document.char_edits_since(self.version) else { return false };
            for edit in &edits {
                if !self.ranges.iter().any(|range| edit.range.start >= range.start && edit.range.end <= range.end) {
                    return false;
                }
                let Some(next) = self.ranges.iter().map(|range| mapped(range, edit)).collect::<Option<Vec<_>>>() else {
                    return false;
                };
                self.ranges = next;
            }
            self.version = document.version();
        }
        if document.selection_version() != self.selection_version {
            let selections = document.selections();
            let inside = selections.len() == self.ranges.len()
                && selections.iter().all(|selection| {
                    let range = selection.range();
                    self.ranges.iter().any(|own| range.start >= own.start && range.end <= own.end)
                });
            if !inside {
                return false;
            }
            self.selection_version = document.selection_version();
        }
        true
    }
}

impl ExtractFlow {
    pub(super) fn open(&mut self, command: &'static str) {
        self.state = Some(State::Asking { command, params: json!({ "waitMs": 0 }) });
    }

    pub(super) fn is_open(&self) -> bool {
        self.state.is_some()
    }

    pub(super) fn open_name(&self) -> Option<&'static str> {
        match self.state.as_ref()? {
            State::Asking { .. } => None,
            State::Choosing { header: true, .. } => Some("extractExpressions"),
            State::Choosing { .. } => Some("extractVariants"),
            State::Naming(_) => Some("inPlaceName"),
        }
    }

    pub(super) fn naming(&self) -> bool {
        matches!(self.state, Some(State::Naming(_)))
    }

    #[cfg(test)]
    pub(super) fn suggested(&self, session: &Session) -> Vec<String> {
        match &self.state {
            Some(State::Naming(naming)) => naming.shown(session),
            _ => Vec::new(),
        }
    }

    pub(super) fn highlighted(&self) -> Vec<Range<usize>> {
        match &self.state {
            Some(State::Choosing { choices, selected, .. }) => choices[*selected].range.iter().cloned().collect(),
            Some(State::Naming(naming)) => naming.ranges.clone(),
            _ => Vec::new(),
        }
    }

    pub(super) fn follow(&mut self, ui: &Ui, registry: &Registry, session: &mut Session) -> Option<String> {
        match self.state.take()? {
            State::Asking { command, params } => self.ask(ui, registry, session, command, params),
            State::Choosing { version, .. } if active(session).is_none_or(|(_, now, _)| now != version) => None,
            State::Naming(mut naming) => {
                if naming.follow(session) {
                    self.state = Some(State::Naming(naming));
                }
                None
            }
            other => {
                self.state = Some(other);
                None
            }
        }
    }

    fn ask(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        command: &'static str,
        mut params: Value,
    ) -> Option<String> {
        let answer = match registry.dispatch(session, command, params.clone()) {
            Ok(answer) => answer,
            Err(error) => return Some(failure(error)),
        };
        if answer["pending"] == true {
            params["start"] = answer["start"].clone();
            params["end"] = answer["end"].clone();
            self.state = Some(State::Asking { command, params });
            ui.ctx().request_repaint_after(POLL);
            return None;
        }
        let version = active(session).map_or(0, |(_, version, _)| version);
        let expressions = ranges(&answer["expressions"]);
        if !expressions.is_empty() {
            let choices = answer["expressions"]
                .as_array()
                .into_iter()
                .flatten()
                .zip(expressions)
                .map(|(expression, range)| Choice {
                    title: expression["text"].as_str().unwrap_or_default().to_owned(),
                    range: Some(range),
                    variant: None,
                })
                .collect();
            self.state = Some(State::Choosing { command, params, choices, selected: 0, version, header: true });
            return None;
        }
        if let Some(variants) = answer["variants"].as_array().filter(|variants| !variants.is_empty()) {
            params["start"] = answer["start"].clone();
            params["end"] = answer["end"].clone();
            let choices = variants
                .iter()
                .enumerate()
                .map(|(index, title)| Choice {
                    title: title.as_str().unwrap_or_default().to_owned(),
                    range: None,
                    variant: Some(index),
                })
                .collect();
            self.state = Some(State::Choosing { command, params, choices, selected: 0, version, header: false });
            return None;
        }
        self.start_naming(registry, session, &answer);
        None
    }

    fn start_naming(&mut self, registry: &Registry, session: &mut Session, answer: &Value) {
        let occurrences = ranges(&answer["occurrences"]);
        let (Some(name), Some(path)) = (answer["name"].as_str(), answer["path"].as_str()) else { return };
        if occurrences.is_empty() || active(session).is_none_or(|(active, _, _)| active != std::path::Path::new(path)) {
            return;
        }
        let near = answer["start"].as_u64().unwrap_or_default() as usize;
        let primary = (0..occurrences.len()).min_by_key(|&index| occurrences[index].start.abs_diff(near)).unwrap_or(0);
        let selections: Vec<Value> =
            occurrences.iter().map(|range| json!({ "anchor": range.start, "head": range.end })).collect();
        if registry
            .dispatch(session, "editor.selection.set", json!({ "selections": selections, "primary": primary }))
            .is_err()
        {
            return;
        }
        let Some((path, version, selection_version)) = active(session) else { return };
        let suggestions = answer["suggestions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|name| name.as_str().map(str::to_owned))
            .collect();
        self.state = Some(State::Naming(Naming {
            path,
            version,
            selection_version,
            ranges: occurrences,
            primary,
            original: name.to_owned(),
            suggestions,
            selected: 0,
        }));
    }

    pub(super) fn keys(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) -> Option<String> {
        let naming = self.naming();
        let shown = match &self.state {
            Some(State::Choosing { choices, .. }) => choices.len(),
            Some(State::Naming(naming)) => naming.shown(session).len(),
            _ => return None,
        };
        let (up, down, enter, escape) = ui.input_mut(|input| {
            let arrows = shown > 0;
            (
                arrows && input.consume_key(Modifiers::NONE, Key::ArrowUp),
                arrows && input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        let selected = match &mut self.state {
            Some(State::Choosing { selected, .. }) => selected,
            Some(State::Naming(naming)) => &mut naming.selected,
            _ => return None,
        };
        if shown > 0 {
            *selected = (*selected).min(shown - 1);
            if up {
                *selected = selected.checked_sub(1).unwrap_or(shown - 1);
            }
            if down {
                *selected = (*selected + 1) % shown;
            }
        }
        let index = *selected;
        if enter {
            return if naming { self.finish(registry, session, shown > 0) } else { self.choose(index) };
        }
        if escape {
            return if naming { self.finish(registry, session, false) } else { self.close() };
        }
        None
    }

    fn close(&mut self) -> Option<String> {
        self.state = None;
        None
    }

    fn choose(&mut self, index: usize) -> Option<String> {
        let Some(State::Choosing { command, mut params, choices, .. }) = self.state.take() else { return None };
        let choice = choices.get(index)?;
        if let Some(range) = &choice.range {
            params["start"] = json!(range.start);
            params["end"] = json!(range.end);
        }
        if let Some(variant) = choice.variant {
            params["variant"] = json!(variant);
        }
        self.state = Some(State::Asking { command, params });
        None
    }

    fn finish(&mut self, registry: &Registry, session: &mut Session, take_suggestion: bool) -> Option<String> {
        let Some(State::Naming(naming)) = self.state.take() else { return None };
        let current = naming.name(session);
        let chosen = take_suggestion.then(|| naming.shown(session).get(naming.selected).cloned()).flatten();
        let mut end = naming.ranges[naming.primary].end;
        if let Some(chosen) = chosen.filter(|chosen| *chosen != current) {
            let selections: Vec<Value> =
                naming.ranges.iter().map(|range| json!({ "anchor": range.start, "head": range.end })).collect();
            let params = json!({ "selections": selections, "primary": naming.primary });
            if let Err(error) = registry
                .dispatch(session, "editor.selection.set", params)
                .and_then(|_| registry.dispatch(session, "editor.edit.insert", json!({ "text": chosen })))
            {
                return Some(failure(error));
            }
            let grown = chosen.chars().count() as isize - current.chars().count() as isize;
            end = (naming.ranges[naming.primary].start as isize + grown * naming.primary as isize) as usize
                + chosen.chars().count();
        }
        let caret = json!({ "selections": [{ "head": end }] });
        registry.dispatch(session, "editor.selection.set", caret).err().map(failure)
    }

    pub(super) fn show(
        &mut self,
        ui: &Ui,
        session: &Session,
        palette: &IdePalette,
        caret: CaretPlace,
    ) -> Option<usize> {
        let (titles, selected, header) = match &self.state {
            Some(State::Choosing { choices, selected, header, .. }) => {
                (choices.iter().map(|choice| choice.title.clone()).collect(), *selected, *header)
            }
            Some(State::Naming(naming)) => (naming.shown(session), naming.selected, false),
            _ => return None,
        };
        if titles.is_empty() {
            return None;
        }
        let area = egui::Area::new(ui.scope_id().with("extract-popup")).order(egui::Order::Foreground);
        let area = PopupAnchor::UNDER_CARET.area(ui.ctx(), area, caret)?;
        let mut chosen = None;
        let shown = area.show(ui.ctx(), |ui| {
            palette.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let font = theme::regular(13.0);
                if header {
                    let galley = ui.painter().layout_no_wrap(
                        t!("ide-extract-expressions"),
                        font.clone(),
                        palette.text_secondary,
                    );
                    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 20.0, ROW), Sense::hover());
                    ui.painter().galley(
                        pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
                        galley,
                        palette.text_secondary,
                    );
                }
                let widest = titles
                    .iter()
                    .map(|title| ui.painter().layout_no_wrap(title.clone(), font.clone(), palette.text).size().x)
                    .fold(0.0_f32, f32::max);
                let width = (widest + 24.0).clamp(160.0, 560.0);
                for (index, title) in titles.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                    if index == selected {
                        ui.painter().rect_filled(rect, 4, palette.selection);
                    } else if response.hovered() {
                        ui.painter().rect_filled(rect, 4, palette.popup_hover);
                    }
                    crate::accessibility::item(&response, egui::Role::ListBoxOption, index == selected, title);
                    let galley = ui.painter().layout_no_wrap(title.clone(), font.clone(), palette.text);
                    ui.painter().galley(
                        pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
                        galley,
                        palette.text,
                    );
                    if response.clicked() {
                        chosen = Some(index);
                    }
                }
            });
        });
        let pressed_outside = ui.input(|input| {
            input.pointer.any_pressed()
                && input.pointer.interact_pos().is_some_and(|pos| !shown.response.rect.contains(pos))
        });
        if pressed_outside && chosen.is_none() && !self.naming() {
            self.state = None;
        }
        chosen
    }

    pub(super) fn clicked(&mut self, registry: &Registry, session: &mut Session, index: usize) -> Option<String> {
        match &mut self.state {
            Some(State::Naming(naming)) => {
                naming.selected = index;
                self.finish(registry, session, true)
            }
            Some(State::Choosing { .. }) => self.choose(index),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_in_an_occurrence_moves_the_ones_after_it() {
        let edit = |start: usize, end: usize, inserted: usize| CharEdit { range: start..end, inserted };
        assert_eq!(mapped(&(10..11), &edit(10, 11, 3)), Some(10..13), "the name replaced");
        assert_eq!(mapped(&(10..11), &edit(11, 11, 1)), Some(10..12), "typed at its end");
        assert_eq!(mapped(&(10..11), &edit(10, 10, 1)), Some(10..12), "typed at its start");
        assert_eq!(mapped(&(20..21), &edit(10, 11, 3)), Some(22..23), "an earlier one grew");
        assert_eq!(mapped(&(20..23), &edit(10, 13, 1)), Some(18..21), "an earlier one shrank");
        assert_eq!(mapped(&(5..6), &edit(10, 11, 3)), Some(5..6), "a later one");
        assert_eq!(mapped(&(10..12), &edit(9, 11, 0)), None, "a change across its edge ends the naming");
    }
}

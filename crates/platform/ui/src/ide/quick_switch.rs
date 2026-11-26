use egui::{Align2, Event, Key, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;

use super::IdePalette;
use super::settings::{CodeStyleScheme, IDE_ZOOM_PRESETS};
use crate::{accessibility, icons, theme};

const ROW: f32 = 24.0;
const MARGIN: f32 = 6.0;
const TITLE: f32 = 30.0;
const SEARCH: f32 = 32.0;
const SEPARATOR: f32 = 9.0;
const MARKER: f32 = 28.0;
const NUMBER: f32 = 18.0;
const TEXT: f32 = 13.0;
const MIN_WIDTH: f32 = 160.0;

pub(super) const VIEW_MODES: [&str; 4] = [
    "view.presentationMode.toggle",
    "view.distractionFreeMode.toggle",
    "view.fullScreen.toggle",
    "view.zenMode.toggle",
];

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Run {
    Command(&'static str, Value),
    Action(&'static str, Value),
    CodeStyleScheme(CodeStyleScheme),
    IdeZoom(u16),
}

impl Run {
    fn to_json(&self) -> Value {
        match self {
            Run::Command(command, params) => json!({ "command": command, "params": params }),
            Run::Action(action, params) => json!({ "action": action, "params": params }),
            Run::CodeStyleScheme(scheme) => json!({ "settings": { "codeStyleScheme": scheme.id() } }),
            Run::IdeZoom(percent) => json!({ "settings": { "ideZoom": percent } }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Marker {
    Arrow,
    Selected,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Choice {
    label: String,
    suffix: Option<String>,
    current: Option<bool>,
    separator: bool,
    run: Run,
}

impl Choice {
    fn new(label: String, current: Option<bool>, run: Run) -> Self {
        Choice { label, suffix: None, current, separator: false, run }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Group {
    id: &'static str,
    title: String,
    choices: Vec<Choice>,
    marker: Marker,
    numbered: bool,
}

pub(super) struct Sources<'a> {
    pub(super) themes: Option<&'a Value>,
    pub(super) keymaps: Option<&'a Value>,
    pub(super) code_style_scheme: CodeStyleScheme,
    pub(super) view_modes: Vec<(&'static str, String)>,
    pub(super) ide_zoom: u16,
}

pub(super) fn groups(sources: &Sources<'_>) -> Vec<Group> {
    let themes = theme_choices(sources.themes);
    let schemes = themes
        .iter()
        .map(|theme| Choice {
            suffix: (theme.current == Some(true)).then(|| t!("ide-quick-switch-theme-default")),
            separator: false,
            ..theme.clone()
        })
        .collect();
    let code_styles = [
        (CodeStyleScheme::Project, t!("ide-quick-switch-code-style-project")),
        (CodeStyleScheme::Default, t!("ide-quick-switch-code-style-default")),
    ]
    .into_iter()
    .map(|(scheme, label)| Choice::new(label, Some(scheme == sources.code_style_scheme), Run::CodeStyleScheme(scheme)))
    .collect();
    let mut keymaps: Vec<Choice> = listed(sources.keymaps, "keymaps")
        .map(|keymap| {
            Choice::new(keymap.name, Some(keymap.current), Run::Command("ui.keymap.select", json!({ "id": keymap.id })))
        })
        .collect();
    keymaps.push(Choice {
        separator: !keymaps.is_empty(),
        ..Choice::new(
            t!("ide-quick-switch-configure-keymap"),
            None,
            Run::Action("view.settings.open", json!({ "page": "keymap" })),
        )
    });
    let modes = sources
        .view_modes
        .iter()
        .map(|(action, title)| Choice::new(title.clone(), None, Run::Action(action, json!({}))))
        .collect();
    let zooms = IDE_ZOOM_PRESETS
        .iter()
        .map(|percent| Choice::new(format!("{percent}%"), Some(*percent == sources.ide_zoom), Run::IdeZoom(*percent)))
        .collect();
    let group = |id, title: &str, choices, marker, numbered| Group { id, title: t!(title), choices, marker, numbered };
    vec![
        group("theme", "ide-quick-switch-theme", themes, Marker::Selected, true),
        group("editorColorScheme", "ide-quick-switch-color-scheme", schemes, Marker::Arrow, true),
        group("codeStyleScheme", "ide-quick-switch-code-style", code_styles, Marker::Arrow, true),
        group("keymap", "ide-quick-switch-keymap", keymaps, Marker::Arrow, true),
        group("viewMode", "ide-quick-switch-view-mode", modes, Marker::None, true),
        group("zoom", "ide-quick-switch-zoom", zooms, Marker::Selected, false),
    ]
}

fn theme_choices(list: Option<&Value>) -> Vec<Choice> {
    let mut themes: Vec<Listed> = listed(list, "themes").collect();
    themes.sort_by_key(|theme| theme.user);
    let first_user = themes.iter().position(|theme| theme.user).filter(|at| *at > 0);
    themes
        .into_iter()
        .enumerate()
        .map(|(index, theme)| Choice {
            separator: Some(index) == first_user,
            ..Choice::new(theme.name, Some(theme.current), Run::Command("ui.theme.select", json!({ "id": theme.id })))
        })
        .collect()
}

struct Listed {
    id: String,
    name: String,
    current: bool,
    user: bool,
}

fn listed<'a>(list: Option<&'a Value>, key: &str) -> impl Iterator<Item = Listed> + 'a {
    let current = list.and_then(|list| list["current"].as_str()).unwrap_or_default();
    let entries = list.and_then(|list| list[key].as_array()).into_iter().flatten();
    entries.filter(|entry| entry.get("error").is_none_or(Value::is_null)).filter_map(move |entry| {
        let id = entry["id"].as_str()?.to_owned();
        let name = entry["name"].as_str().filter(|name| !name.is_empty()).unwrap_or(&id).to_owned();
        let user = entry["source"].as_str() == Some("user");
        Some(Listed { current: id == current, id, name, user })
    })
}

#[derive(Clone, Debug)]
struct Shown {
    groups: Vec<Group>,
    group: Option<usize>,
    selected: usize,
    filter: String,
}

impl Shown {
    fn labels(&self) -> Vec<&str> {
        match self.group.and_then(|group| self.groups.get(group)) {
            Some(group) => group.choices.iter().map(|choice| choice.label.as_str()).collect(),
            None => self.groups.iter().map(|group| group.title.as_str()).collect(),
        }
    }

    fn numbered(&self) -> bool {
        self.group.and_then(|group| self.groups.get(group)).is_none_or(|group| group.numbered)
    }

    fn visible(&self) -> Vec<usize> {
        let forms: Vec<String> = zen_index::layout::forms(&self.filter.to_lowercase());
        self.labels()
            .iter()
            .enumerate()
            .filter(|(_, label)| {
                let label = label.to_lowercase();
                forms.iter().any(|form| label.contains(form.as_str()))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn filter_changed(&mut self) {
        let visible = self.visible();
        if !visible.contains(&self.selected) {
            self.selected = visible.first().copied().unwrap_or(0);
        }
    }
}

fn first_row(group: &Group) -> usize {
    let current = group.choices.iter().position(|choice| choice.current == Some(true));
    match (group.marker, current) {
        (Marker::Arrow, Some(at)) => (at + 1) % group.choices.len(),
        (Marker::Selected, Some(at)) => at,
        _ => 0,
    }
}

enum Input {
    Key(Key),
    Text(String),
}

#[derive(Default)]
pub(super) struct QuickSwitch {
    shown: Option<Shown>,
}

impl QuickSwitch {
    pub(super) fn is_open(&self) -> bool {
        self.shown.is_some()
    }

    pub(super) fn open(&mut self, groups: Vec<Group>) {
        self.shown = Some(Shown { groups, group: None, selected: 0, filter: String::new() });
    }

    pub(super) fn to_json(&self) -> Value {
        let Some(shown) = &self.shown else { return Value::Null };
        let groups: Vec<Value> = shown
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let choices: Vec<Value> = group
                    .choices
                    .iter()
                    .enumerate()
                    .map(|(row, choice)| {
                        let number = (group.numbered && row < 10).then_some((row + 1) % 10);
                        json!({
                            "number": number,
                            "label": choice.label,
                            "suffix": choice.suffix,
                            "current": choice.current,
                            "runs": choice.run.to_json(),
                        })
                    })
                    .collect();
                json!({ "number": index + 1, "id": group.id, "title": group.title, "choices": choices })
            })
            .collect();
        let group = shown.group.map(|group| shown.groups[group].id);
        let selected = shown.visible().contains(&shown.selected).then_some(shown.selected);
        json!({ "groups": groups, "group": group, "selected": selected, "filter": shown.filter })
    }

    fn activate(&mut self, index: usize) -> Option<Run> {
        let shown = self.shown.as_mut()?;
        match shown.group {
            None => {
                let group = shown.groups.get(index).filter(|group| !group.choices.is_empty())?;
                shown.selected = first_row(group);
                shown.group = Some(index);
                shown.filter.clear();
                None
            }
            Some(group) => {
                let run = shown.groups.get(group)?.choices.get(index)?.run.clone();
                self.shown = None;
                Some(run)
            }
        }
    }

    fn press(&mut self, key: Key) -> Option<Run> {
        let shown = self.shown.as_mut()?;
        let visible = shown.visible();
        match key {
            Key::Escape if !shown.filter.is_empty() => {
                shown.filter.clear();
                shown.filter_changed();
            }
            Key::Escape => self.shown = None,
            Key::Backspace => {
                shown.filter.pop();
                shown.filter_changed();
            }
            Key::ArrowDown | Key::ArrowUp if !visible.is_empty() => {
                let at = visible.iter().position(|row| *row == shown.selected).unwrap_or(0);
                let count = visible.len();
                let next = if key == Key::ArrowDown { (at + 1) % count } else { (at + count - 1) % count };
                shown.selected = visible[next];
            }
            Key::Enter if visible.contains(&shown.selected) => {
                let row = shown.selected;
                return self.activate(row);
            }
            _ => {
                let row = digit(key).filter(|_| shown.filter.is_empty() && shown.numbered());
                if let Some(row) = row.filter(|row| *row < visible.len()) {
                    return self.activate(row);
                }
            }
        }
        None
    }

    fn type_text(&mut self, text: &str) {
        let Some(shown) = self.shown.as_mut() else { return };
        let typed: String = text.chars().filter(|c| !c.is_control() && !c.is_ascii_digit()).collect();
        if !typed.is_empty() {
            shown.filter.push_str(&typed);
            shown.filter_changed();
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette) -> Option<Run> {
        self.shown.as_ref()?;
        let mut inputs = Vec::new();
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Key { key, pressed, .. } => {
                    if *pressed {
                        inputs.push(Input::Key(*key));
                    }
                    false
                }
                Event::Text(text) => {
                    inputs.push(Input::Text(text.clone()));
                    false
                }
                Event::Paste(_) | Event::Copy | Event::Cut | Event::Ime(_) => false,
                _ => true,
            });
        });
        for input in inputs {
            match input {
                Input::Key(key) => {
                    if let Some(run) = self.press(key) {
                        return Some(run);
                    }
                }
                Input::Text(text) => self.type_text(&text),
            }
            self.shown.as_ref()?;
        }
        let shown = self.shown.as_mut()?;
        let screen = ui.ctx().content_rect();
        let moved = ui.input(|input| input.pointer.delta() != Vec2::ZERO);
        let pointer = ui.input(|input| input.pointer.interact_pos());
        let pressed = ui.input(|input| input.pointer.any_pressed());

        let group = shown.group.and_then(|group| shown.groups.get(group)).cloned();
        let title = group.as_ref().map_or_else(|| t!("ide-quick-switch-title"), |group| group.title.clone());
        let arrow = group.as_ref().is_some_and(|group| group.marker == Marker::Arrow);
        let numbered = shown.numbered();
        let suffixes: Vec<Option<String>> = match &group {
            Some(group) => group.choices.iter().map(|choice| choice.suffix.clone()).collect(),
            None => vec![None; shown.groups.len()],
        };
        let separators: Vec<bool> = match &group {
            Some(group) => group.choices.iter().map(|choice| choice.separator).collect(),
            None => vec![false; shown.groups.len()],
        };
        let labels: Vec<String> = shown.labels().into_iter().map(str::to_owned).collect();
        let lead = 8.0 + if arrow { MARKER } else { 0.0 } + if numbered { NUMBER } else { 0.0 };
        let measure = |text: &str, font| ui.painter().layout_no_wrap(text.to_owned(), font, p.text).size().x;
        let widest = labels
            .iter()
            .zip(&suffixes)
            .map(|(label, suffix)| {
                measure(label, theme::regular(TEXT))
                    + suffix.as_ref().map_or(0.0, |suffix| 6.0 + measure(suffix, theme::regular(TEXT)))
            })
            .fold(0.0, f32::max);
        let width =
            (lead + widest + 16.0).max(measure(&title, theme::semibold(TEXT)) + 32.0).max(MIN_WIDTH) + MARGIN * 2.0;
        let rows_height =
            ROW * labels.len() as f32 + SEPARATOR * separators.iter().filter(|separator| **separator).count() as f32;
        let height = TITLE + rows_height + MARGIN * 2.0;
        let at = pos2(screen.center().x - width / 2.0, screen.center().y - height / 2.0 - ROW * 2.0)
            .max(screen.min + vec2(8.0, 8.0));
        let visible = shown.visible();
        let filtering = !shown.filter.is_empty();
        let mut hovered = None;
        let mut clicked = None;
        let mut cleared = false;
        let inner = width - MARGIN * 2.0;
        let area = egui::Area::new(ui.scope_id().with("quick-switch"))
            .order(egui::Order::Foreground)
            .fixed_pos(at)
            .show(ui.ctx(), |ui| {
                frame(p).show(ui, |ui| {
                    ui.set_width(inner);
                    ui.set_min_height(height - MARGIN * 2.0 + if filtering { SEARCH } else { 0.0 });
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (heading, _) = ui.allocate_exact_size(vec2(inner, TITLE), Sense::hover());
                    ui.painter().text(heading.center(), Align2::CENTER_CENTER, &title, theme::semibold(TEXT), p.text);
                    accessibility::heading(ui, ui.scope_id().with("quick-switch-title"), heading, || title.clone());
                    if filtering {
                        let (field, response) = ui.allocate_exact_size(vec2(inner, SEARCH), Sense::hover());
                        accessibility::name(&response, egui::Role::SearchInput, &t!("ide-quick-switch-search"));
                        accessibility::edit(&response, |node| node.set_value(shown.filter.clone()));
                        let glass =
                            Rect::from_center_size(pos2(field.left() + 18.0, field.center().y), vec2(16.0, 16.0));
                        icons::search(ui.painter(), glass, p.text_secondary);
                        ui.painter().text(
                            pos2(field.left() + 32.0, field.center().y),
                            Align2::LEFT_CENTER,
                            &shown.filter,
                            theme::regular(TEXT),
                            p.text,
                        );
                        let cross =
                            Rect::from_center_size(pos2(field.right() - 18.0, field.center().y), vec2(16.0, 16.0));
                        let clear = ui.interact(cross, ui.scope_id().with("quick-switch-clear"), Sense::click());
                        accessibility::button(&clear, &t!("ide-quick-switch-clear"));
                        cross_mark(ui.painter(), cross.shrink(4.0), p.text_secondary);
                        cleared = clear.clicked();
                        let line = field.bottom() - 2.0;
                        ui.painter().hline(field.x_range().shrink(6.0), line, egui::Stroke::new(1.0, p.popup_border));
                    }
                    for index in visible.iter().copied() {
                        if separators[index] && !filtering {
                            let (gap, _) = ui.allocate_exact_size(vec2(inner, SEPARATOR), Sense::hover());
                            ui.painter().hline(gap.x_range(), gap.center().y, egui::Stroke::new(1.0, p.popup_border));
                        }
                        let (rect, response) = ui.allocate_exact_size(vec2(inner, ROW), Sense::click());
                        let selected = index == shown.selected;
                        let current = group.as_ref().and_then(|group| group.choices[index].current);
                        let role = if current.is_some() { egui::Role::MenuItemRadio } else { egui::Role::MenuItem };
                        accessibility::item(&response, role, selected, &labels[index]);
                        if let Some(current) = current {
                            let toggled =
                                if current { egui::accesskit::Toggled::True } else { egui::accesskit::Toggled::False };
                            accessibility::edit(&response, |node| node.set_toggled(toggled));
                        }
                        if let Some(suffix) = &suffixes[index] {
                            accessibility::describe(&response, suffix);
                        }
                        if selected {
                            ui.painter().rect_filled(rect, 5, p.selection);
                        }
                        let mut x = rect.left() + 8.0;
                        if arrow {
                            if current == Some(true) {
                                let mark = Rect::from_center_size(pos2(x + 8.0, rect.center().y), vec2(12.0, 12.0));
                                arrow_mark(ui.painter(), mark, p.text);
                            }
                            x += MARKER;
                        }
                        if numbered {
                            number(ui, x, rect, index, p);
                            x += NUMBER;
                        }
                        let label = ui.painter().text(
                            pos2(x, rect.center().y),
                            Align2::LEFT_CENTER,
                            &labels[index],
                            theme::regular(TEXT),
                            p.text,
                        );
                        if let Some(suffix) = &suffixes[index] {
                            ui.painter().text(
                                pos2(label.right() + 6.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                suffix,
                                theme::regular(TEXT),
                                p.text_secondary,
                            );
                        }
                        if response.clicked() {
                            clicked = Some(index);
                        } else if moved && response.hovered() {
                            hovered = Some(index);
                        }
                    }
                });
            });
        if cleared {
            shown.filter.clear();
            shown.filter_changed();
        }
        if let Some(index) = hovered {
            shown.selected = index;
        }
        if let Some(index) = clicked {
            return self.activate(index);
        }
        if pressed && pointer.is_some_and(|pointer| !area.response.rect.contains(pointer)) {
            self.shown = None;
        }
        None
    }
}

fn frame(p: &IdePalette) -> egui::Frame {
    p.popup_frame().corner_radius(8).inner_margin(egui::Margin::same(MARGIN as i8)).shadow(egui::Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: p.popup_shadow,
    })
}

fn number(ui: &Ui, x: f32, rect: Rect, index: usize, p: &IdePalette) {
    if index < 10 {
        let text = ((index + 1) % 10).to_string();
        ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, text, theme::regular(TEXT), p.text_secondary);
    }
}

fn arrow_mark(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.2, color);
    let (left, right, middle) = (rect.left(), rect.right(), rect.center().y);
    painter.line_segment([pos2(left, middle), pos2(right, middle)], stroke);
    let head = rect.height() * 0.4;
    painter.add(egui::Shape::line(
        vec![pos2(right - head, middle - head), pos2(right, middle), pos2(right - head, middle + head)],
        stroke,
    ));
}

fn cross_mark(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let stroke = egui::Stroke::new(1.2, color);
    painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_top(), rect.left_bottom()], stroke);
}

fn digit(key: Key) -> Option<usize> {
    let digits =
        [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9, Key::Num0];
    digits.iter().position(|digit| *digit == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sources() -> (Value, Value) {
        let themes = json!({
            "current": "zen-dark",
            "themes": [
                { "id": "dusk", "name": "Dusk", "dark": true, "source": "user" },
                { "id": "zen-dark", "name": "Zen Dark", "dark": true, "source": "builtin" },
                { "id": "broken", "name": "Broken", "dark": true, "source": "user", "error": "bad" },
                { "id": "zen-light", "name": "Zen Light", "dark": false, "source": "builtin" },
            ],
        });
        let keymaps = json!({
            "current": "mine",
            "keymaps": [
                { "id": "macos", "name": "macOS", "source": "builtin" },
                { "id": "mine", "name": "Mine", "source": "user" },
            ],
        });
        (themes, keymaps)
    }

    fn open_popup() -> QuickSwitch {
        let (themes, keymaps) = sources();
        let modes = VIEW_MODES.iter().map(|action| (*action, format!("Enter {action}"))).collect();
        let groups = groups(&Sources {
            themes: Some(&themes),
            keymaps: Some(&keymaps),
            code_style_scheme: CodeStyleScheme::Default,
            view_modes: modes,
            ide_zoom: 110,
        });
        let mut popup = QuickSwitch::default();
        popup.open(groups);
        popup
    }

    fn selected(popup: &QuickSwitch) -> (Value, Value) {
        let json = popup.to_json();
        (json["group"].clone(), json["selected"].clone())
    }

    #[test]
    fn the_rows_keep_their_order_and_mark_what_is_in_use() {
        let popup = open_popup();
        let json = popup.to_json();
        let ids: Vec<&str> = json["groups"].as_array().unwrap().iter().map(|g| g["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["theme", "editorColorScheme", "codeStyleScheme", "keymap", "viewMode", "zoom"]);
        let labels = |group: usize| -> Vec<(String, Value)> {
            json["groups"][group]["choices"]
                .as_array()
                .unwrap()
                .iter()
                .map(|choice| (choice["label"].as_str().unwrap().to_owned(), choice["current"].clone()))
                .collect()
        };
        let themes = vec![
            ("Zen Dark".to_owned(), json!(true)),
            ("Zen Light".to_owned(), json!(false)),
            ("Dusk".to_owned(), json!(false)),
        ];
        assert_eq!(labels(0), themes, "built-in themes first, a broken one left out");
        assert_eq!(labels(1), themes);
        assert_eq!(json["groups"][1]["choices"][0]["suffix"], t!("ide-quick-switch-theme-default"));
        assert_eq!(json["groups"][1]["choices"][1]["suffix"], Value::Null);
        assert_eq!(json["groups"][0]["choices"][0]["suffix"], Value::Null, "Theme has no suffix");
        assert_eq!(
            labels(2),
            [
                (t!("ide-quick-switch-code-style-project"), json!(false)),
                (t!("ide-quick-switch-code-style-default"), json!(true))
            ]
        );
        assert_eq!(
            labels(3),
            [
                ("macOS".to_owned(), json!(false)),
                ("Mine".to_owned(), json!(true)),
                (t!("ide-quick-switch-configure-keymap"), Value::Null)
            ]
        );
        assert_eq!(labels(4).len(), 4);
        assert!(labels(4).iter().all(|(_, current)| current.is_null()), "View Mode reads Enter or Exit instead");
        assert_eq!(
            labels(5).iter().map(|(label, _)| label.as_str()).collect::<Vec<_>>()[..4],
            ["70%", "80%", "90%", "100%"]
        );
        assert_eq!(labels(5)[4], ("110%".to_owned(), json!(true)));
        assert_eq!(json["groups"][5]["choices"][0]["number"], Value::Null, "Zoom has no digits");
        assert_eq!(json["groups"][3]["choices"][1]["number"], 2);
        assert_eq!(
            json["groups"][3]["choices"][0]["runs"],
            json!({ "command": "ui.keymap.select", "params": { "id": "macos" } })
        );
        assert_eq!(
            json["groups"][3]["choices"][2]["runs"],
            json!({ "action": "view.settings.open", "params": { "page": "keymap" } })
        );
        assert_eq!(json["groups"][2]["choices"][0]["runs"], json!({ "settings": { "codeStyleScheme": "project" } }));
        assert_eq!(
            json["groups"][4]["choices"][2]["runs"],
            json!({ "action": "view.fullScreen.toggle", "params": {} })
        );
        assert_eq!(json["groups"][5]["choices"][8]["runs"], json!({ "settings": { "ideZoom": 200 } }));
        assert_eq!(selected(&popup), (Value::Null, json!(0)), "the first row selected");
    }

    #[test]
    fn enter_replaces_the_popup_by_the_list_selected_after_the_choice_in_use() {
        let mut popup = open_popup();
        assert_eq!(popup.press(Key::ArrowRight), None);
        assert_eq!(selected(&popup), (Value::Null, json!(0)), "→ does nothing");
        assert_eq!(popup.press(Key::ArrowDown), None);
        assert_eq!(popup.press(Key::Enter), None);
        assert_eq!(selected(&popup), (json!("editorColorScheme"), json!(1)), "the row after the scheme in use");
        assert_eq!(popup.press(Key::ArrowLeft), None);
        assert_eq!(selected(&popup), (json!("editorColorScheme"), json!(1)), "← does nothing");
        assert_eq!(popup.press(Key::Enter), Some(Run::Command("ui.theme.select", json!({ "id": "zen-light" }))));
        assert!(!popup.is_open(), "a choice closes the popup");

        let mut popup = open_popup();
        assert_eq!(popup.press(Key::Num3), None);
        assert_eq!(selected(&popup), (json!("codeStyleScheme"), json!(0)), "after the last row, the first");
        let mut popup = open_popup();
        assert_eq!(popup.press(Key::Num1), None);
        assert_eq!(selected(&popup), (json!("theme"), json!(0)), "Theme selects the theme in use");
        let mut popup = open_popup();
        assert_eq!(popup.press(Key::Num6), None);
        assert_eq!(selected(&popup), (json!("zoom"), json!(4)), "Zoom selects the zoom in use");
        assert_eq!(popup.press(Key::Num1), None, "no digits in Zoom");
        assert_eq!(popup.press(Key::ArrowUp), None);
        assert_eq!(popup.press(Key::Enter), Some(Run::IdeZoom(100)));

        let mut popup = open_popup();
        assert_eq!(popup.press(Key::ArrowUp), None);
        assert_eq!(selected(&popup), (Value::Null, json!(5)), "the list wraps around");
        assert_eq!(popup.press(Key::Num4), None);
        assert_eq!(popup.press(Key::Escape), None);
        assert!(!popup.is_open(), "Escape closes the list, the Switch popup is gone");
    }

    #[test]
    fn a_digit_picks_its_row_at_once() {
        let mut popup = open_popup();
        assert_eq!(popup.press(Key::Num5), None);
        assert_eq!(popup.press(Key::Num1), Some(Run::Action("view.presentationMode.toggle", json!({}))));
        let mut popup = open_popup();
        assert_eq!(popup.press(Key::Num3), None);
        assert_eq!(popup.press(Key::Num2), Some(Run::CodeStyleScheme(CodeStyleScheme::Default)));
        let mut popup = open_popup();
        assert_eq!(popup.press(Key::Num9), None, "no ninth row");
        assert_eq!(selected(&popup), (Value::Null, json!(0)));
        assert_eq!(popup.press(Key::Num1), None);
        assert_eq!(popup.press(Key::Num9), None, "no ninth theme");
        assert!(popup.is_open());
        assert_eq!(popup.press(Key::Num3), Some(Run::Command("ui.theme.select", json!({ "id": "dusk" }))));
        assert_eq!(digit(Key::Num0), Some(9));
    }

    #[test]
    fn typing_filters_the_rows_in_either_keyboard_layout() {
        let mut popup = open_popup();
        popup.type_text("k");
        let json = popup.to_json();
        assert_eq!((json["filter"].clone(), json["selected"].clone()), (json!("k"), json!(3)), "only Keymap");
        assert_eq!(popup.press(Key::Backspace), None);
        popup.type_text("л");
        assert_eq!(selected(&popup), (Value::Null, json!(3)), "л is the k key in the Russian layout");
        popup.type_text("1");
        assert_eq!(popup.to_json()["filter"], "л", "digits never go to the filter");
        assert_eq!(popup.press(Key::Num1), None, "a digit picks nothing while filtering");
        assert_eq!(selected(&popup), (Value::Null, json!(3)));
        assert_eq!(popup.press(Key::Escape), None);
        assert!(popup.is_open(), "Escape clears the filter first");
        assert_eq!(popup.to_json()["filter"], "");
        popup.type_text("zzz");
        assert_eq!(popup.to_json()["selected"], Value::Null, "nothing matches");
        assert_eq!(popup.press(Key::Enter), None);
        assert!(popup.is_open());
        popup.open(open_popup().shown.unwrap().groups);
        popup.type_text("Mode");
        assert_eq!(popup.press(Key::Enter), None);
        assert_eq!(selected(&popup), (json!("viewMode"), json!(0)));
        assert_eq!(popup.to_json()["filter"], "", "a list opens unfiltered");
    }

    #[test]
    fn an_empty_list_is_not_opened() {
        let mut popup = QuickSwitch::default();
        popup.open(groups(&Sources {
            themes: None,
            keymaps: None,
            code_style_scheme: CodeStyleScheme::Project,
            view_modes: Vec::new(),
            ide_zoom: 100,
        }));
        assert_eq!(popup.press(Key::Num1), None);
        assert_eq!(selected(&popup), (Value::Null, json!(0)));
        assert_eq!(popup.press(Key::Num3), None);
        assert_eq!(selected(&popup), (json!("codeStyleScheme"), json!(1)));
    }
}

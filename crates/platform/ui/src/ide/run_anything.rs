use std::path::{Path, PathBuf};

use egui::{Align2, Event, Key, Sense, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::run_anything::{AnythingContext, AnythingItem, HELP, ItemKind};

use super::{IdePalette, dialog_frame};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const ROWS: usize = 12;
const WIDTH: f32 = 672.0;
const OPEN: &str = "open";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Anything,
    Choose { debug: bool },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RowKind {
    Recent,
    Configuration,
    Command,
    Help,
    Project,
    Edit,
    Context,
}

#[derive(Clone, Debug, PartialEq)]
struct Row {
    kind: RowKind,
    text: String,
    description: String,
    configuration_id: Option<String>,
    temporary: bool,
    path: Option<PathBuf>,
}

impl Row {
    fn new(kind: RowKind, text: &str) -> Self {
        Self {
            kind,
            text: text.to_owned(),
            description: String::new(),
            configuration_id: None,
            temporary: false,
            path: None,
        }
    }

    fn of(item: AnythingItem) -> Self {
        let kind = match item.kind {
            ItemKind::Recent => RowKind::Recent,
            ItemKind::Configuration => RowKind::Configuration,
            ItemKind::Command => RowKind::Command,
            ItemKind::Help => RowKind::Help,
        };
        Self {
            kind,
            text: item.text,
            description: item.description,
            configuration_id: item.configuration_id,
            temporary: item.temporary,
            path: None,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Outcome {
    Run(Value),
    Start(&'static str, Value),
    Open(PathBuf),
    EditConfigurations,
}

#[derive(Default)]
pub(super) struct RunAnythingPopup {
    mode: Option<Mode>,
    query: String,
    selected: usize,
    context: AnythingContext,
    rows: Vec<Row>,
    built_for: Option<String>,
    choices: Vec<Row>,
}

pub(super) fn context_name(context: AnythingContext) -> String {
    match context {
        AnythingContext::Project => t!("ide-run-anything-context-project"),
        AnythingContext::Module => t!("ide-run-anything-context-module"),
        AnythingContext::File => t!("ide-run-anything-context-file"),
    }
}

fn group_name(kind: RowKind) -> String {
    match kind {
        RowKind::Recent => t!("ide-run-anything-group-recent"),
        RowKind::Configuration | RowKind::Context => t!("ide-run-anything-group-configurations"),
        RowKind::Command => t!("ide-run-anything-group-commands"),
        RowKind::Help => t!("ide-run-anything-group-help"),
        RowKind::Project => t!("ide-run-anything-group-projects"),
        RowKind::Edit => String::new(),
    }
}

fn project_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned())
}

impl RunAnythingPopup {
    #[cfg(test)]
    pub(super) fn typed(&self) -> &str {
        &self.query
    }

    pub(super) fn mode(&self) -> Option<Mode> {
        self.mode
    }

    pub(super) fn is_open(&self) -> bool {
        self.mode.is_some()
    }

    pub(super) fn open(&mut self, mode: Mode, query: &str) {
        if self.mode == Some(mode) && query.is_empty() {
            self.close();
            return;
        }
        let context = self.context;
        *self = Self { mode: Some(mode), query: query.to_owned(), context, ..Self::default() };
    }

    pub(super) fn close(&mut self) {
        self.mode = None;
        self.query.clear();
        self.rows.clear();
        self.choices.clear();
        self.built_for = None;
    }

    pub(super) fn set_context(&mut self, context: AnythingContext) {
        self.context = context;
    }

    #[cfg(test)]
    pub(super) fn context(&self) -> AnythingContext {
        self.context
    }

    fn build(&mut self, registry: &Registry, session: &mut Session, projects: &[PathBuf]) {
        if self.built_for.as_deref() == Some(self.query.as_str()) {
            return;
        }
        self.rows = match self.mode {
            Some(Mode::Anything) => anything_rows(registry, session, &self.query, projects),
            Some(Mode::Choose { .. }) => {
                if self.built_for.is_none() {
                    self.choices = choice_rows(registry, session);
                }
                let query = self.query.trim().to_lowercase();
                self.choices
                    .iter()
                    .filter(|row| {
                        row.kind == RowKind::Edit
                            || query.is_empty()
                            || row.text.to_lowercase().contains(&query)
                            || zen_index::NamePattern::new(&query).matches(&row.text).is_some()
                    })
                    .cloned()
                    .collect()
            }
            None => Vec::new(),
        };
        self.selected = match self.mode {
            Some(Mode::Choose { .. }) if self.rows.len() > 1 => 1,
            _ => 0,
        };
        self.built_for = Some(self.query.clone());
    }

    fn outcome(&mut self, row: Option<&Row>, shift: bool, alt: bool) -> Option<Outcome> {
        let mode = self.mode?;
        if let Mode::Choose { debug } = mode {
            let row = row?;
            let debug = debug != shift;
            let command = if debug { "debug.session.start" } else { "run.process.start" };
            return match (row.kind, &row.configuration_id) {
                (RowKind::Edit, _) => Some(Outcome::EditConfigurations),
                (_, Some(id)) => Some(Outcome::Start(command, json!({ "configurationId": id }))),
                _ => Some(Outcome::Start(command, json!({ "context": {} }))),
            };
        }
        let context = if alt { AnythingContext::File } else { self.context };
        match row {
            Some(row) if row.kind == RowKind::Help => {
                self.query = row.text.clone();
                None
            }
            Some(row) if row.kind == RowKind::Project => row.path.clone().map(Outcome::Open),
            Some(row) => {
                let mut params = json!({ "text": row.text, "debug": shift, "context": context });
                if let Some(id) = &row.configuration_id {
                    params["configurationId"] = json!(id);
                }
                Some(Outcome::Run(params))
            }
            None if !self.query.trim().is_empty() && self.query.trim() != HELP => {
                Some(Outcome::Run(json!({ "text": self.query.trim(), "debug": shift, "context": context })))
            }
            None => None,
        }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        projects: &[PathBuf],
    ) -> Option<Outcome> {
        let mode = self.mode?;
        self.build(registry, session, projects);
        let count = self.rows.len();
        self.selected = self.selected.min(count.saturating_sub(1));
        let (mut enter, mut close, mut complete) = (None, false, false);
        let mut typed_digit = None;
        let held = ui.input(|input| input.modifiers);
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Key { key, pressed, modifiers, .. } => {
                    if *pressed {
                        match key {
                            Key::ArrowDown if count > 0 => self.selected = (self.selected + 1) % count,
                            Key::ArrowUp if count > 0 => self.selected = (self.selected + count - 1) % count,
                            Key::Enter => enter = Some((modifiers.shift, modifiers.alt)),
                            Key::Escape => close = true,
                            Key::Tab => complete = true,
                            Key::Backspace if modifiers.command || modifiers.alt => {
                                let kept = self.query.trim_end().rfind(' ').map_or(0, |at| at + 1);
                                self.query.truncate(if modifiers.command { 0 } else { kept });
                            }
                            Key::Backspace => {
                                self.query.pop();
                            }
                            _ => {}
                        }
                    }
                    false
                }
                Event::Text(text) => {
                    match (mode, text.parse::<usize>()) {
                        (Mode::Choose { .. }, Ok(digit)) if self.query.is_empty() && text.len() == 1 => {
                            typed_digit = Some(digit);
                        }
                        _ => self.query.push_str(text),
                    }
                    false
                }
                Event::Paste(text) => {
                    self.query.push_str(text.lines().next().unwrap_or_default());
                    false
                }
                Event::Copy | Event::Cut | Event::Ime(_) => false,
                _ => true,
            });
        });
        if complete
            && let Some(row) = self.rows.get(self.selected)
            && matches!(row.kind, RowKind::Recent | RowKind::Command | RowKind::Help)
        {
            self.query = row.text.clone();
        }
        let title = match mode {
            Mode::Anything if held.shift => t!("ide-run-anything-title-debug"),
            Mode::Anything if held.alt => t!("ide-run-anything-title-context"),
            Mode::Anything => t!("ide-run-anything-title"),
            Mode::Choose { debug } if debug != held.shift => t!("ide-run-choose-debug"),
            Mode::Choose { .. } => t!("ide-run-choose-run"),
        };
        let mut clicked = None;
        let mut double_clicked = None;
        let screen = ui.ctx().content_rect();
        let width = WIDTH.min(screen.width() - 32.0);
        let area = egui::Area::new(ui.scope_id().with("run-anything"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.center().x - width / 2.0, screen.top() + screen.height() * 0.16));
        let response = area.show(ui.ctx(), |ui| {
            dialog_frame(palette).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&title).font(theme::semibold(13.0)).color(palette.text));
                    if mode == Mode::Anything {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::ComboBox::from_id_salt("run-anything-context")
                                .selected_text(context_name(self.context))
                                .show_ui(ui, |ui| {
                                    for choice in
                                        [AnythingContext::Project, AnythingContext::Module, AnythingContext::File]
                                    {
                                        ui.selectable_value(&mut self.context, choice, context_name(choice));
                                    }
                                });
                        });
                    }
                });
                ui.add_space(8.0);
                let (field, _) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::hover());
                ui.painter().rect_filled(field, 5, palette.editor_background);
                let (shown, color) = if self.query.is_empty() {
                    let hint = match mode {
                        Mode::Anything => t!("ide-run-anything-placeholder"),
                        Mode::Choose { .. } => t!("ide-run-choose-placeholder"),
                    };
                    (hint, palette.text_secondary)
                } else {
                    (format!("{}▏", self.query), palette.text)
                };
                ui.painter().text(
                    pos2(field.left() + 8.0, field.center().y),
                    Align2::LEFT_CENTER,
                    shown,
                    theme::regular(13.0),
                    color,
                );
                if mode == Mode::Anything {
                    ui.painter().text(
                        pos2(field.right() - 8.0, field.center().y),
                        Align2::RIGHT_CENTER,
                        t!("ide-run-anything-help-hint"),
                        theme::regular(12.0),
                        palette.text_secondary,
                    );
                }
                ui.add_space(6.0);
                if self.rows.is_empty() {
                    let empty = if mode == Mode::Anything && !self.query.trim().is_empty() {
                        t!("ide-run-anything-in-terminal")
                    } else {
                        t!("ide-run-anything-nothing")
                    };
                    ui.label(egui::RichText::new(empty).font(theme::regular(12.5)).color(palette.text_secondary));
                } else {
                    super::filter_popups::list(ui, "run-anything-rows", ROW * ROWS as f32, |ui| {
                        let mut group = None;
                        for (index, row) in self.rows.iter().enumerate() {
                            let name = group_name(row.kind);
                            if mode == Mode::Anything && group.as_ref() != Some(&name) {
                                ui.label(
                                    egui::RichText::new(&name).font(theme::regular(11.5)).color(palette.text_secondary),
                                );
                                group = Some(name);
                            }
                            let (rect, response) =
                                ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                            let chosen = index == self.selected;
                            accessibility::item(&response, egui::Role::ListBoxOption, chosen, &row.text);
                            if chosen {
                                ui.painter().rect_filled(rect, 5, palette.selection);
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 5, palette.popup_hover);
                            }
                            let label = match (mode, row.kind) {
                                (Mode::Choose { .. }, RowKind::Edit) => format!("0. {}", row.text),
                                (Mode::Choose { .. }, _) if index < 10 => format!("{index}. {}", row.text),
                                _ => row.text.clone(),
                            };
                            let color = if row.temporary { palette.text_secondary } else { palette.text };
                            ui.painter().text(
                                pos2(rect.left() + 8.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                label,
                                theme::regular(13.0),
                                color,
                            );
                            if !row.description.is_empty() {
                                ui.painter().text(
                                    pos2(rect.right() - 8.0, rect.center().y),
                                    Align2::RIGHT_CENTER,
                                    &row.description,
                                    theme::regular(12.0),
                                    palette.text_secondary,
                                );
                            }
                            if response.clicked() {
                                clicked = Some(index);
                            }
                            if response.double_clicked() {
                                double_clicked = Some(index);
                            }
                        }
                    });
                }
                ui.add_space(6.0);
                let tip = match mode {
                    Mode::Anything => t!("ide-run-anything-tip"),
                    Mode::Choose { debug: false } => t!("ide-run-choose-tip-run"),
                    Mode::Choose { debug: true } => t!("ide-run-choose-tip-debug"),
                };
                ui.label(egui::RichText::new(tip).font(theme::regular(11.5)).color(palette.text_secondary));
            });
        });
        if let Some(index) = clicked {
            self.selected = index;
        }
        if let Some(digit) = typed_digit
            && digit < self.rows.len()
        {
            self.selected = digit;
            enter = Some((false, false));
        }
        if double_clicked.is_some() {
            enter = Some((held.shift, held.alt));
        }
        if ui.input(|input| input.pointer.any_pressed())
            && !egui::Popup::is_any_open(ui.ctx())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !response.response.rect.contains(pointer)
        {
            close = true;
        }
        let mut outcome = None;
        if let Some((shift, alt)) = enter {
            let row = self.rows.get(self.selected).cloned();
            outcome = self.outcome(row.as_ref(), shift, alt);
            if outcome.is_some() {
                close = true;
            }
        }
        if close {
            self.close();
        }
        outcome
    }
}

fn anything_rows(registry: &Registry, session: &mut Session, query: &str, projects: &[PathBuf]) -> Vec<Row> {
    let mut rows: Vec<Row> = registry
        .dispatch(session, "run.anything.list", json!({ "query": query }))
        .ok()
        .and_then(|listed| serde_json::from_value::<Vec<AnythingItem>>(listed["items"].clone()).ok())
        .unwrap_or_default()
        .into_iter()
        .map(Row::of)
        .collect();
    let trimmed = query.trim_start();
    if trimmed.trim() == HELP {
        rows.push(Row { description: t!("ide-run-anything-open-help"), ..Row::new(RowKind::Help, "open ") });
    } else if let Some(rest) = trimmed.strip_prefix(OPEN)
        && (rest.is_empty() || rest.starts_with(' '))
    {
        let wanted = rest.trim().to_lowercase();
        rows.retain(|row| row.kind != RowKind::Command);
        for path in projects {
            let name = project_name(path);
            if wanted.is_empty() || name.to_lowercase().contains(&wanted) {
                rows.push(Row {
                    description: path.to_string_lossy().into_owned(),
                    path: Some(path.clone()),
                    ..Row::new(RowKind::Project, &format!("{OPEN} {name}"))
                });
            }
        }
    }
    rows
}

fn choice_rows(registry: &Registry, session: &mut Session) -> Vec<Row> {
    let mut rows = vec![Row::new(RowKind::Edit, &t!("keymap-view-runConfigurations-open"))];
    let listed = registry.dispatch(session, "run.configuration.list", json!({})).unwrap_or_default();
    let kinds = listed["kinds"].as_array().cloned().unwrap_or_default();
    let kind_name = |kind: &str| {
        kinds
            .iter()
            .find(|info| info["id"] == json!(kind))
            .and_then(|info| info["nameMessage"].as_str())
            .map(|message| t!(message))
            .unwrap_or_default()
    };
    let mut configurations: Vec<Row> = listed["configurations"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|kept| Row {
            description: kind_name(kept["kind"].as_str().unwrap_or_default()),
            configuration_id: kept["id"].as_str().map(str::to_owned),
            temporary: kept["temporary"].as_bool().unwrap_or_default(),
            ..Row::new(RowKind::Configuration, kept["name"].as_str().unwrap_or_default())
        })
        .collect();
    configurations.sort_by_key(|row| row.temporary);
    rows.extend(configurations);
    if let Ok(context) = registry.dispatch(session, "run.context.get", json!({}))
        && context["existing"].is_null()
        && let Some(name) = context["configuration"]["name"].as_str()
    {
        rows.push(Row {
            description: kind_name(context["configuration"]["kind"].as_str().unwrap_or_default()),
            ..Row::new(RowKind::Context, name)
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use zen_run::fake::FakeProvider;
    use zen_run::{RunEnv, RunProvider};
    use zen_workbench::run::Runs;

    use super::*;

    fn setup() -> (Registry, Session, tempfile::TempDir) {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let providers: Vec<Arc<dyn RunProvider>> = vec![Arc::new(FakeProvider::new())];
        session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
        for name in ["server", "client"] {
            let configuration = serde_json::to_value(FakeProvider::configuration(name)).unwrap();
            registry
                .dispatch(&mut session, "run.configuration.create", json!({ "configuration": configuration }))
                .unwrap();
        }
        (registry, session, dir)
    }

    fn texts(popup: &RunAnythingPopup) -> Vec<String> {
        popup.rows.iter().map(|row| row.text.clone()).collect()
    }

    #[test]
    fn open_lists_recent_projects_and_question_mark_names_the_open_prefix() {
        let (registry, mut session, _dir) = setup();
        let projects = vec![PathBuf::from("/work/shop"), PathBuf::from("/work/blog")];
        let mut popup = RunAnythingPopup::default();
        popup.open(Mode::Anything, "open sh");
        popup.build(&registry, &mut session, &projects);
        assert_eq!(texts(&popup), ["open shop"]);
        let row = popup.rows[0].clone();
        assert_eq!(popup.outcome(Some(&row), false, false), Some(Outcome::Open(PathBuf::from("/work/shop"))));
        popup.open(Mode::Anything, "?");
        popup.build(&registry, &mut session, &projects);
        assert_eq!(texts(&popup).last().map(String::as_str), Some("open "));
        let help = popup.rows.last().cloned();
        assert_eq!(popup.outcome(help.as_ref(), false, false), None);
        assert_eq!(popup.query, "open ", "a prefix is typed into the field");
    }

    #[test]
    fn enter_runs_the_row_shift_debugs_and_option_runs_in_the_files_folder() {
        let (registry, mut session, _dir) = setup();
        let mut popup = RunAnythingPopup::default();
        popup.open(Mode::Anything, "");
        popup.build(&registry, &mut session, &[]);
        assert_eq!(texts(&popup), ["server", "client"]);
        let row = popup.rows[1].clone();
        let Some(Outcome::Run(params)) = popup.outcome(Some(&row), true, false) else { panic!() };
        assert_eq!(params["text"], json!("client"));
        assert_eq!(params["debug"], json!(true));
        assert!(params["configurationId"].is_string());
        assert_eq!(params["context"], json!("project"));
        popup.set_context(AnythingContext::Module);
        popup.query = "make all".to_owned();
        let Some(Outcome::Run(params)) = popup.outcome(None, false, false) else { panic!() };
        assert_eq!(params, json!({ "text": "make all", "debug": false, "context": "module" }));
        let Some(Outcome::Run(params)) = popup.outcome(None, false, true) else { panic!() };
        assert_eq!(params["context"], json!("file"));
    }

    #[test]
    fn the_choosers_list_edit_first_then_the_configurations_and_start_the_chosen_one() {
        let (registry, mut session, _dir) = setup();
        let mut popup = RunAnythingPopup::default();
        popup.open(Mode::Choose { debug: false }, "");
        popup.build(&registry, &mut session, &[]);
        assert_eq!(texts(&popup), ["Edit Configurations…", "server", "client"]);
        assert_eq!(popup.selected, 1, "the first configuration is chosen");
        let edit = popup.rows[0].clone();
        assert_eq!(popup.outcome(Some(&edit), false, false), Some(Outcome::EditConfigurations));
        let client = popup.rows[2].clone();
        let id = client.configuration_id.clone().unwrap();
        let run = Outcome::Start("run.process.start", json!({ "configurationId": id }));
        assert_eq!(popup.outcome(Some(&client), false, false), Some(run));
        let debug = Outcome::Start("debug.session.start", json!({ "configurationId": id }));
        assert_eq!(popup.outcome(Some(&client), true, false), Some(debug), "⇧ debugs from Run…");
        popup.open(Mode::Choose { debug: true }, "cli");
        popup.build(&registry, &mut session, &[]);
        assert_eq!(texts(&popup), ["Edit Configurations…", "client"]);
        let client = popup.rows[1].clone();
        let debug = Outcome::Start("debug.session.start", json!({ "configurationId": id }));
        assert_eq!(popup.outcome(Some(&client), false, false), Some(debug));
    }

    #[test]
    fn opening_the_same_popup_again_closes_it_and_the_context_stays() {
        let mut popup = RunAnythingPopup::default();
        popup.set_context(AnythingContext::File);
        popup.open(Mode::Anything, "");
        assert!(popup.is_open());
        popup.open(Mode::Anything, "");
        assert!(!popup.is_open());
        popup.open(Mode::Choose { debug: false }, "");
        assert_eq!(popup.mode(), Some(Mode::Choose { debug: false }));
        assert_eq!(popup.context(), AnythingContext::File);
    }
}

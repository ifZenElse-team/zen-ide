use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::PathBuf;

use egui::{Align, Color32, Id, Key, Layout, Rect, RichText, Role, ScrollArea, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;
use zen_project::generators::{
    Column, Field, FieldKind, FieldOption, GeneratorForm, GeneratorInfo, GeneratorSection, ManagedList, OptionGroup,
    Values,
};
use zen_workbench::jdk::commands::OFFERED;
use zen_workbench::new_project::{LocationCheck, missing_text};

use super::settings::Dispatch;
use super::{IdePalette, dialog_frame, form};
use crate::{accessibility, theme};

const SELECT_HEIGHT: f32 = 220.0;
const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 716.0;
const LIST: f32 = 228.0;
const LABEL: f32 = 110.0;
const FIELD: f32 = 300.0;
const CHOICE: f32 = 170.0;
const ROW: f32 = 28.0;
const TREE_ROW: f32 = 26.0;
const POLL: std::time::Duration = std::time::Duration::from_millis(100);
const FIRST_MODULE: &str = "untitled";
const OUTPUT_HEIGHT: f32 = 130.0;
const TABLE_HEIGHT: f32 = 110.0;

type Row = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq)]
enum Loaded<T> {
    Running,
    Ready(T),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Cancelled,
    Created(PathBuf),
    MorePlugins,
    GeneratorSettings,
    Module { path: PathBuf, registered: Option<String>, reload: Option<String> },
}

#[derive(Clone, Debug, Default, PartialEq)]
struct ModuleMode {
    defaults: Values,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Managing {
    field: String,
    list: ManagedList,
    rows: Vec<Row>,
    added: Row,
}

#[derive(Debug)]
pub(super) struct NewProjectDialog {
    generators: Vec<GeneratorInfo>,
    network_allowed: bool,
    connected: HashSet<String>,
    selected: String,
    location: String,
    git: bool,
    check: LocationCheck,
    forms: BTreeMap<String, Loaded<GeneratorForm>>,
    fresh: HashSet<String>,
    values: BTreeMap<String, Values>,
    searches: BTreeMap<String, String>,
    type_search: String,
    highlighted: Option<String>,
    groups_open: BTreeMap<String, bool>,
    page: u8,
    job: Option<u64>,
    error: Option<String>,
    asking_reuse: bool,
    searching: bool,
    wanted_jdk: Option<u32>,
    downloading_jdk: Option<u32>,
    wanted_generator: Option<String>,
    wanted_list: Option<(String, ManagedList)>,
    managing: Option<Managing>,
    module: Option<ModuleMode>,
    output: Vec<String>,
    pub(super) by_agent: bool,
}

fn run<T: serde::de::DeserializeOwned>(dispatch: &mut Dispatch<'_>, id: &str, params: Value) -> Result<T, String> {
    serde_json::from_value(dispatch(id, params)?).map_err(|error| error.to_string())
}

fn loaded<T: serde::de::DeserializeOwned>(answer: &Value, key: &str) -> Loaded<T> {
    match answer["state"].as_str() {
        Some("running") => Loaded::Running,
        Some("ready") => match serde_json::from_value(answer[key].clone()) {
            Ok(value) => Loaded::Ready(value),
            Err(error) => Loaded::Failed(error.to_string()),
        },
        _ => Loaded::Failed(answer["error"].as_str().unwrap_or_default().to_owned()),
    }
}

impl NewProjectDialog {
    pub(super) fn open(dispatch: &mut Dispatch<'_>, generator: Option<&str>) -> Result<Self, String> {
        let list: Value = dispatch("project.new.generators", json!({}))?;
        let generators: Vec<GeneratorInfo> =
            serde_json::from_value(list["generators"].clone()).map_err(|error| error.to_string())?;
        let selected = match generator {
            Some(generator) if generators.iter().any(|known| known.id == generator) => generator.to_owned(),
            Some(generator) => return Err(t!("workbench-new-project-unknown", generator = generator)),
            None => generators.first().map(|first| first.id.clone()).unwrap_or_default(),
        };
        let location = list["suggested"].as_str().unwrap_or_default().to_owned();
        let check: LocationCheck = run(dispatch, "project.new.check", json!({ "location": location }))?;
        Ok(Self {
            generators,
            network_allowed: list["networkAllowed"].as_bool().unwrap_or(true),
            connected: HashSet::new(),
            selected,
            location,
            git: false,
            check,
            forms: BTreeMap::new(),
            fresh: HashSet::new(),
            values: BTreeMap::new(),
            searches: BTreeMap::new(),
            type_search: String::new(),
            highlighted: None,
            groups_open: BTreeMap::new(),
            page: 0,
            job: None,
            error: None,
            asking_reuse: false,
            searching: false,
            wanted_jdk: None,
            downloading_jdk: None,
            wanted_generator: None,
            wanted_list: None,
            managing: None,
            module: None,
            output: Vec::new(),
            by_agent: false,
        })
    }

    pub(super) fn open_module(dispatch: &mut Dispatch<'_>, generator: Option<&str>) -> Result<Self, String> {
        let mut dialog = Self::open(dispatch, None)?;
        let options: Value = dispatch("project.module.options", json!({}))?;
        dialog.generators = serde_json::from_value(options["generators"].clone()).map_err(|error| error.to_string())?;
        dialog.selected = match generator {
            Some(generator) if dialog.generators.iter().any(|known| known.id == generator) => generator.to_owned(),
            Some(generator) => return Err(t!("workbench-new-project-unknown", generator = generator)),
            None => dialog.generators.first().map(|first| first.id.clone()).unwrap_or_default(),
        };
        let defaults = serde_json::from_value(options["values"].clone()).map_err(|error| error.to_string())?;
        dialog.module = Some(ModuleMode { defaults });
        dialog.location = options["location"].as_str().unwrap_or_default().to_owned();
        dialog.set_name(dispatch, FIRST_MODULE);
        Ok(dialog)
    }

    fn selected_info(&self) -> Option<&GeneratorInfo> {
        self.generators.iter().find(|generator| generator.id == self.selected)
    }

    fn given(&self) -> Values {
        let mut given = self.module.as_ref().map(|module| module.defaults.clone()).unwrap_or_default();
        given.extend(self.values.get(&self.selected).cloned().unwrap_or_default());
        given
    }

    fn recheck(&mut self, dispatch: &mut Dispatch<'_>, location: &str) {
        match run(dispatch, "project.new.check", json!({ "location": location })) {
            Ok(check) => self.check = check,
            Err(error) => self.error = Some(error),
        }
    }

    pub(super) fn set_location(&mut self, dispatch: &mut Dispatch<'_>, location: String) {
        self.location = location;
        if self.module.is_some() {
            let name = self.check.name.clone();
            self.set_name(dispatch, &name);
        } else {
            let location = self.location.clone();
            self.recheck(dispatch, &location);
        }
    }

    pub(super) fn set_name(&mut self, dispatch: &mut Dispatch<'_>, name: &str) {
        let folder = PathBuf::from(&self.location);
        if self.module.is_some() {
            self.recheck(dispatch, &folder.join(name).to_string_lossy());
        } else {
            self.set_location(dispatch, folder.with_file_name(name).to_string_lossy().into_owned());
        }
        if name.is_empty() || name.contains('/') {
            self.check.problem = Some(t!("workbench-new-project-bad-name"));
        }
        self.check.name = name.to_owned();
    }

    fn reason(&self) -> Option<String> {
        if let Some(problem) = &self.check.problem {
            return Some(problem.clone());
        }
        match self.forms.get(&self.selected) {
            Some(Loaded::Ready(form)) => {
                let values = form.resolved(&self.check.name, &self.given());
                form.missing(&values).map(missing_text).or_else(|| form.blocked.clone())
            }
            _ => Some(String::new()),
        }
    }

    pub(super) fn waits_to_connect(&self) -> Option<&str> {
        let service = self.selected_info()?.service.as_deref()?;
        (self.network_allowed && !self.by_agent && !self.connected.contains(service)).then_some(service)
    }

    pub(super) fn connect(&mut self, dispatch: &mut Dispatch<'_>) {
        if let Some(service) = self.waits_to_connect().map(str::to_owned) {
            self.connected.insert(service);
            self.load_form(dispatch, false);
        }
    }

    fn load_form(&mut self, dispatch: &mut Dispatch<'_>, retry: bool) {
        if self.waits_to_connect().is_some() {
            return;
        }
        let first = self.fresh.insert(self.selected.clone());
        if !first && !retry && !matches!(self.forms.get(&self.selected), Some(Loaded::Running)) {
            return;
        }
        let params = json!({
            "generator": self.selected,
            "retry": first || retry,
            "values": self.given(),
            "module": self.module.is_some(),
        });
        let form = match dispatch("project.new.form", params) {
            Ok(answer) => loaded::<GeneratorForm>(&answer, "form"),
            Err(error) => Loaded::Failed(error),
        };
        self.forms.insert(self.selected.clone(), form);
    }

    pub(super) fn download_jdk(&mut self, dispatch: &mut Dispatch<'_>, version: u32) {
        self.error = None;
        match dispatch("toolchain.jdk.download", json!({ "version": version })) {
            Ok(_) => self.downloading_jdk = Some(version),
            Err(error) => self.error = Some(error),
        }
    }

    fn follow_jdk_download(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.downloading_jdk.is_none() {
            return;
        }
        let Ok(list) = dispatch("toolchain.jdk.list", json!({})) else { return };
        if list["downloading"].is_null() {
            self.downloading_jdk = None;
            self.load_form(dispatch, true);
        }
    }

    fn create(&mut self, dispatch: &mut Dispatch<'_>, reuse: bool) {
        let (command, params) = if self.module.is_some() {
            let params = json!({
                "generator": self.selected,
                "name": self.check.name,
                "location": self.location,
                "values": self.given(),
                "reuse": reuse,
            });
            ("project.module.new", params)
        } else {
            let params = json!({
                "generator": self.selected,
                "location": self.location,
                "values": self.given(),
                "git": self.git,
                "reuse": reuse,
            });
            ("project.new.create", params)
        };
        self.error = None;
        self.output.clear();
        match dispatch(command, params) {
            Ok(answer) => self.job = answer["job"].as_u64(),
            Err(error) => self.error = Some(error),
        }
    }

    fn follow_job(&mut self, dispatch: &mut Dispatch<'_>) -> Option<Outcome> {
        let job = self.job?;
        let answer = match dispatch("project.new.status", json!({ "job": job })) {
            Ok(answer) => answer,
            Err(error) => {
                self.job = None;
                self.error = Some(error);
                return None;
            }
        };
        if let Some(lines) = answer["output"].as_array() {
            self.output = lines.iter().filter_map(|line| line.as_str().map(str::to_owned)).collect();
        }
        match loaded::<PathBuf>(&answer, "path") {
            Loaded::Running => None,
            Loaded::Ready(path) => {
                self.job = None;
                let text = |key: &str| answer[key].as_str().map(str::to_owned);
                Some(match self.module {
                    Some(_) => Outcome::Module { path, registered: text("registered"), reload: text("reload") },
                    None => Outcome::Created(path),
                })
            }
            Loaded::Failed(error) => {
                self.job = None;
                self.error = Some(error);
                None
            }
        }
    }

    fn confirm(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.check.not_empty {
            self.asking_reuse = true;
        } else {
            self.create(dispatch, false);
        }
    }

    fn primary(&mut self, dispatch: &mut Dispatch<'_>) {
        if self.reason().is_some() || self.job.is_some() {
            return;
        }
        if self.page < self.last_page() {
            self.page += 1;
        } else {
            self.confirm(dispatch);
        }
    }

    fn last_page(&self) -> u8 {
        match self.forms.get(&self.selected) {
            Some(Loaded::Ready(form)) => form.last_page(),
            _ => 0,
        }
    }

    fn shown_types(&self) -> Vec<&GeneratorInfo> {
        let query = self.type_search.trim().to_lowercase();
        self.generators.iter().filter(|generator| generator.name.to_lowercase().contains(&query)).collect()
    }

    pub(super) fn settings_changed(&mut self) {
        self.forms.clear();
        self.fresh.clear();
    }

    fn type_keys(&mut self, ui: &Ui, dispatch: &mut Dispatch<'_>) {
        let typing = ui.memory(|memory| memory.focused()).is_some_and(|focused| focused != search_id(ui));
        if self.page != 0 || self.job.is_some() || self.asking_reuse || typing {
            return;
        }
        if egui::Popup::is_any_open(ui.ctx()) {
            return;
        }
        let (up, down) = ui.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
            )
        });
        let moved = isize::from(down) - isize::from(up);
        if moved == 0 {
            return;
        }
        let shown: Vec<String> = self.shown_types().iter().map(|generator| generator.id.clone()).collect();
        if shown.is_empty() {
            return;
        }
        let count = shown.len() as isize;
        let at = shown.iter().position(|id| *id == self.selected).map_or(0, |at| at as isize + moved);
        let next = shown[at.rem_euclid(count) as usize].clone();
        if next != self.selected {
            self.choose(dispatch, next);
        }
    }

    #[cfg(test)]
    pub(super) fn set_value(&mut self, field: &str, value: Value) {
        self.values.entry(self.selected.clone()).or_default().insert(field.to_owned(), value);
    }

    #[cfg(test)]
    pub(super) fn given_value(&self, field: &str) -> Option<Value> {
        self.given().get(field).cloned()
    }

    #[cfg(test)]
    pub(super) fn set_type_search(&mut self, text: &str) {
        text.clone_into(&mut self.type_search);
    }

    #[cfg(test)]
    pub(super) fn value(&self, field: &str) -> Option<Value> {
        self.values.get(&self.selected)?.get(field).cloned()
    }

    #[cfg(test)]
    pub(super) fn form_changed(&mut self) {
        self.forms.insert(self.selected.clone(), Loaded::Running);
    }

    #[cfg(test)]
    pub(super) fn save_list(&mut self, dispatch: &mut Dispatch<'_>) {
        self.save_managed_list(dispatch);
    }

    #[cfg(test)]
    pub(super) fn set_search(&mut self, field: &str, text: &str) {
        self.searches.insert(field.to_owned(), text.to_owned());
    }

    pub(super) fn state(&self) -> Value {
        json!({
            "generator": self.selected,
            "location": self.location,
            "name": self.check.name,
            "git": self.git,
            "module": self.module.is_some(),
            "output": self.output,
            "managing": self.managing.as_ref().map(|managing| managing.list.id.clone()),
            "problem": self.reason().filter(|reason| !reason.is_empty()),
            "page": self.page,
            "lastPage": self.last_page(),
            "creating": self.job.is_some(),
            "connects": self.waits_to_connect(),
            "downloadingJdk": self.downloading_jdk,
            "error": self.error,
        })
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) -> Option<Outcome> {
        self.load_form(dispatch, false);
        self.follow_jdk_download(dispatch);
        let created = self.follow_job(dispatch);
        if created.is_some() {
            return created;
        }
        let running = self.job.is_some()
            || self.downloading_jdk.is_some()
            || matches!(self.forms.get(&self.selected), Some(Loaded::Running));
        if running {
            ui.ctx().request_repaint_after(POLL);
        }
        self.searching = false;
        let mut outcome = None;
        let size = ui.ctx().content_rect().size();
        let (width, height) = ((size.x - 80.0).clamp(560.0, WIDTH), (size.y - 80.0).clamp(360.0, HEIGHT));
        let modal = egui::Modal::new(ui.scope_id().with("new-project")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.set_height(height);
            ui.spacing_mut().item_spacing.y = 8.0;
            ui.label(RichText::new(self.title()).font(theme::semibold(15.0)).color(p.text));
            let footer = if self.output.is_empty() { 66.0 } else { 66.0 + OUTPUT_HEIGHT };
            let body = vec2(width, height - footer);
            ui.allocate_ui_with_layout(body, Layout::left_to_right(Align::Min), |ui| {
                ui.set_height(body.y - 28.0);
                if self.page == 0 {
                    self.types(ui, p, dispatch, &mut outcome);
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.set_width(width - LIST - 14.0);
                        self.fields(ui, p, dispatch, &mut outcome);
                    });
                } else {
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        self.next_page(ui, p);
                    });
                }
            });
            ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                self.footer(ui, p, dispatch, &mut outcome);
            });
            self.type_keys(ui, dispatch);
        });
        self.open_wanted_list(dispatch);
        if self.managing.is_some() {
            self.managed_list_dialog(ui, p, dispatch);
        } else if self.asking_reuse {
            self.reuse_dialog(ui, p, dispatch);
        } else if modal.should_close() && self.job.is_none() {
            outcome = Some(Outcome::Cancelled);
        }
        outcome
    }

    fn title(&self) -> String {
        if self.module.is_some() { t!("ide-new-module-title") } else { t!("ide-new-project-title") }
    }

    fn types(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>, outcome: &mut Option<Outcome>) {
        let mut chosen = None;
        ui.vertical(|ui| {
            ui.set_width(LIST);
            let hint = t!("ide-new-project-search");
            let edit = egui::TextEdit::singleline(&mut self.type_search)
                .id(search_id(ui))
                .hint_text(RichText::new(hint.clone()).color(p.text_secondary))
                .desired_width(LIST - 10.0);
            let response = form::framed(ui, p, |ui| ui.add(edit));
            accessibility::edit(&response, |node| node.set_label(hint));
            let links = 30.0;
            let list = vec2(LIST, (ui.available_height() - links).max(ROW));
            ui.allocate_ui(list, |ui| {
                ScrollArea::vertical().id_salt("new-project-types").auto_shrink([false, false]).show(ui, |ui| {
                    let shown: Vec<GeneratorInfo> = self.shown_types().into_iter().cloned().collect();
                    let types = shown.iter().filter(|generator| generator.section == GeneratorSection::Types);
                    let generators = shown.iter().filter(|generator| generator.section == GeneratorSection::Generators);
                    let sections = [
                        (self.title(), types.collect::<Vec<_>>()),
                        (t!("ide-new-project-generators"), generators.collect()),
                    ];
                    for (title, list) in sections {
                        if list.is_empty() {
                            continue;
                        }
                        ui.add_space(6.0);
                        ui.label(RichText::new(title).font(theme::semibold(12.0)).color(p.text_secondary));
                        for generator in list {
                            if self.type_row(ui, p, generator) {
                                chosen = Some(generator.id.clone());
                            }
                        }
                    }
                });
            });
            let more = egui::Button::new(
                RichText::new(t!("ide-new-project-more-plugins")).font(theme::regular(13.0)).color(p.link),
            )
            .frame(false);
            if ui.add(more).clicked() {
                *outcome = Some(Outcome::MorePlugins);
            }
        });
        if let Some(id) = chosen {
            self.choose(dispatch, id);
        }
    }

    fn type_row(&self, ui: &mut Ui, p: &IdePalette, generator: &GeneratorInfo) -> bool {
        let selected = generator.id == self.selected;
        let (row, response) = ui.allocate_exact_size(vec2(LIST, ROW), Sense::click());
        accessibility::item(&response, Role::ListBoxOption, selected, &generator.name);
        if selected {
            ui.painter().rect_filled(row, 6, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(row, 6, p.hover);
        }
        let icon = Rect::from_center_size(pos2(row.left() + 16.0, row.center().y), Vec2::splat(16.0));
        if generator.icon.is_empty() {
            super::glyphs::glyph_folder(ui.painter(), icon, p.text_secondary);
        } else {
            crate::file_icons::file_icon(ui.painter(), icon, &generator.icon);
        }
        ui.painter().text(
            pos2(row.left() + 32.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            &generator.name,
            theme::regular(13.0),
            p.text,
        );
        response.clicked() && !selected && self.job.is_none()
    }

    pub(super) fn choose(&mut self, dispatch: &mut Dispatch<'_>, id: String) {
        self.selected = id;
        self.page = 0;
        self.error = None;
        self.load_form(dispatch, false);
    }

    fn fields(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>, outcome: &mut Option<Outcome>) {
        let mut connect = false;
        ScrollArea::vertical().id_salt("new-project-fields").auto_shrink([false, false]).show(ui, |ui| {
            let info = self.selected_info().cloned().unwrap_or_default();
            if let Some(description) = &info.description {
                ui.label(RichText::new(description).font(theme::regular(13.0)).color(p.text_secondary));
                ui.add_space(6.0);
            }
            let form = self.forms.get(&self.selected).cloned();
            if let Some(Loaded::Ready(form)) = &form {
                if let Some(server) = &form.server {
                    self.server_row(ui, p, &server.label, &server.url, outcome);
                }
                for field in form.fields.iter().take_while(|field| matches!(field.kind, FieldKind::Note { .. })) {
                    self.note(ui, p, field);
                }
            }
            let mut name = self.check.name.clone();
            ui.horizontal(|ui| {
                form::label(ui, p, &t!("ide-new-project-name"), LABEL);
                let field = form::framed(ui, p, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut name).desired_width(FIELD).id_salt("new-project-name"))
                });
                accessibility::edit(&field, |node| node.set_label(t!("ide-new-project-name")));
                if field.changed() {
                    self.set_name(dispatch, &name);
                }
            });
            self.location_row(ui, p, dispatch);
            if self.module.is_none() {
                ui.horizontal(|ui| {
                    ui.add_space(LABEL);
                    form::check(ui, p, &mut self.git, &t!("ide-new-project-git"));
                });
            }
            if info.network {
                let note =
                    if self.network_allowed { t!("ide-new-project-network") } else { t!("ide-new-project-no-network") };
                ui.horizontal(|ui| {
                    ui.add_space(LABEL);
                    ui.label(RichText::new(note).font(theme::regular(12.0)).color(p.text_secondary));
                });
            }
            if let Some(service) = self.waits_to_connect().map(str::to_owned) {
                ui.horizontal(|ui| {
                    ui.add_space(LABEL);
                    let text = t!("ide-new-project-connects", service = service.as_str());
                    ui.label(RichText::new(text).font(theme::regular(13.0)).color(p.text));
                    if ui.button(t!("ide-new-project-connect")).clicked() {
                        connect = true;
                    }
                });
            }
            ui.add_space(6.0);
            match form {
                Some(Loaded::Ready(form)) => {
                    self.form_fields(ui, p, &form);
                    if let Some(version) = self.wanted_jdk.take() {
                        self.download_jdk(dispatch, version);
                    }
                    if let Some(generator) = self.wanted_generator.take() {
                        self.choose(dispatch, generator);
                    }
                }
                Some(Loaded::Failed(error)) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(error).font(theme::regular(13.0)).color(p.error_text));
                        if ui.button(t!("ide-new-project-retry")).clicked() {
                            self.load_form(dispatch, true);
                        }
                    });
                }
                _ => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(p.text_secondary));
                        let text = t!("ide-new-project-loading", name = info.name.as_str());
                        ui.label(RichText::new(text).font(theme::regular(13.0)).color(p.text_secondary));
                    });
                }
            }
        });
        if connect {
            self.connect(dispatch);
        }
    }

    fn server_row(&self, ui: &mut Ui, p: &IdePalette, label: &str, url: &str, outcome: &mut Option<Outcome>) {
        ui.horizontal(|ui| {
            form::label(ui, p, label, LABEL);
            let shown = url.split_once("://").map_or(url, |(_, rest)| rest).trim_end_matches('/');
            let link = egui::Button::new(RichText::new(shown).font(theme::regular(13.0)).color(p.link)).frame(false);
            if ui.add(link).on_hover_text(url).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            let (gear, response) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
            accessibility::button_with(&response, || t!("ide-new-project-server-settings"));
            super::glyphs::glyph_settings(ui.painter(), gear.shrink(2.0), p.text_secondary);
            if response.clicked() {
                *outcome = Some(Outcome::GeneratorSettings);
            }
        });
    }

    fn location_row(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let module = self.module.is_some();
        let parent = if module {
            super::home_relative(std::path::Path::new(&self.location))
        } else {
            self.check.path.parent().map(super::home_relative).unwrap_or_default()
        };
        let mut text = parent.clone();
        ui.horizontal(|ui| {
            form::label(ui, p, &t!("ide-new-project-location"), LABEL);
            let width = ui.available_width() - 10.0;
            let field = form::framed(ui, p, |ui| {
                let margin = egui::Margin { left: 4, right: 26, top: 2, bottom: 2 };
                ui.add(
                    egui::TextEdit::singleline(&mut text)
                        .margin(margin)
                        .desired_width(width - 22.0)
                        .id_salt("new-project-location"),
                )
            });
            accessibility::edit(&field, |node| node.set_label(t!("ide-new-project-location")));
            if field.changed() && text != parent {
                let folder = from_home(&text);
                let chosen = if module { folder } else { folder.join(&self.check.name) };
                self.set_location(dispatch, chosen.to_string_lossy().into_owned());
            }
            let icon =
                Rect::from_center_size(pos2(field.rect.right() - 14.0, field.rect.center().y), Vec2::splat(16.0));
            let browse = ui.interact(icon, ui.scope_id().with("new-project-browse"), Sense::click());
            accessibility::button_with(&browse, || t!("ide-new-project-browse"));
            super::glyphs::glyph_folder(ui.painter(), icon, p.text_secondary);
            if browse.clicked()
                && let Some(folder) = pick_folder(&self.check.path)
            {
                let chosen = if module { folder } else { folder.join(&self.check.name) };
                self.set_location(dispatch, chosen.to_string_lossy().into_owned());
            }
        });
        let path = super::home_relative(&self.check.path);
        ui.horizontal(|ui| {
            ui.add_space(LABEL);
            let text = if module {
                t!("ide-new-module-where", path = path.as_str())
            } else {
                t!("ide-new-project-created-in", path = path.as_str())
            };
            ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
        });
    }

    fn next_page(&mut self, ui: &mut Ui, p: &IdePalette) {
        let Some(Loaded::Ready(form)) = self.forms.get(&self.selected).cloned() else { return };
        self.form_fields(ui, p, &form);
    }

    fn note(&mut self, ui: &mut Ui, p: &IdePalette, field: &Field) {
        let FieldKind::Note { before, link, after } = &field.kind else { return };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.label(RichText::new(before).font(theme::regular(13.0)).color(p.text_secondary));
            if let Some(link) = link
                && ui.link(RichText::new(&link.label).font(theme::regular(13.0))).clicked()
            {
                self.wanted_generator = Some(link.generator.clone());
            }
            ui.label(RichText::new(after).font(theme::regular(13.0)).color(p.text_secondary));
        });
    }

    fn manage_link(&mut self, ui: &mut Ui, field: &Field) {
        if let Some(list) = &field.manage
            && ui.button(&list.label).clicked()
        {
            self.wanted_list = Some((field.id.clone(), list.clone()));
        }
    }

    #[cfg(test)]
    pub(super) fn open_managed_list(&mut self, dispatch: &mut Dispatch<'_>, field: &str) {
        let Some(Loaded::Ready(form)) = self.forms.get(&self.selected) else { return };
        let found = form.fields.iter().find(|known| known.id == field).and_then(|field| field.manage.clone());
        self.wanted_list = found.map(|list| (field.to_owned(), list));
        self.open_wanted_list(dispatch);
    }

    fn open_wanted_list(&mut self, dispatch: &mut Dispatch<'_>) {
        let Some((field, list)) = self.wanted_list.take() else { return };
        let params = json!({ "generator": self.selected, "list": list.id });
        match dispatch("project.newLists.get", params) {
            Ok(answer) => {
                let rows = serde_json::from_value(answer["rows"].clone()).unwrap_or_default();
                self.managing = Some(Managing { field, list, rows, added: Row::new() });
            }
            Err(error) => self.error = Some(error),
        }
    }

    #[cfg(test)]
    pub(super) fn set_managed_row(&mut self, row: Row) {
        if let Some(managing) = &mut self.managing {
            if managing.list.add_one {
                managing.added = row;
            } else {
                managing.rows.push(row);
            }
        }
    }

    fn save_managed_list(&mut self, dispatch: &mut Dispatch<'_>) {
        let Some(mut managing) = self.managing.take() else { return };
        let keep = |row: &Row| row.values().any(|value| !value.trim().is_empty());
        let added = keep(&managing.added).then(|| managing.added.clone());
        managing.rows.extend(added.clone());
        managing.rows.retain(keep);
        let params = json!({ "generator": self.selected, "list": managing.list.id, "rows": managing.rows });
        if let Err(error) = dispatch("project.newLists.set", params) {
            self.error = Some(error);
            return;
        }
        if let Some(row) = added.filter(|_| !managing.list.value.is_empty()) {
            let value = row.iter().fold(managing.list.value.clone(), |value, (column, text)| {
                value.replace(&format!("${{{column}}}"), text)
            });
            self.values.entry(self.selected.clone()).or_default().insert(managing.field, Value::String(value));
        }
        self.forms.insert(self.selected.clone(), Loaded::Running);
    }

    fn managed_list_dialog(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let mut answer = None;
        let id = ui.scope_id().with("new-project-list");
        let Some(mut managing) = self.managing.take() else { return };
        let modal = egui::Modal::new(id).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new(&managing.list.title).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(8.0);
            if managing.list.add_one {
                for column in &managing.list.columns {
                    let text = managing.added.entry(column.id.clone()).or_default();
                    ui.horizontal(|ui| {
                        form::label(ui, p, &column.label, 110.0);
                        let edit = egui::TextEdit::singleline(text)
                            .desired_width(340.0)
                            .id_salt(("new-project-list", &column.id));
                        let field = form::framed(ui, p, |ui| ui.add(edit));
                        accessibility::edit(&field, |node| node.set_label(column.label.clone()));
                    });
                }
            } else {
                let mut rows: Vec<Value> = managing.rows.iter().map(|row| json!(row)).collect();
                if table(ui, p, &managing.list.id, &managing.list.columns, "", &mut rows) {
                    managing.rows = rows.iter().filter_map(|row| serde_json::from_value(row.clone()).ok()).collect();
                }
            }
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let label = RichText::new(t!("settings-ok")).font(theme::regular(13.0));
                let ok = egui::Button::new(label.color(p.accent_text)).corner_radius(6).fill(p.accent);
                if ui.add(ok.min_size(vec2(84.0, 30.0))).clicked() {
                    answer = Some(true);
                }
                if ui.button(t!("settings-cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
        self.managing = Some(managing);
        if answer == Some(true) {
            self.save_managed_list(dispatch);
        } else if answer == Some(false) || modal.should_close() {
            self.managing = None;
        }
    }

    fn form_fields(&mut self, ui: &mut Ui, p: &IdePalette, form: &GeneratorForm) {
        let resolved = form.resolved(&self.check.name, &self.given());
        if let Some(notice) = form.notice.as_ref().filter(|_| self.page == 0) {
            ui.label(RichText::new(notice).font(theme::regular(12.0)).color(p.text_secondary));
        }
        let page = self.page;
        let (advanced, plain): (Vec<&Field>, Vec<&Field>) = form
            .fields
            .iter()
            .filter(|field| field.page == page && field.visible(&resolved))
            .partition(|field| field.advanced);
        for field in plain {
            self.form_field(ui, p, field, &resolved);
        }
        if advanced.is_empty() {
            return;
        }
        ui.add_space(6.0);
        let header = egui::CollapsingHeader::new(t!("ide-new-project-advanced")).id_salt("new-project-advanced").show(
            ui,
            |ui| {
                for field in advanced {
                    self.form_field(ui, p, field, &resolved);
                }
            },
        );
        let line = header.header_response.rect;
        let x = line.left() + 170.0;
        if x < ui.max_rect().right() {
            ui.painter().hline(x..=ui.max_rect().right(), line.center().y, Stroke::new(1.0, p.hover));
        }
    }

    fn form_field(&mut self, ui: &mut Ui, p: &IdePalette, field: &Field, resolved: &Values) {
        let current = resolved.get(&field.id).cloned().unwrap_or(Value::Null);
        if let Some(value) = self.field(ui, p, field, &current, resolved) {
            self.values.entry(self.selected.clone()).or_default().insert(field.id.clone(), value);
            let depends = self.selected_info().is_some_and(|info| info.form_depends_on.contains(&field.id));
            if depends {
                self.forms.insert(self.selected.clone(), Loaded::Running);
            }
        }
        if let Some(help) = &field.help {
            ui.horizontal(|ui| {
                ui.add_space(LABEL);
                ui.label(RichText::new(help).font(theme::regular(12.0)).color(p.text_secondary));
            });
        }
    }

    fn field(&mut self, ui: &mut Ui, p: &IdePalette, field: &Field, current: &Value, all: &Values) -> Option<Value> {
        let mut changed = None;
        match &field.kind {
            FieldKind::Check { .. } => {
                let mut on = current.as_bool().unwrap_or(false);
                ui.horizontal(|ui| {
                    if form::check(ui, p, &mut on, &field.label).changed() {
                        changed = Some(Value::Bool(on));
                    }
                    tooltip(ui, p, field);
                });
            }
            FieldKind::Text { .. } | FieldKind::Path { .. } => {
                let mut text = current.as_str().unwrap_or_default().to_owned();
                ui.horizontal(|ui| {
                    form::label(ui, p, &field.label, LABEL);
                    let edit =
                        egui::TextEdit::singleline(&mut text).desired_width(FIELD).id_salt(("new-project", &field.id));
                    let response = form::framed(ui, p, |ui| ui.add(edit));
                    accessibility::edit(&response, |node| node.set_label(field.label.clone()));
                    if response.changed() {
                        changed = Some(Value::String(text.clone()));
                    }
                    if matches!(field.kind, FieldKind::Path { .. })
                        && ui.button(t!("ide-new-project-browse")).clicked()
                        && let Some(folder) = pick_folder(std::path::Path::new(&text))
                    {
                        changed = Some(Value::String(folder.to_string_lossy().into_owned()));
                    }
                });
            }
            FieldKind::Choice { options, .. } if field.segmented => {
                let selected = current.as_str().unwrap_or_default();
                ui.horizontal(|ui| {
                    form::label(ui, p, &field.label, LABEL);
                    let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
                    let at = options.iter().position(|option| option.value == selected);
                    if let Some(index) = form::segmented(ui, p, &labels, at) {
                        changed = Some(Value::String(options[index].value.clone()));
                    }
                });
            }
            FieldKind::Choice { options, .. } => {
                let selected = current.as_str().unwrap_or_default();
                let shown = options.iter().find(|option| option.value == selected);
                let shown = shown.map_or(selected, |option| option.label.as_str());
                ui.horizontal(|ui| {
                    form::label(ui, p, &field.label, LABEL);
                    form::combo(ui, p, ("new-project", &field.id), &field.label, shown, CHOICE, |ui| {
                        for option in options {
                            let enabled = option.enabled(all);
                            if !enabled && option.disabled_note.is_none() {
                                continue;
                            }
                            let row = egui::Button::selectable(option.value == selected, option.label.as_str());
                            let row = ui.add_enabled(enabled, row);
                            if row.clicked() {
                                changed = Some(Value::String(option.value.clone()));
                            }
                        }
                    });
                    self.manage_link(ui, field);
                });
            }
            FieldKind::Note { .. } => {}
            FieldKind::Table { columns, empty } => {
                ui.label(RichText::new(&field.label).font(theme::semibold(13.0)).color(p.text));
                let mut rows = current.as_array().cloned().unwrap_or_default();
                if table(ui, p, &field.id, columns, empty, &mut rows) {
                    changed = Some(Value::Array(rows));
                }
            }
            FieldKind::Jdk { options, .. } => {
                let selected = current.as_str().unwrap_or_default();
                let shown = options.iter().find(|option| option.value == selected);
                let none = t!("jdk-none");
                let shown = shown.map_or(none.as_str(), |option| option.label.as_str());
                ui.horizontal(|ui| {
                    form::label(ui, p, &field.label, LABEL);
                    let width = ui.available_width() - 34.0;
                    form::combo(ui, p, ("new-project", &field.id), &field.label, shown, width, |ui| {
                        for option in options {
                            let row = egui::Button::selectable(option.value == selected, option.label.as_str());
                            if ui.add(row).clicked() {
                                changed = Some(Value::String(option.value.clone()));
                            }
                        }
                        if !options.is_empty() {
                            ui.separator();
                        }
                        for version in OFFERED {
                            let label = t!("jdk-download-version", version = version);
                            let enabled = self.downloading_jdk.is_none();
                            if ui.add_enabled(enabled, egui::Button::selectable(false, label)).clicked() {
                                self.wanted_jdk = Some(version);
                            }
                        }
                    });
                });
                if let Some(version) = self.downloading_jdk {
                    ui.horizontal(|ui| {
                        ui.add_space(LABEL);
                        ui.add(egui::Spinner::new().size(14.0).color(p.text_secondary));
                        let text = t!("jdk-download-task", version = version);
                        ui.label(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary));
                    });
                }
            }
            FieldKind::Select { groups, .. } => {
                let mut ticked: BTreeSet<String> = current
                    .as_array()
                    .map(|items| items.iter().filter_map(|item| item.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default();
                if self.select(ui, p, field, groups, &mut ticked, all) {
                    changed = Some(Value::from(ticked.into_iter().collect::<Vec<_>>()));
                }
            }
        }
        changed
    }

    fn select(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        field: &Field,
        groups: &[OptionGroup],
        ticked: &mut BTreeSet<String>,
        all: &Values,
    ) -> bool {
        let mut changed = false;
        ui.label(RichText::new(&field.label).font(theme::regular(13.0)).color(p.text));
        let size = vec2(ui.available_width(), ui.available_height().max(TREE_ROW * 6.0));
        let half = (size.x - 20.0) / 2.0;
        ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Min), |ui| {
            ui.vertical(|ui| {
                ui.set_width(half);
                let search = self.searches.entry(field.id.clone()).or_default();
                let hint = t!("ide-new-project-search");
                let edit = egui::TextEdit::singleline(search)
                    .hint_text(RichText::new(hint.clone()).color(p.text_secondary))
                    .desired_width(half - 10.0)
                    .id_salt(("new-project-search", &field.id));
                let response = form::framed(ui, p, |ui| ui.add(edit));
                accessibility::edit(&response, |node| node.set_label(hint));
                self.searching |= response.has_focus() || response.lost_focus();
                let query = search.trim().to_lowercase();
                ScrollArea::vertical().id_salt(("new-project-select", &field.id)).auto_shrink([false, false]).show(
                    ui,
                    |ui| {
                        for (index, group) in groups.iter().enumerate() {
                            let shown: Vec<&FieldOption> =
                                group.options.iter().filter(|option| option_matches(option, &query)).collect();
                            if !shown.is_empty() && self.group_row(ui, p, group, index == 0, &query) {
                                for option in shown {
                                    changed |= self.option_row(ui, p, option, ticked, all);
                                }
                            }
                        }
                    },
                );
            });
            ui.add_space(20.0);
            ui.vertical(|ui| {
                ui.set_width(half);
                self.option_details(ui, p, groups, all);
                let added = vec2(half, (ui.available_height() - 4.0).max(TREE_ROW));
                ui.allocate_ui_with_layout(added, Layout::top_down(Align::Min), |ui| {
                    added_options(ui, p, groups, ticked)
                });
            });
        });
        changed
    }

    fn group_row(&mut self, ui: &mut Ui, p: &IdePalette, group: &OptionGroup, first: bool, query: &str) -> bool {
        let open = !query.is_empty() || self.groups_open.get(&group.name).copied().unwrap_or(first);
        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), TREE_ROW), Sense::click());
        accessibility::name(&response, Role::DisclosureTriangle, &group.name);
        accessibility::edit(&response, |node| if open { node.set_expanded(true) } else { node.set_expanded(false) });
        if response.hovered() {
            ui.painter().rect_filled(row, 4, p.hover);
        }
        let arrow = Rect::from_center_size(pos2(row.left() + 12.0, row.center().y), Vec2::splat(10.0));
        let points = if open {
            vec![arrow.left_center(), arrow.center_bottom(), arrow.right_center()]
        } else {
            vec![arrow.center_top(), arrow.right_center(), arrow.center_bottom()]
        };
        ui.painter().add(egui::Shape::line(points, Stroke::new(1.2, p.text_secondary)));
        ui.painter().text(
            pos2(row.left() + 26.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            &group.name,
            theme::regular(13.0),
            p.text,
        );
        if response.clicked() {
            self.groups_open.insert(group.name.clone(), !open);
        }
        open
    }

    fn option_row(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        option: &FieldOption,
        ticked: &mut BTreeSet<String>,
        all: &Values,
    ) -> bool {
        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), TREE_ROW), Sense::click());
        if self.highlighted.as_deref() == Some(option.value.as_str()) {
            ui.painter().rect_filled(row, 4, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(row, 4, p.hover);
        }
        let mut on = ticked.contains(&option.value);
        let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(row.with_min_x(row.left() + 40.0)));
        inner.add_enabled_ui(option.enabled(all), |ui| form::check(ui, p, &mut on, &option.label));
        let toggled = on != ticked.contains(&option.value);
        if toggled && on {
            ticked.insert(option.value.clone());
        } else if toggled {
            ticked.remove(&option.value);
        }
        if toggled || response.clicked() {
            self.highlighted = Some(option.value.clone());
        }
        toggled
    }

    fn option_details(&self, ui: &mut Ui, p: &IdePalette, groups: &[OptionGroup], all: &Values) {
        let options = groups.iter().flat_map(|group| &group.options);
        let highlighted = options.clone().find(|option| self.highlighted.as_deref() == Some(option.value.as_str()));
        let height = (ui.available_height() / 2.0).max(TREE_ROW);
        let area = vec2(ui.available_width(), height);
        ui.allocate_ui_with_layout(area, Layout::top_down(Align::Min), |ui| {
            ui.set_min_height(height);
            let Some(option) = highlighted.or_else(|| options.clone().next()) else { return };
            ui.label(RichText::new(&option.label).font(theme::semibold(13.0)).color(p.text));
            let enabled = option.enabled(all);
            let note = if enabled { option.description.as_deref() } else { option.disabled_note.as_deref() };
            if let Some(note) = note {
                ui.add(egui::Label::new(RichText::new(note).font(theme::regular(13.0)).color(p.text)).wrap());
            }
            if let Some(url) = option.web_link() {
                let link = RichText::new(t!("ide-new-project-open-guide")).font(theme::regular(13.0)).color(p.link);
                if ui.add(egui::Button::new(link).frame(false)).on_hover_text(url).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        });
    }

    fn footer(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>, outcome: &mut Option<Outcome>) {
        let reason = self.reason();
        let creating = self.job.is_some();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let cancel =
                egui::Button::new(RichText::new(t!("settings-cancel")).font(theme::regular(13.0)).color(p.text))
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, p.button_border));
            if ui.add(cancel).clicked() {
                match self.job.take() {
                    Some(job) => {
                        let _ = dispatch("project.new.cancel", json!({ "job": job }));
                    }
                    None => *outcome = Some(Outcome::Cancelled),
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let last = self.page >= self.last_page();
                let label = if last { t!("ide-new-project-create") } else { t!("ide-new-project-next") };
                let ready = reason.is_none() && self.job.is_none();
                let primary = egui::Button::new(RichText::new(label).font(theme::regular(13.0)).color(p.accent_text))
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(if ready { p.accent } else { p.accent.gamma_multiply(0.4) });
                if ui.add_enabled(ready, primary).clicked() {
                    self.primary(dispatch);
                }
                if self.page > 0 {
                    let back = egui::Button::new(
                        RichText::new(t!("ide-new-project-previous")).font(theme::regular(13.0)).color(p.text),
                    )
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, p.button_border));
                    if ui.add_enabled(self.job.is_none(), back).clicked() {
                        self.page -= 1;
                    }
                }
                if ready
                    && !self.searching
                    && !self.asking_reuse
                    && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter))
                {
                    self.primary(dispatch);
                }
                if self.job.is_some() {
                    ui.add(egui::Spinner::new().size(14.0).color(p.text_secondary));
                }
            });
        });
        let line = self.error.clone().or_else(|| reason.filter(|reason| !reason.is_empty()));
        if let Some(line) = line {
            ui.add(egui::Label::new(RichText::new(line).font(theme::regular(13.0)).color(p.error_text)).truncate());
        }
        if !self.output.is_empty() {
            output_lines(ui, p, &self.output, creating);
        }
    }

    fn reuse_dialog(&mut self, ui: &mut Ui, p: &IdePalette, dispatch: &mut Dispatch<'_>) {
        let mut answer = None;
        let id = ui.scope_id().with("new-project-reuse");
        let modal = egui::Modal::new(id).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            let path = self.check.path.display().to_string();
            let question = t!("ide-new-project-reuse", path = path.as_str());
            ui.add(egui::Label::new(RichText::new(question).font(theme::semibold(14.0)).color(p.text)).wrap());
            let note = RichText::new(t!("ide-new-project-reuse-note")).font(theme::regular(13.0));
            ui.label(note.color(p.text_secondary));
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let label = RichText::new(t!("ide-new-project-reuse-yes")).font(theme::regular(13.0));
                let yes = egui::Button::new(label.color(p.accent_text)).corner_radius(6).fill(p.accent);
                if ui.add(yes.min_size(vec2(84.0, 30.0))).clicked() {
                    answer = Some(true);
                }
                if ui.button(t!("settings-cancel")).clicked() {
                    answer = Some(false);
                }
            });
        });
        if answer == Some(true) {
            self.asking_reuse = false;
            self.create(dispatch, true);
        } else if answer == Some(false) || modal.should_close() {
            self.asking_reuse = false;
        }
    }
}

fn output_lines(ui: &mut Ui, p: &IdePalette, lines: &[String], following: bool) {
    let height = OUTPUT_HEIGHT - 12.0;
    ui.allocate_ui(vec2(ui.available_width(), height), |ui| {
        ui.set_min_height(height);
        ui.set_max_height(height);
        ScrollArea::both().id_salt("new-project-output").stick_to_bottom(following).auto_shrink([false, false]).show(
            ui,
            |ui| {
                for line in lines {
                    let text = RichText::new(line).font(egui::FontId::monospace(11.5)).color(p.text_secondary);
                    ui.add(egui::Label::new(text).extend());
                }
            },
        );
    });
}

fn table(ui: &mut Ui, p: &IdePalette, id: &str, columns: &[Column], empty: &str, rows: &mut Vec<Value>) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        if ui.button("+").on_hover_text(t!("ide-new-project-add-row")).clicked() {
            rows.push(json!({}));
            changed = true;
        }
        let enabled = !rows.is_empty();
        if ui.add_enabled(enabled, egui::Button::new("−")).on_hover_text(t!("ide-new-project-remove-row")).clicked() {
            rows.pop();
            changed = true;
        }
    });
    let width = (ui.available_width() - 20.0).max(120.0) / columns.len().max(1) as f32 - 8.0;
    if rows.is_empty() {
        let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
        ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, empty, theme::regular(13.0), p.text_secondary);
        return changed;
    }
    ui.horizontal(|ui| {
        for column in columns {
            ui.add_sized(vec2(width, 18.0), egui::Label::new(RichText::new(&column.label).color(p.text_secondary)));
        }
    });
    ScrollArea::vertical().id_salt(("new-project-table", id)).max_height(TABLE_HEIGHT).show(ui, |ui| {
        for (index, row) in rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                for column in columns {
                    let mut text = row[column.id.as_str()].as_str().unwrap_or_default().to_owned();
                    let edit =
                        egui::TextEdit::singleline(&mut text).desired_width(width).id_salt((id, index, &column.id));
                    let field = form::framed(ui, p, |ui| ui.add(edit));
                    accessibility::edit(&field, |node| node.set_label(column.label.clone()));
                    if field.changed() {
                        row[column.id.as_str()] = Value::String(text);
                        changed = true;
                    }
                }
            });
        }
    });
    changed
}

pub(super) fn option_list(
    ui: &mut Ui,
    p: &IdePalette,
    field: &Field,
    groups: &[OptionGroup],
    ticked: &mut BTreeSet<String>,
    all: &Values,
    search: &mut String,
) -> (bool, bool) {
    let mut changed = false;
    ui.label(RichText::new(&field.label).font(theme::regular(13.0)).color(p.text));
    let hint = t!("ide-new-project-search");
    let edit = egui::TextEdit::singleline(search)
        .hint_text(RichText::new(hint.clone()).color(p.text_secondary))
        .desired_width(f32::INFINITY)
        .id_salt(("new-project-search", &field.id));
    let response = form::framed(ui, p, |ui| ui.add(edit));
    accessibility::edit(&response, |node| node.set_label(hint));
    let searching = response.has_focus() || response.lost_focus();
    let query = search.trim().to_lowercase();
    ScrollArea::vertical()
        .id_salt(("new-project-select", &field.id))
        .max_height(SELECT_HEIGHT)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for group in groups {
                let shown: Vec<&FieldOption> =
                    group.options.iter().filter(|option| option_matches(option, &query)).collect();
                if shown.is_empty() {
                    continue;
                }
                ui.label(RichText::new(&group.name).font(theme::semibold(12.0)).color(p.text_secondary));
                for option in shown {
                    changed |= select_row(ui, p, option, ticked, all);
                }
            }
        });
    if !ticked.is_empty() {
        let names: Vec<&str> = groups
            .iter()
            .flat_map(|group| &group.options)
            .filter(|option| ticked.contains(&option.value))
            .map(|option| option.label.as_str())
            .collect();
        let text = t!("ide-new-project-selected", names = names.join(", "));
        ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
    }
    (changed, searching)
}

fn select_row(ui: &mut Ui, p: &IdePalette, option: &FieldOption, ticked: &mut BTreeSet<String>, all: &Values) -> bool {
    let enabled = option.enabled(all);
    let mut on = ticked.contains(&option.value);
    let mut changed = false;
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal(|ui| {
            if form::check(ui, p, &mut on, &option.label).changed() {
                changed = true;
            }
            if let Some(url) = option.web_link()
                && ui.small_button("↗").on_hover_text(url).clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        });
    });
    let note = if enabled { option.description.as_deref() } else { option.disabled_note.as_deref() };
    if let Some(note) = note {
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            ui.add(egui::Label::new(RichText::new(note).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
        });
    }
    if changed {
        if on {
            ticked.insert(option.value.clone());
        } else {
            ticked.remove(&option.value);
        }
    }
    changed
}

fn option_matches(option: &FieldOption, query: &str) -> bool {
    query.is_empty()
        || option.label.to_lowercase().contains(query)
        || option.description.as_deref().is_some_and(|text| text.to_lowercase().contains(query))
}

fn added_options(ui: &mut Ui, p: &IdePalette, groups: &[OptionGroup], ticked: &BTreeSet<String>) {
    ui.label(RichText::new(t!("ide-new-project-added")).font(theme::regular(13.0)).color(p.text));
    let names: Vec<&str> = groups
        .iter()
        .flat_map(|group| &group.options)
        .filter(|option| ticked.contains(&option.value))
        .map(|option| option.label.as_str())
        .collect();
    egui::Frame::new().stroke(Stroke::new(1.0, p.text_field_border)).inner_margin(8).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.set_min_height(ui.available_height());
        if names.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new(t!("ide-new-project-none-added")).font(theme::regular(13.0)).color(p.text_secondary),
                );
            });
            return;
        }
        ScrollArea::vertical().id_salt("new-project-added").auto_shrink([false, false]).show(ui, |ui| {
            for name in names {
                ui.label(RichText::new(name).font(theme::regular(13.0)).color(p.text));
            }
        });
    });
}

fn tooltip(ui: &mut Ui, p: &IdePalette, field: &Field) {
    let Some(text) = &field.tooltip else { return };
    let (mark, response) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
    ui.painter().circle_stroke(mark.center(), 7.0, Stroke::new(1.0, p.text_secondary));
    ui.painter().text(mark.center(), egui::Align2::CENTER_CENTER, "?", theme::regular(11.0), p.text_secondary);
    accessibility::name(&response, Role::Label, text);
    response.on_hover_text(text);
}

fn search_id(ui: &Ui) -> Id {
    ui.ctx().viewport_id().0.with("new-project-type-search")
}

fn from_home(text: &str) -> PathBuf {
    match text.strip_prefix("~/") {
        Some(rest) => {
            std::env::var_os("HOME").map_or_else(|| PathBuf::from(text), |home| PathBuf::from(home).join(rest))
        }
        None => PathBuf::from(text),
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn pick_folder(near: &std::path::Path) -> Option<PathBuf> {
    let mut picker = rfd::FileDialog::new().set_title(t!("ide-new-project-location"));
    if let Some(folder) = near.ancestors().find(|folder| folder.is_dir()) {
        picker = picker.set_directory(folder);
    }
    picker.pick_folder()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn pick_folder(_near: &std::path::Path) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "macos")]
pub(super) fn pick_sources(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_file_or_folder()
}

#[cfg(all(not(target_os = "macos"), not(target_arch = "wasm32")))]
pub(super) fn pick_sources(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

#[cfg(target_arch = "wasm32")]
pub(super) fn pick_sources(_title: &str) -> Option<PathBuf> {
    None
}

pub(super) fn sources_notice(found: &Value) -> String {
    let root = found["root"].as_str().unwrap_or_default();
    let name = std::path::Path::new(root).file_name().map(|name| name.to_string_lossy().into_owned());
    let name = name.unwrap_or_default();
    match found["build"].as_str() {
        Some("maven") => t!("ide-sources-opened", name = name, build = "Maven"),
        Some("gradle") => t!("ide-sources-opened", name = name, build = "Gradle"),
        _ => t!("ide-sources-opened-plain", name = name),
    }
}

pub(super) fn where_to_open(ui: &mut Ui, p: &IdePalette, name: &str) -> Option<Option<bool>> {
    let mut answer = None;
    let id = ui.scope_id().with("open-project-where");
    let modal = egui::Modal::new(id).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        let title = t!("ide-open-project-where", name = name);
        ui.add(egui::Label::new(RichText::new(title).font(theme::semibold(15.0)).color(p.text)).wrap());
        ui.add_space(12.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let new = RichText::new(t!("ide-open-project-new-window")).font(theme::regular(13.0)).color(p.accent_text);
            let new = egui::Button::new(new).corner_radius(6).fill(p.accent).min_size(vec2(84.0, 30.0));
            if ui.add(new).clicked() {
                answer = Some(Some(true));
            }
            if ui.button(t!("ide-open-project-this-window")).clicked() {
                answer = Some(Some(false));
            }
            if ui.button(t!("settings-cancel")).clicked() {
                answer = Some(None);
            }
        });
    });
    answer.or_else(|| modal.should_close().then_some(None))
}

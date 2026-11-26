use std::path::{Path, PathBuf};

use egui::{Color32, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_run::{BeforeLaunchTask, Configuration, FieldKind, KindSpec};
use zen_workbench::Workbench;

use super::glyphs::{
    Glyph, glyph_arrow_down, glyph_arrow_up, glyph_folder, glyph_minus, glyph_pencil, glyph_plus_menu, stroke,
};
use super::{IdePalette, IdeView, Side, hint};
use crate::{accessibility, theme};

const LIST_WIDTH: f32 = 260.0;
const ROW: f32 = 24.0;
const LABEL_WIDTH: f32 = 170.0;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Kind {
    pub(super) provider: String,
    pub(super) spec: KindSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TaskKind {
    pub(super) provider: Option<String>,
    pub(super) name: String,
    pub(super) spec: KindSpec,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct TaskState {
    pub(super) selected: Option<usize>,
    owner: Option<usize>,
    pub(super) editing: bool,
    pub(super) adding: bool,
    pub(super) focus: bool,
    reveal: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Entry {
    pub(super) origin: Option<String>,
    pub(super) configuration: Configuration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum EnvTarget {
    Common,
    Field(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Control {
    Row(usize),
    Add,
    AddKind(usize),
    Remove,
    Copy,
    Save,
    NewFolder,
    Search,
    Templates,
    Template(usize),
    Folder(String, String),
    FolderName,
    MemberAdd,
    MemberAddItem(usize),
    MemberRemove,
    MemberRow(usize),
    Name,
    WorkingDirectory,
    ProgramArguments,
    ToolArguments,
    Field(String),
    Choice(String, String),
    EnvAdd(EnvTarget),
    EnvRemove(EnvTarget, usize),
    EnvName(EnvTarget, usize),
    EnvValue(EnvTarget, usize),
    EnvInline,
    EnvFiles,
    BrowseEnvFiles,
    SystemEnv,
    AllowMultiple,
    ActivateToolWindow,
    FocusToolWindow,
    InsertMacro(Box<Control>),
    Macro(Box<Control>, usize),
    Browse(Box<Control>),
    TaskAdd,
    TaskAddKind(usize),
    TaskRemove,
    TaskEdit,
    TaskUp,
    TaskDown,
    TaskRow(usize),
    TaskField(String),
    TaskChoice(String, String),
    ShowPage,
    Ok,
    Cancel,
    Apply,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Draft {
    pub(super) kinds: Vec<Kind>,
    pub(super) task_kinds: Vec<TaskKind>,
    pub(super) tools: Vec<(String, String)>,
    pub(super) artifacts: Vec<String>,
    pub(super) watchers: Vec<(String, String)>,
    pub(super) original: Vec<Configuration>,
    pub(super) entries: Vec<Entry>,
    pub(super) current: Option<usize>,
    pub(super) error: Option<String>,
    pub(super) tasks: TaskState,
    pub(super) macros: Vec<MacroItem>,
    pub(super) system_env: Vec<(String, String)>,
    adding: bool,
    pub(super) search: String,
    pub(super) folder: Option<(String, String)>,
    pub(super) folder_name: String,
    pub(super) templates: Vec<tree::TemplateEntry>,
    pub(super) template_current: Option<usize>,
    pub(super) editing_templates: bool,
    pub(super) members: tree::MemberState,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub(super) struct MacroItem {
    pub(super) token: String,
    #[serde(default)]
    pub(super) description: String,
    #[serde(default)]
    pub(super) value: Option<String>,
}

pub(super) fn env_text(env: &[(String, String)]) -> String {
    env.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join(";")
}

pub(super) fn parse_env_text(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .filter(|pair| !pair.trim().is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((name, value)) => (name.trim().to_owned(), value.to_owned()),
            None => (pair.trim().to_owned(), String::new()),
        })
        .collect()
}

pub(super) fn env_files_text(files: &[PathBuf]) -> String {
    files.iter().map(|file| file.display().to_string()).collect::<Vec<_>>().join(";")
}

pub(super) fn parse_env_files(text: &str) -> Vec<PathBuf> {
    text.split(';').map(str::trim).filter(|file| !file.is_empty()).map(PathBuf::from).collect()
}

pub(super) fn insert_macro(text: &str, token: &str) -> String {
    if text.is_empty() || text.ends_with(char::is_whitespace) || text.ends_with('/') || text.ends_with('=') {
        format!("{text}{token}")
    } else {
        format!("{text} {token}")
    }
}

impl Draft {
    pub(super) fn load(registry: &Registry, session: &mut Session, show: Option<&str>) -> Self {
        let list = match registry.dispatch(session, "run.configuration.list", json!({})) {
            Ok(list) => list,
            Err(error) => return Self { error: Some(dispatch_message(error)), ..Self::default() },
        };
        let original: Vec<Configuration> = serde_json::from_value(list["configurations"].clone()).unwrap_or_default();
        let kinds = list["kinds"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|kind| {
                Some(Kind {
                    provider: kind["provider"].as_str()?.to_owned(),
                    spec: serde_json::from_value(kind.clone()).ok()?,
                })
            })
            .collect();
        let entries: Vec<Entry> = original
            .iter()
            .map(|configuration| Entry { origin: Some(configuration.id.clone()), configuration: configuration.clone() })
            .collect();
        let wanted = show.or(list["selected"].as_str());
        let current = wanted
            .and_then(|id| entries.iter().position(|entry| entry.origin.as_deref() == Some(id)))
            .or((!entries.is_empty()).then_some(0));
        let types = registry.dispatch(session, "run.beforeLaunch.types", json!({})).unwrap_or_default();
        let task_kinds = types["types"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|kind| {
                Some(TaskKind {
                    provider: kind["provider"].as_str().map(str::to_owned),
                    name: kind["name"].as_str()?.to_owned(),
                    spec: serde_json::from_value(kind.clone()).ok()?,
                })
            })
            .collect();
        let tools = registry.dispatch(session, "run.externalTools.list", json!({})).unwrap_or_default();
        let tools = tools["tools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|tool| Some((tool["id"].as_str()?.to_owned(), tool["name"].as_str()?.to_owned())))
            .collect();
        let artifacts = artifact_names(registry, session);
        let watchers = watcher_choices(registry, session);
        let macros = registry
            .dispatch(session, "run.externalTools.macros", json!({}))
            .ok()
            .and_then(|listed| serde_json::from_value(listed).ok())
            .unwrap_or_default();
        let system_env = registry
            .dispatch(session, "run.environment.system", json!({}))
            .ok()
            .and_then(|listed| serde_json::from_value(listed["variables"].clone()).ok())
            .unwrap_or_default();
        Self {
            kinds,
            task_kinds,
            tools,
            artifacts,
            watchers,
            original,
            entries,
            current,
            error: None,
            tasks: TaskState::default(),
            macros,
            system_env,
            adding: false,
            templates: tree::load_templates(registry, session),
            ..Self::default()
        }
    }

    pub(super) fn selected_task(&self) -> Option<usize> {
        let index = self.tasks.selected.filter(|_| self.tasks.owner == self.current)?;
        (index < self.current_entry()?.configuration.common.before_launch.len()).then_some(index)
    }

    pub(super) fn select_task(&mut self, index: Option<usize>) {
        self.tasks.selected = index;
        self.tasks.owner = self.current;
        self.tasks.editing = false;
        self.tasks.reveal = false;
        self.tasks.focus = true;
    }

    fn task_kind(&self, kind: &str) -> Option<&TaskKind> {
        self.task_kinds.iter().find(|known| known.spec.id == kind)
    }

    pub(super) fn task_label(&self, task: &BeforeLaunchTask) -> String {
        let Some(kind) = self.task_kind(&task.kind) else {
            let id =
                if task.kind == zen_workbench::before_launch::SKIPPED { task.text("foreignId") } else { &task.kind };
            return t!("ide-run-configurations-task-unknown", id = id);
        };
        let summary = kind
            .spec
            .fields
            .iter()
            .filter(|field| !matches!(field.kind, FieldKind::Path | FieldKind::Bool | FieldKind::Env))
            .find_map(|field| {
                if field.kind == FieldKind::Artifacts {
                    let names = chosen_names(task.options.get(&field.key).unwrap_or(&Value::Null));
                    return (!names.is_empty()).then(|| names.join(", "));
                }
                let value = task.text(&field.key).trim();
                if value.is_empty() {
                    return None;
                }
                Some(match &field.kind {
                    FieldKind::ExternalTool => self
                        .tools
                        .iter()
                        .find(|(id, _)| id == value)
                        .map_or_else(|| value.to_owned(), |(_, name)| name.clone()),
                    FieldKind::Choice(_) => message_or(&format!("{}-{value}", field.label_message), value),
                    FieldKind::FileWatcher => self
                        .watchers
                        .iter()
                        .find(|(id, _)| id == value)
                        .map_or_else(|| value.to_owned(), |(_, name)| name.clone()),
                    _ => value.to_owned(),
                })
            });
        match summary {
            Some(summary) => t!("ide-run-configurations-task-label", name = kind.name.as_str(), summary = summary),
            None => kind.name.clone(),
        }
    }

    fn current_tasks(&mut self) -> Option<&mut Vec<BeforeLaunchTask>> {
        let configuration = self.current_mut()?;
        (!configuration.is_adopted()).then_some(&mut configuration.common.before_launch)
    }

    pub(super) fn add_task(&mut self, kind: usize) {
        let Some(kind) = self.task_kinds.get(kind) else { return };
        let task = BeforeLaunchTask::new(&kind.spec.id, kind.spec.defaults.clone());
        let editable = !kind.spec.fields.is_empty();
        let Some(tasks) = self.current_tasks() else { return };
        tasks.push(task);
        let index = tasks.len() - 1;
        self.select_task(Some(index));
        self.tasks.editing = editable;
        self.tasks.reveal = editable;
    }

    pub(super) fn remove_task(&mut self) {
        let Some(index) = self.selected_task() else { return };
        let Some(tasks) = self.current_tasks() else { return };
        tasks.remove(index);
        let left = tasks.len();
        self.select_task((left > 0).then(|| index.min(left - 1)));
    }

    pub(super) fn move_task(&mut self, up: bool) {
        let Some(index) = self.selected_task() else { return };
        let Some(tasks) = self.current_tasks() else { return };
        let Some(other) =
            (if up { index.checked_sub(1) } else { Some(index + 1) }).filter(|other| *other < tasks.len())
        else {
            return;
        };
        tasks.swap(index, other);
        self.select_task(Some(other));
    }

    pub(super) fn edit_task(&mut self) {
        if self.selected_task().is_some() {
            self.tasks.editing = !self.tasks.editing;
            self.tasks.reveal = self.tasks.editing;
            self.tasks.focus = true;
        }
    }

    pub(super) fn kind_of(&self, configuration: &Configuration) -> Option<&Kind> {
        self.kinds.iter().find(|kind| kind.spec.id == configuration.kind)
    }

    pub(super) fn current_entry(&self) -> Option<&Entry> {
        self.entries.get(self.current?)
    }

    fn current_mut(&mut self) -> Option<&mut Configuration> {
        let index = self.current?;
        self.entries.get_mut(index).map(|entry| &mut entry.configuration)
    }

    pub(super) fn add(&mut self, kind: usize) {
        let Some(kind) = self.kinds.get(kind) else { return };
        let template = self.templates.iter().find(|template| template.configuration.kind == kind.spec.id);
        let configuration = Configuration {
            name: unique_name(&self.entries, &t!("ide-run-configurations-unnamed"), None),
            kind: kind.spec.id.clone(),
            options: template
                .map_or_else(|| kind.spec.defaults.clone(), |template| template.configuration.options.clone()),
            common: template.map(|template| template.configuration.common.clone()).unwrap_or_default(),
            ..Configuration::default()
        };
        self.entries.push(Entry { origin: None, configuration });
        self.current = Some(self.entries.len() - 1);
        self.folder = None;
        self.editing_templates = false;
    }

    pub(super) fn remove(&mut self) {
        if self.folder.is_some() {
            self.remove_folder();
            return;
        }
        let Some(index) = self.current.filter(|index| *index < self.entries.len()) else { return };
        if self.entries[index].configuration.is_adopted() {
            return;
        }
        self.entries.remove(index);
        self.current = (!self.entries.is_empty()).then(|| index.min(self.entries.len() - 1));
    }

    pub(super) fn copy(&mut self) {
        let Some(index) = self.current.filter(|index| *index < self.entries.len()) else { return };
        let source = &self.entries[index].configuration;
        let configuration = Configuration {
            id: String::new(),
            name: unique_name(&self.entries, &source.name, Some(1)),
            temporary: false,
            adopted_from: None,
            ..source.clone()
        };
        self.entries.insert(index + 1, Entry { origin: None, configuration });
        self.current = Some(index + 1);
    }

    pub(super) fn save_temporary(&mut self) {
        if let Some(configuration) = self.current_mut() {
            configuration.temporary = false;
            configuration.adopted_from = None;
        }
    }

    pub(super) fn problems(&self, index: usize) -> Vec<(String, bool)> {
        let Some(entry) = self.entries.get(index) else { return Vec::new() };
        let configuration = &entry.configuration;
        let mut problems = Vec::new();
        let name = configuration.name.trim();
        if name.is_empty() {
            problems.push((t!("ide-run-configurations-error-name"), true));
        } else if self.entries.iter().enumerate().any(|(other, kept)| {
            other != index && kept.configuration.kind == configuration.kind && kept.configuration.name.trim() == name
        }) {
            problems.push((t!("ide-run-configurations-error-duplicate", name = name), true));
        }
        if self.kind_of(configuration).is_none() {
            problems.push((t!("ide-run-configurations-error-kind", kind = configuration.kind.as_str()), false));
        }
        if let Some(cycle) = self.compound_problem(index) {
            problems.push((cycle, true));
        }
        problems
    }

    pub(super) fn is_valid(&self) -> bool {
        (0..self.entries.len()).all(|index| self.problems(index).iter().all(|(_, blocking)| !blocking))
    }

    pub(super) fn is_modified(&self) -> bool {
        self.templates.iter().any(|template| template.configuration != template.original)
            || self.entries.len() != self.original.len()
            || self.entries.iter().zip(&self.original).any(|(entry, original)| {
                entry.origin.as_deref() != Some(original.id.as_str()) || entry.configuration != *original
            })
    }

    pub(super) fn apply(&mut self, registry: &Registry, session: &mut Session) -> bool {
        if !self.is_valid() {
            return false;
        }
        let mut changed = false;
        let mut failed = None;
        let mut dispatch =
            |command: &str, params: Value, session: &mut Session| match registry.dispatch(session, command, params) {
                Ok(answer) => {
                    changed = true;
                    Some(answer)
                }
                Err(error) => {
                    failed = Some(dispatch_message(error));
                    None
                }
            };
        for original in self.original.iter().filter(|original| !original.is_adopted()) {
            if !self.entries.iter().any(|entry| entry.origin.as_deref() == Some(original.id.as_str())) {
                dispatch("run.configuration.delete", json!({ "id": original.id }), session);
            }
        }
        let shown = self.current;
        let mut shown_id = None;
        for (index, entry) in self.entries.iter_mut().enumerate() {
            match &entry.origin {
                Some(_) if entry.configuration.is_adopted() => {}
                Some(id) if self.original.iter().any(|original| original.id == *id && original.is_adopted()) => {
                    if let Some(saved) = dispatch("run.configuration.saveAdopted", json!({ "id": id }), session)
                        .and_then(|saved| serde_json::from_value::<Configuration>(saved).ok())
                    {
                        let configuration = Configuration { id: saved.id.clone(), ..entry.configuration.clone() };
                        if configuration != saved {
                            dispatch("run.configuration.update", json!({ "configuration": configuration }), session);
                        }
                        entry.origin = Some(saved.id);
                    }
                }
                Some(id) => {
                    if self.original.iter().find(|original| original.id == *id) != Some(&entry.configuration) {
                        let configuration = Configuration { id: id.clone(), ..entry.configuration.clone() };
                        dispatch("run.configuration.update", json!({ "configuration": configuration }), session);
                    }
                }
                None => {
                    let params = json!({ "configuration": entry.configuration, "template": false });
                    if let Some(id) = dispatch("run.configuration.create", params, session)
                        .and_then(|created| created["id"].as_str().map(str::to_owned))
                    {
                        entry.origin = Some(id);
                    }
                }
            }
            if Some(index) == shown {
                shown_id = entry.origin.clone();
            }
        }
        let (templates_changed, templates_failed) = tree::apply_templates(self, registry, session);
        changed |= templates_changed;
        let error = failed.take().or(templates_failed);
        let (search, editing_templates, template_current) =
            (std::mem::take(&mut self.search), self.editing_templates, self.template_current);
        *self = Self::load(registry, session, shown_id.as_deref());
        self.search = search;
        self.editing_templates = editing_templates;
        self.template_current = template_current;
        if error.is_some() {
            self.error = error;
        }
        changed
    }
}

fn dispatch_message(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

fn unique_name(entries: &[Entry], base: &str, first: Option<usize>) -> String {
    let taken = |name: &str| entries.iter().any(|entry| entry.configuration.name == name);
    if first.is_none() && !taken(base) {
        return base.to_owned();
    }
    (first.unwrap_or(1)..).map(|number| format!("{base} ({number})")).find(|name| !taken(name)).unwrap_or_default()
}

fn message_or(id: &str, fallback: &str) -> String {
    let text = t!(id);
    if text == id { fallback.to_owned() } else { text }
}

fn env_pairs(value: &Value) -> Vec<(String, String)> {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

#[derive(Default)]
pub(super) struct RunConfigurationsDialog {
    open: bool,
    draft: Option<Draft>,
    controls: Vec<(Control, Rect)>,
    show: Option<String>,
    launch: Option<String>,
    launched: Option<(String, String)>,
}

pub(super) fn page_before_start(registry: &Registry, session: &mut Session, params: &Value) -> Option<String> {
    if params.get("configuration").is_some_and(|value| !value.is_null())
        || params.get("context").is_some_and(|value| !value.is_null())
    {
        return None;
    }
    let id = params.get("configurationId").cloned().unwrap_or(Value::Null);
    let answer = registry.dispatch(session, "run.beforeLaunch.page", json!({ "configurationId": id })).ok()?;
    answer["show"].as_str().map(str::to_owned)
}

impl RunConfigurationsDialog {
    pub(super) fn open(&mut self) {
        self.open = true;
        self.draft = None;
        self.show = None;
        self.launch = None;
    }

    pub(super) fn open_to_launch(&mut self, id: &str, action: &str) {
        self.open = true;
        self.draft = None;
        self.show = Some(id.to_owned());
        self.launch = Some(action.to_owned());
    }

    pub(super) fn take_launch(&mut self) -> Option<(String, String)> {
        self.launched.take()
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    #[cfg(test)]
    pub(super) fn draft(&self) -> Option<&Draft> {
        self.draft.as_ref()
    }

    #[cfg(test)]
    pub(super) fn control(&self, control: &Control) -> Option<Rect> {
        self.controls.iter().rev().find(|(shown, _)| shown == control).map(|(_, rect)| *rect)
    }

    pub(super) fn show(&mut self, ui: &mut Ui, p: &IdePalette, registry: &Registry, session: &mut Session) -> bool {
        if !self.open {
            return false;
        }
        let mut draft = self.draft.take().unwrap_or_else(|| Draft::load(registry, session, self.show.as_deref()));
        let root = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned);
        let mut controls = Vec::new();
        let mut pressed = None;
        let modal = egui::Modal::new(ui.scope_id().with("run-configurations")).frame(super::dialog_frame(p)).show(
            ui.ctx(),
            |ui| {
                let screen = ui.ctx().content_rect();
                ui.set_width((screen.width() - 80.0).clamp(640.0, 960.0));
                ui.set_height((screen.height() - 120.0).clamp(420.0, 600.0));
                let rect = ui.max_rect();
                ui.painter().text(
                    rect.left_top(),
                    egui::Align2::LEFT_TOP,
                    t!("ide-run-configurations-title"),
                    theme::semibold(14.0),
                    p.text,
                );
                let body = Rect::from_min_max(rect.left_top() + vec2(0.0, 30.0), rect.right_bottom() - vec2(0.0, 48.0));
                let list = Rect::from_min_size(body.min, vec2(LIST_WIDTH, body.height()));
                ui.painter().rect_filled(list, 6, p.frame);
                list_pane(ui, p, list, &mut draft, &mut controls);
                let page = Rect::from_min_max(pos2(list.right() + 20.0, body.top()), body.max);
                let mut page_ui = ui.new_child(egui::UiBuilder::new().max_rect(page));
                egui::ScrollArea::vertical().id_salt("run-configurations-form").auto_shrink([false, false]).show(
                    &mut page_ui,
                    |ui| {
                        form(ui, p, &mut draft, root.as_deref(), &mut controls);
                    },
                );
                let ok = match self.launch.as_deref() {
                    Some("debug.session.start") => t!("ide-run-configurations-debug"),
                    Some(_) => t!("ide-run-configurations-run"),
                    None => t!("settings-ok"),
                };
                pressed = buttons(ui, p, rect, &ok, draft.is_valid(), draft.is_modified(), &mut controls);
                ui.allocate_rect(rect, Sense::hover());
            },
        );
        if draft.adding
            && let Some(anchor) = controls.iter().find(|(control, _)| *control == Control::Add).map(|(_, rect)| *rect)
        {
            add_menu(ui, p, &mut draft, anchor, &mut controls);
        }
        if draft.members.adding
            && let Some(anchor) =
                controls.iter().find(|(control, _)| *control == Control::MemberAdd).map(|(_, rect)| *rect)
        {
            tree::member_menu(ui, p, &mut draft, anchor, &mut controls);
        }
        if draft.tasks.adding
            && let Some(anchor) =
                controls.iter().find(|(control, _)| *control == Control::TaskAdd).map(|(_, rect)| *rect)
        {
            task_menu(ui, p, &mut draft, anchor, &mut controls);
        }
        task_keys(ui, &mut draft);
        let mut saved = false;
        let mut close = modal.should_close();
        match pressed {
            Some(Control::Apply) => saved = draft.apply(registry, session),
            Some(Control::Ok) => {
                saved = draft.apply(registry, session);
                if draft.error.is_none() {
                    if let Some(id) = draft.current_entry().and_then(|entry| entry.origin.clone()) {
                        let _ = registry.dispatch(session, "run.configuration.select", json!({ "id": id.clone() }));
                        saved = true;
                        if let Some(action) = self.launch.clone() {
                            self.launched = Some((action, id));
                        }
                    }
                    close = true;
                }
            }
            Some(Control::Cancel) => close = true,
            _ => {}
        }
        self.controls = controls;
        if close {
            self.open = false;
            self.draft = None;
        } else {
            self.draft = Some(draft);
        }
        saved
    }
}

fn icon_button(
    ui: &Ui,
    p: &IdePalette,
    rect: Rect,
    glyph: Glyph,
    name: &str,
    enabled: bool,
    control: &Control,
) -> Response {
    let response =
        ui.interact(rect, ui.scope_id().with(("run-configurations-icon", format!("{control:?}"))), Sense::click());
    if enabled && response.hovered() {
        ui.painter().rect_filled(rect, 4, p.popup_hover);
    }
    glyph(ui.painter(), rect.shrink(5.0), if enabled { p.text } else { p.gutter_text });
    hint(ui, &response, name, None, Side::Below);
    if !enabled {
        accessibility::disabled(&response);
    }
    response
}

fn glyph_copy(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let size = rect.size() * 0.7;
    painter.rect_stroke(Rect::from_min_size(rect.min, size), 2.0, s, StrokeKind::Middle);
    painter.rect_stroke(Rect::from_min_size(rect.max - size, size), 2.0, s, StrokeKind::Middle);
}

fn glyph_save(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.rect_stroke(rect, 2.0, s, StrokeKind::Middle);
    let label = Rect::from_min_max(
        pos2(rect.left() + rect.width() * 0.25, rect.top()),
        pos2(rect.right() - rect.width() * 0.25, rect.top() + rect.height() * 0.35),
    );
    painter.rect_stroke(label, 1.0, s, StrokeKind::Middle);
    painter.circle_stroke(rect.center() + vec2(0.0, rect.height() * 0.18), rect.width() * 0.14, s);
}

fn list_pane(ui: &mut Ui, p: &IdePalette, list: Rect, draft: &mut Draft, controls: &mut Vec<(Control, Rect)>) {
    let current_temporary = draft.current_entry().is_some_and(|entry| entry.configuration.temporary);
    let current_adopted = draft.current_entry().is_some_and(|entry| entry.configuration.is_adopted());
    let has_current = draft.current_entry().is_some();
    let save =
        if current_adopted { t!("ide-run-configurations-save-adopted") } else { t!("ide-run-configurations-save") };
    let configurations = !draft.editing_templates;
    let folder = draft.folder.is_some();
    let tools: [(Control, Glyph, String, bool); 5] = [
        (Control::Add, glyph_plus_menu, t!("ide-run-configurations-add"), !draft.kinds.is_empty() && configurations),
        (
            Control::Remove,
            glyph_minus,
            t!("ide-run-configurations-remove"),
            configurations && (folder || (has_current && !current_adopted)),
        ),
        (Control::Copy, glyph_copy, t!("ide-run-configurations-copy"), configurations && has_current && !folder),
        (Control::Save, glyph_save, save, configurations && !folder && (current_temporary || current_adopted)),
        (
            Control::NewFolder,
            glyph_folder,
            t!("ide-run-configurations-new-folder"),
            configurations && !folder && has_current && !current_adopted,
        ),
    ];
    let mut x = list.left() + 6.0;
    for (control, glyph, name, enabled) in tools {
        let rect = Rect::from_min_size(pos2(x, list.top() + 4.0), Vec2::splat(24.0));
        let response = icon_button(ui, p, rect, glyph, &name, enabled, &control);
        controls.push((control.clone(), rect));
        if enabled && response.clicked() {
            match control {
                Control::Add => draft.adding = !draft.adding,
                Control::Remove => draft.remove(),
                Control::Copy => draft.copy(),
                Control::NewFolder => draft.new_folder(),
                _ => draft.save_temporary(),
            }
        }
        x += 28.0;
    }
    let search = Rect::from_min_size(pos2(list.left() + 6.0, list.top() + 34.0), vec2(list.width() - 12.0, ROW));
    if configurations {
        tree::search_row(ui, p, search, draft, controls);
    }
    let link = Rect::from_min_max(pos2(list.left(), list.bottom() - 30.0), list.max);
    tree::templates_link(ui, p, link, draft, controls);
    let rows = Rect::from_min_max(pos2(list.left(), search.bottom() + 6.0), pos2(list.right(), link.top()));
    let mut rows_ui = ui.new_child(egui::UiBuilder::new().max_rect(rows.shrink2(vec2(6.0, 0.0))));
    rows_ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().id_salt("run-configurations-list").auto_shrink([false, false]).show(
        &mut rows_ui,
        |ui| {
            if draft.editing_templates {
                tree::templates_list(ui, p, draft, controls);
            } else {
                tree::configurations_tree(ui, p, draft, controls);
            }
        },
    );
}

fn add_menu(ui: &Ui, p: &IdePalette, draft: &mut Draft, anchor: Rect, controls: &mut Vec<(Control, Rect)>) {
    let mut chosen = None;
    let area = egui::Area::new(ui.scope_id().with("run-configurations-add"))
        .order(egui::Order::Tooltip)
        .fixed_pos(anchor.left_bottom() + vec2(0.0, 4.0))
        .show(ui.ctx(), |ui| {
            super::doc_popups::popup_frame(p).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_min_width(220.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
                ui.painter().text(
                    pos2(header.left() + 8.0, header.center().y),
                    egui::Align2::LEFT_CENTER,
                    t!("ide-run-configurations-add"),
                    theme::semibold(12.5),
                    p.text_secondary,
                );
                for (index, kind) in draft.kinds.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    accessibility::name_with(&response, egui::Role::MenuItem, || t!(&kind.spec.name_message));
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    }
                    ui.painter().text(
                        pos2(rect.left() + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        t!(&kind.spec.name_message),
                        theme::regular(13.0),
                        p.text,
                    );
                    controls.push((Control::AddKind(index), rect));
                    if response.clicked() {
                        chosen = Some(index);
                    }
                }
            });
        });
    if let Some(index) = chosen {
        draft.add(index);
        draft.adding = false;
        return;
    }
    let outside = ui.input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos))
    });
    if outside || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        draft.adding = false;
    }
}

fn task_menu(ui: &Ui, p: &IdePalette, draft: &mut Draft, anchor: Rect, controls: &mut Vec<(Control, Rect)>) {
    let mut chosen = None;
    let area = egui::Area::new(ui.scope_id().with("run-configurations-task-add"))
        .order(egui::Order::Tooltip)
        .fixed_pos(anchor.left_bottom() + vec2(0.0, 4.0))
        .show(ui.ctx(), |ui| {
            super::doc_popups::popup_frame(p).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_min_width(240.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                for (index, kind) in draft.task_kinds.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                    accessibility::name(&response, egui::Role::MenuItem, &kind.name);
                    if response.hovered() {
                        ui.painter().rect_filled(rect, 4, p.selection);
                    }
                    ui.painter().text(
                        pos2(rect.left() + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &kind.name,
                        theme::regular(13.0),
                        p.text,
                    );
                    controls.push((Control::TaskAddKind(index), rect));
                    if response.clicked() {
                        chosen = Some(index);
                    }
                }
            });
        });
    if let Some(index) = chosen {
        draft.add_task(index);
        draft.tasks.adding = false;
        return;
    }
    let outside = ui.input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos))
    });
    if outside || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        draft.tasks.adding = false;
    }
}

fn task_keys(ui: &Ui, draft: &mut Draft) {
    let editable = draft.current_entry().is_some_and(|entry| !entry.configuration.is_adopted());
    if !draft.tasks.focus || !editable || ui.ctx().memory(|memory| memory.focused().is_some()) {
        return;
    }
    let pressed = |key: egui::Key, modifiers: egui::Modifiers| ui.input_mut(|input| input.consume_key(modifiers, key));
    if pressed(egui::Key::N, egui::Modifiers::COMMAND) || pressed(egui::Key::Insert, egui::Modifiers::ALT) {
        draft.tasks.adding = true;
    } else if pressed(egui::Key::Delete, egui::Modifiers::ALT) || pressed(egui::Key::Delete, egui::Modifiers::NONE) {
        draft.remove_task();
    } else if pressed(egui::Key::Enter, egui::Modifiers::NONE) {
        draft.edit_task();
    } else if pressed(egui::Key::ArrowUp, egui::Modifiers::ALT) {
        draft.move_task(true);
    } else if pressed(egui::Key::ArrowDown, egui::Modifiers::ALT) {
        draft.move_task(false);
    }
}

fn label(ui: &mut Ui, p: &IdePalette, text: &str) {
    let (rect, _) = ui.allocate_exact_size(vec2(LABEL_WIDTH, ROW), Sense::hover());
    ui.painter().with_clip_rect(rect).text(
        pos2(rect.left(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        text,
        theme::regular(13.0),
        p.text,
    );
}

fn text_row(
    ui: &mut Ui,
    p: &IdePalette,
    name: &str,
    text: &mut String,
    control: Control,
    controls: &mut Vec<(Control, Rect)>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label(ui, p, name);
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .id_salt(("run-configurations-field", format!("{control:?}")))
                .desired_width(f32::INFINITY),
        );
        accessibility::labelled(&response, name);
        controls.push((control, response.rect));
        changed = response.changed();
    });
    changed
}

fn path_row(
    ui: &mut Ui,
    p: &IdePalette,
    name: &str,
    text: &mut String,
    control: Control,
    root: Option<&Path>,
    controls: &mut Vec<(Control, Rect)>,
) -> bool {
    path_row_with(ui, p, name, text, control, root, &[], controls)
}

#[allow(clippy::too_many_arguments)]
fn path_row_with(
    ui: &mut Ui,
    p: &IdePalette,
    name: &str,
    text: &mut String,
    control: Control,
    root: Option<&Path>,
    macros: &[MacroItem],
    controls: &mut Vec<(Control, Rect)>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label(ui, p, name);
        let buttons = if macros.is_empty() { 32.0 } else { 60.0 };
        let width = (ui.available_width() - buttons).max(80.0);
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .id_salt(("run-configurations-field", format!("{control:?}")))
                .desired_width(width),
        );
        accessibility::labelled(&response, name);
        controls.push((control.clone(), response.rect));
        changed = response.changed();
        let (browse, _) = ui.allocate_exact_size(Vec2::splat(ROW), Sense::hover());
        let browse_control = Control::Browse(Box::new(control.clone()));
        if icon_button(ui, p, browse, glyph_folder, &t!("ide-run-configurations-browse"), true, &browse_control)
            .clicked()
            && let Some(chosen) = browse_for(root, text)
        {
            *text = chosen;
            changed = true;
        }
        controls.push((browse_control, browse));
        changed |= macro_button(ui, p, text, &control, macros, controls);
    });
    changed
}

fn macro_row(
    ui: &mut Ui,
    p: &IdePalette,
    name: &str,
    text: &mut String,
    control: Control,
    macros: &[MacroItem],
    controls: &mut Vec<(Control, Rect)>,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label(ui, p, name);
        let width = (ui.available_width() - 32.0).max(80.0);
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .id_salt(("run-configurations-field", format!("{control:?}")))
                .desired_width(width),
        );
        accessibility::labelled(&response, name);
        controls.push((control.clone(), response.rect));
        changed = response.changed();
        changed |= macro_button(ui, p, text, &control, macros, controls);
    });
    changed
}

fn macro_button(
    ui: &mut Ui,
    p: &IdePalette,
    text: &mut String,
    field: &Control,
    macros: &[MacroItem],
    controls: &mut Vec<(Control, Rect)>,
) -> bool {
    if macros.is_empty() {
        return false;
    }
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(ROW), Sense::hover());
    let control = Control::InsertMacro(Box::new(field.clone()));
    let response =
        icon_button(ui, p, rect, glyph_plus_menu, &t!("ide-run-configurations-insert-macro"), true, &control);
    controls.push((control, rect));
    let mut changed = false;
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(320.0);
        for (index, item) in macros.iter().enumerate() {
            let shown = match &item.value {
                Some(value) => format!("{}  {}", item.token, value),
                None => item.token.clone(),
            };
            let entry = ui.button(shown).on_hover_text(&item.description);
            controls.push((Control::Macro(Box::new(field.clone()), index), entry.rect));
            if entry.clicked() {
                *text = insert_macro(text, &item.token);
                changed = true;
                ui.close();
            }
        }
    });
    changed
}

fn check_row(ui: &mut Ui, value: &mut bool, name: &str, control: Control, controls: &mut Vec<(Control, Rect)>) {
    ui.horizontal(|ui| {
        ui.add_space(LABEL_WIDTH);
        let response = ui.checkbox(value, name);
        controls.push((control, response.rect));
    });
}

fn browse_env_files(root: Option<&Path>) -> Vec<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut dialog = rfd::FileDialog::new();
        if let Some(folder) = root {
            dialog = dialog.set_directory(folder);
        }
        dialog
            .pick_files()
            .unwrap_or_default()
            .into_iter()
            .map(|chosen| {
                let shown = root.and_then(|root| chosen.strip_prefix(root).ok()).map(Path::to_owned).unwrap_or(chosen);
                shown.display().to_string()
            })
            .collect()
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = root;
        Vec::new()
    }
}

fn environment(
    ui: &mut Ui,
    p: &IdePalette,
    common: &mut zen_run::Common,
    system: &[(String, String)],
    root: Option<&Path>,
    controls: &mut Vec<(Control, Rect)>,
) {
    let mut inline = env_text(&common.env);
    if text_row(ui, p, &t!("ide-run-configurations-env-inline"), &mut inline, Control::EnvInline, controls) {
        common.env = parse_env_text(&inline);
    }
    let mut files = env_files_text(&common.env_files);
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-run-configurations-env-files"));
        let width = (ui.available_width() - 32.0).max(80.0);
        let response = ui.add(
            egui::TextEdit::singleline(&mut files)
                .id_salt(("run-configurations-field", "EnvFiles"))
                .desired_width(width),
        );
        accessibility::labelled(&response, &t!("ide-run-configurations-env-files"));
        controls.push((Control::EnvFiles, response.rect));
        let mut changed = response.changed();
        let (browse, _) = ui.allocate_exact_size(Vec2::splat(ROW), Sense::hover());
        let name = t!("ide-run-configurations-env-files-browse");
        if icon_button(ui, p, browse, glyph_folder, &name, true, &Control::BrowseEnvFiles).clicked() {
            let mut chosen = parse_env_files(&files);
            chosen.extend(browse_env_files(root).into_iter().map(PathBuf::from));
            files = env_files_text(&chosen);
            changed = true;
        }
        controls.push((Control::BrowseEnvFiles, browse));
        if changed {
            common.env_files = parse_env_files(&files);
        }
    });
    env_table(ui, p, &t!("ide-run-configurations-env"), EnvTarget::Common, &mut common.env, controls);
    ui.horizontal_top(|ui| {
        ui.add_space(LABEL_WIDTH);
        let header = egui::CollapsingHeader::new(t!("ide-run-configurations-env-system"))
            .id_salt("run-configurations-system-env")
            .default_open(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    egui::Grid::new("run-configurations-system-env-grid").num_columns(2).show(ui, |ui| {
                        for (name, value) in system {
                            ui.label(egui::RichText::new(name).color(p.text_secondary));
                            ui.label(egui::RichText::new(value).color(p.text_secondary));
                            ui.end_row();
                        }
                    });
                });
            });
        controls.push((Control::SystemEnv, header.header_response.rect));
    });
}

fn browse_for(root: Option<&Path>, current: &str) -> Option<String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let start = root.map(|root| root.join(current)).unwrap_or_else(|| PathBuf::from(current));
        let mut dialog = rfd::FileDialog::new();
        if let Some(folder) = root {
            dialog = dialog.set_directory(if start.is_dir() { start.as_path() } else { folder });
        }
        let chosen = if start.is_file() { dialog.pick_file() } else { dialog.pick_folder() }?;
        let shown = root.and_then(|root| chosen.strip_prefix(root).ok()).map(Path::to_owned).unwrap_or(chosen);
        Some(shown.display().to_string())
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (root, current);
        None
    }
}

fn env_table(
    ui: &mut Ui,
    p: &IdePalette,
    name: &str,
    target: EnvTarget,
    env: &mut Vec<(String, String)>,
    controls: &mut Vec<(Control, Rect)>,
) -> bool {
    let mut changed = false;
    ui.horizontal_top(|ui| {
        label(ui, p, name);
        ui.vertical(|ui| {
            let mut removed = None;
            for (index, (key, value)) in env.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let width = ((ui.available_width() - 40.0) / 2.0).max(60.0);
                    let id = ("run-configurations-env", format!("{target:?}"), index);
                    let response = ui.add(
                        egui::TextEdit::singleline(key)
                            .id_salt((id.clone(), "name"))
                            .hint_text(t!("ide-run-configurations-env-name"))
                            .desired_width(width),
                    );
                    accessibility::edit(&response, |node| node.set_label(t!("ide-run-configurations-env-name")));
                    controls.push((Control::EnvName(target.clone(), index), response.rect));
                    changed |= response.changed();
                    let response = ui.add(
                        egui::TextEdit::singleline(value)
                            .id_salt((id, "value"))
                            .hint_text(t!("ide-run-configurations-env-value"))
                            .desired_width(width),
                    );
                    accessibility::edit(&response, |node| node.set_label(t!("ide-run-configurations-env-value")));
                    controls.push((Control::EnvValue(target.clone(), index), response.rect));
                    changed |= response.changed();
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(ROW), Sense::hover());
                    let control = Control::EnvRemove(target.clone(), index);
                    if icon_button(ui, p, rect, glyph_minus, &t!("ide-run-configurations-env-remove"), true, &control)
                        .clicked()
                    {
                        removed = Some(index);
                    }
                    controls.push((control, rect));
                });
            }
            if let Some(index) = removed {
                env.remove(index);
                changed = true;
            }
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(ROW), Sense::hover());
            let control = Control::EnvAdd(target.clone());
            if icon_button(ui, p, rect, glyph_plus, &t!("ide-run-configurations-env-add"), true, &control).clicked() {
                env.push((String::new(), String::new()));
                changed = true;
            }
            controls.push((control, rect));
        });
    });
    changed
}

fn glyph_plus(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.line_segment([rect.left_center(), rect.right_center()], s);
    painter.line_segment([rect.center_top(), rect.center_bottom()], s);
}

fn form(ui: &mut Ui, p: &IdePalette, draft: &mut Draft, root: Option<&Path>, controls: &mut Vec<(Control, Rect)>) {
    if let Some(error) = &draft.error {
        ui.label(egui::RichText::new(error).color(p.problem("error")));
        ui.add_space(8.0);
    }
    if draft.editing_templates {
        let names = other_names(draft, usize::MAX);
        let tools = draft.tools.clone();
        let (artifacts, watchers) = (draft.artifacts.clone(), draft.watchers.clone());
        let (macros, system) = (draft.macros.clone(), draft.system_env.clone());
        let refs = Refs { configurations: &names, tools: &tools, artifacts: &artifacts, watchers: &watchers };
        tree::template_form(ui, p, draft, root, &refs, &macros, &system, controls);
        return;
    }
    if draft.folder.is_some() {
        tree::folder_form(ui, p, draft, controls);
        return;
    }
    let Some(index) = draft.current.filter(|index| *index < draft.entries.len()) else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new(t!("ide-run-configurations-empty")).color(p.text_secondary));
        });
        return;
    };
    if zen_workbench::compound::is_compound(&draft.entries[index].configuration) {
        if let Some(file) = &draft.entries[index].configuration.adopted_from {
            let shown = root.and_then(|root| file.strip_prefix(root).ok()).unwrap_or(file);
            let note = t!("ide-run-configurations-adopted", file = shown.display().to_string());
            ui.label(egui::RichText::new(note).color(p.text_secondary));
        }
        tree::compound_form(ui, p, draft, index, controls);
        return;
    }
    let kind = draft.kind_of(&draft.entries[index].configuration).cloned();
    let problems = draft.problems(index);
    let names = other_names(draft, index);
    let tools = draft.tools.clone();
    let artifacts = draft.artifacts.clone();
    let watchers = draft.watchers.clone();
    let refs = Refs { configurations: &names, tools: &tools, artifacts: &artifacts, watchers: &watchers };
    let configuration = &mut draft.entries[index].configuration;
    ui.spacing_mut().item_spacing.y = 8.0;
    if let Some(file) = &configuration.adopted_from {
        let shown = root.and_then(|root| file.strip_prefix(root).ok()).unwrap_or(file);
        let note = t!("ide-run-configurations-adopted", file = shown.display().to_string());
        ui.label(egui::RichText::new(note).color(p.text_secondary));
    }
    let editable = !configuration.is_adopted();
    let (macros, system) = (&draft.macros, &draft.system_env);
    ui.add_enabled_ui(editable, |ui| {
        fields(ui, p, configuration, kind.as_ref(), root, &refs, macros, system, true, controls)
    });
    ui.add_enabled_ui(editable, |ui| before_launch(ui, p, draft, index, root, &refs, controls));
    for (problem, _) in problems {
        ui.label(egui::RichText::new(problem).color(p.problem("error")));
    }
}

fn other_names(draft: &Draft, index: usize) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (other, entry) in draft.entries.iter().enumerate() {
        let name = entry.configuration.name.trim();
        if other != index && !name.is_empty() && !names.iter().any(|kept| kept == name) {
            names.push(name.to_owned());
        }
    }
    names
}

struct Refs<'a> {
    configurations: &'a [String],
    tools: &'a [(String, String)],
    artifacts: &'a [String],
    watchers: &'a [(String, String)],
}

fn artifact_names(registry: &Registry, session: &mut Session) -> Vec<String> {
    let Ok(section) = registry.dispatch(session, "project.structure.artifacts", json!({})) else {
        return Vec::new();
    };
    let names = |key: &str| -> Vec<String> {
        section[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|artifact| artifact["name"].as_str().map(str::to_owned))
            .collect()
    };
    let kept = names("artifacts");
    if kept.is_empty() { names("suggestions") } else { kept }
}

fn chosen_names(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(|name| name.as_str().map(str::to_owned)).collect()
}

#[allow(clippy::too_many_arguments, reason = "a field, its value and where its controls go")]
fn artifact_choices(
    ui: &mut Ui,
    p: &IdePalette,
    name: &str,
    key: &str,
    value: &Value,
    known: &[String],
    scope: Scope,
    controls: &mut Vec<(Control, Rect)>,
) -> Option<Value> {
    let chosen = chosen_names(value);
    let mut listed: Vec<String> = known.to_vec();
    listed.extend(chosen.iter().filter(|name| !known.contains(name)).cloned());
    let mut changed = None;
    ui.horizontal_top(|ui| {
        label(ui, p, name);
        let column = ui.vertical(|ui| {
            if listed.is_empty() {
                ui.label(egui::RichText::new(t!("ide-run-configurations-no-artifacts")).color(p.text_secondary));
            }
            for artifact in &listed {
                let mut on = chosen.contains(artifact);
                let response = ui.checkbox(&mut on, artifact.as_str());
                controls.push((scope.choice(key, artifact), response.rect));
                if response.changed() {
                    changed = Some((artifact.clone(), on));
                }
            }
        });
        controls.push((scope.field(key), column.response.rect));
    });
    let (artifact, on) = changed?;
    let picked: Vec<String> =
        listed.into_iter().filter(|kept| if *kept == artifact { on } else { chosen.contains(kept) }).collect();
    Some(json!(picked))
}

fn watcher_choices(registry: &Registry, session: &mut Session) -> Vec<(String, String)> {
    let listed = registry.dispatch(session, "run.fileWatchers.list", json!({})).unwrap_or_default();
    listed["watchers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|watcher| {
            let id = format!("{}:{}", watcher["level"].as_str()?, watcher["id"].as_str()?);
            Some((id, watcher["name"].as_str()?.to_owned()))
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Scope {
    Configuration,
    Task,
}

impl Scope {
    fn field(self, key: &str) -> Control {
        match self {
            Self::Configuration => Control::Field(key.to_owned()),
            Self::Task => Control::TaskField(key.to_owned()),
        }
    }

    fn choice(self, key: &str, value: &str) -> Control {
        match self {
            Self::Configuration => Control::Choice(key.to_owned(), value.to_owned()),
            Self::Task => Control::TaskChoice(key.to_owned(), value.to_owned()),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fields(
    ui: &mut Ui,
    p: &IdePalette,
    configuration: &mut Configuration,
    kind: Option<&Kind>,
    root: Option<&Path>,
    refs: &Refs<'_>,
    macros: &[MacroItem],
    system: &[(String, String)],
    named: bool,
    controls: &mut Vec<(Control, Rect)>,
) {
    if named {
        text_row(ui, p, &t!("ide-run-configurations-name"), &mut configuration.name, Control::Name, controls);
    }
    let multiple = t!("ide-run-configurations-allow-multiple");
    check_row(ui, &mut configuration.common.allow_multiple_instances, &multiple, Control::AllowMultiple, controls);
    ui.add_space(4.0);
    if let Some(kind) = kind {
        ui.label(egui::RichText::new(t!(&kind.spec.name_message)).font(theme::semibold(13.0)).color(p.text));
        if !configuration.options.is_object() {
            configuration.options = json!({});
        }
        if let Some(options) = configuration.options.as_object_mut() {
            for field in &kind.spec.fields {
                option_field(ui, p, options, field, root, Scope::Configuration, refs, controls);
            }
        }
    }
    let mut folder =
        configuration.common.working_directory.as_ref().map(|folder| folder.display().to_string()).unwrap_or_default();
    if path_row_with(
        ui,
        p,
        &t!("ide-run-configurations-working-dir"),
        &mut folder,
        Control::WorkingDirectory,
        root,
        macros,
        controls,
    ) {
        configuration.common.working_directory = (!folder.is_empty()).then(|| PathBuf::from(&folder));
    }
    environment(ui, p, &mut configuration.common, system, root, controls);
    let tool = kind.map_or_else(
        || t!("ide-run-configurations-tool-args"),
        |kind| message_or(&format!("{}-run-tool-arguments", kind.provider), &t!("ide-run-configurations-tool-args")),
    );
    macro_row(ui, p, &tool, &mut configuration.common.tool_arguments, Control::ToolArguments, macros, controls);
    macro_row(
        ui,
        p,
        &t!("ide-run-configurations-program-args"),
        &mut configuration.common.program_arguments,
        Control::ProgramArguments,
        macros,
        controls,
    );
    let common = &mut configuration.common;
    let activate = t!("ide-run-configurations-activate-tool-window");
    check_row(ui, &mut common.activate_tool_window, &activate, Control::ActivateToolWindow, controls);
    let focus = t!("ide-run-configurations-focus-tool-window");
    check_row(ui, &mut common.focus_tool_window, &focus, Control::FocusToolWindow, controls);
}

#[allow(clippy::too_many_arguments, reason = "a field, its value and where its controls go")]
fn option_field(
    ui: &mut Ui,
    p: &IdePalette,
    options: &mut serde_json::Map<String, Value>,
    field: &zen_run::FieldSpec,
    root: Option<&Path>,
    scope: Scope,
    refs: &Refs<'_>,
    controls: &mut Vec<(Control, Rect)>,
) {
    let name = t!(&field.label_message);
    let value = options.get(&field.key).cloned().unwrap_or(Value::Null);
    let control = scope.field(&field.key);
    let updated = match &field.kind {
        FieldKind::Text => {
            let mut text = value.as_str().unwrap_or_default().to_owned();
            text_row(ui, p, &name, &mut text, control, controls).then_some(Value::String(text))
        }
        FieldKind::Path => {
            let mut text = value.as_str().unwrap_or_default().to_owned();
            path_row(ui, p, &name, &mut text, control, root, controls).then_some(Value::String(text))
        }
        FieldKind::Bool => {
            let mut on = value.as_bool().unwrap_or(false);
            let response = ui.checkbox(&mut on, name);
            controls.push((control, response.rect));
            response.changed().then_some(Value::Bool(on))
        }
        FieldKind::Choice(values) => {
            let shown = |choice: &str| message_or(&format!("{}-{choice}", field.label_message), choice);
            let choices: Vec<(String, String)> = values.iter().map(|choice| (choice.clone(), shown(choice))).collect();
            let current = value.as_str().map(str::to_owned).or_else(|| values.first().cloned()).unwrap_or_default();
            combo(ui, p, &name, &field.key, &current, &choices, scope, controls)
                .filter(|choice| *choice != current || value.is_null())
                .map(Value::String)
        }
        FieldKind::Configuration => {
            let choices: Vec<(String, String)> =
                refs.configurations.iter().map(|name| (name.clone(), name.clone())).collect();
            let current = value.as_str().unwrap_or_default().to_owned();
            combo(ui, p, &name, &field.key, &current, &choices, scope, controls).map(Value::String)
        }
        FieldKind::ExternalTool => {
            let current = value.as_str().unwrap_or_default().to_owned();
            combo(ui, p, &name, &field.key, &current, refs.tools, scope, controls).map(Value::String)
        }
        FieldKind::Artifacts => artifact_choices(ui, p, &name, &field.key, &value, refs.artifacts, scope, controls),
        FieldKind::FileWatcher => {
            let mut choices = vec![(String::new(), t!("ide-run-configurations-all-file-watchers"))];
            choices.extend(refs.watchers.iter().cloned());
            let current = value.as_str().unwrap_or_default().to_owned();
            combo(ui, p, &name, &field.key, &current, &choices, scope, controls).map(Value::String)
        }
        FieldKind::Env => {
            let mut env = env_pairs(&value);
            env_table(ui, p, &name, EnvTarget::Field(field.key.clone()), &mut env, controls).then(|| json!(env))
        }
    };
    if let Some(updated) = updated {
        options.insert(field.key.clone(), updated);
    }
}

#[allow(clippy::too_many_arguments, reason = "a combo box, its choices and where its controls go")]
fn combo(
    ui: &mut Ui,
    p: &IdePalette,
    name: &str,
    key: &str,
    current: &str,
    choices: &[(String, String)],
    scope: Scope,
    controls: &mut Vec<(Control, Rect)>,
) -> Option<String> {
    let shown = choices.iter().find(|(value, _)| value == current).map_or(current, |(_, shown)| shown.as_str());
    let mut chosen = None;
    ui.horizontal(|ui| {
        label(ui, p, name);
        let salt = match scope {
            Scope::Configuration => "run-configurations-choice",
            Scope::Task => "run-configurations-task-choice",
        };
        let combo = egui::ComboBox::from_id_salt((salt, key)).selected_text(shown).width(220.0).show_ui(ui, |ui| {
            for (value, text) in choices {
                let response = ui.selectable_label(value == current, text);
                controls.push((scope.choice(key, value), response.rect));
                if response.clicked() {
                    chosen = Some(value.clone());
                }
            }
        });
        accessibility::labelled(&combo.response, name);
        controls.push((scope.field(key), combo.response.rect));
    });
    chosen
}

fn before_launch(
    ui: &mut Ui,
    p: &IdePalette,
    draft: &mut Draft,
    index: usize,
    root: Option<&Path>,
    refs: &Refs<'_>,
    controls: &mut Vec<(Control, Rect)>,
) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(t!("ide-run-configurations-before-launch")).font(theme::semibold(13.0)).color(p.text));
    let selected = draft.selected_task();
    let count = draft.entries[index].configuration.common.before_launch.len();
    let has_fields = selected
        .and_then(|task| draft.entries[index].configuration.common.before_launch.get(task))
        .and_then(|task| draft.task_kind(&task.kind))
        .is_some_and(|kind| !kind.spec.fields.is_empty());
    let tools: [(Control, Glyph, String, bool); 5] = [
        (Control::TaskAdd, glyph_plus_menu, t!("ide-run-configurations-task-add"), !draft.task_kinds.is_empty()),
        (Control::TaskRemove, glyph_minus, t!("ide-run-configurations-task-remove"), selected.is_some()),
        (Control::TaskEdit, glyph_pencil, t!("ide-run-configurations-task-edit"), has_fields),
        (Control::TaskUp, glyph_arrow_up, t!("ide-run-configurations-task-up"), selected.is_some_and(|task| task > 0)),
        (
            Control::TaskDown,
            glyph_arrow_down,
            t!("ide-run-configurations-task-down"),
            selected.is_some_and(|task| task + 1 < count),
        ),
    ];
    let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let mut x = bar.left();
    for (control, glyph, name, enabled) in tools {
        let rect = Rect::from_min_size(pos2(x, bar.top() + 1.0), Vec2::splat(24.0));
        let response = icon_button(ui, p, rect, glyph, &name, enabled, &control);
        controls.push((control.clone(), rect));
        if enabled && response.clicked() {
            draft.tasks.focus = true;
            match control {
                Control::TaskAdd => draft.tasks.adding = !draft.tasks.adding,
                Control::TaskRemove => draft.remove_task(),
                Control::TaskEdit => draft.edit_task(),
                Control::TaskUp => draft.move_task(true),
                _ => draft.move_task(false),
            }
        }
        x += 28.0;
    }
    let tasks = draft.entries[index].configuration.common.before_launch.clone();
    let labels: Vec<String> = tasks.iter().map(|task| draft.task_label(task)).collect();
    let selected = draft.selected_task();
    let height = labels.len().max(1) as f32 * ROW + 8.0;
    let (list, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    ui.painter().rect_filled(list, 6, p.frame);
    if labels.is_empty() {
        ui.painter().text(
            pos2(list.left() + 10.0, list.center().y),
            egui::Align2::LEFT_CENTER,
            t!("ide-run-configurations-tasks-empty"),
            theme::regular(13.0),
            p.text_secondary,
        );
    }
    for (task, text) in labels.iter().enumerate() {
        let row = Rect::from_min_size(list.min + vec2(4.0, 4.0 + task as f32 * ROW), vec2(list.width() - 8.0, ROW));
        let response = ui.interact(row, ui.scope_id().with(("run-configurations-task", task)), Sense::click());
        if selected == Some(task) {
            ui.painter().rect_filled(row, 4, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(row, 4, p.hover);
        }
        accessibility::name(&response, egui::Role::ListItem, text);
        ui.painter().with_clip_rect(row).text(
            pos2(row.left() + 8.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            theme::regular(13.0),
            p.text,
        );
        controls.push((Control::TaskRow(task), row));
        if response.double_clicked() {
            draft.select_task(Some(task));
            draft.tasks.editing = true;
            draft.tasks.reveal = true;
        } else if response.clicked() {
            draft.select_task(Some(task));
        }
    }
    if draft.tasks.editing
        && let Some(task) = draft.selected_task()
        && let Some(kind) = draft.task_kind(&tasks[task].kind).cloned()
    {
        let options = &mut draft.entries[index].configuration.common.before_launch[task].options;
        let shown = ui
            .vertical(|ui| {
                ui.label(egui::RichText::new(&kind.name).font(theme::semibold(12.5)).color(p.text_secondary));
                for field in &kind.spec.fields {
                    option_field(ui, p, options, field, root, Scope::Task, refs, controls);
                }
            })
            .response;
        if std::mem::take(&mut draft.tasks.reveal) {
            ui.scroll_to_rect_animation(shown.rect, None, egui::style::ScrollAnimation::none());
        }
    }
    let configuration = &mut draft.entries[index].configuration;
    let response = ui.checkbox(&mut configuration.common.show_page, t!("ide-run-configurations-show-page"));
    controls.push((Control::ShowPage, response.rect));
}

fn buttons(
    ui: &mut Ui,
    p: &IdePalette,
    rect: Rect,
    ok: &str,
    valid: bool,
    modified: bool,
    controls: &mut Vec<(Control, Rect)>,
) -> Option<Control> {
    let mut pressed = None;
    let mut right = rect.right();
    let bottom = rect.bottom() - 34.0;
    for (control, text, primary, enabled) in [
        (Control::Apply, t!("settings-apply"), false, valid && modified),
        (Control::Cancel, t!("settings-cancel"), false, true),
        (Control::Ok, ok.to_owned(), true, valid),
    ] {
        let color = if !enabled {
            p.text_secondary
        } else if primary {
            p.accent_text
        } else {
            p.text
        };
        let galley = ui.painter().layout_no_wrap(text, theme::regular(13.0), color);
        let button =
            Rect::from_min_size(pos2(right - galley.size().x - 28.0, bottom), vec2(galley.size().x + 28.0, 30.0));
        let response = ui.interact(
            button,
            ui.scope_id().with(("run-configurations-button", format!("{control:?}"))),
            Sense::click(),
        );
        accessibility::name(
            &response,
            if primary { egui::Role::DefaultButton } else { egui::Role::Button },
            galley.text(),
        );
        if !enabled {
            accessibility::disabled(&response);
        }
        let fill = if primary && enabled {
            p.accent
        } else if enabled && response.hovered() {
            p.popup_hover
        } else {
            p.island
        };
        ui.painter().rect_filled(button, 6, fill);
        if !primary || !enabled {
            ui.painter().rect_stroke(button, 6, Stroke::new(1.0, p.tab_border), StrokeKind::Inside);
        }
        ui.painter().galley(button.center() - galley.size() / 2.0, galley, color);
        controls.push((control.clone(), button));
        if enabled && response.clicked() {
            pressed = Some(control);
        }
        right = button.left() - 8.0;
    }
    pressed
}

impl IdeView {
    pub(super) fn run_configurations_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        if self.run_configurations.show(ui, &p, registry, session) {
            self.run_widget.invalidate();
        }
        if let Some((action, id)) = self.run_configurations.take_launch()
            && let Err(super::agent::ActionError::Dispatch(error)) =
                self.launch_run(registry, session, &action, json!({ "configurationId": id }), false)
        {
            self.report(ui, error);
        }
    }
}

#[cfg(test)]
mod tests;
mod tree;

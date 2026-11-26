use egui::{Align, Layout, RichText, Role, Ui};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::csv_import::{LoadSettings, OnError};
use zen_i18n::t;

use super::{IdePalette, IdeView, dialog_frame, vcs};
use crate::{accessibility, theme};

pub(super) const OPEN: &str = "view.database.transfer";

const WIDTH: f32 = 680.0;
const LIST_HEIGHT: f32 = 150.0;
const MAPPING_HEIGHT: f32 = 260.0;
const BODY_HEIGHT: f32 = 340.0;
const LABEL_WIDTH: f32 = 170.0;
const LISTED: usize = 500;
const CONNECTIONS: usize = 200;

#[derive(Default)]
pub(super) struct TransferRequest(pub(super) Option<Value>);

pub(super) fn request(session: &mut Session, params: Value) {
    session.state_mut::<TransferRequest>().0 = Some(params);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Target,
    Mapping,
    Settings,
    Confirm,
}

impl Step {
    const ALL: [Step; 4] = [Step::Target, Step::Mapping, Step::Settings, Step::Confirm];

    fn title(self) -> String {
        match self {
            Step::Target => t!("ide-dbt-step-target"),
            Step::Mapping => t!("ide-dbt-step-mapping"),
            Step::Settings => t!("ide-dbt-step-settings"),
            Step::Confirm => t!("ide-dbt-step-confirm"),
        }
    }

    fn at(self) -> usize {
        Self::ALL.iter().position(|step| *step == self).unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
struct Source {
    path: Vec<String>,
    chosen: bool,
}

#[derive(Clone, Debug)]
struct Column {
    source: String,
    source_type: String,
    target: String,
    sql_type: String,
    copied: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Planned {
    target: String,
    schema: Option<Vec<String>>,
    tables: Vec<Vec<String>>,
}

#[derive(Clone, Debug)]
struct Target {
    id: String,
    name: String,
    read_only: bool,
}

#[derive(Clone, Debug)]
struct Mapped {
    source: Vec<String>,
    name: String,
    exists: bool,
    rows: Option<i64>,
    columns: Vec<Column>,
}

#[derive(Clone, Debug)]
pub(super) struct TransferDialog {
    connection: String,
    sources: Vec<Source>,
    connections: Vec<Target>,
    target: String,
    schemas: Vec<Vec<String>>,
    schema: Option<Vec<String>>,
    planned: Option<Planned>,
    loading: bool,
    mapping: Vec<Mapped>,
    load: LoadSettings,
    batch: String,
    commit_every: String,
    remembered: bool,
    step: Step,
    error: Option<String>,
    confirm: Option<(String, String, Vec<String>)>,
    running: Option<u64>,
    progress: Value,
    fresh: bool,
}

fn message(error: zen_core::DispatchError) -> (Option<String>, String, Value) {
    match error {
        zen_core::DispatchError::Failed { error, .. } => {
            (error.code.clone(), error.message.clone(), error.data.clone().map(Value::Object).unwrap_or(Value::Null))
        }
        other => (None, other.to_string(), Value::Null),
    }
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

fn schema_name(schema: &[String]) -> String {
    schema.get(1).or_else(|| schema.first()).cloned().unwrap_or_default()
}

impl TransferDialog {
    pub(super) fn open(registry: &Registry, session: &mut Session, params: &Value) -> Result<Self, String> {
        let (connection, path) = match params["connection"].as_str() {
            Some(connection) => (connection.to_owned(), strings(&params["path"])),
            None => {
                let state = registry.dispatch(session, "db.navigator.state", json!({ "limit": 1 })).unwrap_or_default();
                let selected = &state["selected"];
                match selected["connection"].as_str() {
                    Some(connection) => (connection.to_owned(), strings(&selected["path"])),
                    None => return Err(t!("ide-dbt-choose-tables")),
                }
            }
        };
        let sources = match path.as_slice() {
            [_, _, folder, _] if folder == "tables" => vec![Source { path, chosen: true }],
            [catalog, schema] | [catalog, schema, _] => {
                let folder = vec![catalog.clone(), schema.clone(), "tables".to_owned()];
                let params = json!({ "connection": connection, "path": folder, "limit": LISTED });
                let answer =
                    registry.dispatch(session, "db.schema.children", params).map_err(|error| message(error).1)?;
                answer["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|node| node["kind"] == "table")
                    .map(|node| Source { path: strings(&node["path"]), chosen: true })
                    .filter(|source| source.path.len() == 4)
                    .collect()
            }
            _ => return Err(t!("ide-dbt-choose-tables")),
        };
        if sources.is_empty() {
            return Err(t!("ide-dbt-no-tables"));
        }
        let listed =
            registry.dispatch(session, "db.dataSource.list", json!({ "limit": CONNECTIONS })).unwrap_or_default();
        let types = registry.dispatch(session, "db.connectionType.list", json!({})).unwrap_or_default();
        let read_only_type = |id: &str| {
            types["types"].as_array().into_iter().flatten().any(|kind| kind["id"] == id && kind["readOnly"] == true)
        };
        let connections: Vec<Target> = listed["connections"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|known| {
                Some(Target {
                    id: known["id"].as_str()?.to_owned(),
                    name: known["name"].as_str()?.to_owned(),
                    read_only: known["readOnly"] == true || read_only_type(known["type"].as_str().unwrap_or_default()),
                })
            })
            .collect();
        let target = connections
            .iter()
            .find(|known| known.id != connection && !known.read_only)
            .map(|known| known.id.clone())
            .unwrap_or_default();
        let load = LoadSettings::default();
        Ok(Self {
            connection,
            sources,
            connections,
            target,
            schemas: Vec::new(),
            schema: None,
            planned: None,
            loading: false,
            mapping: Vec::new(),
            batch: load.batch.to_string(),
            commit_every: load.commit_every.to_string(),
            load,
            remembered: false,
            step: Step::Target,
            error: None,
            confirm: None,
            running: None,
            progress: Value::Null,
            fresh: true,
        })
    }

    fn chosen(&self) -> Vec<Vec<String>> {
        self.sources.iter().filter(|source| source.chosen).map(|source| source.path.clone()).collect()
    }

    fn name_of(&self, id: &str) -> String {
        self.connections.iter().find(|known| known.id == id).map_or_else(|| id.to_owned(), |known| known.name.clone())
    }

    fn read_only(&self, id: &str) -> bool {
        self.connections.iter().any(|known| known.id == id && known.read_only)
    }

    fn reset_load(&mut self) {
        let load = LoadSettings::default();
        self.batch = load.batch.to_string();
        self.commit_every = load.commit_every.to_string();
        self.load = load;
        self.remembered = false;
    }

    fn refresh(&mut self, registry: &Registry, session: &mut Session) {
        if self.target.is_empty() {
            self.planned = None;
            self.mapping.clear();
            return;
        }
        let key = Planned { target: self.target.clone(), schema: self.schema.clone(), tables: self.chosen() };
        if self.planned.as_ref() == Some(&key) && !self.loading {
            return;
        }
        let mut params = json!({ "source": self.connection, "target": self.target, "tables": key.tables });
        if let Some(schema) = &self.schema {
            params["schema"] = json!(schema);
        }
        match registry.dispatch(session, "db.transfer.preview", params) {
            Ok(preview) => {
                let first = self.planned.is_none();
                self.schemas = preview["schemas"].as_array().into_iter().flatten().map(strings).collect();
                if self.schema.is_none() {
                    self.schema = preview["schema"].as_array().map(|_| strings(&preview["schema"]));
                }
                if first && let Ok(load) = serde_json::from_value::<LoadSettings>(preview["load"].clone()) {
                    self.batch = load.batch.to_string();
                    self.commit_every = load.commit_every.to_string();
                    self.remembered = load != LoadSettings::default();
                    self.load = load;
                }
                let tables = preview["tables"].as_array().cloned().unwrap_or_default();
                self.loading = tables.iter().any(|table| table["loading"] == true);
                self.mapping = tables.iter().map(|table| self.mapped(table)).collect();
                self.planned = Some(Planned { schema: self.schema.clone(), ..key });
                self.error = None;
            }
            Err(error) => {
                self.planned = None;
                self.loading = false;
                self.mapping.clear();
                self.error = Some(message(error).1);
            }
        }
    }

    fn mapped(&self, table: &Value) -> Mapped {
        let source = strings(&table["source"]);
        let before = self.mapping.iter().find(|known| known.source == source);
        let columns = table["columns"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|column| {
                let name = column["source"].as_str().unwrap_or_default().to_owned();
                let kept = before.and_then(|known| known.columns.iter().find(|known| known.source == name));
                Column {
                    target: kept.map_or_else(
                        || column["target"].as_str().unwrap_or_default().to_owned(),
                        |kept| kept.target.clone(),
                    ),
                    sql_type: column["type"].as_str().unwrap_or_default().to_owned(),
                    source_type: column["sourceType"].as_str().unwrap_or_default().to_owned(),
                    copied: kept.is_none_or(|kept| kept.copied),
                    source: name,
                }
            })
            .collect();
        Mapped {
            name: before.map_or_else(|| source.last().cloned().unwrap_or_default(), |known| known.name.clone()),
            exists: table["exists"] == true,
            rows: table["rows"].as_i64(),
            columns,
            source,
        }
    }

    fn load(&self) -> LoadSettings {
        let mut load = self.load.clone();
        load.batch = self.batch.trim().parse().unwrap_or(load.batch).clamp(1, 100_000);
        load.commit_every = self.commit_every.trim().parse().unwrap_or(load.commit_every);
        load
    }

    pub(super) fn start_call(&self, confirmed: Option<&str>) -> Value {
        let tables: Vec<Value> = self
            .mapping
            .iter()
            .map(|mapped| {
                let columns: Vec<Value> = mapped
                    .columns
                    .iter()
                    .filter(|column| column.copied && !column.target.trim().is_empty())
                    .map(|column| {
                        let mut value = json!({ "source": column.source, "target": column.target.trim() });
                        if !mapped.exists && !column.sql_type.trim().is_empty() {
                            value["type"] = json!(column.sql_type.trim());
                        }
                        value
                    })
                    .collect();
                json!({ "source": mapped.source, "name": mapped.name.trim(), "columns": columns })
            })
            .collect();
        let mut params = json!({
            "source": self.connection,
            "target": self.target,
            "schema": self.schema.clone().unwrap_or_default(),
            "tables": tables,
            "load": self.load(),
        });
        if let Some(token) = confirmed {
            params["confirmed"] = json!(token);
        }
        params
    }

    fn start(&mut self, registry: &Registry, session: &mut Session) {
        let token = self.confirm.as_ref().map(|(token, ..)| token.clone());
        match registry.dispatch(session, "db.transfer.start", self.start_call(token.as_deref())) {
            Ok(started) => {
                self.running = started["id"].as_u64();
                self.confirm = None;
                self.error = None;
            }
            Err(error) => {
                let (code, text, data) = message(error);
                if code.as_deref() == Some("confirmExecute") {
                    let statements = data["statements"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter_map(|row| row["text"].as_str())
                        .map(str::to_owned)
                        .collect();
                    self.confirm = Some((data["token"].as_str().unwrap_or_default().to_owned(), text, statements));
                    self.error = None;
                } else {
                    self.confirm = None;
                    self.error = Some(text);
                }
            }
        }
    }

    fn ready(&self) -> Result<(), String> {
        match self.step {
            Step::Target if self.chosen().is_empty() => Err(t!("ide-dbt-no-tables")),
            Step::Target if self.target.is_empty() => Err(t!("ide-dbt-no-target")),
            Step::Target if self.read_only(&self.target) => Err(t!("ide-dbt-read-only-target")),
            Step::Target if self.schema.is_none() => Err(t!("ide-dbt-no-schema")),
            Step::Mapping if self.loading => Err(t!("ide-dbt-loading")),
            Step::Mapping if self.mapping.iter().any(|mapped| mapped.name.trim().is_empty()) => {
                Err(t!("ide-dbt-no-name"))
            }
            Step::Mapping
                if self.mapping.iter().any(|mapped| {
                    mapped.columns.iter().all(|column| !column.copied || column.target.trim().is_empty())
                }) =>
            {
                Err(t!("ide-dbt-no-columns"))
            }
            _ => Ok(()),
        }
    }

    fn next(&mut self, registry: &Registry, session: &mut Session) {
        if self.step == Step::Target && (self.target.is_empty() || self.read_only(&self.target)) {
            self.error = self.ready().err();
            return;
        }
        if self.step == Step::Target {
            self.refresh(registry, session);
            if self.planned.is_none() {
                return;
            }
        }
        if let Err(error) = self.ready() {
            self.error = Some(error);
            return;
        }
        if self.step == Step::Confirm {
            self.start(registry, session);
            return;
        }
        self.error = None;
        self.step = Step::ALL[(self.step.at() + 1).min(Step::ALL.len() - 1)];
    }

    #[cfg(test)]
    pub(super) fn step(&self) -> Step {
        self.step
    }

    #[cfg(test)]
    pub(super) fn running(&self) -> Option<u64> {
        self.running
    }
}

enum Choice {
    Next,
    Back,
    Cancel,
    StopTransfer,
    Background,
}

impl IdeView {
    pub(super) fn transfer_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if let Some(params) = session.state_mut::<TransferRequest>().0.take() {
            match TransferDialog::open(registry, session, &params) {
                Ok(dialog) => self.transfer_dialog = Some(dialog),
                Err(error) => self.notify(zen_workbench::notifications::NewNotification::message(
                    zen_workbench::notifications::Group::General,
                    zen_workbench::notifications::Kind::Error,
                    error,
                )),
            }
        }
        let Some(dialog) = &mut self.transfer_dialog else { return };
        if let Some(id) = dialog.running {
            let state =
                registry.dispatch(session, "db.transfer.state", json!({ "id": id, "limit": 3 })).unwrap_or_default();
            dialog.progress = state["transfers"][0].clone();
            if dialog.progress["status"] != "running" {
                self.transfer_dialog = None;
                return;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else if dialog.loading && dialog.step == Step::Mapping {
            dialog.refresh(registry, session);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        let p = self.palette;
        let (choice, changed) = show(ui, &p, dialog);
        if changed {
            dialog.refresh(registry, session);
        }
        match choice {
            Some(Choice::Next) => dialog.next(registry, session),
            Some(Choice::Back) => {
                dialog.error = None;
                dialog.confirm = None;
                dialog.step = Step::ALL[dialog.step.at().saturating_sub(1)];
            }
            Some(Choice::StopTransfer) => {
                if let Some(id) = dialog.running {
                    let _ = registry.dispatch(session, "db.transfer.cancel", json!({ "id": id }));
                }
            }
            Some(Choice::Cancel | Choice::Background) => self.transfer_dialog = None,
            None => {}
        }
    }
}

fn label(ui: &mut Ui, p: &IdePalette, text: &str) {
    super::form::label(ui, p, text, LABEL_WIDTH);
}

fn field(ui: &mut Ui, p: &IdePalette, name: &str, value: &mut String, width: f32) {
    let edit = egui::TextEdit::singleline(value).desired_width(width);
    let response = super::connection_dialog::input(ui, p, edit);
    accessibility::name(&response, Role::TextInput, name);
}

fn check(ui: &mut Ui, p: &IdePalette, value: &mut bool, name: &str) {
    super::form::check(ui, p, value, name);
}

fn radio<T: PartialEq + Copy>(ui: &mut Ui, p: &IdePalette, value: &mut T, choice: T, name: &str) {
    super::form::radio(ui, p, value, choice, name);
}

fn hint(ui: &mut Ui, p: &IdePalette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
}

fn steps(ui: &mut Ui, p: &IdePalette, current: Step) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (at, step) in Step::ALL.iter().enumerate() {
            if at > 0 {
                ui.label(RichText::new("›").font(theme::regular(12.5)).color(p.text_secondary));
            }
            let text = format!("{}. {}", at + 1, step.title());
            let (font, color) = if *step == current {
                (theme::semibold(12.5), p.accent)
            } else if step.at() < current.at() {
                (theme::regular(12.5), p.text)
            } else {
                (theme::regular(12.5), p.text_secondary)
            };
            ui.label(RichText::new(text).font(font).color(color));
        }
    });
}

fn target_step(ui: &mut Ui, p: &IdePalette, dialog: &mut TransferDialog) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbt-source"));
        ui.label(RichText::new(dialog.name_of(&dialog.connection)).font(theme::regular(13.0)).color(p.text));
    });
    if dialog.sources.len() > 1 {
        ui.horizontal(|ui| {
            let chosen = dialog.sources.iter().filter(|source| source.chosen).count();
            ui.label(
                RichText::new(t!("ide-dbt-tables-chosen", chosen = chosen, count = dialog.sources.len()))
                    .font(theme::regular(12.0))
                    .color(p.text_secondary),
            );
            let all = dialog.sources.iter().all(|source| source.chosen);
            let text = if all { t!("ide-dbt-select-none") } else { t!("ide-dbt-select-all") };
            if vcs::secondary_button(ui, p, &text).clicked() {
                for source in &mut dialog.sources {
                    source.chosen = !all;
                }
            }
        });
    }
    egui::ScrollArea::vertical().id_salt("dbt-tables").max_height(LIST_HEIGHT).show(ui, |ui| {
        ui.set_width(WIDTH);
        ui.horizontal_wrapped(|ui| {
            for source in &mut dialog.sources {
                let name = format!("{}.{}", source.path[1], source.path[3]);
                ui.allocate_ui(egui::vec2(206.0, 20.0), |ui| check(ui, p, &mut source.chosen, &name));
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbt-target"));
        let before = dialog.target.clone();
        let shown =
            if dialog.target.is_empty() { t!("ide-dbt-choose-target") } else { target_title(dialog, &dialog.target) };
        let titles: Vec<(String, String)> =
            dialog.connections.iter().map(|known| (known.id.clone(), target_title(dialog, &known.id))).collect();
        super::form::combo(ui, p, "dbt-target", &t!("ide-dbt-target"), &shown, 280.0, |ui| {
            for (id, title) in titles {
                ui.selectable_value(&mut dialog.target, id, title);
            }
        });
        if dialog.target != before {
            dialog.schema = None;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbt-schema"));
        let before = dialog.schema.clone();
        let shown = dialog.schema.as_deref().map(schema_name).unwrap_or_default();
        super::form::combo(ui, p, "dbt-schema", &t!("ide-dbt-schema"), &shown, 280.0, |ui| {
            for schema in &dialog.schemas {
                ui.selectable_value(&mut dialog.schema, Some(schema.clone()), schema.join("."));
            }
        });
        changed |= dialog.schema != before;
    });
    ui.horizontal(|ui| {
        label(ui, p, "");
        hint(ui, p, &t!("ide-dbt-target-hint"));
    });
    changed
}

fn target_title(dialog: &TransferDialog, id: &str) -> String {
    let name = dialog.name_of(id);
    if dialog.read_only(id) { t!("ide-dbt-read-only-name", name = name.as_str()) } else { name }
}

fn mapping_step(ui: &mut Ui, p: &IdePalette, dialog: &mut TransferDialog) {
    if dialog.loading {
        hint(ui, p, &t!("ide-dbt-loading"));
    }
    hint(ui, p, &t!("ide-dbt-mapping-hint"));
    egui::ScrollArea::vertical().id_salt("dbt-mapping").max_height(MAPPING_HEIGHT).show(ui, |ui| {
        ui.set_width(WIDTH);
        for (index, mapped) in dialog.mapping.iter_mut().enumerate() {
            let source = format!("{}.{}", mapped.source.get(1).cloned().unwrap_or_default(), mapped.source[3]);
            ui.horizontal(|ui| {
                ui.label(RichText::new(source.as_str()).font(theme::semibold(13.0)).color(p.text));
                ui.label(RichText::new("→").font(theme::regular(13.0)).color(p.text_secondary));
                field(ui, p, &t!("ide-dbt-target-of", table = source.as_str()), &mut mapped.name, 200.0);
                let (text, color) = if mapped.exists {
                    (t!("ide-dbt-existing"), p.text_secondary)
                } else {
                    (t!("ide-dbt-new"), p.accent)
                };
                ui.label(RichText::new(text).font(theme::regular(12.0)).color(color));
                if let Some(rows) = mapped.rows.filter(|rows| *rows >= 0) {
                    let text = t!("ide-dbt-rows", rows = rows);
                    ui.label(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary));
                }
            });
            let create = !mapped.exists;
            egui::Grid::new(("dbt-columns", index)).spacing([12.0, 3.0]).show(ui, |ui| {
                let mut titles = vec![t!("ide-dbt-source-column"), t!("ide-dbt-target-column")];
                titles.push(if create { t!("ide-dbt-new-type") } else { t!("ide-dbt-type") });
                for title in titles {
                    ui.label(RichText::new(title).font(theme::semibold(12.0)).color(p.text_secondary));
                }
                ui.end_row();
                for column in &mut mapped.columns {
                    let shown = format!("{} ({})", column.source, column.source_type);
                    check(ui, p, &mut column.copied, &shown);
                    field(ui, p, &t!("ide-dbt-column-of", column = column.source.as_str()), &mut column.target, 170.0);
                    if create {
                        field(
                            ui,
                            p,
                            &t!("ide-dbt-type-of", column = column.source.as_str()),
                            &mut column.sql_type,
                            170.0,
                        );
                    } else {
                        ui.label(
                            RichText::new(column.sql_type.as_str()).font(theme::regular(12.5)).color(p.text_secondary),
                        );
                    }
                    ui.end_row();
                }
            });
            ui.add_space(6.0);
        }
    });
}

fn settings_step(ui: &mut Ui, p: &IdePalette, dialog: &mut TransferDialog) {
    if dialog.remembered {
        ui.horizontal(|ui| {
            label(ui, p, "");
            ui.label(RichText::new(t!("ide-dbt-remembered")).font(theme::regular(12.0)).color(p.text_secondary));
            if ui.link(RichText::new(t!("ide-dbt-reset")).font(theme::regular(12.0)).color(p.link)).clicked() {
                dialog.reset_load();
            }
        });
    }
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbt-batch"));
        field(ui, p, &t!("ide-dbt-batch"), &mut dialog.batch, 70.0);
        hint(ui, p, &t!("ide-dbt-batch-hint"));
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbt-commit-every"));
        field(ui, p, &t!("ide-dbt-commit-every"), &mut dialog.commit_every, 70.0);
        hint(ui, p, &t!("ide-dbt-commit-hint"));
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbt-on-error"));
        radio(ui, p, &mut dialog.load.on_error, OnError::Stop, &t!("ide-dbt-stop"));
        radio(ui, p, &mut dialog.load.on_error, OnError::Skip, &t!("ide-dbt-skip-rows"));
    });
    if dialog.mapping.iter().any(|mapped| mapped.exists) {
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbt-before-loading"));
            check(ui, p, &mut dialog.load.truncate, &t!("ide-dbt-truncate"));
        });
    }
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut dialog.load.server_copy, &t!("ide-dbt-server-copy"));
    });
}

fn confirm_step(ui: &mut Ui, p: &IdePalette, dialog: &TransferDialog) {
    let line = |ui: &mut Ui, name: String, value: String| {
        ui.horizontal(|ui| {
            label(ui, p, &name);
            ui.add(egui::Label::new(RichText::new(value).font(theme::regular(13.0)).color(p.text)).wrap());
        });
    };
    line(ui, t!("ide-dbt-source"), dialog.name_of(&dialog.connection));
    let schema = dialog.schema.as_deref().map(schema_name).unwrap_or_default();
    line(ui, t!("ide-dbt-target"), format!("{} · {schema}", dialog.name_of(&dialog.target)));
    for mapped in &dialog.mapping {
        let target = format!("{schema}.{}", mapped.name.trim());
        let shown = if mapped.exists { target } else { t!("ide-dbt-target-new", table = target) };
        line(ui, format!("{}.{}", mapped.source.get(1).cloned().unwrap_or_default(), mapped.source[3]), shown);
    }
    let load = dialog.load();
    let transactions = if load.commit_every == 0 {
        t!("ide-dbt-one-transaction")
    } else {
        t!("ide-dbt-commit-each", rows = load.commit_every)
    };
    let errors = match load.on_error {
        OnError::Stop => t!("ide-dbt-stop"),
        OnError::Skip => t!("ide-dbt-skip-rows"),
    };
    line(
        ui,
        t!("ide-dbt-loading-label"),
        format!("{} · {transactions} · {errors}", t!("ide-dbt-rows-a-batch", rows = load.batch)),
    );
    if load.truncate && dialog.mapping.iter().any(|mapped| mapped.exists) {
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbt-before-loading"));
            let warning =
                RichText::new(t!("ide-dbt-truncate-warning")).font(theme::regular(13.0)).color(p.warning_text);
            ui.add(egui::Label::new(warning).wrap());
        });
    }
    if let Some((_, text, statements)) = &dialog.confirm {
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new(text.clone()).font(theme::regular(12.5)).color(p.warning_text)).wrap());
        for statement in statements {
            ui.label(RichText::new(statement.clone()).font(egui::FontId::monospace(12.0)).color(p.text));
        }
    }
}

fn progress(ui: &mut Ui, p: &IdePalette, dialog: &TransferDialog) {
    let state = &dialog.progress;
    let tables = state["tables"].as_array().cloned().unwrap_or_default();
    let current = state["current"].as_u64().and_then(|at| usize::try_from(at).ok()).unwrap_or(0);
    let name = tables.get(current).and_then(|table| table["name"].as_str()).unwrap_or_default();
    let line = t!("ide-dbt-copying", name = name, at = current + 1, count = tables.len().max(1));
    ui.label(RichText::new(line).font(theme::regular(13.0)).color(p.text));
    let seconds = state["elapsedMs"].as_u64().unwrap_or(0) as f64 / 1000.0;
    let counters = t!(
        "ide-dbt-counters",
        read = state["rowsRead"].as_u64().unwrap_or(0),
        inserted = state["rowsInserted"].as_u64().unwrap_or(0),
        skipped = state["rowsSkipped"].as_u64().unwrap_or(0),
        seconds = format!("{seconds:.1}")
    );
    ui.label(RichText::new(counters).font(theme::regular(12.0)).color(p.text_secondary));
    let done = tables.iter().filter(|table| table["status"] == "done").count();
    let fraction = done as f32 / tables.len().max(1) as f32;
    let (track, _) = ui.allocate_exact_size(egui::vec2(WIDTH, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(track, 2, p.frame);
    let fill = egui::Rect::from_min_size(track.min, egui::vec2(track.width() * fraction, track.height()));
    if fill.width() > 0.0 {
        ui.painter().rect_filled(fill, 2, p.accent);
    }
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut TransferDialog) -> (Option<Choice>, bool) {
    let mut choice = None;
    let mut changed = false;
    let modal = egui::Modal::new(ui.scope_id().with("transfer-dialog")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(t!("ide-dbt-title")).font(theme::semibold(15.0)).color(p.text));
        if dialog.running.is_some() {
            ui.add_space(6.0);
            progress(ui, p, dialog);
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if vcs::primary_button(ui, p, &t!("ide-dbt-background")).clicked() {
                    choice = Some(Choice::Background);
                }
                if vcs::secondary_button(ui, p, &t!("ide-dbt-stop-transfer")).clicked() {
                    choice = Some(Choice::StopTransfer);
                }
            });
            return;
        }
        steps(ui, p, dialog.step);
        ui.separator();
        ui.allocate_ui_with_layout(egui::vec2(WIDTH, BODY_HEIGHT), Layout::top_down(Align::Min), |ui| {
            ui.set_min_size(egui::vec2(WIDTH, BODY_HEIGHT));
            match dialog.step {
                Step::Target => changed = target_step(ui, p, dialog),
                Step::Mapping => mapping_step(ui, p, dialog),
                Step::Settings => settings_step(ui, p, dialog),
                Step::Confirm => confirm_step(ui, p, dialog),
            }
            if let Some(error) = &dialog.error {
                ui.add(
                    egui::Label::new(RichText::new(error.clone()).font(theme::regular(12.0)).color(p.error_text))
                        .wrap(),
                );
            }
        });
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let text = match (dialog.step, &dialog.confirm) {
                (Step::Confirm, Some(_)) => t!("ide-dbt-transfer-anyway"),
                (Step::Confirm, None) => t!("ide-dbt-transfer"),
                _ => t!("ide-dbt-next"),
            };
            if vcs::primary_button(ui, p, &text).clicked() {
                choice = Some(Choice::Next);
            }
            if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                choice = Some(Choice::Cancel);
            }
            if dialog.step != Step::Target && vcs::secondary_button(ui, p, &t!("ide-dbt-back")).clicked() {
                choice = Some(Choice::Back);
            }
        });
    });
    let fresh = std::mem::replace(&mut dialog.fresh, false);
    let choice = choice.or_else(|| {
        (modal.should_close() && !fresh).then_some(if dialog.running.is_some() {
            Choice::Background
        } else {
            Choice::Cancel
        })
    });
    (choice, changed)
}

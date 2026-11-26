use egui::{Align, Layout, RichText, Role, Ui};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::csv_import::{CsvFormat, Kind, LoadSettings, OnError, SAMPLE_LIMIT};
use zen_i18n::t;

use super::{IdePalette, IdeView, dialog_frame, vcs};
use crate::{accessibility, theme};

pub(super) const OPEN: &str = "view.database.import";

const WIDTH: f32 = 680.0;
const SAMPLE_HEIGHT: f32 = 210.0;
const MAPPING_HEIGHT: f32 = 230.0;
const CELL: usize = 40;
const LABEL_WIDTH: f32 = 150.0;
const KINDS: [Kind; 6] = [Kind::Text, Kind::Integer, Kind::Decimal, Kind::Boolean, Kind::Date, Kind::Timestamp];

#[derive(Default)]
pub(super) struct CsvImportRequest(pub(super) Option<Value>);

pub(super) fn request(session: &mut Session, params: Value) {
    session.state_mut::<CsvImportRequest>().0 = Some(params);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    File,
    Format,
    Mapping,
    Settings,
    Confirm,
}

impl Step {
    const ALL: [Step; 5] = [Step::File, Step::Format, Step::Mapping, Step::Settings, Step::Confirm];

    fn title(self) -> String {
        match self {
            Step::File => t!("ide-dbcsv-step-file"),
            Step::Format => t!("ide-dbcsv-step-format"),
            Step::Mapping => t!("ide-dbcsv-step-mapping"),
            Step::Settings => t!("ide-dbcsv-step-settings"),
            Step::Confirm => t!("ide-dbcsv-step-confirm"),
        }
    }

    fn at(self) -> usize {
        Self::ALL.iter().position(|step| *step == self).unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
struct Mapped {
    name: String,
    target: String,
    kind: Kind,
    sql_type: String,
}

#[derive(Clone, Debug)]
pub(super) struct CsvImportDialog {
    connection: String,
    table: Vec<String>,
    create: bool,
    new_name: String,
    file: String,
    format: CsvFormat,
    delimiter: String,
    quote: String,
    escape: String,
    pattern: String,
    previewed: Option<(String, CsvFormat)>,
    preview: Value,
    targets: Vec<String>,
    mapping: Vec<Mapped>,
    load: LoadSettings,
    batch: String,
    commit_every: String,
    step: Step,
    error: Option<String>,
    confirm: Option<(String, String, Vec<String>)>,
    running: Option<u64>,
    progress: Value,
    fresh: bool,
    browse: bool,
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

fn kind_name(kind: Kind) -> String {
    match kind {
        Kind::Text => t!("ide-dbcsv-kind-text"),
        Kind::Integer => t!("ide-dbcsv-kind-integer"),
        Kind::Decimal => t!("ide-dbcsv-kind-decimal"),
        Kind::Boolean => t!("ide-dbcsv-kind-boolean"),
        Kind::Date => t!("ide-dbcsv-kind-date"),
        Kind::Timestamp => t!("ide-dbcsv-kind-timestamp"),
    }
}

fn char_text(c: char) -> String {
    if c == '\t' { "\\t".to_owned() } else { c.to_string() }
}

fn text_char(text: &str) -> Option<char> {
    if text == "\\t" {
        return Some('\t');
    }
    let mut chars = text.chars();
    chars.next().filter(|_| chars.next().is_none())
}

impl CsvImportDialog {
    pub(super) fn open(registry: &Registry, session: &mut Session, params: &Value) -> Result<Self, String> {
        let (connection, path) = match params["connection"].as_str() {
            Some(connection) => (connection.to_owned(), strings(&params["path"])),
            None => {
                let state = registry.dispatch(session, "db.navigator.state", json!({ "limit": 1 })).unwrap_or_default();
                let selected = &state["selected"];
                match selected["connection"].as_str() {
                    Some(connection) => (connection.to_owned(), strings(&selected["path"])),
                    None => return Err(t!("ide-dbcsv-choose-table")),
                }
            }
        };
        let (table, create) = match path.as_slice() {
            [_, _, folder, _] if folder == "tables" => (path.clone(), false),
            [catalog, schema] | [catalog, schema, _] => {
                (vec![catalog.clone(), schema.clone(), "tables".to_owned(), String::new()], true)
            }
            _ => return Err(t!("ide-dbcsv-choose-table")),
        };
        let remembered =
            registry.dispatch(session, "db.csvImport.settings", json!({})).map_err(|error| message(error).1)?;
        let format: CsvFormat = serde_json::from_value(remembered["format"].clone()).unwrap_or_default();
        let load: LoadSettings = serde_json::from_value(remembered["load"].clone()).unwrap_or_default();
        let folder = remembered["folder"].as_str().map(|folder| format!("{}/", folder.trim_end_matches('/')));
        Ok(Self {
            connection,
            table,
            create,
            new_name: String::new(),
            file: folder.unwrap_or_default(),
            delimiter: char_text(format.delimiter),
            quote: format.quote.to_string(),
            escape: format.escape.map(char_text).unwrap_or_default(),
            pattern: format.timestamp_format.clone().unwrap_or_default(),
            format,
            previewed: None,
            preview: Value::Null,
            targets: Vec::new(),
            mapping: Vec::new(),
            batch: load.batch.to_string(),
            commit_every: load.commit_every.to_string(),
            load,
            step: Step::File,
            error: None,
            confirm: None,
            running: None,
            progress: Value::Null,
            fresh: true,
            browse: false,
        })
    }

    fn table_path(&self) -> Vec<String> {
        let mut path = self.table.clone();
        if self.create
            && let Some(last) = path.last_mut()
        {
            self.new_name.trim().clone_into(last);
        }
        path
    }

    fn read_format(&mut self) -> Result<CsvFormat, String> {
        let mut format = self.format.clone();
        format.delimiter = text_char(&self.delimiter).ok_or_else(|| t!("ide-dbcsv-one-character"))?;
        format.quote = text_char(&self.quote).ok_or_else(|| t!("ide-dbcsv-one-character"))?;
        format.escape = match self.escape.as_str() {
            "" => None,
            text => Some(text_char(text).ok_or_else(|| t!("ide-dbcsv-one-character"))?),
        };
        format.timestamp_format = Some(self.pattern.trim().to_owned()).filter(|pattern| !pattern.is_empty());
        Ok(format)
    }

    fn load(&self) -> LoadSettings {
        let mut load = self.load.clone();
        load.batch = self.batch.trim().parse().unwrap_or(load.batch).clamp(1, 100_000);
        load.commit_every = self.commit_every.trim().parse().unwrap_or(load.commit_every);
        if self.create {
            load.truncate = false;
        }
        load
    }

    fn refresh(&mut self, registry: &Registry, session: &mut Session) {
        let format = match self.read_format() {
            Ok(format) => format,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        if self.previewed.as_ref().is_some_and(|(file, known)| *file == self.file && *known == format) {
            return;
        }
        let params = json!({
            "file": self.file.trim(),
            "format": format,
            "limit": SAMPLE_LIMIT,
            "connection": self.connection,
            "table": self.table_path(),
            "create": self.create,
        });
        match registry.dispatch(session, "db.csvImport.preview", params) {
            Ok(preview) => {
                self.previewed = Some((self.file.clone(), format.clone()));
                self.format = format;
                self.targets = preview["target"]["columns"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|column| column["name"].as_str().map(str::to_owned))
                    .collect();
                let proposed = preview["mapping"].as_array().cloned().unwrap_or_default();
                self.mapping = preview["columns"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .enumerate()
                    .map(|(at, column)| {
                        let found = proposed.iter().find(|mapped| mapped["source"].as_u64() == Some(at as u64));
                        let kind = found.map_or(&column["kind"], |mapped| &mapped["kind"]);
                        Mapped {
                            name: column["name"].as_str().unwrap_or_default().to_owned(),
                            target: found.and_then(|mapped| mapped["target"].as_str()).unwrap_or_default().to_owned(),
                            kind: serde_json::from_value(kind.clone()).unwrap_or_default(),
                            sql_type: found
                                .and_then(|mapped| mapped["sqlType"].as_str())
                                .unwrap_or_default()
                                .to_owned(),
                        }
                    })
                    .collect();
                self.preview = preview;
                self.error = None;
            }
            Err(error) => {
                self.previewed = None;
                self.preview = Value::Null;
                self.error = Some(message(error).1);
            }
        }
    }

    fn columns(&self) -> Vec<Value> {
        self.mapping
            .iter()
            .enumerate()
            .filter(|(_, mapped)| !mapped.target.trim().is_empty())
            .map(|(at, mapped)| {
                let mut column = json!({ "source": at, "target": mapped.target.trim(), "kind": mapped.kind });
                if self.create && !mapped.sql_type.trim().is_empty() {
                    column["sqlType"] = json!(mapped.sql_type.trim());
                }
                column
            })
            .collect()
    }

    pub(super) fn start_call(&self, confirmed: Option<&str>) -> Value {
        let mut params = json!({
            "connection": self.connection,
            "file": self.file.trim(),
            "table": self.table_path(),
            "create": self.create,
            "format": self.format,
            "columns": self.columns(),
            "load": self.load(),
        });
        if let Some(token) = confirmed {
            params["confirmed"] = json!(token);
        }
        params
    }

    fn start(&mut self, registry: &Registry, session: &mut Session) {
        let token = self.confirm.as_ref().map(|(token, ..)| token.clone());
        match registry.dispatch(session, "db.csvImport.start", self.start_call(token.as_deref())) {
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
            Step::File if self.file.trim().is_empty() || self.file.ends_with('/') => Err(t!("ide-dbcsv-no-file")),
            Step::File if self.create && self.new_name.trim().is_empty() => Err(t!("ide-dbcsv-no-table-name")),
            Step::Mapping if self.mapping.iter().all(|mapped| mapped.target.trim().is_empty()) => {
                Err(t!("ide-dbcsv-no-columns"))
            }
            _ => Ok(()),
        }
    }

    fn next(&mut self, registry: &Registry, session: &mut Session) {
        if let Err(error) = self.ready() {
            self.error = Some(error);
            return;
        }
        if matches!(self.step, Step::File | Step::Format) {
            self.refresh(registry, session);
            if self.previewed.is_none() {
                return;
            }
        }
        if self.step == Step::Confirm {
            self.start(registry, session);
            return;
        }
        self.error = None;
        self.step = Step::ALL[(self.step.at() + 1).min(Step::ALL.len() - 1)];
    }

    pub(super) fn set_file(&mut self, file: &str) {
        file.clone_into(&mut self.file);
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
    StopImport,
    Background,
}

impl IdeView {
    pub(super) fn csv_import_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if let Some(params) = session.state_mut::<CsvImportRequest>().0.take() {
            match CsvImportDialog::open(registry, session, &params) {
                Ok(dialog) => self.csv_import_dialog = Some(dialog),
                Err(error) => self.notify(zen_workbench::notifications::NewNotification::message(
                    zen_workbench::notifications::Group::General,
                    zen_workbench::notifications::Kind::Error,
                    error,
                )),
            }
        }
        let Some(dialog) = &mut self.csv_import_dialog else { return };
        if let Some(id) = dialog.running {
            let state =
                registry.dispatch(session, "db.csvImport.state", json!({ "id": id, "limit": 3 })).unwrap_or_default();
            dialog.progress = state["imports"][0].clone();
            if dialog.progress["status"] != "running" {
                self.csv_import_dialog = None;
                return;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if std::mem::take(&mut dialog.browse) {
            let mut picker = rfd::FileDialog::new()
                .set_title(t!("ide-dbcsv-choose-file"))
                .add_filter(t!("ide-dbcsv-csv-files"), &["csv", "tsv", "txt"])
                .add_filter(t!("ide-dbc-path-all-files"), &["*"]);
            if let Some(folder) = std::path::Path::new(dialog.file.trim()).parent().filter(|folder| folder.is_dir()) {
                picker = picker.set_directory(folder);
            }
            if let Some(file) = picker.pick_file() {
                dialog.set_file(&file.display().to_string());
            }
        }
        let p = self.palette;
        let (choice, changed) = show(ui, &p, dialog);
        if changed && dialog.step == Step::Format {
            dialog.previewed = None;
            dialog.refresh(registry, session);
        }
        match choice {
            Some(Choice::Next) => dialog.next(registry, session),
            Some(Choice::Back) => {
                dialog.error = None;
                dialog.confirm = None;
                dialog.step = Step::ALL[dialog.step.at().saturating_sub(1)];
            }
            Some(Choice::StopImport) => {
                if let Some(id) = dialog.running {
                    let _ = registry.dispatch(session, "db.csvImport.cancel", json!({ "id": id }));
                }
            }
            Some(Choice::Cancel | Choice::Background) => self.csv_import_dialog = None,
            None => {}
        }
    }
}

fn label(ui: &mut Ui, p: &IdePalette, text: &str) {
    super::form::label(ui, p, text, LABEL_WIDTH);
}

fn field(ui: &mut Ui, p: &IdePalette, name: &str, value: &mut String, width: f32) -> egui::Response {
    let edit = egui::TextEdit::singleline(value).desired_width(width);
    let response = super::connection_dialog::input(ui, p, edit);
    accessibility::name(&response, Role::TextInput, name);
    response
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

fn file_step(ui: &mut Ui, p: &IdePalette, dialog: &mut CsvImportDialog) {
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbcsv-file"));
        let browse = t!("ide-dbcsv-browse");
        let room = WIDTH
            - LABEL_WIDTH
            - 40.0
            - ui.fonts_mut(|fonts| fonts.layout_no_wrap(browse.clone(), theme::regular(13.0), p.text).size().x);
        field(ui, p, &t!("ide-dbcsv-file"), &mut dialog.file, room - 16.0);
        if vcs::secondary_button(ui, p, &browse).clicked() {
            dialog.browse = true;
        }
    });
    ui.horizontal(|ui| {
        label(ui, p, "");
        hint(ui, p, &t!("ide-dbcsv-file-hint"));
    });
    ui.add_space(4.0);
    if dialog.create {
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbcsv-new-table"));
            field(ui, p, &t!("ide-dbcsv-new-table"), &mut dialog.new_name, 240.0);
            ui.label(
                RichText::new(t!("ide-dbcsv-in-schema", schema = dialog.table.get(1).cloned().unwrap_or_default()))
                    .font(theme::regular(12.5))
                    .color(p.text_secondary),
            );
        });
    } else {
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbcsv-table"));
            let name = format!(
                "{}.{}",
                dialog.table.get(1).cloned().unwrap_or_default(),
                dialog.table.get(3).cloned().unwrap_or_default()
            );
            ui.label(RichText::new(name).font(theme::regular(13.0)).color(p.text));
        });
    }
}

fn sample(ui: &mut Ui, p: &IdePalette, dialog: &CsvImportDialog) {
    let columns = dialog.preview["columns"].as_array().cloned().unwrap_or_default();
    let rows = dialog.preview["rows"].as_array().cloned().unwrap_or_default();
    if columns.is_empty() {
        return;
    }
    let shown = t!("ide-dbcsv-sample", rows = rows.len(), columns = columns.len());
    ui.label(RichText::new(shown).font(theme::regular(12.0)).color(p.text_secondary));
    egui::Frame::new().stroke(egui::Stroke::new(1.0, p.frame)).corner_radius(4).show(ui, |ui| {
        egui::ScrollArea::both().id_salt("dbi-sample").max_height(SAMPLE_HEIGHT).max_width(WIDTH - 4.0).show(
            ui,
            |ui| {
                egui::Grid::new("dbi-sample-grid").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                    for column in &columns {
                        let kind: Kind = serde_json::from_value(column["kind"].clone()).unwrap_or_default();
                        ui.vertical(|ui| {
                            let name = RichText::new(column["name"].as_str().unwrap_or_default())
                                .font(theme::semibold(12.5))
                                .color(p.text);
                            ui.add(egui::Label::new(name).extend());
                            let kind =
                                RichText::new(kind_name(kind)).font(theme::regular(11.0)).color(p.text_secondary);
                            ui.add(egui::Label::new(kind).extend());
                        });
                    }
                    ui.end_row();
                    for row in &rows {
                        for cell in row.as_array().into_iter().flatten() {
                            match cell.as_str() {
                                Some(text) => {
                                    let short: String = text.chars().take(CELL).collect();
                                    let short = if short.len() < text.len() { format!("{short}…") } else { short };
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(short.replace(['\n', '\r'], "↵"))
                                                .font(theme::regular(12.5))
                                                .color(p.text),
                                        )
                                        .extend(),
                                    )
                                }
                                None => {
                                    ui.label(RichText::new("NULL").font(theme::regular(12.5)).color(p.text_secondary))
                                }
                            };
                        }
                        ui.end_row();
                    }
                });
            },
        );
    });
}

fn format_step(ui: &mut Ui, p: &IdePalette, dialog: &mut CsvImportDialog) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbcsv-delimiter"));
        changed |= field(ui, p, &t!("ide-dbcsv-delimiter"), &mut dialog.delimiter, 30.0).changed();
        ui.add_space(10.0);
        ui.label(RichText::new(t!("ide-dbcsv-quote")).font(theme::regular(13.0)).color(p.text));
        changed |= field(ui, p, &t!("ide-dbcsv-quote"), &mut dialog.quote, 24.0).changed();
        ui.add_space(10.0);
        ui.label(RichText::new(t!("ide-dbcsv-escape")).font(theme::regular(13.0)).color(p.text));
        changed |= field(ui, p, &t!("ide-dbcsv-escape"), &mut dialog.escape, 24.0).changed();
        ui.add_space(10.0);
        ui.label(RichText::new(t!("ide-dbcsv-null")).font(theme::regular(13.0)).color(p.text));
        changed |= field(ui, p, &t!("ide-dbcsv-null"), &mut dialog.format.null_text, 60.0).changed();
    });
    ui.horizontal(|ui| {
        label(ui, p, "");
        let before = dialog.format.header;
        check(ui, p, &mut dialog.format.header, &t!("ide-dbcsv-header"));
        changed |= before != dialog.format.header;
        hint(ui, p, &t!("ide-dbcsv-delimiter-hint"));
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbcsv-timestamp"));
        changed |= field(ui, p, &t!("ide-dbcsv-timestamp"), &mut dialog.pattern, 200.0).changed();
        hint(ui, p, &t!("ide-dbcsv-timestamp-hint"));
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbcsv-encoding"));
        ui.label(RichText::new("UTF-8").font(theme::regular(13.0)).color(p.text_secondary));
    });
    sample(ui, p, dialog);
    changed
}

fn kind_combo(ui: &mut Ui, p: &IdePalette, id: (usize, &str), kind: &mut Kind) {
    let name = t!("ide-dbcsv-kind-of", column = id.1);
    super::form::combo(ui, p, ("dbi-kind", id.0), &name, &kind_name(*kind), 130.0, |ui| {
        for choice in KINDS {
            ui.selectable_value(kind, choice, kind_name(choice));
        }
    });
}

fn mapping_step(ui: &mut Ui, p: &IdePalette, dialog: &mut CsvImportDialog) {
    let skip = t!("ide-dbcsv-skip");
    if dialog.create {
        hint(ui, p, &t!("ide-dbcsv-mapping-new"));
    } else if dialog.targets.is_empty() {
        hint(ui, p, &t!("ide-dbcsv-mapping-unknown"));
    } else {
        hint(ui, p, &t!("ide-dbcsv-mapping-existing"));
    }
    let (create, targets) = (dialog.create, dialog.targets.clone());
    egui::ScrollArea::vertical().id_salt("dbi-mapping").max_height(MAPPING_HEIGHT).show(ui, |ui| {
        ui.set_width(WIDTH);
        egui::Grid::new("dbi-mapping-grid").spacing([12.0, 4.0]).show(ui, |ui| {
            for title in [t!("ide-dbcsv-file-column"), t!("ide-dbcsv-target-column"), t!("ide-dbcsv-convert-as")] {
                ui.label(RichText::new(title).font(theme::semibold(12.5)).color(p.text_secondary));
            }
            if create {
                ui.label(RichText::new(t!("ide-dbcsv-sql-type")).font(theme::semibold(12.5)).color(p.text_secondary));
            }
            ui.end_row();
            for (at, mapped) in dialog.mapping.iter_mut().enumerate() {
                ui.label(RichText::new(mapped.name.clone()).font(theme::regular(13.0)).color(p.text));
                if create || targets.is_empty() {
                    field(ui, p, &t!("ide-dbcsv-target-of", column = mapped.name.as_str()), &mut mapped.target, 170.0);
                } else {
                    let shown = if mapped.target.is_empty() { skip.clone() } else { mapped.target.clone() };
                    let name = t!("ide-dbcsv-target-of", column = mapped.name.as_str());
                    super::form::combo(ui, p, ("dbi-target", at), &name, &shown, 170.0, |ui| {
                        ui.selectable_value(&mut mapped.target, String::new(), skip.clone());
                        for target in &targets {
                            ui.selectable_value(&mut mapped.target, target.clone(), target);
                        }
                    });
                }
                kind_combo(ui, p, (at, &mapped.name), &mut mapped.kind);
                if create {
                    field(ui, p, &t!("ide-dbcsv-sql-type"), &mut mapped.sql_type, 150.0);
                }
                ui.end_row();
            }
        });
    });
}

fn settings_step(ui: &mut Ui, p: &IdePalette, dialog: &mut CsvImportDialog) {
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbcsv-batch"));
        field(ui, p, &t!("ide-dbcsv-batch"), &mut dialog.batch, 70.0);
        hint(ui, p, &t!("ide-dbcsv-batch-hint"));
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbcsv-commit-every"));
        field(ui, p, &t!("ide-dbcsv-commit-every"), &mut dialog.commit_every, 70.0);
        hint(ui, p, &t!("ide-dbcsv-commit-hint"));
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbcsv-on-error"));
        radio(ui, p, &mut dialog.load.on_error, OnError::Stop, &t!("ide-dbcsv-stop"));
        radio(ui, p, &mut dialog.load.on_error, OnError::Skip, &t!("ide-dbcsv-skip-rows"));
    });
    if !dialog.create {
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbcsv-before-loading"));
            check(ui, p, &mut dialog.load.truncate, &t!("ide-dbcsv-truncate"));
        });
    }
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut dialog.load.server_copy, &t!("ide-dbcsv-server-copy"));
    });
}

fn confirm_step(ui: &mut Ui, p: &IdePalette, dialog: &CsvImportDialog) {
    let line = |ui: &mut Ui, name: String, value: String| {
        ui.horizontal(|ui| {
            label(ui, p, &name);
            ui.add(egui::Label::new(RichText::new(value).font(theme::regular(13.0)).color(p.text)).wrap());
        });
    };
    let bytes = zen_workbench::database::objects::size_text(dialog.preview["fileBytes"].as_u64().unwrap_or(0));
    line(ui, t!("ide-dbcsv-file"), format!("{} ({bytes})", dialog.file.trim()));
    let schema = dialog.table.get(1).cloned().unwrap_or_default();
    let name = dialog.table_path().last().cloned().unwrap_or_default();
    let target = if dialog.create {
        t!("ide-dbcsv-target-new", table = format!("{schema}.{name}"))
    } else {
        format!("{schema}.{name}")
    };
    line(ui, t!("ide-dbcsv-table"), target);
    let mapped: Vec<String> = dialog
        .mapping
        .iter()
        .filter(|mapped| !mapped.target.trim().is_empty())
        .map(|mapped| mapped.target.trim().to_owned())
        .collect();
    line(ui, t!("ide-dbcsv-columns"), mapped.join(", "));
    let load = dialog.load();
    let transactions = if load.commit_every == 0 {
        t!("ide-dbcsv-one-transaction")
    } else {
        t!("ide-dbcsv-commit-each", rows = load.commit_every)
    };
    let errors = match load.on_error {
        OnError::Stop => t!("ide-dbcsv-stop"),
        OnError::Skip => t!("ide-dbcsv-skip-rows"),
    };
    line(
        ui,
        t!("ide-dbcsv-loading"),
        format!("{} · {transactions} · {errors}", t!("ide-dbcsv-rows-a-batch", rows = load.batch)),
    );
    if load.truncate {
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbcsv-before-loading"));
            let warning = RichText::new(t!("ide-dbcsv-truncate-warning", table = format!("{schema}.{name}")))
                .font(theme::regular(13.0))
                .color(p.warning_text);
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

fn progress(ui: &mut Ui, p: &IdePalette, dialog: &CsvImportDialog) {
    let state = &dialog.progress;
    let file = std::path::Path::new(dialog.file.trim())
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let table = state["table"].as_str().unwrap_or_default();
    ui.label(
        RichText::new(t!("ide-dbcsv-importing", file = file, table = table)).font(theme::regular(13.0)).color(p.text),
    );
    let read = state["bytesRead"].as_u64().unwrap_or(0);
    let total = state["fileBytes"].as_u64().unwrap_or(0).max(1);
    let fraction = (read as f64 / total as f64).clamp(0.0, 1.0) as f32;
    let seconds = state["elapsedMs"].as_u64().unwrap_or(0) as f64 / 1000.0;
    let counters = t!(
        "ide-dbcsv-counters",
        read = state["rowsRead"].as_u64().unwrap_or(0),
        inserted = state["rowsInserted"].as_u64().unwrap_or(0),
        skipped = state["rowsSkipped"].as_u64().unwrap_or(0),
        percent = format!("{:.0}", fraction * 100.0),
        seconds = format!("{seconds:.1}")
    );
    ui.label(RichText::new(counters).font(theme::regular(12.0)).color(p.text_secondary));
    let (track, _) = ui.allocate_exact_size(egui::vec2(WIDTH, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(track, 2, p.frame);
    let fill = egui::Rect::from_min_size(track.min, egui::vec2(track.width() * fraction, track.height()));
    if fill.width() > 0.0 {
        ui.painter().rect_filled(fill, 2, p.accent);
    }
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut CsvImportDialog) -> (Option<Choice>, bool) {
    let mut choice = None;
    let mut changed = false;
    let modal = egui::Modal::new(ui.scope_id().with("csv-import-dialog")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(t!("ide-dbcsv-title")).font(theme::semibold(15.0)).color(p.text));
        if dialog.running.is_some() {
            ui.add_space(6.0);
            progress(ui, p, dialog);
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if vcs::primary_button(ui, p, &t!("ide-dbcsv-background")).clicked() {
                    choice = Some(Choice::Background);
                }
                if vcs::secondary_button(ui, p, &t!("ide-dbcsv-stop-import")).clicked() {
                    choice = Some(Choice::StopImport);
                }
            });
            return;
        }
        steps(ui, p, dialog.step);
        ui.separator();
        match dialog.step {
            Step::File => file_step(ui, p, dialog),
            Step::Format => changed = format_step(ui, p, dialog),
            Step::Mapping => mapping_step(ui, p, dialog),
            Step::Settings => settings_step(ui, p, dialog),
            Step::Confirm => confirm_step(ui, p, dialog),
        }
        if let Some(error) = &dialog.error {
            ui.add(
                egui::Label::new(RichText::new(error.clone()).font(theme::regular(12.0)).color(p.error_text)).wrap(),
            );
        }
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let text = match (dialog.step, &dialog.confirm) {
                (Step::Confirm, Some(_)) => t!("ide-dbcsv-import-anyway"),
                (Step::Confirm, None) => t!("ide-dbcsv-import"),
                _ => t!("ide-dbcsv-next"),
            };
            if vcs::primary_button(ui, p, &text).clicked() {
                choice = Some(Choice::Next);
            }
            if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                choice = Some(Choice::Cancel);
            }
            if dialog.step != Step::File && vcs::secondary_button(ui, p, &t!("ide-dbcsv-back")).clicked() {
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

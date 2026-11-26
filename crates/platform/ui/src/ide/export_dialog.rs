use egui::{Align, Layout, RichText, Role, Ui};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::export::{Compression, Format, JsonStyle, LineEnding, Settings};
use zen_i18n::t;

use super::{IdePalette, IdeView, dialog_frame, vcs};
use crate::{accessibility, theme};

pub(super) const OPEN: &str = "view.database.export";

const WIDTH: f32 = 600.0;
const LABEL_WIDTH: f32 = 150.0;
const LIST_HEIGHT: f32 = 140.0;
const LISTED: usize = 500;

#[derive(Default)]
pub(super) struct ExportDialogRequest(pub(super) Option<Value>);

pub(super) fn request(session: &mut Session, params: Value) {
    session.state_mut::<ExportDialogRequest>().0 = Some(params);
}

#[derive(Clone, Debug)]
struct Table {
    path: Vec<String>,
    label: String,
    chosen: bool,
}

#[derive(Clone, Debug)]
pub(super) struct ExportDialog {
    source: Value,
    what: String,
    named: String,
    server_copy: bool,
    selection: Option<((u64, u64), (u64, u64))>,
    selected_only: bool,
    browse: bool,
    connection: Option<String>,
    tables: Vec<Table>,
    settings: Settings,
    batch: String,
    quote: String,
    row_group: String,
    clipboard: bool,
    folder: String,
    pattern: String,
    exists: Option<Vec<String>>,
    error: Option<String>,
    running: Option<u64>,
    progress: Value,
    fresh: bool,
    finished: bool,
}

fn message(error: zen_core::DispatchError) -> (Option<String>, String, Value) {
    match error {
        zen_core::DispatchError::Failed { error, .. } => {
            (error.code.clone(), error.message.clone(), error.data.clone().map(Value::Object).unwrap_or(Value::Null))
        }
        other => (None, other.to_string(), Value::Null),
    }
}

fn exportable(node: &Value) -> bool {
    let kind = &node["kind"];
    let data = matches!(kind.as_str(), Some("table" | "view"))
        || matches!(kind["other"].as_str(), Some("materializedView" | "foreignTable"));
    data && node["path"].as_array().is_some_and(|path| path.len() == 4)
}

fn strings(value: &Value) -> Vec<String> {
    value.as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

impl ExportDialog {
    pub(super) fn open(registry: &Registry, session: &mut Session, params: &Value) -> Result<Self, String> {
        let described = if params["source"].is_object() {
            params["source"].clone()
        } else if let Some(connection) = params["connection"].as_str() {
            match strings(&params["path"]).as_slice() {
                path @ [_, _, _, _] => json!({ "kind": "tables", "connection": connection, "paths": [path] }),
                _ => Value::Null,
            }
        } else {
            json!({ "kind": "result" })
        };
        let asked = if described.is_null() { json!({}) } else { json!({ "source": described }) };
        let defaults = registry
            .dispatch(session, "db.export.settings", asked.clone())
            .or_else(|_| registry.dispatch(session, "db.export.settings", json!({})))
            .map_err(|error| message(error).1)?;
        let info = &defaults["source"];
        let named = match (info["name"].as_str(), info["connectionName"].as_str()) {
            (Some(name), Some(connection)) if !name.is_empty() => format!("{name} · {connection}"),
            (_, Some(connection)) => connection.to_owned(),
            _ => String::new(),
        };
        let server_copy = info["serverCopy"] == true || (described.is_null() && params["connection"].is_string());
        let range = |value: &Value| Some((value[0].as_u64()?, value[1].as_u64()?));
        let selection = range(&params["selection"]["rows"]).zip(range(&params["selection"]["columns"]));
        let settings: Settings = serde_json::from_value(defaults["settings"].clone()).unwrap_or_default();
        let folder = defaults["folder"].as_str().or(defaults["defaultFolder"].as_str()).unwrap_or_default().to_owned();
        let mut dialog = Self {
            source: json!({ "kind": "result" }),
            what: t!("ide-dbx-source-result"),
            named,
            server_copy,
            selection,
            selected_only: false,
            browse: false,
            connection: None,
            tables: Vec::new(),
            batch: settings.batch.to_string(),
            quote: settings.quote.to_string(),
            row_group: settings.row_group.to_string(),
            settings,
            clipboard: defaults["clipboard"] == true,
            folder,
            pattern: defaults["pattern"].as_str().unwrap_or(zen_db::export::PATTERN).to_owned(),
            exists: None,
            error: None,
            running: None,
            progress: Value::Null,
            fresh: true,
            finished: false,
        };
        if params["source"].is_object() {
            dialog.source = params["source"].clone();
            dialog.what = match params["source"]["kind"].as_str() {
                Some("console") => t!("ide-dbx-source-console"),
                Some("query") => t!("ide-dbx-source-query"),
                Some("tables") => {
                    t!("ide-dbx-source-tables", count = params["source"]["paths"].as_array().map_or(0, Vec::len))
                }
                _ => t!("ide-dbx-source-result"),
            };
        } else if let Some(connection) = params["connection"].as_str() {
            dialog.connection = Some(connection.to_owned());
            dialog.list(registry, session, strings(&params["path"]));
        }
        Ok(dialog)
    }

    fn list(&mut self, registry: &Registry, session: &mut Session, path: Vec<String>) {
        let connection = self.connection.clone().unwrap_or_default();
        let folder = match path.len() {
            2 => [path.clone(), vec!["tables".to_owned()]].concat(),
            _ => path.clone(),
        };
        self.what = t!("ide-dbx-source-object", name = path.last().cloned().unwrap_or_default());
        if path.len() == 4 {
            self.tables = vec![Table { label: path[3].clone(), path, chosen: true }];
            return;
        }
        let params = json!({ "connection": connection, "path": folder, "limit": LISTED });
        match registry.dispatch(session, "db.schema.children", params) {
            Ok(answer) => {
                self.tables = answer["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|node| exportable(node))
                    .map(|node| Table {
                        path: strings(&node["path"]),
                        label: node["label"].as_str().unwrap_or_default().to_owned(),
                        chosen: true,
                    })
                    .collect();
                if self.tables.is_empty() {
                    self.error =
                        Some(answer["message"].as_str().map_or_else(|| t!("ide-dbx-no-tables"), str::to_owned));
                }
            }
            Err(error) => self.error = Some(message(error).1),
        }
    }

    fn source(&self) -> Value {
        if self.selected_only
            && let Some(((first, last), (left, right))) = self.selection
        {
            let id = self.source["id"].clone();
            return json!({ "kind": "cells", "id": id, "rows": [first, last], "columns": [left, right] });
        }
        match &self.connection {
            Some(connection) => {
                let paths: Vec<&Vec<String>> =
                    self.tables.iter().filter(|table| table.chosen).map(|table| &table.path).collect();
                json!({ "kind": "tables", "connection": connection, "paths": paths })
            }
            None => self.source.clone(),
        }
    }

    fn settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        settings.server_copy &= self.server_copy && !self.selected_only;
        settings.batch = self.batch.trim().parse().unwrap_or(settings.batch).clamp(1, 100_000);
        if let Some(quote) = self.quote.chars().next() {
            settings.quote = quote;
        }
        settings.row_group = self.row_group.trim().parse().unwrap_or(settings.row_group).clamp(1_000, 1_000_000);
        settings
    }

    pub(super) fn start_call(&self, overwrite: bool) -> Value {
        let mut params = json!({
            "source": self.source(),
            "settings": self.settings(),
            "clipboard": self.clipboard && !self.settings.format.binary(),
            "pattern": self.pattern,
            "overwrite": overwrite,
        });
        if !self.folder.trim().is_empty() {
            params["folder"] = json!(self.folder.trim());
        }
        params
    }

    fn start(&mut self, registry: &Registry, session: &mut Session) {
        let overwrite = self.exists.is_some();
        match registry.dispatch(session, "db.export.start", self.start_call(overwrite)) {
            Ok(started) if started["job"].is_u64() || started["clipboard"] == true && self.selected_only => {
                self.running = None;
                self.exists = None;
                self.error = None;
                self.fresh = true;
                self.browse = false;
                self.finished = true;
            }
            Ok(started) => {
                self.running = started["id"].as_u64();
                self.exists = None;
                self.error = None;
            }
            Err(error) => {
                let (code, text, data) = message(error);
                if code.as_deref() == Some("exists") {
                    self.exists = Some(strings(&data["paths"]));
                    self.error = None;
                } else {
                    self.exists = None;
                    self.error = Some(text);
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn set_folder(&mut self, folder: &str) {
        folder.clone_into(&mut self.folder);
    }

    #[cfg(test)]
    pub(super) fn running(&self) -> Option<u64> {
        self.running
    }
}

enum Choice {
    Start,
    Cancel,
    StopExport,
    Background,
}

impl IdeView {
    pub(super) fn export_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if let Some(text) = session.state_mut::<zen_workbench::database::exports::Exports>().take_clipboard() {
            ui.ctx().copy_text(text);
        }
        if let Some(params) = session.state_mut::<ExportDialogRequest>().0.take() {
            match ExportDialog::open(registry, session, &params) {
                Ok(dialog) => self.export_dialog = Some(dialog),
                Err(error) => self.notify(zen_workbench::notifications::NewNotification::message(
                    zen_workbench::notifications::Group::General,
                    zen_workbench::notifications::Kind::Error,
                    error,
                )),
            }
        }
        let Some(dialog) = &mut self.export_dialog else { return };
        if let Some(id) = dialog.running {
            let state = registry.dispatch(session, "db.export.state", json!({ "id": id })).unwrap_or_default();
            dialog.progress = state["exports"][0].clone();
            if dialog.progress["status"] != "running" {
                self.export_dialog = None;
                return;
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        if std::mem::take(&mut dialog.browse)
            && let Some(folder) = rfd::FileDialog::new()
                .set_title(t!("ide-dbx-choose-folder"))
                .set_directory(dialog.folder.trim())
                .pick_folder()
        {
            dialog.folder = folder.display().to_string();
        }
        let p = self.palette;
        match show(ui, &p, dialog) {
            Some(Choice::Start) => {
                dialog.start(registry, session);
                if dialog.finished {
                    self.export_dialog = None;
                }
            }
            Some(Choice::StopExport) => {
                if let Some(id) = dialog.running {
                    let _ = registry.dispatch(session, "db.export.cancel", json!({ "id": id }));
                }
            }
            Some(Choice::Cancel | Choice::Background) => self.export_dialog = None,
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

fn tables(ui: &mut Ui, p: &IdePalette, dialog: &mut ExportDialog) {
    if dialog.connection.is_none() || dialog.tables.len() < 2 {
        return;
    }
    ui.horizontal(|ui| {
        let chosen = dialog.tables.iter().filter(|table| table.chosen).count();
        ui.label(
            RichText::new(t!("ide-dbx-tables-chosen", chosen = chosen, count = dialog.tables.len()))
                .font(theme::regular(12.0))
                .color(p.text_secondary),
        );
        let all = dialog.tables.iter().all(|table| table.chosen);
        let text = if all { t!("ide-dbx-select-none") } else { t!("ide-dbx-select-all") };
        if vcs::secondary_button(ui, p, &text).clicked() {
            for table in &mut dialog.tables {
                table.chosen = !all;
            }
        }
    });
    egui::ScrollArea::vertical().id_salt("dbx-tables").max_height(LIST_HEIGHT).show(ui, |ui| {
        ui.set_width(WIDTH);
        ui.horizontal_wrapped(|ui| {
            for table in &mut dialog.tables {
                ui.allocate_ui(egui::vec2(186.0, 20.0), |ui| check(ui, p, &mut table.chosen, &table.label));
            }
        });
    });
}

fn format_settings(ui: &mut Ui, p: &IdePalette, dialog: &mut ExportDialog) {
    let format = dialog.settings.format;
    match format {
        Format::Csv | Format::Tsv => {
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbx-delimiter"));
                if format == Format::Csv {
                    field(ui, p, &t!("ide-dbx-delimiter"), &mut dialog.settings.delimiter, 40.0);
                } else {
                    ui.label(RichText::new(t!("ide-dbx-tab")).font(theme::regular(13.0)).color(p.text_secondary));
                }
                ui.add_space(12.0);
                ui.label(RichText::new(t!("ide-dbx-quote")).font(theme::regular(13.0)).color(p.text));
                field(ui, p, &t!("ide-dbx-quote"), &mut dialog.quote, 24.0);
                ui.add_space(12.0);
                ui.label(RichText::new(t!("ide-dbx-null")).font(theme::regular(13.0)).color(p.text));
                field(ui, p, &t!("ide-dbx-null"), &mut dialog.settings.null_text, 70.0);
            });
            ui.horizontal(|ui| {
                label(ui, p, "");
                check(ui, p, &mut dialog.settings.header, &t!("ide-dbx-header"));
                check(ui, p, &mut dialog.settings.quote_all, &t!("ide-dbx-quote-all"));
                check(ui, p, &mut dialog.settings.escape_formulas, &t!("ide-dbx-escape-formulas"));
            });
            if dialog.server_copy && !dialog.selected_only {
                ui.horizontal(|ui| {
                    label(ui, p, "");
                    check(ui, p, &mut dialog.settings.server_copy, &t!("ide-dbx-server-copy"));
                });
            }
        }
        Format::Json => {
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbx-layout"));
                radio(ui, p, &mut dialog.settings.json, JsonStyle::Array, &t!("ide-dbx-json-array"));
                radio(ui, p, &mut dialog.settings.json, JsonStyle::Lines, &t!("ide-dbx-json-lines"));
            });
        }
        Format::Sql => {
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbx-table"));
                let mut table = dialog.settings.table.clone().unwrap_or_default();
                field(ui, p, &t!("ide-dbx-table"), &mut table, 200.0);
                dialog.settings.table = Some(table).filter(|table| !table.trim().is_empty());
                ui.add_space(12.0);
                ui.label(RichText::new(t!("ide-dbx-batch")).font(theme::regular(13.0)).color(p.text));
                field(ui, p, &t!("ide-dbx-batch"), &mut dialog.batch, 50.0);
            });
            ui.horizontal(|ui| {
                label(ui, p, "");
                hint(ui, p, &t!("ide-dbx-table-hint"));
            });
        }
        Format::Markdown | Format::Html => {
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbx-null"));
                field(ui, p, &t!("ide-dbx-null"), &mut dialog.settings.null_text, 70.0);
            });
        }
        Format::Xml => {}
        Format::Parquet => {
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbx-compression"));
                for codec in Compression::ALL {
                    radio(ui, p, &mut dialog.settings.compression, codec, codec.name());
                }
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbx-row-group"));
                field(ui, p, &t!("ide-dbx-row-group"), &mut dialog.row_group, 80.0);
            });
            ui.horizontal(|ui| {
                label(ui, p, "");
                hint(ui, p, &t!("ide-dbx-parquet-hint"));
            });
            return;
        }
    }
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbx-line-ending"));
        radio(ui, p, &mut dialog.settings.line_ending, LineEnding::Lf, "LF");
        radio(ui, p, &mut dialog.settings.line_ending, LineEnding::CrLf, "CRLF");
        ui.add_space(12.0);
        ui.label(RichText::new(t!("ide-dbx-encoding")).font(theme::regular(13.0)).color(p.text));
        ui.label(RichText::new("UTF-8").font(theme::regular(13.0)).color(p.text_secondary));
        check(ui, p, &mut dialog.settings.bom, &t!("ide-dbx-bom"));
    });
}

fn output(ui: &mut Ui, p: &IdePalette, dialog: &mut ExportDialog) {
    let binary = dialog.settings.format.binary();
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbx-output"));
        if binary {
            ui.label(RichText::new(t!("ide-dbx-to-file")).font(theme::regular(13.0)).color(p.text_secondary));
        } else {
            radio(ui, p, &mut dialog.clipboard, false, &t!("ide-dbx-to-file"));
            radio(ui, p, &mut dialog.clipboard, true, &t!("ide-dbx-to-clipboard"));
        }
    });
    if dialog.clipboard && !binary {
        return;
    }
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbx-folder"));
        let browse = t!("ide-dbx-browse");
        let room = WIDTH
            - LABEL_WIDTH
            - 40.0
            - ui.fonts_mut(|fonts| fonts.layout_no_wrap(browse.clone(), theme::regular(13.0), p.text).size().x);
        field(ui, p, &t!("ide-dbx-folder"), &mut dialog.folder, room - 16.0);
        if vcs::secondary_button(ui, p, &browse).clicked() {
            dialog.browse = true;
        }
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbx-file-name"));
        field(ui, p, &t!("ide-dbx-file-name"), &mut dialog.pattern, 240.0);
        let extension = dialog.settings.format.extension(&dialog.settings);
        ui.label(RichText::new(format!(".{extension}")).font(theme::regular(13.0)).color(p.text_secondary));
    });
    ui.horizontal(|ui| {
        label(ui, p, "");
        hint(ui, p, &t!("ide-dbx-pattern-hint"));
    });
}

fn progress(ui: &mut Ui, p: &IdePalette, dialog: &ExportDialog) {
    let state = &dialog.progress;
    let items = state["items"].as_array().cloned().unwrap_or_default();
    let current = state["current"].as_u64().and_then(|at| usize::try_from(at).ok()).unwrap_or(0);
    let name = items.get(current).and_then(|item| item["name"].as_str()).unwrap_or_default();
    let line = if items.len() > 1 {
        t!("ide-dbx-exporting-of", name = name, at = current + 1, count = items.len())
    } else {
        t!("ide-dbx-exporting", name = name)
    };
    ui.label(RichText::new(line).font(theme::regular(13.0)).color(p.text));
    let rows = state["rows"].as_u64().unwrap_or(0);
    let bytes = zen_workbench::database::objects::size_text(state["bytes"].as_u64().unwrap_or(0));
    let seconds = state["elapsedMs"].as_u64().unwrap_or(0) as f64 / 1000.0;
    let counters = t!("ide-dbx-counters", rows = rows, bytes = bytes, seconds = format!("{seconds:.1}"));
    ui.label(RichText::new(counters).font(theme::regular(12.0)).color(p.text_secondary));
    let done = items.iter().filter(|item| item["status"] == "done").count();
    let (track, _) = ui.allocate_exact_size(egui::vec2(WIDTH, 4.0), egui::Sense::hover());
    ui.painter().rect_filled(track, 2, p.frame);
    let fill = if items.len() > 1 {
        let fraction = done as f32 / items.len() as f32;
        egui::Rect::from_min_size(track.min, egui::vec2(track.width() * fraction, track.height()))
    } else {
        let phase = (ui.input(|input| input.time) as f32 * 0.6).fract();
        let left = track.left() + (track.width() * 1.25) * phase - track.width() * 0.25;
        egui::Rect::from_min_max(
            egui::pos2(left.max(track.left()), track.top()),
            egui::pos2((left + track.width() * 0.25).min(track.right()), track.bottom()),
        )
    };
    if fill.width() > 0.0 {
        ui.painter().rect_filled(fill, 2, p.accent);
    }
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut ExportDialog) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("export-dialog")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(t!("ide-dbx-title")).font(theme::semibold(15.0)).color(p.text));
        if !dialog.named.is_empty() {
            ui.add(
                egui::Label::new(RichText::new(dialog.named.clone()).font(theme::semibold(13.0)).color(p.text))
                    .truncate(),
            );
        }
        let what = if dialog.selected_only { t!("ide-dbx-source-cells") } else { dialog.what.clone() };
        ui.label(RichText::new(what).font(theme::regular(12.5)).color(p.text_secondary));
        ui.add_space(6.0);
        if dialog.running.is_some() {
            progress(ui, p, dialog);
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if vcs::primary_button(ui, p, &t!("ide-dbx-background")).clicked() {
                    choice = Some(Choice::Background);
                }
                if vcs::secondary_button(ui, p, &t!("ide-dbx-stop")).clicked() {
                    choice = Some(Choice::StopExport);
                }
            });
            return;
        }
        tables(ui, p, dialog);
        if let Some(((first, last), (left, right))) = dialog.selection {
            ui.horizontal(|ui| {
                label(ui, p, "");
                let text = t!(
                    "ide-dbx-selected-only",
                    rows = last.saturating_sub(first) + 1,
                    columns = right.saturating_sub(left) + 1
                );
                check(ui, p, &mut dialog.selected_only, &text);
            });
            if dialog.selected_only && dialog.settings.format.binary() {
                dialog.settings.format = Format::Csv;
            }
        }
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbx-format"));
            let layout = Layout::left_to_right(Align::Center).with_main_wrap(true);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), super::form::ROW), layout, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                for format in Format::ALL {
                    let enabled = !(dialog.selected_only && format.binary());
                    ui.add_enabled_ui(enabled, |ui| radio(ui, p, &mut dialog.settings.format, format, format.name()));
                }
            });
        });
        format_settings(ui, p, dialog);
        ui.separator();
        output(ui, p, dialog);
        if let Some(paths) = &dialog.exists {
            let text = t!("ide-dbx-exists", count = paths.len(), path = paths.first().cloned().unwrap_or_default());
            ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.warning_text)).wrap());
        }
        if let Some(error) = &dialog.error {
            ui.add(
                egui::Label::new(RichText::new(error.clone()).font(theme::regular(12.0)).color(p.error_text)).wrap(),
            );
        }
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let ready = dialog.connection.is_none() || dialog.tables.iter().any(|table| table.chosen);
            let text = if dialog.exists.is_some() { t!("ide-dbx-replace") } else { t!("ide-dbx-export") };
            let start = ui.add_enabled_ui(ready, |ui| vcs::primary_button(ui, p, &text)).inner;
            if start.clicked() {
                choice = Some(Choice::Start);
            }
            if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                choice = Some(Choice::Cancel);
            }
        });
    });
    let fresh = std::mem::replace(&mut dialog.fresh, false);
    choice.or_else(|| {
        (modal.should_close() && !fresh).then_some(if dialog.running.is_some() {
            Choice::Background
        } else {
            Choice::Cancel
        })
    })
}

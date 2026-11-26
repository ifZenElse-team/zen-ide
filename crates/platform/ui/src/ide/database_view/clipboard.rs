use std::ops::Range;
use std::time::Duration;

use egui::accesskit::Role;
use egui::{RichText, Ui};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::clip::{CopySettings, Quoting, ValueStyle};
use zen_i18n::t;

use super::super::data_grid::{GridAction, GridOutcome};
use super::super::{IdePalette, theme, vcs};
use super::DatabaseView;
use crate::accessibility;

const PREVIEW_ROWS: usize = 8;
const PREVIEW_COLUMNS: usize = 8;
const PROBLEMS_SHOWN: usize = 6;

#[derive(Clone, Debug)]
struct AdvancedCopy {
    result: u64,
    rows: Range<u64>,
    columns: Range<usize>,
    settings: CopySettings,
    column_delimiter: String,
    quote: String,
    error: Option<String>,
}

#[derive(Clone, Debug)]
struct AdvancedPaste {
    result: u64,
    row: u64,
    column: usize,
    text: String,
    insert: bool,
    delimiter: String,
    header: Option<bool>,
    null_text: String,
    checked: Value,
    error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wanted {
    Copy,
    Paste,
}

#[derive(Default)]
pub(super) struct Clip {
    advanced: Option<AdvancedCopy>,
    paste: Option<AdvancedPaste>,
    job: Option<(u64, u64)>,
    wanted: Option<Wanted>,
}

impl Clip {
    pub(super) fn popups(&self) -> Vec<String> {
        let mut open = Vec::new();
        if self.advanced.is_some() {
            open.push("databaseAdvancedCopy".to_owned());
        }
        if self.paste.is_some() {
            open.push("databaseAdvancedPaste".to_owned());
        }
        open
    }

    pub(super) fn want_copy(&mut self) {
        self.wanted = Some(Wanted::Copy);
    }

    pub(super) fn want_paste(&mut self) {
        self.wanted = Some(Wanted::Paste);
    }

    pub(super) fn job_of(&self, id: u64) -> Option<u64> {
        self.job.filter(|(result, _)| *result == id).map(|(_, job)| job)
    }
}

fn shown_delimiter(text: &str) -> String {
    text.replace('\t', "\\t")
}

fn typed_delimiter(text: &str) -> String {
    text.replace("\\t", "\t")
}

fn code_of(error: &zen_core::DispatchError) -> Option<&str> {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.code.as_deref(),
        _ => None,
    }
}

fn copied(view: &mut DatabaseView, id: u64, answer: Result<Value, zen_core::DispatchError>) {
    let Some(answer) = view.results.data.answer(id, answer) else { return };
    if let Some(job) = answer["job"].as_u64() {
        view.results.clip.job = Some((id, job));
    }
    if let Some(message) = answer["message"].as_str() {
        view.results.data.tell(id, message.to_owned(), false);
    }
}

fn selection(rows: &Range<u64>, columns: &Range<usize>) -> Value {
    json!({ "rows": [rows.start, rows.end.saturating_sub(1)], "columns": [columns.start, columns.end.saturating_sub(1)] })
}

fn with_selection(mut params: Value, rows: &Range<u64>, columns: &Range<usize>) -> Value {
    if let (Some(params), Value::Object(chosen)) = (params.as_object_mut(), selection(rows, columns)) {
        params.extend(chosen);
    }
    params
}

fn open_advanced(
    view: &mut DatabaseView,
    registry: &Registry,
    session: &mut Session,
    id: u64,
    cells: (Range<u64>, Range<usize>),
) {
    let kept = registry.dispatch(session, "db.copy.settings", json!({})).unwrap_or_default();
    let settings: CopySettings = serde_json::from_value(kept["settings"].clone()).unwrap_or_default();
    view.results.clip.advanced = Some(AdvancedCopy {
        result: id,
        rows: cells.0,
        columns: cells.1,
        column_delimiter: shown_delimiter(&settings.column_delimiter),
        quote: settings.quote.to_string(),
        settings,
        error: None,
    });
}

fn check_paste(registry: &Registry, session: &mut Session, dialog: &mut AdvancedPaste) {
    let mut params = json!({
        "id": dialog.result,
        "text": dialog.text,
        "row": dialog.row,
        "column": dialog.column,
        "mode": if dialog.insert { "insert" } else { "overwrite" },
        "nullText": dialog.null_text,
        "dryRun": true,
        "preview": PREVIEW_ROWS + 1,
        "limit": PROBLEMS_SHOWN,
    });
    if !dialog.delimiter.is_empty() {
        params["delimiter"] = json!(dialog.delimiter);
    }
    if let Some(header) = dialog.header {
        params["header"] = json!(header);
    }
    match registry.dispatch(session, "db.result.paste", params) {
        Ok(answer) => {
            if dialog.header.is_none() {
                dialog.header = answer["header"].as_bool();
            }
            dialog.checked = answer;
            dialog.error = None;
        }
        Err(error) => {
            dialog.checked = Value::Null;
            dialog.error = Some(super::message(error));
        }
    }
}

fn open_paste(
    view: &mut DatabaseView,
    registry: &Registry,
    session: &mut Session,
    id: u64,
    (row, column): (u64, usize),
    text: Option<String>,
    insert: bool,
) {
    let text = text.unwrap_or_else(|| zen_workbench::file_clipboard::clipboard_text(session));
    let kept = registry.dispatch(session, "db.copy.settings", json!({})).unwrap_or_default();
    let mut dialog = AdvancedPaste {
        result: id,
        row,
        column,
        text,
        insert,
        delimiter: String::new(),
        header: None,
        null_text: kept["pasteNullText"].as_str().unwrap_or_default().to_owned(),
        checked: Value::Null,
        error: None,
    };
    check_paste(registry, session, &mut dialog);
    view.results.clip.paste = Some(dialog);
}

fn paste(
    view: &mut DatabaseView,
    registry: &Registry,
    session: &mut Session,
    id: u64,
    (row, column): (u64, usize),
    text: Option<String>,
    insert: bool,
) {
    let mut params =
        json!({ "id": id, "row": row, "column": column, "mode": if insert { "insert" } else { "overwrite" } });
    if let Some(text) = &text {
        params["text"] = json!(text);
    }
    match registry.dispatch(session, "db.result.paste", params) {
        Ok(answer) => {
            let note = t!(
                "ide-database-paste-done",
                cells = answer["cells"].as_u64().unwrap_or(0),
                rows = answer["newRows"].as_u64().unwrap_or(0)
            );
            view.results.data.tell(id, note, false);
        }
        Err(error) if code_of(&error) == Some("invalidPaste") => {
            open_paste(view, registry, session, id, (row, column), text, insert);
        }
        Err(error) => view.results.data.tell(id, super::message(error), true),
    }
}

pub(super) fn act(
    view: &mut DatabaseView,
    registry: &Registry,
    session: &mut Session,
    id: u64,
    outcome: &GridOutcome,
    cells: Option<(Range<u64>, Range<usize>)>,
) {
    if let Some((rows, columns, header)) = outcome.copy.clone()
        && rows.end > rows.start
        && columns.end > columns.start
    {
        let params = with_selection(json!({ "id": id, "header": header, "clipboard": true }), &rows, &columns);
        let answer = registry.dispatch(session, "db.result.copy", params);
        copied(view, id, answer);
    }
    for action in &outcome.actions {
        match action {
            GridAction::CopyAs { format, rows, columns } => {
                let params = with_selection(json!({ "id": id, "format": format, "clipboard": true }), rows, columns);
                let answer = registry.dispatch(session, "db.result.copyAs", params);
                copied(view, id, answer);
            }
            GridAction::CopyAdvanced { rows, columns } => {
                open_advanced(view, registry, session, id, (rows.clone(), columns.clone()));
            }
            GridAction::Paste { row, column, text, insert } => {
                paste(view, registry, session, id, (*row, *column), text.clone(), *insert);
            }
            GridAction::PasteAdvanced { row, column } => {
                open_paste(view, registry, session, id, (*row, *column), None, false);
            }
            _ => {}
        }
    }
    match view.results.clip.wanted.take() {
        Some(Wanted::Copy) => {
            if let Some(cells) = cells.filter(|(rows, columns)| !rows.is_empty() && !columns.is_empty()) {
                open_advanced(view, registry, session, id, cells);
            }
        }
        Some(Wanted::Paste) => {
            let anchor = cells.map_or((0, 0), |(rows, columns)| (rows.start, columns.start));
            open_paste(view, registry, session, id, anchor, None, false);
        }
        None => {}
    }
}

pub(super) fn poll(view: &mut DatabaseView, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
    let Some((result, job)) = view.results.clip.job else { return };
    let state = registry.dispatch(session, "db.result.copyState", json!({ "job": job })).unwrap_or_default();
    let job_state = &state["jobs"][0];
    if job_state["status"] == "running" {
        ctx.request_repaint_after(Duration::from_millis(100));
        return;
    }
    view.results.clip.job = None;
    let note = match job_state["message"].as_str() {
        Some(message) => message.to_owned(),
        None => t!("ide-database-copy-done", rows = job_state["rows"].as_u64().unwrap_or(0)),
    };
    view.results.data.tell(result, note, false);
}

pub(super) fn cancel(view: &mut DatabaseView, registry: &Registry, session: &mut Session, id: u64) {
    if let Some(job) = view.results.clip.job_of(id) {
        let _ = registry.dispatch(session, "db.result.copyState", json!({ "job": job, "cancel": true }));
    }
}

fn label(ui: &mut Ui, p: &IdePalette, text: &str) {
    ui.add_sized([150.0, 20.0], egui::Label::new(RichText::new(text).font(theme::regular(13.0)).color(p.text)));
}

fn field(ui: &mut Ui, p: &IdePalette, name: &str, value: &mut String, width: f32) -> egui::Response {
    let edit = egui::TextEdit::singleline(value).desired_width(width);
    let response = super::super::connection_dialog::input(ui, p, edit);
    accessibility::name(&response, Role::TextInput, name);
    response
}

fn check(ui: &mut Ui, p: &IdePalette, value: &mut bool, name: &str) -> egui::Response {
    let response = ui.checkbox(value, RichText::new(name).font(theme::regular(13.0)).color(p.text));
    accessibility::name(&response, Role::CheckBox, name);
    response
}

fn radio<T: PartialEq + Copy>(ui: &mut Ui, p: &IdePalette, value: &mut T, choice: T, name: &str) -> egui::Response {
    let response = ui.radio_value(value, choice, RichText::new(name).font(theme::regular(13.0)).color(p.text));
    accessibility::name(&response, Role::RadioButton, name);
    response
}

fn title(ui: &mut Ui, p: &IdePalette, text: &str) {
    ui.label(RichText::new(text).font(theme::semibold(15.0)).color(p.text));
    ui.add_space(8.0);
}

fn advanced_copy(view: &mut DatabaseView, ui: &Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
    let Some(mut dialog) = view.results.clip.advanced.clone() else { return };
    let mut close = false;
    let mut copy = false;
    let modal = egui::Modal::new(ui.scope_id().with("database-advanced-copy"))
        .frame(super::super::dialog_frame(p))
        .show(ui.ctx(), |ui| {
            ui.set_width(440.0);
            title(ui, p, &t!("ide-database-copy-advanced-title"));
            let settings = &mut dialog.settings;
            ui.horizontal(|ui| {
                label(ui, p, "");
                check(ui, p, &mut settings.header, &t!("ide-database-copy-header"));
                check(ui, p, &mut settings.row_numbers, &t!("ide-database-copy-row-numbers"));
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-copy-column-delimiter"));
                field(ui, p, &t!("ide-database-copy-column-delimiter"), &mut dialog.column_delimiter, 60.0);
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-copy-row-delimiter"));
                let mut crlf = settings.row_delimiter == "\r\n";
                radio(ui, p, &mut crlf, false, "LF");
                radio(ui, p, &mut crlf, true, "CRLF");
                settings.row_delimiter = if crlf { "\r\n" } else { "\n" }.to_owned();
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-copy-quote"));
                field(ui, p, &t!("ide-database-copy-quote"), &mut dialog.quote, 24.0);
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-copy-quoting"));
                radio(ui, p, &mut settings.quoting, Quoting::Needed, &t!("ide-database-copy-quoting-needed"));
                radio(ui, p, &mut settings.quoting, Quoting::Always, &t!("ide-database-copy-quoting-always"));
                radio(ui, p, &mut settings.quoting, Quoting::Never, &t!("ide-database-copy-quoting-never"));
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-copy-null"));
                field(ui, p, &t!("ide-database-copy-null"), &mut settings.null_text, 90.0);
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-copy-values"));
                radio(ui, p, &mut settings.values, ValueStyle::Display, &t!("ide-database-copy-values-display"));
                radio(ui, p, &mut settings.values, ValueStyle::Sql, &t!("ide-database-copy-values-sql"));
                radio(ui, p, &mut settings.values, ValueStyle::Json, &t!("ide-database-copy-values-json"));
            });
            if let Some(error) = &dialog.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).font(theme::regular(12.5)).color(p.error_text));
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                if vcs::primary_button(ui, p, &t!("ide-database-copy-button")).clicked() {
                    copy = true;
                }
                if vcs::secondary_button(ui, p, &t!("ide-database-preview-close")).clicked() {
                    close = true;
                }
            });
        });
    if copy {
        dialog.settings.column_delimiter = typed_delimiter(&dialog.column_delimiter);
        let mut quote = dialog.quote.chars();
        dialog.settings.quote = quote.next().filter(|_| quote.next().is_none()).unwrap_or('"');
        let params = with_selection(
            json!({ "id": dialog.result, "settings": dialog.settings, "remember": true, "clipboard": true }),
            &dialog.rows,
            &dialog.columns,
        );
        match registry.dispatch(session, "db.result.copy", params) {
            Ok(answer) => {
                copied(view, dialog.result, Ok(answer));
                close = true;
            }
            Err(error) => dialog.error = Some(super::message(error)),
        }
    }
    view.results.clip.advanced = if close || modal.should_close() { None } else { Some(dialog) };
}

fn cell_text(value: &Value) -> (String, bool) {
    match value.as_str() {
        Some(text) => (text.replace(['\n', '\r'], " ").chars().take(40).collect(), false),
        None => ("[NULL]".to_owned(), true),
    }
}

fn advanced_paste(view: &mut DatabaseView, ui: &Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
    let Some(mut dialog) = view.results.clip.paste.clone() else { return };
    let mut close = false;
    let mut apply = false;
    let mut changed = false;
    let modal = egui::Modal::new(ui.scope_id().with("database-advanced-paste"))
        .frame(super::super::dialog_frame(p))
        .show(ui.ctx(), |ui| {
            ui.set_width(660.0);
            title(ui, p, &t!("ide-database-paste-title"));
            let checked = &dialog.checked;
            if checked.is_object() {
                let delimiter = match checked["delimiter"].as_str() {
                    Some("\t") => t!("ide-database-paste-tab"),
                    Some(other) => format!("“{other}”"),
                    None => String::new(),
                };
                let summary = t!(
                    "ide-database-paste-summary",
                    rows = checked["rows"].as_u64().unwrap_or(0),
                    columns = checked["columns"].as_u64().unwrap_or(0),
                    delimiter = delimiter
                );
                ui.label(RichText::new(summary).font(theme::regular(12.5)).color(p.text_secondary));
                ui.add_space(8.0);
            }
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-paste-delimiter"));
                for (value, name) in [
                    ("", t!("ide-database-paste-detect")),
                    ("\t", t!("ide-database-paste-tab")),
                    (",", ",".to_owned()),
                    (";", ";".to_owned()),
                    ("|", "|".to_owned()),
                ] {
                    let mut current = dialog.delimiter.as_str();
                    if radio(ui, p, &mut current, value, &name).clicked() {
                        dialog.delimiter = value.to_owned();
                        changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                label(ui, p, "");
                let mut header = dialog.header.unwrap_or(false);
                if check(ui, p, &mut header, &t!("ide-database-paste-header")).changed() {
                    dialog.header = Some(header);
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-paste-mode"));
                changed |= radio(ui, p, &mut dialog.insert, false, &t!("ide-database-paste-overwrite")).changed();
                changed |= radio(ui, p, &mut dialog.insert, true, &t!("ide-database-paste-insert")).changed();
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-database-paste-null"));
                changed |= field(ui, p, &t!("ide-database-paste-null"), &mut dialog.null_text, 90.0).changed();
            });
            ui.add_space(8.0);
            let preview = dialog.checked["preview"].as_array().cloned().unwrap_or_default();
            let skip = usize::from(dialog.checked["header"] == true);
            if !preview.is_empty() {
                egui::Frame::NONE.fill(p.editor_background).inner_margin(6.0).corner_radius(4.0).show(ui, |ui| {
                    egui::Grid::new("database-paste-preview").striped(true).spacing([14.0, 3.0]).show(ui, |ui| {
                        for (at, row) in preview.iter().take(PREVIEW_ROWS + skip).enumerate() {
                            for value in row.as_array().into_iter().flatten().take(PREVIEW_COLUMNS) {
                                let (text, null) = cell_text(value);
                                let color = if at < skip || null { p.text_secondary } else { p.text };
                                let font = if at < skip { theme::semibold(12.0) } else { theme::regular(12.0) };
                                ui.label(RichText::new(text).font(font).color(color));
                            }
                            ui.end_row();
                        }
                    });
                });
                ui.add_space(8.0);
            }
            let total = dialog.checked["problemsTotal"].as_u64().unwrap_or(0);
            if total > 0 {
                ui.label(
                    RichText::new(t!("ide-database-paste-problems", count = total))
                        .font(theme::semibold(12.5))
                        .color(p.warning_text),
                );
                for problem in dialog.checked["problems"].as_array().into_iter().flatten() {
                    let line = t!(
                        "ide-database-paste-problem",
                        line = problem["line"].as_u64().unwrap_or(0),
                        column = problem["name"].as_str().unwrap_or_default(),
                        message = problem["message"].as_str().unwrap_or_default()
                    );
                    ui.label(RichText::new(line).font(theme::regular(12.0)).color(p.error_text));
                }
            } else if dialog.checked.is_object() {
                let note = t!(
                    "ide-database-paste-ready",
                    cells = dialog.checked["cells"].as_u64().unwrap_or(0),
                    rows = dialog.checked["newRows"].as_u64().unwrap_or(0)
                );
                ui.label(RichText::new(note).font(theme::regular(12.5)).color(p.text_secondary));
            }
            if let Some(error) = &dialog.error {
                ui.label(RichText::new(error).font(theme::regular(12.5)).color(p.error_text));
            }
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let label = if total > 0 { t!("ide-database-paste-valid") } else { t!("ide-database-paste-button") };
                let can =
                    dialog.checked["cells"].as_u64().unwrap_or(0) + dialog.checked["newRows"].as_u64().unwrap_or(0) > 0;
                ui.add_enabled_ui(can, |ui| {
                    if vcs::primary_button(ui, p, &label).clicked() {
                        apply = true;
                    }
                });
                if vcs::secondary_button(ui, p, &t!("ide-database-preview-close")).clicked() {
                    close = true;
                }
            });
        });
    if changed {
        check_paste(registry, session, &mut dialog);
    }
    if apply {
        let mut params = json!({
            "id": dialog.result,
            "text": dialog.text,
            "row": dialog.row,
            "column": dialog.column,
            "mode": if dialog.insert { "insert" } else { "overwrite" },
            "nullText": dialog.null_text,
            "skipInvalid": true,
        });
        if !dialog.delimiter.is_empty() {
            params["delimiter"] = json!(dialog.delimiter);
        }
        if let Some(header) = dialog.header {
            params["header"] = json!(header);
        }
        let _ = registry.dispatch(session, "db.copy.settings", json!({ "pasteNullText": dialog.null_text }));
        match registry.dispatch(session, "db.result.paste", params) {
            Ok(answer) => {
                let note = t!(
                    "ide-database-paste-done",
                    cells = answer["cells"].as_u64().unwrap_or(0),
                    rows = answer["newRows"].as_u64().unwrap_or(0)
                );
                view.results.data.tell(dialog.result, note, false);
                close = true;
            }
            Err(error) => dialog.error = Some(super::message(error)),
        }
    }
    view.results.clip.paste = if close || modal.should_close() { None } else { Some(dialog) };
}

pub(super) fn dialogs(view: &mut DatabaseView, ui: &Ui, p: &IdePalette, registry: &Registry, session: &mut Session) {
    advanced_copy(view, ui, p, registry, session);
    advanced_paste(view, ui, p, registry, session);
}

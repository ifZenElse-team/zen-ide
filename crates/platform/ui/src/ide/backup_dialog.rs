use egui::{Align, Layout, RichText, Role, Ui};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_db::backup::{
    Content, DumpMethod, MysqlRestoreOptions, MysqldumpOptions, PgDumpOptions, PgFormat, PgRestoreOptions,
};
use zen_i18n::t;

use super::{IdePalette, IdeView, dialog_frame, vcs};
use crate::{accessibility, theme};

pub(super) const OPEN_BACKUP: &str = "view.database.backup";

pub(super) const OPEN_RESTORE: &str = "view.database.restore";

const WIDTH: f32 = 600.0;
const LABEL: f32 = 128.0;
const POLL: f64 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mode {
    Backup,
    Restore,
}

#[derive(Default)]
pub(super) struct BackupDialogRequest(pub(super) Option<(Mode, Value)>);

pub(super) fn request(session: &mut Session, mode: Mode, params: Value) {
    session.state_mut::<BackupDialogRequest>().0 = Some((mode, params));
}

#[derive(Clone, Debug)]
pub(super) struct BackupDialog {
    mode: Mode,
    connection: String,
    name: String,
    family: String,
    database: String,
    tools: Value,
    tool: Option<usize>,
    pg_dump: PgDumpOptions,
    mysqldump: MysqldumpOptions,
    pg_restore: PgRestoreOptions,
    mysql: MysqlRestoreOptions,
    schemas: String,
    tables: String,
    compression: String,
    jobs: String,
    extra: String,
    folder: String,
    file: String,
    password: Option<String>,
    exists: bool,
    confirm: Option<(String, String)>,
    error: Option<String>,
    polled: f64,
    fresh: bool,
    pinned: Option<Value>,
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

fn list(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect()
}

fn words(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

fn number(text: &str) -> Option<u8> {
    text.trim().parse().ok()
}

fn from<T: serde::de::DeserializeOwned + Default>(value: &Value) -> T {
    serde_json::from_value(value.clone()).unwrap_or_default()
}

impl BackupDialog {
    pub(super) fn open(registry: &Registry, session: &mut Session, mode: Mode, params: &Value) -> Result<Self, String> {
        let (connection, path) = match params["connection"].as_str() {
            Some(connection) => (connection.to_owned(), strings(&params["path"])),
            None => {
                let state = registry.dispatch(session, "db.navigator.state", json!({ "limit": 1 })).unwrap_or_default();
                let selected = &state["selected"];
                match selected["connection"].as_str() {
                    Some(connection) => (connection.to_owned(), strings(&selected["path"])),
                    None => return Err(t!("ide-dbb-no-connection")),
                }
            }
        };
        let source = registry
            .dispatch(session, "db.dataSource.get", json!({ "id": connection }))
            .map_err(|error| message(error).1)?;
        let tools = registry
            .dispatch(session, "db.backup.tools", json!({ "connection": connection }))
            .map_err(|error| message(error).1)?;
        let settings = registry.dispatch(session, "db.backup.settings", json!({})).map_err(|error| message(error).1)?;
        let family = tools["family"].as_str().unwrap_or_default().to_owned();
        let postgres = family == "postgres";
        let database = params["database"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| path.first().filter(|_| postgres).cloned())
            .or_else(|| source["database"].as_str().map(str::to_owned))
            .unwrap_or_default();
        let mut pg_dump: PgDumpOptions = from(&settings["pgDump"]);
        let mut mysqldump: MysqldumpOptions = from(&settings["mysqldump"]);
        pg_dump.schemas.clear();
        pg_dump.tables.clear();
        mysqldump.tables.clear();
        if postgres && path.len() >= 2 {
            pg_dump.schemas = vec![path[1].clone()];
        }
        if path.len() == 4 {
            if postgres {
                pg_dump.schemas.clear();
                pg_dump.tables = vec![format!("{}.{}", path[1], path[3])];
            } else {
                mysqldump.tables = vec![path[3].clone()];
            }
        }
        let pg_restore: PgRestoreOptions = from(&settings["pgRestore"]);
        let mysql: MysqlRestoreOptions = from(&settings["mysql"]);
        let folder = settings["folder"].as_str().or(settings["defaultFolder"].as_str()).unwrap_or_default().to_owned();
        let (schemas, tables) = if postgres {
            (pg_dump.schemas.join(", "), pg_dump.tables.join(", "))
        } else {
            (String::new(), mysqldump.tables.join(", "))
        };
        let (compression, jobs, extra) = match (mode, postgres) {
            (Mode::Backup, true) => (
                pg_dump.compression.map(|level| level.to_string()).unwrap_or_default(),
                pg_dump.jobs.map(|jobs| jobs.to_string()).unwrap_or_default(),
                pg_dump.extra_args.join(" "),
            ),
            (Mode::Backup, false) => (String::new(), String::new(), mysqldump.extra_args.join(" ")),
            (Mode::Restore, true) => (
                String::new(),
                pg_restore.jobs.map(|jobs| jobs.to_string()).unwrap_or_default(),
                pg_restore.extra_args.join(" "),
            ),
            (Mode::Restore, false) => (String::new(), String::new(), mysql.extra_args.join(" ")),
        };
        let mut dialog = Self {
            mode,
            name: source["name"].as_str().unwrap_or(&connection).to_owned(),
            connection,
            family,
            database,
            tools,
            tool: None,
            pg_dump,
            mysqldump,
            pg_restore,
            mysql,
            schemas,
            tables,
            compression,
            jobs,
            extra,
            folder,
            file: params["file"].as_str().unwrap_or_default().to_owned(),
            password: None,
            exists: false,
            confirm: None,
            error: None,
            polled: 0.0,
            fresh: true,
            pinned: None,
        };
        if mode == Mode::Backup {
            dialog.file = dialog.default_name();
        }
        Ok(dialog)
    }

    pub(super) fn reopen(
        registry: &Registry,
        session: &mut Session,
        asked: &zen_workbench::database::backups::Asked,
    ) -> Result<Self, String> {
        let params = &asked.params;
        let mode = if asked.restore { Mode::Restore } else { Mode::Backup };
        let file = params["file"].as_str().unwrap_or_default();
        let opened = json!({ "connection": params["connection"], "database": params["database"], "file": file });
        let mut dialog = Self::open(registry, session, mode, &opened)?;
        if let Some(database) = params["database"].as_str() {
            database.clone_into(&mut dialog.database);
        }
        let path = std::path::Path::new(file);
        match mode {
            Mode::Backup => {
                dialog.folder = path.parent().map(|folder| folder.display().to_string()).unwrap_or_default();
                dialog.file = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                dialog.exists = params["overwrite"] == true;
            }
            Mode::Restore => file.clone_into(&mut dialog.file),
        }
        if params["pgDump"].is_object() {
            dialog.pg_dump = from(&params["pgDump"]);
            dialog.schemas = dialog.pg_dump.schemas.join(", ");
            dialog.tables = dialog.pg_dump.tables.join(", ");
            dialog.compression = dialog.pg_dump.compression.map(|level| level.to_string()).unwrap_or_default();
            dialog.jobs = dialog.pg_dump.jobs.map(|jobs| jobs.to_string()).unwrap_or_default();
            dialog.extra = dialog.pg_dump.extra_args.join(" ");
        }
        if params["mysqldump"].is_object() {
            dialog.mysqldump = from(&params["mysqldump"]);
            dialog.tables = dialog.mysqldump.tables.join(", ");
            dialog.extra = dialog.mysqldump.extra_args.join(" ");
        }
        if params["pgRestore"].is_object() {
            dialog.pg_restore = from(&params["pgRestore"]);
            dialog.jobs = dialog.pg_restore.jobs.map(|jobs| jobs.to_string()).unwrap_or_default();
            dialog.extra = dialog.pg_restore.extra_args.join(" ");
        }
        if params["mysql"].is_object() {
            dialog.mysql = from(&params["mysql"]);
            dialog.extra = dialog.mysql.extra_args.join(" ");
        }
        dialog.pinned = params["tool"].as_object().filter(|tool| !tool.is_empty()).map(|_| params["tool"].clone());
        dialog.pin();
        match (asked.code.as_str(), &asked.token) {
            ("confirmExecute", Some(token)) => dialog.confirm = Some((token.clone(), asked.message.clone())),
            _ => {
                dialog.password = Some(String::new());
                dialog.error = Some(asked.message.clone());
            }
        }
        Ok(dialog)
    }

    fn pin(&mut self) {
        let Some(pinned) = self.pinned.clone() else { return };
        if self.searching() {
            return;
        }
        let same = |tool: &Value| tool["origin"] == pinned["origin"] && tool["program"] == pinned["program"];
        self.tool = self.candidates().iter().position(same);
        self.pinned = None;
    }

    fn postgres(&self) -> bool {
        self.family == "postgres"
    }

    fn default_name(&self) -> String {
        let (_, stamp) = zen_db::engine::now_stamps();
        let family = if self.postgres() { zen_db::backup::Family::Postgres } else { zen_db::backup::Family::Mysql };
        zen_db::backup::file_name(&self.database, &stamp, family, self.pg_dump.format)
    }

    fn wanted_tool(&self) -> &'static str {
        match (self.mode, self.postgres()) {
            (Mode::Backup, true) => "pgDump",
            (Mode::Backup, false) => "mysqldump",
            (Mode::Restore, true) => "pgRestore",
            (Mode::Restore, false) => "mysql",
        }
    }

    fn candidates(&self) -> Vec<Value> {
        let wanted = self.wanted_tool();
        self.tools["tools"].as_array().into_iter().flatten().filter(|tool| tool["tool"] == wanted).cloned().collect()
    }

    fn searching(&self) -> bool {
        self.tools["status"] != "done"
    }

    fn poll(&mut self, registry: &Registry, session: &mut Session, now: f64) {
        if !self.searching() || now - self.polled < POLL {
            return;
        }
        self.polled = now;
        if let Ok(tools) = registry.dispatch(session, "db.backup.tools", json!({ "connection": self.connection })) {
            self.tools = tools;
        }
        self.pin();
    }

    fn tool_choice(&self) -> Value {
        match self.tool.and_then(|index| self.candidates().get(index).cloned()) {
            Some(tool) => json!({ "origin": tool["origin"], "program": tool["program"] }),
            None => self.pinned.clone().unwrap_or_else(|| json!({})),
        }
    }

    pub(super) fn start_call(&self) -> (&'static str, Value) {
        let extra = words(&self.extra);
        let mut params =
            json!({ "connection": self.connection, "database": self.database.trim(), "tool": self.tool_choice() });
        match self.mode {
            Mode::Backup => {
                let file = std::path::Path::new(self.folder.trim()).join(self.file.trim());
                params["file"] = json!(file);
                params["overwrite"] = json!(self.exists);
                if self.postgres() {
                    let mut options = self.pg_dump.clone();
                    options.schemas = list(&self.schemas);
                    options.tables = list(&self.tables);
                    options.compression = number(&self.compression).map(|level| level.min(9));
                    options.jobs = number(&self.jobs);
                    options.extra_args = extra;
                    params["pgDump"] = json!(options);
                } else {
                    let mut options = self.mysqldump.clone();
                    options.tables = list(&self.tables);
                    options.extra_args = extra;
                    params["mysqldump"] = json!(options);
                }
            }
            Mode::Restore => {
                params["file"] = json!(self.file.trim());
                if self.postgres() {
                    let mut options = self.pg_restore.clone();
                    options.jobs = number(&self.jobs);
                    options.extra_args = extra;
                    params["pgRestore"] = json!(options);
                } else {
                    let mut options = self.mysql.clone();
                    options.extra_args = extra;
                    params["mysql"] = json!(options);
                }
                if let Some((token, _)) = &self.confirm {
                    params["confirmed"] = json!(token);
                }
            }
        }
        if let Some(password) = self.password.as_ref().filter(|password| !password.is_empty()) {
            params["password"] = json!(password);
        }
        let command = if self.mode == Mode::Backup { "db.backup.start" } else { "db.backup.restore" };
        (command, params)
    }

    fn start(&mut self, registry: &Registry, session: &mut Session) -> bool {
        let (command, params) = self.start_call();
        match registry.dispatch(session, command, params) {
            Ok(_) => true,
            Err(error) => {
                let (code, text, data) = message(error);
                self.error = None;
                match code.as_deref() {
                    Some("exists") => self.exists = true,
                    Some("confirmExecute") => {
                        self.confirm = Some((data["token"].as_str().unwrap_or_default().to_owned(), text));
                    }
                    Some("passwordNeeded") => {
                        self.password.get_or_insert_with(String::new);
                        self.error = Some(text);
                    }
                    _ => {
                        self.confirm = None;
                        self.error = Some(text);
                    }
                }
                false
            }
        }
    }

    #[cfg(test)]
    pub(super) fn set_folder(&mut self, folder: &str) {
        folder.clone_into(&mut self.folder);
    }

    #[cfg(test)]
    pub(super) fn set_file(&mut self, file: &str) {
        file.clone_into(&mut self.file);
    }
}

enum Choice {
    Start,
    Cancel,
}

impl IdeView {
    pub(super) fn backup_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if let Some((mode, params)) = session.state_mut::<BackupDialogRequest>().0.take() {
            match BackupDialog::open(registry, session, mode, &params) {
                Ok(dialog) => self.backup_dialog = Some(dialog),
                Err(error) => self.notify(zen_workbench::notifications::NewNotification::message(
                    zen_workbench::notifications::Group::General,
                    zen_workbench::notifications::Kind::Error,
                    error,
                )),
            }
        }
        if let Some(asked) = zen_workbench::database::backups::take_asked(session) {
            match BackupDialog::reopen(registry, session, &asked) {
                Ok(dialog) => self.backup_dialog = Some(dialog),
                Err(error) => self.notify(zen_workbench::notifications::NewNotification::message(
                    zen_workbench::notifications::Group::General,
                    zen_workbench::notifications::Kind::Error,
                    error,
                )),
            }
        }
        let Some(dialog) = &mut self.backup_dialog else { return };
        let now = ui.input(|input| input.time);
        dialog.poll(registry, session, now);
        if dialog.searching() {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(POLL));
        }
        let p = self.palette;
        match show(ui, &p, dialog) {
            Some(Choice::Start) => {
                if dialog.start(registry, session) {
                    self.backup_dialog = None;
                }
            }
            Some(Choice::Cancel) => self.backup_dialog = None,
            None => {}
        }
    }
}

fn label(ui: &mut Ui, p: &IdePalette, text: &str) {
    ui.allocate_ui_with_layout(egui::vec2(LABEL, 28.0), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(LABEL);
        ui.label(RichText::new(text).font(theme::regular(13.0)).color(p.text));
    });
}

fn field(ui: &mut Ui, p: &IdePalette, name: &str, value: &mut String, width: f32) -> egui::Response {
    let edit = egui::TextEdit::singleline(value).desired_width(width);
    let response = super::connection_dialog::input(ui, p, edit);
    accessibility::name(&response, Role::TextInput, name);
    response
}

fn check(ui: &mut Ui, p: &IdePalette, value: &mut bool, name: &str) {
    let response = ui.checkbox(value, RichText::new(name).font(theme::regular(13.0)).color(p.text));
    accessibility::name(&response, Role::CheckBox, name);
}

fn radio<T: PartialEq + Copy>(ui: &mut Ui, p: &IdePalette, value: &mut T, choice: T, name: &str) -> bool {
    let response = ui.radio_value(value, choice, RichText::new(name).font(theme::regular(13.0)).color(p.text));
    accessibility::name(&response, Role::RadioButton, name);
    response.changed()
}

fn hint(ui: &mut Ui, p: &IdePalette, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
}

fn tool_text(tool: &Value) -> String {
    let version = tool["version"].as_str().or(tool["program"].as_str()).unwrap_or_default().to_owned();
    match tool["origin"].as_str() {
        Some("docker") => t!(
            "ide-dbb-tool-docker",
            version = version,
            container = tool["container"]["name"].as_str().unwrap_or_default()
        ),
        Some("homebrew") => t!("ide-dbb-tool-homebrew", version = version),
        Some("app") => t!("ide-dbb-tool-app", version = version),
        _ => t!("ide-dbb-tool-path", version = version),
    }
}

fn tool_row(ui: &mut Ui, p: &IdePalette, dialog: &mut BackupDialog) {
    let candidates = dialog.candidates();
    let chosen = dialog.tools["chosen"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|tool| tool["tool"] == dialog.wanted_tool())
        .cloned();
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-tool"));
        if dialog.searching() {
            ui.add(egui::Spinner::new().size(12.0).color(p.text_secondary));
            ui.label(RichText::new(t!("ide-dbb-searching")).font(theme::regular(13.0)).color(p.text_secondary));
            return;
        }
        let automatic = match &chosen {
            Some(tool) => t!("ide-dbb-automatic", tool = tool_text(tool)),
            None => t!("ide-dbb-none-found"),
        };
        let shown = dialog.tool.and_then(|index| candidates.get(index)).map_or_else(|| automatic.clone(), tool_text);
        let response = egui::ComboBox::from_id_salt("dbb-tool")
            .selected_text(RichText::new(shown).font(theme::regular(12.5)).color(p.text))
            .width(WIDTH - LABEL - 16.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut dialog.tool, None, automatic.clone());
                for (index, tool) in candidates.iter().enumerate() {
                    let text = format!("{} — {}", tool_text(tool), tool["program"].as_str().unwrap_or_default());
                    ui.selectable_value(&mut dialog.tool, Some(index), text);
                }
            })
            .response;
        accessibility::name(&response, Role::ComboBox, &t!("ide-dbb-tool"));
    });
    if !dialog.searching() && chosen.is_none() {
        ui.horizontal(|ui| {
            label(ui, p, "");
            let install = if dialog.postgres() { "brew install libpq" } else { "brew install mysql-client" };
            let text = t!("ide-dbb-install", install = install);
            ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.warning_text)).wrap());
        });
    }
    if dialog.mode == Mode::Restore && dialog.postgres() {
        ui.horizontal(|ui| {
            label(ui, p, "");
            hint(ui, p, &t!("ide-dbb-restore-plain-hint"));
        });
    }
}

fn content(ui: &mut Ui, p: &IdePalette, value: &mut Content) {
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-content"));
        radio(ui, p, value, Content::All, &t!("ide-dbb-content-all"));
        radio(ui, p, value, Content::SchemaOnly, &t!("ide-dbb-content-schema"));
        radio(ui, p, value, Content::DataOnly, &t!("ide-dbb-content-data"));
    });
}

fn pg_dump_options(ui: &mut Ui, p: &IdePalette, dialog: &mut BackupDialog) {
    let mut changed = false;
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-format"));
        let format = &mut dialog.pg_dump.format;
        changed |= radio(ui, p, format, PgFormat::Custom, &t!("ide-dbb-format-custom"));
        changed |= radio(ui, p, format, PgFormat::Plain, &t!("ide-dbb-format-plain"));
        changed |= radio(ui, p, format, PgFormat::Directory, &t!("ide-dbb-format-directory"));
        changed |= radio(ui, p, format, PgFormat::Tar, "tar");
    });
    if changed {
        dialog.file = dialog.default_name();
    }
    content(ui, p, &mut dialog.pg_dump.content);
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-schemas"));
        field(ui, p, &t!("ide-dbb-schemas"), &mut dialog.schemas, 170.0);
        ui.add_space(8.0);
        ui.label(RichText::new(t!("ide-dbb-tables")).font(theme::regular(13.0)).color(p.text));
        field(ui, p, &t!("ide-dbb-tables"), &mut dialog.tables, 170.0);
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-compression"));
        field(ui, p, &t!("ide-dbb-compression"), &mut dialog.compression, 30.0);
        ui.add_space(8.0);
        ui.label(RichText::new(t!("ide-dbb-encoding")).font(theme::regular(13.0)).color(p.text));
        let mut encoding = dialog.pg_dump.encoding.clone().unwrap_or_default();
        field(ui, p, &t!("ide-dbb-encoding"), &mut encoding, 80.0);
        dialog.pg_dump.encoding = Some(encoding).filter(|encoding| !encoding.trim().is_empty());
        if dialog.pg_dump.format == PgFormat::Directory {
            ui.add_space(8.0);
            ui.label(RichText::new(t!("ide-dbb-jobs")).font(theme::regular(13.0)).color(p.text));
            field(ui, p, &t!("ide-dbb-jobs"), &mut dialog.jobs, 30.0);
        }
    });
    let options = &mut dialog.pg_dump;
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut options.inserts, &t!("ide-dbb-inserts"));
        check(ui, p, &mut options.clean, &t!("ide-dbb-clean"));
        ui.add_enabled_ui(options.clean, |ui| check(ui, p, &mut options.if_exists, &t!("ide-dbb-if-exists")));
        check(ui, p, &mut options.create, &t!("ide-dbb-create"));
    });
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut options.no_owner, &t!("ide-dbb-no-owner"));
        check(ui, p, &mut options.no_privileges, &t!("ide-dbb-no-privileges"));
        check(ui, p, &mut options.verbose, &t!("ide-dbb-verbose"));
    });
}

fn mysqldump_options(ui: &mut Ui, p: &IdePalette, dialog: &mut BackupDialog) {
    content(ui, p, &mut dialog.mysqldump.content);
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-method"));
        let method = &mut dialog.mysqldump.method;
        radio(ui, p, method, DumpMethod::SingleTransaction, &t!("ide-dbb-method-transaction"));
        radio(ui, p, method, DumpMethod::LockTables, &t!("ide-dbb-method-lock"));
        radio(ui, p, method, DumpMethod::None, &t!("ide-dbb-method-none"));
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-tables"));
        field(ui, p, &t!("ide-dbb-tables"), &mut dialog.tables, 300.0);
    });
    let options = &mut dialog.mysqldump;
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut options.add_drop_table, &t!("ide-dbb-add-drop"));
        check(ui, p, &mut options.routines, &t!("ide-dbb-routines"));
        check(ui, p, &mut options.events, &t!("ide-dbb-events"));
        check(ui, p, &mut options.triggers, &t!("ide-dbb-triggers"));
    });
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut options.extended_insert, &t!("ide-dbb-extended-insert"));
        check(ui, p, &mut options.complete_insert, &t!("ide-dbb-complete-insert"));
        check(ui, p, &mut options.hex_blob, &t!("ide-dbb-hex-blob"));
    });
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut options.comments, &t!("ide-dbb-comments"));
        check(ui, p, &mut options.verbose, &t!("ide-dbb-verbose"));
    });
}

fn pg_restore_options(ui: &mut Ui, p: &IdePalette, dialog: &mut BackupDialog) {
    content(ui, p, &mut dialog.pg_restore.content);
    let options = &mut dialog.pg_restore;
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut options.clean, &t!("ide-dbb-clean"));
        ui.add_enabled_ui(options.clean, |ui| check(ui, p, &mut options.if_exists, &t!("ide-dbb-if-exists")));
        check(ui, p, &mut options.create, &t!("ide-dbb-create"));
        check(ui, p, &mut options.no_owner, &t!("ide-dbb-no-owner"));
    });
    ui.horizontal(|ui| {
        label(ui, p, "");
        check(ui, p, &mut options.no_privileges, &t!("ide-dbb-no-privileges"));
        check(ui, p, &mut options.single_transaction, &t!("ide-dbb-single-transaction"));
        check(ui, p, &mut options.exit_on_error, &t!("ide-dbb-exit-on-error"));
        check(ui, p, &mut options.verbose, &t!("ide-dbb-verbose"));
    });
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-jobs"));
        field(ui, p, &t!("ide-dbb-jobs"), &mut dialog.jobs, 30.0);
    });
}

fn options(ui: &mut Ui, p: &IdePalette, dialog: &mut BackupDialog) {
    match (dialog.mode, dialog.postgres()) {
        (Mode::Backup, true) => pg_dump_options(ui, p, dialog),
        (Mode::Backup, false) => mysqldump_options(ui, p, dialog),
        (Mode::Restore, true) => pg_restore_options(ui, p, dialog),
        (Mode::Restore, false) => {
            ui.horizontal(|ui| {
                label(ui, p, "");
                check(ui, p, &mut dialog.mysql.force, &t!("ide-dbb-force"));
            })
            .inner
        }
    }
    ui.horizontal(|ui| {
        label(ui, p, &t!("ide-dbb-extra"));
        field(ui, p, &t!("ide-dbb-extra"), &mut dialog.extra, WIDTH - LABEL - 16.0);
    });
}

fn output(ui: &mut Ui, p: &IdePalette, dialog: &mut BackupDialog) {
    let mut edited = false;
    match dialog.mode {
        Mode::Backup => {
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbb-folder"));
                edited |= field(ui, p, &t!("ide-dbb-folder"), &mut dialog.folder, WIDTH - LABEL - 16.0).changed();
            });
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbb-file-name"));
                edited |= field(ui, p, &t!("ide-dbb-file-name"), &mut dialog.file, WIDTH - LABEL - 16.0).changed();
            });
        }
        Mode::Restore => {
            ui.horizontal(|ui| {
                label(ui, p, &t!("ide-dbb-backup-file"));
                edited |= field(ui, p, &t!("ide-dbb-backup-file"), &mut dialog.file, WIDTH - LABEL - 16.0).changed();
            });
        }
    }
    if edited {
        dialog.exists = false;
        dialog.confirm = None;
    }
    if let Some(password) = &mut dialog.password {
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbb-password"));
            let edit = egui::TextEdit::singleline(password).password(true).desired_width(200.0);
            let response = super::connection_dialog::input(ui, p, edit);
            accessibility::name(&response, Role::PasswordInput, &t!("ide-dbb-password"));
        });
        ui.horizontal(|ui| {
            label(ui, p, "");
            hint(ui, p, &t!("ide-dbb-password-hint"));
        });
    }
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut BackupDialog) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("backup-dialog")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 6.0;
        let title = if dialog.mode == Mode::Backup { t!("ide-dbb-title-backup") } else { t!("ide-dbb-title-restore") };
        ui.label(RichText::new(title).font(theme::semibold(15.0)).color(p.text));
        ui.label(RichText::new(dialog.name.clone()).font(theme::regular(12.5)).color(p.text_secondary));
        ui.add_space(6.0);
        tool_row(ui, p, dialog);
        ui.horizontal(|ui| {
            label(ui, p, &t!("ide-dbb-database"));
            if field(ui, p, &t!("ide-dbb-database"), &mut dialog.database, 220.0).changed() {
                dialog.confirm = None;
                if dialog.mode == Mode::Backup {
                    dialog.file = dialog.default_name();
                }
            }
        });
        ui.separator();
        options(ui, p, dialog);
        ui.separator();
        output(ui, p, dialog);
        if dialog.exists {
            let path = std::path::Path::new(dialog.folder.trim()).join(dialog.file.trim());
            let text = t!("ide-dbb-exists", path = path.display().to_string());
            ui.add(egui::Label::new(RichText::new(text).font(theme::regular(12.0)).color(p.warning_text)).wrap());
        }
        if let Some((_, question)) = &dialog.confirm {
            ui.add(
                egui::Label::new(RichText::new(question.clone()).font(theme::regular(12.0)).color(p.warning_text))
                    .wrap(),
            );
        }
        if let Some(error) = &dialog.error {
            ui.add(
                egui::Label::new(RichText::new(error.clone()).font(theme::regular(12.0)).color(p.error_text)).wrap(),
            );
        }
        hint(ui, p, &t!("ide-dbb-run-hint"));
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let text = match dialog.mode {
                Mode::Backup if dialog.exists => t!("ide-dbb-replace"),
                Mode::Backup => t!("ide-dbb-start-backup"),
                Mode::Restore if dialog.confirm.is_some() => t!("ide-dbb-restore-confirmed"),
                Mode::Restore => t!("ide-dbb-start-restore"),
            };
            let ready = !dialog.file.trim().is_empty() && !dialog.database.trim().is_empty();
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
    choice.or_else(|| (modal.should_close() && !fresh).then_some(Choice::Cancel))
}

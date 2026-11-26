use std::collections::BTreeSet;

use egui::{Align, Key, Layout, RichText, Role, Ui};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::{IdePalette, IdeView, dialog_frame, vcs};
use crate::{accessibility, theme};

pub(super) const OPEN: &str = "view.dataSource.import";

const WIDTH: f32 = 640.0;
const LIST_HEIGHT: f32 = 260.0;
const LISTED: usize = 200;

#[derive(Default)]
pub(super) struct ImportDialogRequest(pub(super) Option<Value>);

pub(super) fn request(session: &mut Session, from: Option<&str>) {
    session.state_mut::<ImportDialogRequest>().0 = Some(json!({ "from": from }));
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    Dbeaver,
    Jetbrains,
}

impl Source {
    fn command(self) -> &'static str {
        match self {
            Source::Dbeaver => "db.import.dbeaver",
            Source::Jetbrains => "db.import.jetbrains",
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ImportDialog {
    source: Source,
    path: String,
    entries: Vec<Value>,
    chosen: BTreeSet<String>,
    project: bool,
    has_project: bool,
    listed: bool,
    error: Option<String>,
    fresh: bool,
}

fn message(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

impl ImportDialog {
    pub(super) fn open(session: &Session, params: &Value) -> Self {
        let source = if params["from"] == "jetbrains" { Source::Jetbrains } else { Source::Dbeaver };
        let has_project =
            session.state::<zen_workbench::Workbench>().and_then(zen_workbench::Workbench::project).is_some();
        let mut dialog = Self {
            source,
            path: String::new(),
            entries: Vec::new(),
            chosen: BTreeSet::new(),
            project: false,
            has_project,
            listed: false,
            error: None,
            fresh: true,
        };
        dialog.path = params["path"].as_str().map(str::to_owned).unwrap_or_else(|| dialog.usual_path(session));
        dialog
    }

    fn usual_path(&self, session: &Session) -> String {
        let path = match self.source {
            Source::Dbeaver => zen_workbench::database::imports::dbeaver_workspace(),
            Source::Jetbrains => {
                session.state::<zen_workbench::Workbench>().and_then(zen_workbench::Workbench::project).map(Into::into)
            }
        };
        path.map(|path| path.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn params(&self) -> Value {
        let mut params = json!({ "scope": if self.project { "project" } else { "global" }, "limit": LISTED });
        if !self.path.trim().is_empty() {
            params["path"] = json!(self.path.trim());
        }
        params
    }

    pub(super) fn list(&mut self, registry: &Registry, session: &mut Session) {
        let mut params = self.params();
        params["preview"] = json!(true);
        self.listed = true;
        match registry.dispatch(session, self.source.command(), params) {
            Ok(answer) => {
                self.entries = answer["entries"].as_array().cloned().unwrap_or_default();
                self.chosen = self
                    .entries
                    .iter()
                    .filter(|entry| entry["status"] == "ready")
                    .filter_map(|entry| entry["key"].as_str().map(str::to_owned))
                    .collect();
                self.error = None;
            }
            Err(error) => {
                self.entries.clear();
                self.chosen.clear();
                self.error = Some(message(error));
            }
        }
    }

    pub(super) fn import(&mut self, registry: &Registry, session: &mut Session) -> Option<u64> {
        let mut params = self.params();
        params["keys"] = json!(self.chosen.iter().collect::<Vec<_>>());
        match registry.dispatch(session, self.source.command(), params) {
            Ok(answer) => Some(answer["added"].as_u64().unwrap_or(0)),
            Err(error) => {
                self.error = Some(message(error));
                None
            }
        }
    }
}

enum Choice {
    List,
    Import,
    Cancel,
}

impl IdeView {
    pub(super) fn import_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if let Some(params) = session.state_mut::<ImportDialogRequest>().0.take() {
            let mut dialog = ImportDialog::open(session, &params);
            if !dialog.path.is_empty() {
                dialog.list(registry, session);
            }
            self.import_dialog = Some(dialog);
        }
        let Some(dialog) = &mut self.import_dialog else { return };
        let p = self.palette;
        match show(ui, &p, dialog, session) {
            Some(Choice::List) => dialog.list(registry, session),
            Some(Choice::Import) => {
                if let Some(added) = dialog.import(registry, session) {
                    self.import_dialog = None;
                    self.notify(zen_workbench::notifications::NewNotification::worded(
                        zen_workbench::notifications::Group::General,
                        zen_workbench::notifications::Kind::Info,
                        zen_i18n::Message::new("ide-dbi-imported").with_number("count", added as f64),
                    ));
                }
            }
            Some(Choice::Cancel) => self.import_dialog = None,
            None => {}
        }
    }
}

fn status(p: &IdePalette, entry: &Value) -> (String, egui::Color32) {
    match entry["status"].as_str().unwrap_or_default() {
        "exists" => (t!("ide-dbi-exists"), p.text_secondary),
        "unsupported" => (entry["reason"].as_str().unwrap_or_default().to_owned(), p.text_secondary),
        "added" => (t!("ide-dbi-added"), p.vcs_added),
        _ => (String::new(), p.text_secondary),
    }
}

fn entries(ui: &mut Ui, p: &IdePalette, dialog: &mut ImportDialog) {
    egui::ScrollArea::vertical().id_salt("dbi-entries").max_height(LIST_HEIGHT).min_scrolled_height(LIST_HEIGHT).show(
        ui,
        |ui| {
            ui.set_min_height(LIST_HEIGHT);
            ui.set_width(WIDTH);
            if dialog.entries.is_empty() {
                let text =
                    if dialog.listed && dialog.error.is_none() { t!("ide-dbi-empty") } else { t!("ide-dbi-hint") };
                ui.label(RichText::new(text).font(theme::regular(12.0)).color(p.text_secondary));
            }
            for entry in &dialog.entries {
                let key = entry["key"].as_str().unwrap_or_default().to_owned();
                let ready = entry["status"] == "ready" || entry["status"] == "notChosen";
                let name = entry["name"].as_str().unwrap_or_default();
                let place = [entry["host"].as_str(), entry["database"].as_str()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" / ");
                let detail = format!("{} {place}", entry["product"].as_str().unwrap_or_default());
                ui.horizontal(|ui| {
                    let mut on = dialog.chosen.contains(&key);
                    let check = ui.add_enabled(ready, egui::Checkbox::new(&mut on, RichText::new(name).color(p.text)));
                    accessibility::name(&check, Role::CheckBox, name);
                    if check.changed() {
                        if on {
                            dialog.chosen.insert(key.clone());
                        } else {
                            dialog.chosen.remove(&key);
                        }
                    }
                    ui.label(RichText::new(detail).font(theme::regular(12.0)).color(p.text_secondary));
                    let (text, color) = status(p, entry);
                    if !text.is_empty() {
                        ui.label(RichText::new(text).font(theme::regular(12.0)).color(color));
                    }
                });
            }
        },
    );
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut ImportDialog, session: &Session) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("import-dialog")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(WIDTH);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(t!("ide-dbi-title")).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let before = dialog.source;
            let dbeaver_label = t!("ide-dbi-import-dbeaver");
            let dbeaver =
                ui.radio_value(&mut dialog.source, Source::Dbeaver, RichText::new(&dbeaver_label).color(p.text));
            accessibility::name(&dbeaver, Role::RadioButton, &dbeaver_label);
            let jetbrains = t!("ide-dbi-jetbrains");
            let idea = ui.radio_value(&mut dialog.source, Source::Jetbrains, RichText::new(&jetbrains).color(p.text));
            accessibility::name(&idea, Role::RadioButton, &jetbrains);
            if dialog.source != before {
                dialog.path = dialog.usual_path(session);
                dialog.entries.clear();
                dialog.chosen.clear();
                dialog.listed = false;
                dialog.error = None;
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(t!("ide-dbi-path")).font(theme::regular(13.0)).color(p.text));
            let edit = egui::TextEdit::singleline(&mut dialog.path).desired_width(WIDTH - 200.0);
            let field = super::connection_dialog::input(ui, p, edit);
            accessibility::name(&field, Role::TextInput, &t!("ide-dbi-path"));
            if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                choice = Some(Choice::List);
            }
            if vcs::secondary_button(ui, p, &t!("ide-dbi-list")).clicked() {
                choice = Some(Choice::List);
            }
        });
        let hint = match dialog.source {
            Source::Dbeaver => t!("ide-dbi-dbeaver-hint"),
            Source::Jetbrains => t!("ide-dbi-jetbrains-hint"),
        };
        ui.add(egui::Label::new(RichText::new(hint).font(theme::regular(12.0)).color(p.text_secondary)).wrap());
        ui.add_space(4.0);
        entries(ui, p, dialog);
        if dialog.has_project {
            ui.checkbox(&mut dialog.project, RichText::new(t!("ide-dbi-project")).color(p.text));
        }
        if let Some(error) = &dialog.error {
            ui.add(
                egui::Label::new(RichText::new(error.clone()).font(theme::regular(12.0)).color(p.error_text)).wrap(),
            );
        }
        ui.add_space(10.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let enabled = !dialog.chosen.is_empty();
            let import = ui.add_enabled_ui(enabled, |ui| vcs::primary_button(ui, p, &t!("ide-dbi-import"))).inner;
            if import.clicked() {
                choice = Some(Choice::Import);
            }
            if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                choice = Some(Choice::Cancel);
            }
        });
    });
    let fresh = std::mem::replace(&mut dialog.fresh, false);
    choice.or_else(|| (modal.should_close() && !fresh).then_some(Choice::Cancel))
}

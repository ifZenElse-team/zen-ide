use std::time::{Duration, Instant};

use egui::{Align, Key, Layout, RichText, Ui};
use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_i18n::t;
use zen_workbench::database::connections::IdleTransactions;

use super::{IdePalette, IdeView, dialog_frame, vcs};
use crate::theme;

pub(super) const PENDING: &str = "view.database.pendingTransactions";

const LOG_SHOWN: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Transactions,
    Edits,
    Structure,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Question {
    kind: Kind,
    message: String,
    pending: Vec<Value>,
    results: Vec<u64>,
    command: String,
    params: Value,
    error: Option<String>,
    preview: Option<(String, u64)>,
    saving: bool,
    tab: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Dialog {
    Question(Question),
    Log(Value),
    Pending,
    Execute(super::execute_dialog::ExecuteQuestion),
    Parameters(super::parameters_dialog::ParametersQuestion),
    Several(super::several_dialog::SeveralQuestion),
}

impl Dialog {
    pub(super) fn name(&self) -> &'static str {
        match self {
            Dialog::Question(_) => "transactionQuestion",
            Dialog::Log(_) => "transactionLog",
            Dialog::Pending => "pendingTransactions",
            Dialog::Execute(_) => "executeConfirmation",
            Dialog::Parameters(_) => "bindParameters",
            Dialog::Several(_) => "severalConnections",
        }
    }
}

#[derive(Default)]
pub(super) struct TransactionDialogRequest(pub(super) Option<Dialog>);

#[derive(Default)]
pub(super) struct TransactionDialogs {
    pub(super) open: Option<Dialog>,
    error: Option<String>,
    opened: u64,
}

pub(super) fn question(error: &DispatchError) -> Option<Question> {
    let DispatchError::Failed { error, .. } = error else { return None };
    let kind = match error.code.as_deref() {
        Some(zen_workbench::database::transactions::OPEN_TRANSACTIONS) => Kind::Transactions,
        Some(zen_workbench::database::edits::PENDING_CHANGES) => Kind::Edits,
        Some(zen_workbench::database::alter::PENDING_STRUCTURE) => Kind::Structure,
        _ => return None,
    };
    let data = error.data.as_ref()?;
    let retry = data.get("retry")?;
    Some(Question {
        kind,
        message: error.message.clone(),
        pending: data.get("pending").and_then(Value::as_array).cloned().unwrap_or_default(),
        results: data
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_u64)
            .collect(),
        command: retry.get("command")?.as_str()?.to_owned(),
        params: retry.get("params").cloned().unwrap_or_else(|| json!({})),
        error: None,
        preview: None,
        saving: false,
        tab: data.get("tab").and_then(Value::as_str).map(str::to_owned),
    })
}

pub(super) fn dialog(error: &DispatchError) -> Option<Dialog> {
    question(error)
        .map(Dialog::Question)
        .or_else(|| super::execute_dialog::question(error).map(Dialog::Execute))
        .or_else(|| super::parameters_dialog::question(error).map(Dialog::Parameters))
}

pub(super) fn ask(session: &mut Session, error: &DispatchError) -> bool {
    match dialog(error) {
        Some(dialog) => {
            session.state_mut::<TransactionDialogRequest>().0 = Some(dialog);
            true
        }
        None => false,
    }
}

pub(super) fn request(session: &mut Session, dialog: Dialog) {
    session.state_mut::<TransactionDialogRequest>().0 = Some(dialog);
}

fn message(error: DispatchError) -> String {
    match error {
        DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

pub(super) fn context_label(context: &Value) -> String {
    match context["kind"].as_str() {
        Some("console") => {
            context["name"].as_str().unwrap_or_default().rsplit('/').next().unwrap_or_default().to_owned()
        }
        Some("metadata") => t!("ide-dbtx-context-metadata"),
        _ => t!("ide-dbtx-context-main"),
    }
}

pub(super) fn age(seconds: u64) -> String {
    match seconds {
        0..60 => t!("ide-dbtx-seconds", count = seconds),
        60..3600 => t!("ide-dbtx-minutes", count = seconds / 60),
        _ => t!("ide-dbtx-hours", count = seconds / 3600),
    }
}

pub(super) fn pending_line(state: &Value) -> String {
    t!(
        "ide-dbtx-pending-line",
        name = state["name"].as_str().unwrap_or_default(),
        context = context_label(&state["context"]),
        statements = state["statements"].as_u64().unwrap_or(0),
        modifying = state["modifying"].as_u64().unwrap_or(0),
        age = age(state["ageSeconds"].as_u64().unwrap_or(0))
    )
}

fn title(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::semibold(15.0)).color(p.text)).wrap());
}

fn line(ui: &mut Ui, text: String, color: egui::Color32) {
    ui.add(egui::Label::new(RichText::new(text).font(theme::regular(13.0)).color(color)).wrap());
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Answer {
    Commit,
    Rollback,
    Save,
    Discard,
    Close,
}

fn question_dialog(ui: &mut Ui, p: &IdePalette, question: &Question, fresh: bool) -> Option<Answer> {
    let mut answer = None;
    let modal =
        egui::Modal::new(ui.scope_id().with("transaction-question")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let heading = match question.kind {
                Kind::Edits => t!("ide-dbedit-question-title"),
                Kind::Structure => t!("ide-dbalter-question-title"),
                Kind::Transactions => t!("ide-dbtx-question-title"),
            };
            title(ui, p, heading);
            ui.add_space(12.0);
            let text = match question.kind {
                Kind::Edits if question.command == "workspace.tab.close" => t!("ide-dbedit-question-close"),
                Kind::Edits => t!("ide-dbedit-question-go-on", count = question.results.len().max(1)),
                _ => question.message.clone(),
            };
            line(ui, text, p.text);
            if let Some((sql, count)) = &question.preview {
                ui.add_space(10.0);
                line(ui, t!("ide-database-preview-confirm", count = *count), p.warning_text);
                ui.add_space(6.0);
                egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().max_height(200.0).auto_shrink([false, true]).show(ui, |ui| {
                        ui.label(RichText::new(sql).font(egui::FontId::monospace(12.0)).color(p.text));
                    });
                });
            }
            if question.saving {
                ui.add_space(10.0);
                line(ui, t!("ide-database-edit-saving"), p.text_secondary);
            }
            if !question.pending.is_empty() {
                ui.add_space(10.0);
                for state in question.pending.iter().take(8) {
                    line(ui, format!("• {}", pending_line(state)), p.text_secondary);
                    ui.add_space(3.0);
                }
            }
            if let Some(error) = &question.error {
                ui.add_space(8.0);
                line(ui, error.clone(), p.error_text);
            }
            ui.add_space(20.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                match question.kind {
                    Kind::Transactions => {
                        if vcs::primary_button(ui, p, &t!("ide-dbtx-commit")).clicked() {
                            answer = Some(Answer::Commit);
                        }
                        if vcs::secondary_button(ui, p, &t!("ide-dbtx-rollback")).clicked() {
                            answer = Some(Answer::Rollback);
                        }
                    }
                    Kind::Edits if !question.saving => {
                        if vcs::primary_button(ui, p, &t!("ide-database-edit-save")).clicked() {
                            answer = Some(Answer::Save);
                        }
                        if vcs::secondary_button(ui, p, &t!("ide-dbedit-discard")).clicked() {
                            answer = Some(Answer::Discard);
                        }
                    }
                    Kind::Edits => {}
                    Kind::Structure => {
                        if vcs::primary_button(ui, p, &t!("ide-dbedit-discard")).clicked() {
                            answer = Some(Answer::Discard);
                        }
                    }
                }
                if vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                    answer = Some(Answer::Close);
                }
            });
        });
    answer.or_else(|| (!fresh && modal.should_close()).then_some(Answer::Close))
}

fn log_row(ui: &mut Ui, p: &IdePalette, statement: &Value) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let modifying = statement["modifying"] == true;
        let failed = statement["failed"] == true;
        let (mark, color) = match (failed, modifying) {
            (true, _) => ("✕", p.error_text),
            (false, true) => ("●", p.warning_text),
            (false, false) => ("○", p.text_secondary),
        };
        ui.label(RichText::new(mark).font(theme::regular(11.0)).color(color));
        ui.label(
            RichText::new(statement["summary"].as_str().unwrap_or_default())
                .font(egui::FontId::monospace(12.5))
                .color(p.text),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(age(statement["secondsAgo"].as_u64().unwrap_or(0)))
                    .font(theme::regular(12.0))
                    .color(p.text_secondary),
            );
            if let Some(rows) = statement["affected"].as_u64() {
                ui.label(
                    RichText::new(t!("ide-dbtx-rows", count = rows)).font(theme::regular(12.0)).color(p.text_secondary),
                );
            }
        });
    });
}

pub(super) fn mode_label(mode: &str) -> String {
    match mode {
        "manual" => t!("ide-dbtx-mode-manual"),
        "smart" => t!("ide-dbtx-mode-smart"),
        _ => t!("ide-dbtx-mode-auto"),
    }
}

fn log_dialog(ui: &mut Ui, p: &IdePalette, state: &Value, error: Option<&str>, fresh: bool) -> Option<Answer> {
    let mut answer = None;
    let modal = egui::Modal::new(ui.scope_id().with("transaction-log")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(620.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        let heading = t!(
            "ide-dbtx-log-title",
            name = state["name"].as_str().unwrap_or_default(),
            context = context_label(&state["context"])
        );
        title(ui, p, heading);
        ui.add_space(8.0);
        let isolation = state["isolation"].as_str().map_or_else(|| t!("ide-dbtx-isolation-default"), str::to_owned);
        let summary = if state["inTransaction"] == true {
            t!(
                "ide-dbtx-log-summary",
                mode = mode_label(state["mode"].as_str().unwrap_or_default()),
                isolation = isolation.as_str(),
                statements = state["statements"].as_u64().unwrap_or(0),
                modifying = state["modifying"].as_u64().unwrap_or(0),
                age = age(state["ageSeconds"].as_u64().unwrap_or(0))
            )
        } else {
            t!(
                "ide-dbtx-log-none",
                mode = mode_label(state["mode"].as_str().unwrap_or_default()),
                isolation = isolation.as_str()
            )
        };
        line(ui, summary, p.text_secondary);
        ui.add_space(10.0);
        let log = state["log"].as_array().cloned().unwrap_or_default();
        egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical().max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                if log.is_empty() {
                    line(ui, t!("ide-dbtx-log-empty"), p.text_secondary);
                }
                if state["truncated"] == true {
                    line(ui, t!("ide-dbtx-log-older"), p.text_secondary);
                }
                for statement in &log {
                    log_row(ui, p, statement);
                    ui.add_space(2.0);
                }
            });
        });
        if let Some(error) = error {
            ui.add_space(8.0);
            line(ui, error.to_owned(), p.error_text);
        }
        ui.add_space(16.0);
        let open = state["inTransaction"] == true;
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if vcs::secondary_button(ui, p, &t!("ide-dbtx-close")).clicked() {
                answer = Some(Answer::Close);
            }
            if open {
                if vcs::secondary_button(ui, p, &t!("ide-dbtx-rollback")).clicked() {
                    answer = Some(Answer::Rollback);
                }
                if vcs::primary_button(ui, p, &t!("ide-dbtx-commit")).clicked() {
                    answer = Some(Answer::Commit);
                }
            }
        });
    });
    answer.or_else(|| (!fresh && modal.should_close()).then_some(Answer::Close))
}

fn pending_dialog(
    ui: &mut Ui,
    p: &IdePalette,
    pending: &Value,
    (error, fresh): (Option<&str>, bool),
) -> (bool, Option<(Value, bool)>) {
    let mut chosen = None;
    let mut close = false;
    let modal =
        egui::Modal::new(ui.scope_id().with("pending-transactions")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
            ui.set_width(640.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            title(ui, p, t!("ide-dbtx-pending-title"));
            ui.add_space(12.0);
            let transactions = pending["transactions"].as_array().cloned().unwrap_or_default();
            if transactions.is_empty() {
                line(ui, t!("ide-dbtx-pending-none"), p.text_secondary);
            }
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                for state in &transactions {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let color = if state["modifying"].as_u64().unwrap_or(0) > 0 { p.warning_text } else { p.text };
                        ui.label(RichText::new(pending_line(state)).font(theme::regular(13.0)).color(color));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let target = json!({ "connection": state["connection"], "context": state["context"] });
                            if ui.button(t!("ide-dbtx-rollback")).clicked() {
                                chosen = Some((target.clone(), false));
                            }
                            if ui.button(t!("ide-dbtx-commit")).clicked() {
                                chosen = Some((target, true));
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
            });
            if pending["truncated"] == true {
                line(ui, t!("ide-dbtx-pending-more", total = pending["total"].as_u64().unwrap_or(0)), p.text_secondary);
            }
            if let Some(error) = error {
                ui.add_space(8.0);
                line(ui, error.to_owned(), p.error_text);
            }
            ui.add_space(16.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if vcs::secondary_button(ui, p, &t!("ide-dbtx-close")).clicked() {
                    close = true;
                }
            });
        });
    (close || (!fresh && modal.should_close()), chosen)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Idle {
    Keep,
    RollBack,
}

fn idle_dialog(ui: &mut Ui, p: &IdePalette, name: &str, context: &str, left: u64) -> Option<Idle> {
    let mut answer = None;
    egui::Modal::new(ui.scope_id().with("idle-transaction")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        title(ui, p, t!("ide-dbtx-idle-title"));
        ui.add_space(12.0);
        line(ui, t!("ide-dbtx-idle-text", name = name, context = context, seconds = left), p.text);
        ui.add_space(20.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if vcs::primary_button(ui, p, &t!("ide-dbtx-keep-open")).clicked() {
                answer = Some(Idle::Keep);
            }
            if vcs::secondary_button(ui, p, &t!("ide-dbtx-rollback-now")).clicked() {
                answer = Some(Idle::RollBack);
            }
        });
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            answer = Some(Idle::Keep);
        }
    });
    answer
}

fn retry(registry: &Registry, session: &mut Session, mut question: Question) -> Option<Dialog> {
    match registry.dispatch(session, &question.command, question.params.clone()) {
        Ok(_) => None,
        Err(error) => match self::question(&error) {
            Some(next) if next.kind != Kind::Edits => Some(Dialog::Question(next)),
            _ => {
                question.saving = false;
                question.error = Some(message(error));
                Some(Dialog::Question(question))
            }
        },
    }
}

fn save_all(registry: &Registry, session: &mut Session, question: &mut Question) -> Result<(), String> {
    let confirmed = question.preview.is_some();
    for id in &question.results {
        let params = json!({ "id": id, "confirmed": confirmed });
        match registry.dispatch(session, "db.result.save", params) {
            Ok(_) => {}
            Err(DispatchError::Failed { error, .. }) if error.code.as_deref() == Some("confirmDataChange") => {
                let data = error.data.unwrap_or_default();
                let sql = data["preview"].as_str().unwrap_or_default().to_owned();
                question.preview = Some((sql, data["statements"].as_u64().unwrap_or(0)));
                return Ok(());
            }
            Err(error) => return Err(message(error)),
        }
    }
    question.saving = true;
    Ok(())
}

fn edits_answer(
    registry: &Registry,
    session: &mut Session,
    mut question: Question,
    answer: Option<Answer>,
) -> Option<Dialog> {
    match answer {
        Some(Answer::Close) => return None,
        Some(Answer::Discard) => {
            for id in &question.results {
                if let Err(error) = registry.dispatch(session, "db.result.revert", json!({ "id": id })) {
                    question.error = Some(message(error));
                    return Some(Dialog::Question(question));
                }
            }
            return retry(registry, session, question);
        }
        Some(Answer::Save) => {
            question.error = None;
            if let Err(error) = save_all(registry, session, &mut question) {
                question.error = Some(error);
            }
        }
        _ => {}
    }
    if !question.saving {
        return Some(Dialog::Question(question));
    }
    let mut failed = None;
    for id in &question.results {
        let Ok(state) = registry.dispatch(session, "db.result.editState", json!({ "id": id })) else { continue };
        if state["saving"].is_u64() {
            return Some(Dialog::Question(question));
        }
        if state["dirty"] == true {
            failed = Some(state["lastSave"]["message"].as_str().unwrap_or_default().to_owned());
        }
    }
    match failed {
        Some(error) => {
            question.saving = false;
            question.error = Some(error);
            Some(Dialog::Question(question))
        }
        None => retry(registry, session, question),
    }
}

fn with_policy(params: &Value, policy: &str) -> Value {
    let mut params = params.clone();
    if let Some(object) = params.as_object_mut() {
        object.insert("transactions".to_owned(), json!(policy));
    }
    params
}

impl IdeView {
    pub(super) fn open_transaction_dialog(&mut self, ctx: &egui::Context, dialog: Dialog) {
        if let (Some(Dialog::Execute(open)), Dialog::Execute(asked)) = (&self.transaction_dialogs.open, &dialog)
            && open.same(asked)
        {
            return;
        }
        self.transaction_dialogs.error = None;
        self.transaction_dialogs.open = Some(dialog);
        self.transaction_dialogs.opened = ctx.cumulative_pass_nr();
    }

    pub(super) fn transaction_dialogs(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        if let Some(dialog) = session.state_mut::<TransactionDialogRequest>().0.take() {
            self.open_transaction_dialog(ui.ctx(), dialog);
        }
        let p = self.palette;
        let fresh = self.transaction_dialogs.opened == ui.ctx().cumulative_pass_nr();
        match self.transaction_dialogs.open.clone() {
            Some(Dialog::Question(question)) if question.kind == Kind::Edits => {
                let answer = question_dialog(ui, &p, &question, fresh);
                self.transaction_dialogs.open = edits_answer(registry, session, question, answer);
                if matches!(&self.transaction_dialogs.open, Some(Dialog::Question(question)) if question.saving) {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
            Some(Dialog::Question(question)) if question.kind == Kind::Structure => {
                match question_dialog(ui, &p, &question, fresh) {
                    Some(Answer::Discard) => {
                        let tab = question.tab.clone();
                        let discarded = registry.dispatch(session, "db.structure.discard", json!({ "tab": tab }));
                        self.transaction_dialogs.open = match discarded {
                            Ok(_) => retry(registry, session, question),
                            Err(error) => {
                                let mut question = question;
                                question.error = Some(message(error));
                                Some(Dialog::Question(question))
                            }
                        };
                    }
                    Some(_) => self.transaction_dialogs.open = None,
                    None => {}
                }
            }
            Some(Dialog::Question(question)) => match question_dialog(ui, &p, &question, fresh) {
                Some(Answer::Close) => self.transaction_dialogs.open = None,
                Some(answer) => {
                    let policy = if answer == Answer::Commit { "commit" } else { "rollback" };
                    match registry.dispatch(session, &question.command, with_policy(&question.params, policy)) {
                        Ok(_) => self.transaction_dialogs.open = None,
                        Err(error) => {
                            let mut question = question;
                            question.error = Some(message(error));
                            self.transaction_dialogs.open = Some(Dialog::Question(question));
                        }
                    }
                }
                None => {}
            },
            Some(Dialog::Log(target)) => {
                let mut params = target.clone();
                if let Some(object) = params.as_object_mut() {
                    object.insert("limit".to_owned(), json!(LOG_SHOWN));
                }
                match registry.dispatch(session, "db.transaction.status", params) {
                    Ok(state) => {
                        ui.ctx().request_repaint_after(Duration::from_secs(1));
                        let error = self.transaction_dialogs.error.clone();
                        match log_dialog(ui, &p, &state, error.as_deref(), fresh) {
                            Some(Answer::Close) => self.transaction_dialogs.open = None,
                            Some(answer) => {
                                let command = if answer == Answer::Commit {
                                    "db.transaction.commit"
                                } else {
                                    "db.transaction.rollback"
                                };
                                self.transaction_dialogs.error =
                                    registry.dispatch(session, command, target).err().map(message);
                            }
                            None => {}
                        }
                    }
                    Err(_) => self.transaction_dialogs.open = None,
                }
            }
            Some(Dialog::Execute(question)) => {
                self.transaction_dialogs.open =
                    super::execute_dialog::show(ui, &p, (registry, session), question, fresh).map(Dialog::Execute);
            }
            Some(Dialog::Parameters(question)) => {
                let next = super::parameters_dialog::show(ui, &p, (registry, session), question, fresh);
                if next.as_ref().is_some_and(|next| !matches!(next, Dialog::Parameters(_))) {
                    self.transaction_dialogs.opened = ui.ctx().cumulative_pass_nr();
                }
                self.transaction_dialogs.open = next;
            }
            Some(Dialog::Several(question)) => {
                self.transaction_dialogs.open =
                    super::several_dialog::show(ui, &p, (registry, session), question, fresh);
                if self.transaction_dialogs.open.as_ref().is_some_and(|open| !matches!(open, Dialog::Several(_))) {
                    self.transaction_dialogs.opened = ui.ctx().cumulative_pass_nr();
                }
            }
            Some(Dialog::Pending) => {
                let pending =
                    registry.dispatch(session, "db.transaction.pending", json!({ "limit": 100 })).unwrap_or_default();
                ui.ctx().request_repaint_after(Duration::from_secs(1));
                let error = self.transaction_dialogs.error.clone();
                let (close, chosen) = pending_dialog(ui, &p, &pending, (error.as_deref(), fresh));
                if let Some((target, commit)) = chosen {
                    let command = if commit { "db.transaction.commit" } else { "db.transaction.rollback" };
                    self.transaction_dialogs.error = registry.dispatch(session, command, target).err().map(message);
                }
                if close {
                    self.transaction_dialogs.open = None;
                }
            }
            None => {}
        }
        self.idle_countdown(ui, registry, session);
    }

    fn idle_countdown(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let now = Instant::now();
        let idle = {
            let idle = session.state_mut::<IdleTransactions>();
            idle.0.retain(|known| known.until + Duration::from_secs(3) > now);
            idle.0.first().cloned()
        };
        let Some(idle) = idle else { return };
        let left = idle.until.saturating_duration_since(now).as_secs();
        ui.ctx().request_repaint_after(Duration::from_millis(250));
        let context = serde_json::to_value(&idle.context).unwrap_or_default();
        let p = self.palette;
        let Some(answer) = idle_dialog(ui, &p, &idle.name, &context_label(&context), left) else { return };
        let target = json!({ "connection": idle.connection, "context": context });
        let command = if answer == Idle::Keep { "db.transaction.postpone" } else { "db.transaction.rollback" };
        if let Err(error) = registry.dispatch(session, command, target) {
            self.report_at(ui.ctx(), error);
        }
        session.state_mut::<IdleTransactions>().0.retain(|known| *known != idle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_with_pending_changes_becomes_a_question() {
        let error = zen_core::CommandError::with_code("openTransactions", "pending").with_data(json!({
            "pending": [{ "name": "Shop", "context": { "kind": "console", "name": "shop/console" }, "statements": 3, "modifying": 2, "ageSeconds": 75 }],
            "retry": { "command": "db.connection.disconnect", "params": { "connection": "shop" } },
        }));
        let refused =
            DispatchError::Failed { id: zen_core::CommandId::new("db.connection.disconnect").unwrap(), error };
        let asked = question(&refused).unwrap();
        assert_eq!(asked.command, "db.connection.disconnect");
        assert_eq!(with_policy(&asked.params, "commit"), json!({ "connection": "shop", "transactions": "commit" }));
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::set_language(Some("en"));
        assert_eq!(pending_line(&asked.pending[0]), "Shop · console: 3 statements, 2 changing data, open 1 min");
        let other = DispatchError::Failed {
            id: zen_core::CommandId::new("db.connection.disconnect").unwrap(),
            error: zen_core::CommandError::new("no"),
        };
        assert!(question(&other).is_none());
    }
}

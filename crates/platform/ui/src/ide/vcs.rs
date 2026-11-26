use egui::{Align2, Color32, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_vcs::FileStatus;
use zen_workbench::vcs::{FileStatuses, OperationInfo, Outgoing, RepositoryInfo, StatusReport};

use super::front::FrontKind;
use super::glyphs::{Glyph, glyph_branch, glyph_incoming, glyph_outgoing};
use super::{IdePalette, IdeView, dialog_frame};
use crate::theme;

const POLL: f64 = 3.0;

#[derive(Default)]
pub(super) struct VcsUi {
    pub(super) info: RepositoryInfo,
    pub(super) statuses: FileStatuses,
    revision: Option<u64>,
    pub(super) unsaved: u64,
    listed_unsaved: u64,
    last_poll: f64,
    woken: bool,
    pub(super) operation: Option<OperationInfo>,
    seen_operation: Option<u64>,
    pub(super) open_branches: bool,
    pub(super) popup: super::branches_popup::BranchesPopup,
    new_branch: Option<NewBranch>,
    checkout_revision: Option<RevisionDialog>,
    update: Option<bool>,
    rebase: bool,
    push: Option<PushDialog>,
}

struct NewBranch {
    name: String,
    start: Option<String>,
    checkout: bool,
    focused: bool,
    error: Option<String>,
}

#[derive(Default)]
struct RevisionDialog {
    revision: String,
    focused: bool,
    error: Option<String>,
}

struct PushDialog {
    outgoing: Result<Outgoing, String>,
}

pub(super) fn status_color(p: &IdePalette, status: FileStatus) -> Color32 {
    match status {
        FileStatus::Added => p.vcs_added,
        FileStatus::Modified | FileStatus::Renamed => p.vcs_modified,
        FileStatus::Deleted => p.vcs_deleted,
        FileStatus::Unversioned => p.vcs_unversioned,
        FileStatus::Ignored => p.vcs_ignored,
        FileStatus::Conflicted => p.vcs_conflict,
    }
}

impl IdeView {
    pub(super) fn vcs_frame(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session, now: f64) {
        if !std::mem::replace(&mut self.vcs.woken, true) {
            let ctx = ctx.clone();
            let viewport = ctx.viewport_id();
            session.state_mut::<zen_workbench::vcs::Vcs>().set_waker(std::sync::Arc::new(move || {
                ctx.request_repaint_of(viewport);
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }));
        }
        self.vcs.info = registry
            .dispatch(session, "vcs.repository.info", json!({}))
            .ok()
            .and_then(|info| serde_json::from_value(info).ok())
            .unwrap_or_default();
        if self.vcs.info.root.is_none() {
            self.vcs.statuses = FileStatuses::default();
            self.vcs.revision = None;
            return;
        }
        self.vcs.unsaved = unsaved_stamp(session);
        if self.vcs.revision != Some(self.vcs.info.revision) || self.vcs.unsaved != self.vcs.listed_unsaved {
            self.vcs.listed_unsaved = self.vcs.unsaved;
            let report: StatusReport = registry
                .dispatch(session, "vcs.status.list", json!({}))
                .ok()
                .and_then(|report| serde_json::from_value(report).ok())
                .unwrap_or_default();
            self.vcs.statuses = FileStatuses::new(&report);
            self.vcs.revision = Some(report.revision);
        }
        if now - self.vcs.last_poll >= POLL {
            self.vcs.last_poll = now;
            let _ = registry.dispatch(session, "vcs.status.refresh", json!({ "ifChanged": true }));
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(POLL));
        self.vcs.operation = registry
            .dispatch(session, "vcs.operation.status", json!({}))
            .ok()
            .and_then(|operation| serde_json::from_value(operation).ok());
        let Some(operation) = self.vcs.operation.clone() else { return };
        if operation.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
        } else if self.vcs.seen_operation.is_some_and(|seen| seen < operation.id)
            && let Some(message) =
                operation.message.as_ref().map(zen_i18n::Message::text).filter(|message| !message.is_empty())
        {
            let conflicted = operation.succeeded == Some(false)
                && operation.kind == "update"
                && registry
                    .dispatch(session, "vcs.conflict.list", json!({}))
                    .ok()
                    .is_some_and(|list| list["files"].as_array().is_some_and(|files| !files.is_empty()));
            if conflicted {
                self.git_workspace.show_conflicts(message);
            } else {
                self.post_at(ctx, message);
            }
        }
        if !operation.running && operation.kind == "commit" {
            let succeeded = operation.succeeded == Some(true);
            if self.commit_window.committing {
                self.commit_window.committing = false;
                if succeeded {
                    self.commit_window.committed();
                }
            }
            if self.git_workspace.committing {
                self.staged_commit_over(succeeded);
            }
        }
        if !operation.running {
            self.vcs.seen_operation = Some(operation.id);
        } else if self.vcs.seen_operation.is_none() {
            self.vcs.seen_operation = Some(operation.id - 1);
        }
    }

    pub(super) fn vcs_files_changed(&mut self, registry: &Registry, session: &mut Session) {
        if self.vcs.info.root.is_some() {
            let _ = registry.dispatch(session, "vcs.status.refresh", json!({}));
        }
    }

    pub(super) fn vcs_color(&self, path: &std::path::Path) -> Option<Color32> {
        self.vcs.statuses.of(path).map(|status| status_color(&self.palette, status))
    }

    pub(super) fn vcs_widget(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        x: f32,
        y: f32,
    ) -> f32 {
        if self.vcs.info.root.is_none() {
            self.vcs.popup.close();
            return x;
        }
        let p = self.palette;
        let galley = ui.painter().layout_no_wrap(self.vcs.info.label.clone(), theme::regular(13.0), p.text);
        let arrows: Vec<(Glyph, Color32)> = [
            (glyph_incoming as Glyph, self.vcs.info.behind, p.vcs_incoming),
            (glyph_outgoing as Glyph, self.vcs.info.ahead, p.vcs_outgoing),
        ]
        .into_iter()
        .filter(|(_, count, _)| *count > 0)
        .map(|(glyph, _, color)| (glyph, color))
        .collect();
        let arrows_width = arrows.len() as f32 * 12.0;
        let widget = Rect::from_min_size(pos2(x, y - 14.5), vec2(galley.size().x + arrows_width + 56.0, 29.0));
        let response = ui.interact(widget, ui.scope_id().with("vcs-widget"), Sense::click());
        crate::accessibility::button(&response, galley.text());
        if self.vcs.popup.open {
            ui.painter().rect_filled(widget, 6, p.hover);
        } else {
            super::hover(ui, &response, &p);
            super::hint(ui, &response, &t!("ide-action-branches"), None, super::Side::Below);
        }
        glyph_branch(
            ui.painter(),
            Rect::from_center_size(pos2(widget.left() + 17.0, y), Vec2::splat(14.0)),
            p.text_secondary,
        );
        let text_right = widget.left() + 32.0 + galley.size().x;
        ui.painter().galley(pos2(widget.left() + 32.0, y - galley.size().y / 2.0), galley, p.text);
        for (index, (glyph, color)) in arrows.into_iter().enumerate() {
            let center = pos2(text_right + 10.0 + index as f32 * 12.0, y);
            glyph(ui.painter(), Rect::from_center_size(center, Vec2::splat(8.0)), color);
        }
        crate::icons::chevron_down(
            ui.painter(),
            Rect::from_center_size(pos2(widget.right() - 13.5, y), Vec2::splat(11.0)),
            p.text_secondary,
        );
        if std::mem::take(&mut self.vcs.open_branches) && !self.vcs.popup.open {
            self.vcs.popup.toggle();
        }
        if response.clicked() {
            self.vcs.popup.toggle();
        }
        self.branches_popup(ui, registry, session, widget);
        widget.right()
    }

    pub(super) fn open_vcs_dialogs(&self) -> Vec<&'static str> {
        let vcs = &self.vcs;
        [
            (vcs.popup.open || vcs.open_branches, "branches"),
            (vcs.new_branch.is_some(), "newBranch"),
            (vcs.checkout_revision.is_some(), "checkoutRevision"),
            (vcs.update.is_some(), "updateProject"),
            (vcs.push.is_some(), "push"),
        ]
        .into_iter()
        .filter_map(|(open, name)| open.then_some(name))
        .collect()
    }

    pub(super) fn open_new_branch(&mut self, start: Option<String>) {
        if self.vcs.info.root.is_some() {
            self.vcs.new_branch =
                Some(NewBranch { name: String::new(), start, checkout: true, focused: false, error: None });
        }
    }

    pub(super) fn open_checkout_revision(&mut self) {
        if self.vcs.info.root.is_some() {
            self.vcs.checkout_revision = Some(RevisionDialog::default());
        }
    }

    pub(super) fn open_update(&mut self) {
        if self.vcs.info.root.is_some() {
            self.vcs.update = Some(self.vcs.rebase);
        }
    }

    pub(super) fn open_push(&mut self, registry: &Registry, session: &mut Session) {
        if self.vcs.info.root.is_none() {
            return;
        }
        let outgoing = match registry.dispatch(session, "vcs.push.outgoing", json!({})) {
            Ok(value) => serde_json::from_value(value).map_err(|error| error.to_string()),
            Err(zen_core::DispatchError::Failed { error, .. }) => Err(error.message),
            Err(other) => Err(other.to_string()),
        };
        self.vcs.push = Some(PushDialog { outgoing });
    }

    pub(super) fn open_diff(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        path: &std::path::Path,
    ) {
        let view = match registry.dispatch(session, "vcs.diff.file", json!({ "path": path })) {
            Ok(value) => serde_json::from_value(value).ok(),
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                self.post_at(ctx, error.message);
                None
            }
            Err(_) => None,
        };
        if let Some(view) = view {
            self.show_diff(session, view);
        }
    }

    pub(super) fn refresh_diff(&mut self, registry: &Registry, session: &mut Session) {
        let Some(diff) = &self.diff else { return };
        let Some(part) = diff.view.part else { return };
        let params = json!({ "path": diff.view.path, "part": part });
        let view =
            registry.dispatch(session, "vcs.diff.file", params).ok().and_then(|view| serde_json::from_value(view).ok());
        let revision = self.vcs.info.revision;
        let highlighter = self.diff.as_ref().and_then(|diff| self.highlighter_at(&diff.view.path));
        let mut rehunk = false;
        if let Some(diff) = &mut self.diff {
            if let Some(view) = view {
                diff.replace(view, highlighter);
                rehunk = diff.whitespace != zen_workbench::compare::Whitespace::None;
            }
            diff.revision = Some(revision);
        }
        if rehunk {
            self.rehunk_diff(registry, session);
        }
    }

    pub(super) fn show_diff(&mut self, session: &Session, view: zen_workbench::vcs::commit::DiffView) {
        let highlighter = self.highlighter_at(&view.path);
        let revision = view.part.is_some().then_some(self.vcs.info.revision);
        self.diff = Some(super::diff_view::DiffTab::new(view, highlighter, self.diff_unified));
        if let Some(diff) = &mut self.diff {
            diff.revision = revision;
        }
        self.bring_front(FrontKind::Diff, session);
        self.diff_from_workspace = false;
        self.close_text_dialogs();
    }

    pub(super) fn highlighter_for(&self, name: &str) -> Option<std::sync::Arc<dyn zen_syntax::Highlighter>> {
        self.plugins.highlighter(&self.product.highlighters, name)
    }

    pub(super) fn highlighter_at(&self, path: &std::path::Path) -> Option<std::sync::Arc<dyn zen_syntax::Highlighter>> {
        let known = zen_vfs::file_types::known_name_at(path, None)?;
        self.highlighter_for(&known)
    }

    pub(super) fn rollback_context(&self, session: &Session) -> Vec<std::path::PathBuf> {
        let active = session
            .state::<zen_workbench::Workbench>()
            .and_then(zen_workbench::Workbench::active_document)
            .map(|document| document.path().to_owned());
        if self.commit_open {
            return self.commit_window.selected().into_iter().collect();
        }
        match active {
            Some(path) if self.vcs.statuses.of(&path).is_some() => vec![path],
            _ => Vec::new(),
        }
    }

    pub(super) fn vcs_dialogs(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        self.new_branch_dialog(ui, registry, session);
        self.checkout_revision_dialog(ui, registry, session);
        self.update_dialog(ui, registry, session);
        self.push_dialog(ui, registry, session);
        self.workspace_dialogs(ui, registry, session);
    }

    fn new_branch_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &mut self.vcs.new_branch else { return };
        let p = self.palette;
        let (mut create, mut cancel) = (false, false);
        let modal =
            egui::Modal::new(ui.scope_id().with("vcs-new-branch")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(t!("vcs-new-branch-title")).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(14.0);
                ui.label(egui::RichText::new(t!("vcs-new-branch-name")).font(theme::regular(13.0)).color(p.text));
                ui.add_space(6.0);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut dialog.name)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 6)),
                );
                if !std::mem::replace(&mut dialog.focused, true) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    create = true;
                }
                if let Some(error) = &dialog.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(error).font(theme::regular(12.0)).color(p.vcs_conflict));
                }
                ui.add_space(10.0);
                ui.checkbox(&mut dialog.checkout, t!("vcs-new-branch-checkout"));
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if primary_button(ui, &p, &t!("vcs-new-branch-create")).clicked() {
                        create = true;
                    }
                    if secondary_button(ui, &p, &t!("settings-cancel")).clicked() {
                        cancel = true;
                    }
                });
            });
        if create {
            let params = json!({ "name": dialog.name.trim(), "startPoint": dialog.start, "checkout": dialog.checkout });
            match registry.dispatch(session, "vcs.branch.create", params) {
                Ok(_) => self.vcs.new_branch = None,
                Err(zen_core::DispatchError::Failed { error, .. }) => dialog.error = Some(error.message),
                Err(other) => dialog.error = Some(other.to_string()),
            }
        } else if cancel || modal.should_close() {
            self.vcs.new_branch = None;
        }
    }

    fn checkout_revision_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &mut self.vcs.checkout_revision else { return };
        let p = self.palette;
        let (mut checkout, mut cancel) = (false, false);
        let modal = egui::Modal::new(ui.scope_id().with("vcs-checkout-revision")).frame(dialog_frame(&p)).show(
            ui.ctx(),
            |ui| {
                ui.set_width(420.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let title = t!("vcs-checkout-revision-title");
                ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(14.0);
                let label = t!("vcs-checkout-revision-label");
                ui.label(egui::RichText::new(label).font(theme::regular(13.0)).color(p.text));
                ui.add_space(6.0);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut dialog.revision)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 6)),
                );
                if !std::mem::replace(&mut dialog.focused, true) {
                    field.request_focus();
                }
                if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    checkout = true;
                }
                if let Some(error) = &dialog.error {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(error).font(theme::regular(12.0)).color(p.vcs_conflict));
                }
                ui.add_space(20.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if primary_button(ui, &p, &t!("vcs-checkout-revision-checkout")).clicked() {
                        checkout = true;
                    }
                    if secondary_button(ui, &p, &t!("settings-cancel")).clicked() {
                        cancel = true;
                    }
                });
            },
        );
        if checkout {
            let params = json!({ "revision": dialog.revision.trim() });
            let checkout = |session: &mut Session| registry.dispatch(session, "vcs.revision.checkout", params);
            match zen_workbench::vcs::off_the_frame(session, checkout) {
                Ok(_) => self.vcs.checkout_revision = None,
                Err(zen_core::DispatchError::Failed { error, .. }) => dialog.error = Some(error.message),
                Err(other) => dialog.error = Some(other.to_string()),
            }
        } else if cancel || modal.should_close() {
            self.vcs.checkout_revision = None;
        }
    }

    fn update_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(mut rebase) = self.vcs.update else { return };
        let p = self.palette;
        let (mut ok, mut cancel) = (false, false);
        let modal = egui::Modal::new(ui.scope_id().with("vcs-update")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.set_width(440.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.label(egui::RichText::new(t!("vcs-update-title")).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(14.0);
            ui.label(egui::RichText::new(t!("vcs-update-type")).font(theme::regular(13.0)).color(p.text_secondary));
            ui.add_space(8.0);
            ui.radio_value(&mut rebase, false, t!("vcs-update-merge"));
            ui.add_space(6.0);
            ui.radio_value(&mut rebase, true, t!("vcs-update-rebase"));
            ui.add_space(20.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if primary_button(ui, &p, &t!("vcs-ok")).clicked() {
                    ok = true;
                }
                if secondary_button(ui, &p, &t!("settings-cancel")).clicked() {
                    cancel = true;
                }
            });
            if ui.input(|input| input.key_pressed(Key::Enter)) {
                ok = true;
            }
        });
        self.vcs.update = Some(rebase);
        if ok {
            self.vcs.update = None;
            self.vcs.rebase = rebase;
            self.dispatch(ui, registry, session, "vcs.project.update", json!({ "rebase": rebase }));
        } else if cancel || modal.should_close() {
            self.vcs.update = None;
        }
    }

    fn push_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &self.vcs.push else { return };
        let p = self.palette;
        let project = self
            .vcs
            .info
            .root
            .as_ref()
            .and_then(|root| root.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let (mut push, mut force, mut cancel) = (false, false, false);
        let modal = egui::Modal::new(ui.scope_id().with("vcs-push")).frame(dialog_frame(&p)).show(ui.ctx(), |ui| {
            ui.set_width(620.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            let title = t!("vcs-push-title", repository = project.unwrap_or_default().as_str());
            ui.label(egui::RichText::new(title).font(theme::semibold(15.0)).color(p.text));
            ui.add_space(14.0);
            match &dialog.outgoing {
                Err(error) => {
                    ui.label(egui::RichText::new(error).font(theme::regular(13.0)).color(p.vcs_conflict));
                }
                Ok(outgoing) => {
                    ui.horizontal(|ui| {
                        let (icon, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        glyph_branch(ui.painter(), icon.shrink(1.0), p.text_secondary);
                        ui.add_space(6.0);
                        let route = format!("{} → {} : {}", outgoing.branch, outgoing.remote, outgoing.target);
                        ui.label(egui::RichText::new(route).font(theme::semibold(13.0)).color(p.text));
                        if outgoing.new_branch {
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new(t!("vcs-push-new")).font(theme::semibold(12.0)).color(p.vcs_added),
                            );
                        }
                    });
                    ui.add_space(8.0);
                    egui::Frame::new().fill(p.island).corner_radius(6).inner_margin(6).show(ui, |ui| {
                        ui.set_min_height(220.0);
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                            if outgoing.commits.is_empty() {
                                ui.label(
                                    egui::RichText::new(t!("vcs-push-no-commits"))
                                        .font(theme::regular(13.0))
                                        .color(p.text_secondary),
                                );
                            }
                            for commit in &outgoing.commits {
                                let (row, response) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                                let painter = ui.painter().with_clip_rect(row);
                                let meta = painter.text(
                                    pos2(row.right() - 6.0, row.center().y),
                                    Align2::RIGHT_CENTER,
                                    format!(
                                        "{}  {}",
                                        commit.author,
                                        format_date(commit.time + i64::from(commit.offset))
                                    ),
                                    theme::regular(12.5),
                                    p.text_secondary,
                                );
                                let width = (meta.left() - 16.0 - (row.left() + 6.0)).max(0.0);
                                let subject =
                                    painter.layout_job(truncated(&commit.subject, theme::regular(13.0), p.text, width));
                                let elided = subject.elided;
                                painter.galley(
                                    pos2(row.left() + 6.0, row.center().y - subject.size().y / 2.0),
                                    subject,
                                    p.text,
                                );
                                if elided {
                                    response.on_hover_text(&commit.subject);
                                }
                            }
                        });
                    });
                }
            }
            ui.add_space(20.0);
            let can_push = dialog.outgoing.is_ok();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if can_push {
                    if primary_button(ui, &p, &t!("vcs-push-push")).clicked() {
                        push = true;
                    }
                    if secondary_button(ui, &p, &t!("vcs-push-force")).clicked() {
                        force = true;
                    }
                }
                if secondary_button(ui, &p, &t!("settings-cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if push || force {
            let remote_tip = dialog.outgoing.as_ref().ok().and_then(|outgoing| outgoing.remote_tip.clone());
            self.vcs.push = None;
            self.dispatch(ui, registry, session, "vcs.remote.push", json!({ "force": force, "remoteTip": remote_tip }));
        } else if cancel || modal.should_close() {
            self.vcs.push = None;
        }
    }
}

fn unsaved_stamp(session: &Session) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Some(workbench) = session.state::<zen_workbench::Workbench>() {
        for document in workbench.documents().iter().filter(|document| document.is_dirty()) {
            (document.path(), document.version()).hash(&mut hasher);
        }
    }
    hasher.finish()
}

pub(super) fn truncated(text: &str, font: egui::FontId, color: Color32, width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    job
}

pub(super) fn short_branch(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    if chars.len() <= 32 {
        return name.to_owned();
    }
    let head: String = chars[..14].iter().collect();
    let tail: String = chars[chars.len() - 15..].iter().collect();
    format!("{head}…{tail}")
}

pub(super) fn primary_button(ui: &mut Ui, p: &IdePalette, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).font(theme::regular(13.0)).color(p.accent_text))
            .min_size(vec2(84.0, 30.0))
            .corner_radius(6)
            .fill(p.accent),
    )
}

pub(super) fn secondary_button(ui: &mut Ui, p: &IdePalette, label: &str) -> egui::Response {
    secondary_button_sized(ui, p, label, BUTTON_WIDTH)
}

pub(super) const BUTTON_WIDTH: f32 = 84.0;

pub(super) const BUTTON_PADDING: f32 = 12.0;

pub(super) fn secondary_button_sized(ui: &mut Ui, p: &IdePalette, label: &str, width: f32) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).font(theme::regular(13.0)).color(p.text))
            .min_size(vec2(width, 30.0))
            .corner_radius(6)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, p.button_border)),
    )
}

pub(super) fn format_date(seconds: i64) -> String {
    let days = seconds.div_euclid(86_400);
    let minutes = seconds.rem_euclid(86_400) / 60;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frame_waits_for_a_full_status() {
        let sources = [
            include_str!("mod.rs"),
            include_str!("commit_window.rs"),
            include_str!("git_workspace/conflicts.rs"),
            include_str!("git_workspace.rs"),
        ];
        for source in sources {
            let code = source.split("#[cfg(test)]").next().unwrap_or_default();
            assert!(!code.contains(concat!("\"vcs.status.refresh\", json!({ \"", "wait\"")));
            assert!(!code.contains(concat!("\"vcs.status.refresh\", serde_json::json!({ \"", "wait\"")));
        }
    }

    #[test]
    fn dates_read_as_calendar_dates() {
        assert_eq!(format_date(0), "1970-01-01 00:00");
        assert_eq!(format_date(1_792_332_300), "2026-10-18 14:05");
        assert_eq!(format_date(951_782_400), "2000-02-29 00:00");
    }

    #[test]
    fn long_branch_names_are_shortened_in_the_middle() {
        assert_eq!(short_branch("main"), "main");
        let long = "feature/published-course-short-description";
        let short = short_branch(long);
        assert_eq!(short, "feature/publis…ort-description");
        assert!(short.chars().count() <= 32);
    }

    #[test]
    fn every_status_has_its_own_colour() {
        let p = IdePalette::zen_dark();
        let all = [
            FileStatus::Added,
            FileStatus::Modified,
            FileStatus::Deleted,
            FileStatus::Unversioned,
            FileStatus::Ignored,
            FileStatus::Conflicted,
        ];
        let colours: std::collections::HashSet<_> = all.iter().map(|status| status_color(&p, *status)).collect();
        assert_eq!(colours.len(), all.len());
        assert_eq!(status_color(&p, FileStatus::Renamed), p.vcs_modified);
    }
}

use std::path::{Path, PathBuf};

use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::compare::{CommitsDiff, FileAction, FileHistoryList};
use zen_workbench::vcs::workspace::GitWorkspace;

use super::super::find_bar::glyph_close;
use super::super::vcs::{format_date, truncated};
use super::{
    Ask, FilePick, GitWorkspaceView, commit_file_rows, draw_commit_files, file_counts, file_status, files_toolbar,
    status_mark,
};
use super::{IdePalette, IdeView, Side, hint};
use crate::theme;

pub(super) struct Compared {
    key: (String, Option<String>, Option<u64>),
    diff: CommitsDiff,
    subjects: (String, Option<String>),
}

pub(super) struct History {
    key: (PathBuf, Option<u64>),
    list: FileHistoryList,
    opened: Option<String>,
}

#[cfg(test)]
impl GitWorkspaceView {
    pub(in crate::ide) fn compared_files(&self) -> Option<Vec<String>> {
        let compared = self.compared.as_ref()?;
        Some(compared.diff.files.iter().map(|file| format!("{} {}", file.relative, file.status)).collect())
    }

    pub(in crate::ide) fn history_subjects(&self) -> Option<Vec<String>> {
        let history = self.file_history.as_ref()?;
        Some(history.list.commits.iter().map(|entry| entry.subject.clone()).collect())
    }
}

fn short(id: &str) -> String {
    id.chars().take(7).collect()
}

fn header(ui: &mut Ui, p: &IdePalette, caption: &str, value: &str, mono: bool) -> bool {
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
    let close = Rect::from_center_size(pos2(row.right() - 20.0, row.center().y), Vec2::splat(22.0));
    let room = (close.left() - row.left() - 24.0).max(40.0);
    let label = ui.painter().layout_no_wrap(format!("{caption}:"), theme::regular(13.0), p.text_secondary);
    let font = if mono { FontId::monospace(13.0) } else { theme::semibold(13.0) };
    let value = ui.painter().layout(value.to_owned(), font, p.text, (room - label.size().x - 6.0).max(20.0));
    let width = label.size().x + 6.0 + value.size().x;
    let start = (row.left() + (close.left() - 8.0 - row.left() - width) / 2.0).max(row.left() + 12.0);
    let label_width = label.size().x;
    ui.painter().galley(pos2(start, row.center().y - label.size().y / 2.0), label, p.text_secondary);
    ui.painter().galley(pos2(start + label_width + 6.0, row.center().y - value.size().y / 2.0), value, p.text);
    ui.painter().hline(row.x_range(), row.bottom() - 0.5, Stroke::new(1.0, p.frame));
    let response = ui.interact(close, ui.scope_id().with("gw-panel-close"), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(close, 4, p.hover);
    }
    glyph_close(ui.painter(), close.shrink(6.0), p.text_secondary);
    hint(ui, &response, &t!("vcs-workspace-panel-close"), None, Side::Below);
    response.clicked()
}

fn point(ui: &mut Ui, p: &IdePalette, caption: &str, id: Option<&str>, subject: &str, asks: &mut Vec<Ask>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_sized(
            vec2(34.0, 18.0),
            egui::Label::new(egui::RichText::new(caption).font(theme::regular(12.0)).color(p.text_secondary)),
        );
        match id {
            Some(id) => {
                let link = ui.add(
                    egui::Label::new(egui::RichText::new(short(id)).font(FontId::monospace(12.5)).color(p.link))
                        .sense(Sense::click()),
                );
                if link.clicked() {
                    asks.push(Ask::Select(id.to_owned()));
                }
                hint(ui, &link, id, Some(&t!("vcs-workspace-compare-select")), Side::Below);
                link.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
            None => {
                ui.label(egui::RichText::new(t!("vcs-workspace-wip")).font(theme::semibold(12.5)).color(p.text));
            }
        }
        ui.add(egui::Label::new(egui::RichText::new(subject).font(theme::regular(13.0)).color(p.text)).truncate());
    });
}

impl IdeView {
    pub(super) fn workspace_compare(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
        (from, to): (&str, Option<&str>),
        asks: &mut Vec<Ask>,
    ) {
        let status = to.is_none().then_some(self.vcs.info.revision);
        let key = (from.to_owned(), to.map(str::to_owned), status);
        let view = &mut self.git_workspace;
        if view.compared.as_ref().is_none_or(|compared| compared.key != key) {
            let diff = registry
                .dispatch(session, "vcs.diff.commits", json!({ "from": from, "to": to }))
                .ok()
                .and_then(|diff| serde_json::from_value::<CommitsDiff>(diff).ok())
                .unwrap_or_default();
            let mut subject = |id: &str| {
                registry
                    .dispatch(session, "vcs.commit.details", json!({ "id": id }))
                    .ok()
                    .and_then(|details| details["subject"].as_str().map(str::to_owned))
                    .unwrap_or_default()
            };
            let subjects = (subject(from), to.map(&mut subject));
            view.compared = Some(Compared { key, diff, subjects });
        }
        let Some(compared) = view.compared.as_ref() else { return };
        let (diff, subjects) = (compared.diff.clone(), compared.subjects.clone());
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let settings = session.state::<GitWorkspace>().map(|workspace| workspace.view().clone()).unwrap_or_default();
        let selected = session.state::<GitWorkspace>().and_then(|w| w.selected_commit().map(str::to_owned));
        egui::ScrollArea::vertical().id_salt("gw-compare-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let target = to.map_or_else(|| t!("vcs-workspace-wip"), short);
            let value = format!("{} → {}", short(from), target);
            if header(ui, p, &t!("vcs-workspace-compare-title"), &value, true) {
                asks.push(match &selected {
                    Some(id) => Ask::Select(id.clone()),
                    None => Ask::SelectWip,
                });
            }
            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 12, bottom: 4 }).show(ui, |ui| {
                egui::Frame::new()
                    .stroke(Stroke::new(1.0, p.frame))
                    .corner_radius(6)
                    .inner_margin(egui::Margin { left: 12, right: 12, top: 8, bottom: 8 })
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 6.0;
                        point(ui, p, &t!("vcs-workspace-compare-from"), Some(from), &subjects.0, asks);
                        let to_subject = subjects
                            .1
                            .clone()
                            .unwrap_or_else(|| t!("vcs-workspace-wip-on", branch = self.vcs.info.label.as_str()));
                        point(ui, p, &t!("vcs-workspace-compare-to"), to, &to_subject, asks);
                    });
            });
            ui.add_space(10.0);
            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 4, bottom: 4 }).show(ui, |ui| {
                file_counts(ui, p, &diff.files);
                ui.add_space(8.0);
                if let Some(ask) = files_toolbar(ui, p, settings.clone(), false) {
                    asks.push(ask);
                }
            });
            ui.add_space(4.0);
            let rows = commit_file_rows(&diff.files, None);
            let view = &mut self.git_workspace;
            let menu =
                view.file_menu_for.as_ref().map(|target| target.path.as_path()).zip(Some(&mut view.file_menu_at));
            match draw_commit_files(ui, p, &rows, (settings.files, &root), &mut view.collapsed_folders, menu) {
                Some(FilePick::Open(path)) => {
                    asks.push(Ask::DiffBetween { base: diff.from.clone(), id: diff.to.clone(), path });
                }
                Some(FilePick::Menu(path, at)) => {
                    view.file_right_click = Some(at);
                    asks.push(Ask::FileMenu(Some((path, None))));
                }
                None => {}
            }
            ui.add_space(10.0);
        });
    }

    pub(super) fn workspace_file_history(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        p: &IdePalette,
        path: &Path,
        asks: &mut Vec<Ask>,
    ) {
        let view = &mut self.git_workspace;
        let key = (path.to_owned(), view.graph_revision);
        if view.file_history.as_ref().is_none_or(|history| history.key != key) {
            let list = registry
                .dispatch(session, "vcs.file.history", json!({ "path": path }))
                .ok()
                .and_then(|list| serde_json::from_value::<FileHistoryList>(list).ok())
                .unwrap_or_default();
            let opened = view.file_history.take().filter(|history| history.key.0 == path).and_then(|h| h.opened);
            view.file_history = Some(History { key, list, opened });
        }
        let selected = session.state::<GitWorkspace>().and_then(|w| w.selected_commit().map(str::to_owned));
        let Some(history) = view.file_history.as_mut() else { return };
        let list = history.list.clone();
        let name = path.file_name().map_or_else(|| path.display().to_string(), |name| name.to_string_lossy().into());
        egui::ScrollArea::vertical().id_salt("gw-history-scroll").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if header(ui, p, &t!("vcs-workspace-history-title"), &name, false) {
                asks.push(match &selected {
                    Some(id) => Ask::Select(id.clone()),
                    None => Ask::SelectWip,
                });
            }
            egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 14, top: 10, bottom: 8 }).show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&list.relative).font(theme::regular(12.5)).color(p.text_secondary),
                    )
                    .truncate(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let count = if list.more {
                        t!("vcs-workspace-history-newest", count = list.commits.len())
                    } else {
                        t!("vcs-workspace-history-count", count = list.commits.len())
                    };
                    ui.label(egui::RichText::new(count).font(theme::regular(12.5)).color(p.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let link = ui.add(
                            egui::Label::new(
                                egui::RichText::new(t!("vcs-workspace-history-in-log"))
                                    .font(theme::regular(12.5))
                                    .color(p.link),
                            )
                            .sense(Sense::click()),
                        );
                        if link.clicked() {
                            asks.push(Ask::ActionWith("view.vcs.showHistory", json!({ "path": path })));
                        }
                        link.on_hover_cursor(egui::CursorIcon::PointingHand);
                    });
                });
            });
            ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, p.frame));
            ui.add_space(4.0);
            if list.commits.is_empty() {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(t!("vcs-workspace-history-empty"))
                            .font(theme::regular(12.5))
                            .color(p.text_secondary),
                    );
                });
            }
            for entry in &list.commits {
                let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
                let band = row.shrink2(vec2(8.0, 1.0));
                if history.opened.as_deref() == Some(entry.id.as_str()) {
                    ui.painter().rect_filled(band, 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(band, 5, p.hover);
                }
                let mark = Rect::from_center_size(pos2(row.left() + 24.0, row.top() + 15.0), Vec2::splat(13.0));
                status_mark(ui.painter(), p, mark, file_status(&entry.status));
                let text =
                    Rect::from_min_max(pos2(row.left() + 38.0, row.top()), pos2(row.right() - 12.0, row.bottom()));
                let painter = ui.painter().with_clip_rect(text.intersect(ui.clip_rect()));
                let fit = |text: &str, font: FontId, color, width: f32| {
                    painter.layout_job(truncated(text, font, color, width.max(0.0)))
                };
                let subject = fit(&entry.subject, theme::regular(13.0), p.text, text.width());
                painter.galley(pos2(text.left(), row.top() + 15.0 - subject.size().y / 2.0), subject, p.text);
                let sha = painter.text(
                    pos2(text.left(), row.top() + 33.0),
                    Align2::LEFT_CENTER,
                    &entry.short_id[..entry.short_id.len().min(7)],
                    FontId::monospace(11.5),
                    p.link,
                );
                let link =
                    ui.interact(sha.expand(2.0), ui.scope_id().with(("gw-history-sha", &entry.id)), Sense::click());
                if link.clicked() {
                    asks.push(Ask::Select(entry.id.clone()));
                }
                hint(ui, &link, &entry.id, Some(&t!("vcs-workspace-compare-select")), Side::Below);
                let link = link.on_hover_cursor(egui::CursorIcon::PointingHand);
                if link.hovered() {
                    painter.hline(sha.x_range(), sha.bottom(), Stroke::new(1.0, p.link));
                }
                let date = format_date(entry.time + i64::from(entry.offset));
                let date = painter.text(
                    pos2(text.right(), row.top() + 33.0),
                    Align2::RIGHT_CENTER,
                    date,
                    theme::regular(11.5),
                    p.text_secondary,
                );
                let author = Rect::from_x_y_ranges(sha.right() + 8.0..=date.left() - 8.0, row.y_range());
                let name = fit(&entry.author, theme::regular(11.5), p.text_secondary, author.width());
                painter.galley(pos2(author.left(), row.top() + 33.0 - name.size().y / 2.0), name, p.text_secondary);
                let mut tip = format!("{}\n{}", entry.id, entry.subject);
                if let Some(old) = &entry.old_path {
                    tip.push('\n');
                    tip.push_str(&t!("vcs-workspace-history-renamed", path = old.as_str()));
                }
                let response = response.on_hover_text(tip);
                if response.clicked() {
                    history.opened = Some(entry.id.clone());
                    match entry.parents.first().filter(|_| entry.old_path.is_some()) {
                        Some(parent) => asks.push(Ask::DiffBetween {
                            base: parent.clone(),
                            id: Some(entry.id.clone()),
                            path: entry.path.clone(),
                        }),
                        None => {
                            asks.push(Ask::Diff { id: Some(entry.id.clone()), path: entry.path.clone(), part: None })
                        }
                    }
                }
                response.on_hover_cursor(egui::CursorIcon::PointingHand);
            }
            ui.add_space(10.0);
        });
    }
}

pub(super) fn file_menu(ui: &Ui, view: &mut GitWorkspaceView, asks: &mut Vec<Ask>) {
    let (Some(target), Some(at)) = (view.file_menu_for.clone(), view.file_menu_at) else { return };
    let mut focus_first = view.file_menu_keyboard;
    let mut focused = false;
    let mut open = true;
    let exists = target.path.is_file();
    let popup = egui::Popup::new(ui.scope_id().with("gw-file-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    popup.show(|ui| {
        ui.set_min_width(200.0);
        let mut item = |ui: &mut Ui, title: String, enabled: bool, action: FileAction| {
            let response = ui.add_enabled(enabled, egui::Button::new(title));
            if focus_first && !ui.is_sizing_pass() {
                focus_first = false;
                focused = true;
                response.request_focus();
            }
            if response.clicked() {
                asks.push(Ask::File(target.path.clone(), action));
                ui.close();
            }
        };
        item(ui, t!("vcs-workspace-file-history"), true, FileAction::History);
        item(ui, t!("vcs-workspace-file-blame"), exists, FileAction::Blame);
        ui.separator();
        item(ui, t!("vcs-workspace-file-open"), exists, FileAction::Open);
    });
    if focused {
        view.file_menu_keyboard = false;
    }
    if !open {
        asks.push(Ask::FileMenu(None));
    }
}

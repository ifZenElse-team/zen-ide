use std::collections::{BTreeMap, BTreeSet};

use egui::{Align2, Color32, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::notifications::{Action, Group, Kind, NewNotification};

use super::settings::Dispatch;
use super::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::theme;

const POLL_SECONDS: f64 = 1.0;
const INSTALLED_SECONDS: f64 = 6.0;
const LOG_LIMIT: usize = 5000;
const POPUP_WIDTH: f32 = 440.0;

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ServerInfo {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) state: String,
    pub(super) version: Option<String>,
    pub(super) path: Option<String>,
    pub(super) source: String,
    pub(super) args: Vec<String>,
    pub(super) files: u32,
    pub(super) languages: Vec<String>,
    pub(super) extensions: Vec<String>,
    pub(super) names: Vec<String>,
    pub(super) enabled: bool,
    pub(super) message: Option<String>,
    pub(super) installable: bool,
    pub(super) installer: Option<Installer>,
    pub(super) installing: bool,
    pub(super) missing_plugins: Vec<String>,
    pub(super) needed: bool,
    pub(super) dismissed: bool,
    pub(super) busy: bool,
    pub(super) progress: Vec<Progress>,
    pub(super) alternatives: Vec<String>,
    pub(super) notice: Option<String>,
    pub(super) notice_url: Option<String>,
    pub(super) java_needed: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Progress {
    pub(super) title: String,
    pub(super) message: Option<String>,
    pub(super) percentage: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Installer {
    pub(super) tool: String,
    pub(super) download: String,
}

impl ServerInfo {
    fn is_missing(&self) -> bool {
        self.state == "missing"
    }

    fn offers_install(&self) -> bool {
        (self.is_missing() || self.lacks_plugins()) && self.needed && self.installable && !self.installing
    }

    fn lacks_plugins(&self) -> bool {
        !self.is_missing() && !self.missing_plugins.is_empty()
    }

    fn shown_state(&self) -> &str {
        if self.installing {
            "installing"
        } else if self.busy && self.state == "running" {
            "busy"
        } else {
            &self.state
        }
    }

    pub(super) fn work(&self) -> Option<String> {
        let progress = self.progress.first().filter(|_| self.busy)?;
        Some(match progress.percentage {
            Some(percentage) => format!("{} {percentage}%", progress.title),
            None => progress.title.clone(),
        })
    }

    fn serves(&self, file_name: &str) -> bool {
        zen_workbench::language::servers::file_matches(&self.extensions, &self.names, file_name)
    }

    fn download(&self) -> Option<(String, String)> {
        match self.installer.as_ref().filter(|installer| !installer.download.is_empty()) {
            Some(installer) => {
                Some((t!("ide-lsp-download", tool = installer.tool.as_str()), installer.download.clone()))
            }
            None => self.notice_url.clone().map(|url| (t!("ide-lsp-notice-link"), url)),
        }
    }

    fn with_notice(&self, text: String) -> String {
        match self.notice.as_deref().filter(|notice| !notice.is_empty()) {
            Some(notice) => format!("{text} {notice}"),
            None => text,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
struct ServerList {
    #[serde(default)]
    servers: Vec<ServerInfo>,
}

#[derive(Debug, Default, Deserialize)]
struct LogChunk {
    #[serde(default)]
    lines: Vec<String>,
    #[serde(default)]
    next: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ServerAction {
    Install(String),
    Restart(String),
    Stop(String),
    ShowLog(String),
    Dismiss(String),
    Close(String),
    OpenSettings,
    Download(String),
    DownloadJdk(String, u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Banner {
    pub(super) id: String,
    pub(super) text: String,
    pub(super) install: bool,
    pub(super) dismiss: bool,
    pub(super) download: Option<(String, String)>,
    pub(super) jdk: Option<u32>,
}

#[derive(Debug, Default)]
struct LogView {
    id: String,
    name: String,
    lines: Vec<String>,
    next: u64,
    polled: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct InstallFinished {
    pub(super) text: String,
    pub(super) failed: bool,
}

#[derive(Debug, Default)]
pub(super) struct LanguageServers {
    servers: Vec<ServerInfo>,
    loaded: bool,
    polled: Option<f64>,
    popup: bool,
    closed: BTreeSet<String>,
    installed: BTreeMap<String, f64>,
    asked_install: BTreeSet<String>,
    finished: Vec<InstallFinished>,
    log: Option<LogView>,
    hits: Vec<(String, &'static str, Rect)>,
}

impl LanguageServers {
    pub(super) fn servers(&self) -> &[ServerInfo] {
        &self.servers
    }

    pub(super) fn invalidate(&mut self) {
        self.polled = None;
    }

    pub(super) fn poll(&mut self, now: f64, focused: bool, dispatch: &mut Dispatch<'_>) -> bool {
        if self.loaded && (!focused || self.polled.is_some_and(|at| now - at < POLL_SECONDS)) {
            return false;
        }
        self.polled = Some(now);
        let servers = dispatch("lsp.servers.list", json!({}))
            .ok()
            .and_then(|list| serde_json::from_value::<ServerList>(list).ok())
            .unwrap_or_default()
            .servers;
        let changed = !self.loaded || servers != self.servers;
        self.loaded = true;
        if changed {
            for server in &servers {
                let was_installing = self.server(&server.id).is_some_and(|before| before.installing);
                let installed = !server.is_missing() && server.state != "failed";
                if was_installing && !server.installing && installed {
                    self.installed.insert(server.id.clone(), now);
                }
                if was_installing && !server.installing && self.asked_install.remove(&server.id) {
                    self.finished.push(install_finished(server, installed));
                }
            }
            self.servers = servers;
        }
        changed
    }

    pub(super) fn follow_notification(&mut self, command: &str, id: &str) -> Option<String> {
        self.popup = false;
        self.invalidate();
        if command == "lsp.server.dismiss" {
            self.closed.insert(id.to_owned());
            return None;
        }
        self.closed.remove(id);
        self.asked_install.insert(id.to_owned());
        let name = self.server(id).map_or(id, |server| server.name.as_str());
        Some(t!("ide-lsp-installing", name = name))
    }

    pub(super) fn take_finished(&mut self) -> Vec<InstallFinished> {
        std::mem::take(&mut self.finished)
    }

    pub(super) fn is_busy(&self) -> bool {
        self.log.is_some() || self.servers.iter().any(|server| server.installing || server.state == "starting")
    }

    pub(super) fn act(&mut self, action: ServerAction, dispatch: &mut Dispatch<'_>) -> Result<(), String> {
        self.popup = false;
        let (command, id) = match action {
            ServerAction::Install(id) => {
                self.closed.remove(&id);
                ("lsp.server.install", id)
            }
            ServerAction::Restart(id) => ("lsp.server.restart", id),
            ServerAction::Stop(id) => ("lsp.server.stop", id),
            ServerAction::Dismiss(id) => {
                self.closed.insert(id.clone());
                ("lsp.server.dismiss", id)
            }
            ServerAction::Close(id) => {
                self.closed.insert(id.clone());
                self.installed.remove(&id);
                return Ok(());
            }
            ServerAction::ShowLog(id) => {
                let name = self.server(&id).map_or_else(|| id.clone(), |server| server.name.clone());
                self.log = Some(LogView { id, name, ..LogView::default() });
                return Ok(());
            }
            ServerAction::DownloadJdk(id, version) => {
                self.closed.insert(id);
                self.invalidate();
                return dispatch("toolchain.jdk.download", json!({ "version": version })).map(|_| ());
            }
            ServerAction::OpenSettings | ServerAction::Download(_) => return Ok(()),
        };
        self.invalidate();
        dispatch(command, json!({ "id": id })).map(|_| ())
    }

    fn server(&self, id: &str) -> Option<&ServerInfo> {
        self.servers.iter().find(|server| server.id == id)
    }

    fn shown(&self) -> Vec<&ServerInfo> {
        self.servers
            .iter()
            .filter(|server| {
                server.state != "disabled"
                    && (server.needed || !matches!(server.state.as_str(), "notStarted" | "missing"))
            })
            .collect()
    }

    pub(super) fn banner(&mut self, file_name: &str, now: f64) -> Option<Banner> {
        let server = self
            .servers
            .iter()
            .find(|server| {
                server.serves(file_name) && server.alternatives.first().is_none_or(|chosen| *chosen == server.id)
            })?
            .clone();
        if let Some(&at) = self.installed.get(&server.id) {
            if now - at > INSTALLED_SECONDS {
                self.installed.remove(&server.id);
                return None;
            }
            let version = server.version.clone().unwrap_or_default();
            let text = t!("ide-lsp-installed", name = server.name.as_str(), version = version.as_str());
            return Some(Banner { id: server.id, text, install: false, dismiss: false, download: None, jdk: None });
        }
        if server.lacks_plugins() && server.needed && !server.dismissed && !self.closed.contains(&server.id) {
            if server.installing {
                let detail = server.message.clone().unwrap_or_default();
                let text = t!("ide-lsp-status", name = server.name.as_str(), detail = detail.as_str());
                return Some(Banner { id: server.id, text, install: false, dismiss: false, download: None, jdk: None });
            }
            let plugins = server.missing_plugins.join(", ");
            let text = t!("ide-lsp-plugins-missing", name = server.name.as_str(), plugins = plugins.as_str());
            let download = server.download();
            return Some(Banner {
                id: server.id,
                text,
                install: server.installable,
                dismiss: true,
                download,
                jdk: None,
            });
        }
        if let Some(version) = server.java_needed
            && !server.is_missing()
            && server.needed
            && !server.dismissed
            && !self.closed.contains(&server.id)
        {
            let text = t!("ide-lsp-java-needed", name = server.name.as_str(), version = version);
            return Some(Banner {
                id: server.id,
                text,
                install: false,
                dismiss: true,
                download: None,
                jdk: Some(version),
            });
        }
        if !server.is_missing() || !server.needed || server.dismissed || self.closed.contains(&server.id) {
            return None;
        }
        let detail = server.message.as_deref().filter(|message| !message.is_empty());
        if server.installing {
            let text = t!("ide-lsp-status", name = server.name.as_str(), detail = detail.unwrap_or_default());
            return Some(Banner { id: server.id, text, install: false, dismiss: false, download: None, jdk: None });
        }
        let text = match detail {
            Some(detail) => t!("ide-lsp-missing-detail", name = server.name.as_str(), detail = detail),
            None => t!("ide-lsp-missing", name = server.name.as_str()),
        };
        let text = server.with_notice(text);
        let download = server.download();
        Some(Banner { id: server.id, text, install: server.installable, dismiss: true, download, jdk: None })
    }

    #[cfg(test)]
    pub(super) fn hit(&self, id: &str, action: &str) -> Option<Rect> {
        self.hits.iter().find(|(shown, name, _)| shown == id && *name == action).map(|(_, _, rect)| *rect)
    }

    pub(super) fn close_popup(&mut self) {
        self.popup = false;
    }

    #[cfg(test)]
    pub(super) fn popup_open(&self) -> bool {
        self.popup
    }

    #[cfg(test)]
    pub(super) fn log(&self) -> Option<(&str, &[String])> {
        self.log.as_ref().map(|log| (log.id.as_str(), log.lines.as_slice()))
    }

    pub(super) fn widget(&mut self, ui: &mut Ui, p: &IdePalette, right: f32, y: f32) -> (f32, Option<ServerAction>) {
        self.hits.clear();
        let shown = self.shown();
        if shown.is_empty() {
            self.popup = false;
            return (right, None);
        }
        let worst = shown.iter().map(|server| severity(server.shown_state())).max().unwrap_or(0);
        let state = shown
            .iter()
            .find(|server| severity(server.shown_state()) == worst)
            .map_or("", |server| server.shown_state());
        let label = match shown.as_slice() {
            [single] => match single.work() {
                Some(work) => t!("ide-lsp-widget-busy", server = single.name.as_str(), work = work),
                None => single.name.clone(),
            },
            _ => t!("ide-lsp-widget-count", count = shown.len()),
        };
        let color = state_color(p, state);
        let galley = ui.painter().layout_no_wrap(label, theme::regular(12.5), p.text_secondary);
        let width = galley.size().x + 40.0;
        let rect = Rect::from_min_size(pos2(right - width, y - 10.0), vec2(width, 20.0));
        let response = ui.interact(rect, ui.scope_id().with("lsp-widget"), Sense::click());
        hover(ui, &response, p);
        if !self.popup {
            hint(ui, &response, &t!("ide-lsp-title"), None, Side::Left);
        }
        crate::accessibility::button(&response, galley.text());
        crate::accessibility::edit(&response, |node| node.set_expanded(self.popup));
        glyph_braces(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 12.0, y), vec2(12.0, 11.0)),
            p.text_secondary,
        );
        ui.painter().circle_filled(pos2(rect.left() + 24.0, y), 3.5, color);
        ui.painter().galley(pos2(rect.left() + 32.0, y - galley.size().y / 2.0), galley, p.text_secondary);
        self.hits.push((String::new(), "widget", rect));
        if response.clicked() {
            self.popup = !self.popup;
        }
        let action = if self.popup { self.popup(ui, p, rect) } else { None };
        (rect.left(), action)
    }

    fn popup(&mut self, ui: &Ui, p: &IdePalette, anchor: Rect) -> Option<ServerAction> {
        let servers: Vec<ServerInfo> = self.shown().into_iter().cloned().collect();
        let screen = ui.ctx().content_rect();
        let left = (anchor.right() - POPUP_WIDTH).clamp(screen.left() + 4.0, screen.right() - POPUP_WIDTH - 4.0);
        let mut chosen = None;
        let mut hits = Vec::new();
        let area = egui::Area::new(ui.scope_id().with("lsp-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(left, anchor.top() - 6.0))
            .pivot(Align2::LEFT_BOTTOM)
            .show(ui.ctx(), |ui| {
                super::doc_popups::popup_frame(p).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                    ui.set_width(POPUP_WIDTH - 12.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let (title, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                    ui.painter().text(
                        pos2(title.left() + 8.0, title.center().y),
                        Align2::LEFT_CENTER,
                        t!("ide-lsp-title"),
                        theme::semibold(13.0),
                        p.text,
                    );
                    for server in &servers {
                        if let Some(action) = server_row(ui, p, server, &mut hits) {
                            chosen = Some(action);
                        }
                    }
                    ui.add_space(4.0);
                    let (line, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                    ui.painter().hline(line.x_range(), line.center().y, Stroke::new(1.0, p.tab_border));
                    ui.add_space(4.0);
                    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    crate::accessibility::name_with(&response, egui::Role::MenuItem, || t!("ide-lsp-settings"));
                    if response.hovered() {
                        ui.painter().rect_filled(row, 4, p.selection);
                    }
                    ui.painter().text(
                        pos2(row.left() + 8.0, row.center().y),
                        Align2::LEFT_CENTER,
                        t!("ide-lsp-settings"),
                        theme::regular(13.0),
                        p.text,
                    );
                    hits.push((String::new(), "settings", row));
                    if response.clicked() {
                        chosen = Some(ServerAction::OpenSettings);
                    }
                });
            });
        self.hits.extend(hits);
        let closing = ui.input(|input| {
            input.key_pressed(Key::Escape)
                || (input.pointer.any_pressed()
                    && input
                        .pointer
                        .interact_pos()
                        .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos)))
        });
        if closing || chosen.is_some() {
            self.popup = false;
        }
        chosen
    }

    pub(super) fn log_window(&mut self, ui: &mut Ui, p: &IdePalette, now: f64, dispatch: &mut Dispatch<'_>) {
        let Some(log) = self.log.as_mut() else { return };
        if log.polled.is_none_or(|at| now - at >= POLL_SECONDS) {
            log.polled = Some(now);
            let chunk = dispatch("lsp.server.log", json!({ "id": log.id, "since": log.next }))
                .ok()
                .and_then(|chunk| serde_json::from_value::<LogChunk>(chunk).ok());
            if let Some(chunk) = chunk {
                log.lines.extend(chunk.lines);
                log.next = chunk.next.max(log.next);
                let excess = log.lines.len().saturating_sub(LOG_LIMIT);
                log.lines.drain(..excess);
            }
        }
        let mut close = false;
        let modal =
            egui::Modal::new(ui.scope_id().with("lsp-log")).frame(super::dialog_frame(p)).show(ui.ctx(), |ui| {
                let screen = ui.ctx().content_rect();
                ui.set_width((screen.width() - 120.0).clamp(420.0, 860.0));
                ui.label(
                    egui::RichText::new(t!("ide-lsp-log-title", name = log.name.as_str()))
                        .font(theme::semibold(14.0))
                        .color(p.text),
                );
                ui.add_space(10.0);
                egui::Frame::new().fill(p.editor_background).corner_radius(6).inner_margin(8).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    egui::ScrollArea::vertical()
                        .max_height((screen.height() - 220.0).clamp(160.0, 520.0))
                        .stick_to_bottom(true)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if log.lines.is_empty() {
                                ui.label(egui::RichText::new(t!("ide-lsp-log-empty")).color(p.text_secondary));
                            }
                            for line in &log.lines {
                                ui.label(egui::RichText::new(line).font(egui::FontId::monospace(12.0)).color(p.code));
                            }
                        });
                });
                ui.add_space(12.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if super::vcs::primary_button(ui, p, &t!("ide-lsp-close")).clicked() {
                        close = true;
                    }
                });
            });
        if close || modal.should_close() {
            self.log = None;
        }
    }
}

fn server_row(
    ui: &mut Ui,
    p: &IdePalette,
    server: &ServerInfo,
    hits: &mut Vec<(String, &'static str, Rect)>,
) -> Option<ServerAction> {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.popup_hover);
    }
    let top = rect.top() + 13.0;
    ui.painter().circle_filled(pos2(rect.left() + 12.0, top), 4.0, state_color(p, server.shown_state()));
    let name = ui.painter().text(
        pos2(rect.left() + 24.0, top),
        Align2::LEFT_CENTER,
        &server.name,
        theme::semibold(13.0),
        p.text,
    );
    let mut details = vec![state_label(server.shown_state())];
    details.extend(server.version.clone());
    if matches!(server.source.as_str(), "local" | "path" | "custom" | "project") {
        details.push(source_label(&server.source));
    }
    if server.files > 0 {
        details.push(t!("ide-lsp-files", count = server.files));
    }
    let details = details.join(" · ");
    crate::accessibility::label(ui, ui.scope_id().with(("lsp-server", &server.id)), rect, || {
        format!("{}, {details}", server.name)
    });
    ui.painter().with_clip_rect(rect).text(
        pos2(name.right() + 10.0, top),
        Align2::LEFT_CENTER,
        details,
        theme::regular(12.5),
        p.text_secondary,
    );
    let bottom = rect.bottom() - 13.0;
    let mut x = rect.left() + 24.0;
    let mut actions: Vec<(&'static str, String, ServerAction)> = Vec::new();
    if server.offers_install() {
        actions.push(("install", t!("ide-lsp-install"), ServerAction::Install(server.id.clone())));
    }
    if !server.is_missing() {
        actions.push(("restart", t!("ide-lsp-restart"), ServerAction::Restart(server.id.clone())));
    }
    if matches!(server.state.as_str(), "running" | "starting") {
        actions.push(("stop", t!("ide-lsp-stop"), ServerAction::Stop(server.id.clone())));
    }
    actions.push(("log", t!("ide-lsp-show-log"), ServerAction::ShowLog(server.id.clone())));
    let mut chosen = None;
    for (name, label, action) in actions {
        let galley = ui.painter().layout_no_wrap(label, theme::regular(12.5), p.link);
        let link = Rect::from_min_size(pos2(x, bottom - galley.size().y / 2.0), galley.size());
        let response = ui
            .interact(link, ui.scope_id().with(("lsp-row", &server.id, name)), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        crate::accessibility::name(&response, egui::Role::Link, galley.text());
        if response.hovered() {
            ui.painter().line_segment([link.left_bottom(), link.right_bottom()], Stroke::new(1.0, p.link));
        }
        ui.painter().galley(link.min, galley, p.link);
        hits.push((server.id.clone(), name, link));
        if response.clicked() {
            chosen = Some(action);
        }
        x = link.right() + 16.0;
    }
    chosen
}

pub(super) fn source_label(source: &str) -> String {
    match source {
        "local" => t!("settings-lsp-source-local"),
        "path" => t!("settings-lsp-source-path"),
        "custom" => t!("settings-lsp-source-custom"),
        "project" => t!("settings-lsp-source-project"),
        _ => "—".to_owned(),
    }
}

fn severity(state: &str) -> u8 {
    match state {
        "failed" => 4,
        "missing" => 3,
        "starting" | "installing" | "busy" => 2,
        "running" => 1,
        _ => 0,
    }
}

fn state_color(p: &IdePalette, state: &str) -> Color32 {
    match state {
        "running" => p.run,
        "starting" | "installing" | "busy" => p.problem("warning"),
        "failed" => p.problem("error"),
        "missing" => p.warning_text,
        _ => p.text_secondary,
    }
}

pub(super) fn state_label(state: &str) -> String {
    match state {
        "running" => t!("ide-lsp-state-running"),
        "starting" => t!("ide-lsp-state-starting"),
        "failed" => t!("ide-lsp-state-failed"),
        "stopped" => t!("ide-lsp-state-stopped"),
        "missing" => t!("ide-lsp-state-missing"),
        "installing" => t!("ide-lsp-state-installing"),
        "busy" => t!("ide-lsp-state-busy"),
        "disabled" => t!("ide-lsp-state-disabled"),
        _ => t!("ide-lsp-state-not-started"),
    }
}

fn glyph_braces(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.2, color);
    let (top, middle, bottom) = (rect.top(), rect.center().y, rect.bottom());
    for (outer, inner) in [(rect.left() + 1.0, rect.left() + 3.5), (rect.right() - 1.0, rect.right() - 3.5)] {
        painter.line_segment([pos2(inner, top), pos2((outer + inner) / 2.0, top + 1.5)], stroke);
        painter
            .line_segment([pos2((outer + inner) / 2.0, top + 1.5), pos2((outer + inner) / 2.0, middle - 1.0)], stroke);
        painter.line_segment([pos2((outer + inner) / 2.0, middle - 1.0), pos2(outer, middle)], stroke);
        painter.line_segment([pos2(outer, middle), pos2((outer + inner) / 2.0, middle + 1.0)], stroke);
        painter.line_segment(
            [pos2((outer + inner) / 2.0, middle + 1.0), pos2((outer + inner) / 2.0, bottom - 1.5)],
            stroke,
        );
        painter.line_segment([pos2((outer + inner) / 2.0, bottom - 1.5), pos2(inner, bottom)], stroke);
    }
}

fn glyph_cross(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.3, color);
    painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
    painter.line_segment([rect.right_top(), rect.left_bottom()], stroke);
}

pub(super) fn split_args(text: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut started = false;
    for ch in text.chars() {
        match (quote, ch) {
            (Some(open), _) if ch == open => quote = None,
            (Some(_), _) => current.push(ch),
            (None, '"' | '\'') => {
                quote = Some(ch);
                started = true;
            }
            (None, _) if ch.is_whitespace() => {
                if started {
                    args.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (None, _) => {
                current.push(ch);
                started = true;
            }
        }
    }
    if started {
        args.push(current);
    }
    args
}

pub(super) fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| if arg.is_empty() || arg.contains(char::is_whitespace) { format!("\"{arg}\"") } else { arg.clone() })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Default)]
pub(super) struct ServersPage {
    selected: Option<String>,
    path: String,
    args: String,
    shown: (Option<String>, Vec<String>),
    applied: bool,
}

impl ServersPage {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        servers: Option<&[ServerInfo]>,
    ) -> Option<(&'static str, Value)> {
        let Some(servers) = servers else {
            ui.label(egui::RichText::new(t!("settings-lsp-no-project")).color(p.text_secondary));
            return None;
        };
        if servers.is_empty() {
            ui.label(egui::RichText::new(t!("settings-lsp-none")).color(p.text_secondary));
            return None;
        }
        match servers.iter().find(|server| self.selected.as_deref() == Some(server.id.as_str())) {
            None => self.select(&servers[0]),
            Some(server)
                if self.shown != (server.path.clone(), server.args.clone()) && (self.applied || !self.edited()) =>
            {
                self.select(server)
            }
            Some(_) => {}
        }
        let mut request = None;
        let selected = self.selected.as_deref();
        if let Some(index) = servers_table(ui, p, servers, selected)
            && selected != Some(servers[index].id.as_str())
        {
            self.select(&servers[index]);
        }
        let server = servers.iter().find(|server| self.selected.as_deref() == Some(server.id.as_str()))?;
        ui.add_space(14.0);
        ui.label(egui::RichText::new(&server.name).font(theme::semibold(13.0)).color(p.text));
        ui.add_space(6.0);
        if server.alternatives.len() > 1 {
            let language = server.languages.first().cloned().unwrap_or_default();
            let mut letters = language.chars();
            let language: String =
                letters.next().map(|first| first.to_uppercase().chain(letters).collect()).unwrap_or_default();
            let name_of = |id: &str| {
                servers.iter().find(|other| other.id == id).map_or(id.to_owned(), |other| other.name.clone())
            };
            let chosen = server.alternatives[0].clone();
            ui.horizontal(|ui| {
                let label = ui
                    .label(egui::RichText::new(t!("settings-lsp-chosen", language = language.as_str())).color(p.text));
                let combo = egui::ComboBox::from_id_salt(("settings-lsp-choice", language.as_str()))
                    .selected_text(name_of(&chosen))
                    .show_ui(ui, |ui| {
                        for id in &server.alternatives {
                            if ui.selectable_label(*id == chosen, name_of(id)).clicked() && *id != chosen {
                                request = Some(("lsp.server.choose", json!({ "id": id })));
                            }
                        }
                    });
                combo.response.labelled_by(label.id);
            });
            ui.add_space(6.0);
        }
        let mut enabled = server.enabled;
        if ui.checkbox(&mut enabled, t!("settings-lsp-enabled")).changed() {
            request = Some(("lsp.server.configure", json!({ "id": server.id, "enabled": enabled })));
        }
        ui.add_space(6.0);
        if let Some(notice) = server.notice.as_deref().filter(|notice| !notice.is_empty()) {
            ui.label(egui::RichText::new(notice).color(p.text_secondary));
            if let Some(url) = server.notice_url.as_deref()
                && ui.link(t!("ide-lsp-notice-link")).clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            ui.add_space(6.0);
        }
        if let Some(message) = server.message.as_deref().filter(|message| !message.is_empty()) {
            let color = if server.state == "failed" { p.error_text } else { p.text_secondary };
            ui.label(egui::RichText::new(message).color(color));
            if let Some((label, url)) = server.download()
                && ui.link(label).clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            ui.add_space(6.0);
        }
        egui::Grid::new("settings-lsp-edit").num_columns(2).spacing(vec2(12.0, 8.0)).show(ui, |ui| {
            let label = ui.label(egui::RichText::new(t!("settings-lsp-path")).color(p.text));
            ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text(t!("settings-lsp-path-hint"))
                    .desired_width(ui.available_width().min(460.0)),
            )
            .labelled_by(label.id);
            ui.end_row();
            let label = ui.label(egui::RichText::new(t!("settings-lsp-arguments")).color(p.text));
            ui.add(egui::TextEdit::singleline(&mut self.args).desired_width(ui.available_width().min(460.0)))
                .labelled_by(label.id);
            ui.end_row();
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let configure = self.configure(server);
            if ui.add_enabled(configure.is_some(), egui::Button::new(t!("settings-lsp-apply"))).clicked() {
                self.applied = true;
                request = configure;
            }
            if server.is_missing()
                && server.installable
                && !server.installing
                && ui.button(t!("ide-lsp-install")).clicked()
            {
                request = Some(("lsp.server.install", json!({ "id": server.id })));
            }
            if !server.is_missing() && ui.button(t!("ide-lsp-restart")).clicked() {
                request = Some(("lsp.server.restart", json!({ "id": server.id })));
            }
            if server.dismissed && ui.button(t!("settings-lsp-undismiss")).clicked() {
                request = Some(("lsp.server.undismiss", json!({ "id": server.id })));
            }
        });
        ui.add_space(8.0);
        ui.label(egui::RichText::new(t!("settings-lsp-note")).color(p.text_secondary));
        request
    }

    fn configure(&self, server: &ServerInfo) -> Option<(&'static str, Value)> {
        let mut params = json!({ "id": server.id });
        let path = self.path.trim();
        if path != server.path.as_deref().unwrap_or_default() {
            params["path"] = json!(path);
        }
        let args = split_args(&self.args);
        if args != server.args {
            params["args"] = json!(args);
        }
        (params.as_object().is_some_and(|params| params.len() > 1)).then_some(("lsp.server.configure", params))
    }

    fn edited(&self) -> bool {
        self.path.trim() != self.shown.0.as_deref().unwrap_or_default() || split_args(&self.args) != self.shown.1
    }

    fn select(&mut self, server: &ServerInfo) {
        self.selected = Some(server.id.clone());
        self.shown = (server.path.clone(), server.args.clone());
        self.applied = false;
        self.path = server.path.clone().unwrap_or_default();
        self.args = join_args(&server.args);
    }
}

const TABLE_COLUMNS: [(&str, f32); 6] = [
    ("settings-lsp-name", 72.0),
    ("settings-lsp-state", 84.0),
    ("settings-lsp-version", 52.0),
    ("settings-lsp-source", 70.0),
    ("settings-lsp-path", 90.0),
    ("settings-lsp-arguments", 56.0),
];
const TABLE_GAP: f32 = 16.0;
const TABLE_ROW: f32 = 24.0;
const TABLE_PAD: f32 = 6.0;
const STATE_DOT: f32 = 14.0;

pub(super) fn column_widths(natural: &[f32], least: &[f32], available: f32) -> (Vec<f32>, bool) {
    let floor: Vec<f32> = natural.iter().zip(least).map(|(natural, least)| natural.min(*least)).collect();
    let width = |cap: f32| -> Vec<f32> {
        natural.iter().zip(&floor).map(|(natural, floor)| natural.min(cap.max(*floor))).collect()
    };
    if natural.iter().sum::<f32>() <= available {
        return (natural.to_vec(), false);
    }
    if floor.iter().sum::<f32>() > available {
        return (floor, true);
    }
    let (mut low, mut high) = (0.0_f32, natural.iter().copied().fold(0.0, f32::max));
    for _ in 0..40 {
        let middle = (low + high) / 2.0;
        if width(middle).iter().sum::<f32>() <= available {
            low = middle;
        } else {
            high = middle;
        }
    }
    (width(low), false)
}

fn cut(ui: &Ui, text: &str, font: egui::FontId, color: Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    ui.painter().layout_job(job)
}

fn servers_table(ui: &mut Ui, p: &IdePalette, servers: &[ServerInfo], selected: Option<&str>) -> Option<usize> {
    let font = theme::regular(13.0);
    let header_font = theme::semibold(12.5);
    let rows: Vec<[String; 6]> = servers
        .iter()
        .map(|server| {
            [
                server.name.clone(),
                state_label(server.shown_state()),
                server.version.clone().unwrap_or_else(|| "—".to_owned()),
                source_label(&server.source),
                server.path.clone().unwrap_or_else(|| "—".to_owned()),
                join_args(&server.args),
            ]
        })
        .collect();
    let measure = |text: &str, font: egui::FontId| ui.painter().layout_no_wrap(text.to_owned(), font, p.text).size().x;
    let natural: Vec<f32> = TABLE_COLUMNS
        .iter()
        .enumerate()
        .map(|(column, (key, _))| {
            let dot = if column == 1 { STATE_DOT } else { 0.0 };
            rows.iter()
                .map(|row| measure(&row[column], font.clone()) + dot)
                .fold(measure(&t!(*key), header_font.clone()), f32::max)
                .ceil()
        })
        .collect();
    let least: Vec<f32> = TABLE_COLUMNS.iter().map(|(_, least)| *least).collect();
    let gaps = TABLE_GAP * (TABLE_COLUMNS.len() - 1) as f32 + 2.0 * TABLE_PAD;
    let mut clicked = None;
    egui::Frame::new().stroke(Stroke::new(1.0, p.tab_border)).corner_radius(6).inner_margin(4).show(ui, |ui| {
        let inner = ui.available_width();
        ui.set_width(inner);
        let (widths, scroll) = column_widths(&natural, &least, inner - gaps);
        let total = (widths.iter().sum::<f32>() + gaps).max(inner);
        let mut draw = |ui: &mut Ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let (header, _) = ui.allocate_exact_size(vec2(total, TABLE_ROW), Sense::hover());
            let mut x = header.left() + TABLE_PAD;
            for ((key, _), width) in TABLE_COLUMNS.iter().zip(&widths) {
                let galley = cut(ui, &t!(*key), header_font.clone(), p.text_secondary, *width);
                ui.painter().galley(pos2(x, header.center().y - galley.size().y / 2.0), galley, p.text_secondary);
                x += width + TABLE_GAP;
            }
            for (index, (server, row)) in servers.iter().zip(&rows).enumerate() {
                let (rect, response) = ui.allocate_exact_size(vec2(total, TABLE_ROW), Sense::click());
                crate::accessibility::item(
                    &response,
                    egui::Role::ListBoxOption,
                    selected == Some(server.id.as_str()),
                    &server.name,
                );
                if selected == Some(server.id.as_str()) {
                    ui.painter().rect_filled(rect, 4, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect, 4, p.hover);
                }
                let pointer = response.hover_pos();
                let mut full = None;
                let mut x = rect.left() + TABLE_PAD;
                for (column, (text, width)) in row.iter().zip(&widths).enumerate() {
                    let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(*width, rect.height()));
                    let mut left = x;
                    let mut room = *width;
                    if column == 1 {
                        ui.painter().circle_filled(
                            pos2(left + 4.0, rect.center().y),
                            3.5,
                            state_color(p, server.shown_state()),
                        );
                        left += STATE_DOT;
                        room -= STATE_DOT;
                    }
                    let color = if matches!(column, 3..=5) { p.text_secondary } else { p.text };
                    let galley = cut(ui, text, font.clone(), color, room);
                    if galley.elided && pointer.is_some_and(|pointer| cell.contains(pointer)) {
                        full = Some(text.clone());
                    }
                    ui.painter().galley(pos2(left, rect.center().y - galley.size().y / 2.0), galley, color);
                    x += width + TABLE_GAP;
                }
                if response.clicked() {
                    clicked = Some(index);
                }
                if let Some(full) = full {
                    response.on_hover_text(full);
                }
            }
        };
        if scroll {
            egui::ScrollArea::horizontal().id_salt("settings-lsp-table").show(ui, draw);
        } else {
            draw(ui);
        }
    });
    clicked
}

fn dispatcher<'a>(
    registry: &'a Registry,
    session: &'a mut Session,
) -> impl FnMut(&str, Value) -> Result<Value, String> + 'a {
    move |id: &str, params| {
        registry.dispatch(session, id, params).map_err(|error| match error {
            zen_core::DispatchError::Failed { error, .. } => error.message,
            other => other.to_string(),
        })
    }
}

impl IdeView {
    pub(super) fn poll_language_servers(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        now: f64,
        focused: bool,
    ) {
        if self.lsp.poll(now, focused, &mut dispatcher(registry, session)) {
            ctx.request_repaint();
        }
        for finished in self.lsp.take_finished() {
            let kind = if finished.failed { Kind::Error } else { Kind::Info };
            self.notify(NewNotification::message(Group::LanguageServers, kind, finished.text));
        }
        if focused && self.lsp.is_busy() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    pub(super) fn language_server_widget(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        right: f32,
        y: f32,
    ) -> f32 {
        let p = self.palette;
        let (left, action) = self.lsp.widget(ui, &p, right, y);
        if let Some(action) = action {
            self.language_server_action(ui, registry, session, action);
        }
        left
    }

    pub(super) fn language_server_action(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        action: ServerAction,
    ) {
        match action {
            ServerAction::OpenSettings => {
                self.settings.open_language_servers();
                return;
            }
            ServerAction::Download(url) => {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                return;
            }
            _ => {}
        }
        if let Err(message) = self.lsp.act(action, &mut dispatcher(registry, session)) {
            self.post(ui, message);
        }
    }

    pub(super) fn language_server_banner(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        name: &str,
    ) {
        let now = ui.input(|input| input.time);
        let Some(banner) = self.lsp.banner(name, now) else { return };
        if !banner.dismiss {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        self.log_banner(&banner);
        let p = self.palette;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::BANNER), Sense::hover());
        let band =
            Rect::from_min_max(pos2(rect.left() + 6.0, rect.top()), pos2(rect.right() - 6.0, rect.bottom() - 4.0));
        ui.painter().rect_filled(band, 6, p.banner);
        let y = band.center().y;
        let text = ui.painter().with_clip_rect(band.shrink(4.0)).text(
            pos2(band.left() + 14.0, y),
            Align2::LEFT_CENTER,
            &banner.text,
            theme::regular(13.0),
            p.text,
        );
        crate::accessibility::announce(ui, ui.scope_id().with("lsp-banner"), text, || banner.text.clone());
        let mut x = text.right() + 20.0;
        let mut action = None;
        if banner.install && self.link(ui, &mut x, y, &t!("ide-lsp-install"), "lsp-install") {
            action = Some(ServerAction::Install(banner.id.clone()));
        }
        if let Some((label, url)) = &banner.download
            && self.link(ui, &mut x, y, label, "lsp-download")
        {
            action = Some(ServerAction::Download(url.clone()));
        }
        if let Some(version) = banner.jdk
            && self.link(ui, &mut x, y, &t!("ide-lsp-download-jdk", version = version), "lsp-download-jdk")
        {
            action = Some(ServerAction::DownloadJdk(banner.id.clone(), version));
        }
        if banner.dismiss && self.link(ui, &mut x, y, &t!("ide-lsp-dont-ask"), "lsp-dismiss") {
            action = Some(ServerAction::Dismiss(banner.id.clone()));
        }
        let close = Rect::from_center_size(pos2(band.right() - 18.0, y), Vec2::splat(22.0));
        let response = ui.interact(close, ui.scope_id().with("lsp-banner-close"), Sense::click());
        hover(ui, &response, &p);
        hint(ui, &response, &t!("ide-lsp-close"), None, Side::Left);
        glyph_cross(ui.painter(), close.shrink(7.0), p.text_secondary);
        if response.clicked() {
            action = Some(ServerAction::Close(banner.id));
        }
        if let Some(action) = action {
            self.language_server_action(ui, registry, session, action);
        }
    }

    fn log_banner(&mut self, banner: &Banner) {
        let installing = self.lsp.server(&banner.id).is_some_and(|server| server.installing);
        let (key, kind) = match (banner.dismiss, installing) {
            (true, _) => (format!("lsp-missing:{}", banner.id), Kind::Warning),
            (false, false) => (format!("lsp-installed:{}", banner.text), Kind::Info),
            (false, true) => return,
        };
        let mut logged = NewNotification::message(Group::LanguageServers, kind, banner.text.clone()).silent();
        if let Some(version) = banner.jdk {
            logged = logged.action(Action::new(
                "download-jdk",
                t!("ide-lsp-download-jdk", version = version),
                "toolchain.jdk.download",
                json!({ "version": version }),
            ));
        }
        if banner.install {
            logged = logged.action(
                Action::new("install", t!("ide-lsp-install"), "lsp.server.install", json!({ "id": banner.id })).once(),
            );
        }
        if banner.dismiss {
            logged = logged.action(
                Action::new("dismiss", t!("ide-lsp-dont-ask"), "lsp.server.dismiss", json!({ "id": banner.id })).once(),
            );
        }
        self.notifications.queue_once(key, logged);
    }

    pub(super) fn language_server_log(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let now = ui.input(|input| input.time);
        self.lsp.log_window(ui, &p, now, &mut dispatcher(registry, session));
    }
}

fn install_finished(server: &ServerInfo, installed: bool) -> InstallFinished {
    if installed {
        let version = server.version.as_deref().unwrap_or_default();
        let text = t!("ide-lsp-installed", name = server.name.as_str(), version = version);
        return InstallFinished { text, failed: false };
    }
    let detail = server.message.as_deref().unwrap_or_default();
    InstallFinished { text: t!("ide-lsp-install-failed", name = server.name.as_str(), detail = detail), failed: true }
}

#[cfg(test)]
mod tests;

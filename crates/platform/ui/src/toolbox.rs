use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use zen_core::{Registry, Session};
use zen_i18n::{LANGUAGES, t};
use zen_toolbox::markdown::Block;
use zen_toolbox::{
    Job, ReleaseNote, ReleaseNotesParams, SelfUpdate, ServerStatus, Snapshot, Tool, ToolState, ToolVersion, Tools,
};

use crate::components::{self, IconButton, hover_fill, lerp};
use crate::theme::{self, Palette};
use crate::{accessibility, icons};

mod header;
mod row;
mod settings;

mod metrics {
    pub(super) const MARGIN: f32 = 12.0;
    pub(super) const WINDOW_RADIUS: f32 = 12.0;
    pub(super) const HEADER: f32 = 66.0;
    pub(super) const TABS: f32 = 42.0;
    pub(super) const PANEL_RADIUS: u8 = 12;
    pub(super) const PANEL_PADDING: i8 = 6;
    pub(super) const ROW: f32 = 76.0;
    pub(super) const ROW_COMPACT: f32 = 60.0;
    pub(super) const MARK: f32 = 42.0;
    pub(super) const DIALOG_WIDTH: f32 = 340.0;
    pub(super) const DIALOG_NOTES_HEIGHT: f32 = 460.0;
}

const TOOLBOX_NAME: &str = "Zen Toolbox";
const KOZO_SIDE: f32 = 80.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tab {
    Tools,
    Projects,
}

pub struct ToolboxView {
    palette: Palette,
    tab: Tab,
    search_open: bool,
    query: String,
    available_open: bool,
    notice: Option<(String, f64)>,
    confirm_uninstall: Option<(String, String)>,
    versions: Option<VersionsDialog>,
    whats_new: Option<WhatsNew>,
    own_notes_pending: Option<Option<String>>,
    settings: Option<settings::SettingsPage>,
    licences: Option<crate::licences::LicencesView>,
}

struct VersionsDialog {
    id: String,
    tool: String,
    versions: Vec<ToolVersion>,
}

struct WhatsNew {
    id: String,
    tool: String,
    releases: Result<Vec<(ReleaseNote, Vec<Block>)>, String>,
}

impl WhatsNew {
    fn new(id: &str, tool: &str, releases: Result<Vec<ReleaseNote>, String>) -> Self {
        let releases = releases.map(|notes| {
            notes
                .into_iter()
                .map(|note| {
                    let blocks = zen_toolbox::markdown::parse(&note.notes);
                    (note, blocks)
                })
                .collect()
        });
        Self { id: id.to_owned(), tool: tool.to_owned(), releases }
    }
}

fn changelog_url(id: &str) -> String {
    format!("https://ide.ifzenelse.space/changelog.html#{id}")
}

impl Default for ToolboxView {
    fn default() -> Self {
        Self {
            palette: Palette::DARK,
            tab: Tab::Tools,
            search_open: false,
            query: String::new(),
            available_open: true,
            notice: None,
            confirm_uninstall: None,
            versions: None,
            whats_new: None,
            own_notes_pending: None,
            settings: None,
            licences: None,
        }
    }
}

const NOTICE_SECONDS: f64 = 5.0;

impl ToolboxView {
    pub fn new(palette: Palette) -> Self {
        Self { palette, ..Self::default() }
    }

    pub fn show_own_notes(&mut self, newer_than: Option<String>) {
        self.own_notes_pending = Some(newer_than);
    }

    fn own_notes(&mut self, session: &mut Session) {
        let Some(newer_than) = &self.own_notes_pending else { return };
        let Some(tools) = session.state::<Tools>() else { return };
        let Some(notes) = tools.own_notes(newer_than.as_deref()) else { return };
        let version = tools.self_version();
        self.own_notes_pending = None;
        if !notes.is_empty() {
            self.whats_new = Some(WhatsNew::new("toolbox", TOOLBOX_NAME, Ok(notes)));
        }
        let _ = session.settings_mut().set(zen_toolbox::WHATS_NEW_SETTING, version);
    }

    fn open_whats_new(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        id: &str,
        name: &str,
        newer_than: Option<String>,
    ) {
        let params = ReleaseNotesParams { id: id.to_owned(), newer_than };
        let releases = registry.run::<zen_toolbox::ReleaseNotes>(session, params).map_err(error_message);
        self.whats_new = Some(WhatsNew::new(id, name, releases));
    }

    pub fn has_dialog(&self) -> bool {
        self.whats_new.is_some() || self.settings.is_some() || self.versions.is_some() || self.licences.is_some()
    }

    fn whats_new(&mut self, ui: &mut Ui) {
        let Some(dialog) = &self.whats_new else { return };
        let p = self.palette;
        let mut close = false;
        let modal = egui::Modal::new(ui.scope_id().with("whats-new")).show(ui.ctx(), |ui| {
            ui.set_width(metrics::DIALOG_WIDTH);
            ui.label(
                egui::RichText::new(t!("toolbox-whats-new-title", tool = dialog.tool.as_str()))
                    .font(theme::semibold(15.0))
                    .color(p.text),
            );
            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height(metrics::DIALOG_NOTES_HEIGHT).auto_shrink([false, true]).show(
                ui,
                |ui| {
                    let secondary = |ui: &mut Ui, text: String| {
                        ui.label(egui::RichText::new(text).font(theme::regular(13.0)).color(p.text_secondary));
                    };
                    match &dialog.releases {
                        Err(message) => secondary(ui, message.clone()),
                        Ok(releases) if releases.is_empty() => secondary(ui, t!("toolbox-whats-new-none")),
                        Ok(releases) => {
                            for (index, (note, blocks)) in releases.iter().enumerate() {
                                if index > 0 {
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(4.0);
                                }
                                release_heading(ui, note, p);
                                ui.add_space(4.0);
                                if blocks.is_empty() {
                                    ui.label(
                                        egui::RichText::new(t!("toolbox-whats-new-empty"))
                                            .font(theme::regular(13.0))
                                            .color(p.text_muted),
                                    );
                                } else {
                                    crate::markdown::show(ui, blocks, p);
                                }
                            }
                        }
                    }
                },
            );
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t!("toolbox-whats-new-close")).clicked() {
                    close = true;
                }
                if ui.link(t!("toolbox-whats-new-full")).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(changelog_url(&dialog.id)));
                }
            });
        });
        if close || modal.should_close() {
            self.whats_new = None;
        }
    }

    pub fn show(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let window = ui.max_rect();
        let snapshot = session.state::<Tools>().map(Tools::snapshot).unwrap_or_default();
        if let Some(tools) = session.state::<Tools>() {
            for message in tools.take_messages() {
                self.post(ui, message);
            }
            if tools.busy() {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        let [top_left, top_right, bottom_left, bottom_right] = p.window;
        icons::fill_rounded(ui.painter(), window, metrics::WINDOW_RADIUS, |at| {
            lerp(lerp(top_left, top_right, at.x), lerp(bottom_left, bottom_right, at.x), at.y)
        });
        ui.painter().rect_stroke(window, metrics::WINDOW_RADIUS, Stroke::new(1.0, p.window_edge), StrokeKind::Inside);

        let content = window.shrink2(vec2(metrics::MARGIN, 0.0));
        let header = Rect::from_min_size(content.min, vec2(content.width(), metrics::HEADER));
        self.header(ui, header, registry, session, &snapshot);

        if self.settings.is_some() {
            let body = Rect::from_min_max(header.left_bottom(), content.max - vec2(0.0, metrics::MARGIN));
            self.settings_page(ui, body, registry, session);
            self.notice(ui, window);
            return;
        }

        let tabs = Rect::from_min_size(header.left_bottom(), vec2(content.width(), metrics::TABS));
        self.tabs(ui, tabs, snapshot.updates());

        let mut body =
            Rect::from_min_max(tabs.left_bottom() + vec2(0.0, 8.0), content.max - vec2(0.0, metrics::MARGIN));
        if self.search_open {
            let field = Rect::from_min_size(body.min, vec2(body.width(), components::metrics::TEXT_FIELD));
            self.search_field(ui, field);
            body.min.y = field.bottom() + 8.0;
        }

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut child, |ui| match self.tab {
            Tab::Tools => self.tools_tab(ui, registry, session, &snapshot),
            Tab::Projects => self.projects_tab(ui),
        });

        self.confirm_uninstall(ui, registry, session);
        self.versions_dialog(ui, registry, session);
        self.own_notes(session);
        self.whats_new(ui);
        self.notice(ui, window);
    }

    fn confirm_uninstall(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some((id, name)) = self.confirm_uninstall.clone() else { return };
        let p = self.palette;
        let mut decision = None;
        egui::Modal::new(ui.scope_id().with("confirm-uninstall")).show(ui.ctx(), |ui| {
            ui.set_width(280.0);
            ui.label(
                egui::RichText::new(t!("toolbox-uninstall-title", tool = name.as_str()))
                    .font(theme::semibold(15.0))
                    .color(p.text),
            );
            ui.add_space(6.0);
            ui.label(egui::RichText::new(t!("toolbox-uninstall-detail")).color(p.text_secondary));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button(t!("toolbox-cancel")).clicked() {
                    decision = Some(false);
                }
                if ui.button(t!("toolbox-uninstall")).clicked() {
                    decision = Some(true);
                }
            });
        });
        match decision {
            Some(true) => {
                self.confirm_uninstall = None;
                self.dispatch(ui, registry, session, "toolbox.tool.uninstall", serde_json::json!({ "id": id }));
            }
            Some(false) => self.confirm_uninstall = None,
            None => {}
        }
    }

    fn open_versions(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, tool: &Tool) {
        let id = serde_json::json!({ "id": tool.id });
        let listed = registry
            .dispatch(session, "toolbox.tool.versions", id)
            .map_err(error_message)
            .and_then(|value| serde_json::from_value::<Vec<ToolVersion>>(value).map_err(|error| error.to_string()));
        match listed {
            Ok(versions) => {
                self.versions = Some(VersionsDialog { id: tool.id.clone(), tool: tool.name.clone(), versions });
            }
            Err(error) => self.post(ui, error),
        }
    }

    fn versions_dialog(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(dialog) = &self.versions else { return };
        let p = self.palette;
        let mut close = false;
        let mut install = None;
        egui::Modal::new(ui.scope_id().with("tool-versions")).show(ui.ctx(), |ui| {
            ui.set_width(320.0);
            ui.label(
                egui::RichText::new(t!("toolbox-versions-title", tool = dialog.tool.as_str()))
                    .font(theme::semibold(15.0))
                    .color(p.text),
            );
            ui.add_space(4.0);
            ui.label(egui::RichText::new(t!("toolbox-versions-detail")).color(p.text_secondary));
            ui.add_space(8.0);
            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                for version in &dialog.versions {
                    ui.horizontal(|ui| {
                        let text = with_size(format!("{} · {}", version.version, version.date), version.download_size);
                        ui.label(egui::RichText::new(text).color(p.text));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if version.installed {
                                ui.label(egui::RichText::new(t!("toolbox-versions-installed")).color(p.text_secondary));
                            } else if ui.button(t!("toolbox-versions-install")).clicked() {
                                install = Some(version.version.clone());
                            }
                        });
                    });
                }
            });
            ui.add_space(12.0);
            close = ui.button(t!("toolbox-versions-close")).clicked();
        });
        if let Some(version) = install {
            let params = serde_json::json!({ "id": dialog.id, "version": version });
            self.versions = None;
            self.dispatch(ui, registry, session, "toolbox.tool.installVersion", params);
        } else if close || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.versions = None;
        }
    }

    fn header(&mut self, ui: &mut Ui, rect: Rect, registry: &Registry, session: &mut Session, snapshot: &Snapshot) {
        let p = self.palette;

        let family = t!("toolbox-wordmark-family");
        let product = t!("toolbox-wordmark-product");
        let environment = t!("toolbox-environment-local");
        let layout = header::HeaderLayout::new(
            ui,
            rect,
            &header::HeaderText {
                family: &family,
                product: &product,
                wordmark_color: (p.text_secondary, p.text),
                environment: &environment,
                environment_color: p.text,
            },
        );
        icons::product_mark(ui, layout.mark, "toolbox", "Zen Toolbox", false);
        ui.painter().galley(layout.family.min, layout.family_galley.clone(), p.text_secondary);
        ui.painter().galley(layout.product.min, layout.product_galley.clone(), p.text);

        let gear = layout.gear;
        let gear_response = components::icon_button_named(
            ui,
            &p,
            gear,
            ui.scope_id().with("settings"),
            IconButton { icon: icons::gear, icon_size: 16.0, color: p.text_secondary, radius: 8 },
            || t!("toolbox-menu-settings"),
        );
        if snapshot.updates() > 0 || matches!(snapshot.self_update, SelfUpdate::Available(_)) {
            ui.painter().circle_filled(gear.right_top() + vec2(-7.0, 7.0), 3.5, p.attention);
        }
        let can_refresh = registry.is_available(session, "toolbox.releases.refresh");
        let mut refresh = false;
        let mut own_notes = false;
        let mut open_settings = false;
        let mut language_choice: Option<String> = None;
        let mut launch_at_login = snapshot.launch_at_login;
        components::popup_menu(&gear_response, |ui| {
            let label = if snapshot.refreshing {
                t!("toolbox-menu-checking-updates")
            } else {
                t!("toolbox-menu-check-updates")
            };
            if can_refresh {
                refresh = components::menu_item(ui, label);
            } else {
                components::menu_item_disabled(ui, label, None);
            }
            own_notes = components::menu_item(ui, t!("toolbox-menu-whats-new"));
            open_settings = components::menu_item(ui, t!("toolbox-menu-settings"));
            language_menu(ui, &mut language_choice);
            if let Some(on) = &mut launch_at_login {
                launch_at_login_switch(ui, &p, on);
            }
            components::menu_item_disabled(ui, t!("toolbox-menu-about"), None);
            ui.separator();
            if components::menu_item(ui, t!("toolbox-menu-quit")) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
        if refresh {
            self.dispatch(ui, registry, session, "toolbox.releases.refresh", serde_json::Value::Null);
        }
        if let Some(on) = launch_at_login.filter(|&on| Some(on) != snapshot.launch_at_login) {
            let params = serde_json::json!({ "launchAtLogin": on });
            self.dispatch(ui, registry, session, "toolbox.settings.set", params);
        }
        if own_notes {
            self.open_whats_new(registry, session, zen_toolbox::TOOLBOX_ID, TOOLBOX_NAME, None);
        }
        if open_settings {
            self.open_settings(session);
        }
        if let Some(choice) = language_choice {
            zen_i18n::set_language(Some(choice.as_str()));
            let _ = session.settings_mut().set(LANGUAGE_SETTING, choice.as_str());
            ui.ctx().request_repaint();
        }

        let env = layout.selector;
        let env_response = ui.interact(env, ui.scope_id().with("environment"), Sense::click());
        accessibility::name_with(&env_response, egui::Role::ComboBox, || environment.clone());
        hover_fill(ui, &env_response, &p, 8);
        let painter = ui.painter();
        icons::laptop(painter, layout.laptop, p.text_secondary);
        painter.galley(layout.name.min, layout.name_galley.clone(), p.text);
        icons::chevron_down(painter, layout.chevron, p.text_secondary);
        let env_response = if layout.elided { env_response.on_hover_text(environment.as_str()) } else { env_response };
        components::popup_menu(&env_response, |ui| {
            let _ = ui.selectable_label(true, t!("toolbox-environment-local"));
            components::menu_item_disabled(ui, t!("toolbox-environment-remote"), Some(&t!("toolbox-not-implemented")));
        });
    }

    fn tabs(&mut self, ui: &mut Ui, rect: Rect, updates: usize) {
        let p = self.palette;
        let items = [
            components::Tab { label: t!("toolbox-tab-tools"), badge: Some(updates) },
            components::Tab { label: t!("toolbox-tab-projects"), badge: None },
        ];
        let order = [Tab::Tools, Tab::Projects];
        let active = order.iter().position(|tab| *tab == self.tab).unwrap_or(0);
        if let Some(index) = components::tabs(ui, &p, rect, &items, active) {
            self.tab = order[index];
        }

        let search = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(30.0));
        let color = if self.search_open { p.link } else { p.text_secondary };
        let response = components::icon_button_named(
            ui,
            &p,
            search,
            ui.scope_id().with("search"),
            IconButton { icon: icons::search, icon_size: 14.0, color, radius: 8 },
            || t!("toolbox-search-hint"),
        );
        if response.clicked() {
            self.search_open = !self.search_open;
            if !self.search_open {
                self.query.clear();
            }
        }
    }

    fn search_field(&mut self, ui: &mut Ui, rect: Rect) {
        let p = self.palette;
        let hint = t!("toolbox-search-hint");
        let response = components::text_field(ui, &p, rect, &mut self.query, &hint, Some(icons::search));
        if !response.has_focus() && self.query.is_empty() {
            response.request_focus();
        }
    }

    fn matches(&self, tool: &Tool) -> bool {
        let query = self.query.trim().to_lowercase();
        query.is_empty() || tool.name.to_lowercase().contains(&query) || tool.subject.to_lowercase().contains(&query)
    }

    fn tools_tab(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, snapshot: &Snapshot) {
        self.self_update_banner(ui, registry, session, &snapshot.self_update, snapshot.self_update_size);
        if !snapshot.demo {
            self.server_status(ui, registry, session, &snapshot.server, snapshot.refreshing);
        }
        let (installed, other): (Vec<Tool>, Vec<Tool>) = snapshot
            .tools
            .clone()
            .into_iter()
            .filter(|tool| self.matches(tool))
            .partition(|tool| matches!(tool.state, ToolState::Installed { .. }));

        if !installed.is_empty() {
            let can_update_all = registry.is_available(session, "toolbox.tools.updateAll");
            self.panel(ui, |view, ui| {
                let header = view.section_header(ui, &t!("toolbox-section-installed"), None);
                if can_update_all && view.link(ui, header, &t!("toolbox-update-all")) {
                    view.dispatch(ui, registry, session, "toolbox.tools.updateAll", serde_json::Value::Null);
                }
                for tool in &installed {
                    view.tool_row(ui, tool, snapshot.jobs.get(&tool.id), registry, session);
                }
                ui.add_space(4.0);
            });
        }

        if !other.is_empty() {
            ui.add_space(10.0);
            self.panel(ui, |view, ui| {
                let open = view.available_open;
                let header = view.section_header(ui, &t!("toolbox-section-available"), Some(open));
                let response = ui.interact(header, ui.scope_id().with("available-header"), Sense::click());
                accessibility::name_with(&response, egui::Role::DisclosureTriangle, || t!("toolbox-section-available"));
                accessibility::edit(&response, |node| node.set_expanded(open));
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    view.available_open = !open;
                }
                if open {
                    for tool in &other {
                        view.tool_row(ui, tool, snapshot.jobs.get(&tool.id), registry, session);
                    }
                }
                ui.add_space(4.0);
            });
        }
    }

    fn projects_tab(&mut self, ui: &mut Ui) {
        let p = self.palette;
        self.panel(ui, |_, ui| {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                let (kozo, _) = ui.allocate_exact_size(Vec2::splat(KOZO_SIDE), Sense::hover());
                crate::mascot::show(ui, kozo, "toolbox", zen_mascot::State::Idle, KOZO_SIDE / zen_mascot::SIZE as f32);
                ui.add_space(12.0);
                ui.label(
                    egui::RichText::new(t!("toolbox-projects-empty-title")).font(theme::semibold(14.5)).color(p.text),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t!("toolbox-projects-empty-detail"))
                        .font(theme::regular(13.0))
                        .color(p.text_secondary),
                );
            });
            ui.add_space(36.0);
        });
    }

    fn panel(&mut self, ui: &mut Ui, add_contents: impl FnOnce(&mut Self, &mut Ui)) {
        let p = self.palette;
        egui::Frame::new()
            .fill(p.panel)
            .stroke(Stroke::new(1.0, p.panel_edge))
            .corner_radius(CornerRadius::same(metrics::PANEL_RADIUS))
            .inner_margin(egui::Margin::symmetric(metrics::PANEL_PADDING, 4))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                add_contents(self, ui);
            });
    }

    fn section_header(&mut self, ui: &mut Ui, title: &str, open: Option<bool>) -> Rect {
        let p = self.palette;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
        let mut x = rect.left() + 10.0;
        if let Some(open) = open {
            let chevron = Rect::from_center_size(pos2(x + 6.0, rect.center().y), Vec2::splat(13.0));
            if open {
                icons::chevron_down(ui.painter(), chevron, p.text_secondary);
            } else {
                icons::chevron_right(ui.painter(), chevron, p.text_secondary);
            }
            x += 19.0;
        }
        ui.painter().text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            title,
            theme::semibold(13.0),
            p.text_secondary,
        );
        rect
    }

    fn link(&mut self, ui: &mut Ui, row: Rect, text: &str) -> bool {
        let p = self.palette;
        let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::semibold(13.0), p.link);
        let min = pos2(row.right() - 10.0 - galley.size().x, row.center().y - galley.size().y / 2.0);
        let rect = Rect::from_min_size(min, galley.size());
        let response = ui.interact(rect, ui.scope_id().with(("link", text)), Sense::click());
        accessibility::name(&response, egui::Role::Link, text);
        ui.painter().galley(rect.min, galley, p.link);
        if response.hovered() {
            ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, p.link));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        response.clicked()
    }

    fn self_update_banner(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        state: &SelfUpdate,
        size: Option<u64>,
    ) {
        let p = self.palette;
        let (text, action, job) = match state {
            SelfUpdate::UpToDate => return,
            SelfUpdate::Available(version) => (
                with_size(t!("toolbox-self-available", version = version.as_str()), size),
                Some((t!("toolbox-action-update"), "toolbox.self.update")),
                None,
            ),
            SelfUpdate::Downloading(version, job) => {
                (t!("toolbox-self-downloading", version = version.as_str()), None, Some(job.clone()))
            }
            SelfUpdate::ReadyToRestart(version) => (
                t!("toolbox-self-ready", version = version.as_str()),
                Some((t!("toolbox-action-restart"), "toolbox.self.restart")),
                None,
            ),
        };
        self.panel(ui, |view, ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::hover());
            let mark = Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), Vec2::splat(30.0));
            icons::product_mark(ui, mark, "toolbox", "Zen Toolbox", false);
            ui.painter().text(
                pos2(mark.right() + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                text,
                theme::semibold(13.5),
                p.text,
            );
            if let Some(job) = &job {
                view.progress_bar(
                    ui,
                    Rect::from_min_max(
                        pos2(mark.right() + 12.0, rect.bottom() - 10.0),
                        pos2(rect.right() - 12.0, rect.bottom() - 7.0),
                    ),
                    job,
                );
            }
            if let Some((label, command)) = action {
                let anchor = pos2(rect.right() - 10.0, rect.center().y);
                if components::primary_button(ui, &p, anchor, &label, ui.scope_id().with(("self", command))).clicked() {
                    view.dispatch(ui, registry, session, command, serde_json::Value::Null);
                }
            }
        });
        ui.add_space(10.0);
    }

    fn server_status(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        status: &ServerStatus,
        refreshing: bool,
    ) {
        let p = self.palette;
        let (title, detail, action) = match status {
            ServerStatus::NotConfigured | ServerStatus::Online { releases: 1.. } => return,
            ServerStatus::Checking => (t!("toolbox-server-checking"), String::new(), None),
            ServerStatus::NotChecked => (
                t!("toolbox-server-not-checked"),
                t!("toolbox-server-not-checked-detail"),
                Some(t!("toolbox-action-check")),
            ),
            ServerStatus::Online { releases: 0 } => {
                (t!("toolbox-server-no-releases"), t!("toolbox-server-no-releases-detail"), None)
            }
            ServerStatus::Unreachable(error) => {
                (t!("toolbox-server-unreachable"), error.clone(), Some(t!("toolbox-action-retry")))
            }
        };
        self.panel(ui, |view, ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - if action.is_some() { 96.0 } else { 10.0 });
                    ui.label(egui::RichText::new(&title).font(theme::semibold(14.0)).color(p.text));
                    if !detail.is_empty() {
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(&detail).font(theme::regular(12.5)).color(p.text_secondary));
                    }
                });
            });
            ui.add_space(10.0);
            if let Some(action) = &action {
                let rect = ui.min_rect();
                let anchor = pos2(rect.right() - 10.0, rect.top() + 24.0);
                let label = if refreshing { t!("toolbox-action-checking") } else { action.clone() };
                if components::primary_button(ui, &p, anchor, &label, ui.scope_id().with("retry")).clicked()
                    && !refreshing
                {
                    view.dispatch(ui, registry, session, "toolbox.releases.refresh", serde_json::Value::Null);
                }
            }
        });
        ui.add_space(10.0);
    }

    fn progress_bar(&self, ui: &Ui, rect: Rect, job: &Job) {
        let p = self.palette;
        ui.painter().rect_filled(rect, 2, p.hover);
        let fill = match job.fraction() {
            Some(fraction) if !job.unpacking => {
                Rect::from_min_max(rect.min, pos2(rect.left() + rect.width() * fraction, rect.bottom()))
            }
            _ => {
                let t = (ui.input(|input| input.time) * 0.8).fract() as f32;
                let width = rect.width() * 0.3;
                let left = rect.left() + (rect.width() + width) * t - width;
                Rect::from_min_max(
                    pos2(left.max(rect.left()), rect.top()),
                    pos2((left + width).min(rect.right()), rect.bottom()),
                )
            }
        };
        ui.painter().rect_filled(fill, 2, p.accent[1]);
    }

    fn tool_row(&mut self, ui: &mut Ui, tool: &Tool, job: Option<&Job>, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let installed = matches!(tool.state, ToolState::Installed { .. });
        let coming_soon = tool.state == ToolState::ComingSoon;
        let whats_new: Option<Option<String>> = match &tool.state {
            ToolState::Installed { version, update: Some(_), .. } => Some(Some(version.clone())),
            ToolState::Available { .. } => Some(None),
            _ => None,
        };
        let version = if let Some(job) = job {
            match job.fraction() {
                _ if job.unpacking => t!("toolbox-job-installing"),
                Some(fraction) => t!("toolbox-job-downloading-percent", percent = (fraction * 100.0).round()),
                None => t!("toolbox-job-downloading"),
            }
        } else {
            match &tool.state {
                ToolState::Installed { version, update: Some(update), .. } => with_size(
                    t!("toolbox-version-update", version = version.as_str(), update = update.as_str()),
                    tool.download_size,
                ),
                ToolState::Available { version } => with_size(version.clone(), tool.download_size),
                ToolState::Installed { version, update: None, .. } => version.clone(),
                ToolState::ComingSoon => t!("toolbox-coming-soon"),
            }
        };
        let action = match tool.state {
            _ if job.is_some() => None,
            ToolState::Installed { update: Some(_), .. } => Some((t!("toolbox-action-update"), "toolbox.tool.update")),
            ToolState::Available { .. } => Some((t!("toolbox-action-install"), "toolbox.tool.install")),
            _ => None,
        };
        let has_menu = installed && job.is_none();
        let link_text = t!("toolbox-whats-new");
        let layout = row::RowLayout::new(
            ui,
            ui.available_width(),
            &row::RowContent {
                name: &tool.name,
                name_color: if coming_soon { p.text_muted } else { p.text },
                version: &version,
                version_color: if coming_soon { p.text_muted } else { p.text_secondary },
                link: whats_new.is_some().then_some((link_text.as_str(), p.link)),
                progress: job.is_some(),
                button: action.as_ref().map(|(label, _)| label.as_str()),
                more: has_menu,
            },
        );

        let sense = if installed { Sense::click() } else { Sense::hover() };
        let response = components::list_row_named(ui, &p, layout.height, sense, || tool.name.clone());
        let rect = response.rect;
        let layout = layout.at(rect.min);
        let subject = t!(&format!("toolbox-subject-{}", tool.id));
        let hint = if layout.elided { format!("{subject}\n\n{}\n{version}", tool.name) } else { subject };
        let response = response.on_hover_text(hint);
        if response.clicked() {
            self.dispatch(ui, registry, session, "toolbox.tool.launch", serde_json::json!({ "id": tool.id }));
        }
        let hovered = response.hovered();

        let mark = layout.mark;
        icons::product_mark(ui, mark, &tool.id, &tool.name, coming_soon);
        if let ToolState::Installed { running: true, .. } = tool.state {
            let ring = if p.dark { p.window[3] } else { Color32::WHITE };
            icons::status_dot(ui.painter(), mark, p.running, ring);
        }
        ui.painter().galley(layout.name.min, layout.name_galley.clone(), p.text);
        ui.painter().galley(layout.version.min, layout.version_galley.clone(), p.text_secondary);

        if let Some(job) = job {
            if let Some(bar) = layout.bar {
                self.progress_bar(ui, bar, job);
            }
        } else if let (Some(newer_than), Some((link, galley))) = (whats_new, layout.link.clone()) {
            let link_response = ui.interact(link, ui.scope_id().with(("whats-new", &tool.id)), Sense::click());
            accessibility::name_with(&link_response, egui::Role::Link, || t!("toolbox-whats-new"));
            ui.painter().galley(link.min, galley, p.link);
            if link_response.hovered() {
                ui.painter().line_segment([link.left_bottom(), link.right_bottom()], Stroke::new(1.0, p.link));
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if link_response.clicked() {
                self.open_whats_new(registry, session, &tool.id, &tool.name, newer_than);
            }
        }

        let more = layout.more;
        if let (Some((label, command)), Some(button)) = (action, layout.button) {
            let anchor = button.right_center();
            if components::primary_button(ui, &p, anchor, &label, ui.scope_id().with((command, &tool.id))).clicked() {
                self.dispatch(ui, registry, session, command, serde_json::json!({ "id": tool.id }));
            }
        }
        if has_menu {
            let more_response = ui.interact(more, ui.scope_id().with(("more", &tool.id)), Sense::click());
            accessibility::button_with(&more_response, || t!("a11y-options"));
            hover_fill(ui, &more_response, &p, 6);
            let dots = Rect::from_center_size(more.center(), Vec2::splat(15.0));
            icons::more_vertical(ui.painter(), dots, if hovered { p.text } else { p.text_secondary });
            let (mut uninstall, mut notes, mut versions, mut automatic) = (false, false, false, false);
            let updates_by_itself = zen_toolbox::auto_update::is_on(session, &tool.id);
            components::popup_menu(&more_response, |ui| {
                notes = components::menu_item(ui, t!("toolbox-whats-new"));
                versions = components::menu_item(ui, t!("toolbox-versions-menu"));
                let mut on = updates_by_itself;
                automatic = ui.checkbox(&mut on, t!("toolbox-auto-update-menu")).clicked();
                components::menu_item_disabled(ui, t!("toolbox-tool-settings"), Some(&t!("toolbox-not-implemented")));
                ui.separator();
                uninstall = components::menu_item(ui, t!("toolbox-uninstall-menu"));
            });
            if uninstall {
                self.confirm_uninstall = Some((tool.id.clone(), tool.name.clone()));
            }
            if notes {
                self.open_whats_new(registry, session, &tool.id, &tool.name, None);
            }
            if versions {
                self.open_versions(ui, registry, session, tool);
            }
            if automatic {
                let params = serde_json::json!({ "id": tool.id, "enabled": !updates_by_itself });
                self.dispatch(ui, registry, session, "toolbox.tool.setAutoUpdate", params);
            }
        }
    }

    fn dispatch(
        &mut self,
        ui: &Ui,
        registry: &Registry,
        session: &mut Session,
        command: &str,
        params: serde_json::Value,
    ) {
        if let Err(error) = registry.dispatch(session, command, params) {
            self.post(ui, error_message(error));
        }
    }

    fn post(&mut self, ui: &Ui, message: String) {
        self.notice = Some((message, ui.input(|input| input.time)));
    }

    fn notice(&mut self, ui: &mut Ui, window: Rect) {
        let Some((message, posted)) = &self.notice else { return };
        let now = ui.input(|input| input.time);
        if now - posted > NOTICE_SECONDS {
            self.notice = None;
            return;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        let p = self.palette;
        let width = window.width() - 2.0 * metrics::MARGIN;
        let galley = ui.painter().layout(message.clone(), theme::regular(13.0), p.text, width - 28.0);
        let height = galley.size().y + 22.0;
        let min = pos2(window.left() + metrics::MARGIN, window.bottom() - metrics::MARGIN - height);
        let rect = Rect::from_min_size(min, vec2(width, height));
        ui.painter().rect_filled(rect, 10, p.popup);
        ui.painter().rect_stroke(rect, 10, Stroke::new(1.0, p.panel_edge), StrokeKind::Inside);
        ui.painter().galley(rect.min + vec2(14.0, 11.0), galley, p.text);
        let response = ui.interact(rect, ui.scope_id().with("notice"), Sense::click());
        accessibility::name(&response, egui::Role::Status, message);
        accessibility::edit(&response, |node| node.set_live(egui::accesskit::Live::Polite));
        if response.clicked() {
            self.notice = None;
        }
    }
}

pub const LANGUAGE_SETTING: &str = "ui.language";

pub const LOCALES: &[(&str, &str)] = &[
    ("en", include_str!("../locales/en/ui.ftl")),
    ("ru", include_str!("../locales/ru/ui.ftl")),
    ("en", include_str!("../locales/en/accessibility.ftl")),
    ("ru", include_str!("../locales/ru/accessibility.ftl")),
    ("en", include_str!("../locales/en/terminal.ftl")),
    ("ru", include_str!("../locales/ru/terminal.ftl")),
    ("en", include_str!("../locales/en/settings-appearance.ftl")),
    ("ru", include_str!("../locales/ru/settings-appearance.ftl")),
    ("en", include_str!("../locales/en/settings-editor.ftl")),
    ("ru", include_str!("../locales/ru/settings-editor.ftl")),
    ("en", include_str!("../locales/en/settings-modules.ftl")),
    ("ru", include_str!("../locales/ru/settings-modules.ftl")),
    ("en", include_str!("../locales/en/settings-search.ftl")),
    ("ru", include_str!("../locales/ru/settings-search.ftl")),
    ("en", include_str!("../locales/en/settings-keymap.ftl")),
    ("ru", include_str!("../locales/ru/settings-keymap.ftl")),
    ("en", include_str!("../locales/en/commands.ftl")),
    ("ru", include_str!("../locales/ru/commands.ftl")),
    ("en", include_str!("../locales/en/synonyms.ftl")),
    ("ru", include_str!("../locales/ru/synonyms.ftl")),
    ("en", include_str!("../locales/en/local-history.ftl")),
    ("ru", include_str!("../locales/ru/local-history.ftl")),
    ("en", include_str!("../locales/en/search-scopes.ftl")),
    ("ru", include_str!("../locales/ru/search-scopes.ftl")),
    ("en", include_str!("../locales/en/headless.ftl")),
    ("ru", include_str!("../locales/ru/headless.ftl")),
    ("en", include_str!("../locales/en/command-line.ftl")),
    ("ru", include_str!("../locales/ru/command-line.ftl")),
    ("en", include_str!("../locales/en/caffeine.ftl")),
    ("ru", include_str!("../locales/ru/caffeine.ftl")),
    ("en", include_str!("../locales/en/database-history.ftl")),
    ("ru", include_str!("../locales/ru/database-history.ftl")),
    ("en", include_str!("../locales/en/database-plan.ftl")),
    ("ru", include_str!("../locales/ru/database-plan.ftl")),
    ("en", include_str!("../locales/en/database-diagram.ftl")),
    ("ru", include_str!("../locales/ru/database-diagram.ftl")),
    ("en", include_str!("../locales/en/database-sessions.ftl")),
    ("ru", include_str!("../locales/ru/database-sessions.ftl")),
    ("en", include_str!("../locales/en/database-migrations.ftl")),
    ("ru", include_str!("../locales/ru/database-migrations.ftl")),
    ("en", include_str!("../locales/en/database-clients.ftl")),
    ("ru", include_str!("../locales/ru/database-clients.ftl")),
    ("en", include_str!("../locales/en/database-recovery.ftl")),
    ("ru", include_str!("../locales/ru/database-recovery.ftl")),
    ("en", include_str!("../locales/en/database-find.ftl")),
    ("ru", include_str!("../locales/ru/database-find.ftl")),
    ("en", include_str!("../locales/en/database-compare.ftl")),
    ("ru", include_str!("../locales/ru/database-compare.ftl")),
    ("en", include_str!("../locales/en/database-clipboard.ftl")),
    ("ru", include_str!("../locales/ru/database-clipboard.ftl")),
    ("en", include_str!("../locales/en/database-backup.ftl")),
    ("ru", include_str!("../locales/ru/database-backup.ftl")),
    ("en", include_str!("../locales/en/database-csv-import.ftl")),
    ("ru", include_str!("../locales/ru/database-csv-import.ftl")),
    ("en", include_str!("../locales/en/database-transfer.ftl")),
    ("ru", include_str!("../locales/ru/database-transfer.ftl")),
    ("en", include_str!("../locales/en/database-structure.ftl")),
    ("ru", include_str!("../locales/ru/database-structure.ftl")),
    ("en", include_str!("../locales/en/external-tools.ftl")),
    ("ru", include_str!("../locales/ru/external-tools.ftl")),
    ("en", include_str!("../locales/en/ant.ftl")),
    ("ru", include_str!("../locales/ru/ant.ftl")),
    ("en", include_str!("../locales/en/npm.ftl")),
    ("ru", include_str!("../locales/ru/npm.ftl")),
    ("en", include_str!("../locales/en/plugin_tools.ftl")),
    ("ru", include_str!("../locales/ru/plugin_tools.ftl")),
    ("en", include_str!("../locales/en/endpoints.ftl")),
    ("ru", include_str!("../locales/ru/endpoints.ftl")),
    ("en", include_str!("../locales/en/resource-bundles.ftl")),
    ("ru", include_str!("../locales/ru/resource-bundles.ftl")),
    ("en", include_str!("../locales/en/dependencies.ftl")),
    ("ru", include_str!("../locales/ru/dependencies.ftl")),
    ("en", include_str!("../locales/en/file-watchers.ftl")),
    ("ru", include_str!("../locales/ru/file-watchers.ftl")),
];

fn language_menu(ui: &mut Ui, choice: &mut Option<String>) {
    let _ = ui.menu_button(t!("toolbox-menu-language"), |ui| {
        let chosen = zen_i18n::chosen_language().unwrap_or_else(|| zen_i18n::SOURCE.to_owned());
        for (code, name) in LANGUAGES {
            if ui.selectable_label(chosen == *code, *name).clicked() {
                *choice = Some((*code).to_owned());
            }
        }
        let system = zen_i18n::system_language();
        let system_name = LANGUAGES.iter().find(|(code, _)| *code == system).map_or("English", |(_, name)| *name);
        let label = t!("toolbox-menu-language-system", language = system_name);
        if ui.selectable_label(chosen == zen_i18n::SYSTEM, label).clicked() {
            *choice = Some(zen_i18n::SYSTEM.to_owned());
        }
    });
}

fn with_size(text: String, size: Option<u64>) -> String {
    match size {
        Some(bytes) => t!("toolbox-with-size", text = text.as_str(), size = zen_toolbox::megabytes(bytes)),
        None => text,
    }
}

fn launch_at_login_switch(ui: &mut Ui, p: &Palette, on: &mut bool) {
    let label = t!("toolbox-menu-launch-at-login");
    let padding = ui.spacing().button_padding.x;
    let galley = egui::WidgetText::from(label.as_str()).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Button,
    );
    let natural = padding + galley.size().x + 16.0 + components::metrics::TOGGLE_WIDTH + padding;
    let height = ui.spacing().interact_size.y.max(galley.size().y);
    let (rect, row) = ui.allocate_exact_size(vec2(ui.min_rect().width().max(natural), height), Sense::hover());
    let text_pos = pos2(rect.left() + padding, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(text_pos, galley, ui.visuals().widgets.inactive.fg_stroke.color);
    let toggle = Rect::from_min_max(
        pos2(rect.right() - padding - components::metrics::TOGGLE_WIDTH, rect.top()),
        pos2(rect.right() - padding, rect.bottom()),
    );
    let switch = ui
        .scope_builder(
            egui::UiBuilder::new().max_rect(toggle).layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| components::toggle(ui, p, on, &label),
        )
        .inner;
    let response = row.union(switch);
    let _ = response.on_hover_text(t!("toolbox-menu-launch-at-login-hint"));
}

fn error_message(error: zen_core::DispatchError) -> String {
    match error {
        zen_core::DispatchError::Failed { error, .. } => error.message,
        other => other.to_string(),
    }
}

fn release_heading(ui: &mut Ui, note: &ReleaseNote, p: Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(egui::RichText::new(&note.version).font(theme::semibold(14.0)).color(p.text));
        if !note.date.is_empty() {
            ui.label(egui::RichText::new(&note.date).font(theme::regular(12.5)).color(p.text_secondary));
        }
        if note.installed {
            ui.label(egui::RichText::new(t!("toolbox-whats-new-installed")).font(theme::semibold(12.0)).color(p.link));
        }
    });
}

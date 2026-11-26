mod branches;
mod details;
mod format;
mod paths;

use std::collections::HashSet;

use egui::{Align2, Color32, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::branches::BranchList;
use zen_workbench::vcs::log::{CommitDetailsInfo, LogCommit, LogList, LogSort};
use zen_workbench::vcs::log_view::{LogFilters, LogTab, LogViewInfo};

use super::glyphs::{
    Glyph, glyph_cherry_pick, glyph_eye, glyph_label, glyph_minus, glyph_more, glyph_plus, glyph_refresh, glyph_search,
    glyph_sort,
};
use super::run_window::Tool;
use super::workspace::GitLayout;
use super::{IdePalette, IdeView, Side, customization, hint, hover, metrics};
use crate::theme;

const ROW: f32 = 26.0;
const LANE: f32 = 14.0;
const PAGE: usize = 500;
const BAR: f32 = 40.0;
const STRIPE: f32 = 36.0;
const AUTHOR: f32 = 132.0;
const DATE: f32 = 128.0;
const LABEL_NAME: f32 = 130.0;

type ReadKey = (usize, LogFilters, u64, Option<String>, usize);

#[derive(Default)]
pub(super) struct GitWindow {
    view: LogViewInfo,
    commits: Vec<LogCommit>,
    read_for: Option<ReadKey>,
    more: bool,
    limit: usize,
    error: Option<String>,
    local_offset: i32,
    details: Option<CommitDetailsInfo>,
    shown_selection: Option<String>,
    clicked: bool,
    pending_show: Option<String>,
    branches: BranchList,
    branches_for: Option<(u64, Option<String>)>,
    branches_hidden: bool,
    flat_branches: bool,
    branch_search: String,
    collapsed_branches: HashSet<String>,
    selected_branch: Option<branches::BranchKey>,
    flat_changes: bool,
    collapsed_changes: HashSet<String>,
    selected_change: Option<usize>,
    text: String,
    text_for: Option<(usize, String)>,
    goto: Option<String>,
    date_dialog: Option<(String, String, bool)>,
    path_input: String,
    paths_dialog: Option<paths::PathsDialog>,
}

impl GitWindow {
    pub(super) fn show_commit(&mut self, id: String) {
        self.pending_show = Some(id);
    }

    pub(super) fn layout(&self) -> GitLayout {
        GitLayout {
            favorites: None,
            branches_hidden: self.branches_hidden,
            flat_branches: self.flat_branches,
            flat_changes: self.flat_changes,
            ..GitLayout::default()
        }
    }

    pub(super) fn restore(&mut self, layout: &GitLayout) {
        self.branches_hidden = layout.branches_hidden;
        self.flat_branches = layout.flat_branches;
        self.flat_changes = layout.flat_changes;
    }

    pub(super) fn open_dialogs(&self) -> Vec<&'static str> {
        [
            (self.goto.is_some(), "gitGoTo"),
            (self.date_dialog.is_some(), "gitDateRange"),
            (self.paths_dialog.is_some(), "gitPaths"),
        ]
        .into_iter()
        .filter_map(|(open, name)| open.then_some(name))
        .collect()
    }

    fn tab(&self) -> LogTab {
        self.view.tabs.get(self.view.active).cloned().unwrap_or_default()
    }

    pub(super) fn view_action(&mut self, action: &str) -> bool {
        match action {
            "view.vcs.logBranches" => self.branches_hidden = !self.branches_hidden,
            "view.vcs.logGroupBranches" => self.flat_branches = !self.flat_branches,
            "view.vcs.logGroupChanges" => self.flat_changes = !self.flat_changes,
            "view.vcs.logExpandBranches" => self.collapsed_branches.clear(),
            "view.vcs.logCollapseBranches" => self.collapsed_branches = self.branch_folders(),
            "view.vcs.logExpandChanges" => self.collapsed_changes.clear(),
            "view.vcs.logCollapseChanges" => {
                let paths: Vec<&str> = self
                    .details
                    .iter()
                    .flat_map(|details| details.files.iter().map(|file| file.relative.as_str()))
                    .collect();
                self.collapsed_changes = format::change_folders(&paths).into_iter().collect();
            }
            "view.vcs.logGoTo" => self.goto = Some(String::new()),
            _ => return false,
        }
        true
    }
}

enum GitAction {
    Hide,
    Run(&'static str, Value),
    View(&'static str),
    Diff { id: String, path: std::path::PathBuf },
    NewBranch(Option<String>),
}

impl IdeView {
    pub(super) fn git_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let mut actions = Vec::new();
        self.read_view(registry, session);
        self.git_header(ui, &p, &mut actions);
        if self.vcs.info.root.is_none() {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                if self.safe_mode {
                    ui.label(egui::RichText::new(t!("ide-vcs-safe-mode")).color(p.text_secondary));
                    if ui.link(t!("ide-safe-mode-trust")).clicked() {
                        self.ask_trust(session);
                    }
                } else {
                    ui.label(egui::RichText::new(t!("vcs-commit-no-repository")).color(p.text_secondary));
                }
            });
        } else {
            self.read_log(registry, session);
            self.read_details(registry, session);
            let full = ui.available_rect_before_wrap();
            ui.allocate_rect(full, Sense::hover());
            let stripe = Rect::from_min_max(full.min, pos2(full.left() + STRIPE, full.bottom()));
            let panel_width = (full.width() * 0.25).clamp(180.0, 420.0);
            let branches = if self.git_window.branches_hidden {
                Rect::from_min_max(pos2(stripe.right(), full.top()), pos2(stripe.right(), full.bottom()))
            } else {
                Rect::from_min_max(pos2(stripe.right(), full.top()), pos2(stripe.right() + panel_width, full.bottom()))
            };
            let details_width = (full.width() * 0.2).clamp(220.0, 420.0);
            let details = Rect::from_min_max(pos2(full.right() - details_width, full.top()), full.max);
            let log =
                Rect::from_min_max(pos2(branches.right() + 1.0, full.top()), pos2(details.left() - 1.0, full.bottom()));
            let line = Stroke::new(1.0, p.frame);
            ui.painter().vline(stripe.right(), full.y_range(), line);
            if !self.git_window.branches_hidden {
                ui.painter().vline(branches.right(), full.y_range(), line);
            }
            ui.painter().vline(details.left(), full.y_range(), line);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(stripe).id_salt("git-stripe"));
            self.git_window.branch_toolbar(&mut child, &p, &self.vcs.info, &mut actions);
            if !self.git_window.branches_hidden {
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(branches).id_salt("git-branches"));
                self.git_window.branch_panel(&mut child, &p, &mut actions);
            }
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(log).id_salt("git-log"));
            let root = self.vcs.info.root.clone();
            self.git_window.filter_bar(&mut child, &p, root.as_deref(), &mut actions);
            self.git_window.commit_list(&mut child, &p, &self.vcs.info, &mut actions);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(details).id_salt("git-details"));
            self.git_window.details_panel(&mut child, &p, &mut actions);
            self.git_window.date_range_dialog(ui, &p, &mut actions);
            if let Some(dialog) = &mut self.git_window.paths_dialog
                && !dialog.show(ui, &p, &mut actions)
            {
                self.git_window.paths_dialog = None;
            }
        }
        for action in actions {
            match action {
                GitAction::Hide => self.bottom = None,
                GitAction::Run(command, params) => {
                    if command == "vcs.log.select" {
                        self.git_window.clicked = true;
                    }
                    if let Err(message) = super::run_window::call(registry, session, command, params) {
                        let dialog = session
                            .state::<zen_workbench::vcs::workspace::GitWorkspace>()
                            .is_some_and(zen_workbench::vcs::workspace::GitWorkspace::has_conflicts);
                        if !dialog {
                            self.post(ui, message);
                        }
                    }
                    if command == "vcs.status.refresh" {
                        self.git_window.read_for = None;
                        self.git_window.branches_for = None;
                    }
                }
                GitAction::View(action) => {
                    self.git_window.view_action(action);
                }
                GitAction::NewBranch(start) => self.open_new_branch(start),
                GitAction::Diff { id, path } => {
                    match super::run_window::call(
                        registry,
                        session,
                        "vcs.diff.commit",
                        json!({ "id": id, "path": path }),
                    ) {
                        Ok(value) => {
                            if let Ok(view) = serde_json::from_value(value) {
                                self.show_diff(session, view);
                            }
                        }
                        Err(message) => self.post(ui, message),
                    }
                }
            }
        }
    }

    pub(super) fn git_show_diff(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let window = &self.git_window;
        let Some(details) = &window.details else { return };
        let Some(file) = window.selected_change.and_then(|at| details.files.get(at)).or(details.files.first()) else {
            return;
        };
        let params = json!({ "id": details.id, "path": file.path });
        match super::run_window::call(registry, session, "vcs.diff.commit", params) {
            Ok(value) => {
                if let Ok(view) = serde_json::from_value(value) {
                    self.show_diff(session, view);
                }
            }
            Err(message) => self.post_at(ctx, message),
        }
    }

    pub(super) fn git_select_paths(&mut self, registry: &Registry, session: &mut Session) -> Result<(), String> {
        let Some(root) = self.vcs.info.root.clone() else {
            return Err("Select Folders needs a Git repository".to_owned());
        };
        self.read_view(registry, session);
        let paths = self.git_window.tab().filters.paths;
        self.git_window.paths_dialog = Some(paths::PathsDialog::new(&root, &paths));
        Ok(())
    }

    pub(super) fn git_show_history(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &std::path::Path,
    ) -> Result<(), String> {
        super::run_window::call(registry, session, "vcs.logTab.new", json!({ "paths": [path] }))?;
        Ok(())
    }

    fn read_view(&mut self, registry: &Registry, session: &mut Session) {
        if let Some(id) = self.git_window.pending_show.take() {
            let _ = registry.dispatch(session, "vcs.logTab.select", json!({ "index": 0 }));
            let _ = registry.dispatch(session, "vcs.log.filter", json!({ "tab": 0, "reset": true }));
            let _ = registry.dispatch(session, "vcs.log.select", json!({ "tab": 0, "id": id }));
        }
        if let Some(view) = registry
            .dispatch(session, "vcs.log.view", json!({}))
            .ok()
            .and_then(|view| serde_json::from_value(view).ok())
        {
            self.git_window.view = view;
        }
    }

    fn read_log(&mut self, registry: &Registry, session: &mut Session) {
        let window = &mut self.git_window;
        if window.limit == 0 {
            window.limit = PAGE;
        }
        let tab = window.tab();
        let key = (
            window.view.active,
            tab.filters.clone(),
            self.vcs.info.revision,
            self.vcs.info.commit.clone(),
            window.limit,
        );
        if window.read_for.as_ref() != Some(&key) {
            let params = json!({ "tab": window.view.active, "limit": window.limit });
            let list: LogList = registry
                .dispatch(session, "vcs.log.list", params)
                .ok()
                .and_then(|list| serde_json::from_value(list).ok())
                .unwrap_or_default();
            window.commits = list.commits;
            window.more = list.more;
            window.error = list.error;
            window.local_offset = list.local_offset;
            window.read_for = Some(key);
            window.shown_selection = None;
        }
        let branches_key = (self.vcs.info.revision, self.vcs.info.commit.clone());
        if window.branches_for.as_ref() != Some(&branches_key) {
            window.branches = registry
                .dispatch(session, "vcs.branch.list", json!({}))
                .ok()
                .and_then(|list| serde_json::from_value(list).ok())
                .unwrap_or_default();
            window.branches_for = Some(branches_key);
        }
    }

    fn read_details(&mut self, registry: &Registry, session: &mut Session) {
        let window = &mut self.git_window;
        let Some(id) = window.tab().selected else {
            window.details = None;
            return;
        };
        if window.details.as_ref().is_none_or(|details| details.id != id) {
            window.details = registry
                .dispatch(session, "vcs.commit.details", json!({ "id": id }))
                .ok()
                .and_then(|details| serde_json::from_value(details).ok());
            window.selected_change = None;
        }
    }

    fn git_header(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<GitAction>) {
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        let y = header.center().y;
        let title = ui.painter().text(
            pos2(header.left() + 12.0, y),
            Align2::LEFT_CENTER,
            t!("ide-tool-git"),
            theme::semibold(13.0),
            p.text,
        );
        let window = &mut self.git_window;
        let mut x = title.right() + 14.0;
        let titles: Vec<String> =
            window.view.tabs.iter().enumerate().map(|(index, tab)| tab_title(index, tab)).collect();
        for (index, name) in titles.iter().enumerate() {
            let galley = ui.painter().layout_no_wrap(name.clone(), theme::regular(13.0), p.text);
            let closable = index > 0;
            let width = galley.size().x + 20.0 + if closable { 18.0 } else { 0.0 };
            let tab = Rect::from_min_size(pos2(x, y - 13.0), vec2(width, 26.0));
            let response = ui.interact(tab, ui.scope_id().with(("git-tab", index)), Sense::click());
            crate::accessibility::item(&response, egui::Role::Tab, index == window.view.active, name);
            if index == window.view.active {
                ui.painter().rect_filled(tab, 6, p.selection);
            } else {
                hover(ui, &response, p);
            }
            ui.painter().galley(pos2(tab.left() + 10.0, y - galley.size().y / 2.0), galley, p.text);
            if closable {
                let close = Rect::from_center_size(pos2(tab.right() - 13.0, y), Vec2::splat(14.0));
                let hit = ui.interact(close, ui.scope_id().with(("git-tab-close", index)), Sense::click());
                if response.hovered() || hit.hovered() || index == window.view.active {
                    let color = if hit.hovered() { p.text } else { p.text_secondary };
                    let r = close.shrink(3.5);
                    ui.painter().line_segment([r.left_top(), r.right_bottom()], Stroke::new(1.2, color));
                    ui.painter().line_segment([r.right_top(), r.left_bottom()], Stroke::new(1.2, color));
                }
                hint(ui, &hit, &t!("vcs-log-close-tab"), None, Side::Below);
                if hit.clicked() {
                    actions.push(GitAction::Run("vcs.logTab.close", json!({ "index": index })));
                    continue;
                }
            }
            if response.clicked() && index != window.view.active {
                actions.push(GitAction::Run("vcs.logTab.select", json!({ "index": index })));
            }
            x = tab.right() + 4.0;
        }
        let plus = Rect::from_center_size(pos2(x + 14.0, y), Vec2::splat(24.0));
        if header_button(ui, p, plus, "git-new-tab", glyph_plus, &t!("vcs-log-new-tab")).clicked() {
            actions.push(GitAction::Run("vcs.logTab.new", json!({})));
        }
        let list = Rect::from_center_size(pos2(plus.right() + 14.0, y), Vec2::splat(24.0));
        let response = ui.interact(list, ui.scope_id().with("git-tab-list"), Sense::click());
        hover(ui, &response, p);
        hint(ui, &response, &t!("vcs-log-tabs"), None, Side::Below);
        crate::icons::chevron_down(ui.painter(), list.shrink(6.5), p.text_secondary);
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(180.0);
            for (index, name) in titles.iter().enumerate() {
                if ui.selectable_label(index == window.view.active, name).clicked() {
                    actions.push(GitAction::Run("vcs.logTab.select", json!({ "index": index })));
                    ui.close();
                }
            }
        });
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, y), Vec2::splat(24.0));
        if header_button(ui, p, hide, "git-hide", glyph_minus, &t!("ide-project-hide")).clicked() {
            actions.push(GitAction::Hide);
        }
        let options = Rect::from_center_size(pos2(hide.left() - 16.0, y), Vec2::splat(24.0));
        let response = header_button(ui, p, options, "git-options", glyph_more, &t!("vcs-branches-options"));
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(240.0);
            view_options(ui, window, actions);
        });
    }
}

fn view_options(ui: &mut Ui, window: &GitWindow, actions: &mut Vec<GitAction>) {
    let toggles = [
        (!window.branches_hidden, "vcs-log-show-branches", "view.vcs.logBranches"),
        (!window.flat_branches, "vcs-log-group-branches", "view.vcs.logGroupBranches"),
        (!window.flat_changes, "vcs-log-group-changes", "view.vcs.logGroupChanges"),
    ];
    for (on, name, action) in toggles {
        let mut checked = on;
        if ui.checkbox(&mut checked, t!(name)).clicked() {
            actions.push(GitAction::View(action));
        }
    }
}

fn tab_title(index: usize, tab: &LogTab) -> String {
    if let Some(title) = &tab.title {
        return title.clone();
    }
    let filters = &tab.filters;
    let what = [&filters.branches, &filters.users, &filters.paths].into_iter().find(|list| !list.is_empty());
    match what {
        Some(list) if index > 0 => t!("vcs-log-tab-filtered", filter = list.join(", ").as_str()),
        _ if index > 0 && !filters.text.trim().is_empty() => {
            t!("vcs-log-tab-filtered", filter = filters.text.trim())
        }
        _ => t!("vcs-log-tab"),
    }
}

fn header_button(ui: &Ui, p: &IdePalette, rect: Rect, salt: &str, glyph: Glyph, name: &str) -> egui::Response {
    let response = ui.interact(rect, ui.scope_id().with(salt), Sense::click());
    hover(ui, &response, p);
    hint(ui, &response, name, None, Side::Below);
    glyph(ui.painter(), rect.shrink(6.0), p.text_secondary);
    response
}

fn active_selection(window: &GitWindow) -> Option<String> {
    window.view.tabs.get(window.view.active).and_then(|tab| tab.selected.clone())
}

impl GitWindow {
    fn filter_bar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        root: Option<&std::path::Path>,
        actions: &mut Vec<GitAction>,
    ) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), BAR), Sense::hover());
        let y = bar.center().y;
        let tab = self.tab();
        let filters = tab.filters.clone();
        let active = self.view.active;
        let typed_for = (active, filters.text.clone());
        if self.text_for.as_ref() != Some(&typed_for) {
            self.text = filters.text.clone();
            self.text_for = Some(typed_for);
        }
        let tools: [(&'static str, Glyph, String); 4] = [
            ("git-goto", glyph_search, t!("vcs-log-goto")),
            ("git-view-options", glyph_eye, t!("vcs-log-view-options")),
            ("git-cherry-pick", glyph_cherry_pick, t!("vcs-log-cherry-pick")),
            ("git-refresh", glyph_refresh, t!("vcs-log-refresh")),
        ];
        let mut right = bar.right() - 6.0;
        for (salt, glyph, name) in tools {
            let rect = Rect::from_center_size(pos2(right - 13.0, y), Vec2::splat(26.0));
            right = rect.left() - 2.0;
            let enabled = salt != "git-cherry-pick" || tab.selected.is_some();
            let clicked = Tool::new(salt, glyph, name, p.text_secondary).show_hinted(ui, p, rect, enabled, Side::Below);
            let response = ui.interact(rect, ui.scope_id().with(salt), Sense::click());
            match salt {
                "git-goto" if clicked => self.goto = Some(String::new()),
                "git-refresh" if clicked => actions.push(GitAction::Run("vcs.status.refresh", json!({}))),
                "git-cherry-pick" if clicked => {
                    if let Some(id) = &tab.selected {
                        actions.push(GitAction::Run("vcs.commit.cherryPick", json!({ "id": id })));
                    }
                }
                "git-view-options" => {
                    egui::Popup::menu(&response).show(|ui| {
                        ui.set_min_width(260.0);
                        view_options(ui, self, actions);
                    });
                }
                _ => {}
            }
        }
        let (since, until) = (filters.since, filters.until);
        let offset = self.local_offset;
        let date_value = date_label(since, until, offset);
        let joined = |list: &[String]| Some(list.join(", ")).filter(|value| !value.is_empty());
        let buttons: [(&str, &str, Option<String>); 4] = [
            ("branches", "vcs-log-branch", joined(&filters.branches)),
            ("users", "vcs-log-user", joined(&filters.users)),
            ("date", "vcs-log-date", date_value),
            ("paths", "vcs-log-paths", joined(&filters.paths)),
        ];
        let limit = right - 36.0;
        let measure = |text: String| ui.painter().layout_no_wrap(text, theme::regular(13.0), p.text).size().x;
        let (minimum, natural): (Vec<f32>, Vec<f32>) = buttons
            .iter()
            .map(|(_, name, value)| match value {
                Some(value) => (
                    measure(t!("vcs-log-filter-value", filter = t!(*name).as_str(), value = "…")),
                    measure(t!("vcs-log-filter-value", filter = t!(*name).as_str(), value = value.as_str())).min(150.0),
                ),
                None => {
                    let width = measure(t!(*name));
                    (width, width)
                }
            })
            .unzip();
        let needed: f32 = minimum.iter().sum::<f32>() + buttons.len() as f32 * 34.0 + 10.0;
        let field_width = (bar.width() * 0.34).clamp(170.0, 300.0).min((limit - bar.left() - 6.0 - needed).max(110.0));
        let field = Rect::from_min_size(pos2(bar.left() + 6.0, y - 15.0), vec2(field_width, 30.0));
        ui.painter().rect_stroke(field, 5, Stroke::new(1.0, p.text_field_border), egui::StrokeKind::Inside);
        glyph_search(
            ui.painter(),
            Rect::from_center_size(pos2(field.left() + 15.0, y), Vec2::splat(14.0)),
            p.text_secondary,
        );
        let toggles = [
            (".*", filters.regex, "vcs-log-regex", "regex"),
            ("Cc", filters.match_case, "vcs-log-match-case", "matchCase"),
        ];
        let mut toggle_left = field.right() - 4.0;
        for (mark, on, name, param) in toggles.into_iter().rev() {
            let rect = Rect::from_min_size(pos2(toggle_left - 24.0, y - 11.0), vec2(24.0, 22.0));
            toggle_left = rect.left() - 2.0;
            let response = ui.interact(rect, ui.scope_id().with(("git-text-toggle", param)), Sense::click());
            if on {
                ui.painter().rect_filled(rect, 4, p.popup_hover);
            } else {
                hover(ui, &response, p);
            }
            let color = if on { p.text } else { p.text_secondary };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, mark, theme::semibold(12.5), color);
            hint(ui, &response, &t!(name), None, Side::Below);
            if response.clicked() {
                actions.push(GitAction::Run("vcs.log.filter", json!({ param: !on })));
            }
        }
        let text_rect =
            Rect::from_min_max(pos2(field.left() + 30.0, field.top()), pos2(toggle_left - 2.0, field.bottom()));
        let edit = ui.put(
            text_rect,
            egui::TextEdit::singleline(&mut self.text)
                .id(ui.scope_id().with("git-log-text"))
                .frame(egui::Frame::NONE)
                .align(Align2::LEFT_CENTER)
                .font(theme::regular(13.0))
                .hint_text(t!("vcs-log-search"))
                .desired_width(text_rect.width()),
        );
        if edit.changed() {
            actions.push(GitAction::Run("vcs.log.filter", json!({ "text": self.text })));
            self.text_for = Some((active, self.text.clone()));
        }
        let mut x = field.right() + 10.0;
        let all_branches: Vec<String> = std::iter::once("HEAD".to_owned())
            .chain(self.view.favorites.iter().cloned())
            .chain(self.branches.local.iter().map(|branch| branch.name.clone()))
            .chain(self.branches.remote.iter().map(|branch| branch.name.clone()))
            .chain(self.branches.tags.iter().map(|tag| tag.name.clone()))
            .fold(Vec::new(), |mut names, name| {
                if !names.contains(&name) {
                    names.push(name);
                }
                names
            });
        let me = self.view.user_name.clone();
        let mut authors: Vec<String> = self.commits.iter().map(|commit| commit.author.clone()).collect();
        authors.sort();
        authors.dedup();
        let text_widths = format::share(limit - x - buttons.len() as f32 * 34.0, &minimum, &natural);
        for ((which, name, value), text_width) in buttons.into_iter().zip(text_widths) {
            if x + text_width + 30.0 > limit {
                break;
            }
            let (response, cleared, width) = filter_button(ui, p, x, y, text_width, which, &t!(name), value.as_deref());
            x += width + 4.0;
            if cleared {
                let params = match which {
                    "branches" => json!({ "branches": [] }),
                    "users" => json!({ "users": [] }),
                    "date" => json!({ "date": {} }),
                    _ => json!({ "paths": [] }),
                };
                actions.push(GitAction::Run("vcs.log.filter", params));
                continue;
            }
            egui::Popup::menu(&response).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_min_width(220.0);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| match which {
                    "branches" => {
                        for name in &all_branches {
                            let on = filters.branches.contains(name);
                            if ui.selectable_label(on, name).clicked() {
                                actions.push(GitAction::Run(
                                    "vcs.log.filter",
                                    json!({ "branches": toggled(&filters.branches, name) }),
                                ));
                            }
                        }
                    }
                    "users" => {
                        let mut names: Vec<(String, String)> = Vec::new();
                        if let Some(me) = &me {
                            names.push((me.clone(), t!("vcs-log-me")));
                        }
                        names.extend(
                            authors
                                .iter()
                                .filter(|author| Some(*author) != me.as_ref())
                                .map(|author| (author.clone(), author.clone())),
                        );
                        for (name, shown) in names {
                            let on = filters.users.contains(&name);
                            if ui.selectable_label(on, shown).clicked() {
                                actions.push(GitAction::Run(
                                    "vcs.log.filter",
                                    json!({ "users": toggled(&filters.users, &name) }),
                                ));
                            }
                        }
                    }
                    "date" => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0, |since| since.as_secs() as i64);
                        for (name, days) in
                            [("vcs-log-last-day", 1), ("vcs-log-last-week", 7), ("vcs-log-last-month", 30)]
                        {
                            if ui.selectable_label(false, t!(name)).clicked() {
                                let since = now - days * 86_400;
                                actions.push(GitAction::Run("vcs.log.filter", json!({ "date": { "since": since } })));
                                ui.close();
                            }
                        }
                        ui.separator();
                        if ui.selectable_label(false, t!("vcs-log-select")).clicked() {
                            let day = |time: Option<i64>| {
                                time.map(|time| {
                                    let (year, month, day) =
                                        format::civil((time + i64::from(offset)).div_euclid(86_400));
                                    format!("{year:04}-{month:02}-{day:02}")
                                })
                                .unwrap_or_default()
                            };
                            self.date_dialog = Some((day(since), day(until), false));
                            ui.close();
                        }
                    }
                    _ => {
                        if ui.selectable_label(filters.paths.is_empty(), t!("vcs-log-paths-all")).clicked() {
                            actions.push(GitAction::Run("vcs.log.filter", json!({ "paths": [] })));
                            ui.close();
                        }
                        if ui.selectable_label(false, t!("vcs-log-paths-select")).clicked() {
                            self.paths_dialog = root.map(|root| paths::PathsDialog::new(root, &filters.paths));
                            ui.close();
                        }
                        if !self.view.recent_paths.is_empty() {
                            ui.separator();
                            for set in &self.view.recent_paths {
                                let on = *set == filters.paths;
                                if ui.selectable_label(on, set.join(", ")).clicked() {
                                    actions.push(GitAction::Run("vcs.log.filter", json!({ "paths": set })));
                                    ui.close();
                                }
                            }
                        }
                        ui.separator();
                        let field = ui.add(
                            egui::TextEdit::singleline(&mut self.path_input)
                                .hint_text(t!("vcs-log-path-hint"))
                                .desired_width(220.0),
                        );
                        if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                            let path = self.path_input.trim().trim_matches('/').to_owned();
                            if !path.is_empty() {
                                let mut paths = filters.paths.clone();
                                paths.push(path);
                                actions.push(GitAction::Run("vcs.log.filter", json!({ "paths": paths })));
                            }
                            self.path_input.clear();
                        }
                    }
                });
            });
        }
        if x < right - 30.0 {
            let sort = Rect::from_center_size(pos2(x + 13.0, y), Vec2::splat(26.0));
            let response = ui.interact(sort, ui.scope_id().with("git-sort"), Sense::click());
            hover(ui, &response, p);
            hint(ui, &response, &t!("vcs-log-sort"), None, Side::Below);
            glyph_sort(ui.painter(), sort.shrink(6.5), p.text_secondary);
            egui::Popup::menu(&response).show(|ui| {
                ui.set_min_width(200.0);
                for (order, name) in
                    [(LogSort::Date, "vcs-log-sort-date"), (LogSort::Topological, "vcs-log-sort-topological")]
                {
                    if ui.selectable_label(filters.sort == order, t!(name)).clicked() {
                        actions.push(GitAction::Run("vcs.log.filter", json!({ "sort": order })));
                        ui.close();
                    }
                }
            });
        }
        if let Some(goto) = &mut self.goto {
            let rect = Rect::from_min_max(pos2(field.left(), field.top()), pos2(right - 2.0, field.bottom()));
            ui.painter().rect_filled(rect, 5, p.island);
            ui.painter().rect_stroke(rect, 5, Stroke::new(1.0, p.accent), egui::StrokeKind::Inside);
            let inner = rect.shrink2(vec2(10.0, 0.0));
            let edit = ui.put(
                inner,
                egui::TextEdit::singleline(goto)
                    .id(ui.scope_id().with("git-goto-field"))
                    .frame(egui::Frame::NONE)
                    .align(Align2::LEFT_CENTER)
                    .font(theme::regular(13.0))
                    .hint_text(t!("vcs-log-goto-hint"))
                    .desired_width(inner.width()),
            );
            if !edit.has_focus() && !edit.lost_focus() {
                edit.request_focus();
            }
            let (enter, escape) = ui.input(|input| (input.key_pressed(Key::Enter), input.key_pressed(Key::Escape)));
            if edit.lost_focus() && enter && !goto.trim().is_empty() {
                actions.push(GitAction::Run("vcs.log.select", json!({ "id": goto.trim() })));
                self.goto = None;
            } else if escape || (edit.lost_focus() && !enter) {
                self.goto = None;
            }
        }
        ui.painter().hline(bar.x_range(), bar.bottom(), Stroke::new(1.0, p.frame));
    }

    fn commit_list(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        info: &zen_workbench::vcs::RepositoryInfo,
        actions: &mut Vec<GitAction>,
    ) {
        if let Some(error) = &self.error {
            message(ui, p, t!("vcs-log-invalid-regex", error = error.as_str()));
            return;
        }
        if self.commits.is_empty() {
            message(ui, p, t!("vcs-log-empty"));
            return;
        }
        let lanes = self
            .commits
            .iter()
            .flat_map(|commit| commit.up.iter().chain(&commit.down).map(|edge| edge.from.max(edge.to)))
            .max()
            .unwrap_or(0)
            + 1;
        let graph_width = (lanes as f32 * LANE + 16.0).min(220.0);
        let selected = active_selection(self);
        let reveal = if selected != self.shown_selection {
            self.shown_selection = selected.clone();
            !std::mem::take(&mut self.clicked)
        } else {
            false
        };
        let reveal = reveal
            .then(|| selected.as_ref().and_then(|id| self.commits.iter().position(|commit| &commit.id == id)))
            .flatten();
        let mut area =
            egui::ScrollArea::vertical().id_salt(("git-commits", self.view.active)).auto_shrink([false, false]);
        if let Some(row) = reveal {
            area = area.vertical_scroll_offset((row as f32 - 3.0).max(0.0) * ROW);
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs() as i64);
        let upstreams: std::collections::HashMap<String, String> = self
            .branches
            .local
            .iter()
            .filter_map(|branch| branch.upstream.clone().map(|upstream| (branch.name.clone(), upstream)))
            .collect();
        let upstream = |name: &str| upstreams.get(name).cloned();
        let me = (self.view.user_name.clone(), self.view.user_email.clone());
        let mut load_more = false;
        let head = info.commit.clone();
        ui.spacing_mut().item_spacing.y = 0.0;
        area.show_rows(ui, ROW, self.commits.len() + usize::from(self.more), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let width = ui.available_width();
            for index in range {
                let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                let Some(commit) = self.commits.get(index) else {
                    load_more = true;
                    ui.painter().text(
                        rect.left_center() + vec2(12.0, 0.0),
                        Align2::LEFT_CENTER,
                        t!("vcs-log-loading"),
                        theme::regular(12.5),
                        p.text_secondary,
                    );
                    continue;
                };
                let is_selected = selected.as_ref() == Some(&commit.id);
                crate::accessibility::item(&response, egui::Role::ListBoxOption, is_selected, &commit.subject);
                let background = if is_selected {
                    p.selection
                } else if response.hovered() {
                    p.hover
                } else {
                    p.island
                };
                if background != p.island {
                    ui.painter().rect_filled(rect, 0, background);
                }
                let on_head = head.as_ref() == Some(&commit.id);
                draw_graph(
                    ui.painter(),
                    p,
                    Rect::from_min_size(rect.min, vec2(graph_width, ROW)),
                    commit,
                    on_head,
                    background,
                );
                let date_left = rect.right() - DATE;
                let author_left = (date_left - AUTHOR).max(rect.left() + graph_width + 120.0);
                let painter = ui.painter().with_clip_rect(rect);
                let date = format::relative_date(commit.time, now, self.local_offset);
                let ink = if commit.parents.len() > 1 { p.text_secondary } else { p.text };
                let date_galley =
                    painter.layout_job(super::vcs::truncated(&date, theme::regular(13.0), ink, DATE - 12.0));
                painter.galley(pos2(date_left, rect.center().y - date_galley.size().y / 2.0), date_galley, ink);
                let mine = me.0.as_ref().is_some_and(|name| *name == commit.author)
                    || me.1.as_ref().is_some_and(|email| *email == commit.email);
                let author_font = if mine { theme::semibold(13.0) } else { theme::regular(13.0) };
                let author = painter.layout_job(super::vcs::truncated(
                    &commit.author,
                    author_font,
                    ink,
                    date_left - author_left - 12.0,
                ));
                painter.galley(pos2(author_left, rect.center().y - author.size().y / 2.0), author, ink);
                let left = rect.left() + graph_width + 6.0;
                let room = author_left - 12.0 - left;
                let labels = format::row_labels(&commit.labels, self.branches.current.as_deref(), &upstream);
                let label_font = theme::regular(12.5);
                let name = |text: &str, width: f32| {
                    painter.layout_job(super::vcs::truncated(text, label_font.clone(), p.text_secondary, width))
                };
                let widths: Vec<f32> =
                    labels.iter().map(|label| 16.0 + name(&label.text, LABEL_NAME).size().x).collect();
                let more_width =
                    painter.layout_no_wrap("+99".to_owned(), label_font.clone(), p.text_secondary).size().x;
                let natural = painter.layout_no_wrap(commit.subject.clone(), theme::regular(13.0), ink).size().x;
                let layout = format::labels_layout(room, natural, &widths, more_width);
                let subject = painter.layout_job(super::vcs::truncated(
                    &commit.subject,
                    theme::regular(13.0),
                    ink,
                    layout.subject,
                ));
                let subject_right = left + subject.size().x;
                let elided = subject.elided;
                painter.galley(pos2(left, rect.center().y - subject.size().y / 2.0), subject, ink);
                let mut x = subject_right + format::LABEL_GAP;
                let mut tucked: Vec<&str> = Vec::new();
                for (label, width) in labels.iter().zip(&layout.shown) {
                    let color = match label.kind {
                        format::LabelKind::Current | format::LabelKind::Local => p.vcs_current_branch,
                        _ => p.text_secondary,
                    };
                    let icon = Rect::from_center_size(pos2(x + 7.0, rect.center().y), Vec2::splat(12.0));
                    glyph_label(&painter, icon, color, background);
                    let galley = name(&label.text, (width - 16.0).min(LABEL_NAME));
                    if galley.elided {
                        tucked.push(&label.text);
                    }
                    painter.galley(pos2(x + 16.0, rect.center().y - galley.size().y / 2.0), galley, p.text_secondary);
                    x += width + format::LABEL_GAP;
                }
                if layout.hidden > 0 {
                    let more = painter.layout_no_wrap(format!("+{}", layout.hidden), label_font, p.text_secondary);
                    let width = more.size().x;
                    painter.galley(pos2(x, rect.center().y - more.size().y / 2.0), more, p.text_secondary);
                    x += width;
                    tucked.extend(labels[layout.shown.len()..].iter().map(|label| label.text.as_str()));
                }
                let over_labels = response.hover_pos().is_some_and(|at| at.x >= subject_right && at.x <= x);
                if over_labels && !tucked.is_empty() {
                    response.clone().on_hover_text(tucked.join("\n"));
                } else if elided {
                    response.clone().on_hover_text(&commit.subject);
                }
                if response.clicked() && !is_selected {
                    actions.push(GitAction::Run("vcs.log.select", json!({ "id": commit.id })));
                }
                let id = commit.id.clone();
                response.context_menu(|ui| {
                    ui.set_min_width(220.0);
                    customization::popup(
                        ui,
                        customization::GIT_LOG_POPUP,
                        |_| true,
                        |ui, slot| {
                            if ui.button(t!(slot)).clicked() {
                                actions.push(match slot {
                                    "vcs-log-cherry-pick" => {
                                        GitAction::Run("vcs.commit.cherryPick", json!({ "id": id }))
                                    }
                                    "keymap-view-vcs-checkoutRevision" => {
                                        GitAction::Run("vcs.revision.checkout", json!({ "revision": id }))
                                    }
                                    _ => GitAction::NewBranch(Some(id.clone())),
                                });
                                ui.close();
                            }
                        },
                    );
                });
            }
        });
        if load_more && self.more {
            self.limit += PAGE;
        }
    }

    fn date_range_dialog(&mut self, ui: &Ui, p: &IdePalette, actions: &mut Vec<GitAction>) {
        let Some((from, to, invalid)) = &mut self.date_dialog else { return };
        let (mut ok, mut cancel) = (false, false);
        let modal =
            egui::Modal::new(ui.scope_id().with("git-date-range")).frame(super::dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(360.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(t!("vcs-log-date-title")).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(14.0);
                for (label, value) in [("vcs-log-date-since", &mut *from), ("vcs-log-date-until", &mut *to)] {
                    ui.horizontal(|ui| {
                        ui.add_sized(vec2(60.0, 28.0), egui::Label::new(egui::RichText::new(t!(label)).color(p.text)));
                        ui.add(
                            egui::TextEdit::singleline(value)
                                .hint_text(t!("vcs-log-date-hint"))
                                .desired_width(200.0)
                                .margin(egui::Margin::symmetric(8, 5)),
                        );
                    });
                    ui.add_space(6.0);
                }
                if *invalid {
                    ui.label(
                        egui::RichText::new(t!("vcs-log-date-invalid")).font(theme::regular(12.0)).color(p.error_text),
                    );
                }
                ui.add_space(14.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if super::vcs::primary_button(ui, p, &t!("vcs-ok")).clicked() {
                        ok = true;
                    }
                    if super::vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                        cancel = true;
                    }
                });
                if ui.input(|input| input.key_pressed(Key::Enter)) {
                    ok = true;
                }
            });
        if ok {
            let offset = i64::from(self.local_offset);
            let parse = |text: &str| -> Result<Option<i64>, ()> {
                if text.trim().is_empty() {
                    return Ok(None);
                }
                format::parse_day(text).map(Some).ok_or(())
            };
            match (parse(from), parse(to)) {
                (Ok(since), Ok(until)) => {
                    let since = since.map(|day| day * 86_400 - offset);
                    let until = until.map(|day| day * 86_400 + 86_399 - offset);
                    actions
                        .push(GitAction::Run("vcs.log.filter", json!({ "date": { "since": since, "until": until } })));
                    self.date_dialog = None;
                }
                _ => *invalid = true,
            }
        } else if cancel || modal.should_close() {
            self.date_dialog = None;
        }
    }
}

fn date_label(since: Option<i64>, until: Option<i64>, offset: i32) -> Option<String> {
    let day = |time: i64| format::date_and_time(time + i64::from(offset)).0;
    match (since, until) {
        (None, None) => None,
        (Some(since), None) => Some(t!("vcs-log-since", date = day(since).as_str())),
        (None, Some(until)) => Some(t!("vcs-log-until", date = day(until).as_str())),
        (Some(since), Some(until)) => Some(format!("{} – {}", day(since), day(until))),
    }
}

fn toggled(list: &[String], item: &str) -> Vec<String> {
    if list.iter().any(|known| known == item) {
        list.iter().filter(|known| *known != item).cloned().collect()
    } else {
        list.iter().cloned().chain(std::iter::once(item.to_owned())).collect()
    }
}

#[allow(clippy::too_many_arguments)]
fn filter_button(
    ui: &Ui,
    p: &IdePalette,
    x: f32,
    y: f32,
    text_width: f32,
    salt: &str,
    name: &str,
    value: Option<&str>,
) -> (egui::Response, bool, f32) {
    let text = match value {
        Some(value) => t!("vcs-log-filter-value", filter = name, value = value),
        None => name.to_owned(),
    };
    let color = if value.is_some() { p.text } else { p.text_secondary };
    let galley =
        ui.painter().layout_job(super::vcs::truncated(&text, theme::regular(13.0), color, text_width.max(20.0) + 0.5));
    let width = galley.size().x + 30.0;
    let rect = Rect::from_min_size(pos2(x, y - 13.0), vec2(width, 26.0));
    let response = ui.interact(rect, ui.scope_id().with(("git-filter", salt)), Sense::click());
    crate::accessibility::button(&response, galley.text());
    hover(ui, &response, p);
    if galley.elided && !egui::Popup::is_any_open(ui.ctx()) {
        hint(ui, &response, &text, None, Side::Below);
    }
    ui.painter().galley(pos2(rect.left() + 6.0, y - galley.size().y / 2.0), galley, color);
    let mark = Rect::from_center_size(pos2(rect.right() - 12.0, y), Vec2::splat(11.0));
    let mut cleared = false;
    if value.is_some() {
        let hit = ui.interact(mark.expand(3.0), ui.scope_id().with(("git-filter-clear", salt)), Sense::click());
        let r = mark.shrink(2.0);
        let stroke = Stroke::new(1.2, if hit.hovered() { p.text } else { p.text_secondary });
        ui.painter().line_segment([r.left_top(), r.right_bottom()], stroke);
        ui.painter().line_segment([r.right_top(), r.left_bottom()], stroke);
        hint(ui, &hit, &t!("vcs-log-clear-filter"), None, Side::Below);
        cleared = hit.clicked();
    } else {
        crate::icons::chevron_down(ui.painter(), mark, p.text_secondary);
    }
    (response, cleared, width)
}

fn message(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(egui::RichText::new(text).color(p.text_secondary));
    });
}

fn draw_graph(
    painter: &egui::Painter,
    p: &IdePalette,
    rect: Rect,
    commit: &LogCommit,
    head: bool,
    background: Color32,
) {
    let x = |lane: usize| rect.left() + 15.0 + lane as f32 * LANE;
    let (top, middle, bottom) = (rect.top(), rect.center().y, rect.bottom());
    let color = |index: usize| p.graph[index % p.graph.len()];
    let painter = painter.with_clip_rect(rect);
    for edge in &commit.up {
        line(&painter, pos2(x(edge.from), top), pos2(x(edge.to), middle), color(edge.color));
    }
    for edge in &commit.down {
        line(&painter, pos2(x(edge.from), middle), pos2(x(edge.to), bottom), color(edge.color));
    }
    let dot = pos2(x(commit.lane), middle);
    if head {
        painter.circle_filled(dot, 6.5, background);
        painter.circle_stroke(dot, 5.5, Stroke::new(1.6, color(commit.color)));
        painter.circle_filled(dot, 2.6, color(commit.color));
    } else {
        painter.circle_filled(dot, 3.6, color(commit.color));
    }
}

pub(super) fn line(painter: &egui::Painter, from: egui::Pos2, to: egui::Pos2, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    if (from.x - to.x).abs() < 0.5 {
        painter.line_segment([from, to], stroke);
    } else {
        let bend = egui::epaint::CubicBezierShape::from_points_stroke(
            [from, pos2(from.x, (from.y + to.y) / 2.0), pos2(to.x, (from.y + to.y) / 2.0), to],
            false,
            Color32::TRANSPARENT,
            stroke,
        );
        painter.add(bend);
    }
}

#[cfg(test)]
mod tests {
    use egui::accesskit::TreeUpdate;

    use super::*;

    fn labels(tree: &TreeUpdate) -> Vec<String> {
        tree.nodes.iter().filter_map(|(_, node)| node.label().or(node.value()).map(str::to_owned)).collect()
    }

    #[test]
    fn the_paths_filter_offers_select_folders_and_the_recent_paths() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("internal/auth")).unwrap();
        let palette = IdePalette::zen_dark();
        let languages: &[&str] = if std::env::var_os("ZEN_UI_SNAPSHOTS").is_some() { &["en", "ru"] } else { &["en"] };
        zen_i18n::register(crate::toolbox::LOCALES);
        for language in languages {
            zen_i18n::set_language(Some(language));
            let mut window = GitWindow::default();
            let filters =
                LogFilters { paths: vec!["internal".to_owned()], since: Some(1_767_225_600), ..LogFilters::default() };
            window.view.tabs = vec![LogTab { filters, ..LogTab::default() }];
            window.view.recent_paths =
                vec![vec!["internal".to_owned()], vec!["cmd/tool".to_owned(), "docs".to_owned()]];
            let mut screen = crate::offscreen::Offscreen::new(vec2(820.0, 230.0), 2.0);
            palette.apply(screen.context());
            let mut actions = Vec::new();
            let mut draw = |ui: &mut Ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
                window.filter_bar(ui, &palette, Some(root.path()), &mut actions);
            };
            screen.render(&mut draw);
            let tree = screen.accessibility_tree(&mut draw);
            let name = t!("vcs-log-paths");
            let bounds = tree
                .nodes
                .iter()
                .find(|(_, node)| node.label().is_some_and(|label| label.starts_with(name.as_str())))
                .and_then(|(_, node)| node.bounds())
                .unwrap_or_else(|| panic!("no Paths filter in {:?}", labels(&tree)));
            let at = pos2((bounds.x0 + bounds.x1) as f32 / 2.0 - 8.0, (bounds.y0 + bounds.y1) as f32 / 2.0);
            let click = |pressed: bool| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            screen.step(vec![egui::Event::PointerMoved(at)], &mut draw);
            screen.step(vec![click(true)], &mut draw);
            screen.step(vec![click(false)], &mut draw);
            let image = screen.render(&mut draw);
            let shown = labels(&screen.accessibility_tree(&mut draw));
            let items = [
                t!("vcs-log-paths-all"),
                t!("vcs-log-paths-select"),
                "internal".to_owned(),
                "cmd/tool, docs".to_owned(),
            ];
            for item in items {
                assert!(shown.contains(&item), "{item} in {shown:?}");
            }
            if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") {
                let file = std::path::Path::new(&dir).join(format!("paths-popup-{language}.png"));
                crate::offscreen::write_png(&file, &image).unwrap();
            }
        }
        zen_i18n::set_language(Some("en"));
    }
}

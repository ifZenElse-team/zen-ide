mod customize;
mod learn;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Align2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use zen_i18n::t;

use super::app::{Overview, Request};
use super::clone_dialog::CloneDialog;
use super::glyphs::*;
use super::manage_settings::{ImportDialog, ImportEnd};
use super::new_project_dialog::{NewProjectDialog, Outcome, pick_sources, sources_notice};
use super::settings::{Dispatch, IdeSettings, PageId, SECTIONS, SettingsContext, SettingsDialog};
use super::{IdePalette, IdeProduct, Side, draw_notice, project_row};
use crate::{accessibility, theme};

pub(super) use learn::guide_url;

pub(super) fn title(product: &IdeProduct) -> String {
    t!("ide-welcome-title", product = product.name)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WelcomeSection {
    #[default]
    Projects,
    Customize,
    Plugins,
    Learn,
}

impl WelcomeSection {
    const ALL: [Self; 4] = [Self::Projects, Self::Customize, Self::Plugins, Self::Learn];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Projects => "projects",
            Self::Customize => "customize",
            Self::Plugins => "plugins",
            Self::Learn => "learn",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Projects => "ide-welcome-projects",
            Self::Customize => "ide-welcome-customize",
            Self::Plugins => "ide-welcome-plugins",
            Self::Learn => "ide-welcome-learn",
        }
    }

    pub(super) fn from_params(params: &serde_json::Value) -> Result<Option<Self>, String> {
        let Some(name) = params.get("section").and_then(serde_json::Value::as_str) else { return Ok(None) };
        Self::ALL
            .into_iter()
            .find(|section| section.name() == name)
            .map(Some)
            .ok_or_else(|| format!("no welcome section `{name}`; the sections are projects, customize, plugins, learn"))
    }
}

const SIDEBAR: f32 = 226.0;
const TITLE_BAR: f32 = 30.0;
const BUTTONS: [&str; 3] = ["ide-welcome-new", "ide-welcome-open", "ide-welcome-clone"];
const ROW: f32 = 30.0;
const GAP: f32 = 8.0;
const SEARCH_GAP: f32 = 12.0;
const SEARCH_MIN: f32 = 120.0;
const KOZO_SIDE: f32 = 80.0;

#[derive(Debug, PartialEq)]
struct HeaderLayout {
    search: Rect,
    buttons: Vec<Rect>,
    bottom: f32,
}

fn header_layout(widths: &[f32], left: f32, right: f32, end: f32, top: f32) -> HeaderLayout {
    let row = widths.iter().sum::<f32>() + GAP * widths.len().saturating_sub(1) as f32;
    let start = end - row;
    if start - SEARCH_GAP - left >= SEARCH_MIN {
        let mut x = start;
        let buttons = widths
            .iter()
            .map(|width| {
                let button = Rect::from_min_size(pos2(x, top), vec2(*width, ROW));
                x = button.right() + GAP;
                button
            })
            .collect();
        let search = Rect::from_min_max(pos2(left, top), pos2(start - SEARCH_GAP, top + ROW));
        return HeaderLayout { search, buttons, bottom: top + ROW };
    }
    let search = Rect::from_min_max(pos2(left, top), pos2((end + GAP - SEARCH_GAP).max(left), top + ROW));
    let (mut x, mut y) = (left, top + ROW + GAP);
    let buttons = widths
        .iter()
        .map(|width| {
            if x > left && x + width > right {
                (x, y) = (left, y + ROW + GAP);
            }
            let button = Rect::from_min_size(pos2(x, y), vec2(*width, ROW));
            x = button.right() + GAP;
            button
        })
        .collect();
    HeaderLayout { search, buttons, bottom: y + ROW }
}

pub(super) struct WelcomeView {
    product: Arc<IdeProduct>,
    pub(super) palette: IdePalette,
    search: String,
    notice: Option<(String, f64)>,
    settings: SettingsDialog,
    pub(super) new_project: Option<NewProjectDialog>,
    wanted_project: Option<(Option<String>, bool)>,
    pub(super) clone: Option<CloneDialog>,
    wanted_clone: Option<bool>,
    wanted_sources: bool,
    pub(super) section: WelcomeSection,
    customize: customize::Customize,
    wanted_import: Option<serde_json::Value>,
    wanted_settings: Option<Option<PageId>>,
    pub(super) import: Option<ImportDialog>,
    facts: RecentFacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProjectFacts {
    pub(super) branch: Option<String>,
    pub(super) exists: bool,
}

#[derive(Default)]
struct RecentFacts {
    known: Arc<Mutex<HashMap<PathBuf, ProjectFacts>>>,
    asked: Vec<PathBuf>,
    focused: bool,
}

impl RecentFacts {
    fn follow(&mut self, ctx: &egui::Context, projects: Vec<PathBuf>) {
        let focused = ctx.input(|input| input.viewport().focused.unwrap_or(input.focused));
        let was_focused = std::mem::replace(&mut self.focused, focused);
        let came_back = focused && !was_focused;
        if projects == self.asked && !came_back {
            return;
        }
        self.asked.clone_from(&projects);
        let known = Arc::clone(&self.known);
        let ctx = ctx.clone();
        let reader = std::thread::Builder::new().name("zen-recent-projects".to_owned());
        let _ = reader.spawn(move || {
            for project in projects {
                let facts = ProjectFacts { branch: super::git_branch(&project), exists: project.is_dir() };
                known.lock().unwrap_or_else(std::sync::PoisonError::into_inner).insert(project, facts);
            }
            ctx.request_repaint();
        });
    }

    fn of(&self, project: &Path) -> ProjectFacts {
        let known = self.known.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        known.get(project).cloned().unwrap_or(ProjectFacts { branch: None, exists: true })
    }
}

impl WelcomeView {
    pub(super) fn new(product: Arc<IdeProduct>) -> Self {
        Self {
            product,
            palette: IdePalette::zen_dark(),
            search: String::new(),
            notice: None,
            facts: RecentFacts::default(),
            settings: SettingsDialog::default(),
            new_project: None,
            wanted_project: None,
            clone: None,
            wanted_clone: None,
            wanted_sources: false,
            section: WelcomeSection::default(),
            customize: customize::Customize::default(),
            wanted_import: None,
            wanted_settings: None,
            import: None,
        }
    }

    pub(super) fn state(&self) -> serde_json::Value {
        let mut popups = Vec::new();
        if let Some(page) = self.settings.open_page() {
            popups.push(format!("settings:{}", page.name()));
        }
        if self.new_project.is_some() {
            popups.push("newProject".to_owned());
        }
        if self.clone.is_some() {
            popups.push("clone".to_owned());
        }
        if let Some(dialog) = &self.import {
            popups.push(dialog.name().to_owned());
        }
        serde_json::json!({ "welcome": { "section": self.section.name() }, "popups": popups })
    }

    pub(super) fn want_import(&mut self, params: serde_json::Value) {
        self.wanted_import = Some(params);
    }

    pub(super) fn want_settings(&mut self, page: Option<&str>) {
        self.wanted_settings = Some(page.and_then(|page| PageId::from_name(page, SECTIONS)));
    }

    pub(super) fn set_palette(&mut self, palette: IdePalette) {
        self.palette = palette;
    }

    pub(super) fn show(&mut self, ui: &mut Ui, overview: &Overview, dispatch: &mut Dispatch<'_>) -> Vec<Request> {
        let mut requests = Vec::new();
        let p = self.palette;
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0, p.frame);

        let title_strip = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + TITLE_BAR));
        let strip = super::appearance::title_strip(ui, title_strip, ui.scope_id().with("welcome-drag"));
        accessibility::name_with(&strip, egui::Role::TitleBar, || title(&self.product));
        ui.painter().text(
            title_strip.center(),
            Align2::CENTER_CENTER,
            title(&self.product),
            theme::semibold(13.0),
            p.text,
        );

        let sidebar =
            Rect::from_min_max(pos2(rect.left(), title_strip.bottom()), pos2(rect.left() + SIDEBAR, rect.bottom()));
        ui.painter().rect_filled(sidebar, 0, p.island);
        ui.painter().line_segment([sidebar.right_top(), sidebar.right_bottom()], Stroke::new(1.0, p.hover));
        self.sidebar(ui, sidebar);

        let main = Rect::from_min_max(pos2(sidebar.right(), title_strip.bottom()), rect.max);
        match self.section {
            WelcomeSection::Projects => self.main(ui, main, overview, &mut requests),
            WelcomeSection::Customize => self.customize(ui, main, overview, dispatch, &mut requests),
            WelcomeSection::Plugins => {
                if !self.settings.is_open() {
                    let mut pane = ui.new_child(egui::UiBuilder::new().max_rect(main.shrink(20.0)));
                    if self.settings.embedded_plugins(&mut pane, &p, dispatch) {
                        requests.push(Request::Restart);
                    }
                }
            }
            WelcomeSection::Learn => self.learn(ui, main),
        }
        if let Some(page) = self.wanted_settings.take() {
            match page {
                Some(page) => self.settings.open_at(page),
                None => self.settings.open(),
            }
        }
        if ui.input_mut(|input| {
            input.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Comma))
        }) {
            self.settings.open();
        }
        let context = SettingsContext {
            settings: &overview.settings,
            mcp_url: overview.automation.as_deref(),
            product_id: self.product.id,
            executable: self
                .settings
                .is_open()
                .then(|| {
                    let found = crate::ide::launcher_dialog::launcher_on_path(self.product.id);
                    crate::ide::launcher_dialog::program_for_commands(
                        std::env::current_exe().ok(),
                        self.product.id,
                        found.as_deref(),
                    )
                })
                .flatten(),
            form: None,
            sections: SECTIONS,
            language_servers: None,
            declared: &[],
            folding: &[],
        };
        if let Some(settings) = self.settings.show(ui, &p, &context, Some(dispatch)) {
            requests.push(Request::Settings(Box::new(settings)));
        }
        self.new_project_dialog(ui, dispatch, &mut requests);
        self.clone_dialog(ui, dispatch, &mut requests);
        self.sources_dialog(ui, dispatch, &mut requests);
        self.import_dialog(ui, dispatch, &mut requests);
        draw_notice(ui, &p, &mut self.notice, 16.0);
        requests
    }

    pub(super) fn want_new_project(&mut self, generator: Option<String>, by_agent: bool) {
        self.wanted_project = Some((generator, by_agent));
    }

    pub(super) fn driven_by_agent(&self) -> bool {
        self.new_project.as_ref().is_some_and(|dialog| dialog.by_agent)
            || self.clone.as_ref().is_some_and(|dialog| dialog.by_agent)
            || matches!(self.wanted_project, Some((_, true)))
            || self.wanted_clone == Some(true)
    }

    pub(super) fn want_clone(&mut self, by_agent: bool) {
        self.wanted_clone = Some(by_agent);
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn want_sources(&mut self) {
        self.wanted_sources = true;
    }

    fn import_dialog(&mut self, ui: &mut Ui, dispatch: &mut Dispatch<'_>, requests: &mut Vec<Request>) {
        if let Some(params) = self.wanted_import.take() {
            match ImportDialog::open(dispatch, &params) {
                Ok(dialog) => self.import = Some(dialog),
                Err(error) => self.post(ui, error),
            }
        }
        let p = self.palette;
        let Some(dialog) = &mut self.import else { return };
        let end = dialog.show(ui, &p, self.product.name, dispatch);
        if end.is_some() {
            self.import = None;
            self.customize.refresh();
        }
        if end == Some(ImportEnd::Restart) {
            requests.push(Request::Restart);
        }
    }

    fn clone_dialog(&mut self, ui: &mut Ui, dispatch: &mut Dispatch<'_>, requests: &mut Vec<Request>) {
        if let Some(by_agent) = self.wanted_clone.take() {
            match CloneDialog::open(dispatch) {
                Ok(mut dialog) => {
                    dialog.by_agent = by_agent;
                    self.clone = Some(dialog);
                }
                Err(error) => self.post(ui, error),
            }
        }
        let p = self.palette;
        let Some(dialog) = &mut self.clone else { return };
        let by_agent = dialog.by_agent;
        match dialog.show(ui, &p, dispatch) {
            Some(Outcome::Created(path)) => {
                self.clone = None;
                requests.push(Request::open_created(by_agent, path));
            }
            Some(_) => self.clone = None,
            None => {}
        }
    }

    fn sources_dialog(&mut self, ui: &mut Ui, dispatch: &mut Dispatch<'_>, requests: &mut Vec<Request>) {
        if !std::mem::take(&mut self.wanted_sources) {
            return;
        }
        let Some(path) = pick_sources(&t!("ide-module-sources-title")) else { return };
        let params = serde_json::json!({ "path": path });
        match dispatch("project.fromSources.detect", params) {
            Ok(found) => {
                self.post(ui, sources_notice(&found));
                if let Some(root) = found["root"].as_str() {
                    requests.push(Request::Open(root.into()));
                }
            }
            Err(error) => self.post(ui, error),
        }
    }

    fn new_project_dialog(&mut self, ui: &mut Ui, dispatch: &mut Dispatch<'_>, requests: &mut Vec<Request>) {
        if let Some((generator, by_agent)) = self.wanted_project.take() {
            match NewProjectDialog::open(dispatch, generator.as_deref()) {
                Ok(mut dialog) => {
                    dialog.by_agent = by_agent;
                    self.new_project = Some(dialog);
                }
                Err(error) => self.post(ui, error),
            }
        }
        let p = self.palette;
        let Some(dialog) = &mut self.new_project else { return };
        if self.settings.is_open() {
            dialog.settings_changed();
            return;
        }
        let by_agent = dialog.by_agent;
        match dialog.show(ui, &p, dispatch) {
            Some(Outcome::Created(path)) => {
                self.new_project = None;
                requests.push(Request::open_created(by_agent, path));
            }
            Some(Outcome::Cancelled | Outcome::Module { .. }) => self.new_project = None,
            Some(Outcome::MorePlugins) => {
                self.settings.open_at(PageId::Plugins);
                self.settings.show_plugins_tab("marketplace");
            }
            Some(Outcome::GeneratorSettings) => self.settings.open_at(PageId::SystemSettings),
            None => {}
        }
    }

    pub(super) fn post(&mut self, ui: &Ui, message: String) {
        self.notice = Some((message, ui.input(|input| input.time)));
    }

    fn sidebar(&mut self, ui: &mut Ui, sidebar: Rect) {
        let p = self.palette;
        let mark = Rect::from_min_size(pos2(sidebar.left() + 18.0, sidebar.top() + 16.0), Vec2::splat(32.0));
        crate::icons::product_mark(ui, mark, self.product.id, self.product.name, false);
        ui.painter().text(
            pos2(mark.right() + 12.0, mark.top() + 1.0),
            Align2::LEFT_TOP,
            self.product.name,
            theme::semibold(14.0),
            p.text,
        );
        ui.painter().text(
            pos2(mark.right() + 12.0, mark.bottom() + 1.0),
            Align2::LEFT_BOTTOM,
            self.product.version,
            theme::regular(12.5),
            p.text_secondary,
        );

        let mut y = mark.bottom() + 22.0;
        for section in WelcomeSection::ALL {
            let item = Rect::from_min_size(pos2(sidebar.left() + 12.0, y), vec2(sidebar.width() - 24.0, 30.0));
            let response = ui.interact(item, ui.scope_id().with(("welcome-section", section.name())), Sense::click());
            let selected = section == self.section;
            if selected {
                ui.painter().rect_filled(item, 6, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(item, 6, p.hover);
            }
            ui.painter().text(
                pos2(item.left() + 12.0, item.center().y),
                Align2::LEFT_CENTER,
                t!(section.label()),
                theme::regular(13.5),
                p.text,
            );
            if response.clicked() {
                self.section = section;
                self.customize.refresh();
            }
            accessibility::name_with(&response, egui::Role::Tab, || t!(section.label()));
            accessibility::edit(&response, |node| node.set_selected(selected));
            y += 34.0;
        }

        let gear = Rect::from_center_size(pos2(sidebar.left() + 26.0, sidebar.bottom() - 24.0), Vec2::splat(26.0));
        let side = KOZO_SIDE.min(gear.top() - GAP - (y + GAP));
        let kozo = Rect::from_center_size(pos2(sidebar.center().x, gear.top() - GAP - side / 2.0), Vec2::splat(side));
        crate::mascot::show(ui, kozo, self.product.id, zen_mascot::State::Idle, KOZO_SIDE / zen_mascot::SIZE as f32);
        let response = ui.interact(gear, ui.scope_id().with("welcome-settings"), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(gear, 6, p.hover);
        }
        glyph_settings(ui.painter(), gear.shrink(6.0), p.text_secondary);
        super::hint(
            ui,
            &response,
            &t!("settings-title"),
            crate::keymap::shortcut("view.settings.open").as_deref(),
            Side::Right,
        );
        if response.clicked() {
            self.settings.open();
        }
    }

    fn language_switch(&self, ui: &mut Ui, corner: egui::Pos2) -> (Rect, Option<String>) {
        let p = self.palette;
        let current = zen_i18n::language();
        let name = zen_i18n::LANGUAGES
            .iter()
            .find(|(code, _)| *code == current)
            .map_or_else(|| current.clone(), |(_, name)| (*name).to_owned());
        let galley = ui.painter().layout_no_wrap(name.clone(), theme::regular(13.5), p.text);
        let width = 8.0 + 14.0 + 6.0 + galley.size().x + 6.0 + 10.0 + 8.0;
        let rect = Rect::from_min_size(pos2(corner.x - width, corner.y), vec2(width, ROW));
        let response = ui.interact(rect, ui.scope_id().with("welcome-language"), Sense::click());
        let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&response));
        if response.hovered() || open {
            ui.painter().rect_filled(rect, 6, p.hover);
        }
        glyph_globe(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 15.0, rect.center().y), Vec2::splat(14.0)),
            p.text_secondary,
        );
        let text = pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(text, galley, p.text);
        crate::icons::chevron_down(
            ui.painter(),
            Rect::from_center_size(pos2(rect.right() - 13.0, rect.center().y), Vec2::splat(10.0)),
            p.text_secondary,
        );
        if !open {
            super::hint(ui, &response, &t!("ide-welcome-language"), None, Side::Below);
        }
        accessibility::name_with(&response, egui::Role::ComboBox, || t!("ide-welcome-language"));
        accessibility::edit(&response, |node| node.set_value(name.clone()));
        let mut chosen = None;
        egui::Popup::menu(&response).align(egui::RectAlign::BOTTOM_END).show(|ui| {
            let width = rect.width().max(140.0);
            for (code, language) in zen_i18n::LANGUAGES {
                let checked = *code == current;
                let (row, response) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
                if response.hovered() {
                    ui.painter().rect_filled(row, 5, p.selection);
                }
                if checked {
                    glyph_check(
                        ui.painter(),
                        Rect::from_center_size(pos2(row.left() + 14.0, row.center().y), Vec2::splat(11.0)),
                        p.text,
                    );
                }
                ui.painter().text(
                    pos2(row.left() + 30.0, row.center().y),
                    Align2::LEFT_CENTER,
                    *language,
                    theme::regular(13.0),
                    p.text,
                );
                accessibility::item(&response, egui::Role::MenuItemRadio, checked, language);
                if response.clicked() {
                    chosen = Some((*code).to_owned());
                    ui.close();
                }
            }
        });
        (rect, chosen)
    }

    fn main(&mut self, ui: &mut Ui, main: Rect, overview: &Overview, requests: &mut Vec<Request>) {
        let p = self.palette;
        let top = main.top() + 14.0;
        let (switch, chosen) = self.language_switch(ui, pos2(main.right() - 20.0, top));
        if let Some(language) = chosen.filter(|language| overview.settings.language.as_ref() != Some(language)) {
            requests.push(Request::Settings(Box::new(IdeSettings {
                language: Some(language),
                ..overview.settings.clone()
            })));
        }
        let galleys: Vec<_> =
            BUTTONS.iter().map(|label| ui.painter().layout_no_wrap(t!(label), theme::regular(13.5), p.text)).collect();
        let widths: Vec<f32> = galleys.iter().map(|galley| galley.size().x + 26.0).collect();
        let header = header_layout(&widths, main.left() + 20.0, main.right() - 20.0, switch.left() - GAP, top);
        for ((label, button), galley) in BUTTONS.into_iter().zip(header.buttons).zip(galleys) {
            let response = ui.interact(button, ui.scope_id().with(label), Sense::click());
            ui.painter().rect_filled(button, 6, if response.hovered() { p.hover } else { p.frame });
            ui.painter().rect_stroke(button, 6, Stroke::new(1.0, p.button_border), StrokeKind::Inside);
            ui.painter().galley(button.center() - galley.size() / 2.0, Arc::clone(&galley), p.text);
            if response.clicked() && label == "ide-welcome-new" {
                self.wanted_project = Some((None, false));
            } else if response.clicked() && label == "ide-welcome-clone" {
                self.wanted_clone = Some(false);
            } else if response.clicked() {
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(folder) = rfd::FileDialog::new().set_title(t!("ide-open-title")).pick_folder() {
                    requests.push(Request::Open(folder));
                }
            }
            accessibility::button(&response, galley.text());
        }
        let search = header.search;
        glyph_search(
            ui.painter(),
            Rect::from_center_size(pos2(search.left() + 10.0, search.center().y), Vec2::splat(14.0)),
            p.text_secondary,
        );
        let field = Rect::from_min_max(pos2(search.left() + 26.0, search.top() + 4.0), search.max);
        let search = ui.put(
            field,
            egui::TextEdit::singleline(&mut self.search)
                .hint_text(egui::RichText::new(t!("ide-welcome-search")).color(p.text_secondary))
                .frame(egui::Frame::NONE)
                .font(theme::regular(14.0)),
        );
        accessibility::edit(&search, |node| node.set_label(t!("ide-welcome-search")));
        let line = header.bottom + 14.0;
        ui.painter().line_segment([pos2(main.left(), line), pos2(main.right(), line)], Stroke::new(1.0, p.hover));

        let list =
            Rect::from_min_max(pos2(main.left() + 12.0, line + 8.0), pos2(main.right() - 12.0, main.bottom() - 8.0));
        self.facts.follow(ui.ctx(), overview.recent.iter().map(|project| project.path.clone()).collect());
        let query = self.search.trim().to_lowercase();
        let projects: Vec<_> = overview
            .recent
            .iter()
            .filter(|project| query.is_empty() || project.path.to_string_lossy().to_lowercase().contains(&query))
            .collect();
        if projects.is_empty() {
            let text = if overview.recent.is_empty() { t!("ide-welcome-empty") } else { t!("ide-welcome-no-match") };
            ui.painter().text(
                pos2(list.center().x, list.top() + 60.0),
                Align2::CENTER_CENTER,
                text,
                theme::regular(13.5),
                p.text_secondary,
            );
            return;
        }
        let mut list_ui = ui.new_child(egui::UiBuilder::new().max_rect(list));
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(&mut list_ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            for project in projects {
                let row = project_row(ui, &p, &project.path, false, Some(true), self.facts.of(&project.path));
                if row.remove {
                    requests.push(Request::RemoveRecent(project.path.clone()));
                } else if row.clicked {
                    requests.push(Request::Open(project.path.clone()));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;

    use serde_json::{Value, json};

    use crate::offscreen::Offscreen;

    struct Fixture {
        view: WelcomeView,
        screen: Offscreen,
        calls: RefCell<Vec<(String, Value)>>,
        requests: Vec<Request>,
    }

    impl Fixture {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let product = Arc::new(IdeProduct { name: "Test", id: "test", version: "0.0.0", ..IdeProduct::default() });
            Self {
                view: WelcomeView::new(product),
                screen: Offscreen::new(vec2(960.0, 650.0), 1.0),
                calls: RefCell::new(Vec::new()),
                requests: Vec::new(),
            }
        }

        fn answer(calls: &RefCell<Vec<(String, Value)>>, id: &str, params: Value) -> Result<Value, String> {
            calls.borrow_mut().push((id.to_owned(), params));
            Ok(match id {
                "ui.theme.list" => json!({
                    "current": "dusk",
                    "themes": [{ "id": "dusk", "name": "Dusk" }, { "id": "bad", "name": "Bad", "error": "broken" }],
                    "appearance": { "followSystem": false },
                }),
                "ui.keymap.list" => json!({ "current": "mine", "keymaps": [{ "id": "mine", "name": "Mine" }] }),
                "settings.ide.components" => json!({ "data": "/tmp", "components": [], "backups": "/tmp/backups" }),
                _ => Value::Null,
            })
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            let Self { view, screen, calls, requests } = self;
            screen.step(events, |ui| {
                let overview = Overview::default();
                let mut dispatch = |id: &str, params| Self::answer(calls, id, params);
                requests.extend(view.show(ui, &overview, &mut dispatch));
            });
        }

        fn tree(&mut self) -> egui::accesskit::TreeUpdate {
            let Self { view, screen, calls, .. } = self;
            screen.accessibility_tree(|ui| {
                let overview = Overview::default();
                let mut dispatch = |id: &str, params| Self::answer(calls, id, params);
                let _ = view.show(ui, &overview, &mut dispatch);
            })
        }

        fn click(&mut self, role: egui::Role, name: &str) {
            let tree = self.tree();
            let node =
                accessibility::testing::find(&tree, role, name).unwrap_or_else(|| panic!("no {role:?} named {name:?}"));
            let bounds = node.bounds().expect("bounds");
            let at = pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
            let button = |pressed| egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                self.frame(events);
            }
        }

        fn called(&self, id: &str) -> Option<Value> {
            self.calls.borrow().iter().rev().find(|(called, _)| called == id).map(|(_, params)| params.clone())
        }
    }

    #[test]
    fn the_left_column_has_four_sections_and_no_stub() {
        let mut fixture = Fixture::new();
        let tree = fixture.tree();
        for section in WelcomeSection::ALL {
            assert!(accessibility::testing::find(&tree, egui::Role::Tab, &t!(section.label())).is_some());
        }
        assert!(accessibility::testing::find(&tree, egui::Role::Tab, "Remote Development").is_none());
        fixture.click(egui::Role::Tab, &t!("ide-welcome-learn"));
        assert_eq!(fixture.view.section, WelcomeSection::Learn);
        let tree = fixture.tree();
        assert!(
            accessibility::testing::find(&tree, egui::Role::Link, &t!("ide-welcome-learn-keyboard-shortcuts"))
                .is_some()
        );
    }

    #[test]
    fn the_plugins_section_is_the_settings_page() {
        let mut fixture = Fixture::new();
        fixture.view.section = WelcomeSection::Plugins;
        fixture.frame(Vec::new());
        assert!(fixture.called("plugins.catalog.list").is_some(), "{:?}", fixture.calls);
    }

    #[test]
    fn customize_changes_the_theme_at_once_and_offers_the_rest() {
        let mut fixture = Fixture::new();
        fixture.view.section = WelcomeSection::Customize;
        fixture.frame(Vec::new());
        assert!(fixture.called("ui.keymap.list").is_some());

        fixture.click(egui::Role::CheckBox, &t!("settings-theme-sync"));
        assert_eq!(fixture.called("ui.theme.followSystem"), Some(json!({ "enabled": true })));

        fixture.click(egui::Role::Link, &t!("ide-welcome-customize-configure"));
        assert_eq!(fixture.view.state()["popups"], json!(["settings:keymap"]));

        let mut fixture = Fixture::new();
        fixture.view.section = WelcomeSection::Customize;
        fixture.click(egui::Role::Link, &t!("keymap-view-ideSettings-import"));
        assert_eq!(fixture.view.state()["popups"], json!(["importSettings"]));
    }

    #[test]
    fn the_sections_can_be_looked_at() {
        let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS") else { return };
        for section in WelcomeSection::ALL {
            let mut fixture = Fixture::new();
            fixture.view.section = section;
            fixture.frame(Vec::new());
            let Fixture { view, screen, calls, .. } = &mut fixture;
            let image = screen.render(|ui| {
                let mut dispatch = |id: &str, params| Fixture::answer(calls, id, params);
                let _ = view.show(ui, &Overview::default(), &mut dispatch);
            });
            let file = Path::new(&dir).join(format!("welcome-{}.png", section.name()));
            crate::offscreen::write_png(&file, &image).unwrap();
        }
    }

    #[test]
    fn dragging_the_font_size_applies_it_at_once() {
        let mut fixture = Fixture::new();
        fixture.view.section = WelcomeSection::Customize;
        let tree = fixture.tree();
        let node = accessibility::testing::find(&tree, egui::Role::SpinButton, &t!("ide-welcome-customize-font"))
            .unwrap_or_else(|| panic!("no font size"));
        let bounds = node.bounds().expect("bounds");
        let at = pos2(((bounds.x0 + bounds.x1) / 2.0) as f32, ((bounds.y0 + bounds.y1) / 2.0) as f32);
        let press = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        fixture.frame(vec![egui::Event::PointerMoved(at)]);
        fixture.frame(vec![press(true)]);
        fixture.frame(vec![egui::Event::PointerMoved(at + vec2(16.0, 0.0))]);
        let applied = fixture.requests.iter().find_map(|request| match request {
            Request::Settings(settings) => settings.ui_font_size,
            _ => None,
        });
        assert!(applied.is_some_and(|size| size > 13.0), "{applied:?}");
    }

    #[test]
    fn the_recent_projects_are_read_once_and_again_when_the_window_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::write(project.join(".git/HEAD"), "ref: refs/heads/first\n").unwrap();
        let ctx = egui::Context::default();
        let mut facts = RecentFacts::default();
        let settled = |facts: &RecentFacts, branch: &str| {
            let start = std::time::Instant::now();
            while facts.of(&project).branch.as_deref() != Some(branch) {
                assert!(start.elapsed() < std::time::Duration::from_secs(60), "not read in the background");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        let frame = |facts: &mut RecentFacts, focused: bool| {
            let mut input = egui::RawInput { focused, ..Default::default() };
            let info = egui::ViewportInfo { focused: Some(focused), ..Default::default() };
            input.viewports.insert(egui::ViewportId::ROOT, info);
            let mut output = ctx.run_ui(input, |ui| facts.follow(ui.ctx(), vec![project.clone()]));
            output.textures_delta.clear();
        };
        frame(&mut facts, true);
        settled(&facts, "first");
        std::fs::write(project.join(".git/HEAD"), "ref: refs/heads/second\n").unwrap();
        frame(&mut facts, true);
        frame(&mut facts, true);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(facts.of(&project).branch.as_deref(), Some("first"), "no frame reads the disk");
        frame(&mut facts, false);
        frame(&mut facts, true);
        settled(&facts, "second");
    }

    fn assert_apart(layout: &HeaderLayout, left: f32, right: f32, end: f32) {
        let switch = Rect::from_min_max(pos2(end + GAP, layout.search.top()), pos2(right, layout.search.bottom()));
        let mut all = vec![layout.search, switch];
        all.extend(&layout.buttons);
        for (index, one) in all.iter().enumerate() {
            for other in &all[index + 1..] {
                assert!(!one.intersects(*other), "{one:?} overlaps {other:?} in {layout:?}");
            }
        }
        for button in &layout.buttons {
            assert!(button.left() >= left && button.right() <= right, "{button:?} leaves the pane");
            assert!(button.bottom() <= layout.bottom, "{button:?} is below the header");
        }
    }

    #[test]
    fn the_search_gives_way_first_and_the_buttons_keep_one_row_while_they_fit() {
        let widths = [119.0, 82.0, 201.0];
        let (left, right) = (246.0, 940.0);
        let end = right - 104.0 - GAP;
        let layout = header_layout(&widths, left, right, end, 44.0);
        assert_apart(&layout, left, right, end);
        assert_eq!(layout.bottom, 44.0 + ROW, "one row");
        assert_eq!(layout.buttons[2].right(), end, "the buttons end at the switch");
        assert!(layout.search.width() >= SEARCH_MIN, "{layout:?}");
        assert!(layout.buttons[0].left() < layout.buttons[1].left(), "New Project comes first");
    }

    #[test]
    fn in_a_narrow_window_the_buttons_move_under_the_search_and_never_overlap() {
        let widths = [119.0, 82.0, 201.0];
        for (left, right) in [(246.0, 620.0), (246.0, 460.0)] {
            let end = right - 100.0;
            let layout = header_layout(&widths, left, right, end, 44.0);
            assert_apart(&layout, left, right, end);
            assert!(layout.buttons.iter().all(|button| button.top() > layout.search.bottom()), "{layout:?}");
            assert_eq!(layout.search.right(), end + GAP - SEARCH_GAP, "the search takes the first row");
        }
        let layout = header_layout(&widths, 246.0, 460.0, 360.0, 44.0);
        assert!(layout.bottom > 44.0 + 2.0 * ROW + GAP, "the buttons wrap to a further row: {layout:?}");
    }
}

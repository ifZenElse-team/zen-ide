use std::path::{Path, PathBuf};

use egui::{Align2, Color32, CursorIcon, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::IdePalette;
use super::caret_popup::{CaretPlace, PopupAnchor};
use crate::theme;

const LINGER: f64 = 0.2;
const BATCH_ROWS: usize = 12;
const BATCH_ROW: f32 = 24.0;
const WIDTH: f32 = 560.0;
const BODY: f32 = 13.0;

#[derive(Default)]
pub(super) struct ProblemPopups {
    hover: Option<Hover>,
    menu: Option<Menu>,
    batch: Option<Batch>,
    details: Option<Details>,
    actions: Vec<(String, Value)>,
}

struct Hover {
    path: PathBuf,
    problems: Vec<Value>,
    at: Pos2,
    since: f64,
    seen: f64,
    rect: Option<Rect>,
}

struct Menu {
    path: PathBuf,
    fixes: Vec<Value>,
    selected: usize,
    at: PopupAnchor,
    opened: f64,
    main: bool,
    title: Option<String>,
}

struct Batch {
    path: PathBuf,
    title: String,
    rows: Vec<(Value, bool)>,
}

struct Details {
    title: String,
    blocks: Vec<super::doc_popups::Block>,
}

enum Pick {
    Fix(Value),
    Details(String, String),
    More(Vec<Value>),
}

fn hover_delay(ctx: &egui::Context) -> f64 {
    f64::from(ctx.global_style().interaction.tooltip_delay)
}

fn fixes_of(problem: &Value) -> Vec<Value> {
    let mut fixes = problem["fixes"].as_array().cloned().unwrap_or_default();
    for fix in &mut fixes {
        if fix["kind"] == "details" {
            fix["details"] = problem["details"].clone();
            fix["heading"] = problem["message"].clone();
        }
    }
    fixes
}

fn is_heading(fix: &Value) -> bool {
    fix["kind"] == "heading"
}

fn first_choice(fixes: &[Value]) -> usize {
    fixes.iter().position(|fix| !is_heading(fix)).unwrap_or(0)
}

impl ProblemPopups {
    pub(super) fn take_actions(&mut self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.actions)
    }

    pub(super) fn is_open(&self) -> bool {
        self.menu.is_some() || self.batch.is_some() || self.details.is_some()
    }

    pub(super) fn show_details_of(&mut self, title: &str, markdown: &str) {
        self.hover = None;
        let (title, markdown) = match markdown.trim_start().split_once('\n') {
            Some((first, rest)) if first.starts_with('#') => (first.trim_start_matches('#').trim(), rest),
            _ => (title, markdown),
        };
        self.details = Some(Details { title: title.to_owned(), blocks: super::doc_popups::parse(markdown) });
    }

    pub(super) fn open_names(&self) -> Vec<&'static str> {
        let mut names = Vec::new();
        if self.hover.as_ref().is_some_and(|hover| hover.rect.is_some()) {
            names.push("problem");
        }
        if self.menu.is_some() {
            names.push("contextActions");
        }
        if self.batch.is_some() {
            names.push("problemBatch");
        }
        if self.details.is_some() {
            names.push("problemDetails");
        }
        names
    }

    pub(super) fn hover(&mut self, ui: &Ui, path: &Path, hovered: Vec<Value>, pointer: Option<Pos2>, line_bottom: f32) {
        let now = ui.input(|input| input.time);
        let delay = hover_delay(ui.ctx());
        let over_popup = self
            .hover
            .as_ref()
            .and_then(|hover| hover.rect)
            .zip(ui.ctx().pointer_hover_pos())
            .is_some_and(|(rect, pointer)| rect.expand(6.0).contains(pointer));
        if self.menu.is_some() || self.batch.is_some() || self.details.is_some() {
            self.hover = None;
            return;
        }
        let key = |problems: &[Value]| -> Vec<(Value, Value)> {
            problems.iter().map(|problem| (problem["start"].clone(), problem["message"].clone())).collect()
        };
        match (&mut self.hover, pointer) {
            (Some(hover), _) if over_popup => hover.seen = now,
            (Some(hover), Some(_)) if !hovered.is_empty() && key(&hover.problems) == key(&hovered) => {
                hover.seen = now;
                hover.problems = hovered;
            }
            (_, Some(pointer)) if !hovered.is_empty() => {
                self.hover = Some(Hover {
                    path: path.to_owned(),
                    problems: hovered,
                    at: pos2(pointer.x - 24.0, line_bottom + 2.0),
                    since: now,
                    seen: now,
                    rect: None,
                });
            }
            (Some(hover), _) if hover.rect.is_none() => self.hover = None,
            (Some(hover), _) if now - hover.seen > LINGER => self.hover = None,
            _ => {}
        }
        if let Some(hover) = &self.hover {
            let wait = (hover.since + delay - now).max(LINGER - (now - hover.seen)).max(0.05);
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
    }

    pub(super) fn context_actions(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        main: bool,
    ) -> Option<String> {
        let path = session.state::<zen_workbench::Workbench>()?.active_document()?.path().to_owned();
        let problems = registry.dispatch(session, "problems.fixes.list", json!({ "path": path })).ok()?;
        let mut fixes: Vec<Value> = Vec::new();
        for fix in problems.as_array().into_iter().flatten().flat_map(fixes_of) {
            if !fixes.iter().any(|kept| kept["id"] == fix["id"]) {
                fixes.push(fix);
            }
        }
        if fixes.is_empty() {
            return Some(t!("ide-problem-no-fixes"));
        }
        self.hover = None;
        if main {
            let first = fixes.iter().find(|fix| !is_heading(fix)).cloned()?;
            return self.pick(registry, session, &path, &first);
        }
        let at = PopupAnchor::Caret(vec2(0.0, 2.0));
        let selected = first_choice(&fixes);
        let opened = ctx.input(|input| input.time);
        self.menu = Some(Menu { path, fixes, selected, at, opened, main: true, title: None });
        None
    }

    pub(super) fn open_menu(&mut self, ctx: &egui::Context, path: PathBuf, title: Option<String>, fixes: Vec<Value>) {
        self.hover = None;
        let at = PopupAnchor::Caret(vec2(0.0, 2.0));
        let selected = first_choice(&fixes);
        self.menu = Some(Menu { path, fixes, selected, at, opened: ctx.input(|input| input.time), main: false, title });
    }

    #[cfg(test)]
    pub(super) fn menu_title(&self) -> Option<String> {
        self.menu.as_ref().and_then(|menu| menu.title.clone())
    }

    #[cfg(test)]
    pub(super) fn menu_titles(&self) -> Vec<String> {
        self.menu
            .iter()
            .flat_map(|menu| menu.fixes.iter())
            .map(|fix| fix["title"].as_str().unwrap_or_default().to_owned())
            .collect()
    }

    fn pick(&mut self, registry: &Registry, session: &mut Session, path: &Path, fix: &Value) -> Option<String> {
        match fix["kind"].as_str() {
            Some("batch") => {
                self.open_batch(registry, session, path, fix);
                return None;
            }
            Some("details") => {
                let heading = fix["heading"].as_str().unwrap_or_default().trim_end_matches(['.', ':']).to_owned();
                self.show_details_of(&heading, fix["details"].as_str().unwrap_or_default());
                return None;
            }
            Some("action") => {
                let action = fix["args"]["action"].as_str().unwrap_or_default().to_owned();
                self.actions.push((action, fix["args"]["params"].clone()));
                return None;
            }
            _ => {}
        }
        let id = fix["id"].as_str()?;
        match registry.dispatch(session, "problems.fix.apply", json!({ "path": path, "ids": [id] })) {
            Ok(_) => None,
            Err(zen_core::DispatchError::Failed { error, .. }) => Some(error.message),
            Err(error) => Some(error.to_string()),
        }
    }

    fn open_batch(&mut self, registry: &Registry, session: &mut Session, path: &Path, fix: &Value) {
        let problems = registry.dispatch(session, "problems.file.list", json!({ "path": path })).unwrap_or_default();
        let mut rows: Vec<(Value, bool)> = Vec::new();
        for candidate in zen_workbench::language::listed_problems(&problems)
            .iter()
            .flat_map(|problem| problem["fixes"].as_array().cloned().unwrap_or_default())
        {
            if candidate["kind"] == "apply"
                && candidate["batch"] == fix["batch"]
                && candidate["label"].is_string()
                && !rows.iter().any(|(kept, _)| kept["id"] == candidate["id"])
            {
                rows.push((candidate, true));
            }
        }
        let title = fix["title"].as_str().unwrap_or_default().trim_end_matches(['…', '.']).to_owned();
        self.batch = Some(Batch { path: path.to_owned(), title, rows });
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
        caret: CaretPlace,
    ) -> Option<String> {
        let mut notice = None;
        if let Some(pick) = self.show_hover(ui, palette) {
            let path = self.hover.as_ref().map(|hover| hover.path.clone()).unwrap_or_default();
            notice = self.act(ui, registry, session, &path, pick);
        }
        if let Some(pick) = self.show_menu(ui, palette, caret) {
            let path = self.menu.take().map(|menu| menu.path).unwrap_or_default();
            notice = self.act(ui, registry, session, &path, pick);
        }
        notice = self.show_batch(ui, registry, session, palette).or(notice);
        self.show_details(ui, palette);
        notice
    }

    fn act(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, path: &Path, pick: Pick) -> Option<String> {
        match pick {
            Pick::Fix(fix) => {
                self.hover = None;
                self.pick(registry, session, path, &fix)
            }
            Pick::Details(title, markdown) => {
                self.show_details_of(&title, &markdown);
                None
            }
            Pick::More(fixes) => {
                let at = PopupAnchor::At(self.hover.take().map(|hover| hover.at).unwrap_or_default());
                self.menu = Some(Menu {
                    path: path.to_owned(),
                    selected: first_choice(&fixes),
                    fixes,
                    at,
                    opened: ui.input(|input| input.time),
                    main: true,
                    title: None,
                });
                None
            }
        }
    }

    fn show_hover(&mut self, ui: &mut Ui, palette: &IdePalette) -> Option<Pick> {
        let now = ui.input(|input| input.time);
        let delay = hover_delay(ui.ctx());
        let hover = self.hover.as_mut()?;
        if now - hover.since < delay {
            return None;
        }
        let mut problems = hover.problems.clone();
        problems.sort_by_key(|problem| rank(problem["severity"].as_str().unwrap_or_default()));
        let (mains, fixes) = footer_fixes(&problems);
        let mut pick = None;
        let shown = egui::Area::new(ui.scope_id().with("problem-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(hover.at)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                let width = popup_width(ui, palette, &problems, &mains);
                super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::ZERO).show(ui, |ui| {
                    ui.set_width(width);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    let body =
                        egui::Frame::new().inner_margin(egui::Margin { left: 14, right: 18, top: 10, bottom: 10 });
                    body.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        for (index, problem) in problems.iter().enumerate() {
                            if index > 0 {
                                ui.add_space(10.0);
                            }
                            if let Some(chosen) = problem_body(ui, palette, problem) {
                                pick = Some(chosen);
                            }
                        }
                    });
                    if fixes.is_empty() {
                        return;
                    }
                    let top = ui.cursor().top();
                    ui.painter().hline(ui.min_rect().x_range(), top, Stroke::new(1.0, palette.popup_border));
                    let footer = egui::Frame::new()
                        .fill(palette.popup_code)
                        .corner_radius(egui::CornerRadius { nw: 0, ne: 0, sw: 8, se: 8 })
                        .inner_margin(egui::Margin { left: 14, right: 14, top: 7, bottom: 7 });
                    footer.show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            for (index, main) in mains.iter().enumerate() {
                                if index > 0 {
                                    ui.add_space(22.0);
                                }
                                if link(ui, palette, main["title"].as_str().unwrap_or_default()).clicked() {
                                    pick = Some(Pick::Fix(main.clone()));
                                }
                                if index == 0 {
                                    shortcut(ui, palette, "view.problem.fix");
                                }
                            }
                            ui.add_space(22.0);
                            if link(ui, palette, &t!("ide-problem-more-actions")).clicked() {
                                pick = Some(Pick::More(fixes.clone()));
                            }
                            shortcut(ui, palette, "view.intentions.open");
                        });
                    });
                });
            });
        hover.rect = Some(shown.response.rect);
        pick
    }

    fn show_menu(&mut self, ui: &mut Ui, palette: &IdePalette, caret: CaretPlace) -> Option<Pick> {
        let menu = self.menu.as_mut()?;
        let count = menu.fixes.len().max(1);
        let none = Modifiers::NONE;
        let (up, down, enter, escape) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::Enter),
                input.consume_key(none, Key::Escape),
            )
        });
        for _ in 0..count {
            if up {
                menu.selected = menu.selected.checked_sub(1).unwrap_or(count - 1);
            }
            if down {
                menu.selected = (menu.selected + 1) % count;
            }
            if !(up || down) || !menu.fixes.get(menu.selected).is_some_and(is_heading) {
                break;
            }
        }
        if escape {
            self.menu = None;
            return None;
        }
        if enter {
            return menu.fixes.get(menu.selected).filter(|fix| !is_heading(fix)).cloned().map(Pick::Fix);
        }
        let area = egui::Area::new(ui.scope_id().with("context-actions")).order(egui::Order::Foreground);
        let area = menu.at.area(ui.ctx(), area, caret)?;
        let mut pick = None;
        let shown = area.show(ui.ctx(), |ui| {
            super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::symmetric(4, 4)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let widest = menu
                    .fixes
                    .iter()
                    .map(|fix| {
                        let title = fix["title"].as_str().unwrap_or_default().to_owned();
                        ui.painter().layout_no_wrap(title, theme::regular(BODY), palette.text).size().x
                    })
                    .fold(220.0_f32, f32::max);
                let width = widest + 80.0;
                let main_keys = if menu.main { crate::keymap::shortcut("view.problem.fix") } else { None };
                let main_index = first_choice(&menu.fixes);
                if let Some(title) = &menu.title {
                    let (rect, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
                    ui.painter().text(
                        pos2(rect.left() + 10.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        title,
                        theme::semibold(BODY),
                        palette.text,
                    );
                }
                for (index, fix) in menu.fixes.iter().enumerate() {
                    let heading = is_heading(fix);
                    let sense = if heading { Sense::hover() } else { Sense::click() };
                    let (rect, response) = ui.allocate_exact_size(vec2(width, 26.0), sense);
                    if response.hovered() && !heading {
                        menu.selected = index;
                    }
                    if index == menu.selected && !heading {
                        ui.painter().rect_filled(rect, 4, palette.popup_hover);
                    }
                    ui.painter().text(
                        pos2(rect.left() + 10.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        fix["title"].as_str().unwrap_or_default(),
                        theme::regular(BODY),
                        if heading { palette.text_secondary } else { palette.text },
                    );
                    if index == main_index
                        && let Some(keys) = &main_keys
                    {
                        ui.painter().text(
                            pos2(rect.right() - 10.0, rect.center().y),
                            Align2::RIGHT_CENTER,
                            keys,
                            theme::regular(BODY - 1.0),
                            palette.text_secondary,
                        );
                    }
                    if response.clicked() && !heading {
                        pick = Some(Pick::Fix(fix.clone()));
                    }
                }
            });
        });
        let now = ui.input(|input| input.time);
        let rect = shown.response.rect;
        let clicked_outside = ui.input(|input| {
            input.pointer.any_pressed() && input.pointer.interact_pos().is_some_and(|at| !rect.contains(at))
        });
        if pick.is_none() && clicked_outside && now - menu.opened > 0.1 {
            self.menu = None;
        }
        pick
    }

    fn show_batch(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
    ) -> Option<String> {
        let batch = self.batch.as_mut()?;
        let p = palette;
        let (mut apply, mut cancel) = (false, false);
        let modal =
            egui::Modal::new(ui.scope_id().with("problem-batch")).frame(super::dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let font = FontId::monospace(BODY - 0.5);
                let widest = batch
                    .rows
                    .iter()
                    .map(|(fix, _)| {
                        let label = fix["label"].as_str().unwrap_or_default().to_owned();
                        ui.painter().layout_no_wrap(label, font.clone(), p.text).size().x
                    })
                    .fold(0.0_f32, f32::max);
                let window = ui.ctx().content_rect().width();
                let width = (widest + 40.0).clamp(380.0, (window - 120.0).clamp(380.0, 760.0));
                ui.set_width(width);
                ui.label(egui::RichText::new(&batch.title).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(14.0);
                if batch.rows.is_empty() {
                    let empty = egui::RichText::new(t!("ide-problem-batch-empty"));
                    ui.label(empty.font(theme::regular(BODY)).color(p.text_secondary));
                } else {
                    let height = BATCH_ROW * batch.rows.len().min(BATCH_ROWS) as f32;
                    egui::ScrollArea::vertical().max_height(height).auto_shrink([false, true]).show(ui, |ui| {
                        for (fix, checked) in &mut batch.rows {
                            let label = fix["label"].as_str().unwrap_or_default();
                            ui.allocate_ui(vec2(ui.available_width(), BATCH_ROW), |ui| {
                                ui.set_min_height(BATCH_ROW);
                                ui.checkbox(checked, egui::RichText::new(label).font(font.clone()).color(p.text));
                            });
                        }
                    });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        if link(ui, p, &t!("ide-problem-batch-select-all")).clicked() {
                            batch.rows.iter_mut().for_each(|(_, checked)| *checked = true);
                        }
                        if link(ui, p, &t!("ide-problem-batch-select-none")).clicked() {
                            batch.rows.iter_mut().for_each(|(_, checked)| *checked = false);
                        }
                    });
                }
                ui.add_space(20.0);
                let row = vec2(ui.available_width(), 30.0);
                ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let count = batch.rows.iter().filter(|(_, checked)| *checked).count();
                    let label = egui::RichText::new(if count == 0 {
                        t!("ide-problem-batch-apply")
                    } else {
                        t!("ide-problem-batch-apply-count", count = count)
                    });
                    let ok = egui::Button::new(label.font(theme::regular(13.0)).color(p.accent_text))
                        .min_size(vec2(84.0, 30.0))
                        .corner_radius(6)
                        .fill(if count > 0 { p.accent } else { p.accent.gamma_multiply(0.4) });
                    if ui.add_enabled(count > 0, ok).clicked() {
                        apply = true;
                    }
                    let label = egui::RichText::new(t!("settings-cancel"));
                    let back = egui::Button::new(label.font(theme::regular(13.0)).color(p.text))
                        .min_size(vec2(84.0, 30.0))
                        .corner_radius(6)
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0, p.button_border));
                    if ui.add(back).clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            let ids: Vec<Value> =
                batch.rows.iter().filter(|(_, checked)| *checked).map(|(fix, _)| fix["id"].clone()).collect();
            let path = batch.path.clone();
            self.batch = None;
            return match registry.dispatch(session, "problems.fix.apply", json!({ "path": path, "ids": ids })) {
                Ok(_) => None,
                Err(zen_core::DispatchError::Failed { error, .. }) => Some(error.message),
                Err(error) => Some(error.to_string()),
            };
        }
        if cancel || modal.should_close() {
            self.batch = None;
        }
        None
    }

    fn show_details(&mut self, ui: &mut Ui, palette: &IdePalette) {
        let Some(details) = &self.details else { return };
        let p = palette;
        let mut close = false;
        let modal = egui::Modal::new(ui.scope_id().with("problem-details")).frame(super::dialog_frame(p)).show(
            ui.ctx(),
            |ui| {
                let width = 640.0_f32.min(ui.ctx().content_rect().width() - 80.0).max(280.0);
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(&details.title).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(12.0);
                let height = (ui.ctx().content_rect().height() - 220.0).clamp(160.0, 520.0);
                egui::ScrollArea::vertical().max_height(height).show(ui, |ui| {
                    super::doc_popups::render(ui, &details.blocks, p, width - 16.0, None);
                });
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = egui::RichText::new(t!("ide-problem-details-close"));
                    let button = egui::Button::new(label.font(theme::regular(13.0)).color(p.accent_text))
                        .min_size(vec2(84.0, 30.0))
                        .corner_radius(6)
                        .fill(p.accent);
                    if ui.add(button).clicked() {
                        close = true;
                    }
                });
            },
        );
        if close || modal.should_close() {
            self.details = None;
        }
    }
}

fn problem_body(ui: &mut Ui, palette: &IdePalette, problem: &Value) -> Option<Pick> {
    let mut pick = None;
    let message = problem["message"].as_str().unwrap_or_default();
    let heading = egui::RichText::new(message).font(theme::regular(BODY + 0.5)).color(palette.text);
    ui.add(egui::Label::new(heading).wrap());
    if let Some(note) = problem["note"].as_str() {
        ui.add_space(6.0);
        let mut job = egui::text::LayoutJob::default();
        let (label, text) = match note.split_once(": ") {
            Some((label, text)) => (format!("{label}: "), text),
            None => (String::new(), note),
        };
        let format = |color| egui::TextFormat { font_id: theme::regular(BODY), color, ..Default::default() };
        job.append(&label, 0.0, format(palette.text_secondary));
        job.append(text, 0.0, format(palette.text));
        job.wrap.max_width = ui.available_width();
        ui.add(egui::Label::new(job).wrap());
    }
    let rows = problem["rows"].as_array().cloned().unwrap_or_default();
    if !rows.is_empty() {
        ui.add_space(6.0);
    }
    for row in &rows {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let label = row["label"].as_str().unwrap_or_default();
            match row["url"].as_str() {
                Some(url) => {
                    if link(ui, palette, label).clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                }
                None => {
                    ui.label(egui::RichText::new(label).font(theme::regular(BODY)).color(palette.text));
                }
            }
            if let Some(badge) = row["badge"].as_str() {
                ui.label(egui::RichText::new(badge).font(theme::regular(BODY)).color(palette.text));
            }
            let text = egui::RichText::new(row["text"].as_str().unwrap_or_default());
            ui.add(egui::Label::new(text.font(theme::regular(BODY)).color(palette.text)).truncate());
        });
    }
    let shown_by_fix =
        problem["fixes"].as_array().is_some_and(|fixes| fixes.iter().any(|fix| fix["kind"] == "details"));
    if let Some(details) = problem["details"].as_str().filter(|_| !shown_by_fix) {
        ui.add_space(8.0);
        if link(ui, palette, &t!("ide-problem-show-details")).clicked() {
            pick = Some(Pick::Details(message.trim_end_matches(':').to_owned(), details.to_owned()));
        }
    }
    pick
}

fn footer_fixes(problems: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let (mut mains, mut all): (Vec<Value>, Vec<Value>) = (Vec::new(), Vec::new());
    for (index, problem) in problems.iter().enumerate() {
        let fixes = fixes_of(problem);
        let main =
            if index == 0 { fixes.first() } else { fixes.iter().find(|fix| fix["kind"] == "apply") }.or(fixes.first());
        let inline = fixes.iter().filter(|fix| fix["inline"] == true);
        for main in main.into_iter().chain(inline) {
            if !mains.iter().any(|kept| kept["id"] == main["id"] || kept["title"] == main["title"]) {
                mains.push(main.clone());
            }
        }
        for fix in fixes {
            if !all.iter().any(|kept| kept["id"] == fix["id"]) {
                all.push(fix);
            }
        }
    }
    (mains, all)
}

fn popup_width(ui: &Ui, palette: &IdePalette, problems: &[Value], mains: &[Value]) -> f32 {
    let measure = |text: &str, size: f32| {
        ui.painter().layout_no_wrap(text.to_owned(), theme::regular(size), palette.text).size().x
    };
    let mut widest: f32 = 0.0;
    for problem in problems {
        widest = widest.max(measure(problem["message"].as_str().unwrap_or_default(), BODY + 0.5));
        widest = widest.max(measure(problem["note"].as_str().unwrap_or_default(), BODY));
        for row in problem["rows"].as_array().into_iter().flatten() {
            let parts = ["label", "badge", "text"].map(|part| measure(row[part].as_str().unwrap_or_default(), BODY));
            widest = widest.max(parts.iter().sum::<f32>() + 20.0);
        }
    }
    if !mains.is_empty() {
        let keys = |action: &str| crate::keymap::shortcut(action).map_or(0.0, |keys| measure(&keys, BODY - 1.0));
        let links: f32 =
            mains.iter().map(|main| measure(main["title"].as_str().unwrap_or_default(), BODY) + 22.0 + 8.0).sum();
        let footer = links
            + keys("view.problem.fix")
            + measure(&t!("ide-problem-more-actions"), BODY)
            + keys("view.intentions.open")
            + 3.0 * 8.0;
        widest = widest.max(footer);
    }
    (widest + 32.0 + 4.0).min(WIDTH)
}

fn rank(severity: &str) -> u8 {
    match severity {
        "error" => 0,
        "warning" => 1,
        "weakWarning" => 2,
        "typo" => 3,
        "information" => 4,
        _ => 5,
    }
}

fn link(ui: &mut Ui, palette: &IdePalette, text: &str) -> egui::Response {
    let label = egui::RichText::new(text).font(theme::regular(BODY)).color(palette.link);
    let response = ui.add(egui::Label::new(label).sense(Sense::click()));
    if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    response
}

fn shortcut(ui: &mut Ui, palette: &IdePalette, action: &str) {
    if let Some(keys) = crate::keymap::shortcut(action) {
        ui.label(egui::RichText::new(keys).font(theme::regular(BODY - 1.0)).color(palette.text_secondary));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_problem_on_the_spot_offers_its_fix() {
        let fix = |id: &str, title: &str, kind: &str| json!({ "id": id, "title": title, "kind": kind });
        let problems = [
            json!({ "severity": "error", "message": "Missing go.sum entry", "fixes": [
                fix("go.tidy:a", "Run go mod tidy", "apply"),
            ] }),
            json!({ "severity": "weakWarning", "message": "Newer version is available: v0.22.0", "fixes": [
                fix("go.updateDependencies:a", "Update dependencies…", "batch"),
                fix("go.update:a@v0.22.0", "Update to v0.22.0", "apply"),
                fix("go.updateTidy:a@v0.22.0", "Update to v0.22.0 and run go mod tidy", "apply"),
            ] }),
            json!({ "severity": "error", "message": "The same fix again", "fixes": [
                fix("go.tidy:a", "Run go mod tidy", "apply"),
            ] }),
        ];
        let (mains, all) = footer_fixes(&problems);
        let titles =
            |fixes: &[Value]| fixes.iter().map(|fix| fix["title"].as_str().unwrap().to_owned()).collect::<Vec<_>>();
        assert_eq!(titles(&mains), ["Run go mod tidy", "Update to v0.22.0"]);
        assert_eq!(
            titles(&all),
            ["Run go mod tidy", "Update dependencies…", "Update to v0.22.0", "Update to v0.22.0 and run go mod tidy"]
        );
        let (mains, _) = footer_fixes(&problems[1..2]);
        assert_eq!(titles(&mains), ["Update dependencies…"]);
    }

    #[test]
    fn fixes_marked_inline_are_offered_on_the_spot() {
        let problems = [
            json!({ "severity": "weakWarning", "message": "Newer Go version is available: 1.27.3", "fixes": [
            { "id": "go.goDirective:1.27.3", "title": "Update go directive to 1.27.3", "kind": "apply", "inline": true },
            { "id": "go.goDirectivePatch:1.26.10", "title": "Update to 1.26.10", "kind": "apply", "inline": true },
        ] }),
        ];
        let (mains, all) = footer_fixes(&problems);
        let titles: Vec<&str> = mains.iter().map(|fix| fix["title"].as_str().unwrap()).collect();
        assert_eq!(titles, ["Update go directive to 1.27.3", "Update to 1.26.10"]);
        assert_eq!(all.len(), 2);
    }
}

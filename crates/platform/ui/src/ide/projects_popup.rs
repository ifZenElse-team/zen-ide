use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, Event, FontId, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_i18n::t;

use super::app::Request;
use super::glyphs::{glyph_branch, glyph_folder, glyph_search};
use super::{IdePalette, IdeView, Side, git_branch, home_relative, in_development, project_badge, project_name};
use crate::{accessibility, theme};

pub(super) const WIDTH: f32 = 460.0;
const LIST_HEIGHT: f32 = 420.0;
const FIELD: f32 = 30.0;
const ITEM: f32 = 26.0;

pub(super) fn layouts(query: &str) -> Vec<String> {
    zen_index::layout::forms(query)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Match {
    pub(super) score: i64,
    pub(super) name: Vec<usize>,
    pub(super) path: Vec<usize>,
}

const PATH_PENALTY: i64 = 100_000;

pub(super) fn project_match(query: &str, name: &str, path: &str) -> Option<Match> {
    let query = query.trim();
    if query.is_empty() {
        return Some(Match::default());
    }
    layouts(query)
        .iter()
        .filter_map(|form| {
            if let Some(score) = zen_index::score(form, name, name)
                && let Some(positions) = zen_index::match_positions(form, name)
            {
                return Some(Match { score, name: positions, path: Vec::new() });
            }
            let path_chars: Vec<char> = path.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
            let wanted: Vec<char> = form.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect();
            let at = path_chars.windows(wanted.len()).position(|window| window == wanted.as_slice())?;
            let score = -(at as i64) - path_chars.len() as i64;
            Some(Match { score: score - PATH_PENALTY, name: Vec::new(), path: (at..at + wanted.len()).collect() })
        })
        .max_by_key(|found| found.score)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Project {
    pub(super) path: PathBuf,
    pub(super) open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Open,
    Recent,
}

impl Section {
    fn title(self) -> String {
        match self {
            Section::Open => t!("ide-menu-open-projects"),
            Section::Recent => t!("ide-menu-recent-projects"),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Section::Open => "open",
            Section::Recent => "recent",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    OpenFolder,
    Clone,
    Project(PathBuf),
}

#[derive(Clone, Debug)]
enum Row {
    Action(Choice),
    Heading(Section),
    Project { index: usize, section: Section, found: Match },
    Nothing,
}

impl Row {
    fn choice(&self, projects: &[Project]) -> Option<Choice> {
        match self {
            Row::Action(choice) => Some(choice.clone()),
            Row::Project { index, .. } => Some(Choice::Project(projects[*index].path.clone())),
            Row::Heading(_) | Row::Nothing => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    OpenFolder,
    Clone,
    Focus(PathBuf),
    Open(PathBuf),
}

#[derive(Default)]
pub(super) struct ProjectsPopup {
    open: bool,
    filter: String,
    filtered_for: Option<String>,
    selected: Option<Choice>,
    reveal: bool,
    height: Option<f32>,
    branches: HashMap<PathBuf, Option<String>>,
    drawn: Option<Rect>,
}

impl ProjectsPopup {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn toggle(&mut self) {
        if self.open { self.close() } else { self.show_empty() }
    }

    pub(super) fn show_empty(&mut self) {
        *self = Self { open: true, ..Self::default() };
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.filter.clear();
        self.filtered_for = None;
        self.selected = None;
    }

    #[cfg(test)]
    pub(super) fn filter(&self) -> &str {
        &self.filter
    }

    pub(super) fn set_filter(&mut self, filter: &str) {
        self.filter = filter.chars().filter(|c| !c.is_control()).collect();
    }

    fn branch(&mut self, path: &Path) -> Option<String> {
        self.branches.entry(path.to_owned()).or_insert_with(|| git_branch(path)).clone()
    }

    fn filtering(&self) -> bool {
        !self.filter.trim().is_empty()
    }

    fn rows(&self, projects: &[Project]) -> Vec<Row> {
        let filtering = self.filtering();
        let mut rows = Vec::new();
        if !filtering {
            rows.push(Row::Action(Choice::OpenFolder));
            rows.push(Row::Action(Choice::Clone));
        }
        let mut sections: Vec<(Section, Vec<(usize, Match)>)> = [Section::Open, Section::Recent]
            .into_iter()
            .map(|section| {
                let mut found: Vec<(usize, Match)> = projects
                    .iter()
                    .enumerate()
                    .filter(|(_, project)| project.open == (section == Section::Open))
                    .filter_map(|(index, project)| {
                        let path = home_relative(&project.path);
                        project_match(&self.filter, &project_name(&project.path), &path).map(|found| (index, found))
                    })
                    .collect();
                if filtering {
                    found.sort_by_key(|(_, found)| std::cmp::Reverse(found.score));
                }
                (section, found)
            })
            .filter(|(_, found)| !found.is_empty())
            .collect();
        if filtering {
            sections.sort_by_key(|(_, found)| std::cmp::Reverse(found.first().map_or(i64::MIN, |(_, m)| m.score)));
        }
        for (section, found) in sections {
            rows.push(Row::Heading(section));
            rows.extend(found.into_iter().map(|(index, found)| Row::Project { index, section, found }));
        }
        if filtering && !rows.iter().any(|row| matches!(row, Row::Project { .. })) {
            rows.push(Row::Nothing);
        }
        rows
    }

    fn sync_selection(&mut self, rows: &[Row], projects: &[Project]) {
        let choices: Vec<Choice> = rows.iter().filter_map(|row| row.choice(projects)).collect();
        if self.filtered_for.as_deref() != Some(self.filter.as_str())
            || self.selected.as_ref().is_some_and(|selected| !choices.contains(selected))
        {
            self.selected = if self.filtering() {
                rows.iter().find(|row| matches!(row, Row::Project { .. })).and_then(|row| row.choice(projects))
            } else {
                None
            };
            self.filtered_for = Some(self.filter.clone());
            self.reveal = true;
        }
    }

    pub(super) fn state(&mut self, projects: &[Project], current: Option<&Path>) -> Value {
        let rows = self.rows(projects);
        self.sync_selection(&rows, projects);
        let listed: Vec<Value> = rows
            .iter()
            .filter_map(|row| match row {
                Row::Project { index, section, .. } => Some((*index, *section)),
                _ => None,
            })
            .map(|(index, section)| {
                let path = projects[index].path.clone();
                json!({
                    "name": project_name(&path),
                    "path": path,
                    "branch": self.branch(&path),
                    "section": section.id(),
                    "current": current == Some(path.as_path()),
                    "selected": self.selected == Some(Choice::Project(path.clone())),
                })
            })
            .collect();
        json!({ "open": self.open, "filter": self.filter, "projects": listed })
    }

    fn keys(&mut self, ui: &Ui, choices: &[Choice]) -> Option<Choice> {
        let mut moves: Vec<bool> = Vec::new();
        let mut enter = false;
        let mut escape = false;
        let mut typed = String::new();
        let mut erase: Vec<egui::Modifiers> = Vec::new();
        ui.input_mut(|input| {
            input.events.retain(|event| match event {
                Event::Text(text) | Event::Paste(text) => {
                    typed.push_str(text.lines().next().unwrap_or_default());
                    false
                }
                Event::Key { key, pressed, modifiers, .. }
                    if !modifiers.command && !modifiers.ctrl || *key == Key::Backspace =>
                {
                    if *pressed {
                        match key {
                            Key::ArrowDown => moves.push(true),
                            Key::ArrowUp => moves.push(false),
                            Key::Enter => enter = true,
                            Key::Escape => escape = true,
                            Key::Backspace => erase.push(*modifiers),
                            _ => {}
                        }
                    }
                    false
                }
                _ => true,
            });
        });
        for modifiers in erase {
            if modifiers.command {
                self.filter.clear();
            } else if modifiers.alt {
                let kept = self.filter.trim_end().rfind(|c: char| !c.is_alphanumeric()).map_or(0, |at| at + 1);
                self.filter.truncate(kept.min(self.filter.trim_end().len()));
            } else {
                self.filter.pop();
            }
        }
        if !typed.is_empty() {
            let filter = format!("{}{typed}", self.filter);
            self.set_filter(&filter);
        }
        if escape {
            if self.filter.is_empty() {
                self.close();
                return None;
            }
            self.filter.clear();
        }
        for down in moves {
            let at = self.selected.as_ref().and_then(|selected| choices.iter().position(|c| c == selected));
            let next = match (at, down) {
                (None, true) => Some(0),
                (None, false) => choices.len().checked_sub(1),
                (Some(at), true) => Some((at + 1).min(choices.len().saturating_sub(1))),
                (Some(at), false) => Some(at.saturating_sub(1)),
            };
            if let Some(choice) = next.and_then(|next| choices.get(next)) {
                self.selected = Some(choice.clone());
                self.reveal = true;
            }
        }
        if enter { self.selected.clone() } else { None }
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        anchor: Rect,
        projects: &[Project],
        current: &Path,
    ) -> Option<Outcome> {
        if !self.open {
            return None;
        }
        let rows = self.rows(projects);
        self.sync_selection(&rows, projects);
        let choices: Vec<Choice> = rows.iter().filter_map(|row| row.choice(projects)).collect();
        let mut chosen = self.keys(ui, &choices);
        if !self.open {
            return None;
        }
        let rows = self.rows(projects);
        self.sync_selection(&rows, projects);
        for row in &rows {
            if let Row::Project { index, .. } = row {
                self.branch(&projects[*index].path);
            }
        }

        let filtering = self.filtering();
        let moving = ui.input(|input| input.pointer.is_moving());
        let mut clear = false;
        let origin = pos2(anchor.left() - 4.0, anchor.bottom() + 4.0);
        let area = egui::Area::new(ui.scope_id().with("projects-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(origin)
            .show(ui.ctx(), |ui| {
                super::dialog_frame(p).corner_radius(10).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                    ui.set_width(WIDTH - 12.0);
                    ui.spacing_mut().item_spacing = Vec2::ZERO;
                    let top = ui.cursor().top();
                    if filtering {
                        clear = self.field(ui, p);
                    }
                    let mut section = None;
                    let mut index = 0;
                    while index < rows.len() {
                        match &rows[index] {
                            Row::Action(choice) => {
                                let response = action_row(ui, p, choice, self.selected.as_ref() == Some(choice));
                                if response.hovered() && moving {
                                    self.selected = Some(choice.clone());
                                }
                                let clicked = if *choice == Choice::Clone {
                                    in_development(ui, &response, "", Side::Right)
                                } else {
                                    response.clicked()
                                };
                                if clicked {
                                    chosen = Some(choice.clone());
                                }
                                index += 1;
                            }
                            Row::Heading(heading) => {
                                section = Some(*heading);
                                heading_row(ui, p, *heading);
                                index += 1;
                            }
                            Row::Nothing => {
                                let (rect, _) =
                                    ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::hover());
                                ui.painter().text(
                                    rect.center(),
                                    Align2::CENTER_CENTER,
                                    t!("ide-projects-nothing"),
                                    theme::regular(13.0),
                                    p.text_secondary,
                                );
                                accessibility::label(ui, ui.scope_id().with("projects-nothing"), rect, || {
                                    t!("ide-projects-nothing")
                                });
                                index += 1;
                            }
                            Row::Project { .. } => {
                                let end = rows[index..]
                                    .iter()
                                    .position(|row| !matches!(row, Row::Project { .. }))
                                    .map_or(rows.len(), |at| index + at);
                                let group = &rows[index..end];
                                let mut draw = |ui: &mut Ui| {
                                    for row in group {
                                        let Row::Project { index, found, .. } = row else { continue };
                                        let project = &projects[*index];
                                        let choice = Choice::Project(project.path.clone());
                                        let selected = self.selected.as_ref() == Some(&choice);
                                        let branch = self.branches.get(&project.path).cloned().flatten();
                                        let response = project_row(
                                            ui,
                                            p,
                                            &project.path,
                                            branch.as_deref(),
                                            found,
                                            selected,
                                            project.path == current,
                                        );
                                        if selected && self.reveal {
                                            ui.scroll_to_rect(response.rect, None);
                                        }
                                        if response.hovered() && moving {
                                            self.selected = Some(choice.clone());
                                        }
                                        if response.clicked() && project.path.is_dir() {
                                            chosen = Some(choice);
                                        }
                                    }
                                };
                                if section == Some(Section::Recent) {
                                    egui::ScrollArea::vertical()
                                        .id_salt("projects-popup-recent")
                                        .max_height(LIST_HEIGHT)
                                        .auto_shrink([false, true])
                                        .show(ui, draw);
                                } else {
                                    draw(ui);
                                }
                                index = end;
                            }
                        }
                    }
                    let used = ui.cursor().top() - top;
                    match self.height {
                        Some(height) if filtering && used < height => ui.add_space(height - used),
                        _ if !filtering => self.height = Some(used),
                        _ => {}
                    }
                });
            });
        self.reveal = false;
        self.drawn = Some(area.response.rect);
        if clear {
            self.filter.clear();
        }
        if let Some(choice) = chosen {
            self.close();
            return Some(match choice {
                Choice::OpenFolder => Outcome::OpenFolder,
                Choice::Clone => Outcome::Clone,
                Choice::Project(path) if projects.iter().any(|project| project.path == path && project.open) => {
                    Outcome::Focus(path)
                }
                Choice::Project(path) => Outcome::Open(path),
            });
        }
        if ui.input(|input| input.pointer.any_pressed())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !anchor.contains(pointer)
            && !area.response.rect.contains(pointer)
        {
            self.close();
        }
        None
    }

    fn field(&self, ui: &mut Ui, p: &IdePalette) -> bool {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), FIELD), Sense::hover());
        accessibility::name(&response, egui::Role::TextInput, &t!("ide-projects-search"));
        accessibility::edit(&response, |node| node.set_value(self.filter.clone()));
        glyph_search(
            ui.painter(),
            Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(14.0)),
            p.text_secondary,
        );
        let room = rect.width() - 30.0 - 34.0;
        let galley = ui.painter().layout_no_wrap(self.filter.clone(), theme::regular(13.5), p.text);
        let x = rect.left() + 30.0 - (galley.size().x - room).max(0.0);
        let text_rect =
            Rect::from_min_max(pos2(rect.left() + 30.0, rect.top()), pos2(rect.right() - 34.0, rect.bottom()));
        let caret_x = x + galley.size().x + 1.0;
        let painter = ui.painter().with_clip_rect(text_rect.expand2(vec2(2.0, 0.0)).intersect(ui.clip_rect()));
        painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, p.text);
        painter.vline(caret_x, rect.center().y - 8.0..=rect.center().y + 8.0, Stroke::new(1.0, p.text));
        let cross = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(20.0));
        let cross_response = ui.interact(cross, ui.scope_id().with("projects-filter-clear"), Sense::click());
        accessibility::button_with(&cross_response, || t!("ide-projects-clear"));
        let color = if cross_response.hovered() { p.text } else { p.text_secondary };
        let c = cross.shrink(5.5);
        ui.painter().line_segment([c.left_top(), c.right_bottom()], Stroke::new(1.2, color));
        ui.painter().line_segment([c.right_top(), c.left_bottom()], Stroke::new(1.2, color));
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, p.popup_border));
        ui.add_space(4.0);
        cross_response.clicked()
    }
}

fn action_row(ui: &mut Ui, p: &IdePalette, choice: &Choice, selected: bool) -> egui::Response {
    let (label, glyph): (String, super::glyphs::Glyph) = match choice {
        Choice::OpenFolder => (t!("ide-menu-open"), glyph_folder),
        _ => (t!("ide-menu-clone"), glyph_branch),
    };
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ITEM), Sense::click());
    accessibility::item(&response, egui::Role::MenuItem, selected, &label);
    if selected {
        ui.painter().rect_filled(rect, 5, p.selection);
    }
    glyph(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(13.0)),
        p.text_secondary,
    );
    ui.painter().text(
        pos2(rect.left() + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        theme::regular(13.0),
        p.text,
    );
    response
}

fn heading_row(ui: &mut Ui, p: &IdePalette, section: Section) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
    let text = ui.painter().text(
        pos2(rect.left() + 6.0, rect.center().y + 2.0),
        Align2::LEFT_CENTER,
        section.title(),
        theme::semibold(12.0),
        p.text_secondary,
    );
    accessibility::heading(ui, ui.scope_id().with(("projects-heading", section.id())), text, || section.title());
}

fn marked_line(text: &str, marked: &[usize], font: FontId, color: Color32, width: f32, p: &IdePalette) -> LayoutJob {
    let plain = TextFormat::simple(font, color);
    let mark = TextFormat { background: p.search_highlight, ..plain.clone() };
    let mut job = LayoutJob::default();
    for (index, character) in text.chars().enumerate() {
        let format = if marked.contains(&index) { mark.clone() } else { plain.clone() };
        job.append(character.encode_utf8(&mut [0; 4]), 0.0, format);
    }
    job.wrap =
        egui::text::TextWrapping {
            max_width: width, max_rows: 1, break_anywhere: true, overflow_character: Some('…')
        };
    job
}

fn project_row(
    ui: &mut Ui,
    p: &IdePalette,
    path: &Path,
    branch: Option<&str>,
    found: &Match,
    selected: bool,
    current: bool,
) -> egui::Response {
    let name = project_name(path);
    let exists = path.is_dir();
    let text_width = (ui.available_width() - 42.0 - 12.0).max(40.0);
    let name_color = if exists { p.text } else { p.text_secondary };
    let path_text = home_relative(path);
    let name_galley =
        ui.painter().layout_job(marked_line(&name, &found.name, theme::regular(14.0), name_color, text_width, p));
    let path_galley = ui.painter().layout_job(marked_line(
        &path_text,
        &found.path,
        theme::regular(12.5),
        p.text_secondary,
        text_width,
        p,
    ));
    let branch_galley = branch.map(|branch| {
        ui.painter().layout_job(marked_line(branch, &[], theme::regular(12.5), p.text_secondary, text_width - 17.0, p))
    });
    const GAP: f32 = 3.0;
    let text_height = name_galley.size().y
        + GAP
        + path_galley.size().y
        + branch_galley.as_ref().map_or(0.0, |galley| GAP + galley.size().y);
    let height = (text_height + 18.0).max(44.0);
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    accessibility::item(&response, egui::Role::ListBoxOption, selected, &name);
    accessibility::describe(&response, &path_text);
    let row = rect.shrink2(vec2(2.0, 1.0));
    if selected {
        ui.painter().rect_filled(row, 6, p.selection);
    } else if current {
        ui.painter().rect_filled(row, 6, p.popup_hover);
    }
    let top = row.center().y - text_height / 2.0;
    let badge =
        Rect::from_min_size(pos2(row.left() + 8.0, top + (name_galley.size().y - 22.0) / 2.0 + 2.0), Vec2::splat(22.0));
    project_badge(ui, p, badge, &name);
    let x = badge.right() + 10.0;
    let mut y = top;
    let name_height = name_galley.size().y;
    ui.painter().galley(pos2(x, y), name_galley, p.text);
    y += name_height + GAP;
    let path_height = path_galley.size().y;
    ui.painter().galley(pos2(x, y), path_galley, p.text_secondary);
    y += path_height + GAP;
    if let Some(galley) = branch_galley {
        glyph_branch(
            ui.painter(),
            Rect::from_min_size(pos2(x, y + (galley.size().y - 11.0) / 2.0), Vec2::splat(11.0)),
            p.text_secondary,
        );
        ui.painter().galley(pos2(x + 17.0, y), galley, p.text_secondary);
    }
    response
}

impl IdeView {
    fn popup_projects(&self) -> Vec<Project> {
        let open = &self.overview.open;
        open.iter()
            .map(|path| Project { path: path.clone(), open: true })
            .chain(
                self.overview
                    .recent
                    .iter()
                    .filter(|project| !open.contains(&project.path))
                    .map(|project| Project { path: project.path.clone(), open: false }),
            )
            .collect()
    }

    pub(super) fn projects_popup(&mut self, ui: &mut Ui, anchor: Rect, current: &Path) {
        if !self.projects.is_open() {
            return;
        }
        let projects = self.popup_projects();
        let p = self.palette;
        match self.projects.show(ui, &p, anchor, &projects, current) {
            Some(Outcome::OpenFolder) => self.open_folder_dialog(),
            Some(Outcome::Clone) => self.post(ui, t!("ide-in-development-notice")),
            Some(Outcome::Focus(path)) => self.requests.push(Request::Focus(path)),
            Some(Outcome::Open(path)) => self.requests.push(Request::Open(path)),
            None => {}
        }
    }

    pub(super) fn projects_popup_action(&mut self, params: &Value, current: Option<&Path>) -> Value {
        let open = params.get("open").and_then(Value::as_bool);
        let filter = params.get("filter").and_then(Value::as_str);
        match (open, filter) {
            (Some(false), _) => self.projects.close(),
            (None, None) => self.projects.toggle(),
            _ => {
                if !self.projects.is_open() {
                    self.projects.show_empty();
                }
                if let Some(filter) = filter {
                    self.projects.set_filter(filter);
                }
            }
        }
        let projects = self.popup_projects();
        self.projects.state(&projects, current)
    }
}

#[cfg(test)]
mod tests {
    use egui::{Modifiers, Pos2};

    use super::*;

    #[test]
    fn the_other_layout_is_read_both_ways() {
        assert_eq!(layouts("ишд"), ["ишд", "bil"]);
        assert_eq!(layouts("bil"), ["bil", "ишд"]);
    }

    #[test]
    fn wrong_layout_finds_the_project() {
        let found = project_match("ишд", "billing-service", "~/p/billing-service").unwrap();
        assert_eq!(found.name, [0, 1, 2], "bil");
        assert!(project_match("ишд", "zen-ide", "~/p/zen-ide").is_none());
        assert!(project_match("ифы", "billing-service", "~/p/billing-service").is_none(), "bas");
        assert!(project_match("иы", "billing-service", "~/p/billing-service").is_some(), "bs in Russian keys");
        assert!(project_match("ерф", "тест", "~/p/тест").is_none());
        assert!(project_match("ntcn", "тест", "~/p/тест").is_some(), "Latin keys find a Russian name");
    }

    #[test]
    fn initials_word_starts_and_the_path() {
        let found = project_match("bs", "billing-service", "~/p/billing-service").unwrap();
        assert_eq!(found.name, [0, 8], "the initials are marked");
        let skills = project_match("fps", "fast-path-search", "~/f/fast-path-search").unwrap();
        assert_eq!(skills.name, [0, 5, 10]);
        let by_path = project_match("acme", "web-server", "~/Projects/acme/backend/web-server").unwrap();
        assert!(by_path.name.is_empty() && by_path.path.len() == 4);
        assert!(skills.score > by_path.score, "the name before the path");
        assert!(project_match("xyz", "web-server", "~/Projects/acme/backend/web-server").is_none());
        assert!(
            project_match("fps", "billing-service", "~/client/billing-service").is_none(),
            "scattered letters of the path do not match"
        );
    }

    fn projects() -> Vec<Project> {
        let project = |path: &str, open: bool| Project { path: PathBuf::from(path), open };
        vec![
            project("/p/billing-service", true),
            project("/p/fast-path-scheduler", false),
            project("/p/zen-ide", false),
            project("/q/acme/web-server", false),
            project("/p/fast-path-search", false),
        ]
    }

    fn names(popup: &ProjectsPopup) -> Vec<String> {
        let projects = projects();
        popup
            .rows(&projects)
            .iter()
            .filter_map(|row| match row {
                Row::Project { index, .. } => Some(project_name(&projects[*index].path)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_best_match_comes_first_in_the_group_with_the_best_match_first() {
        let mut popup = ProjectsPopup::default();
        popup.show_empty();
        assert_eq!(names(&popup).len(), 5);
        popup.set_filter("fps");
        assert_eq!(names(&popup), ["fast-path-search", "fast-path-scheduler"]);
        popup.set_filter("ишд");
        assert_eq!(names(&popup), ["billing-service"]);
        popup.set_filter("zzz");
        assert!(names(&popup).is_empty());
        assert!(matches!(popup.rows(&projects()).as_slice(), [Row::Nothing]), "and says so");
        popup.set_filter("zi");
        let rows = popup.rows(&projects());
        assert!(matches!(rows[0], Row::Heading(Section::Recent)), "the group with the best match first");
    }

    struct Harness {
        ctx: egui::Context,
        popup: ProjectsPopup,
        outcomes: Vec<Outcome>,
        left: Vec<Event>,
    }

    impl Harness {
        fn new() -> Self {
            zen_i18n::register(crate::toolbox::LOCALES);
            let ctx = egui::Context::default();
            theme::install(&ctx);
            let mut popup = ProjectsPopup::default();
            popup.toggle();
            Self { ctx, popup, outcomes: Vec::new(), left: Vec::new() }
        }

        fn frame(&mut self, events: Vec<Event>) {
            let input = egui::RawInput {
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0))),
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                let anchor = Rect::from_min_size(pos2(100.0, 10.0), vec2(160.0, 28.0));
                let palette = IdePalette::zen_dark();
                self.outcomes.extend(self.popup.show(
                    ui,
                    &palette,
                    anchor,
                    &projects(),
                    Path::new("/p/billing-service"),
                ));
                self.left.extend(ui.input(|input| input.events.clone()));
            });
            output.textures_delta.clear();
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]);
        }
    }

    #[test]
    fn typing_filters_at_once_and_enter_opens_the_best_match() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        assert!(harness.popup.filter().is_empty());
        harness.frame(vec![Event::Text("ишд".to_owned())]);
        assert_eq!(harness.popup.filter(), "ишд", "no click into a field");
        assert_eq!(names(&harness.popup), ["billing-service"]);
        harness.key(Key::Backspace);
        assert_eq!(harness.popup.filter(), "иш");
        harness.frame(vec![Event::Text("д".to_owned())]);
        harness.key(Key::Escape);
        assert!(harness.popup.is_open() && harness.popup.filter().is_empty(), "⎋ clears first");
        harness.frame(vec![Event::Text("fps".to_owned())]);
        assert_eq!(harness.popup.selected, Some(Choice::Project(PathBuf::from("/p/fast-path-search"))));
        harness.key(Key::ArrowDown);
        assert_eq!(harness.popup.selected, Some(Choice::Project(PathBuf::from("/p/fast-path-scheduler"))));
        harness.key(Key::ArrowUp);
        harness.key(Key::Enter);
        assert_eq!(harness.outcomes, [Outcome::Open(PathBuf::from("/p/fast-path-search"))]);
        assert!(!harness.popup.is_open());
        assert!(harness.left.is_empty(), "nothing typed reaches the editor: {:?}", harness.left);
    }

    #[test]
    fn enter_on_an_open_project_brings_it_to_the_front_and_escape_twice_closes() {
        let mut harness = Harness::new();
        harness.frame(vec![Event::Text("BS".to_owned())]);
        harness.key(Key::Enter);
        assert_eq!(harness.outcomes, [Outcome::Focus(PathBuf::from("/p/billing-service"))]);
        harness.popup.toggle();
        harness.frame(vec![Event::Text("x".to_owned())]);
        harness.key(Key::Escape);
        harness.key(Key::Escape);
        assert!(!harness.popup.is_open());
    }

    #[test]
    fn the_popup_keeps_its_size_while_filtering_and_says_when_nothing_matches() {
        let mut harness = Harness::new();
        harness.frame(Vec::new());
        harness.frame(Vec::new());
        let unfiltered = harness.popup.drawn.unwrap().size();
        harness.frame(vec![Event::Text("qqq".to_owned())]);
        harness.frame(Vec::new());
        assert!(matches!(harness.popup.rows(&projects()).as_slice(), [Row::Nothing]));
        assert_eq!(harness.popup.drawn.unwrap().size(), unfiltered);
        harness.frame(vec![Event::Text("\u{8}".to_owned())]);
        assert_eq!(harness.popup.filter(), "qqq", "control characters are not typed");
    }
}

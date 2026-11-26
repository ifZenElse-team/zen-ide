use std::path::Path;

use egui::{Align2, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::glyphs::{Glyph, glyph_split_right};
use super::{IdePalette, Side, dialog_frame, hint};
use crate::{accessibility, theme};

const ROW: f32 = 26.0;

const TOOLS_WIDTH: f32 = 190.0;

const SHOW_AFTER: f64 = 0.15;

const TOOLS: [&str; 12] = [
    "commit",
    "project",
    "bookmarks",
    "find",
    "run",
    "debug",
    "problems",
    "structure",
    "services",
    "git",
    "terminal",
    "notifications",
];

const IN_DEVELOPMENT: [(&str, &str, Glyph); 0] = [];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    RecentFiles,
    Switcher,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Column {
    Tools,
    #[default]
    Files,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Action {
    Tool(&'static str),
    RecentLocations,
    Failed(zen_core::DispatchError),
}

struct ToolRow {
    glyph: Glyph,
    name: String,
    shortcut: String,
    action: Option<&'static str>,
}

fn tool_rows() -> Vec<ToolRow> {
    TOOLS
        .iter()
        .filter_map(|id| match super::tool_stripes::kind(id) {
            Some(kind) => Some(ToolRow {
                glyph: kind.glyph,
                name: t!(kind.title),
                shortcut: crate::keymap::shortcut(kind.action).unwrap_or_default(),
                action: Some(kind.action),
            }),
            None => IN_DEVELOPMENT.iter().find(|(planned, ..)| planned == id).map(|&(_, title, glyph)| ToolRow {
                glyph,
                name: t!(title),
                shortcut: String::new(),
                action: None,
            }),
        })
        .collect()
}

struct InnerRow {
    workspace: String,
    tab: String,
    title: String,
    owner: String,
    icon: String,
}

fn inner_rows(session: &Session, filter: &str) -> Vec<InnerRow> {
    let Some(workspaces) = session.state::<zen_workbench::workspaces::Workspaces>() else { return Vec::new() };
    let filter = filter.to_lowercase();
    workspaces
        .registered()
        .iter()
        .flat_map(|info| {
            workspaces.tabs(&info.id).iter().filter(|tab| tab.path.is_none()).map(|tab| InnerRow {
                workspace: info.id.clone(),
                tab: tab.id.clone(),
                title: tab.title.clone(),
                owner: info.title.clone(),
                icon: info.icon.clone(),
            })
        })
        .filter(|row| row.title.to_lowercase().contains(&filter))
        .collect()
}

#[derive(Default)]
pub(super) struct RecentFiles {
    open: bool,
    mode: Mode,
    filter: String,
    selected: usize,
    tool: usize,
    column: Column,
    edited_only: bool,
    hold: Option<Modifiers>,
    steps: isize,
    opened_at: Option<f64>,
}

impl RecentFiles {
    #[cfg(test)]
    pub(super) fn typed(&self) -> &str {
        &self.filter
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn edited_only(&self) -> bool {
        self.edited_only
    }

    pub(super) fn name(&self) -> Option<&'static str> {
        self.open.then_some(match self.mode {
            Mode::Switcher => "switcher",
            Mode::RecentFiles => "recentFiles",
        })
    }

    pub(super) fn press(&mut self) {
        if self.open && self.mode == Mode::RecentFiles {
            self.edited_only = !self.edited_only;
            self.selected = 0;
        } else {
            *self = Self { open: true, selected: 1, ..Self::default() };
        }
    }

    pub(super) fn open_with(&mut self, edited_only: bool) {
        *self = Self { open: true, selected: usize::from(!edited_only), edited_only, ..Self::default() };
    }

    pub(super) fn switch(&mut self, backward: bool, hold: Option<Modifiers>) {
        if !self.open {
            *self = Self { open: true, mode: Mode::Switcher, hold, ..Self::default() };
        }
        self.steps += if backward { -1 } else { 1 };
    }

    fn step(&mut self, delta: isize, files: usize, wrap: bool) {
        let (index, len) = match self.column {
            Column::Files => (&mut self.selected, files),
            Column::Tools => (&mut self.tool, TOOLS.len()),
        };
        if len == 0 {
            return;
        }
        let moved = *index as isize + delta;
        *index = if wrap { moved.rem_euclid(len as isize) as usize } else { moved.clamp(0, len as isize - 1) as usize };
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        palette: &IdePalette,
    ) -> Option<Action> {
        if !self.open {
            return None;
        }
        let now = ui.input(|input| input.time);
        let opened_at = *self.opened_at.get_or_insert(now);
        let params = json!({ "editedOnly": self.edited_only, "query": self.filter });
        let recent = registry.dispatch(session, "navigate.recent.files", params).ok().unwrap_or_default();
        let mut files: Vec<(String, String, Vec<std::ops::Range<usize>>)> = recent
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|file| {
                let ranges = super::search_everywhere::ranges_of(&file["ranges"]);
                Some((file["path"].as_str()?.to_owned(), file["relative"].as_str()?.to_owned(), ranges))
            })
            .collect();
        let tools = tool_rows();
        let inner = if self.edited_only { Vec::new() } else { inner_rows(session, &self.filter) };
        let count = files.len() + inner.len();
        if count == 0 && self.mode == Mode::Switcher {
            self.column = Column::Tools;
        }
        self.selected = self.selected.min(count.saturating_sub(1));
        let steps = std::mem::take(&mut self.steps);
        if steps != 0 {
            self.step(steps, count, true);
        }

        let hold = self.hold.unwrap_or_default();
        let (mut open, mut split, mut close, mut activate, mut action) = (None, None, false, false, None);
        let mut tab: Option<usize> = None;
        let keys = ui.input_mut(|input| {
            for event in &input.events {
                if let egui::Event::Text(text) | egui::Event::Paste(text) = event {
                    self.filter.push_str(text.lines().next().unwrap_or_default());
                    self.selected = 0;
                }
            }
            take_keys(input, hold)
        });
        super::filter_popups::take_typing(ui);
        for (key, modifiers) in keys {
            let remove = match key {
                Key::ArrowDown | Key::ArrowUp => {
                    self.step(if key == Key::ArrowDown { 1 } else { -1 }, count, false);
                    false
                }
                Key::Tab => {
                    self.step(if modifiers.shift { -1 } else { 1 }, count, true);
                    false
                }
                Key::ArrowLeft => {
                    self.column = Column::Tools;
                    false
                }
                Key::ArrowRight => {
                    if count > 0 {
                        self.column = Column::Files;
                    }
                    false
                }
                Key::Enter if modifiers.shift => {
                    if self.column == Column::Files {
                        split = files.get(self.selected).map(|(path, ..)| path.clone());
                    }
                    false
                }
                Key::Enter => {
                    activate = true;
                    false
                }
                Key::Escape => {
                    close = true;
                    false
                }
                Key::Backspace if !modifiers.command => self.filter.pop().is_none(),
                _ => true,
            };
            if remove
                && self.column == Column::Files
                && let Some(row) = self.selected.checked_sub(files.len()).and_then(|index| inner.get(index))
            {
                let params = json!({ "workspace": row.workspace, "tab": row.tab });
                if let Err(error) = registry.dispatch(session, "workspace.tab.close", params) {
                    action = Some(Action::Failed(error));
                }
            } else if remove
                && self.column == Column::Files
                && let Some((path, ..)) = files.get(self.selected)
            {
                match registry.dispatch(session, "navigate.recent.remove", json!({ "path": path })) {
                    Ok(_) => {
                        files.remove(self.selected);
                        self.selected = self.selected.min(files.len().saturating_sub(1));
                    }
                    Err(error) => action = Some(Action::Failed(error)),
                }
            }
            if activate || close || split.is_some() || action.is_some() {
                break;
            }
        }
        let released = self.hold.is_some_and(|hold| {
            let held = ui.input(|input| input.modifiers);
            (hold.ctrl && !held.ctrl) || (hold.alt && !held.alt) || (hold.command && !held.command)
        });
        if activate || released {
            match self.column {
                Column::Files if self.selected >= files.len() => tab = Some(self.selected - files.len()),
                Column::Files => open = files.get(self.selected).map(|(path, ..)| path.clone()),
                Column::Tools => action = tools.get(self.tool).and_then(|tool| tool.action).map(Action::Tool),
            }
            close = true;
        }
        if !close && split.is_none() && self.hold.is_some() && now - opened_at < SHOW_AFTER {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(SHOW_AFTER - (now - opened_at)));
        } else if !close && split.is_none() {
            let (clicked, picked) = self.draw(ui, palette, &files, &inner, &tools);
            if let Some(picked) = clicked {
                action = Some(picked);
            }
            match picked {
                Some(Pick::Open(path)) => open = Some(path),
                Some(Pick::Split(path)) => split = Some(path),
                Some(Pick::Tab(index)) => tab = Some(index),
                Some(Pick::Outside) => close = true,
                None => {}
            }
        }
        if let Some(row) = tab.and_then(|index| inner.get(index)) {
            let params = json!({ "workspace": row.workspace, "tab": row.tab });
            if let Err(error) = registry.dispatch(session, "workspace.tab.select", params) {
                action = Some(Action::Failed(error));
            }
            close = true;
        } else if let Some(path) = split {
            let _ = registry.dispatch(session, "editor.split.openRight", json!({ "path": path }));
            close = true;
        } else if let Some(path) = open {
            let _ = registry.dispatch(session, "file.document.open", json!({ "path": path }));
            close = true;
        }
        if close || action.is_some() {
            self.open = false;
        }
        action
    }

    fn draw(
        &mut self,
        ui: &mut Ui,
        palette: &IdePalette,
        files: &[(String, String, Vec<std::ops::Range<usize>>)],
        inner: &[InnerRow],
        tools: &[ToolRow],
    ) -> (Option<Action>, Option<Pick>) {
        let (mut action, mut pick) = (None, None);
        let screen = ui.ctx().content_rect();
        let body_height = (tools.len() as f32 * ROW + 8.0 + ROW).max(294.0);
        let size = vec2(680.0, body_height + 86.0);
        let area = egui::Area::new(ui.scope_id().with("recent-files"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(screen.center().x - size.x / 2.0, screen.center().y - size.y / 2.0 - 40.0));
        let response = area.show(ui.ctx(), |ui| {
            dialog_frame(palette).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.set_width(size.x - 20.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let name = match self.mode {
                    Mode::Switcher => t!("ide-switcher-title"),
                    Mode::RecentFiles => t!("ide-recent-files-title"),
                };
                let title = if self.filter.is_empty() { name } else { format!("{name}: {}", self.filter) };
                let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
                ui.painter().text(
                    pos2(bar.left(), bar.center().y),
                    Align2::LEFT_CENTER,
                    title,
                    theme::semibold(13.0),
                    palette.text,
                );
                if self.mode == Mode::RecentFiles {
                    let toggle = check_option(
                        ui,
                        bar.right_center(),
                        self.edited_only,
                        &t!("ide-recent-edited-only"),
                        "⌘E",
                        palette,
                    );
                    if toggle.clicked() {
                        self.edited_only = !self.edited_only;
                        self.selected = 0;
                    }
                }
                ui.add_space(8.0);
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(TOOLS_WIDTH);
                        ui.set_min_height(body_height);
                        for (index, tool) in tools.iter().enumerate() {
                            let (rect, response) = ui.allocate_exact_size(vec2(TOOLS_WIDTH, ROW), Sense::click());
                            let selected = index == self.tool;
                            accessibility::item(
                                &response,
                                egui::Role::ListBoxOption,
                                selected && self.column == Column::Tools,
                                &tool.name,
                            );
                            accessibility::describe(&response, &tool.shortcut);
                            if selected && self.column == Column::Tools {
                                ui.painter().rect_filled(rect, 5, palette.selection);
                            } else if response.hovered() {
                                ui.painter().rect_filled(rect, 5, palette.popup_hover);
                            }
                            let ready = tool.action.is_some();
                            let color = if ready { palette.text } else { palette.text_secondary };
                            (tool.glyph)(
                                ui.painter(),
                                Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(14.0)),
                                palette.text_secondary,
                            );
                            ui.painter().text(
                                pos2(rect.left() + 28.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                &tool.name,
                                theme::regular(13.0),
                                color,
                            );
                            shortcut_text(ui, rect, &tool.shortcut, palette);
                            match tool.action {
                                Some(tool_action) if response.clicked() => action = Some(Action::Tool(tool_action)),
                                Some(_) => {}
                                None => {
                                    super::in_development(ui, &response, &tool.name, Side::Right);
                                }
                            }
                        }
                        ui.add_space(8.0);
                        let name = t!("keymap-view-recentLocations-open");
                        let (rect, response) = ui.allocate_exact_size(vec2(TOOLS_WIDTH, ROW), Sense::click());
                        accessibility::name(&response, egui::Role::Link, &name);
                        let text = ui.painter().text(
                            pos2(rect.left() + 6.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            &name,
                            theme::regular(13.0),
                            palette.link,
                        );
                        if response.hovered() {
                            let y = text.bottom() + 1.0;
                            ui.painter().line_segment(
                                [pos2(text.left(), y), pos2(text.right(), y)],
                                egui::Stroke::new(1.0, palette.link),
                            );
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        shortcut_text(ui, rect, "⇧⌘E", palette);
                        if response.clicked() {
                            action = Some(Action::RecentLocations);
                        }
                    });
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.set_min_height(body_height);
                        if files.is_empty() && inner.is_empty() {
                            let message =
                                if self.edited_only { t!("ide-recent-no-edited") } else { t!("ide-goto-file-nothing") };
                            ui.label(
                                egui::RichText::new(message).font(theme::regular(13.0)).color(palette.text_secondary),
                            );
                        }
                        egui::ScrollArea::vertical().max_height(body_height).auto_shrink([false, true]).show(
                            ui,
                            |ui| {
                                for (index, (path, relative, ranges)) in files.iter().enumerate() {
                                    let (rect, response) =
                                        ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                                    let selected = index == self.selected;
                                    if selected && self.column == Column::Files {
                                        ui.painter().rect_filled(rect, 5, palette.selection);
                                        ui.scroll_to_rect(rect, None);
                                    } else if selected || response.hovered() {
                                        ui.painter().rect_filled(rect, 5, palette.popup_hover);
                                    }
                                    let name = relative.rsplit('/').next().unwrap_or(relative);
                                    let at = relative.chars().count() - name.chars().count();
                                    let in_name: Vec<std::ops::Range<usize>> = ranges
                                        .iter()
                                        .filter(|range| range.start >= at)
                                        .map(|range| range.start - at..range.end - at)
                                        .collect();
                                    accessibility::item(
                                        &response,
                                        egui::Role::ListBoxOption,
                                        selected && self.column == Column::Files,
                                        name,
                                    );
                                    crate::file_icons::file_icon_at(
                                        ui.painter(),
                                        Rect::from_center_size(
                                            pos2(rect.left() + 14.0, rect.center().y),
                                            Vec2::splat(16.0),
                                        ),
                                        Path::new(path),
                                    );
                                    super::search_everywhere::matched_text(
                                        ui.painter(),
                                        pos2(rect.left() + 30.0, rect.center().y),
                                        name,
                                        &in_name,
                                        theme::regular(13.5),
                                        palette.text,
                                        palette,
                                    );
                                    let icon = Rect::from_center_size(
                                        pos2(rect.right() - 14.0, rect.center().y),
                                        Vec2::splat(20.0),
                                    );
                                    let pointer = ui.input(|input| input.pointer.hover_pos());
                                    let on_row = pointer.is_some_and(|at| rect.contains(at));
                                    let mut on_icon = false;
                                    if on_row {
                                        let button =
                                            ui.interact(icon, ui.scope_id().with(("split", index)), Sense::click());
                                        on_icon = button.hovered();
                                        if on_icon {
                                            ui.painter().rect_filled(icon, 4, palette.hover);
                                        }
                                        glyph_split_right(ui.painter(), icon.shrink(4.0), palette.text_secondary);
                                        hint(ui, &button, &t!("ide-tree-open-right-split"), Some("⇧↵"), Side::Below);
                                        if button.clicked() {
                                            pick = Some(Pick::Split(path.clone()));
                                        }
                                    }
                                    if response.clicked() && !on_icon {
                                        pick = Some(Pick::Open(path.clone()));
                                    }
                                }
                                for (index, row) in inner.iter().enumerate() {
                                    let (rect, response) =
                                        ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
                                    let selected = files.len() + index == self.selected;
                                    if selected && self.column == Column::Files {
                                        ui.painter().rect_filled(rect, 5, palette.selection);
                                        ui.scroll_to_rect(rect, None);
                                    } else if selected || response.hovered() {
                                        ui.painter().rect_filled(rect, 5, palette.popup_hover);
                                    }
                                    accessibility::item(
                                        &response,
                                        egui::Role::ListBoxOption,
                                        selected && self.column == Column::Files,
                                        &row.title,
                                    );
                                    let glyph = super::workspace_host::workspace_glyph(&row.icon);
                                    glyph(
                                        ui.painter(),
                                        Rect::from_center_size(
                                            pos2(rect.left() + 14.0, rect.center().y),
                                            Vec2::splat(14.0),
                                        ),
                                        palette.text_secondary,
                                    );
                                    ui.painter().text(
                                        pos2(rect.left() + 30.0, rect.center().y),
                                        Align2::LEFT_CENTER,
                                        &row.title,
                                        theme::regular(13.5),
                                        palette.text,
                                    );
                                    if response.clicked() {
                                        pick = Some(Pick::Tab(index));
                                    }
                                }
                            },
                        );
                    });
                });
                ui.add_space(6.0);
                let (footer, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
                ui.painter().line_segment(
                    [pos2(footer.left(), footer.top()), pos2(footer.right(), footer.top())],
                    egui::Stroke::new(1.0, palette.tab_border),
                );
                let shown = match files.get(self.selected) {
                    Some((path, ..)) => Some(with_home(Path::new(path))),
                    None => self
                        .selected
                        .checked_sub(files.len())
                        .and_then(|index| inner.get(index))
                        .map(|row| t!("a11y-workspace", title = row.owner.as_str())),
                };
                if let Some(shown) = shown.filter(|_| self.column == Column::Files) {
                    ui.painter().with_clip_rect(footer).text(
                        pos2(footer.left() + 4.0, footer.center().y + 3.0),
                        Align2::LEFT_CENTER,
                        shown,
                        theme::regular(12.0),
                        palette.text_secondary,
                    );
                }
            });
        });
        if pick.is_none()
            && ui.input(|input| input.pointer.any_pressed())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !response.response.rect.contains(pointer)
        {
            pick = Some(Pick::Outside);
        }
        (action, pick)
    }
}

enum Pick {
    Open(String),
    Split(String),
    Tab(usize),
    Outside,
}

fn take_keys(input: &mut egui::InputState, hold: Modifiers) -> Vec<(Key, Modifiers)> {
    let mut taken = Vec::new();
    input.events.retain(|event| {
        let egui::Event::Key { key, pressed: true, modifiers, .. } = *event else { return true };
        let ours = matches!(
            key,
            Key::ArrowDown
                | Key::ArrowUp
                | Key::ArrowLeft
                | Key::ArrowRight
                | Key::Tab
                | Key::Enter
                | Key::Escape
                | Key::Delete
                | Key::Backspace
        );
        let held = (!modifiers.shift || matches!(key, Key::Tab | Key::Enter))
            && (!modifiers.alt || hold.alt)
            && (!modifiers.ctrl || hold.ctrl)
            && (!modifiers.command || hold.command)
            && (!modifiers.mac_cmd || hold.mac_cmd);
        let command_backspace = key == Key::Backspace && modifiers.command && !modifiers.shift && !modifiers.alt;
        if ours && (held || command_backspace) {
            taken.push((key, modifiers));
            return false;
        }
        true
    });
    taken
}

fn shortcut_text(ui: &Ui, rect: Rect, shortcut: &str, palette: &IdePalette) {
    ui.painter().text(
        pos2(rect.right() - 6.0, rect.center().y),
        Align2::RIGHT_CENTER,
        shortcut,
        theme::regular(12.0),
        palette.text_secondary,
    );
}

pub(super) fn check_option(
    ui: &mut Ui,
    right: egui::Pos2,
    checked: bool,
    label: &str,
    shortcut: &str,
    palette: &IdePalette,
) -> egui::Response {
    let painter = ui.painter();
    let shortcut_galley = painter.layout_no_wrap(shortcut.to_owned(), theme::regular(12.0), palette.text_secondary);
    let label_galley = painter.layout_no_wrap(label.to_owned(), theme::regular(13.0), palette.text);
    let gap = if shortcut.is_empty() { 0.0 } else { 8.0 };
    let width = 14.0 + 6.0 + label_galley.size().x + gap + shortcut_galley.size().x;
    let rect = Rect::from_min_max(pos2(right.x - width, right.y - 10.0), pos2(right.x, right.y + 10.0));
    let response = ui.interact(rect, ui.scope_id().with(("check-option", label)), Sense::click());
    accessibility::name(&response, egui::Role::CheckBox, label);
    accessibility::pressed(&response, checked);
    let boxed = Rect::from_center_size(pos2(rect.left() + 7.0, rect.center().y), Vec2::splat(14.0));
    super::commit_window::checkbox(ui.painter(), boxed, Some(checked), palette);
    let label_at = pos2(boxed.right() + 6.0, rect.center().y - label_galley.size().y / 2.0);
    let label_width = label_galley.size().x;
    ui.painter().galley(label_at, label_galley, palette.text);
    let shortcut_at = pos2(label_at.x + label_width + gap, rect.center().y - shortcut_galley.size().y / 2.0);
    ui.painter().galley(shortcut_at, shortcut_galley, palette.text_secondary);
    response
}

pub(super) fn with_home(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_e_again_toggles_show_edited_only() {
        let mut popup = RecentFiles::default();
        popup.press();
        assert!(popup.is_open() && !popup.edited_only());
        assert_eq!(popup.selected, 1, "the previous file is selected");
        popup.press();
        assert!(popup.edited_only(), "⌘E again shows the edited files only");
        popup.press();
        assert!(!popup.edited_only());
        popup.open = false;
        popup.press();
        assert!(!popup.edited_only(), "it opens with every file again");
        popup.open_with(true);
        assert!(popup.is_open() && popup.edited_only());
    }

    #[test]
    fn the_switcher_opens_moving_down_or_up_and_command_e_turns_it_into_recent_files() {
        let mut popup = RecentFiles::default();
        popup.switch(false, Some(Modifiers::CTRL));
        assert_eq!(popup.name(), Some("switcher"));
        assert_eq!(popup.steps, 1, "to the previous file");
        popup.switch(true, Some(Modifiers::CTRL));
        assert_eq!(popup.steps, 0, "⌃⇧⇥ moves back up");
        popup.step(-1, 3, true);
        assert_eq!(popup.selected, 2, "up from the first file wraps to the last");
        popup.column = Column::Tools;
        popup.step(-1, 3, true);
        assert_eq!(popup.tool, TOOLS.len() - 1);
        popup.press();
        assert_eq!(popup.name(), Some("recentFiles"));
        assert_eq!((popup.selected, popup.column), (1, Column::Files));
    }

    #[test]
    fn the_tool_windows_column_lists_every_tool_window_with_its_shortcut() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let rows = tool_rows();
        let shortcuts: Vec<&str> = rows.iter().map(|row| row.shortcut.as_str()).collect();
        assert_eq!(shortcuts, ["⌘0", "⌘1", "⌘2", "⌘3", "⌘4", "⌘5", "⌘6", "⌘7", "⌘8", "⌘9", "⌥F12", ""]);
        assert!(rows.iter().all(|row| !row.name.is_empty()));
        let planned: Vec<&str> = rows.iter().filter(|row| row.action.is_none()).map(|row| row.name.as_str()).collect();
        assert!(planned.is_empty(), "every tool window of the list is built: {planned:?}");
        for (id, ..) in IN_DEVELOPMENT {
            assert!(super::super::tool_stripes::kind(id).is_none(), "{id} is built: take it out of IN_DEVELOPMENT");
        }
    }

    #[test]
    fn keys_held_with_the_switcher_still_reach_it_one_by_one() {
        let ctx = egui::Context::default();
        let key =
            |key, modifiers| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        let input = egui::RawInput {
            events: vec![
                key(Key::ArrowDown, Modifiers::CTRL),
                key(Key::ArrowUp, Modifiers::COMMAND),
                key(Key::ArrowDown, Modifiers::CTRL),
                key(Key::Tab, Modifiers::CTRL | Modifiers::SHIFT),
                key(Key::ArrowDown, Modifiers::SHIFT),
                key(Key::Backspace, Modifiers::COMMAND),
            ],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            ui.input_mut(|input| {
                let keys: Vec<Key> = take_keys(input, Modifiers::CTRL).into_iter().map(|(key, _)| key).collect();
                assert_eq!(keys, [Key::ArrowDown, Key::ArrowDown, Key::Tab, Key::Backspace], "⌘↑ and ⇧↓ are not ours");
                assert_eq!(input.events.len(), 2);
            });
        });
        output.textures_delta.clear();
    }

    #[test]
    fn the_switcher_lists_the_tool_windows_and_the_files() {
        let project = tempfile::tempdir().unwrap();
        for name in ["main.go", "util.go", "go.mod"] {
            std::fs::write(project.path().join(name), "package main\n").unwrap();
        }
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        for name in ["go.mod", "util.go", "main.go"] {
            registry
                .dispatch(&mut session, "file.document.open", json!({ "path": project.path().join(name) }))
                .unwrap();
        }
        let palette = IdePalette::zen_dark();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS");
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        zen_i18n::register(crate::toolbox::LOCALES);
        for language in languages {
            if snapshots.is_some() {
                zen_i18n::set_language(Some(language));
            }
            let mut popup = RecentFiles::default();
            popup.switch(false, None);
            let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 560.0), 2.0);
            palette.apply(screen.context());
            let mut draw = |ui: &mut Ui| {
                ui.painter().rect_filled(ui.max_rect(), 0, palette.island);
                popup.show(ui, &registry, &mut session, &palette);
            };
            screen.render(&mut draw);
            let image = screen.render(&mut draw);
            let tree = screen.accessibility_tree(&mut draw);
            let labels: Vec<String> =
                tree.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect();
            let rows = tool_rows();
            for wanted in rows.iter().map(|row| row.name.as_str()).chain(["main.go", "util.go", "go.mod"]) {
                assert!(labels.iter().any(|label| label == wanted), "{wanted} in {labels:?}");
            }
            let selected = tree.nodes.iter().find(|(_, node)| node.is_selected() == Some(true));
            assert_eq!(selected.and_then(|(_, node)| node.label()), Some("util.go"), "the previous file is selected");
            if let Some(dir) = &snapshots {
                let file = Path::new(dir).join(format!("switcher-{language}.png"));
                crate::offscreen::write_png(&file, &image).unwrap();
            }
        }
        if snapshots.is_some() {
            zen_i18n::set_language(Some("en"));
        }
    }

    #[test]
    fn paths_under_home_are_written_with_a_tilde() {
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else { return };
        assert_eq!(with_home(&home.join("go/main.go")), "~/go/main.go");
        assert_eq!(with_home(Path::new("/opt/x.go")), "/opt/x.go");
    }
}

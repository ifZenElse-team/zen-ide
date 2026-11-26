use std::time::{Duration, Instant};

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::find_results::{
    FindResults, NodeKind, ResultNode, ResultsKind, ResultsTree, ViewOption, ViewOptions,
};

use super::find_window::FindAction;
use super::glyphs::{
    glyph_collapse_all, glyph_expand_all, glyph_eye, glyph_group, glyph_plus, glyph_rerun, glyph_settings,
};
use super::search_filter::menu_row_with;
use super::{IdePalette, IdeView, Side, customization, dialog_frame, hint, hover};
use crate::{accessibility, theme};

const ROW: f32 = 24.0;
const TOOLBAR: f32 = 32.0;
const INDENT: f32 = 16.0;
const PREVIEW_LINES: usize = 21;
const REREAD_WHILE_SEARCHING: Duration = Duration::from_millis(400);

#[derive(Debug)]
struct SourcePreview {
    node: String,
    first: usize,
    lines: Vec<String>,
    line: usize,
    highlights: Vec<[usize; 2]>,
}

#[derive(Debug, Default)]
pub(super) struct ResultsTab {
    pub(super) id: u64,
    pub(super) tree: Option<ResultsTree>,
    pub(super) stale: bool,
    rows: Vec<(String, Rect)>,
    reveal: bool,
    offset: f32,
    read_at: Option<Instant>,
    preview: Option<SourcePreview>,
    tree_id: Option<egui::Id>,
}

const GROUP_BY: [(ViewOption, &str, &str); 8] = [
    (ViewOption::DirectoryStructure, "ide-results-group-directory-structure", "⌃D"),
    (ViewOption::FileStructure, "ide-results-group-file-structure", "⌃S"),
    (ViewOption::Module, "ide-results-group-module", "⌃M"),
    (ViewOption::FlattenModules, "ide-results-group-flatten-modules", "⌃O"),
    (ViewOption::Directory, "ide-results-group-directory", "⌃P"),
    (ViewOption::ProductionTest, "ide-results-group-production-test", ""),
    (ViewOption::UsageType, "ide-results-group-usage-type", "⌃T"),
    (ViewOption::MergeSameLine, "ide-results-group-merge-lines", "⌃L"),
];

pub(super) fn own_key(key: Key, modifiers: Modifiers) -> bool {
    let command = modifiers.command && !modifiers.ctrl;
    let control = modifiers.ctrl && !modifiers.command && !modifiers.alt && !modifiers.shift;
    let plain_alt = modifiers.alt && !modifiers.command && !modifiers.ctrl && !modifiers.shift;
    (command && !modifiers.alt && !modifiers.shift && matches!(key, Key::R | Key::Equals | Key::Plus | Key::Minus))
        || (command && modifiers.shift && !modifiers.alt && key == Key::F)
        || (command && modifiers.alt && !modifiers.shift && matches!(key, Key::ArrowUp | Key::ArrowDown))
        || (control && matches!(key, Key::D | Key::S | Key::M | Key::O | Key::P | Key::T | Key::L))
        || (plain_alt && matches!(key, Key::L | Key::D))
        || (modifiers.is_none() && matches!(key, Key::Backspace | Key::Delete))
}

fn command(name: &'static str, params: Value) -> FindAction {
    FindAction::Command(name, params)
}

fn option_key(key: Key) -> Option<ViewOption> {
    Some(match key {
        Key::D => ViewOption::DirectoryStructure,
        Key::S => ViewOption::FileStructure,
        Key::M => ViewOption::Module,
        Key::O => ViewOption::FlattenModules,
        Key::P => ViewOption::Directory,
        Key::T => ViewOption::UsageType,
        Key::L => ViewOption::MergeSameLine,
        _ => return None,
    })
}

fn visible<'a>(nodes: &'a [ResultNode], depth: usize, out: &mut Vec<(&'a ResultNode, usize)>) {
    for node in nodes {
        out.push((node, depth));
        if !node.collapsed {
            visible(&node.children, depth + 1, out);
        }
    }
}

fn find<'a>(nodes: &'a [ResultNode], id: &str) -> Option<&'a ResultNode> {
    nodes.iter().find_map(|node| if node.id == id { Some(node) } else { find(&node.children, id) })
}

impl ResultsTab {
    pub(super) fn new(id: u64) -> Self {
        Self { id, stale: true, reveal: true, ..Self::default() }
    }

    pub(super) fn title(&self) -> String {
        self.tree.as_ref().map_or_else(|| t!("ide-find-searching"), |tree| tree.info.title.clone())
    }

    pub(super) fn summary(&self) -> Option<String> {
        let tree = self.tree.as_ref()?;
        let info = &tree.info;
        let in_files = info.total.saturating_sub(info.elsewhere);
        let files = (in_files > 0).then(|| {
            t!("ide-find-usages-summary", usages = t!("ide-find-usages-count", count = in_files), files = info.files)
        });
        let elsewhere = (info.elsewhere > 0).then_some(info.elsewhere);
        match (files, elsewhere) {
            (Some(files), Some(count)) => Some(t!("ide-find-usages-and-elsewhere", files = files, count = count)),
            (None, Some(count)) => {
                Some(t!("ide-find-usages-elsewhere", usages = t!("ide-find-usages-count", count = count)))
            }
            (files, None) => files,
        }
    }

    pub(super) fn has_focus(&self, ctx: &egui::Context) -> bool {
        self.tree_id.is_some_and(|id| ctx.memory(|memory| memory.has_focus(id)))
    }

    pub(super) fn pinned(&self) -> bool {
        self.tree.as_ref().is_some_and(|tree| tree.info.pinned)
    }

    fn selected(&self) -> Option<&ResultNode> {
        let tree = self.tree.as_ref()?;
        find(&tree.nodes, tree.selected.as_deref()?)
    }

    fn select(&mut self, id: &str, actions: &mut Vec<FindAction>) {
        if let Some(tree) = &mut self.tree {
            tree.selected = Some(id.to_owned());
        }
        self.reveal = true;
        actions.push(command("search.results.select", json!({ "tab": self.id, "node": id })));
    }

    fn open(&mut self, node: &ResultNode, focus_editor: bool, actions: &mut Vec<FindAction>) {
        if let Some(run) = &node.command {
            actions.push(FindAction::Run(run.command.clone(), run.params.clone()));
            return;
        }
        let (Some(path), Some(offset)) = (node.path.clone(), node.offset) else { return };
        actions.push(FindAction::GoTo { path, offset, focus_editor });
    }

    fn excluding(&self, node: &ResultNode) -> FindAction {
        let name = if node.excluded { "search.results.include" } else { "search.results.exclude" };
        command(name, json!({ "tab": self.id, "nodes": [node.id] }))
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        focus: &mut bool,
        (text_new_tab, usages_new_tab): (bool, bool),
        actions: &mut Vec<FindAction>,
    ) {
        let Some(tree) = &self.tree else {
            message(ui, p, t!("ide-find-searching"));
            return;
        };
        if tree.info.done && tree.info.total == 0 && tree.info.kind != ResultsKind::Text {
            message(ui, p, tree.info.error.clone().unwrap_or_else(|| t!("ide-find-none")));
            return;
        }
        let new_tab = if matches!(tree.info.kind, ResultsKind::Usages | ResultsKind::Database) {
            usages_new_tab
        } else {
            text_new_tab
        };
        let options = tree.options.clone();
        let replace = tree.info.replacement.is_some();
        let area = ui.available_rect_before_wrap();
        let toolbar = Rect::from_min_size(area.min, vec2(TOOLBAR, area.height()));
        self.toolbar(ui, p, toolbar, &options, new_tab, actions);
        ui.painter().vline(toolbar.right(), toolbar.y_range(), Stroke::new(1.0, p.tab_border));
        let footer = if replace { 40.0 } else { 0.0 };
        let body =
            Rect::from_min_max(pos2(toolbar.right() + 1.0, area.top()), pos2(area.right(), area.bottom() - footer));
        let (tree_rect, preview_rect) = if options.preview_source {
            let split = body.left() + body.width() * 0.55;
            (
                Rect::from_min_max(body.min, pos2(split, body.bottom())),
                Some(Rect::from_min_max(pos2(split + 1.0, body.top()), body.max)),
            )
        } else {
            (body, None)
        };
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(tree_rect).id_salt(("find-results-tree", self.id)));
        self.tree_view(&mut child, p, focus, actions);
        if let Some(rect) = preview_rect {
            ui.painter().vline(rect.left() - 1.0, rect.y_range(), Stroke::new(1.0, p.tab_border));
            self.preview_view(ui, p, rect);
        }
        if replace {
            let bar = Rect::from_min_max(pos2(toolbar.right() + 1.0, area.bottom() - footer), area.max);
            self.replace_bar(ui, p, bar, actions);
        }
        ui.advance_cursor_after_rect(area);
    }

    fn toolbar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        rect: Rect,
        options: &ViewOptions,
        new_tab: bool,
        actions: &mut Vec<FindAction>,
    ) {
        let tab = json!({ "tab": self.id });
        let set = |option: &str, on: bool| command("search.results.setOption", json!({ "option": option, "on": on }));
        let pinned = self.pinned();
        let usages = self
            .tree
            .as_ref()
            .is_some_and(|tree| matches!(tree.info.kind, ResultsKind::Usages | ResultsKind::Database));
        let access = self.tree.as_ref().is_some_and(|tree| tree.info.access_known);
        let new_tab_action = if usages {
            command("navigate.usagesSettings.set", json!({ "newTab": !new_tab }))
        } else {
            FindAction::ToggleNewTab
        };
        let mut tools: Vec<Tool> = vec![
            Tool::new(
                "rerun",
                t!("ide-results-rerun"),
                "⌘R",
                None,
                glyph_rerun,
                command("search.results.rerun", tab.clone()),
            ),
            Tool::new(
                "previous",
                t!("ide-results-previous"),
                "⌥⌘↑",
                None,
                arrow_up,
                command("search.results.previous", tab.clone()),
            ),
            Tool::new(
                "next",
                t!("ide-results-next"),
                "⌥⌘↓",
                None,
                arrow_down,
                command("search.results.next", tab.clone()),
            ),
            Tool::new(
                "settings",
                t!("ide-results-settings"),
                "⇧⌘F",
                None,
                glyph_settings,
                FindAction::Settings(self.id),
            ),
            Tool::new("group", t!("ide-results-group-by"), "", None, glyph_group, FindAction::FocusEditor),
            Tool::new(
                "comments",
                t!("ide-results-comments"),
                "",
                Some(options.show_in_comments),
                comment_glyph,
                set("showInComments", !options.show_in_comments),
            ),
        ];
        if access {
            tools.push(Tool::new(
                "read",
                t!("ide-results-show-read"),
                "",
                Some(options.show_read),
                read_glyph,
                set("showRead", !options.show_read),
            ));
            tools.push(Tool::new(
                "write",
                t!("ide-results-show-write"),
                "",
                Some(options.show_write),
                write_glyph,
                set("showWrite", !options.show_write),
            ));
        }
        tools.extend([
            Tool::new(
                "expand",
                t!("ide-results-expand-all"),
                "⌘+",
                None,
                glyph_expand_all,
                command("search.results.expand", json!({ "tab": self.id, "expanded": true })),
            ),
            Tool::new(
                "collapse",
                t!("ide-results-collapse-all"),
                "⌘−",
                None,
                glyph_collapse_all,
                command("search.results.expand", json!({ "tab": self.id, "expanded": false })),
            ),
            Tool::new(
                "preview",
                t!("ide-results-preview"),
                "",
                Some(options.preview_source),
                glyph_eye,
                set("previewSource", !options.preview_source),
            ),
            Tool::new(
                "pin",
                t!("ide-results-pin"),
                "",
                Some(pinned),
                super::find_in_files::glyph_pin,
                command("search.results.pin", json!({ "tab": self.id, "on": !pinned })),
            ),
            Tool::new("new-tab", t!("ide-fif-new-tab"), "", Some(new_tab), glyph_plus, new_tab_action),
        ]);
        let pitch = 28.0;
        let room = ((rect.height() - 4.0) / pitch).floor().max(1.0) as usize;
        let shown = if tools.len() <= room { tools.len() } else { room.saturating_sub(1) };
        for (index, tool) in tools.iter().enumerate().take(shown) {
            let slot = Rect::from_min_size(
                pos2(rect.center().x - 12.0, rect.top() + 4.0 + pitch * index as f32),
                Vec2::splat(24.0),
            );
            let response = ui.interact(slot, ui.scope_id().with(("find-results-tool", tool.salt)), Sense::click());
            let on = tool.on == Some(true);
            if on {
                ui.painter().rect_filled(slot, 6, p.popup_hover);
            } else {
                hover(ui, &response, p);
            }
            hint(ui, &response, &tool.name, (!tool.keys.is_empty()).then_some(tool.keys), Side::Right);
            if let Some(on) = tool.on {
                accessibility::pressed(&response, on);
            }
            (tool.glyph)(ui.painter(), slot.shrink(5.0), if on { p.text } else { p.text_secondary });
            if tool.salt == "group" {
                self.group_menu(&response, p, options, actions);
            } else if response.clicked() {
                actions.push(tool.action.clone());
            }
        }
        if shown < tools.len() {
            let slot = Rect::from_min_size(
                pos2(rect.center().x - 12.0, rect.top() + 4.0 + pitch * shown as f32),
                Vec2::splat(24.0),
            );
            let response = ui.interact(slot, ui.scope_id().with("find-results-more"), Sense::click());
            hover(ui, &response, p);
            hint(ui, &response, &t!("ide-find-more"), None, Side::Right);
            super::glyphs::glyph_more(ui.painter(), slot.shrink(5.0), p.text_secondary);
            egui::Popup::menu(&response).show(|ui| {
                ui.set_min_width(280.0);
                for tool in &tools[shown..] {
                    if tool.salt == "group" {
                        for (option, label, keys) in GROUP_BY {
                            let on = options.get(option);
                            if menu_row_with(ui, p, &t!(label), on, keys).clicked() {
                                actions
                                    .push(command("search.results.setOption", json!({ "option": option, "on": !on })));
                                ui.close();
                            }
                        }
                        continue;
                    }
                    let row = menu_row_with(ui, p, &tool.name, tool.on == Some(true), tool.keys);
                    accessibility::item(&row, egui::Role::MenuItem, tool.on == Some(true), &tool.name);
                    if row.clicked() {
                        actions.push(tool.action.clone());
                        ui.close();
                    }
                }
            });
        }
    }

    fn group_menu(
        &self,
        response: &egui::Response,
        p: &IdePalette,
        options: &ViewOptions,
        actions: &mut Vec<FindAction>,
    ) {
        egui::Popup::menu(response).show(|ui| {
            ui.set_min_width(300.0);
            for (option, label, keys) in GROUP_BY {
                let on = options.get(option);
                let text = t!(label);
                let row = menu_row_with(ui, p, &text, on, keys);
                accessibility::item(&row, egui::Role::MenuItemCheckBox, on, &text);
                if row.clicked() {
                    actions.push(command("search.results.setOption", json!({ "option": option, "on": !on })));
                    ui.close();
                }
            }
        });
    }

    fn tree_view(&mut self, ui: &mut Ui, p: &IdePalette, focus: &mut bool, actions: &mut Vec<FindAction>) {
        let id = ui.make_persistent_id("find-results-tree");
        self.tree_id = Some(id);
        let area = ui.interact(ui.max_rect(), id, Sense::click());
        accessibility::name_with(&area, egui::Role::Tree, || t!("ide-find-title"));
        if std::mem::take(focus) || area.clicked() {
            ui.memory_mut(|memory| memory.request_focus(id));
        }
        let focused = ui.memory(|memory| memory.has_focus(id));
        if focused {
            ui.memory_mut(|memory| {
                memory.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: false, horizontal_arrows: true, vertical_arrows: true, escape: true },
                );
            });
            self.keys(ui, actions);
        }
        let Some(tree) = &self.tree else { return };
        let mut rows = Vec::new();
        visible(&tree.nodes, 0, &mut rows);
        let selected = tree.selected.clone();
        let line_width =
            rows.iter().filter_map(|(node, _)| node.line).max().map_or(1, |line| line.to_string().len()) as f32 * 7.5;
        let mut clicked: Option<(String, bool, bool)> = None;
        let mut menu: Option<FindAction> = None;
        let mut drawn = Vec::new();
        let reveal = std::mem::take(&mut self.reveal);
        let mut scroll = egui::ScrollArea::both().auto_shrink([false, false]);
        let at = selected.as_deref().and_then(|id| rows.iter().position(|(node, _)| node.id == id));
        if reveal && let Some(at) = at {
            let top = at as f32 * ROW;
            let height = ui.available_height();
            if top < self.offset {
                scroll = scroll.vertical_scroll_offset(top);
            } else if top + ROW > self.offset + height {
                scroll = scroll.vertical_scroll_offset(top + ROW - height);
            }
        }
        ui.spacing_mut().item_spacing.y = 0.0;
        let output = scroll.show_rows(ui, ROW, rows.len(), |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (node, depth) in &rows[range] {
                let width = ui.available_width().max(200.0);
                let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                drawn.push((node.id.clone(), rect));
                let is_selected = selected.as_deref() == Some(node.id.as_str());
                if is_selected {
                    ui.painter().rect_filled(
                        rect.shrink2(vec2(4.0, 0.0)),
                        4,
                        if focused { p.selection } else { p.hover },
                    );
                } else if response.hovered() {
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 4, p.hover);
                } else if node.test && matches!(node.kind, NodeKind::File | NodeKind::Usage) {
                    ui.painter().rect_filled(rect, 0, p.test_row);
                }
                let left = rect.left() + 8.0 + INDENT * *depth as f32;
                let chevron = Rect::from_center_size(pos2(left + 6.0, rect.center().y), Vec2::splat(12.0));
                if !node.children.is_empty() {
                    if node.collapsed {
                        crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
                    } else {
                        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
                    }
                }
                let name = node_name(node);
                accessibility::tree_item(
                    &response,
                    *depth + 1,
                    (!node.children.is_empty()).then_some(!node.collapsed),
                    is_selected,
                    &name,
                );
                draw_node(ui, p, rect, left + 16.0, line_width, node);
                if response.clicked() || response.double_clicked() {
                    let on_chevron = response.interact_pointer_pos().is_some_and(|at| at.x < chevron.right() + 4.0);
                    clicked = Some((node.id.clone(), response.double_clicked(), on_chevron));
                }
                response.context_menu(|ui| {
                    ui.set_min_width(220.0);
                    customization::popup(
                        ui,
                        customization::FIND_RESULTS_POPUP,
                        |_| true,
                        |ui, _| {
                            let label = if node.excluded { t!("ide-find-include") } else { t!("ide-find-exclude") };
                            if menu_row_with(ui, p, &label, false, "⌫").clicked() {
                                menu = Some(self.excluding(node));
                                ui.close();
                            }
                        },
                    );
                });
            }
        });
        self.rows = drawn;
        self.offset = output.state.clamped_offset().y;
        if let Some(action) = menu {
            actions.push(action);
        }
        if let Some((id, double, on_chevron)) = clicked {
            ui.memory_mut(|memory| memory.request_focus(area.id));
            let node = self.tree.as_ref().and_then(|tree| find(&tree.nodes, &id)).cloned();
            self.select(&id, actions);
            if let Some(node) = node {
                if !node.children.is_empty() && (double || on_chevron) {
                    actions.push(command(
                        "search.results.expand",
                        json!({ "tab": self.id, "nodes": [node.id], "expanded": node.collapsed }),
                    ));
                } else if node.kind == NodeKind::Usage {
                    self.open(&node, double, actions);
                }
            }
        }
    }

    fn keys(&mut self, ui: &mut Ui, actions: &mut Vec<FindAction>) {
        let none = Modifiers::NONE;
        let (up, down, left, right, enter, escape, exclude) = ui.input_mut(|input| {
            (
                input.consume_key(none, Key::ArrowUp),
                input.consume_key(none, Key::ArrowDown),
                input.consume_key(none, Key::ArrowLeft),
                input.consume_key(none, Key::ArrowRight),
                input.consume_key(none, Key::Enter),
                input.consume_key(none, Key::Escape),
                input.consume_key(none, Key::Backspace) || input.consume_key(none, Key::Delete),
            )
        });
        let shortcut = |modifiers: Modifiers, key: Key| {
            ui.input_mut(|input| {
                let before = input.events.len();
                input.events.retain(|event| {
                    !matches!(event, egui::Event::Key { key: pressed, pressed: true, modifiers: held, .. }
                        if *pressed == key && held.matches_exact(modifiers))
                });
                input.events.len() != before
            })
        };
        let tab = json!({ "tab": self.id });
        if shortcut(Modifiers::COMMAND, Key::R) {
            actions.push(command("search.results.rerun", tab.clone()));
        }
        if shortcut(Modifiers::COMMAND | Modifiers::SHIFT, Key::F) {
            actions.push(FindAction::Settings(self.id));
        }
        if shortcut(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowUp) {
            actions.push(command("search.results.previous", tab.clone()));
        }
        if shortcut(Modifiers::COMMAND | Modifiers::ALT, Key::ArrowDown) {
            actions.push(command("search.results.next", tab.clone()));
        }
        if shortcut(Modifiers::COMMAND, Key::Equals) || shortcut(Modifiers::COMMAND, Key::Plus) {
            actions.push(command("search.results.expand", json!({ "tab": self.id, "expanded": true })));
        }
        if shortcut(Modifiers::COMMAND, Key::Minus) {
            actions.push(command("search.results.expand", json!({ "tab": self.id, "expanded": false })));
        }
        for key in [Key::D, Key::S, Key::M, Key::O, Key::P, Key::T, Key::L] {
            if shortcut(Modifiers::CTRL, key)
                && let Some(option) = option_key(key)
            {
                actions.push(command("search.results.setOption", json!({ "option": option })));
            }
        }
        let replace = self.tree.as_ref().is_some_and(|tree| tree.info.replacement.is_some());
        if shortcut(Modifiers::ALT, Key::L) && replace {
            actions.push(self.replace_selected());
        }
        if shortcut(Modifiers::ALT, Key::D) && replace {
            actions.push(command("search.results.replace", json!({ "tab": self.id, "nodes": ["found"] })));
        }
        let Some(tree) = &self.tree else { return };
        let mut rows = Vec::new();
        visible(&tree.nodes, 0, &mut rows);
        let ids: Vec<String> = rows.iter().map(|(node, _)| node.id.clone()).collect();
        let at = tree.selected.as_ref().and_then(|selected| ids.iter().position(|id| id == selected));
        let current = self.selected().cloned();
        if up || down {
            let next = match at {
                None => 0,
                Some(at) if up => at.saturating_sub(1),
                Some(at) => (at + 1).min(ids.len().saturating_sub(1)),
            };
            if let Some(id) = ids.get(next).cloned() {
                self.select(&id, actions);
            }
        }
        if let Some(node) = current {
            let group = !node.children.is_empty();
            if (left && group && !node.collapsed) || (right && group && node.collapsed) || (enter && group) {
                actions.push(command(
                    "search.results.expand",
                    json!({ "tab": self.id, "nodes": [node.id], "expanded": node.collapsed }),
                ));
            } else if left && let Some((parent, _)) = node.id.rsplit_once('/') {
                let parent = ids.iter().rev().find(|id| parent.starts_with(id.as_str())).cloned();
                if let Some(parent) = parent {
                    self.select(&parent, actions);
                }
            } else if enter && node.kind == NodeKind::Usage {
                self.open(&node, true, actions);
            }
            if exclude {
                actions.push(self.excluding(&node));
            }
        }
        if escape {
            actions.push(FindAction::FocusEditor);
        }
    }

    fn replace_selected(&self) -> FindAction {
        let node = self.tree.as_ref().and_then(|tree| tree.selected.clone());
        command("search.results.replace", json!({ "tab": self.id, "nodes": node.into_iter().collect::<Vec<_>>() }))
    }

    fn replace_bar(&self, ui: &mut Ui, p: &IdePalette, bar: Rect, actions: &mut Vec<FindAction>) {
        ui.painter().hline(bar.x_range(), bar.top(), Stroke::new(1.0, p.tab_border));
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(bar.shrink2(vec2(12.0, 6.0)))
                .layout(egui::Layout::left_to_right(egui::Align::Center))
                .id_salt(("find-results-replace", self.id)),
        );
        child.spacing_mut().item_spacing.x = 8.0;
        let done = self.tree.as_ref().is_some_and(|tree| tree.info.done);
        let selected = done && self.tree.as_ref().is_some_and(|tree| tree.selected.is_some());
        let button = |text: String| {
            egui::Button::new(egui::RichText::new(text).font(theme::regular(13.0)).color(p.text))
                .min_size(vec2(0.0, 28.0))
                .corner_radius(6)
                .stroke(Stroke::new(1.0, p.button_border))
        };
        let response = child.add_enabled(selected, button(t!("ide-results-replace-selected")));
        hint(&child, &response, &t!("ide-results-replace-selected"), Some("⌥L"), Side::Below);
        if response.clicked() {
            actions.push(self.replace_selected());
        }
        let response = child.add_enabled(done, button(t!("ide-results-replace-all")));
        hint(&child, &response, &t!("ide-results-replace-all"), Some("⌥D"), Side::Below);
        if response.clicked() {
            actions.push(command("search.results.replace", json!({ "tab": self.id, "nodes": ["found"] })));
        }
    }

    fn preview_view(&self, ui: &mut Ui, p: &IdePalette, rect: Rect) {
        ui.painter().rect_filled(rect, 0, p.editor_background);
        let Some(preview) = self
            .preview
            .as_ref()
            .filter(|preview| Some(&preview.node) == self.tree.as_ref().and_then(|tree| tree.selected.as_ref()))
        else {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                t!("ide-results-select-to-preview"),
                theme::regular(13.0),
                p.text_secondary,
            );
            return;
        };
        let family = super::metrics::editor_family(ui.ctx());
        let font = egui::FontId::new(12.5, family);
        let painter = ui.painter().with_clip_rect(rect);
        let height = 18.0;
        let top = rect.top() + 8.0;
        let gutter = 44.0;
        for (index, line) in preview.lines.iter().enumerate() {
            let y = top + index as f32 * height;
            let number = preview.first + index;
            let row = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), height));
            if number == preview.line {
                painter.rect_filled(row, 0, p.current_line);
            }
            painter.text(
                pos2(rect.left() + gutter - 8.0, row.center().y),
                Align2::RIGHT_CENTER,
                (number + 1).to_string(),
                font.clone(),
                p.gutter_text,
            );
            let mut job = LayoutJob::default();
            let plain = TextFormat { font_id: font.clone(), color: p.text, ..TextFormat::default() };
            let matched =
                TextFormat { font_id: font.clone(), color: p.text, background: p.search, ..TextFormat::default() };
            let chars: Vec<char> = line.chars().collect();
            let lead = chars.iter().take_while(|c| c.is_whitespace()).count();
            let mut at = 0;
            if number == preview.line {
                for [from, to] in &preview.highlights {
                    let (from, to) = ((lead + from).min(chars.len()), (lead + to).min(chars.len()));
                    if from < at {
                        continue;
                    }
                    job.append(&chars[at..from].iter().collect::<String>(), 0.0, plain.clone());
                    job.append(&chars[from..to].iter().collect::<String>(), 0.0, matched.clone());
                    at = to;
                }
            }
            job.append(&chars[at..].iter().collect::<String>().replace('\t', "    "), 0.0, plain);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            painter.galley(pos2(rect.left() + gutter, row.center().y - galley.size().y / 2.0), galley, p.text);
        }
    }
}

fn node_name(node: &ResultNode) -> String {
    match node.kind {
        NodeKind::Usage => format!("{}: {}", node.line.unwrap_or(0), node.label),
        _ => node.label.clone(),
    }
}

fn draw_node(ui: &Ui, p: &IdePalette, rect: Rect, left: f32, line_width: f32, node: &ResultNode) {
    let painter = ui.painter().with_clip_rect(rect);
    let text_color = if node.excluded { p.text_secondary } else { p.text };
    let mut x = left;
    match node.kind {
        NodeKind::File => {
            crate::file_icons::file_icon(
                &painter,
                Rect::from_center_size(pos2(x + 7.0, rect.center().y), Vec2::splat(15.0)),
                &node.label,
            );
            x += 20.0;
        }
        NodeKind::Directory | NodeKind::Module => {
            let color = if node.kind == NodeKind::Module { p.accent } else { p.text_secondary };
            super::glyphs::glyph_folder(
                &painter,
                Rect::from_center_size(pos2(x + 7.0, rect.center().y), Vec2::splat(14.0)),
                color,
            );
            x += 20.0;
        }
        _ => {}
    }
    let (galley, start) = if node.kind == NodeKind::Usage {
        painter.text(
            pos2(x + line_width, rect.center().y),
            Align2::RIGHT_CENTER,
            node.line.unwrap_or(0).to_string(),
            theme::regular(12.5),
            p.text_secondary,
        );
        let job = usage_job(&node.label, &node.highlights, text_color, p);
        (ui.fonts_mut(|fonts| fonts.layout_job(job)), x + line_width + 10.0)
    } else {
        let font = if node.kind == NodeKind::Found { theme::semibold(13.0) } else { theme::regular(13.0) };
        (ui.fonts_mut(|fonts| fonts.layout_no_wrap(node.label.clone(), font, text_color)), x)
    };
    let label = Rect::from_min_size(pos2(start, rect.center().y - galley.size().y / 2.0), galley.size());
    painter.galley(label.min, galley, text_color);
    if node.excluded {
        painter.hline(label.x_range(), label.center().y, Stroke::new(1.0, p.text_secondary));
    }
    let mut after = label.right() + 8.0;
    if let Some(detail) = &node.detail {
        let shown = painter.text(
            pos2(after, rect.center().y),
            Align2::LEFT_CENTER,
            detail,
            theme::regular(12.5),
            p.text_secondary,
        );
        after = shown.right() + 8.0;
    }
    if !matches!(node.kind, NodeKind::Usage | NodeKind::Targets | NodeKind::Target) {
        painter.text(
            pos2(after, rect.center().y),
            Align2::LEFT_CENTER,
            t!("ide-results-count", count = node.count),
            theme::regular(12.5),
            p.text_secondary,
        );
    }
}

fn usage_job(label: &str, highlights: &[[usize; 2]], color: Color32, p: &IdePalette) -> LayoutJob {
    let chars: Vec<char> = label.chars().collect();
    let plain = TextFormat { font_id: theme::regular(13.0), color, ..TextFormat::default() };
    let matched = TextFormat { font_id: theme::bold(13.0), color, background: p.search, ..TextFormat::default() };
    let mut job = LayoutJob::default();
    let mut at = 0;
    for [from, to] in highlights {
        let (from, to) = ((*from).min(chars.len()), (*to).min(chars.len()));
        if from < at {
            continue;
        }
        job.append(&chars[at..from].iter().collect::<String>(), 0.0, plain.clone());
        job.append(&chars[from..to].iter().collect::<String>(), 0.0, matched.clone());
        at = to;
    }
    job.append(&chars[at..].iter().collect::<String>(), 0.0, plain);
    job
}

struct Tool {
    salt: &'static str,
    name: String,
    keys: &'static str,
    on: Option<bool>,
    glyph: fn(&egui::Painter, Rect, Color32),
    action: FindAction,
}

impl Tool {
    fn new(
        salt: &'static str,
        name: String,
        keys: &'static str,
        on: Option<bool>,
        glyph: fn(&egui::Painter, Rect, Color32),
        action: FindAction,
    ) -> Self {
        Self { salt, name, keys, on, glyph, action }
    }
}

fn arrow_up(painter: &egui::Painter, rect: Rect, color: Color32) {
    arrow(painter, rect, color, true);
}

fn arrow_down(painter: &egui::Painter, rect: Rect, color: Color32) {
    arrow(painter, rect, color, false);
}

fn arrow(painter: &egui::Painter, rect: Rect, color: Color32, up: bool) {
    let stroke = super::glyphs::stroke(rect, color);
    let (x, top, bottom) = (rect.center().x, rect.top() + 1.0, rect.bottom() - 1.0);
    let w = rect.width() * 0.32;
    painter.line_segment([pos2(x, top), pos2(x, bottom)], stroke);
    let (tip, back) = if up { (top, top + w) } else { (bottom, bottom - w) };
    painter.line_segment([pos2(x - w, back), pos2(x, tip)], stroke);
    painter.line_segment([pos2(x + w, back), pos2(x, tip)], stroke);
}

fn comment_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = super::glyphs::stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    for dx in [0.18, 0.5] {
        painter.line_segment(
            [
                pos2(rect.left() + w * (dx + 0.22), rect.top() + h * 0.15),
                pos2(rect.left() + w * dx, rect.bottom() - h * 0.15),
            ],
            stroke,
        );
    }
}

fn read_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = super::glyphs::stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let eye = Rect::from_center_size(rect.center(), vec2(w * 0.9, h * 0.5));
    painter.line_segment([eye.left_center(), pos2(eye.center().x, eye.top())], stroke);
    painter.line_segment([pos2(eye.center().x, eye.top()), eye.right_center()], stroke);
    painter.line_segment([eye.left_center(), pos2(eye.center().x, eye.bottom())], stroke);
    painter.line_segment([pos2(eye.center().x, eye.bottom()), eye.right_center()], stroke);
    painter.circle_stroke(rect.center(), w * 0.14, stroke);
}

fn write_glyph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let stroke = super::glyphs::stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let tip = pos2(rect.left() + w * 0.12, rect.bottom() - h * 0.12);
    let back = pos2(rect.right() - w * 0.12, rect.top() + h * 0.12);
    let side = vec2(w * 0.12, h * 0.12);
    painter.line_segment([tip, back], stroke);
    painter.line_segment([tip + vec2(side.x, 0.0), back + vec2(side.x, side.y)], stroke);
    painter.line_segment([back, back + vec2(side.x, side.y)], stroke);
    painter.line_segment(
        [
            pos2(rect.left() + w * 0.12, rect.bottom() - h * 0.04),
            pos2(rect.right() - w * 0.2, rect.bottom() - h * 0.04),
        ],
        stroke,
    );
}

fn message(ui: &mut Ui, p: &IdePalette, text: String) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        ui.label(egui::RichText::new(text).color(p.text_secondary));
    });
}

impl IdeView {
    pub(super) fn follow_results(&mut self, ctx: &egui::Context, registry: &Registry, session: &mut Session) {
        let Some(results) = session.state::<FindResults>() else { return };
        let language = zen_i18n::language();
        let signature =
            language.bytes().fold(results.signature(), |sum, byte| sum.wrapping_mul(131).wrapping_add(u64::from(byte)));
        self.find_window.asking = results.asking();
        if results.running() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.find_window.results_seen != Some(signature) {
            let tabs = results.tabs();
            let active = results.active();
            self.find_window.results_seen = Some(signature);
            if self.find_window.sync_results(&tabs, active) {
                self.show_tool("find");
            }
        }
        let shown = self.find_window.active_results().map(|tab| tab.id);
        let running: Vec<u64> = session
            .state::<FindResults>()
            .map(|results| results.tabs().into_iter().map(|(id, _)| id).filter(|id| results.searching(*id)).collect())
            .unwrap_or_default();
        let now = Instant::now();
        for tab in self.find_window.results_tabs() {
            let searching = running.contains(&tab.id);
            let read_lately = tab.read_at.is_some_and(|at| now.duration_since(at) < REREAD_WHILE_SEARCHING);
            let wanted = tab.tree.is_none() || !(searching && (read_lately || Some(tab.id) != shown));
            if !tab.stale || !wanted {
                continue;
            }
            tab.stale = false;
            tab.read_at = Some(now);
            let read = registry.dispatch(session, "search.results.tree", json!({ "tab": tab.id }));
            if let Some(tree) = read.ok().and_then(|value| serde_json::from_value::<ResultsTree>(value).ok()) {
                tab.tree = Some(tree);
            }
        }
        if self.find_window.results_tabs().any(|tab| tab.stale) {
            ctx.request_repaint_after(REREAD_WHILE_SEARCHING);
        }
        let Some(tab) = self.find_window.active_results() else { return };
        let wanted = tab
            .tree
            .as_ref()
            .filter(|tree| tree.options.preview_source)
            .and_then(|tree| find(&tree.nodes, tree.selected.as_deref()?))
            .filter(|node| node.kind == NodeKind::Usage)
            .cloned();
        let Some(node) = wanted else { return };
        if tab.preview.as_ref().is_some_and(|preview| preview.node == node.id) {
            return;
        }
        let line = node.line.unwrap_or(1).saturating_sub(1);
        let asked = json!({ "path": node.path, "line": line, "lines": PREVIEW_LINES });
        let Ok(shown) = registry.dispatch(session, "search.text.preview", asked) else { return };
        tab.preview = Some(SourcePreview {
            node: node.id.clone(),
            first: shown["first"].as_u64().unwrap_or(0) as usize,
            lines: shown["lines"]
                .as_array()
                .map(|lines| lines.iter().map(|line| line.as_str().unwrap_or_default().to_owned()).collect())
                .unwrap_or_default(),
            line,
            highlights: node.highlights.clone(),
        });
    }

    pub(super) fn too_many_results(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let Some(tab) = self.find_window.asking else { return };
        let palette = self.palette;
        let mut choice = None;
        let modal =
            egui::Modal::new(ui.scope_id().with("fif-too-many")).frame(dialog_frame(&palette)).show(ui.ctx(), |ui| {
                ui.set_width(380.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(
                    egui::RichText::new(t!("ide-fif-too-many-title")).font(theme::semibold(15.0)).color(palette.text),
                );
                ui.add_space(10.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t!("ide-fif-too-many-question"))
                            .font(theme::regular(13.0))
                            .color(palette.text),
                    )
                    .wrap(),
                );
                ui.add_space(18.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let go_on = egui::Button::new(
                        egui::RichText::new(t!("ide-fif-too-many-continue"))
                            .font(theme::regular(13.0))
                            .color(palette.accent_text),
                    )
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(palette.accent);
                    if ui.add(go_on).clicked() {
                        choice = Some(true);
                    }
                    let abort = egui::Button::new(
                        egui::RichText::new(t!("ide-fif-too-many-abort"))
                            .font(theme::regular(13.0))
                            .color(palette.text),
                    )
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, palette.button_border));
                    if ui.add(abort).clicked() {
                        choice = Some(false);
                    }
                });
                if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter)) {
                    choice = Some(true);
                }
            });
        let Some(go_on) = choice.or_else(|| modal.should_close().then_some(false)) else { return };
        self.find_window.asking = None;
        self.dispatch(ui, registry, session, "search.results.tooMany", json!({ "tab": tab, "continue": go_on }));
    }
}

impl IdeView {
    pub(super) fn find_settings(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, tab: u64) {
        if let Err(super::agent::ActionError::Dispatch(error)) = self.reopen_find_in_files(registry, session, Some(tab))
        {
            self.report(ui, error);
        }
    }

    pub(super) fn reopen_find_in_files(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        tab: Option<u64>,
    ) -> Result<(), super::agent::ActionError> {
        use super::agent::ActionError::{Dispatch, Refused};
        let list = registry.dispatch(session, "search.results.list", json!({})).map_err(Dispatch)?;
        let wanted = tab.or_else(|| list["active"].as_u64());
        let tabs = list["tabs"].as_array().cloned().unwrap_or_default();
        let found = tabs.iter().find(|info| wanted.is_none_or(|id| info["tab"].as_u64() == Some(id))).or(tabs.last());
        let Some(request) = found.map(|info| info["request"].clone()) else {
            return Err(Refused("There is no tab of occurrences in the Find tool window".to_owned()));
        };
        match found.and_then(|info| info["kind"].as_str()) {
            Some("usages") => {
                let place = json!({ "path": request["path"], "offset": request["offset"], "scope": request["scope"] });
                return registry.dispatch(session, "navigate.usages.settings", place).map(|_| ()).map_err(Dispatch);
            }
            Some("file") => {
                registry
                    .dispatch(session, "file.document.open", json!({ "path": request["path"] }))
                    .map_err(Dispatch)?;
                let query = json!({ "query": request["query"] });
                registry.dispatch(session, "editor.find.set", query).map_err(Dispatch)?;
                registry.dispatch(session, "editor.find.open", json!({})).map_err(Dispatch)?;
                self.find_bar.focus();
                return Ok(());
            }
            _ => {}
        }
        let scopes = registry.dispatch(session, "search.scope.list", json!({})).map_err(Dispatch)?;
        let mut selection = scopes["selection"].clone();
        let directory = request["directory"].as_str().unwrap_or_default();
        let scope = request["scope"].as_str().unwrap_or(zen_workbench::search_scopes::PROJECT_FILES);
        if !directory.is_empty() {
            selection["area"] = json!("directory");
            selection["directory"] = json!(directory);
            selection["recursive"] = json!(request["recursive"].as_bool().unwrap_or(true));
        } else if scope == zen_workbench::search_scopes::PROJECT_FILES {
            selection["area"] = json!("project");
        } else if let Some(module) = scope.strip_prefix(zen_workbench::search_scopes::MODULE_PREFIX) {
            selection["area"] = json!("module");
            selection["module"] = json!(module);
        } else {
            selection["area"] = json!("scope");
            selection["scope"] = json!(scope);
        }
        registry.dispatch(session, "search.scope.select", selection).map_err(Dispatch)?;
        self.find_in_files.reopen_with(&request);
        Ok(())
    }
}

#[cfg(test)]
mod tests;

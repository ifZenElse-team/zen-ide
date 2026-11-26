use std::collections::HashMap;

use egui::{Align2, Color32, Key, Modifiers, Rect, Sense, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::{BranchInfo, BranchList};

use super::glyphs::{
    Glyph, glyph_branch, glyph_collapse_all, glyph_commit, glyph_expand_all, glyph_folder, glyph_incoming, glyph_label,
    glyph_outgoing, glyph_plus, glyph_search, glyph_settings,
};
use super::vcs::{short_branch, truncated};
use super::{IdePalette, IdeView};
use crate::theme;

pub(super) const WIDTH: f32 = 370.0;
const ROW: f32 = 24.0;
const PAD: f32 = 12.0;
const INDENT: f32 = 19.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Section {
    Recent,
    Local,
    Remote,
    Tags,
}

impl Section {
    const ALL: [Section; 4] = [Section::Recent, Section::Local, Section::Remote, Section::Tags];

    fn id(self) -> &'static str {
        match self {
            Section::Recent => "recent",
            Section::Local => "local",
            Section::Remote => "remote",
            Section::Tags => "tags",
        }
    }

    fn title(self) -> String {
        match self {
            Section::Recent => t!("vcs-branches-recent"),
            Section::Local => t!("vcs-branches-local"),
            Section::Remote => t!("vcs-branches-remote"),
            Section::Tags => t!("vcs-branches-tags"),
        }
    }

    fn open_by_default(self) -> bool {
        self == Section::Recent
    }
}

#[derive(Default)]
pub(super) struct BranchesPopup {
    pub(super) open: bool,
    search: String,
    searched: String,
    list: BranchList,
    listed_at: Option<u64>,
    expanded: HashMap<String, bool>,
    all: Option<bool>,
    selected: Option<String>,
    reveal: bool,
    submenu: Option<Submenu>,
    options: bool,
}

struct Submenu {
    key: String,
    keyboard: bool,
    selected: usize,
}

#[derive(Clone, Debug)]
enum Row {
    Action { action: &'static str, title: String, glyph: Option<Glyph> },
    Separator,
    Group { section: Section, expanded: bool },
    Folder { key: String, name: String, depth: usize, expanded: bool },
    Branch { section: Section, depth: usize, name: String, leaf: String, info: Option<BranchInfo> },
    Nothing,
}

impl Row {
    fn key(&self) -> Option<String> {
        match self {
            Row::Action { action, .. } => Some(format!("action:{action}")),
            Row::Group { section, .. } => Some(format!("group:{}", section.id())),
            Row::Folder { key, .. } => Some(key.clone()),
            Row::Branch { section, name, .. } => Some(format!("branch:{}:{name}", section.id())),
            Row::Separator | Row::Nothing => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Entry {
    Folder { path: String, name: String, depth: usize, expanded: bool },
    Leaf { index: usize, name: String, depth: usize },
}

pub(super) fn tree(names: &[&str], expanded: &dyn Fn(&str) -> bool) -> Vec<Entry> {
    let items: Vec<(usize, &str)> = names.iter().copied().enumerate().collect();
    let mut entries = Vec::new();
    add_entries(&items, "", 0, expanded, &mut entries);
    entries
}

fn add_entries(
    items: &[(usize, &str)],
    prefix: &str,
    depth: usize,
    expanded: &dyn Fn(&str) -> bool,
    entries: &mut Vec<Entry>,
) {
    let head = |rest: &str| rest.split_once('/').map(|(head, _)| head.to_owned()).filter(|head| !head.is_empty());
    let mut placed = vec![false; items.len()];
    for at in 0..items.len() {
        if placed[at] {
            continue;
        }
        let (index, rest) = items[at];
        let Some(folder) = head(rest) else {
            placed[at] = true;
            entries.push(Entry::Leaf { index, name: rest.to_owned(), depth });
            continue;
        };
        let mut inner = Vec::new();
        for (other, &(index, rest)) in items.iter().enumerate().skip(at) {
            if !placed[other] && head(rest).as_deref() == Some(folder.as_str()) {
                placed[other] = true;
                inner.push((index, &rest[folder.len() + 1..]));
            }
        }
        let path = format!("{prefix}{folder}");
        let open = expanded(&path);
        entries.push(Entry::Folder { path: path.clone(), name: folder, depth, expanded: open });
        if open {
            add_entries(&inner, &format!("{path}/"), depth + 1, expanded, entries);
        }
    }
}

struct SubItem {
    title: String,
    action: SubAction,
    separated: bool,
}

enum SubAction {
    Command(&'static str, Value),
    NewBranchFrom(String),
}

fn sub_items(section: Section, name: &str, info: Option<&BranchInfo>, current: Option<&str>) -> Vec<SubItem> {
    let is_current = info.is_some_and(|info| info.current);
    let shown = short_branch(name);
    let mut items = Vec::new();
    let item = |title: String, action: SubAction| SubItem { title, action, separated: false };
    if section == Section::Tags {
        items.push(item(
            t!("vcs-branch-checkout"),
            SubAction::Command("vcs.revision.checkout", json!({ "revision": name })),
        ));
    } else if !is_current {
        items.push(item(t!("vcs-branch-checkout"), SubAction::Command("vcs.branch.checkout", json!({ "name": name }))));
    }
    items.push(item(t!("vcs-branch-new-from", branch = shown.as_str()), SubAction::NewBranchFrom(name.to_owned())));
    if !is_current && let Some(current) = current {
        let title = t!("vcs-branch-merge", branch = shown.as_str(), current = short_branch(current).as_str());
        items.push(item(title, SubAction::Command("vcs.branch.merge", json!({ "name": name }))));
    }
    if matches!(section, Section::Recent | Section::Local) && !is_current {
        items.push(SubItem {
            title: t!("vcs-branch-delete"),
            action: SubAction::Command("vcs.branch.delete", json!({ "name": name })),
            separated: true,
        });
    }
    items
}

enum Outcome {
    Action(&'static str),
    Command(&'static str, Value),
    NewBranchFrom(String),
}

impl BranchesPopup {
    pub(super) fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open = true;
            self.search.clear();
            self.searched.clear();
            self.selected = None;
            self.listed_at = None;
        }
    }

    pub(super) fn close(&mut self) {
        self.open = false;
        self.submenu = None;
        self.options = false;
    }

    fn is_expanded(&self, key: &str, default: bool) -> bool {
        self.expanded.get(key).copied().or(self.all).unwrap_or(default)
    }

    fn toggle_expanded(&mut self, key: &str, default: bool) {
        let open = self.is_expanded(key, default);
        self.expanded.insert(key.to_owned(), !open);
    }

    fn rows(&self) -> Vec<Row> {
        let query = self.search.trim().to_lowercase();
        let searching = !query.is_empty();
        let matches = |text: &str| !searching || text.to_lowercase().contains(&query);
        let mut rows = Vec::new();
        let blocks: [&[(&'static str, &str, Option<Glyph>)]; 2] = [
            &[
                ("view.vcs.update", "keymap-view-vcs-update", Some(glyph_incoming)),
                ("view.vcs.commit", "keymap-view-vcs-commit", Some(glyph_commit)),
                ("view.vcs.push", "keymap-view-vcs-push", Some(glyph_outgoing)),
            ],
            &[
                ("view.vcs.newBranch", "keymap-view-vcs-newBranch", Some(glyph_plus)),
                ("view.vcs.checkoutRevision", "keymap-view-vcs-checkoutRevision", None),
            ],
        ];
        for block in blocks {
            let before = rows.len();
            for &(action, title, glyph) in block {
                let title = t!(title);
                if matches(&title) {
                    rows.push(Row::Action { action, title, glyph });
                }
            }
            if rows.len() > before {
                rows.push(Row::Separator);
            }
        }
        let list = &self.list;
        let mut any = false;
        for section in Section::ALL {
            let items: Vec<(String, Option<BranchInfo>)> = match section {
                Section::Recent => list
                    .recent
                    .iter()
                    .map(|name| (name.clone(), list.local.iter().find(|branch| branch.name == *name).cloned()))
                    .collect(),
                Section::Local => list.local.iter().map(|branch| (branch.name.clone(), Some(branch.clone()))).collect(),
                Section::Remote => {
                    list.remote.iter().map(|branch| (branch.name.clone(), Some(branch.clone()))).collect()
                }
                Section::Tags => list.tags.iter().map(|tag| (tag.name.clone(), None)).collect(),
            };
            let items: Vec<_> = items.into_iter().filter(|(name, _)| matches(name)).collect();
            if items.is_empty() {
                continue;
            }
            any = true;
            let expanded = searching || self.is_expanded(&format!("group:{}", section.id()), section.open_by_default());
            rows.push(Row::Group { section, expanded });
            if !expanded {
                continue;
            }
            let names: Vec<&str> = items.iter().map(|(name, _)| name.as_str()).collect();
            let prefix = format!("folder:{}:", section.id());
            let open = |path: &str| searching || self.is_expanded(&format!("{prefix}{path}"), true);
            for entry in tree(&names, &open) {
                rows.push(match entry {
                    Entry::Folder { path, name, depth, expanded } => {
                        Row::Folder { key: format!("{prefix}{path}"), name, depth: depth + 1, expanded }
                    }
                    Entry::Leaf { index, name, depth } => Row::Branch {
                        section,
                        depth: depth + 1,
                        name: items[index].0.clone(),
                        leaf: name,
                        info: items[index].1.clone(),
                    },
                });
            }
        }
        if !any && matches!(rows.last(), Some(Row::Separator)) {
            rows.pop();
        }
        if rows.is_empty() {
            rows.push(Row::Nothing);
        }
        rows
    }
}

impl IdeView {
    pub(super) fn branches_popup(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, anchor: Rect) {
        if !self.vcs.popup.open {
            return;
        }
        let revision = self.vcs.info.revision;
        if self.vcs.popup.listed_at != Some(revision) {
            self.vcs.popup.list = registry
                .dispatch(session, "vcs.branch.list", json!({}))
                .ok()
                .and_then(|list| serde_json::from_value(list).ok())
                .unwrap_or_default();
            self.vcs.popup.listed_at = Some(revision);
        }
        let p = self.palette;
        let outcome = self.vcs.popup.show(ui, &p, anchor);
        let Some(outcome) = outcome else { return };
        self.vcs.popup.close();
        match outcome {
            Outcome::Action(action) => self.run_action(ui, registry, session, action, json!({})),
            Outcome::Command(command, params) => self.dispatch(ui, registry, session, command, params),
            Outcome::NewBranchFrom(start) => self.open_new_branch(Some(start)),
        }
    }
}

impl BranchesPopup {
    fn show(&mut self, ui: &mut Ui, p: &IdePalette, anchor: Rect) -> Option<Outcome> {
        let mut rows = self.rows();
        let keys: Vec<Option<String>> = rows.iter().map(Row::key).collect();
        let selectable: Vec<usize> = (0..rows.len()).filter(|&index| keys[index].is_some()).collect();
        if self.searched != self.search || self.selected.as_ref().is_none_or(|key| !keys.contains(&Some(key.clone()))) {
            let searching = !self.search.trim().is_empty();
            let first_branch = rows.iter().position(|row| matches!(row, Row::Branch { .. }));
            let first =
                if searching { first_branch.or(selectable.first().copied()) } else { selectable.first().copied() };
            self.selected = first.and_then(|index| keys[index].clone());
            self.searched = self.search.clone();
            self.submenu = None;
        }
        let selected_index = self.selected.as_ref().and_then(|key| keys.iter().position(|k| k.as_ref() == Some(key)));
        let current = self.list.current.clone();
        let sub_of = |row: &Row| match row {
            Row::Branch { section, name, info, .. } => {
                Some(sub_items(*section, name, info.as_ref(), current.as_deref()))
            }
            _ => None,
        };
        let submenu_row =
            self.submenu.as_ref().and_then(|submenu| keys.iter().position(|k| k.as_ref() == Some(&submenu.key)));
        let submenu_items = submenu_row.and_then(|index| sub_of(&rows[index])).unwrap_or_default();
        if submenu_row.is_none() {
            self.submenu = None;
        }

        let mut outcome = None;
        let searching = !self.search.is_empty();
        let (up, down, enter, escape, left, right) = ui.input_mut(|input| {
            (
                input.consume_key(Modifiers::NONE, Key::ArrowUp),
                input.consume_key(Modifiers::NONE, Key::ArrowDown),
                input.consume_key(Modifiers::NONE, Key::Enter),
                input.consume_key(Modifiers::NONE, Key::Escape),
                !searching && input.consume_key(Modifiers::NONE, Key::ArrowLeft),
                !searching && input.consume_key(Modifiers::NONE, Key::ArrowRight),
            )
        });
        if escape {
            if self.options {
                self.options = false;
            } else if self.submenu.is_some() {
                self.submenu = None;
            } else {
                self.close();
                return None;
            }
        }
        let in_submenu = self.submenu.as_ref().is_some_and(|submenu| submenu.keyboard);
        if in_submenu {
            if let Some(submenu) = &mut self.submenu {
                if down && submenu.selected + 1 < submenu_items.len() {
                    submenu.selected += 1;
                }
                if up {
                    submenu.selected = submenu.selected.saturating_sub(1);
                }
                if enter && let Some(item) = submenu_items.get(submenu.selected) {
                    outcome = Some(sub_outcome(&item.action));
                }
            }
            if left {
                self.submenu = None;
            }
        } else {
            let position = selected_index.and_then(|index| selectable.iter().position(|&s| s == index));
            if down || up {
                let next = match (position, down) {
                    (None, _) => 0,
                    (Some(at), true) => (at + 1).min(selectable.len().saturating_sub(1)),
                    (Some(at), false) => at.saturating_sub(1),
                };
                if let Some(&index) = selectable.get(next) {
                    self.selected = keys[index].clone();
                    self.reveal = true;
                    self.submenu = None;
                }
            }
            if let Some(index) = selected_index {
                match &rows[index] {
                    Row::Action { action, .. } if enter => outcome = Some(Outcome::Action(action)),
                    Row::Group { section, expanded } if enter || (left && *expanded) || (right && !*expanded) => {
                        self.toggle_expanded(&format!("group:{}", section.id()), section.open_by_default());
                    }
                    Row::Folder { key, expanded, .. } if enter || (left && *expanded) || (right && !*expanded) => {
                        let key = key.clone();
                        self.toggle_expanded(&key, true);
                    }
                    Row::Branch { .. } if enter || right => {
                        self.submenu =
                            Some(Submenu { key: keys[index].clone().unwrap_or_default(), keyboard: true, selected: 0 });
                    }
                    _ => {}
                }
            }
        }
        if outcome.is_some() {
            return outcome;
        }
        if up || down || enter || left || right {
            rows = self.rows();
        }

        let screen = ui.ctx().content_rect();
        let origin = pos2(anchor.left() - 5.0, anchor.bottom() + 5.0);
        let max_list = (screen.bottom() - origin.y - 80.0).clamp(120.0, 620.0);
        let mut row_rects: HashMap<String, Rect> = HashMap::new();
        let mut toggles: Vec<(String, bool)> = Vec::new();
        let mut open_submenu: Option<(String, bool)> = None;
        let mut collapse_all = None;
        let mut options_button = Rect::NOTHING;
        let moving = ui.input(|input| input.pointer.is_moving());
        let area = egui::Area::new(ui.scope_id().with("vcs-branches-popup"))
            .order(egui::Order::Foreground)
            .fixed_pos(origin)
            .show(ui.ctx(), |ui| {
                super::dialog_frame(p)
                    .corner_radius(10)
                    .inner_margin(egui::Margin { left: PAD as i8, right: PAD as i8, top: 14, bottom: 10 })
                    .show(ui, |ui| {
                        ui.set_width(WIDTH - 2.0 * PAD);
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), 29.0), Sense::hover());
                        let field = Rect::from_min_max(
                            header.min + vec2(1.0, 0.0),
                            pos2(header.right() - 57.0, header.bottom()),
                        );
                        ui.painter().rect_stroke(
                            field,
                            5,
                            egui::Stroke::new(1.0, p.text_field_border),
                            egui::StrokeKind::Inside,
                        );
                        glyph_search(
                            ui.painter(),
                            Rect::from_center_size(pos2(field.left() + 15.5, field.center().y), Vec2::splat(14.0)),
                            p.text_secondary,
                        );
                        let text = Rect::from_min_max(
                            pos2(field.left() + 31.0, field.top()),
                            pos2(field.right() - 6.0, field.bottom()),
                        );
                        let search = ui.put(
                            text,
                            egui::TextEdit::singleline(&mut self.search)
                                .id(ui.scope_id().with("vcs-branches-search"))
                                .frame(egui::Frame::NONE)
                                .align(Align2::LEFT_CENTER)
                                .font(theme::regular(13.0))
                                .hint_text(t!("vcs-branches-search"))
                                .desired_width(text.width()),
                        );
                        crate::accessibility::edit(&search, |node| node.set_label(t!("vcs-branches-search")));
                        if !search.has_focus() {
                            search.request_focus();
                        }
                        let any_open = rows.iter().any(|row| matches!(row, Row::Group { expanded: true, .. }));
                        let buttons: [(f32, Glyph, String); 2] = [
                            (
                                header.right() - 35.5,
                                if any_open { glyph_collapse_all } else { glyph_expand_all },
                                if any_open { t!("vcs-branches-collapse") } else { t!("vcs-branches-expand") },
                            ),
                            (header.right() - 9.0, glyph_settings, t!("vcs-branches-options")),
                        ];
                        for (index, (x, glyph, hint)) in buttons.into_iter().enumerate() {
                            let button = Rect::from_center_size(pos2(x, header.center().y), Vec2::splat(24.0));
                            let response =
                                ui.interact(button, ui.scope_id().with(("vcs-branches-button", index)), Sense::click());
                            crate::accessibility::button(&response, &hint);
                            if response.hovered() || (index == 1 && self.options) {
                                ui.painter().rect_filled(button, 5, p.popup_hover);
                            }
                            glyph(
                                ui.painter(),
                                Rect::from_center_size(button.center(), Vec2::splat(15.0)),
                                p.text_secondary,
                            );
                            if !self.options {
                                response.clone().on_hover_text(hint);
                            }
                            if response.clicked() {
                                if index == 0 {
                                    collapse_all = Some(!any_open);
                                } else {
                                    self.options = !self.options;
                                }
                            }
                            if index == 1 {
                                options_button = button;
                            }
                        }
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical()
                            .id_salt("vcs-branches-list")
                            .max_height(max_list)
                            .min_scrolled_height((rows.len() as f32 * ROW).min(max_list))
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for row in &rows {
                                    let key = row.key();
                                    let (rect, response) = ui.allocate_exact_size(
                                        vec2(ui.available_width(), ROW),
                                        if key.is_some() { Sense::click() } else { Sense::hover() },
                                    );
                                    let selected = key.is_some() && key == self.selected;
                                    crate::accessibility::name_with(&response, egui::Role::MenuItem, || match row {
                                        Row::Action { title, .. } => title.clone(),
                                        Row::Group { section, .. } => section.title(),
                                        Row::Folder { name, .. } | Row::Branch { name, .. } => name.clone(),
                                        Row::Separator | Row::Nothing => String::new(),
                                    });
                                    if let Some(key) = &key {
                                        row_rects.insert(key.clone(), rect);
                                        if response.hovered() && moving {
                                            self.selected = Some(key.clone());
                                            let branch = matches!(row, Row::Branch { .. });
                                            open_submenu =
                                                Some((if branch { key.clone() } else { String::new() }, false));
                                        }
                                    }
                                    if selected {
                                        ui.painter().rect_filled(rect, 5, p.selection);
                                        if self.reveal {
                                            ui.scroll_to_rect(rect, None);
                                        }
                                    }
                                    draw_row(ui, p, row, rect, selected);
                                    if response.clicked() {
                                        match row {
                                            Row::Action { action, .. } => outcome = Some(Outcome::Action(action)),
                                            Row::Group { section, .. } => {
                                                toggles.push((
                                                    format!("group:{}", section.id()),
                                                    section.open_by_default(),
                                                ));
                                            }
                                            Row::Folder { key, .. } => toggles.push((key.clone(), true)),
                                            Row::Branch { .. } => {
                                                open_submenu = key.clone().map(|key| (key, false));
                                            }
                                            Row::Separator | Row::Nothing => {}
                                        }
                                    }
                                }
                            });
                    });
            });
        self.reveal = false;
        let popup_rect = area.response.rect;
        let popup_layer = area.response.layer_id;
        for (key, default) in toggles {
            self.toggle_expanded(&key, default);
        }
        if let Some(open) = collapse_all {
            self.expanded.clear();
            self.all = Some(open);
        }
        match open_submenu {
            Some((key, _)) if key.is_empty() => self.submenu = None,
            Some((key, keyboard)) if self.submenu.as_ref().is_none_or(|submenu| submenu.key != key) => {
                self.submenu = Some(Submenu { key, keyboard, selected: 0 });
            }
            _ => {}
        }
        let mut rects = vec![popup_rect];
        let mut options_rect = None;

        if let Some(submenu) = &mut self.submenu
            && let Some(row) = row_rects.get(&submenu.key)
        {
            let items = rows
                .iter()
                .find(|candidate| candidate.key().as_ref() == Some(&submenu.key))
                .and_then(sub_of)
                .unwrap_or_default();
            let width = items
                .iter()
                .map(|item| ui.painter().layout_no_wrap(item.title.clone(), theme::regular(13.0), p.text).size().x)
                .fold(140.0_f32, f32::max)
                + 40.0;
            let area = egui::Area::new(ui.scope_id().with("vcs-branches-submenu"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(popup_rect.right() - 4.0, row.top() - 7.0))
                .show(ui.ctx(), |ui| {
                    super::dialog_frame(p).corner_radius(8).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing = Vec2::ZERO;
                        for (index, item) in items.iter().enumerate() {
                            if item.separated {
                                let (line, _) = ui.allocate_exact_size(vec2(width, 9.0), Sense::hover());
                                ui.painter().hline(
                                    line.x_range(),
                                    line.center().y,
                                    egui::Stroke::new(1.0, p.popup_border),
                                );
                            }
                            let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                            crate::accessibility::name(&response, egui::Role::MenuItem, &item.title);
                            if response.hovered() && moving {
                                submenu.keyboard = true;
                                submenu.selected = index;
                            }
                            if submenu.keyboard && submenu.selected == index {
                                ui.painter().rect_filled(rect, 5, p.selection);
                            }
                            ui.painter().text(
                                pos2(rect.left() + 10.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                &item.title,
                                theme::regular(13.0),
                                p.text,
                            );
                            if response.clicked() {
                                outcome = Some(sub_outcome(&item.action));
                            }
                        }
                    });
                });
            ui.ctx().set_sublayer(popup_layer, area.response.layer_id);
            rects.push(area.response.rect);
        }

        if self.options {
            let area = egui::Area::new(ui.scope_id().with("vcs-branches-options"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos2(options_button.left(), options_button.bottom() + 4.0))
                .show(ui.ctx(), |ui| {
                    super::dialog_frame(p).corner_radius(8).inner_margin(egui::Margin::same(6)).show(ui, |ui| {
                        ui.set_width(160.0);
                        let (rect, response) = ui.allocate_exact_size(vec2(160.0, ROW), Sense::click());
                        crate::accessibility::name_with(&response, egui::Role::MenuItem, || {
                            t!("keymap-vcs-remote-fetch")
                        });
                        if response.hovered() {
                            ui.painter().rect_filled(rect, 5, p.selection);
                        }
                        ui.painter().text(
                            pos2(rect.left() + 10.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            t!("keymap-vcs-remote-fetch"),
                            theme::regular(13.0),
                            p.text,
                        );
                        if response.clicked() {
                            outcome = Some(Outcome::Command("vcs.remote.fetch", json!({})));
                        }
                    });
                });
            ui.ctx().set_sublayer(popup_layer, area.response.layer_id);
            rects.push(area.response.rect);
            options_rect = Some(area.response.rect);
        }
        if outcome.is_some() {
            return outcome;
        }

        if ui.input(|input| input.pointer.any_pressed())
            && let Some(pointer) = ui.input(|input| input.pointer.interact_pos())
            && !anchor.contains(pointer)
        {
            if !rects.iter().any(|rect| rect.contains(pointer)) {
                self.close();
            } else if options_rect.is_some_and(|rect| !rect.contains(pointer)) && !options_button.contains(pointer) {
                self.options = false;
            }
        }
        None
    }
}

fn sub_outcome(action: &SubAction) -> Outcome {
    match action {
        SubAction::Command(command, params) => Outcome::Command(command, params.clone()),
        SubAction::NewBranchFrom(name) => Outcome::NewBranchFrom(name.clone()),
    }
}

fn icon_x(depth: usize) -> f32 {
    36.5 + INDENT * depth as f32
}

fn draw_row(ui: &Ui, p: &IdePalette, row: &Row, rect: Rect, selected: bool) {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let y = rect.center().y;
    let text = |x: f32, label: &str, width: f32| {
        let galley = painter.layout_job(truncated(label, theme::regular(13.0), p.text, width.max(20.0)));
        let right = x + galley.size().x;
        painter.galley(pos2(x, y - galley.size().y / 2.0), galley, p.text);
        right
    };
    match row {
        Row::Action { action, title, glyph } => {
            if let Some(glyph) = glyph {
                glyph(&painter, Rect::from_center_size(pos2(rect.left() + 16.5, y), Vec2::splat(14.0)), p.text);
            }
            let mut right = rect.right() - 7.5;
            if let Some(shortcut) = crate::keymap::shortcut(action) {
                let shown = painter.text(
                    pos2(right, y),
                    Align2::RIGHT_CENTER,
                    shortcut,
                    theme::regular(13.0),
                    p.text_secondary,
                );
                right = shown.left() - 12.0;
            }
            text(rect.left() + 31.5, title, right - rect.left() - 31.5);
        }
        Row::Separator => {
            painter.hline((rect.left() - 4.0)..=(rect.right() + 4.0), y, egui::Stroke::new(1.0, p.popup_border));
        }
        Row::Nothing => {
            painter.text(
                pos2(rect.left() + 31.5, y),
                Align2::LEFT_CENTER,
                t!("vcs-branches-nothing"),
                theme::regular(13.0),
                p.text_secondary,
            );
        }
        Row::Group { section, expanded } => {
            chevron(&painter, pos2(rect.left() + 17.5, y), *expanded, p.text);
            text(rect.left() + 31.5, &section.title(), rect.width() - 40.0);
        }
        Row::Folder { name, depth, expanded, .. } => {
            let icon = rect.left() + icon_x(*depth);
            chevron(&painter, pos2(icon - INDENT, y), *expanded, p.text);
            glyph_folder(&painter, Rect::from_center_size(pos2(icon, y), Vec2::splat(14.0)), p.text);
            text(icon + 15.0, name, rect.right() - icon - 30.0);
        }
        Row::Branch { section, depth, leaf, info, .. } => {
            let icon = Rect::from_center_size(pos2(rect.left() + icon_x(*depth), y), Vec2::splat(14.0));
            let current = info.as_ref().is_some_and(|info| info.current);
            if current {
                glyph_label(&painter, icon, p.vcs_current_branch, if selected { p.selection } else { p.popup });
            } else if *section == Section::Tags {
                glyph_label(&painter, icon, p.text_secondary, if selected { p.selection } else { p.popup });
            } else {
                glyph_branch(&painter, icon, p.text);
            }
            crate::icons::chevron_right(
                &painter,
                Rect::from_center_size(pos2(rect.right() - 8.0, y), Vec2::splat(12.0)),
                p.text_secondary,
            );
            let right = rect.right() - 20.5;
            let x = icon.right() + 8.0;
            let counts: Vec<(Glyph, usize, Color32)> = info
                .as_ref()
                .map(|info| {
                    [
                        (glyph_incoming as Glyph, info.behind, p.vcs_incoming),
                        (glyph_outgoing as Glyph, info.ahead, p.vcs_outgoing),
                    ]
                    .into_iter()
                    .filter(|(_, count, _)| *count > 0)
                    .collect()
                })
                .unwrap_or_default();
            let count_width: f32 = counts
                .iter()
                .map(|(_, count, _)| {
                    16.0 + ui.painter().layout_no_wrap(count.to_string(), theme::regular(12.0), p.text).size().x
                })
                .sum();
            let name_width = ui.painter().layout_no_wrap(leaf.clone(), theme::regular(13.0), p.text).size().x;
            let left_for_upstream = right - x - name_width - count_width - 16.0;
            let mut name_room = right - x - count_width;
            if let Some(upstream) = info.as_ref().and_then(|info| info.upstream.as_deref())
                && left_for_upstream >= 48.0
            {
                let galley =
                    painter.layout_job(truncated(upstream, theme::regular(13.0), p.text_secondary, left_for_upstream));
                let start = right - galley.size().x;
                painter.galley(pos2(start, y - galley.size().y / 2.0), galley, p.text_secondary);
                name_room = start - 16.0 - x - count_width;
            }
            let mut after = text(x, leaf, name_room);
            for (glyph, count, color) in counts {
                after += 6.0;
                glyph(&painter, Rect::from_center_size(pos2(after + 4.0, y), Vec2::splat(8.0)), color);
                let shown = painter.text(
                    pos2(after + 10.0, y),
                    Align2::LEFT_CENTER,
                    count.to_string(),
                    theme::regular(12.0),
                    color,
                );
                after = shown.right();
            }
        }
    }
}

fn chevron(painter: &egui::Painter, center: egui::Pos2, open: bool, color: Color32) {
    let rect = Rect::from_center_size(center, Vec2::splat(12.0));
    if open {
        crate::icons::chevron_down(painter, rect, color);
    } else {
        crate::icons::chevron_right(painter, rect, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(entries: &[Entry], names: &[&str]) -> Vec<String> {
        entries
            .iter()
            .map(|entry| match entry {
                Entry::Folder { path, depth, expanded, .. } => {
                    format!("{}{path}/{}", "  ".repeat(*depth), if *expanded { "" } else { " +" })
                }
                Entry::Leaf { index, name, depth } => format!("{}{name} ({})", "  ".repeat(*depth), names[*index]),
            })
            .collect()
    }

    #[test]
    fn names_with_a_slash_are_grouped_into_folders_in_place() {
        let names = ["main", "feature/published-course-short-description", "refactoring", "feature/ui/menu", "fix/a"];
        let entries = tree(&names, &|_| true);
        assert_eq!(
            shown(&entries, &names),
            [
                "main (main)",
                "feature/",
                "  published-course-short-description (feature/published-course-short-description)",
                "  feature/ui/",
                "    menu (feature/ui/menu)",
                "refactoring (refactoring)",
                "fix/",
                "  a (fix/a)",
            ]
        );
    }

    #[test]
    fn a_closed_folder_hides_its_branches() {
        let names = ["origin/main", "origin/feature/x", "upstream/main"];
        let entries = tree(&names, &|path| path != "origin/feature");
        assert_eq!(
            shown(&entries, &names),
            ["origin/", "  main (origin/main)", "  origin/feature/ +", "upstream/", "  main (upstream/main)"]
        );
        let closed = tree(&names, &|_| false);
        assert_eq!(shown(&closed, &names), ["origin/ +", "upstream/ +"]);
    }

    #[test]
    fn the_popup_lists_groups_and_searches_branches() {
        let branch = |name: &str, current: bool| BranchInfo {
            name: name.to_owned(),
            current,
            upstream: Some(format!("origin/{name}")),
            ..BranchInfo::default()
        };
        let mut popup = BranchesPopup {
            list: BranchList {
                current: Some("main".to_owned()),
                recent: vec!["main".to_owned(), "feature/x".to_owned()],
                local: vec![branch("feature/x", false), branch("main", true)],
                remote: vec![BranchInfo { name: "origin/main".to_owned(), ..BranchInfo::default() }],
                tags: vec![zen_workbench::vcs::TagInfo { name: "v1".to_owned(), commit: String::new() }],
            },
            ..BranchesPopup::default()
        };
        let keys = |popup: &BranchesPopup| -> Vec<String> {
            popup.rows().iter().map(|row| row.key().unwrap_or_else(|| "-".to_owned())).collect()
        };
        assert_eq!(
            keys(&popup),
            [
                "action:view.vcs.update",
                "action:view.vcs.commit",
                "action:view.vcs.push",
                "-",
                "action:view.vcs.newBranch",
                "action:view.vcs.checkoutRevision",
                "-",
                "group:recent",
                "branch:recent:main",
                "folder:recent:feature",
                "branch:recent:feature/x",
                "group:local",
                "group:remote",
                "group:tags",
            ]
        );
        popup.toggle_expanded("folder:recent:feature", true);
        popup.toggle_expanded("group:tags", false);
        assert_eq!(
            keys(&popup)[9..],
            ["folder:recent:feature", "group:local", "group:remote", "group:tags", "branch:tags:v1"]
        );
        popup.all = Some(false);
        popup.expanded.clear();
        assert_eq!(keys(&popup)[7..], ["group:recent", "group:local", "group:remote", "group:tags"]);

        popup.search = "MAIN".to_owned();
        assert_eq!(
            keys(&popup),
            [
                "group:recent",
                "branch:recent:main",
                "group:local",
                "branch:local:main",
                "group:remote",
                "folder:remote:origin",
                "branch:remote:origin/main"
            ]
        );
        popup.search = "push".to_owned();
        assert_eq!(keys(&popup), ["action:view.vcs.push"]);
        popup.search = "nothing like it".to_owned();
        assert!(matches!(popup.rows().as_slice(), [Row::Nothing]));
    }

    #[test]
    fn a_branch_offers_what_can_be_done_with_it() {
        let current = BranchInfo { name: "main".to_owned(), current: true, ..BranchInfo::default() };
        let other = BranchInfo { name: "feature/x".to_owned(), ..BranchInfo::default() };
        let titles = |items: Vec<SubItem>| -> Vec<String> { items.into_iter().map(|item| item.title).collect() };
        assert_eq!(titles(sub_items(Section::Local, "main", Some(&current), Some("main"))).len(), 1, "New Branch only");
        let local = sub_items(Section::Local, "feature/x", Some(&other), Some("main"));
        assert_eq!(local.len(), 4, "Checkout, New Branch from, Merge, Delete");
        assert!(local[3].separated);
        assert!(matches!(&local[0].action, SubAction::Command("vcs.branch.checkout", _)));
        let tag = sub_items(Section::Tags, "v1", None, Some("main"));
        assert!(
            matches!(&tag[0].action, SubAction::Command("vcs.revision.checkout", params) if params["revision"] == "v1")
        );
        assert_eq!(tag.len(), 3, "a tag is not deleted here");
        assert_eq!(
            sub_items(Section::Remote, "origin/x", Some(&other), None).len(),
            2,
            "detached: nothing to merge into"
        );
    }
}

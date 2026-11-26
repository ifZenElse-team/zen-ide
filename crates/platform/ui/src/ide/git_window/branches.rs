use std::collections::HashSet;

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_workbench::vcs::RepositoryInfo;
use zen_workbench::vcs::branches::BranchInfo;

use super::super::branches_popup::{Entry, tree};
use super::super::glyphs::{
    Glyph, glyph_back, glyph_branch, glyph_collapse_all, glyph_compare, glyph_expand_all, glyph_fetch, glyph_folder,
    glyph_forward, glyph_group, glyph_incoming, glyph_label, glyph_locate, glyph_outgoing, glyph_plus, glyph_search,
    glyph_star, glyph_trash,
};
use super::super::run_window::Tool;
use super::super::{IdePalette, Side, customization};
use super::{GitAction, GitWindow};
use crate::theme;

const TREE_ROW: f32 = 24.0;
const INDENT: f32 = 19.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Section {
    Local,
    Remote,
    Tags,
}

impl Section {
    fn id(self) -> &'static str {
        match self {
            Section::Local => "local",
            Section::Remote => "remote",
            Section::Tags => "tags",
        }
    }

    fn title(self) -> String {
        match self {
            Section::Local => t!("vcs-branches-local"),
            Section::Remote => t!("vcs-branches-remote"),
            Section::Tags => t!("vcs-branches-tags"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BranchKey {
    pub(super) section: Section,
    pub(super) name: String,
}

#[derive(Clone, Debug, PartialEq)]
enum BranchRow {
    Head,
    Group { section: Section, expanded: bool },
    Folder { key: String, name: String, depth: usize, expanded: bool },
    Branch { section: Section, name: String, leaf: String, depth: usize, info: Option<BranchInfo> },
}

type Named = (String, Option<BranchInfo>);

fn rows(
    local: &[BranchInfo],
    remote: &[BranchInfo],
    tags: &[String],
    search: &str,
    flat: bool,
    collapsed: &HashSet<String>,
) -> Vec<BranchRow> {
    let search = search.trim().to_lowercase();
    let mut rows = vec![BranchRow::Head];
    let lists: [(Section, Vec<Named>); 3] = [
        (Section::Local, local.iter().map(|branch| (branch.name.clone(), Some(branch.clone()))).collect()),
        (Section::Remote, remote.iter().map(|branch| (branch.name.clone(), Some(branch.clone()))).collect()),
        (Section::Tags, tags.iter().map(|tag| (tag.clone(), None)).collect()),
    ];
    for (section, list) in lists {
        let list: Vec<Named> =
            list.into_iter().filter(|(name, _)| search.is_empty() || name.to_lowercase().contains(&search)).collect();
        if list.is_empty() {
            continue;
        }
        let key = format!("group:{}", section.id());
        let expanded = !search.is_empty() || !collapsed.contains(&key);
        rows.push(BranchRow::Group { section, expanded });
        if !expanded {
            continue;
        }
        let names: Vec<&str> = list.iter().map(|(name, _)| name.as_str()).collect();
        let entries = if flat || section == Section::Tags {
            names
                .iter()
                .enumerate()
                .map(|(index, name)| Entry::Leaf { index, name: name.to_string(), depth: 0 })
                .collect()
        } else {
            let open = |path: &str| !search.is_empty() || !collapsed.contains(&format!("{}:{path}", section.id()));
            tree(&names, &open)
        };
        for entry in entries {
            rows.push(match entry {
                Entry::Folder { path, name, depth, expanded } => {
                    BranchRow::Folder { key: format!("{}:{path}", section.id()), name, depth, expanded }
                }
                Entry::Leaf { index, name, depth } => BranchRow::Branch {
                    section,
                    name: list[index].0.clone(),
                    leaf: name,
                    depth,
                    info: list[index].1.clone(),
                },
            });
        }
    }
    rows
}

impl GitWindow {
    pub(super) fn branch_folders(&self) -> HashSet<String> {
        let none = HashSet::new();
        let tags: Vec<String> = self.branches.tags.iter().map(|tag| tag.name.clone()).collect();
        let mut keys: HashSet<String> =
            ["local", "remote", "tags"].iter().map(|section| format!("group:{section}")).collect();
        for row in rows(&self.branches.local, &self.branches.remote, &tags, "", self.flat_branches, &none) {
            if let BranchRow::Folder { key, .. } = row {
                keys.insert(key);
            }
        }
        keys
    }

    fn selected_info(&self) -> Option<(Section, BranchInfo)> {
        let key = self.selected_branch.as_ref()?;
        let list = match key.section {
            Section::Local => &self.branches.local,
            Section::Remote => &self.branches.remote,
            Section::Tags => return None,
        };
        list.iter().find(|branch| branch.name == key.name).map(|branch| (key.section, branch.clone()))
    }

    pub(super) fn branch_toolbar(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        info: &RepositoryInfo,
        actions: &mut Vec<GitAction>,
    ) {
        let full = ui.max_rect();
        let selected = self.selected_info();
        let name = self.selected_branch.as_ref().map(|key| key.name.clone());
        let local = selected.as_ref().filter(|(section, _)| *section == Section::Local).map(|(_, info)| info.clone());
        let current = info.branch.clone();
        let favorite = name.as_ref().is_some_and(|name| self.view.favorites.contains(name));
        let hidden = self.branches_hidden;
        let y = std::cell::Cell::new(full.top() + 8.0);
        let button = |ui: &Ui,
                      salt: &'static str,
                      glyph: Glyph,
                      name: String,
                      enabled: bool,
                      on: Option<bool>|
         -> bool {
            let rect = Rect::from_min_size(pos2(full.center().x - 13.0, y.get()), Vec2::splat(26.0));
            y.set(y.get() + 30.0);
            Tool { on, ..Tool::new(salt, glyph, name, p.text_secondary) }.show_hinted(ui, p, rect, enabled, Side::Right)
        };
        let separator = |ui: &Ui| {
            ui.painter().hline((full.left() + 9.0)..=(full.right() - 9.0), y.get() + 1.0, Stroke::new(1.0, p.frame));
            y.set(y.get() + 6.0);
        };
        let (show_name, show_glyph): (String, Glyph) = if hidden {
            (t!("vcs-log-show-branches"), glyph_forward)
        } else {
            (t!("vcs-log-hide-branches"), glyph_back)
        };
        if button(ui, "git-branches-hide", show_glyph, show_name, true, None) {
            actions.push(GitAction::View("view.vcs.logBranches"));
        }
        if hidden {
            return;
        }
        separator(ui);
        let is_current = local.as_ref().is_some_and(|branch| Some(&branch.name) == current.as_ref());
        let tools: [(&'static str, Glyph, String, bool, Option<bool>); 7] = [
            ("git-branch-new", glyph_plus, t!("keymap-view-vcs-newBranch"), true, None),
            (
                "git-branch-update",
                glyph_incoming,
                t!("vcs-log-update-selected"),
                local.as_ref().is_some_and(|b| b.upstream.is_some()),
                None,
            ),
            ("git-branch-delete", glyph_trash, t!("vcs-log-delete-branch"), local.is_some() && !is_current, None),
            ("git-branch-compare", glyph_compare, t!("vcs-log-compare"), name.is_some() && !is_current, None),
            ("git-branch-navigate", glyph_locate, t!("vcs-log-navigate"), name.is_some(), None),
            ("git-branch-fetch", glyph_fetch, t!("keymap-vcs-remote-fetch"), true, None),
            (
                "git-branch-favorite",
                glyph_star,
                if favorite { t!("vcs-log-unfavorite") } else { t!("vcs-log-favorite") },
                selected.is_some(),
                None,
            ),
        ];
        for (salt, glyph, title, enabled, on) in tools {
            if !button(ui, salt, glyph, title, enabled, on) {
                continue;
            }
            let name = name.clone().unwrap_or_default();
            match salt {
                "git-branch-new" => {
                    actions.push(GitAction::NewBranch(self.selected_branch.as_ref().map(|key| key.name.clone())))
                }
                "git-branch-update" => actions.push(GitAction::Run("vcs.branch.update", json!({ "name": name }))),
                "git-branch-delete" => actions.push(GitAction::Run("vcs.branch.delete", json!({ "name": name }))),
                "git-branch-compare" => {
                    let title = t!(
                        "vcs-log-compare-tab",
                        branch = name.as_str(),
                        current = current.as_deref().unwrap_or("HEAD")
                    );
                    let params = json!({ "title": title, "branches": [name], "exclude": ["HEAD"] });
                    actions.push(GitAction::Run("vcs.logTab.new", params));
                }
                "git-branch-navigate" => actions.push(GitAction::Run("vcs.log.select", json!({ "id": name }))),
                "git-branch-fetch" => actions.push(GitAction::Run("vcs.remote.fetch", json!({}))),
                _ => actions.push(GitAction::Run("vcs.branch.favorite", json!({ "name": name }))),
            }
        }
        separator(ui);
        if button(ui, "git-branch-group", glyph_group, t!("vcs-log-group-branches"), !hidden, Some(!self.flat_branches))
        {
            actions.push(GitAction::View("view.vcs.logGroupBranches"));
        }
        if button(ui, "git-branch-expand", glyph_expand_all, t!("vcs-branches-expand"), !hidden, None) {
            actions.push(GitAction::View("view.vcs.logExpandBranches"));
        }
        if button(ui, "git-branch-collapse", glyph_collapse_all, t!("vcs-branches-collapse"), !hidden, None) {
            actions.push(GitAction::View("view.vcs.logCollapseBranches"));
        }
    }

    pub(super) fn branch_panel(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<GitAction>) {
        let full = ui.max_rect();
        let field = Rect::from_min_size(full.min + vec2(6.0, 5.0), vec2(full.width() - 12.0, 30.0));
        ui.painter().rect_stroke(field, 5, Stroke::new(1.0, p.text_field_border), egui::StrokeKind::Inside);
        glyph_search(
            ui.painter(),
            Rect::from_center_size(pos2(field.left() + 15.0, field.center().y), Vec2::splat(14.0)),
            p.text_secondary,
        );
        let text =
            Rect::from_min_max(pos2(field.left() + 30.0, field.top()), pos2(field.right() - 6.0, field.bottom()));
        ui.put(
            text,
            egui::TextEdit::singleline(&mut self.branch_search)
                .id(ui.scope_id().with("git-branch-search"))
                .frame(egui::Frame::NONE)
                .align(Align2::LEFT_CENTER)
                .font(theme::regular(13.0))
                .hint_text(t!("vcs-log-branch-search"))
                .desired_width(text.width()),
        );
        let list = Rect::from_min_max(pos2(full.left(), field.bottom() + 6.0), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(list).id_salt("git-branch-tree"));
        let tags: Vec<String> = self.branches.tags.iter().map(|tag| tag.name.clone()).collect();
        let shown = rows(
            &self.branches.local,
            &self.branches.remote,
            &tags,
            &self.branch_search,
            self.flat_branches,
            &self.collapsed_branches,
        );
        let filtered: Vec<String> = self.tab().filters.branches;
        let current = self.branches.current.clone();
        let favorites = self.view.favorites.clone();
        let mut toggle: Option<String> = None;
        let mut select: Option<Option<BranchKey>> = None;
        egui::ScrollArea::vertical().id_salt("git-branch-list").auto_shrink([false, false]).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &shown {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), TREE_ROW), Sense::click());
                let key = match row {
                    BranchRow::Branch { section, name, .. } => {
                        Some(BranchKey { section: *section, name: name.clone() })
                    }
                    _ => None,
                };
                let selected = match row {
                    BranchRow::Head => filtered == ["HEAD"] && self.selected_branch.is_none(),
                    BranchRow::Branch { .. } => key.is_some() && key == self.selected_branch,
                    _ => false,
                };
                if selected {
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 5, p.selection);
                } else if response.hovered() {
                    ui.painter().rect_filled(rect.shrink2(vec2(4.0, 0.0)), 5, p.hover);
                }
                let background = if selected { p.selection } else { p.island };
                draw_row(ui, p, row, rect, current.as_deref(), &favorites, background);
                match row {
                    BranchRow::Head if response.clicked() => {
                        select = Some(None);
                        actions.push(GitAction::Run("vcs.log.filter", json!({ "branches": ["HEAD"] })));
                    }
                    BranchRow::Group { section, .. } if response.clicked() => {
                        toggle = Some(format!("group:{}", section.id()))
                    }
                    BranchRow::Folder { key, .. } if response.clicked() => toggle = Some(key.clone()),
                    BranchRow::Branch { section, name, info, .. } => {
                        if response.clicked() {
                            select = Some(key.clone());
                            actions.push(GitAction::Run("vcs.log.filter", json!({ "branches": [name] })));
                        }
                        let (section, name, info) = (*section, name.clone(), info.clone());
                        let is_favorite = favorites.contains(&name);
                        response.context_menu(|ui| {
                            branch_menu(ui, section, &name, info.as_ref(), current.as_deref(), is_favorite, actions);
                        });
                    }
                    _ => {}
                }
            }
        });
        if let Some(key) = toggle
            && !self.collapsed_branches.remove(&key)
        {
            self.collapsed_branches.insert(key);
        }
        if let Some(selection) = select {
            self.selected_branch = selection;
        }
    }
}

fn branch_menu(
    ui: &mut Ui,
    section: Section,
    name: &str,
    info: Option<&BranchInfo>,
    current: Option<&str>,
    favorite: bool,
    actions: &mut Vec<GitAction>,
) {
    ui.set_min_width(240.0);
    let is_current = Some(name) == current && section == Section::Local;
    let mut item = |ui: &mut Ui, title: String, enabled: bool, action: GitAction| {
        if ui.add_enabled(enabled, egui::Button::new(title)).clicked() {
            actions.push(action);
            ui.close();
        }
    };
    let shown = |slot: &str| match slot {
        customization::BRANCH_MERGE => current.is_some(),
        "vcs-log-update-selected" | "vcs-branch-delete" => section == Section::Local,
        _ => true,
    };
    customization::popup(ui, customization::GIT_BRANCH_POPUP, shown, |ui, slot| match slot {
        "vcs-branch-checkout" => {
            let checkout = if section == Section::Tags {
                GitAction::Run("vcs.revision.checkout", json!({ "revision": name }))
            } else {
                GitAction::Run("vcs.branch.checkout", json!({ "name": name }))
            };
            item(ui, t!("vcs-branch-checkout"), !is_current, checkout);
        }
        customization::BRANCH_NEW => {
            item(ui, t!("vcs-branch-new-from", branch = name), true, GitAction::NewBranch(Some(name.to_owned())));
        }
        customization::BRANCH_MERGE => {
            let Some(current) = current else { return };
            item(
                ui,
                t!("vcs-branch-merge", branch = name, current = current),
                !is_current,
                GitAction::Run("vcs.branch.merge", json!({ "name": name })),
            );
        }
        "vcs-log-update-selected" => {
            let tracked = info.is_some_and(|info| info.upstream.is_some());
            let update = GitAction::Run("vcs.branch.update", json!({ "name": name }));
            item(ui, t!("vcs-log-update-selected"), tracked, update);
        }
        "vcs-log-compare" => {
            let title = t!("vcs-log-compare-tab", branch = name, current = current.unwrap_or("HEAD"));
            item(
                ui,
                t!("vcs-log-compare"),
                !is_current,
                GitAction::Run("vcs.logTab.new", json!({ "title": title, "branches": [name], "exclude": ["HEAD"] })),
            );
        }
        "vcs-log-favorite" => {
            let star = if favorite { t!("vcs-log-unfavorite") } else { t!("vcs-log-favorite") };
            item(ui, star, section != Section::Tags, GitAction::Run("vcs.branch.favorite", json!({ "name": name })));
        }
        "vcs-branch-delete" => {
            let delete = GitAction::Run("vcs.branch.delete", json!({ "name": name }));
            item(ui, t!("vcs-branch-delete"), !is_current, delete);
        }
        _ => {}
    });
}

fn draw_row(
    ui: &Ui,
    p: &IdePalette,
    row: &BranchRow,
    rect: Rect,
    current: Option<&str>,
    favorites: &[String],
    background: Color32,
) {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let y = rect.center().y;
    let text = |x: f32, label: &str| {
        let galley = painter.layout_job(super::super::vcs::truncated(
            label,
            theme::regular(13.0),
            p.text,
            (rect.right() - x - 8.0).max(20.0),
        ));
        let right = x + galley.size().x;
        painter.galley(pos2(x, y - galley.size().y / 2.0), galley, p.text);
        right
    };
    match row {
        BranchRow::Head => {
            text(rect.left() + 30.0, &format!("HEAD ({})", t!("vcs-log-current-branch")));
        }
        BranchRow::Group { section, expanded } => {
            chevron(&painter, pos2(rect.left() + 16.0, y), *expanded, p.text_secondary);
            text(rect.left() + 30.0, &section.title());
        }
        BranchRow::Folder { name, depth, expanded, .. } => {
            let icon = rect.left() + 50.0 + INDENT * *depth as f32;
            chevron(&painter, pos2(icon - 16.0, y), *expanded, p.text_secondary);
            glyph_folder(&painter, Rect::from_center_size(pos2(icon, y), Vec2::splat(14.0)), p.text_secondary);
            text(icon + 12.0, name);
        }
        BranchRow::Branch { section, name, leaf, depth, info } => {
            let center = pos2(rect.left() + 50.0 + INDENT * *depth as f32, y);
            let icon = Rect::from_center_size(center, Vec2::splat(14.0));
            let is_current = *section == Section::Local && Some(name.as_str()) == current;
            if is_current {
                glyph_label(&painter, icon, p.vcs_current_branch, background);
            } else if *section == Section::Tags {
                glyph_label(&painter, icon, p.text_secondary, background);
            } else if favorites.iter().any(|favorite| favorite == name) {
                glyph_star(&painter, icon.shrink(0.5), p.vcs_current_branch);
            } else {
                glyph_branch(&painter, icon, p.text_secondary);
            }
            let mut after = text(icon.right() + 6.0, leaf);
            let counts: Vec<(Glyph, usize, Color32)> = info
                .iter()
                .flat_map(|info| {
                    [
                        (glyph_incoming as Glyph, info.behind, p.vcs_incoming),
                        (glyph_outgoing as Glyph, info.ahead, p.vcs_outgoing),
                    ]
                })
                .filter(|(_, count, _)| *count > 0)
                .collect();
            for (glyph, count, color) in counts {
                after += 8.0;
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

    fn branch(name: &str) -> BranchInfo {
        BranchInfo { name: name.to_owned(), ..BranchInfo::default() }
    }

    fn shown(rows: &[BranchRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match row {
                BranchRow::Head => "HEAD".to_owned(),
                BranchRow::Group { section, expanded } => {
                    format!("[{}{}]", section.id(), if *expanded { "" } else { "+" })
                }
                BranchRow::Folder { name, depth, .. } => format!("{depth}:{name}/"),
                BranchRow::Branch { leaf, depth, .. } => format!("{depth}:{leaf}"),
            })
            .collect()
    }

    #[test]
    fn the_branch_tree_has_groups_and_folders_for_slashes() {
        let local = [branch("feature/a"), branch("main"), branch("feature/b"), branch("refactoring")];
        let remote = [branch("origin/main"), branch("origin/refactoring")];
        let tags = ["v0.0.1".to_owned(), "v0.0.2".to_owned()];
        let none = HashSet::new();
        assert_eq!(
            shown(&rows(&local, &remote, &tags, "", false, &none)),
            [
                "HEAD",
                "[local]",
                "0:feature/",
                "1:a",
                "1:b",
                "0:main",
                "0:refactoring",
                "[remote]",
                "0:origin/",
                "1:main",
                "1:refactoring",
                "[tags]",
                "0:v0.0.1",
                "0:v0.0.2",
            ]
        );
        let collapsed: HashSet<String> = ["local:feature".to_owned(), "group:tags".to_owned()].into();
        assert_eq!(
            shown(&rows(&local, &remote, &tags, "", false, &collapsed)),
            [
                "HEAD",
                "[local]",
                "0:feature/",
                "0:main",
                "0:refactoring",
                "[remote]",
                "0:origin/",
                "1:main",
                "1:refactoring",
                "[tags+]"
            ]
        );
        assert_eq!(
            shown(&rows(&local, &remote, &tags, "MAIN", false, &collapsed)),
            ["HEAD", "[local]", "0:main", "[remote]", "0:origin/", "1:main"]
        );
        assert_eq!(
            shown(&rows(&local, &[], &[], "feature", true, &none)),
            ["HEAD", "[local]", "0:feature/a", "0:feature/b"]
        );
    }
}

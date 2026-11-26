use std::path::PathBuf;

use egui::{Align2, Color32, Rect, Sense, Ui, Vec2, pos2, vec2};
use zen_i18n::t;
use zen_workbench::vcs::graph_search::PanelFilter;
use zen_workbench::vcs::workspace::GraphCommit;
use zen_workbench::vcs::worktree::{SubmoduleAction, SubmoduleInfo, SubmoduleStatus, WorktreeAction, WorktreeInfo};

use super::super::glyphs::{glyph_check, glyph_lock, glyph_submodule, glyph_worktree};
use super::super::{IdePalette, Side, hint};
use super::look::{self, Tint};
use super::{Ask, Columns, GitWorkspaceView, NODE, lane_color};
use crate::theme;

pub(super) const RECOUNT: f64 = 5.0;

pub(super) fn shown_worktrees<'a>(view: &'a GitWorkspaceView, shows: &PanelFilter) -> Vec<&'a WorktreeInfo> {
    view.worktrees
        .iter()
        .filter(|worktree| shows.shows(&worktree.name) || worktree.branch.as_ref().is_some_and(|b| shows.shows(b)))
        .collect()
}

pub(super) fn shown_submodules<'a>(view: &'a GitWorkspaceView, shows: &PanelFilter) -> Vec<&'a SubmoduleInfo> {
    view.submodules.iter().filter(|submodule| shows.shows(&submodule.path) || shows.shows(&submodule.name)).collect()
}

pub(super) fn worktree_row(
    ui: &mut Ui,
    p: &IdePalette,
    view: &mut GitWorkspaceView,
    worktree: &WorktreeInfo,
    selected: Option<&str>,
    asks: &mut Vec<Ask>,
) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let inner = rect.shrink2(vec2(6.0, 0.0));
    let is_selected = view.worktree_menu_for.as_ref() == Some(&worktree.path)
        || (selected.is_some() && selected == worktree.head.as_deref() && worktree.current);
    if is_selected {
        ui.painter().rect_filled(inner, 5, p.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(inner, 5, p.hover);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), Vec2::splat(13.0));
    let icon_color = if worktree.current { p.vcs_current_branch } else { p.text_secondary };
    glyph_worktree(ui.painter(), icon, icon_color);
    let mut right = inner.right() - 6.0;
    if let Some(changes) = worktree.changes.filter(|changes| *changes > 0) {
        let galley = ui.painter().layout_no_wrap(changes.to_string(), theme::regular(11.0), p.text);
        let pill = Rect::from_min_max(
            pos2(right - galley.size().x - 12.0, rect.center().y - 8.0),
            pos2(right, rect.center().y + 8.0),
        );
        ui.painter().rect_filled(pill, 8, p.vcs_modified.gamma_multiply(0.3));
        ui.painter().galley(pill.center() - galley.size() / 2.0, galley, p.text);
        right = pill.left() - 6.0;
    }
    if worktree.locked {
        let lock = Rect::from_center_size(pos2(right - 5.0, rect.center().y), Vec2::splat(11.0));
        glyph_lock(ui.painter(), lock, p.text_secondary);
        right = lock.left() - 6.0;
    }
    let font = if worktree.current { theme::semibold(13.0) } else { theme::regular(13.0) };
    let color = if worktree.prunable { super::search::out_text(p) } else { p.text };
    let room = (right - icon.right() - 8.0).max(8.0);
    let galley = ui.painter().layout_job(super::super::vcs::truncated(worktree.label(), font, color, room));
    ui.painter().galley(pos2(icon.right() + 8.0, rect.center().y - galley.size().y / 2.0), galley, color);
    let detail = worktree_detail(worktree);
    hint(ui, &response, &worktree.path.display().to_string(), Some(&detail), Side::Right);
    if response.double_clicked() && !worktree.current && !worktree.prunable {
        asks.push(Ask::Worktree(worktree.path.clone(), WorktreeAction::Open));
    } else if response.clicked()
        && let Some(head) = &worktree.head
    {
        asks.push(Ask::Select(head.clone()));
    }
    if view.worktree_menu_at.is_none() && view.worktree_menu_for.as_ref() == Some(&worktree.path) {
        view.worktree_menu_at = Some(pos2(rect.left() + 32.0, rect.bottom()));
    }
    if response.secondary_clicked() {
        view.worktree_right_click = Some(response.interact_pointer_pos().unwrap_or(rect.center()));
        asks.push(Ask::WorktreeMenu(Some(worktree.path.clone())));
    }
}

pub(super) fn worktree_detail(worktree: &WorktreeInfo) -> String {
    let mut lines = Vec::new();
    lines.push(match (&worktree.branch, &worktree.head) {
        (Some(branch), _) => t!("vcs-workspace-worktree-on", branch = branch.as_str()),
        (None, Some(head)) => t!("vcs-workspace-worktree-detached", commit = &head[..head.len().min(7)]),
        (None, None) => t!("vcs-workspace-worktree-detached", commit = ""),
    });
    if worktree.current {
        lines.push(t!("vcs-workspace-worktree-this"));
    } else if worktree.main {
        lines.push(t!("vcs-workspace-worktree-main"));
    }
    if worktree.locked {
        lines.push(match &worktree.lock_reason {
            Some(reason) => t!("vcs-workspace-worktree-locked-because", reason = reason.as_str()),
            None => t!("vcs-workspace-worktree-locked"),
        });
    }
    if worktree.prunable {
        lines.push(t!("vcs-workspace-worktree-gone"));
    } else if let Some(changes) = worktree.changes {
        lines.push(t!("vcs-workspace-worktree-changed", count = changes));
    }
    lines.join("\n")
}

pub(super) fn submodule_row(
    ui: &mut Ui,
    p: &IdePalette,
    view: &mut GitWorkspaceView,
    submodule: &SubmoduleInfo,
    asks: &mut Vec<Ask>,
) {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let inner = rect.shrink2(vec2(6.0, 0.0));
    if view.submodule_menu_for.as_deref() == Some(submodule.path.as_str()) {
        ui.painter().rect_filled(inner, 5, p.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(inner, 5, p.hover);
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 26.0, rect.center().y), Vec2::splat(12.0));
    glyph_submodule(ui.painter(), icon, p.text_secondary);
    let mark = Rect::from_center_size(pos2(inner.right() - 12.0, rect.center().y), Vec2::splat(11.0));
    let (_, mark_color) = submodule_state(p, submodule.status);
    match submodule.status {
        SubmoduleStatus::Current => glyph_check(ui.painter(), mark, mark_color),
        SubmoduleStatus::Moved | SubmoduleStatus::Conflicted => {
            ui.painter().circle_filled(mark.center(), 3.5, mark_color);
        }
        SubmoduleStatus::Uninitialized => {
            ui.painter().circle_stroke(mark.center(), 3.5, egui::Stroke::new(1.2, mark_color));
        }
    }
    let right = mark.left() - 6.0;
    let color = if submodule.status == SubmoduleStatus::Uninitialized { p.text_secondary } else { p.text };
    let room = (right - icon.right() - 8.0).max(8.0);
    let font = theme::regular(13.0);
    let galley = ui.painter().layout_job(super::super::vcs::truncated(&submodule.path, font, color, room));
    ui.painter().galley(pos2(icon.right() + 8.0, rect.center().y - galley.size().y / 2.0), galley, color);
    hint(ui, &response, &submodule.path, Some(&submodule_detail(submodule)), Side::Right);
    if response.double_clicked() && submodule.status != SubmoduleStatus::Uninitialized {
        asks.push(Ask::Submodule(submodule.path.clone(), SubmoduleAction::Open));
    }
    if view.submodule_menu_at.is_none() && view.submodule_menu_for.as_deref() == Some(submodule.path.as_str()) {
        view.submodule_menu_at = Some(pos2(rect.left() + 32.0, rect.bottom()));
    }
    if response.secondary_clicked() {
        view.submodule_right_click = Some(response.interact_pointer_pos().unwrap_or(rect.center()));
        asks.push(Ask::SubmoduleMenu(Some(submodule.path.clone())));
    }
}

fn submodule_state(p: &IdePalette, status: SubmoduleStatus) -> (String, Color32) {
    let color = match status {
        SubmoduleStatus::Current | SubmoduleStatus::Uninitialized => p.text_secondary,
        SubmoduleStatus::Moved => p.vcs_modified,
        SubmoduleStatus::Conflicted => p.vcs_conflict,
    };
    (submodule_state_text(status), color)
}

fn submodule_state_text(status: SubmoduleStatus) -> String {
    match status {
        SubmoduleStatus::Current => t!("vcs-workspace-submodule-current"),
        SubmoduleStatus::Moved => t!("vcs-workspace-submodule-moved"),
        SubmoduleStatus::Uninitialized => t!("vcs-workspace-submodule-uninitialized"),
        SubmoduleStatus::Conflicted => t!("vcs-workspace-submodule-conflicted"),
    }
}

pub(super) fn submodule_detail(submodule: &SubmoduleInfo) -> String {
    let short = |id: &Option<String>| id.as_deref().map(|id| id[..id.len().min(7)].to_owned()).unwrap_or_default();
    let mut lines = vec![submodule_state_text(submodule.status)];
    if let Some(url) = &submodule.url {
        lines.push(url.clone());
    }
    lines.push(t!("vcs-workspace-submodule-recorded", commit = short(&submodule.recorded)));
    match submodule.status {
        SubmoduleStatus::Uninitialized => lines.push(t!("vcs-workspace-submodule-not-cloned")),
        SubmoduleStatus::Current => lines.push(t!("vcs-workspace-submodule-at-recorded")),
        SubmoduleStatus::Moved => {
            lines.push(t!("vcs-workspace-submodule-checked-out", commit = short(&submodule.checked_out)));
        }
        SubmoduleStatus::Conflicted => lines.push(t!("vcs-workspace-submodule-conflict")),
    }
    lines.join("\n")
}

pub(super) fn worktree_menu(ui: &Ui, view: &mut GitWorkspaceView, asks: &mut Vec<Ask>) {
    let (Some(path), Some(at)) = (view.worktree_menu_for.clone(), view.worktree_menu_at) else { return };
    let worktree = view.worktrees.iter().find(|worktree| worktree.path == path).cloned().unwrap_or_default();
    let mut focus_first = view.worktree_menu_keyboard;
    let mut focused = false;
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("gw-worktree-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    popup.show(|ui| {
        ui.set_min_width(230.0);
        let mut item = |ui: &mut Ui, title: String, enabled: bool, action: WorktreeAction| {
            let response = ui.add_enabled(enabled, egui::Button::new(title));
            if enabled && focus_first && !ui.is_sizing_pass() {
                focus_first = false;
                focused = true;
                response.request_focus();
            }
            if response.clicked() {
                asks.push(Ask::Worktree(path.clone(), action));
                ui.close();
            }
        };
        let own = worktree.main || worktree.current;
        item(ui, t!("vcs-workspace-worktree-open"), !worktree.current && !worktree.prunable, WorktreeAction::Open);
        ui.separator();
        if worktree.locked {
            item(ui, t!("vcs-workspace-worktree-unlock"), true, WorktreeAction::Unlock);
        } else {
            item(ui, t!("vcs-workspace-worktree-lock"), !worktree.main, WorktreeAction::Lock);
        }
        ui.separator();
        item(ui, t!("vcs-workspace-worktree-remove"), !own, WorktreeAction::Remove);
    });
    if focused {
        view.worktree_menu_keyboard = false;
    }
    if !open {
        asks.push(Ask::WorktreeMenu(None));
    }
}

pub(super) fn submodule_menu(ui: &Ui, view: &mut GitWorkspaceView, asks: &mut Vec<Ask>) {
    let (Some(path), Some(at)) = (view.submodule_menu_for.clone(), view.submodule_menu_at) else { return };
    let status = view.submodules.iter().find(|submodule| submodule.path == path).map(|submodule| submodule.status);
    let initialised = status.is_some_and(|status| status != SubmoduleStatus::Uninitialized);
    let mut focus_first = view.submodule_menu_keyboard;
    let mut focused = false;
    let mut open = true;
    let popup = egui::Popup::new(ui.scope_id().with("gw-submodule-menu"), ui.ctx().clone(), at, ui.layer_id())
        .kind(egui::PopupKind::Menu)
        .layout(egui::Layout::top_down_justified(egui::Align::Min))
        .open_bool(&mut open)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    popup.show(|ui| {
        ui.set_min_width(230.0);
        let mut item = |ui: &mut Ui, title: String, enabled: bool, action: SubmoduleAction| {
            let response = ui.add_enabled(enabled, egui::Button::new(title));
            if enabled && focus_first && !ui.is_sizing_pass() {
                focus_first = false;
                focused = true;
                response.request_focus();
            }
            if response.clicked() {
                asks.push(Ask::Submodule(path.clone(), action));
                ui.close();
            }
        };
        item(ui, t!("vcs-workspace-submodule-update"), initialised, SubmoduleAction::Update);
        item(
            ui,
            t!("vcs-workspace-submodule-initialize"),
            status.is_some() && !initialised,
            SubmoduleAction::Initialize,
        );
        ui.separator();
        item(ui, t!("vcs-workspace-submodule-open"), initialised, SubmoduleAction::Open);
    });
    if focused {
        view.submodule_menu_keyboard = false;
    }
    if !open {
        asks.push(Ask::SubmoduleMenu(None));
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn draw_worktree_wip_row(
    ui: &Ui,
    p: &IdePalette,
    columns: &Columns,
    rect: Rect,
    response: &egui::Response,
    commit: &GraphCommit,
    changes: Option<usize>,
    selected: bool,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let color = lane_color(p, commit.color);
    let middle = rect.center().y;
    let node = pos2(columns.node_x(commit.lane), middle);
    let tint = if selected {
        Tint::Selected
    } else if response.hovered() {
        Tint::Hovered
    } else {
        Tint::Row
    };
    let band = Rect::from_min_max(
        pos2(columns.band(node.x, rect).0, rect.top() + 2.0),
        pos2(columns.band(node.x, rect).1, rect.bottom() - 2.0),
    );
    painter.rect_filled(band, 0, look::row_tint(p, color, tint));
    painter.rect_filled(Rect::from_min_size(pos2(columns.message.0, band.top()), vec2(2.0, band.height())), 0, color);
    super::draw_edges(&painter, p, columns, commit, rect);
    let graph = painter.with_clip_rect(Rect::from_x_y_ranges(columns.graph.0..=columns.graph.1, rect.y_range()));
    graph.circle_filled(node, NODE - 1.0, look::row_tint(p, color, tint));
    super::dashed_ring(&graph, node, color);
    let wip = painter.text(
        pos2(columns.message.0 + 10.0, middle),
        Align2::LEFT_CENTER,
        t!("vcs-workspace-wip"),
        theme::semibold(13.0),
        p.text_secondary,
    );
    let name = painter.text(
        pos2(wip.right() + 8.0, middle),
        Align2::LEFT_CENTER,
        t!("vcs-workspace-worktree-wip", name = commit.subject.as_str()),
        theme::regular(13.0),
        p.text_secondary,
    );
    if let Some(changes) = changes.filter(|changes| *changes > 0) {
        let galley =
            painter.layout_no_wrap(t!("vcs-workspace-wip-count", count = changes), theme::regular(11.5), p.text);
        let badge = Rect::from_min_size(pos2(name.right() + 10.0, middle - 9.0), vec2(galley.size().x + 14.0, 18.0));
        painter.rect_filled(badge, 9, p.vcs_modified.gamma_multiply(0.3));
        painter.galley(badge.center() - galley.size() / 2.0, galley, p.text);
    }
}

pub(super) fn offered_folder(root: &std::path::Path, branch: &str) -> String {
    zen_workbench::vcs::worktree::default_worktree_folder(root, branch).display().to_string()
}

pub(super) fn local_name(branch: &str, remote: bool) -> String {
    if remote { branch.split_once('/').map_or(branch, |(_, name)| name).to_owned() } else { branch.to_owned() }
}

pub(super) fn remove_text(worktree: &WorktreeInfo) -> String {
    let folder = worktree.path.display().to_string();
    let mut text = t!("vcs-remove-worktree-text", name = worktree.label(), folder = folder.as_str());
    if let Some(changes) = worktree.changes.filter(|changes| *changes > 0) {
        text.push(' ');
        text.push_str(&t!("vcs-remove-worktree-changes", count = changes));
    }
    text
}

pub(super) fn remove_params(worktree: &WorktreeInfo, delete_branch: bool) -> serde_json::Value {
    serde_json::json!({
        "path": worktree.path,
        "deleteBranch": delete_branch && worktree.branch.is_some(),
        "force": worktree.changes.unwrap_or(0) > 0,
    })
}

pub(super) fn create_params(branch: &str, folder: &str, new_branch: &str, open: bool) -> serde_json::Value {
    let new_branch = Some(new_branch.trim()).filter(|name| !name.is_empty());
    serde_json::json!({
        "branch": branch,
        "path": PathBuf::from(folder.trim()),
        "newBranch": new_branch,
        "open": open,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialogs_run_their_commands_with_what_was_chosen() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let worktree = WorktreeInfo {
            path: "/work/app-feature".into(),
            name: "app-feature".into(),
            branch: Some("feature".into()),
            changes: Some(2),
            ..WorktreeInfo::default()
        };
        let params = remove_params(&worktree, true);
        assert_eq!(params["deleteBranch"], true);
        assert_eq!(params["force"], true, "removed with its changes, as the dialog says");
        assert!(remove_text(&worktree).contains("2"), "{}", remove_text(&worktree));
        let detached = WorktreeInfo { branch: None, changes: Some(0), ..worktree.clone() };
        assert_eq!(remove_params(&detached, true)["deleteBranch"], false, "no branch to delete");
        assert_eq!(remove_params(&detached, false)["force"], false);

        let params = create_params("origin/fix", " /work/app-fix ", " ", true);
        assert_eq!(params["newBranch"], serde_json::Value::Null);
        assert_eq!(params["path"], "/work/app-fix");
        assert_eq!(local_name("origin/fix/login", true), "fix/login");
        assert_eq!(local_name("feature", false), "feature");
    }

    fn menu_frames(view: &mut GitWorkspaceView, keys: &[Option<egui::Key>], submodules: bool) -> Vec<Ask> {
        let ctx = egui::Context::default();
        let mut ran = Vec::new();
        for key in keys {
            let events = key
                .map(|key| {
                    vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }]
                })
                .unwrap_or_default();
            let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                let mut asks = Vec::new();
                if submodules {
                    submodule_menu(ui, view, &mut asks);
                } else {
                    worktree_menu(ui, view, &mut asks);
                }
                ran.extend(asks.into_iter().filter(|ask| matches!(ask, Ask::Worktree(..) | Ask::Submodule(..))));
            });
            output.textures_delta.clear();
        }
        ran
    }

    #[test]
    fn the_worktree_and_submodule_menus_offer_what_applies_from_the_keyboard() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let down = Some(egui::Key::ArrowDown);
        let enter = Some(egui::Key::Enter);
        let linked =
            WorktreeInfo { path: "/work/app-side".into(), branch: Some("side".into()), ..WorktreeInfo::default() };
        let mut view = GitWorkspaceView {
            worktrees: vec![linked.clone()],
            worktree_menu_for: Some(linked.path.clone()),
            worktree_menu_at: Some(pos2(100.0, 100.0)),
            worktree_menu_keyboard: true,
            ..GitWorkspaceView::default()
        };
        let ran = menu_frames(&mut view, &[None, None, None, down, None, down, None, enter, None], false);
        assert!(matches!(ran.as_slice(), [Ask::Worktree(path, WorktreeAction::Remove)] if *path == linked.path));
        assert!(!view.worktree_menu_keyboard, "the keyboard was given once");

        let own = WorktreeInfo { current: true, locked: true, ..linked.clone() };
        let mut view = GitWorkspaceView {
            worktrees: vec![own.clone()],
            worktree_menu_for: Some(own.path.clone()),
            worktree_menu_at: Some(pos2(100.0, 100.0)),
            worktree_menu_keyboard: true,
            ..GitWorkspaceView::default()
        };
        let ran = menu_frames(&mut view, &[None, None, None, enter, None], false);
        assert!(matches!(ran.as_slice(), [Ask::Worktree(_, WorktreeAction::Unlock)]), "the first item that applies");

        let submodule = SubmoduleInfo { path: "libs/lib".into(), ..SubmoduleInfo::default() };
        let mut view = GitWorkspaceView {
            submodules: vec![submodule.clone()],
            submodule_menu_for: Some(submodule.path.clone()),
            submodule_menu_at: Some(pos2(100.0, 100.0)),
            submodule_menu_keyboard: true,
            ..GitWorkspaceView::default()
        };
        let ran = menu_frames(&mut view, &[None, None, None, enter, None], true);
        assert!(matches!(ran.as_slice(), [Ask::Submodule(path, SubmoduleAction::Initialize)] if path == "libs/lib"));
        let moved = SubmoduleInfo { status: SubmoduleStatus::Moved, ..submodule };
        let mut view = GitWorkspaceView {
            submodules: vec![moved],
            submodule_menu_for: Some("libs/lib".into()),
            submodule_menu_at: Some(pos2(100.0, 100.0)),
            submodule_menu_keyboard: true,
            ..GitWorkspaceView::default()
        };
        let ran = menu_frames(&mut view, &[None, None, None, down, None, enter, None], true);
        assert!(matches!(ran.as_slice(), [Ask::Submodule(_, SubmoduleAction::Open)]), "Initialise is skipped");
    }

    #[test]
    fn hints_say_where_a_worktree_and_a_submodule_are() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let worktree = WorktreeInfo {
            path: "/work/app".into(),
            name: "app".into(),
            head: Some("1234567890".into()),
            locked: true,
            lock_reason: Some("on a disk".into()),
            changes: Some(1),
            ..WorktreeInfo::default()
        };
        let detail = worktree_detail(&worktree);
        assert!(detail.contains("1234567"), "{detail}");
        assert!(detail.contains("on a disk"), "{detail}");
        let submodule = SubmoduleInfo {
            path: "libs/lib".into(),
            url: Some("https://example.com/lib.git".into()),
            recorded: Some("abcdef0123".into()),
            checked_out: Some("0123456789".into()),
            status: SubmoduleStatus::Moved,
            ..SubmoduleInfo::default()
        };
        let detail = submodule_detail(&submodule);
        assert!(detail.contains("abcdef0") && detail.contains("0123456") && detail.contains("example.com"), "{detail}");
    }
}

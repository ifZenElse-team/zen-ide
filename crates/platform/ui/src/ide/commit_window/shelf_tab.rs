use std::collections::BTreeSet;
use std::path::PathBuf;

use egui::{Align2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::vcs::ChangeInfo;
use zen_workbench::vcs::shelf::{ShelfInfo, ShelfList, ShelvedFile};

use super::{ChangesDialog, customization, file_label, glyph_diff};
use crate::ide::glyphs::{Glyph, glyph_pop, glyph_trash};
use crate::ide::vcs::format_date;
use crate::ide::{IdePalette, IdeView, Side, hint, hover, metrics};
use crate::theme;

#[derive(Debug, Default)]
pub(in crate::ide) struct ShelfView {
    pub(super) shelves: Vec<ShelfInfo>,
    root: Option<PathBuf>,
    pub(in crate::ide) stale: bool,
    selected: Option<(u64, Option<PathBuf>)>,
    collapsed: BTreeSet<u64>,
    unshelved_open: bool,
}

enum ShelfAction {
    Unshelve(u64, Vec<PathBuf>),
    Diff(u64, PathBuf),
    Rename(u64, String),
    Delete(u64, String),
    CreatePatch(u64),
    CopyPatch(u64),
}

fn as_change(file: &ShelvedFile) -> ChangeInfo {
    ChangeInfo {
        path: file.path.clone(),
        relative: file.relative.clone(),
        status: file.status.clone(),
        old_path: file.old_path.clone(),
        ..ChangeInfo::default()
    }
}

impl IdeView {
    pub(in crate::ide) fn refresh_shelf(&mut self, registry: &Registry, session: &mut Session, force: bool) {
        let root = self.vcs.info.root.clone();
        let view = &mut self.commit_window.shelf;
        if !force && !view.stale && view.root == root {
            return;
        }
        view.stale = false;
        view.root = root;
        let list: ShelfList = registry
            .dispatch(session, "vcs.shelf.list", json!({}))
            .ok()
            .and_then(|list| serde_json::from_value(list).ok())
            .unwrap_or_default();
        view.shelves = list.shelves;
        if view.selected.as_ref().is_some_and(|(id, _)| !view.shelves.iter().any(|shelf| shelf.id == *id)) {
            view.selected = None;
        }
    }

    pub(in crate::ide) fn shelf_context(&self) -> Option<(u64, Vec<PathBuf>)> {
        let view = &self.commit_window.shelf;
        if let Some((id, file)) = &view.selected {
            return Some((*id, file.iter().cloned().collect()));
        }
        view.shelves.iter().find(|shelf| !shelf.applied).map(|shelf| (shelf.id, Vec::new()))
    }

    pub(super) fn shelf_tab(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, p: &IdePalette) {
        let mut actions = Vec::new();
        self.shelf_toolbar(ui, p, &mut actions);
        egui::ScrollArea::vertical()
            .id_salt("commit-shelf")
            .auto_shrink([false, false])
            .show(ui, |ui| self.shelf_tree(ui, p, &mut actions));
        for action in actions {
            match action {
                ShelfAction::Unshelve(id, paths) => self.open_unshelve(registry, session, id, paths),
                ShelfAction::Diff(id, path) => {
                    match registry.dispatch(session, "vcs.shelf.diff", json!({ "id": id, "path": path })) {
                        Ok(view) => {
                            if let Ok(view) = serde_json::from_value(view) {
                                self.show_diff(session, view);
                            }
                        }
                        Err(error) => self.report(ui, error),
                    }
                }
                ShelfAction::Rename(id, name) => {
                    self.changes_dialog = Some(ChangesDialog::RenameShelf { id, name, focused: false, error: None });
                }
                ShelfAction::Delete(id, name) => {
                    self.changes_dialog = Some(ChangesDialog::DeleteShelf { id, name, error: None });
                }
                ShelfAction::CreatePatch(id) => self.open_create_shelf_patch(registry, session, id),
                ShelfAction::CopyPatch(id) => {
                    if let Err(error) = self.copy_patch(ui.ctx(), registry, session, Vec::new(), Some(id)) {
                        self.report(ui, error);
                    }
                }
            }
        }
    }

    fn shelf_toolbar(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<ShelfAction>) {
        let view = &self.commit_window.shelf;
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
        let selected = view.selected.clone();
        let shelf = selected.as_ref().and_then(|(id, _)| view.shelves.iter().find(|shelf| shelf.id == *id));
        let file = selected.as_ref().and_then(|(_, file)| file.clone());
        let buttons: [(Glyph, String, bool); 3] = [
            (glyph_pop, t!("keymap-view-vcs-unshelve"), shelf.is_some()),
            (glyph_diff, t!("keymap-view-vcs-showDiff"), file.is_some()),
            (glyph_trash, t!("vcs-shelf-delete"), shelf.is_some()),
        ];
        let mut x = bar.left() + 10.0;
        for (index, (glyph, name, enabled)) in buttons.into_iter().enumerate() {
            let button = Rect::from_min_size(pos2(x, bar.center().y - 12.0), Vec2::splat(24.0));
            let response = ui.interact(
                button,
                ui.scope_id().with(("shelf-tool", index)),
                if enabled { Sense::click() } else { Sense::hover() },
            );
            if enabled {
                hover(ui, &response, p);
            }
            hint(ui, &response, &name, None, Side::Below);
            let color = if enabled { p.text_secondary } else { p.text_secondary.gamma_multiply(0.4) };
            glyph(ui.painter(), button.shrink(6.0), color);
            if response.clicked()
                && let Some(shelf) = shelf
            {
                match index {
                    0 => actions.push(ShelfAction::Unshelve(shelf.id, file.iter().cloned().collect())),
                    1 => actions.extend(file.clone().map(|file| ShelfAction::Diff(shelf.id, file))),
                    _ => actions.push(ShelfAction::Delete(shelf.id, shelf.name.clone())),
                }
            }
            x = button.right() + 4.0;
            if index == 1 {
                ui.painter().vline(x + 2.0, bar.y_range().shrink(7.0), Stroke::new(1.0, p.tab_border));
                x += 8.0;
            }
        }
    }

    fn shelf_tree(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<ShelfAction>) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let root = self.vcs.info.root.clone().unwrap_or_default();
        let view = &mut self.commit_window.shelf;
        if view.shelves.is_empty() {
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                ui.add_space(14.0);
                ui.label(egui::RichText::new(t!("vcs-shelf-empty")).color(p.text_secondary));
            });
            return;
        }
        let offset = i64::from(zen_vcs::local_offset());
        let (applied, waiting): (Vec<ShelfInfo>, Vec<ShelfInfo>) =
            view.shelves.iter().cloned().partition(|shelf| shelf.applied);
        for shelf in &waiting {
            shelf_rows(ui, p, view, shelf, 0.0, offset, &root, actions);
        }
        if applied.is_empty() {
            return;
        }
        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.hover);
        }
        let chevron = Rect::from_center_size(pos2(row.left() + 20.0, row.center().y), Vec2::splat(11.0));
        if view.unshelved_open {
            crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
        } else {
            crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
        }
        let title = t!("vcs-shelf-unshelved");
        crate::accessibility::tree_item(&response, 1, Some(view.unshelved_open), false, &title);
        let label = ui.painter().text(
            pos2(row.left() + 34.0, row.center().y),
            Align2::LEFT_CENTER,
            &title,
            theme::regular(13.0),
            p.text,
        );
        ui.painter().text(
            pos2(label.right() + 8.0, row.center().y),
            Align2::LEFT_CENTER,
            applied.len().to_string(),
            theme::regular(12.5),
            p.text_secondary,
        );
        if response.clicked() {
            view.unshelved_open = !view.unshelved_open;
        }
        if view.unshelved_open {
            for shelf in &applied {
                shelf_rows(ui, p, view, shelf, 16.0, offset, &root, actions);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn shelf_rows(
    ui: &mut Ui,
    p: &IdePalette,
    view: &mut ShelfView,
    shelf: &ShelfInfo,
    indent: f32,
    offset: i64,
    root: &std::path::Path,
    actions: &mut Vec<ShelfAction>,
) {
    let open = !view.collapsed.contains(&shelf.id);
    let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
    let selected = view.selected == Some((shelf.id, None));
    if selected {
        ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.hover);
    }
    let chevron = Rect::from_center_size(pos2(row.left() + 20.0 + indent, row.center().y), Vec2::splat(11.0));
    if open {
        crate::icons::chevron_down(ui.painter(), chevron, p.text_secondary);
    } else {
        crate::icons::chevron_right(ui.painter(), chevron, p.text_secondary);
    }
    crate::accessibility::tree_item(&response, 1 + usize::from(indent > 0.0), Some(open), selected, &shelf.name);
    let color = if shelf.applied { p.text_secondary } else { p.text };
    let painter = ui.painter().with_clip_rect(row);
    let label = painter.text(
        pos2(row.left() + 34.0 + indent, row.center().y),
        Align2::LEFT_CENTER,
        &shelf.name,
        theme::regular(13.0),
        color,
    );
    let detail = format!(
        "{}  ·  {}",
        t!("vcs-commit-files", count = shelf.files.len()),
        format_date(i64::try_from(shelf.time).unwrap_or_default() + offset)
    );
    painter.text(
        pos2(label.right() + 8.0, row.center().y),
        Align2::LEFT_CENTER,
        detail,
        theme::regular(12.5),
        p.text_secondary,
    );
    let on_chevron =
        response.interact_pointer_pos().is_some_and(|pointer| pointer.x < chevron.right() + 6.0 && response.clicked());
    if (on_chevron || response.double_clicked()) && !view.collapsed.remove(&shelf.id) {
        view.collapsed.insert(shelf.id);
    }
    if response.clicked() || response.secondary_clicked() {
        view.selected = Some((shelf.id, None));
    }
    response.context_menu(|ui| {
        customization::popup(
            ui,
            customization::SHELF_POPUP,
            |_| true,
            |ui, slot| {
                if ui.button(t!(slot)).clicked() {
                    actions.push(match slot {
                        "keymap-view-vcs-unshelve" => ShelfAction::Unshelve(shelf.id, Vec::new()),
                        "vcs-shelf-rename" => ShelfAction::Rename(shelf.id, shelf.name.clone()),
                        "keymap-view-vcs-createPatch" => ShelfAction::CreatePatch(shelf.id),
                        "keymap-view-vcs-copyPatch" => ShelfAction::CopyPatch(shelf.id),
                        _ => ShelfAction::Delete(shelf.id, shelf.name.clone()),
                    });
                    ui.close();
                }
            },
        );
    });
    if !open {
        return;
    }
    for file in &shelf.files {
        let (row, response) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TREE_ROW), Sense::click());
        let selected = view.selected.as_ref() == Some(&(shelf.id, Some(file.path.clone())));
        if selected {
            ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.selection);
        } else if response.hovered() {
            ui.painter().rect_filled(row.shrink2(vec2(6.0, 0.0)), 5, p.hover);
        }
        crate::accessibility::tree_item(&response, 2 + usize::from(indent > 0.0), None, selected, &file.relative);
        file_label(ui, row, 50.0 + indent, &as_change(file), root, p);
        if response.double_clicked() {
            actions.push(ShelfAction::Diff(shelf.id, file.path.clone()));
        } else if response.clicked() || response.secondary_clicked() {
            view.selected = Some((shelf.id, Some(file.path.clone())));
        }
        response.context_menu(|ui| {
            customization::popup(
                ui,
                customization::SHELF_FILE_POPUP,
                |_| true,
                |ui, slot| {
                    if ui.button(t!(slot)).clicked() {
                        actions.push(match slot {
                            "keymap-view-vcs-showDiff" => ShelfAction::Diff(shelf.id, file.path.clone()),
                            _ => ShelfAction::Unshelve(shelf.id, vec![file.path.clone()]),
                        });
                        ui.close();
                    }
                },
            );
        });
    }
}

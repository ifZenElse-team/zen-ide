use std::path::Path;

use egui::Ui;
use zen_i18n::t;

use super::libraries_tree::{RowKind, RowMode, group_path};
use super::{IdeView, TreeFrame};

impl IdeView {
    pub(super) fn scratch_rows(&mut self, ui: &mut Ui, project: &Path, active: Option<&Path>, frame: &mut TreeFrame) {
        let [scratches, buffers] = match self.scratch_folders.as_slice() {
            [scratches, buffers] => [scratches.clone(), buffers.clone()],
            _ => return,
        };
        let title = t!("ide-scratch-node");
        let top = group_path(project, "scratches");
        let group = RowMode { read_only: true, label: Some(&title), kind: RowKind::Group, ..RowMode::default() };
        self.tree_row(ui, &folder_entry(&top, &title), 0, active, frame, group);
        if !self.expanded.contains(&top) {
            return;
        }
        let label = t!("ide-scratch-scratches");
        let mode = RowMode { read_only: true, label: Some(&label), ..RowMode::default() };
        self.tree_row(ui, &folder_entry(&scratches, &label), 1, active, frame, mode);
        let has_buffers = std::fs::read_dir(&buffers).is_ok_and(|mut entries| entries.next().is_some());
        if has_buffers {
            let label = t!("ide-scratch-buffers");
            let mode = RowMode { read_only: true, label: Some(&label), ..RowMode::default() };
            self.tree_row(ui, &folder_entry(&buffers, &label), 1, active, frame, mode);
        }
    }
}

fn folder_entry(path: &Path, label: &str) -> zen_vfs::Entry {
    let name = path.file_name().map_or_else(|| label.to_owned(), |name| name.to_string_lossy().into_owned());
    zen_vfs::Entry { path: path.to_owned(), name, is_dir: true, is_symlink: false }
}

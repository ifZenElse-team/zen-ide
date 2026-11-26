use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use egui::{Align2, Key, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_i18n::t;

use super::GitAction;
use crate::ide::{IdePalette, hover};
use crate::theme;

const ROW: f32 = 24.0;
const INDENT: f32 = 16.0;

pub(super) struct PathsDialog {
    root: PathBuf,
    checked: BTreeSet<String>,
    expanded: BTreeSet<String>,
    entries: HashMap<String, Vec<(String, bool)>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Mark {
    Off,
    On,
    Covered,
    Partial,
}

fn inside(path: &str, folder: &str) -> bool {
    path.len() > folder.len() && path.starts_with(folder) && path.as_bytes()[folder.len()] == b'/'
}

pub(super) fn mark(checked: &BTreeSet<String>, path: &str) -> Mark {
    if checked.contains(path) {
        Mark::On
    } else if checked.iter().any(|known| inside(path, known)) {
        Mark::Covered
    } else if checked.iter().any(|known| inside(known, path)) {
        Mark::Partial
    } else {
        Mark::Off
    }
}

pub(super) fn toggle(checked: &mut BTreeSet<String>, path: &str) {
    match mark(checked, path) {
        Mark::On => {
            checked.remove(path);
        }
        Mark::Covered => {}
        Mark::Off | Mark::Partial => {
            checked.retain(|known| !inside(known, path));
            checked.insert(path.to_owned());
        }
    }
}

pub(super) fn sorted(mut entries: Vec<(String, bool)>) -> Vec<(String, bool)> {
    entries.retain(|(name, _)| name != ".git");
    entries.sort_by(|(a, a_dir), (b, b_dir)| b_dir.cmp(a_dir).then_with(|| a.to_lowercase().cmp(&b.to_lowercase())));
    entries
}

impl PathsDialog {
    pub(super) fn new(root: &Path, paths: &[String]) -> Self {
        let mut expanded = BTreeSet::new();
        for path in paths {
            let mut folder = String::new();
            for part in path.split('/').collect::<Vec<_>>().iter().rev().skip(1).rev() {
                if !folder.is_empty() {
                    folder.push('/');
                }
                folder.push_str(part);
                expanded.insert(folder.clone());
            }
        }
        PathsDialog {
            root: root.to_owned(),
            checked: paths.iter().cloned().collect(),
            expanded,
            entries: HashMap::new(),
        }
    }

    fn entries(&mut self, folder: &str) -> Vec<(String, bool)> {
        let root = &self.root;
        self.entries
            .entry(folder.to_owned())
            .or_insert_with(|| {
                let entries = std::fs::read_dir(root.join(folder))
                    .into_iter()
                    .flatten()
                    .flatten()
                    .filter_map(|entry| {
                        let name = entry.file_name().to_str()?.to_owned();
                        Some((name, entry.file_type().ok()?.is_dir()))
                    })
                    .collect();
                sorted(entries)
            })
            .clone()
    }

    fn rows(&mut self) -> Vec<(String, usize, bool)> {
        let mut rows = Vec::new();
        self.push_rows("", 0, &mut rows);
        rows
    }

    fn push_rows(&mut self, folder: &str, depth: usize, rows: &mut Vec<(String, usize, bool)>) {
        for (name, dir) in self.entries(folder) {
            let path = if folder.is_empty() { name } else { format!("{folder}/{name}") };
            rows.push((path.clone(), depth, dir));
            if dir && self.expanded.contains(&path) {
                self.push_rows(&path, depth + 1, rows);
            }
        }
    }

    pub(super) fn show(&mut self, ui: &Ui, p: &IdePalette, actions: &mut Vec<GitAction>) -> bool {
        let (mut ok, mut cancel) = (false, false);
        let rows = self.rows();
        let modal =
            egui::Modal::new(ui.scope_id().with("git-paths")).frame(crate::ide::dialog_frame(p)).show(ui.ctx(), |ui| {
                ui.set_width(460.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(egui::RichText::new(t!("vcs-log-paths-title")).font(theme::semibold(15.0)).color(p.text));
                ui.add_space(12.0);
                let frame = egui::Frame::new()
                    .fill(p.island)
                    .stroke(Stroke::new(1.0, p.tab_border))
                    .corner_radius(6)
                    .inner_margin(egui::Margin::same(4));
                frame.show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("git-paths-tree")
                        .max_height(360.0)
                        .min_scrolled_height(360.0)
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            for (path, depth, dir) in &rows {
                                self.row(ui, p, path, *depth, *dir);
                            }
                        });
                });
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    let count = self.checked.len();
                    ui.label(
                        egui::RichText::new(t!("vcs-log-paths-selected", count = count))
                            .font(theme::regular(12.5))
                            .color(p.text_secondary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        if crate::ide::vcs::primary_button(ui, p, &t!("vcs-ok")).clicked() {
                            ok = true;
                        }
                        if crate::ide::vcs::secondary_button(ui, p, &t!("settings-cancel")).clicked() {
                            cancel = true;
                        }
                    });
                });
                if ui.input(|input| input.key_pressed(Key::Enter)) {
                    ok = true;
                }
            });
        if ok {
            let paths: Vec<&String> = self.checked.iter().collect();
            actions.push(GitAction::Run("vcs.log.filter", json!({ "paths": paths })));
            return false;
        }
        !(cancel || modal.should_close())
    }

    fn row(&mut self, ui: &mut Ui, p: &IdePalette, path: &str, depth: usize, dir: bool) {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
        hover(ui, &response, p);
        let name = path.rsplit('/').next().unwrap_or(path);
        let state = mark(&self.checked, path);
        let open = self.expanded.contains(path);
        let painter = ui.painter().with_clip_rect(rect);
        let mut x = rect.left() + 6.0 + depth as f32 * INDENT;
        let y = rect.center().y;
        let arrow = Rect::from_center_size(pos2(x + 6.0, y), Vec2::splat(12.0));
        if dir {
            if open {
                crate::icons::chevron_down(&painter, arrow, p.text_secondary);
            } else {
                crate::icons::chevron_right(&painter, arrow, p.text_secondary);
            }
        }
        x += 16.0;
        let check = Rect::from_center_size(pos2(x + 7.0, y), Vec2::splat(14.0));
        let shown = match state {
            Mark::Off => Some(false),
            Mark::On => Some(true),
            Mark::Covered => Some(true),
            Mark::Partial => None,
        };
        crate::ide::commit_window::checkbox(&painter, check, shown, p);
        if state == Mark::Covered {
            painter.rect_filled(check, 3, p.island.gamma_multiply(0.45));
        }
        x += 22.0;
        let icon = Rect::from_center_size(pos2(x + 8.0, y), Vec2::splat(16.0));
        if dir {
            crate::file_icons::folder_icon(&painter, icon, name, false, open);
        } else {
            crate::file_icons::file_icon(&painter, icon, name);
        }
        x += 22.0;
        painter.text(pos2(x, y), Align2::LEFT_CENTER, name, theme::regular(13.0), p.text);
        crate::accessibility::item(&response, egui::Role::CheckBox, state == Mark::On, name);
        let at = response.interact_pointer_pos();
        let on_arrow = at.is_some_and(|at| dir && arrow.expand(4.0).contains(at));
        if response.double_clicked() && dir || response.clicked() && on_arrow {
            if !self.expanded.remove(path) {
                self.expanded.insert(path.to_owned());
            }
        } else if response.clicked() {
            toggle(&mut self.checked, path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    #[test]
    fn a_checked_folder_covers_what_is_inside_it() {
        let checked = set(&["docs", "src/main.go"]);
        assert_eq!(mark(&checked, "docs"), Mark::On);
        assert_eq!(mark(&checked, "docs/guide/a.md"), Mark::Covered);
        assert_eq!(mark(&checked, "docsy"), Mark::Off, "a name that only starts the same");
        assert_eq!(mark(&checked, "src"), Mark::Partial);
        assert_eq!(mark(&checked, "src/util.go"), Mark::Off);
    }

    #[test]
    fn checking_a_folder_takes_the_place_of_its_contents() {
        let mut checked = set(&["src/a.go", "src/b/c.go", "srcs"]);
        toggle(&mut checked, "src");
        assert_eq!(checked, set(&["src", "srcs"]));
        toggle(&mut checked, "src/a.go");
        assert_eq!(checked, set(&["src", "srcs"]), "covered by src");
        toggle(&mut checked, "src");
        assert_eq!(checked, set(&["srcs"]));
    }

    #[test]
    fn folders_come_first_and_git_is_left_out() {
        let entries = vec![
            ("b.go".to_owned(), false),
            (".git".to_owned(), true),
            ("Docs".to_owned(), true),
            ("a.go".to_owned(), false),
            ("cmd".to_owned(), true),
        ];
        let names: Vec<String> = sorted(entries).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["cmd", "Docs", "a.go", "b.go"]);
    }

    #[test]
    fn the_tree_lists_expanded_folders_in_place() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("cmd/tool")).unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("cmd/tool/main.go"), "").unwrap();
        std::fs::write(dir.path().join("cmd/a.go"), "").unwrap();
        std::fs::write(dir.path().join("go.mod"), "").unwrap();
        let mut dialog = PathsDialog::new(dir.path(), &["cmd/tool/main.go".to_owned()]);
        let rows: Vec<(String, usize)> = dialog.rows().into_iter().map(|(path, depth, _)| (path, depth)).collect();
        let expected = [("cmd", 0), ("cmd/tool", 1), ("cmd/tool/main.go", 2), ("cmd/a.go", 1), ("go.mod", 0)];
        assert_eq!(rows, expected.map(|(path, depth)| (path.to_owned(), depth)));
        assert_eq!(mark(&dialog.checked, "cmd"), Mark::Partial);
    }
}

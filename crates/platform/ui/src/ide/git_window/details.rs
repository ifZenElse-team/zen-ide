use egui::{Align2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use zen_i18n::t;

use super::super::glyphs::{glyph_collapse_all, glyph_diff, glyph_expand_all, glyph_folder, glyph_group};
use super::super::run_window::Tool;
use super::super::vcs::status_color;
use super::super::{IdePalette, Side};
use super::format::{self, ChangeRow};
use super::{BAR, GitAction, GitWindow};
use crate::theme;

const FILE_ROW: f32 = 24.0;
const INDENT: f32 = 18.0;

impl GitWindow {
    pub(super) fn details_panel(&mut self, ui: &mut Ui, p: &IdePalette, actions: &mut Vec<GitAction>) {
        let full = ui.max_rect();
        let Some(details) = self.details.clone() else {
            let split = (full.top() + BAR + (full.height() - BAR) * 0.5).round();
            ui.painter().hline(full.x_range(), split, Stroke::new(1.0, p.frame));
            let disabled = [
                (bar_x(full, 19.0), glyph_diff as super::super::glyphs::Glyph),
                (bar_x(full, 49.0), glyph_group),
                (bar_x(full, -49.0), glyph_expand_all),
                (bar_x(full, -19.0), glyph_collapse_all),
            ];
            for (x, glyph) in disabled {
                glyph(
                    ui.painter(),
                    Rect::from_center_size(pos2(x, full.top() + BAR / 2.0), Vec2::splat(14.0)),
                    p.gutter_text,
                );
            }
            for (center, text) in [
                ((full.top() + BAR + split) / 2.0, t!("vcs-log-select-commit")),
                ((split + full.bottom()) / 2.0, t!("vcs-log-no-commit")),
            ] {
                let mut job =
                    egui::text::LayoutJob::simple(text, theme::regular(13.0), p.text_secondary, full.width() - 24.0);
                job.halign = egui::Align::Center;
                let galley = ui.painter().layout_job(job);
                ui.painter().galley(pos2(full.center().x, center - galley.size().y / 2.0), galley, p.text_secondary);
            }
            return;
        };
        let bar = Rect::from_min_size(full.min, vec2(full.width(), BAR));
        let y = bar.center().y;
        let at = |x: f32| Rect::from_center_size(pos2(x, y), Vec2::splat(26.0));
        let has_files = !details.files.is_empty();
        if Tool::new("git-show-diff", glyph_diff, t!("vcs-change-diff"), p.text_secondary).show_hinted(
            ui,
            p,
            at(bar.left() + 19.0),
            has_files,
            Side::Below,
        ) {
            let file = self.selected_change.and_then(|index| details.files.get(index)).or(details.files.first());
            if let Some(file) = file {
                actions.push(GitAction::Diff { id: details.id.clone(), path: file.path.clone() });
            }
        }
        let group = Tool {
            on: Some(!self.flat_changes),
            ..Tool::new("git-group-changes", glyph_group, t!("vcs-log-group-changes"), p.text_secondary)
        };
        if group.show_hinted(ui, p, at(bar.left() + 49.0), true, Side::Below) {
            actions.push(GitAction::View("view.vcs.logGroupChanges"));
        }
        let grouped = !self.flat_changes;
        if Tool::new("git-collapse-changes", glyph_collapse_all, t!("vcs-branches-collapse"), p.text_secondary)
            .show_hinted(ui, p, at(bar.right() - 19.0), grouped, Side::Below)
        {
            actions.push(GitAction::View("view.vcs.logCollapseChanges"));
        }
        if Tool::new("git-expand-changes", glyph_expand_all, t!("vcs-branches-expand"), p.text_secondary).show_hinted(
            ui,
            p,
            at(bar.right() - 49.0),
            grouped,
            Side::Below,
        ) {
            actions.push(GitAction::View("view.vcs.logExpandChanges"));
        }
        let split = (full.top() + BAR + (full.height() - BAR) * 0.5).round();
        let files = Rect::from_min_max(pos2(full.left(), bar.bottom()), pos2(full.right(), split));
        let text = Rect::from_min_max(pos2(full.left(), split + 1.0), full.max);
        ui.painter().hline(full.x_range(), split, Stroke::new(1.0, p.frame));
        let paths: Vec<&str> = details.files.iter().map(|file| file.relative.as_str()).collect();
        let rows = format::change_rows(&paths, grouped, &self.collapsed_changes);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(files).id_salt("git-changes"));
        let mut toggle: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("git-changes-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in &rows {
                let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), FILE_ROW), Sense::click());
                let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                let y = rect.center().y;
                match row {
                    ChangeRow::Folder { key, name, depth, files, expanded } => {
                        if response.hovered() {
                            painter.rect_filled(rect.shrink2(vec2(4.0, 0.0)), 5, p.hover);
                        }
                        let icon = rect.left() + 32.0 + INDENT * *depth as f32;
                        let chevron = Rect::from_center_size(pos2(icon - 16.0, y), Vec2::splat(12.0));
                        if *expanded {
                            crate::icons::chevron_down(&painter, chevron, p.text_secondary);
                        } else {
                            crate::icons::chevron_right(&painter, chevron, p.text_secondary);
                        }
                        glyph_folder(
                            &painter,
                            Rect::from_center_size(pos2(icon, y), Vec2::splat(14.0)),
                            p.text_secondary,
                        );
                        let shown =
                            painter.text(pos2(icon + 12.0, y), Align2::LEFT_CENTER, name, theme::regular(13.0), p.text);
                        painter.text(
                            pos2(shown.right() + 7.0, y),
                            Align2::LEFT_CENTER,
                            t!("vcs-commit-files", count = *files),
                            theme::regular(13.0),
                            p.text_secondary,
                        );
                        if response.clicked() {
                            toggle = Some(key.clone());
                        }
                    }
                    ChangeRow::File { index, name, depth } => {
                        let file = &details.files[*index];
                        let selected = self.selected_change == Some(*index);
                        if selected {
                            painter.rect_filled(rect.shrink2(vec2(4.0, 0.0)), 5, p.selection);
                        } else if response.hovered() {
                            painter.rect_filled(rect.shrink2(vec2(4.0, 0.0)), 5, p.hover);
                        }
                        let icon = rect.left() + 32.0 + INDENT * *depth as f32;
                        crate::file_icons::file_icon(
                            &painter,
                            Rect::from_center_size(pos2(icon, y), Vec2::splat(15.0)),
                            name,
                        );
                        let status = match file.status.as_str() {
                            "added" => zen_vcs::FileStatus::Added,
                            "deleted" => zen_vcs::FileStatus::Deleted,
                            "renamed" => zen_vcs::FileStatus::Renamed,
                            _ => zen_vcs::FileStatus::Modified,
                        };
                        let shown = painter.text(
                            pos2(icon + 12.0, y),
                            Align2::LEFT_CENTER,
                            name,
                            theme::regular(13.0),
                            status_color(p, status),
                        );
                        if !grouped && let Some((folder, _)) = file.relative.rsplit_once('/') {
                            painter.text(
                                pos2(shown.right() + 7.0, y),
                                Align2::LEFT_CENTER,
                                folder,
                                theme::regular(12.5),
                                p.text_secondary,
                            );
                        }
                        if response.clicked() {
                            self.selected_change = Some(*index);
                        }
                        if response.double_clicked() {
                            actions.push(GitAction::Diff { id: details.id.clone(), path: file.path.clone() });
                        }
                    }
                }
            }
        });
        if let Some(key) = toggle
            && !self.collapsed_changes.remove(&key)
        {
            self.collapsed_changes.insert(key);
        }
        let mut child =
            ui.new_child(egui::UiBuilder::new().max_rect(text.shrink2(vec2(12.0, 10.0))).id_salt("git-message"));
        let mono = |size: f32| egui::FontId::new(size, theme::family::editor());
        let mono_bold = |size: f32| egui::FontId::new(size, theme::family::editor_bold());
        let offset = self.local_offset;
        egui::ScrollArea::vertical().id_salt("git-message-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(egui::Label::new(egui::RichText::new(&details.subject).font(mono_bold(13.0)).color(p.text)).wrap());
            if !details.body.is_empty() {
                ui.add_space(12.0);
                ui.add(egui::Label::new(egui::RichText::new(&details.body).font(mono(13.0)).color(p.text)).wrap());
            }
            ui.add_space(14.0);
            let short: String = details.id.chars().take(8).collect();
            let (date, time) = format::date_and_time(details.time + i64::from(offset));
            let line = t!(
                "vcs-log-by",
                id = short.as_str(),
                author = details.author.as_str(),
                email = details.email.as_str(),
                date = date.as_str(),
                time = time.as_str()
            );
            let rest = line.strip_prefix(short.as_str()).map(str::to_owned);
            let mut job = egui::text::LayoutJob::default();
            match rest {
                Some(rest) => {
                    job.append(&short, 0.0, egui::TextFormat::simple(mono(12.5), p.link));
                    job.append(&rest, 0.0, egui::TextFormat::simple(mono(12.5), p.text_secondary));
                }
                None => job.append(&line, 0.0, egui::TextFormat::simple(mono(12.5), p.text_secondary)),
            }
            job.wrap.max_width = ui.available_width();
            ui.add(egui::Label::new(job).wrap());
            let branches: Vec<&str> = details
                .contained_in
                .iter()
                .filter(|label| label.kind != "tag")
                .map(|label| label.name.as_str())
                .collect();
            if !branches.is_empty() {
                ui.add_space(6.0);
                let line = t!("vcs-log-in-branches", count = branches.len(), branches = branches.join(", ").as_str());
                ui.add(egui::Label::new(egui::RichText::new(line).font(mono(12.5)).color(p.text_secondary)).wrap());
            }
        });
    }
}

fn bar_x(full: Rect, from: f32) -> f32 {
    if from >= 0.0 { full.left() + from } else { full.right() + from }
}

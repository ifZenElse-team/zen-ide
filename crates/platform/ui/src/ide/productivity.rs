use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use egui::{Sense, Ui, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::keymap::{GROUPS, group_of};
use zen_i18n::t;

use super::{IdePalette, IdeView, dialog_frame, keymap_page};
use crate::{accessibility, theme};

const FILE: &str = "productivity.json";
const SAVE_EVERY: Duration = Duration::from_secs(30);
const DAY: u64 = 24 * 60 * 60;
const ROW: f32 = 24.0;
const WIDTHS: [f32; 4] = [300.0, 170.0, 70.0, 110.0];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Used {
    times: u64,
    last_used: u64,
}

#[derive(Debug, Default)]
pub(super) struct Usage {
    file: Option<PathBuf>,
    used: BTreeMap<String, Used>,
    changed: bool,
    saved_at: Option<Instant>,
}

pub(super) type SharedUsage = Arc<Mutex<Usage>>;

#[derive(Clone, Debug, Default)]
pub(super) struct Help {
    pub(super) folder: Option<PathBuf>,
    pub(super) usage: SharedUsage,
}

impl Help {
    pub(super) fn new(folder: Option<PathBuf>) -> Self {
        let file = folder.as_ref().map(|folder| folder.join(FILE));
        let used = file
            .as_ref()
            .and_then(|file| std::fs::read_to_string(file).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        let usage = Usage { file, used, ..Usage::default() };
        Self { folder, usage: Arc::new(Mutex::new(usage)) }
    }

    pub(super) fn record(&self, action: &str) {
        lock(&self.usage).record(action, seconds_now(), Instant::now());
    }
}

fn lock(usage: &SharedUsage) -> std::sync::MutexGuard<'_, Usage> {
    usage.lock().unwrap_or_else(PoisonError::into_inner)
}

fn seconds_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

impl Usage {
    fn record(&mut self, action: &str, seconds: u64, now: Instant) {
        let used = self.used.entry(action.to_owned()).or_default();
        used.times += 1;
        used.last_used = seconds;
        self.changed = true;
        if self.saved_at.is_none_or(|saved| now.duration_since(saved) >= SAVE_EVERY) {
            self.save(now);
        }
    }

    fn save(&mut self, now: Instant) {
        let Some(file) = &self.file else { return };
        let Ok(text) = serde_json::to_string(&self.used) else { return };
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = zen_vfs::write_atomic(file, text.as_bytes());
        self.changed = false;
        self.saved_at = Some(now);
    }

    fn reset(&mut self) {
        self.used.clear();
        self.save(Instant::now());
    }
}

impl Drop for Usage {
    fn drop(&mut self) {
        if self.changed {
            self.save(Instant::now());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Feature {
    action: String,
    title: String,
    group: String,
    used: Used,
}

fn features(usage: &SharedUsage) -> Vec<Feature> {
    let usage = lock(usage);
    let mut rows: BTreeMap<String, Feature> = BTreeMap::new();
    for binding in crate::keymap::current().bindings() {
        let group = group_of(&binding.action).min(GROUPS.len() - 1);
        let used = usage.used.get(&binding.action).or_else(|| usage.used.get(&binding.command));
        rows.entry(binding.action.clone()).or_insert_with(|| Feature {
            title: keymap_page::action_title(&binding.action),
            group: t!(GROUPS[group]),
            used: used.copied().unwrap_or_default(),
            action: binding.action,
        });
    }
    rows.into_values().collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Column {
    Feature,
    Group,
    #[default]
    Times,
    LastUsed,
}

impl Column {
    const ALL: [Self; 4] = [Self::Feature, Self::Group, Self::Times, Self::LastUsed];

    fn label(self) -> &'static str {
        match self {
            Self::Feature => "ide-productivity-feature",
            Self::Group => "ide-productivity-group",
            Self::Times => "ide-productivity-times",
            Self::LastUsed => "ide-productivity-last-used",
        }
    }
}

fn sorted(mut rows: Vec<Feature>, column: Column, ascending: bool) -> Vec<Feature> {
    rows.sort_by(|a, b| {
        let order = match column {
            Column::Feature => a.title.cmp(&b.title),
            Column::Group => a.group.cmp(&b.group),
            Column::Times => a.used.times.cmp(&b.used.times),
            Column::LastUsed => a.used.last_used.cmp(&b.used.last_used),
        };
        let order = order.then_with(|| a.title.cmp(&b.title));
        if ascending { order } else { order.reverse() }
    });
    rows
}

fn last_used(used: Used, now: u64) -> String {
    if used.times == 0 {
        return t!("ide-productivity-never");
    }
    t!("ide-productivity-days-ago", days = now.saturating_sub(used.last_used) / DAY)
}

#[derive(Debug, Default)]
pub(super) struct ProductivityDialog {
    column: Column,
    ascending: bool,
    rows: Vec<Feature>,
    now: u64,
}

impl ProductivityDialog {
    fn load(&mut self, usage: &SharedUsage) {
        self.now = seconds_now();
        self.rows = sorted(features(usage), self.column, self.ascending);
    }

    fn sort_by(&mut self, column: Column) {
        if self.column == column {
            self.ascending = !self.ascending;
        } else {
            self.column = column;
            self.ascending = matches!(column, Column::Feature | Column::Group);
        }
        self.rows = sorted(std::mem::take(&mut self.rows), self.column, self.ascending);
    }
}

fn used_rows(rows: &[Feature]) -> Value {
    let list: Vec<Value> = rows
        .iter()
        .filter(|row| row.used.times > 0)
        .map(|row| {
            json!({
                "action": row.action,
                "title": row.title,
                "group": row.group,
                "times": row.used.times,
                "lastUsed": row.used.last_used,
            })
        })
        .collect();
    json!({ "features": rows.len(), "used": list })
}

enum Choice {
    Close,
    Reset,
}

fn show(ui: &mut Ui, p: &IdePalette, dialog: &mut ProductivityDialog) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("my-productivity")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        let width: f32 = WIDTHS.iter().sum();
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 8.0;
        ui.label(egui::RichText::new(t!("keymap-view-productivity-open")).font(theme::semibold(13.0)).color(p.text));
        ui.label(egui::RichText::new(t!("ide-productivity-note")).color(p.text_secondary));
        let (header, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
        let mut x = header.left();
        for (column, column_width) in Column::ALL.into_iter().zip(WIDTHS) {
            let cell = egui::Rect::from_min_size(egui::pos2(x, header.top()), vec2(column_width, ROW));
            let response = ui.interact(cell, ui.scope_id().with(column.label()), Sense::click());
            let arrow = if dialog.column != column {
                ""
            } else if dialog.ascending {
                " ↑"
            } else {
                " ↓"
            };
            let label = format!("{}{arrow}", t!(column.label()));
            ui.painter().text(
                cell.left_center(),
                egui::Align2::LEFT_CENTER,
                &label,
                theme::semibold(12.5),
                p.text_secondary,
            );
            accessibility::button(&response, &t!(column.label()));
            if response.clicked() {
                dialog.sort_by(column);
            }
            x += column_width;
        }
        let rows = dialog.rows.len();
        let now = dialog.now;
        let height = (ui.ctx().content_rect().height() - 260.0).clamp(160.0, 420.0);
        let list = &dialog.rows;
        egui::ScrollArea::vertical().max_height(height).auto_shrink([false, false]).show_rows(
            ui,
            ROW,
            rows,
            |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for row in &list[range] {
                    let (line, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
                    let cells =
                        [row.title.clone(), row.group.clone(), row.used.times.to_string(), last_used(row.used, now)];
                    let mut x = line.left();
                    for (cell, column_width) in cells.iter().zip(WIDTHS) {
                        let area = egui::Rect::from_min_size(egui::pos2(x, line.top()), vec2(column_width - 8.0, ROW));
                        ui.painter().with_clip_rect(area).text(
                            area.left_center(),
                            egui::Align2::LEFT_CENTER,
                            cell,
                            theme::regular(13.0),
                            p.text,
                        );
                        x += column_width;
                    }
                }
            },
        );
        ui.horizontal(|ui| {
            if ui.button(t!("ide-productivity-reset")).clicked() {
                choice = Some(Choice::Reset);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if super::vcs::primary_button(ui, p, &t!("ide-activity-ok")).clicked() {
                    choice = Some(Choice::Close);
                }
            });
        });
    });
    choice.or_else(|| modal.should_close().then_some(Choice::Close))
}

impl IdeView {
    pub(super) fn open_productivity(&mut self) -> Value {
        let mut dialog = ProductivityDialog::default();
        dialog.load(&self.help.usage);
        let answer = used_rows(&dialog.rows);
        self.productivity = Some(dialog);
        answer
    }

    pub(super) fn reset_productivity(&mut self) {
        lock(&self.help.usage).reset();
        if let Some(dialog) = &mut self.productivity {
            dialog.load(&self.help.usage);
        }
    }

    pub(super) fn productivity_dialog(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let Some(dialog) = &mut self.productivity else { return };
        match show(ui, &p, dialog) {
            Some(Choice::Close) => self.productivity = None,
            Some(Choice::Reset) => self.reset_productivity(),
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feature(title: &str, times: u64, last_used: u64) -> Feature {
        Feature {
            action: title.to_owned(),
            title: title.to_owned(),
            group: "Group".to_owned(),
            used: Used { times, last_used },
        }
    }

    fn titles(rows: &[Feature]) -> Vec<&str> {
        rows.iter().map(|row| row.title.as_str()).collect()
    }

    #[test]
    fn the_counts_are_kept_in_the_data_folder_and_survive_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let help = Help::new(Some(dir.path().to_owned()));
        help.record("view.findAction.open");
        help.record("view.findAction.open");
        drop(help);
        let help = Help::new(Some(dir.path().to_owned()));
        let used = lock(&help.usage).used["view.findAction.open"];
        assert_eq!(used.times, 2);
        assert!(used.last_used > 0);
    }

    #[test]
    fn counting_saves_at_most_every_half_minute() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE);
        let help = Help::new(Some(dir.path().to_owned()));
        let start = Instant::now();
        lock(&help.usage).record("one", 1, start);
        let first = std::fs::read_to_string(&file).unwrap();
        lock(&help.usage).record("two", 2, start + Duration::from_secs(1));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), first, "not yet");
        lock(&help.usage).record("three", 3, start + SAVE_EVERY);
        assert!(std::fs::read_to_string(&file).unwrap().contains("two"));
    }

    #[test]
    fn reset_clears_the_counts_on_disk_too() {
        let dir = tempfile::tempdir().unwrap();
        let help = Help::new(Some(dir.path().to_owned()));
        help.record("view.findAction.open");
        lock(&help.usage).reset();
        drop(help);
        assert!(lock(&Help::new(Some(dir.path().to_owned())).usage).used.is_empty());
    }

    #[test]
    fn every_column_sorts_and_a_second_click_reverses() {
        let rows = vec![feature("b", 1, 30), feature("a", 5, 10), feature("c", 0, 0)];
        let mut dialog = ProductivityDialog { rows: sorted(rows, Column::Times, false), ..Default::default() };
        assert_eq!(titles(&dialog.rows), ["a", "b", "c"], "most used first");
        dialog.sort_by(Column::LastUsed);
        assert_eq!(titles(&dialog.rows), ["b", "a", "c"], "latest first");
        dialog.sort_by(Column::LastUsed);
        assert_eq!(titles(&dialog.rows), ["c", "a", "b"]);
        dialog.sort_by(Column::Feature);
        assert_eq!(titles(&dialog.rows), ["a", "b", "c"]);
    }

    #[test]
    fn agents_get_the_used_features_only() {
        let answer = used_rows(&[feature("a", 2, 10), feature("b", 0, 0)]);
        assert_eq!(answer["features"], 2);
        assert_eq!(answer["used"].as_array().unwrap().len(), 1);
        assert_eq!(answer["used"][0]["times"], 2);
    }

    #[test]
    fn a_feature_never_used_says_so() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::with_language("en", || {
            assert_eq!(last_used(Used::default(), DAY), "Never");
            assert_eq!(last_used(Used { times: 1, last_used: DAY }, DAY + 5), "Today");
            assert_eq!(last_used(Used { times: 1, last_used: 0 }, DAY), "Yesterday");
            assert_eq!(last_used(Used { times: 1, last_used: 0 }, 3 * DAY), "3 days ago");
        });
    }
}

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Color32, Painter, Rect, Stroke, pos2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::todo::TodoRange;

use super::IdePalette;

pub(super) const MARK_WIDTH: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TodoMark {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) color: Option<Color32>,
}

#[derive(Default)]
pub(super) struct TodoMarks {
    shown: Option<(PathBuf, u64, Value, Arc<Vec<TodoMark>>)>,
}

fn color_of(text: Option<&str>) -> Option<Color32> {
    let hex = text?.strip_prefix('#').filter(|hex| hex.len() == 6)?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    let [_, red, green, blue] = value.to_be_bytes();
    Some(Color32::from_rgb(red, green, blue))
}

impl TodoMarks {
    pub(super) fn follow(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        version: u64,
        settled: bool,
    ) {
        let settings = session.settings().get::<Value>(zen_workbench::todo_patterns::SETTINGS).unwrap_or_default();
        let same_file = self
            .shown
            .as_ref()
            .is_some_and(|(shown, at, seen, _)| shown == path && *seen == settings && (*at == version || !settled));
        if same_file {
            return;
        }
        let ranges = registry
            .dispatch(session, "todo.document.ranges", json!({ "path": path }))
            .ok()
            .and_then(|answer| serde_json::from_value::<Vec<TodoRange>>(answer).ok())
            .unwrap_or_default();
        let marks = ranges.into_iter().map(|range| TodoMark {
            start: range.start,
            end: range.end,
            color: color_of(range.color.as_deref()),
        });
        self.shown = Some((path.to_owned(), version, settings, Arc::new(marks.collect())));
    }

    pub(super) fn of(&self, path: &Path) -> Arc<Vec<TodoMark>> {
        let shown = self.shown.as_ref().filter(|(shown, ..)| shown == path);
        shown.map(|(_, _, _, marks)| Arc::clone(marks)).unwrap_or_default()
    }
}

pub(super) fn paint_on_scroll_bar(
    painter: &Painter,
    track: Rect,
    lines: impl Iterator<Item = (usize, Option<Color32>)>,
    total: usize,
    palette: &IdePalette,
) {
    let total = total.max(1) as f32;
    for (line, color) in lines {
        let y = track.top() + track.height() * (line as f32 + 0.5) / total;
        let stroke = Stroke::new(2.0, color.unwrap_or(palette.todo));
        painter.line_segment([pos2(track.right() - MARK_WIDTH, y), pos2(track.right(), y)], stroke);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_s_own_colour_is_read_and_a_bad_one_falls_back_to_the_scheme() {
        assert_eq!(color_of(Some("#112233")), Some(Color32::from_rgb(0x11, 0x22, 0x33)));
        assert_eq!(color_of(Some("nope")), None);
        assert_eq!(color_of(None), None);
    }

    #[test]
    fn the_marks_of_a_file_follow_its_text_and_the_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.go");
        std::fs::write(&file, "package a\n\n// TODO one\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        super::super::todo_window::tests::comments_by_slashes(&mut session);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let mut marks = TodoMarks::default();
        marks.follow(&registry, &mut session, &file, 1, true);
        assert_eq!(marks.of(&file).len(), 1);
        let settings = zen_workbench::todo_patterns::TodoSettings { patterns: Vec::new(), ..Default::default() };
        session.settings_mut().set(zen_workbench::todo_patterns::SETTINGS, &settings).unwrap();
        marks.follow(&registry, &mut session, &file, 1, true);
        assert!(marks.of(&file).is_empty(), "a change of the patterns shows at once");
    }
}

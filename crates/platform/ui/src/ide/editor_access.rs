use std::ops::Range;
use std::path::{Path, PathBuf};

use egui::accesskit::{Node, Role, TextDirection, TextPosition, TextSelection};
use egui::{Id, Rect, Sense, Ui};
use zen_i18n::t;

use crate::accessibility;

const MARGIN: usize = 100;

#[derive(Default)]
pub(super) struct EditorAccess {
    key: Option<(PathBuf, u64, Vec<Range<usize>>)>,
    runs: Vec<(usize, Node)>,
}

pub(super) struct Shown<'a> {
    pub(super) path: &'a Path,
    pub(super) version: u64,
    pub(super) lines: usize,
    pub(super) line: &'a dyn Fn(usize) -> &'a str,
    pub(super) view: (usize, usize),
    pub(super) selection: ((usize, usize), (usize, usize)),
    pub(super) bounds: &'a dyn Fn(usize) -> Option<Rect>,
    pub(super) split: Option<(usize, usize)>,
}

impl EditorAccess {
    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.key.is_none() && self.runs.is_empty()
    }

    pub(super) fn publish(&mut self, ui: &mut Ui, id: Id, shown: &Shown<'_>) {
        if !accessibility::active(ui.ctx()) {
            self.key = None;
            self.runs = Vec::new();
            return;
        }
        let caret_line = shown.selection.1.0;
        let ranges = windows(shown.view, caret_line, shown.lines);
        let key = (shown.path.to_owned(), shown.version, ranges);
        if self.key.as_ref() != Some(&key) {
            self.runs = key.2.iter().flat_map(Clone::clone).map(|line| (line, run(shown, line))).collect();
            self.key = Some(key);
        }
        let position = |(line, column): (usize, usize)| {
            let (index, column) = match self.runs.binary_search_by_key(&line, |(run, _)| *run) {
                Ok(index) => (index, column),
                Err(index) if index < self.runs.len() => (index, 0),
                Err(_) => (self.runs.len().checked_sub(1)?, usize::MAX),
            };
            let (line, node) = &self.runs[index];
            let length = node.character_lengths().len();
            let end = if *line + 1 < shown.lines { length.saturating_sub(1) } else { length };
            Some(TextPosition { node: run_id(id, *line).accesskit_id(), character_index: column.min(end) })
        };
        let selection = position(shown.selection.0)
            .zip(position(shown.selection.1))
            .map(|(anchor, focus)| TextSelection { anchor, focus });
        let file = shown.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        accessibility::edit_id(ui.ctx(), id, |node| {
            node.set_role(Role::MultilineTextInput);
            node.set_label(match shown.split {
                Some((split, count)) => t!("a11y-editor-split", file = file, split = split, count = count),
                None => t!("a11y-editor", file = file),
            });
            if let Some(selection) = selection {
                node.set_text_selection(selection);
            }
        });
        let builder = egui::UiBuilder::new().id_salt("a11y-lines").accessibility_parent(id);
        let lines = ui.new_child(builder);
        for (line, node) in &self.runs {
            let bounds = (shown.bounds)(*line);
            let response = lines.interact(bounds.unwrap_or(Rect::NOTHING), run_id(id, *line), Sense::hover());
            accessibility::edit(&response, |built| {
                *built = node.clone();
                if let Some(rect) = bounds {
                    built.set_bounds(egui::accesskit::Rect {
                        x0: rect.min.x.into(),
                        y0: rect.min.y.into(),
                        x1: rect.max.x.into(),
                        y1: rect.max.y.into(),
                    });
                }
            });
        }
    }
}

fn run_id(editor: Id, line: usize) -> Id {
    editor.with(("a11y-line", line))
}

fn run(shown: &Shown<'_>, line: usize) -> Node {
    let mut value = (shown.line)(line).to_owned();
    if line + 1 < shown.lines {
        value.push('\n');
    }
    let lengths: Vec<u8> = value.chars().map(|c| c.len_utf8() as u8).collect();
    let mut node = Node::new(Role::TextRun);
    node.set_text_direction(TextDirection::LeftToRight);
    node.set_value(value);
    node.set_character_lengths(lengths);
    node
}

fn windows((first, last): (usize, usize), caret: usize, lines: usize) -> Vec<Range<usize>> {
    let around = |from: usize, to: usize| from.saturating_sub(MARGIN)..(to + MARGIN + 1).min(lines);
    let view = around(first, last);
    let caret = around(caret, caret);
    if caret.start <= view.end && view.start <= caret.end {
        std::iter::once(view.start.min(caret.start)..view.end.max(caret.end)).collect()
    } else if caret.start < view.start {
        vec![caret, view]
    } else {
        vec![view, caret]
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn the_lines_are_read_again_only_when_the_text_or_the_view_changes() {
        let ctx = egui::Context::default();
        let read = Cell::new(0);
        let text = ["package main", "", "func main() {}"];
        let line = |line: usize| {
            read.set(read.get() + 1);
            text[line]
        };
        let mut access = EditorAccess::default();
        let mut frame = |version: u64, view: (usize, usize)| {
            let shown = Shown {
                path: Path::new("main.go"),
                version,
                lines: text.len(),
                line: &line,
                view,
                selection: ((0, 0), (0, 0)),
                bounds: &|_| None,
                split: None,
            };
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                let id = ui.make_persistent_id("editor");
                let _ = ui.interact(ui.max_rect(), id, Sense::click());
                access.publish(ui, id, &shown);
            });
            output.textures_delta.clear();
        };
        frame(1, (0, 2));
        assert_eq!(read.get(), 0, "nothing is read without a screen reader");
        ctx.enable_accesskit();
        frame(1, (0, 2));
        assert_eq!(read.get(), 3);
        frame(1, (0, 2));
        frame(1, (1, 2));
        assert_eq!(read.get(), 3, "the same text around the same lines is kept");
        frame(2, (0, 2));
        assert_eq!(read.get(), 6, "an edit reads the lines again");
    }

    #[test]
    fn a_selection_reaching_past_the_read_lines_ends_at_the_nearest_one() {
        let text: Vec<String> = (0..1000).map(|line| format!("ліня {line} 字")).collect();
        let line = |line: usize| text[line].as_str();
        let mut access = EditorAccess::default();
        let ctx = egui::Context::default();
        let mut select = |selection| {
            let shown = Shown {
                path: Path::new("main.go"),
                version: 1,
                lines: text.len(),
                line: &line,
                view: (0, 10),
                selection,
                bounds: &|_| None,
                split: None,
            };
            let tree = accessibility::testing::tree_of(&ctx, egui::vec2(400.0, 400.0), |ui| {
                let id = ui.make_persistent_id("editor");
                let _ = ui.interact(ui.max_rect(), id, Sense::click());
                access.publish(ui, id, &shown);
            });
            let node = |id| tree.nodes.iter().find(|(other, _)| *other == id).map(|(_, node)| node.clone()).unwrap();
            let editor = tree.nodes.iter().find(|(_, node)| node.role() == Role::MultilineTextInput).unwrap();
            let selection = editor.1.text_selection().unwrap();
            let at = |position: TextPosition| {
                let run = node(position.node);
                (run.value().unwrap().to_owned(), position.character_index, run.character_lengths().to_vec())
            };
            (at(selection.anchor), at(selection.focus))
        };
        let (anchor, focus) = select(((900, 3), (5, 2)));
        assert_eq!((anchor.0.as_str(), anchor.1), ("ліня 110 字\n", 10), "the end of the last line read");
        assert_eq!((focus.0.as_str(), focus.1), ("ліня 5 字\n", 2));
        assert_eq!(focus.2, vec![2, 2, 2, 2, 1, 1, 1, 3, 1], "characters are counted in UTF-8 bytes each");
        let (anchor, _) = select(((500, 3), (990, 0)));
        assert_eq!((anchor.0.as_str(), anchor.1), ("ліня 890 字\n", 0), "the start of the next line read");
    }

    #[test]
    fn the_lines_read_out_are_those_around_the_view_and_the_caret() {
        assert_eq!(windows((0, 30), 5, 50), vec![0..50]);
        assert_eq!(windows((1000, 1040), 1020, 5000), vec![900..1141]);
        assert_eq!(windows((1000, 1040), 10, 5000), vec![0..111, 900..1141]);
        assert_eq!(windows((0, 40), 3000, 5000), vec![0..141, 2900..3101]);
    }
}

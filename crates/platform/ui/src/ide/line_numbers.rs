use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use zen_core::{Registry, Session};
use zen_text::Point;
use zen_workbench::language::method_separators::{MethodSeparatorsState, moved_offset};
use zen_workbench::language::structure::StructureStatus;
use zen_workbench::line_numbers::LineNumbersMode;
use zen_workbench::{Document, Workbench};

const PENDING_POLL: f64 = 0.3;

#[derive(Clone, Copy, Debug)]
pub(super) struct Numbering<'a> {
    pub(super) mode: LineNumbersMode,
    pub(super) rows: &'a [usize],
    pub(super) caret_row: usize,
}

impl Numbering<'_> {
    pub(super) fn label(&self, line: usize) -> String {
        if self.mode == LineNumbersMode::Absolute {
            return (line + 1).to_string();
        }
        let row = self.rows.partition_point(|&shown| shown <= line).saturating_sub(1);
        self.mode.number(line, row.abs_diff(self.caret_row)).to_string()
    }
}

#[derive(Debug, Default)]
pub(super) struct MethodSeparators {
    read_for: Option<(PathBuf, u64)>,
    starts: Vec<(usize, usize)>,
    starts_for: u64,
    lines_then: usize,
    lines: Arc<Vec<usize>>,
    pending: bool,
    read_at: f64,
    #[cfg(test)]
    reads: usize,
}

impl MethodSeparators {
    #[allow(clippy::too_many_arguments, reason = "what decides whether to read again")]
    pub(super) fn follow(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        version: u64,
        settled: bool,
        enabled: bool,
    ) {
        if !enabled {
            self.read_for = None;
            self.starts.clear();
            self.lines = Arc::default();
            self.pending = false;
            return;
        }
        let now = ctx.input(|input| input.time);
        let same_file = self.read_for.as_ref().is_some_and(|(shown, _)| shown == path);
        let current = same_file && self.read_for.as_ref().is_some_and(|(_, at)| *at == version);
        let due = if !same_file {
            true
        } else if !current {
            settled
        } else {
            self.pending && now - self.read_at >= PENDING_POLL
        };
        if !due {
            if current && self.pending {
                ctx.request_repaint_after(Duration::from_secs_f64(PENDING_POLL));
            }
            if same_file && self.starts_for != version {
                self.move_along(session, version);
            }
            return;
        }
        let state = registry
            .dispatch(session, "editor.methodSeparators.list", json!({ "path": path }))
            .ok()
            .and_then(|answer| serde_json::from_value::<MethodSeparatorsState>(answer).ok());
        #[cfg(test)]
        {
            self.reads += 1;
        }
        self.read_at = now;
        self.read_for = Some((path.to_owned(), version));
        self.starts_for = version;
        self.pending = state.as_ref().is_some_and(|state| state.status == StructureStatus::Pending);
        let lines: Vec<usize> = state
            .map(|state| state.separators.iter().map(|separator| separator.line.saturating_sub(1) as usize).collect())
            .unwrap_or_default();
        let document = session.state::<Workbench>().and_then(Workbench::active_document);
        let text = document.map(Document::snapshot);
        self.lines_then = text.map_or(0, zen_text::Text::len_lines);
        self.starts = text
            .map(|text| {
                lines.iter().filter_map(|&line| Some((text.point_to_offset(Point::new(line, 0)).ok()?, line))).collect()
            })
            .unwrap_or_default();
        self.lines = Arc::new(lines);
        if self.pending {
            ctx.request_repaint_after(Duration::from_secs_f64(PENDING_POLL));
        }
    }

    fn move_along(&mut self, session: &Session, version: u64) {
        let document = session.state::<Workbench>().and_then(Workbench::active_document);
        let edits = document.and_then(|document| Some((document, document.char_edits_since(self.starts_for)?)));
        let Some((document, edits)) = edits else {
            self.starts.clear();
            self.lines = Arc::default();
            self.starts_for = version;
            return;
        };
        let text = document.snapshot();
        let lines_now = text.len_lines();
        let added = lines_now as isize - self.lines_then as isize;
        self.starts = self
            .starts
            .iter()
            .filter_map(|&(start, line)| {
                if let [edit] = edits.as_slice() {
                    if start < edit.range.start {
                        return Some((start, line));
                    }
                    if start > edit.range.end {
                        return Some((start - edit.range.len() + edit.inserted, line.checked_add_signed(added)?));
                    }
                }
                let moved = moved_offset(start, &edits)?;
                let point = text.offset_to_point(moved).ok()?;
                (point.column == 0).then_some((moved, point.line))
            })
            .collect();
        self.starts.dedup();
        self.starts_for = version;
        self.lines_then = lines_now;
        self.lines = Arc::new(self.starts.iter().map(|&(_, line)| line).collect());
    }

    pub(super) fn lines(&self, path: &Path, version: u64) -> Arc<Vec<usize>> {
        match &self.read_for {
            Some((shown, _)) if shown == path && self.starts_for == version => Arc::clone(&self.lines),
            _ => Arc::default(),
        }
    }

    #[cfg(test)]
    pub(super) fn reads(&self) -> usize {
        self.reads
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_numbers_count_the_rows_shown_from_the_caret() {
        let rows = [0, 1, 2, 7, 8];
        let numbering = |mode| Numbering { mode, rows: &rows, caret_row: 1 };
        let labels = |mode| [0, 1, 7, 8].map(|line| numbering(mode).label(line));
        assert_eq!(labels(LineNumbersMode::Absolute), ["1", "2", "8", "9"]);
        assert_eq!(labels(LineNumbersMode::Relative), ["1", "0", "2", "3"], "the folded lines 3 to 6 do not count");
        assert_eq!(labels(LineNumbersMode::Hybrid), ["1", "2", "2", "3"]);
    }
}

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use egui::text::{LayoutJob, TextFormat};
use egui::{FontId, Painter, Pos2, Rect, Sense, Ui};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::glyphs::glyph_run_to_cursor;
use super::{IdePalette, Side, hint};
use crate::accessibility;

const PENDING_POLL: Duration = Duration::from_millis(30);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Shown {
    pub name: String,
    pub value: String,
    pub changed: bool,
}

pub(super) type Lines = Arc<HashMap<usize, Vec<Shown>>>;

#[derive(Debug, Default)]
pub(super) struct InlineValues {
    key: Option<(PathBuf, Value)>,
    lines: Lines,
    pending: bool,
    drawn: Vec<(usize, String, bool)>,
}

impl InlineValues {
    pub(super) fn follow(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        stop: Option<&Value>,
    ) {
        let enabled = session.settings().get_or(zen_workbench::debug::INLINE_VALUES, true);
        let Some(stop) = stop.filter(|_| enabled) else {
            *self = Self { drawn: std::mem::take(&mut self.drawn), ..Self::default() };
            return;
        };
        let key = (path.to_owned(), stop.clone());
        if self.key.as_ref() == Some(&key) && !self.pending {
            return;
        }
        if self.key.as_ref().is_none_or(|(shown, _)| shown != path) {
            self.lines = Lines::default();
        }
        self.key = Some(key);
        match registry.dispatch(session, "debug.inlineValues.get", json!({ "path": path, "waitMs": 0 })) {
            Ok(answer) if answer["pending"] == true => {
                self.pending = true;
                ctx.request_repaint_after(PENDING_POLL);
            }
            Ok(answer) => {
                self.pending = false;
                self.lines = Arc::new(read(&answer));
            }
            Err(_) => {
                self.pending = false;
                self.lines = Lines::default();
            }
        }
    }

    pub(super) fn lines(&mut self) -> Lines {
        self.drawn.clear();
        self.lines.clone()
    }

    pub(super) fn paint(
        &mut self,
        painter: &Painter,
        at: Pos2,
        line: usize,
        values: &[Shown],
        font: &FontId,
        p: &IdePalette,
    ) {
        let format = |color| TextFormat { font_id: font.clone(), color, ..TextFormat::default() };
        let mut job = LayoutJob::default();
        for (index, shown) in values.iter().enumerate() {
            if index > 0 {
                job.append(", ", 0.0, format(p.inlay_hint_foreground));
            }
            let color = if shown.changed { p.vcs_modified } else { p.inlay_hint_foreground };
            job.append(&format!("{} = {}", shown.name, shown.value), 0.0, format(color));
        }
        let changed = values.iter().any(|shown| shown.changed);
        self.drawn.push((line, job.text.clone(), changed));
        painter.galley(at, painter.layout_job(job), p.inlay_hint_foreground);
    }

    #[cfg(test)]
    pub(super) fn drawn(&self) -> &[(usize, String, bool)] {
        &self.drawn
    }
}

fn read(answer: &Value) -> HashMap<usize, Vec<Shown>> {
    answer["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| {
            let at = usize::try_from(line["line"].as_u64()?).ok()?;
            let values = line["values"]
                .as_array()?
                .iter()
                .map(|value| Shown {
                    name: value["name"].as_str().unwrap_or_default().to_owned(),
                    value: value["value"].as_str().unwrap_or_default().to_owned(),
                    changed: value["changed"] == true,
                })
                .collect();
            Some((at, values))
        })
        .collect()
}

pub(super) fn run_to_cursor_button(ui: &Ui, rect: Rect, p: &IdePalette) -> bool {
    let response = ui.interact(rect, ui.scope_id().with("run-to-cursor-inlay"), Sense::click());
    let name = t!("ide-debug-run-to-cursor");
    accessibility::button_with(&response, || name.clone());
    let fill = if response.hovered() { p.popup_hover } else { p.popup };
    ui.painter().rect_filled(rect, 3, fill);
    ui.painter().rect_stroke(rect, 3, egui::Stroke::new(1.0, p.popup_border), egui::StrokeKind::Inside);
    glyph_run_to_cursor(ui.painter(), rect.shrink(3.0), p.run);
    let shortcut = crate::keymap::shortcut("debug.execution.runToCursor");
    hint(ui, &response, &name, shortcut.as_deref(), Side::Below);
    response.clicked()
}

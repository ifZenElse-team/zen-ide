use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::ecolor::Hsva;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_workbench::plugins::colors::ColorList;

use super::IdePalette;

const SIDE: f32 = 10.0;

type Shown = ((PathBuf, u64), Arc<Vec<Swatch>>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Swatch {
    pub(super) line: usize,
    pub(super) start: usize,
    pub(super) rgba: [u8; 4],
}

struct Picker {
    path: PathBuf,
    start: usize,
    at: Pos2,
    original: [u8; 4],
    color: Hsva,
    fresh: bool,
    closing: Option<bool>,
}

#[derive(Default)]
pub(super) struct ColorSwatches {
    shown: Option<Shown>,
    picker: Option<Picker>,
    pub(super) drawn: Vec<(usize, Rect)>,
}

fn rgba_of(color: &str) -> Option<[u8; 4]> {
    let hex = color.strip_prefix('#').filter(|hex| hex.len() == 8)?;
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(value.to_be_bytes())
}

impl ColorSwatches {
    pub(super) fn follow(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        version: u64,
        settled: bool,
    ) {
        if self.shown.as_ref().is_some_and(|((shown, at), _)| shown == path && (*at == version || !settled)) {
            return;
        }
        let listed = registry
            .dispatch(session, "editor.colors.list", json!({ "path": path }))
            .ok()
            .and_then(|answer| serde_json::from_value::<ColorList>(answer).ok())
            .unwrap_or_default();
        let mut swatches: Vec<Swatch> = Vec::new();
        for literal in listed.colors {
            if swatches.last().is_some_and(|last| last.line == literal.line) {
                continue;
            }
            if let Some(rgba) = rgba_of(&literal.color) {
                swatches.push(Swatch { line: literal.line, start: literal.start, rgba });
            }
        }
        if self.picker.as_ref().is_some_and(|picker| picker.path != path) {
            self.picker = None;
        }
        self.shown = Some(((path.to_owned(), version), Arc::new(swatches)));
    }

    pub(super) fn on_line(&self, path: &Path, line: usize) -> Option<Swatch> {
        let (_, swatches) = self.shown.as_ref().filter(|((shown, _), _)| shown == path)?;
        let at = swatches.binary_search_by_key(&line, |swatch| swatch.line).ok()?;
        swatches.get(at).cloned()
    }

    pub(super) fn begin(&mut self) {
        self.drawn.clear();
    }

    pub(super) fn paint(
        &mut self,
        ui: &Ui,
        painter: &egui::Painter,
        id: egui::Id,
        (path, swatch): (&Path, &Swatch),
        rect: Rect,
        palette: &IdePalette,
    ) -> bool {
        let square = Rect::from_center_size(rect.center(), Vec2::splat(SIDE));
        let response = ui.interact(rect, id.with(("color-swatch", swatch.line)), Sense::click());
        if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let shown = match self.picker.as_ref().filter(|picker| picker.path == path && picker.start == swatch.start) {
            Some(picker) => picker.color.to_srgba_unmultiplied(),
            None => swatch.rgba,
        };
        let [r, g, b, a] = shown;
        if a < 255 {
            let half = Rect::from_min_max(square.center_top(), square.right_bottom());
            painter.rect_filled(square, 2, palette.text);
            painter.rect_filled(half, 0, palette.text_secondary);
        }
        painter.rect_filled(square, 2, Color32::from_rgba_unmultiplied(r, g, b, a));
        painter.rect_stroke(square, 2, Stroke::new(1.0, palette.gutter_text), StrokeKind::Inside);
        self.drawn.push((swatch.line, square));
        if response.clicked() {
            self.picker = Some(Picker {
                path: path.to_owned(),
                start: swatch.start,
                at: rect.left_bottom() + vec2(0.0, 2.0),
                original: swatch.rgba,
                color: Hsva::from_srgba_unmultiplied(swatch.rgba),
                fresh: true,
                closing: None,
            });
        }
        response.clicked()
    }

    pub(super) fn keys(&mut self, ui: &Ui) {
        let Some(picker) = &mut self.picker else { return };
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            picker.closing = Some(false);
        } else if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            picker.closing = Some(true);
        }
    }

    pub(super) fn picker(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, palette: &IdePalette) {
        let Some(picker) = &mut self.picker else { return };
        let fresh = std::mem::take(&mut picker.fresh);
        let area = egui::Area::new(ui.scope_id().with("color-picker"))
            .order(egui::Order::Foreground)
            .fixed_pos(picker.at)
            .show(ui.ctx(), |ui| {
                super::doc_popups::popup_frame(palette).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    egui::widgets::color_picker::color_picker_hsva_2d(
                        ui,
                        &mut picker.color,
                        egui::widgets::color_picker::Alpha::OnlyBlend,
                    );
                });
            });
        match picker.closing {
            Some(false) => {
                self.picker = None;
                return;
            }
            Some(true) => {}
            None if !fresh && area.response.clicked_elsewhere() => {}
            None => return,
        }
        let Some(picker) = self.picker.take() else { return };
        let chosen = picker.color.to_srgba_unmultiplied();
        if chosen != picker.original {
            let [r, g, b, a] = chosen;
            let color = format!("#{r:02x}{g:02x}{b:02x}{a:02x}");
            let _ = registry.dispatch(
                session,
                "editor.colors.set",
                json!({ "path": picker.path, "offset": picker.start, "color": color }),
            );
        }
    }

    #[cfg(test)]
    pub(super) fn pick(&mut self, rgba: [u8; 4]) {
        if let Some(picker) = &mut self.picker {
            picker.color = Hsva::from_srgba_unmultiplied(rgba);
        }
    }
}

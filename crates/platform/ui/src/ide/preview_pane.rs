use std::path::Path;
use std::sync::Arc;

use egui::{Align2, Color32, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_syntax::Highlighter;
use zen_workbench::Workbench;
use zen_workbench::preview::{PreviewKind, PreviewMode, PreviewState, Previews, SplitOrientation};

use super::glyphs::{Glyph, stroke};
use super::image_view::{MARGIN, TOOLBAR, zoom_buttons, zoom_shortcut};
use super::{IdePalette, IdeView, Side, hint};
use crate::theme;

const DIVIDER: f32 = 5.0;
const BUTTON: f32 = 22.0;
const GROUP_GAP: f32 = 6.0;

const MODES: [(PreviewMode, Glyph, &str, &str); 3] = [
    (PreviewMode::EditorOnly, glyph_editor_only, "ide-preview-editor-only", "editor.preview.editorOnly"),
    (
        PreviewMode::EditorAndPreview,
        glyph_editor_and_preview,
        "ide-preview-editor-and-preview",
        "editor.preview.editorAndPreview",
    ),
    (PreviewMode::PreviewOnly, glyph_preview_only, "ide-preview-preview-only", "editor.preview.previewOnly"),
];

impl IdeView {
    #[allow(clippy::too_many_arguments, reason = "the editor's own drawing needs all of them")]
    pub(super) fn text_with_preview(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        highlighter: Option<Arc<dyn Highlighter>>,
        palette: &IdePalette,
        area: Rect,
        (path, kind): (&Path, PreviewKind),
        focused: bool,
    ) -> Option<(usize, usize)> {
        let (mode, orientation, split) = {
            let previews = session.state::<Previews>();
            (
                previews.map_or(kind.default_mode(), |previews| previews.mode_of(path, kind)),
                previews.map_or_else(SplitOrientation::default, |previews| previews.orientation_of(path)),
                previews.map_or(0.5, Previews::split),
            )
        };
        let (editor_rect, preview_rect) = match mode {
            PreviewMode::EditorOnly => (Some(area), None),
            PreviewMode::PreviewOnly => (None, Some(area)),
            PreviewMode::EditorAndPreview => {
                let (editor, preview, divider) = split_rects(area, orientation, split);
                self.preview_divider(ui, session, area, orientation, divider);
                (Some(editor), Some(preview))
            }
        };
        if editor_rect.is_some()
            && let Some(line) = self.markdown_views.take_editor_line(path)
        {
            self.editor_view.scroll_to_line(line);
        }
        let caret = editor_rect.and_then(|rect| {
            let mut editor_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            self.editor_with_breadcrumbs(&mut editor_ui, registry, session, highlighter, palette)
        });
        if let Some(rect) = preview_rect {
            let mut preview_ui = ui.new_child(egui::UiBuilder::new().max_rect(rect));
            let keyboard = focused && mode == PreviewMode::PreviewOnly;
            self.preview_pane(&mut preview_ui, registry, session, (path, kind), keyboard);
        }
        ui.advance_cursor_after_rect(area);
        self.preview_switch(ui, registry, session, area, mode, orientation);
        caret
    }

    pub(super) fn follow_markdown_layout(&mut self, session: &mut Session) {
        if let Some(layout) = session.state_mut::<Previews>().take_markdown_layout_set() {
            let mut settings = self.overview.settings.clone();
            settings.markdown.preview_layout = layout;
            self.overview.settings = settings.clone();
            self.requests.push(super::Request::Settings(Box::new(settings)));
        }
    }

    fn preview_divider(
        &mut self,
        ui: &mut Ui,
        session: &mut Session,
        area: Rect,
        orientation: SplitOrientation,
        rect: Rect,
    ) {
        let p = self.palette;
        let response = ui.interact(rect, ui.scope_id().with("preview-divider"), Sense::drag());
        let line = Stroke::new(1.0, p.tab_border);
        let vertical = orientation == SplitOrientation::Vertical;
        if vertical {
            ui.painter().vline(rect.center().x, area.y_range(), line);
        } else {
            ui.painter().hline(area.x_range(), rect.center().y, line);
        }
        if response.hovered() || response.dragged() {
            let icon = if vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical };
            ui.ctx().set_cursor_icon(icon);
        }
        if response.dragged()
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let share = if vertical {
                (pointer.x - area.left()) / area.width()
            } else {
                (pointer.y - area.top()) / area.height()
            };
            session.state_mut::<Previews>().set_split(share);
        }
    }

    fn preview_switch(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        area: Rect,
        mode: PreviewMode,
        orientation: SplitOrientation,
    ) {
        let p = self.palette;
        let top = area.top() + 6.0;
        let right = area.right() - 18.0;
        let mut clicked = None;
        if mode == PreviewMode::EditorAndPreview {
            let left = right - (MODES.len() + 1) as f32 * (BUTTON + 2.0) - GROUP_GAP;
            let rect = Rect::from_min_size(pos2(left, top), Vec2::splat(BUTTON));
            let response = ui.interact(rect, ui.scope_id().with("preview-orientation"), Sense::click());
            if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            let glyph: Glyph = match orientation {
                SplitOrientation::Vertical => glyph_split_horizontally,
                SplitOrientation::Horizontal => glyph_split_vertically,
            };
            glyph(ui.painter(), rect.shrink(5.0), p.text_secondary);
            let name = t!("ide-preview-change-orientation");
            hint(ui, &response, &name, None, Side::Below);
            crate::accessibility::name(&response, egui::Role::Button, &name);
            if response.clicked() {
                clicked = Some("editor.preview.toggleOrientation");
            }
        }
        for (index, (shown, glyph, name, command)) in MODES.into_iter().enumerate() {
            let left = right - (MODES.len() - index) as f32 * (BUTTON + 2.0);
            let rect = Rect::from_min_size(pos2(left, top), Vec2::splat(BUTTON));
            let response = ui.interact(rect, ui.scope_id().with(("preview-mode", index)), Sense::click());
            let pressed = shown == mode;
            if pressed {
                ui.painter().rect_filled(rect, 4, p.selection);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.hover);
            }
            glyph(ui.painter(), rect.shrink(5.0), if pressed { p.text } else { p.text_secondary });
            hint(ui, &response, &t!(name), None, Side::Below);
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, pressed, t!(name)));
            if response.clicked() {
                clicked = Some(command);
            }
        }
        if let Some(command) = clicked {
            self.dispatch(ui, registry, session, command, serde_json::json!({}));
        }
    }

    fn preview_pane(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        (path, kind): (&Path, PreviewKind),
        keyboard: bool,
    ) {
        let p = self.palette;
        let Some(version) = session.state::<Workbench>().and_then(|workbench| {
            workbench.documents().iter().find(|document| document.path() == path).map(|document| document.version())
        }) else {
            return;
        };
        let wanted = session.state::<Previews>().is_none_or(|previews| previews.wants(path, version));
        let text = wanted
            .then(|| {
                session
                    .state::<Workbench>()
                    .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
                    .map(|document| document.text().to_owned())
            })
            .flatten()
            .unwrap_or_default();
        let open: Vec<std::path::PathBuf> = session
            .state::<Workbench>()
            .map(|workbench| workbench.documents().iter().map(|document| document.path().to_owned()).collect())
            .unwrap_or_default();
        let previews = session.state_mut::<Previews>();
        previews.retain(&open);
        let state = previews.poll(kind, path, version, || text);
        if session.state::<Previews>().is_some_and(|previews| previews.rendering(path)) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }
        let pane = ui.available_rect_before_wrap();
        ui.painter().rect_filled(pane, 0, p.editor_background);
        if let PreviewState::Ready { content: zen_workbench::preview::Content::Markdown(rendered), .. } = &state {
            let rendered = Arc::clone(rendered);
            self.markdown_pane(ui, registry, session, path, &rendered);
            return;
        }
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOOLBAR), Sense::hover());
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
        let mut action = zoom_buttons(ui, &p, bar);
        if keyboard {
            action = action.or_else(|| zoom_shortcut(ui));
        }
        let canvas = ui.available_rect_before_wrap();
        let note = |ui: &mut Ui, text: String| {
            let wrap = (canvas.width() - 32.0).max(40.0);
            let galley = ui.painter().layout(text, theme::regular(13.0), p.text_secondary, wrap);
            let at = canvas.center() - galley.size() / 2.0;
            ui.painter().with_clip_rect(canvas).galley(at, galley, p.text_secondary);
        };
        let picture = match state {
            PreviewState::Rendering => {
                ui.put(
                    Rect::from_center_size(canvas.center(), Vec2::splat(28.0)),
                    egui::Spinner::new().size(28.0).color(p.text_secondary),
                );
                return;
            }
            PreviewState::Invalid { error, .. } => {
                let first = error.lines().next().unwrap_or_default().chars().take(160).collect::<String>();
                note(ui, t!("ide-preview-invalid", error = first));
                return;
            }
            PreviewState::Ready { content, .. } => match content.picture() {
                Some(picture) => Arc::clone(picture),
                None => return,
            },
        };
        let natural = Vec2::from(picture.natural_size());
        let fit = ((canvas.width() - 2.0 * MARGIN) / natural.x).min((canvas.height() - 2.0 * MARGIN) / natural.y);
        session.state_mut::<Previews>().set_fit_scale(path, fit);
        let scale = session.state::<Previews>().map_or(1.0, |previews| previews.scale(path));
        ui.painter().text(
            pos2(bar.left() + 140.0, bar.center().y),
            Align2::LEFT_CENTER,
            format!("{:.0}%", scale * 100.0),
            theme::regular(12.5),
            p.text,
        );
        let hovered = self.picture_canvas(ui, &picture, scale, ("preview", path), keyboard);
        let zoom = ui.input(|input| input.zoom_delta());
        if hovered && (zoom - 1.0).abs() > f32::EPSILON {
            self.dispatch(ui, registry, session, "editor.image.zoom", serde_json::json!({ "scale": scale * zoom }));
        } else if let Some(action) = action {
            self.dispatch(ui, registry, session, "editor.image.zoom", serde_json::json!({ "action": action }));
        }
    }
}

fn split_rects(area: Rect, orientation: SplitOrientation, split: f32) -> (Rect, Rect, Rect) {
    match orientation {
        SplitOrientation::Vertical => {
            let middle = area.left() + (area.width() * split).round();
            let (before, after) = (middle - DIVIDER / 2.0, middle + DIVIDER / 2.0);
            (
                Rect::from_min_max(area.min, pos2(before, area.bottom())),
                Rect::from_min_max(pos2(after, area.top()), area.max),
                Rect::from_min_max(pos2(before, area.top()), pos2(after, area.bottom())),
            )
        }
        SplitOrientation::Horizontal => {
            let middle = area.top() + (area.height() * split).round();
            let (before, after) = (middle - DIVIDER / 2.0, middle + DIVIDER / 2.0);
            (
                Rect::from_min_max(area.min, pos2(area.right(), before)),
                Rect::from_min_max(pos2(area.left(), after), area.max),
                Rect::from_min_max(pos2(area.left(), before), pos2(area.right(), after)),
            )
        }
    }
}

fn glyph_split_vertically(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.rect_stroke(rect, 1, s, egui::StrokeKind::Inside);
    painter.vline(rect.center().x, rect.y_range(), s);
}

fn glyph_split_horizontally(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.rect_stroke(rect, 1, s, egui::StrokeKind::Inside);
    painter.hline(rect.x_range(), rect.center().y, s);
}

fn glyph_editor_only(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    for (index, width) in [1.0, 0.7, 0.9, 0.55].into_iter().enumerate() {
        let y = rect.top() + rect.height() * (0.15 + index as f32 * 0.23);
        painter.line_segment([pos2(rect.left(), y), pos2(rect.left() + rect.width() * width, y)], s);
    }
}

fn glyph_editor_and_preview(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.rect_stroke(rect, 1, s, egui::StrokeKind::Inside);
    painter.vline(rect.center().x, rect.y_range(), s);
    for index in 0..3 {
        let y = rect.top() + rect.height() * (0.3 + index as f32 * 0.2);
        painter.line_segment(
            [pos2(rect.left() + rect.width() * 0.15, y), pos2(rect.center().x - rect.width() * 0.1, y)],
            s,
        );
    }
}

fn glyph_preview_only(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.rect_stroke(rect, 1, s, egui::StrokeKind::Inside);
    let base = rect.bottom() - rect.height() * 0.2;
    painter.line_segment(
        [
            pos2(rect.left() + rect.width() * 0.12, base),
            pos2(rect.left() + rect.width() * 0.42, rect.top() + rect.height() * 0.45),
        ],
        s,
    );
    painter.line_segment(
        [
            pos2(rect.left() + rect.width() * 0.42, rect.top() + rect.height() * 0.45),
            pos2(rect.right() - rect.width() * 0.12, base),
        ],
        s,
    );
    painter.circle_stroke(
        pos2(rect.right() - rect.width() * 0.3, rect.top() + rect.height() * 0.3),
        rect.width() * 0.1,
        s,
    );
}

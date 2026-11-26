use std::sync::Arc;

use egui::{Align2, Color32, Key, KeyboardShortcut, Modifiers, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::Workbench;

use super::glyphs::{Glyph, stroke};
use super::{IdePalette, IdeView, Side, hint};
use crate::theme;

pub(super) struct ImageTexture {
    picture: Arc<zen_image::Picture>,
    texture: egui::TextureHandle,
    chessboard: egui::TextureHandle,
}

pub(super) const MARGIN: f32 = 24.0;
pub(super) const TOOLBAR: f32 = 34.0;
const SQUARE: f32 = 8.0;

impl IdeView {
    pub(super) fn image_viewer(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session, focused: bool) {
        let p = self.palette;
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return };
        let Some(image) = document.image() else { return };
        let path = document.path().to_owned();
        let Some(picture) = image.picture().cloned() else {
            let error = image.error().map(str::to_owned);
            let area = ui.available_rect_before_wrap();
            match error {
                Some(error) => {
                    ui.painter().text(
                        area.center(),
                        Align2::CENTER_CENTER,
                        t!("ide-image-failed", error = error),
                        theme::regular(13.0),
                        p.text_secondary,
                    );
                }
                None => {
                    ui.put(
                        Rect::from_center_size(area.center(), Vec2::splat(28.0)),
                        egui::Spinner::new().size(28.0).color(p.text_secondary),
                    );
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
                }
            }
            return;
        };
        let natural = Vec2::from(picture.natural_size());
        let mut canvas = ui.available_rect_before_wrap();
        canvas.min.y += TOOLBAR + ui.spacing().item_spacing.y;
        let fit = ((canvas.width() - 2.0 * MARGIN) / natural.x).min((canvas.height() - 2.0 * MARGIN) / natural.y);
        let Some(image) = session.state_mut::<Workbench>().active_document_mut().and_then(|d| d.image_mut()) else {
            return;
        };
        image.set_fit_scale(fit);
        let (file_size, scale) = (image.file_size(), image.scale());

        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), TOOLBAR), Sense::hover());
        ui.painter().hline(bar.x_range(), bar.bottom() - 0.5, Stroke::new(1.0, p.tab_border));
        let mut action = zoom_buttons(ui, &p, bar);
        let size_and_format = t!(
            "ide-image-info",
            width = format!("{}", natural.x.round()),
            height = format!("{}", natural.y.round()),
            format = picture.format
        );
        let bits = picture.bits_per_pixel.map(|bits| t!("ide-image-bits", bits = bits));
        let info = [Some(size_and_format), bits, Some(file_size_text(file_size))]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("  ");
        let right = ui.painter().text(
            pos2(bar.right() - 12.0, bar.center().y),
            Align2::RIGHT_CENTER,
            info,
            theme::regular(12.5),
            p.text_secondary,
        );
        ui.painter().text(
            pos2(right.left() - 16.0, bar.center().y),
            Align2::RIGHT_CENTER,
            format!("{:.0}%", scale * 100.0),
            theme::regular(12.5),
            p.text,
        );

        if focused {
            action = action.or_else(|| zoom_shortcut(ui));
        }

        let hovered = self.picture_canvas(ui, &picture, scale, ("image", path), focused);

        let zoom = ui.input(|input| input.zoom_delta());
        let mut new_scale = None;
        if hovered && (zoom - 1.0).abs() > f32::EPSILON {
            new_scale = Some(scale * zoom);
        }
        if let Some(scale) = new_scale {
            self.dispatch(ui, registry, session, "editor.image.zoom", serde_json::json!({ "scale": scale }));
        } else if let Some(action) = action {
            self.dispatch(ui, registry, session, "editor.image.zoom", serde_json::json!({ "action": action }));
        }
    }

    pub(super) fn picture_canvas(
        &mut self,
        ui: &mut Ui,
        picture: &Arc<zen_image::Picture>,
        scale: f32,
        id: impl std::hash::Hash + std::fmt::Debug,
        focused: bool,
    ) -> bool {
        let p = self.palette;
        let natural = Vec2::from(picture.natural_size());
        let canvas = ui.available_rect_before_wrap();
        if focused || ui.rect_contains_pointer(canvas) {
            self.free_scroll.push(canvas);
        }
        let textures = self.image_texture(ui, picture, &p);
        let size = natural * scale;
        let hovered = ui.rect_contains_pointer(canvas);
        egui::ScrollArea::both().id_salt(id).auto_shrink([false, false]).show(ui, |ui| {
            let room = ui.available_size();
            let content = vec2((size.x + 2.0 * MARGIN).max(room.x), (size.y + 2.0 * MARGIN).max(room.y));
            let (area, response) = ui.allocate_exact_size(content, Sense::drag());
            let movable = size.x + 2.0 * MARGIN > room.x + 0.5 || size.y + 2.0 * MARGIN > room.y + 0.5;
            if movable && response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            } else if movable && response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            let picture_rect = Rect::from_center_size(area.center(), size);
            let painter = ui.painter();
            let board_uv = Rect::from_min_size(egui::Pos2::ZERO, size / (2.0 * SQUARE));
            painter.image(textures.chessboard.id(), picture_rect, board_uv, Color32::WHITE);
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.image(textures.texture.id(), picture_rect, uv, Color32::WHITE);
            painter.rect_stroke(picture_rect.expand(0.5), 0, Stroke::new(1.0, p.tab_border), egui::StrokeKind::Outside);
            if movable && response.dragged() {
                ui.scroll_with_delta_animation(response.drag_delta(), egui::style::ScrollAnimation::none());
            }
        });
        hovered
    }

    fn image_texture(&mut self, ui: &Ui, picture: &Arc<zen_image::Picture>, p: &IdePalette) -> &ImageTexture {
        if let Some(at) = self.image_textures.iter().position(|cached| Arc::ptr_eq(&cached.picture, picture)) {
            let cached = self.image_textures.remove(at);
            self.image_textures.insert(0, cached);
        } else {
            let max_side = ui.ctx().input(|input| input.max_texture_side);
            let shrunk;
            let shown = if picture.width.max(picture.height) > max_side {
                shrunk = picture.shrunk_to(max_side);
                &shrunk
            } else {
                picture.as_ref()
            };
            let pixels = egui::ColorImage::from_rgba_unmultiplied([shown.width, shown.height], &shown.rgba);
            let magnification =
                if picture.vector_size.is_some() { egui::TextureFilter::Linear } else { egui::TextureFilter::Nearest };
            let options = egui::TextureOptions {
                magnification,
                minification: egui::TextureFilter::Linear,
                wrap_mode: egui::TextureWrapMode::ClampToEdge,
                mipmap_mode: Some(egui::TextureFilter::Linear),
            };
            let texture = ui.ctx().load_texture("image-viewer", pixels, options);
            let (light, dark) = (p.checker_light, p.checker_dark);
            let board = egui::ColorImage::new([2, 2], vec![light, dark, dark, light]);
            let board_options = egui::TextureOptions {
                magnification: egui::TextureFilter::Nearest,
                minification: egui::TextureFilter::Nearest,
                wrap_mode: egui::TextureWrapMode::Repeat,
                mipmap_mode: None,
            };
            let chessboard = ui.ctx().load_texture("image-viewer-chessboard", board, board_options);
            self.image_textures.insert(0, ImageTexture { picture: Arc::clone(picture), texture, chessboard });
            self.image_textures.truncate(self.groups_ui.image_slots());
        }
        &self.image_textures[0]
    }
}

pub(super) fn zoom_buttons(ui: &mut Ui, p: &IdePalette, bar: Rect) -> Option<&'static str> {
    let buttons: [(Glyph, &str, &str, &'static str); 4] = [
        (glyph_zoom_in, "ide-image-zoom-in", "⌘=", "in"),
        (glyph_zoom_out, "ide-image-zoom-out", "⌘−", "out"),
        (glyph_actual_size, "ide-image-actual-size", "⌘/", "actual"),
        (glyph_fit, "ide-image-fit", "", "fit"),
    ];
    let mut action = None;
    for (index, (glyph, name, shortcut, id)) in buttons.into_iter().enumerate() {
        let rect =
            Rect::from_center_size(pos2(bar.left() + 22.0 + index as f32 * 30.0, bar.center().y), Vec2::splat(26.0));
        let response = ui.interact(rect, ui.scope_id().with(("image-tool", index)), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 5, p.hover);
        }
        glyph(ui.painter(), rect.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!(name), (!shortcut.is_empty()).then_some(shortcut), Side::Below);
        if response.clicked() {
            action = Some(id);
        }
    }
    action
}

pub(super) fn zoom_shortcut(ui: &mut Ui) -> Option<&'static str> {
    let shortcut = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
    ui.input_mut(|input| {
        if input.consume_shortcut(&shortcut(Key::Equals)) || input.consume_shortcut(&shortcut(Key::Plus)) {
            Some("in")
        } else if input.consume_shortcut(&shortcut(Key::Minus)) {
            Some("out")
        } else if input.consume_shortcut(&shortcut(Key::Slash)) {
            Some("actual")
        } else {
            None
        }
    })
}

pub(super) fn file_size_text(bytes: u64) -> String {
    let (message, number) = file_size_parts(bytes);
    t!(message, size = number)
}

fn file_size_parts(bytes: u64) -> (&'static str, String) {
    let bytes = bytes as f64;
    if bytes < 1024.0 {
        ("ide-size-bytes", format!("{bytes}"))
    } else if bytes < 1024.0 * 1024.0 {
        ("ide-size-kb", format!("{:.1}", bytes / 1024.0))
    } else {
        ("ide-size-mb", format!("{:.1}", bytes / 1024.0 / 1024.0))
    }
}

fn glyph_magnifier(painter: &egui::Painter, rect: Rect, color: Color32) -> (egui::Pos2, f32) {
    let s = stroke(rect, color);
    let center = rect.center() - vec2(rect.width() * 0.1, rect.height() * 0.1);
    let radius = rect.width() * 0.36;
    painter.circle_stroke(center, radius, s);
    painter.line_segment([center + vec2(radius * 0.72, radius * 0.72), rect.right_bottom()], s);
    (center, radius)
}

fn glyph_zoom_in(painter: &egui::Painter, rect: Rect, color: Color32) {
    let (center, radius) = glyph_magnifier(painter, rect, color);
    let s = stroke(rect, color);
    let arm = radius * 0.5;
    painter.line_segment([center - vec2(arm, 0.0), center + vec2(arm, 0.0)], s);
    painter.line_segment([center - vec2(0.0, arm), center + vec2(0.0, arm)], s);
}

fn glyph_zoom_out(painter: &egui::Painter, rect: Rect, color: Color32) {
    let (center, radius) = glyph_magnifier(painter, rect, color);
    let arm = radius * 0.5;
    painter.line_segment([center - vec2(arm, 0.0), center + vec2(arm, 0.0)], stroke(rect, color));
}

fn glyph_actual_size(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.text(rect.center(), Align2::CENTER_CENTER, "1:1", theme::semibold(rect.height() * 0.62), color);
}

pub(super) fn glyph_fit(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.06);
    let arm = r.width() * 0.3;
    for (corner, dx, dy) in [
        (r.left_top(), 1.0, 1.0),
        (r.right_top(), -1.0, 1.0),
        (r.left_bottom(), 1.0, -1.0),
        (r.right_bottom(), -1.0, -1.0),
    ] {
        painter.line_segment([corner, corner + vec2(arm * dx, 0.0)], s);
        painter.line_segment([corner, corner + vec2(0.0, arm * dy)], s);
    }
    let inner = r.shrink(r.width() * 0.3);
    painter.rect_stroke(inner, 1, s, egui::StrokeKind::Middle);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_sizes() {
        assert_eq!(file_size_parts(512), ("ide-size-bytes", "512".to_owned()));
        assert_eq!(file_size_parts(12_800), ("ide-size-kb", "12.5".to_owned()));
        assert_eq!(file_size_parts(3 * 1024 * 1024), ("ide-size-mb", "3.0".to_owned()));
    }
}

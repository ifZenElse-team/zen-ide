use egui::{Color32, Mesh, Pos2, Rect, Shape, Ui, Vec2, pos2, vec2};
use zen_mascot::{FRAME_MS, Grid, SIZE, STILL, State, Variant};

pub(crate) fn paint(painter: &egui::Painter, origin: Pos2, pixel: f32, variant: Variant, grid: &Grid) {
    let mut mesh = Mesh::default();
    for (row, inks) in grid.iter().enumerate() {
        let mut column = 0;
        while column < SIZE {
            let ink = inks[column];
            let start = column;
            while column < SIZE && inks[column] == ink {
                column += 1;
            }
            if let Some([r, g, b]) = ink.rgb(variant) {
                let min = origin + vec2(start as f32 * pixel, row as f32 * pixel);
                let size = vec2((column - start) as f32 * pixel, pixel);
                mesh.add_colored_rect(Rect::from_min_size(min, size), Color32::from_rgb(r, g, b));
            }
        }
    }
    painter.add(Shape::mesh(mesh));
}

pub(crate) fn pixel_size(room: f32, largest: f32, pixels_per_point: f32) -> Option<f32> {
    let physical = (room.min(largest * SIZE as f32) * pixels_per_point / SIZE as f32).floor();
    (physical >= 1.0).then(|| physical / pixels_per_point)
}

pub(crate) fn frame_now(time: f64, moving: bool) -> (usize, Option<std::time::Duration>) {
    if !moving {
        return (STILL, None);
    }
    let ms = (time.max(0.0) * 1000.0) as u64;
    let left = FRAME_MS - ms % FRAME_MS;
    (zen_mascot::frame_at(ms), Some(std::time::Duration::from_millis(left.max(16))))
}

pub(crate) fn show(ui: &Ui, rect: Rect, id: &str, state: State, largest: f32) -> Option<Rect> {
    let ppp = ui.ctx().pixels_per_point();
    let pixel = pixel_size(rect.width().min(rect.height()), largest, ppp)?;
    let side = pixel * SIZE as f32;
    let corner = rect.center() - Vec2::splat(side / 2.0);
    let origin = pos2((corner.x * ppp).round() / ppp, (corner.y * ppp).round() / ppp);
    let visible = ui.input(|input| {
        let viewport = input.viewport();
        viewport.minimized != Some(true) && viewport.occluded != Some(true)
    });
    let moving = visible && !crate::accessibility::reduce_motion();
    let (index, next) = frame_now(ui.input(|input| input.time), moving);
    let variant = Variant::of(id);
    paint(ui.painter(), origin, pixel, variant, &zen_mascot::frame(variant, state, index));
    if let Some(next) = next {
        ui.ctx().request_repaint_after(next);
    }
    Some(Rect::from_min_size(origin, Vec2::splat(side)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_robes_are_the_colours_of_the_product_tiles() {
        for variant in Variant::ALL {
            let (robe, shade) = variant.robe();
            let tile = match variant {
                Variant::Zen | Variant::Toolbox => {
                    (Color32::from_rgb(0x8B, 0x7C, 0xF8), Color32::from_rgb(0x43, 0x38, 0xCA))
                }
                _ => crate::icons::product_colors(variant.id()),
            };
            let rgb = |c: Color32| [c.r(), c.g(), c.b()];
            assert_eq!((robe, shade), (rgb(tile.0), rgb(tile.1)), "{variant:?}");
        }
    }

    #[test]
    fn a_pixel_is_whole_physical_pixels_and_never_larger_than_asked() {
        assert_eq!(pixel_size(80.0, 2.0, 1.0), Some(2.0));
        assert_eq!(pixel_size(400.0, 2.0, 2.0), Some(2.0), "at most two points");
        assert_eq!(pixel_size(100.0, 4.0, 2.0), Some(2.5), "five physical pixels on a Retina screen");
        assert_eq!(pixel_size(70.0, 2.0, 1.0), Some(1.0));
        assert_eq!(pixel_size(39.0, 2.0, 1.0), None, "too small for even one pixel a pixel");
    }

    #[test]
    fn he_moves_frame_by_frame_and_stands_still_when_motion_is_reduced() {
        assert_eq!(frame_now(0.0, true).0, 0);
        assert_eq!(frame_now(0.17, true).0, 1);
        assert_eq!(frame_now(1.29, true).0, 0, "eight frames of 160 ms, then again");
        let (_, next) = frame_now(0.1, true);
        assert_eq!(next, Some(std::time::Duration::from_millis(60)), "the next frame is asked for");
        assert_eq!(frame_now(0.5, false), (STILL, None), "still, and no frames asked for");
    }
}

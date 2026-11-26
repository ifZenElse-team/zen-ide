use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use egui::epaint::{ClippedPrimitive, ImageDelta, Primitive, Vertex};
use egui::{
    Color32, ColorImage, Context, Event, FullOutput, ImageData, Pos2, RawInput, Rect, TextureId, Ui, Vec2, ViewportId,
};

const SETTLE_FRAMES: usize = 3;

const MAX_FRAMES: usize = 120;

pub struct Offscreen {
    ctx: Context,
    size: Vec2,
    pixels_per_point: f32,
    background: Color32,
    textures: HashMap<TextureId, Arc<ColorImage>>,
    time: f64,
    animating: bool,
}

impl Offscreen {
    pub fn new(size: Vec2, pixels_per_point: f32) -> Self {
        let ctx = Context::default();
        crate::theme::install(&ctx);
        Self {
            ctx,
            size,
            pixels_per_point,
            background: Color32::TRANSPARENT,
            textures: HashMap::new(),
            time: 0.0,
            animating: false,
        }
    }

    pub fn set_background(&mut self, background: Color32) {
        self.background = background;
    }

    pub fn set_size(&mut self, size: Vec2) {
        self.size = size;
    }

    pub fn context(&self) -> &Context {
        &self.ctx
    }

    pub fn render(&mut self, mut ui: impl FnMut(&mut Ui)) -> Frame {
        for drawn in 1..MAX_FRAMES {
            let output = self.pass(Vec::new(), &mut ui);
            if drawn >= SETTLE_FRAMES && !self.animating {
                return self.paint(output);
            }
            self.discard(output);
        }
        let output = self.pass(Vec::new(), &mut ui);
        self.paint(output)
    }

    pub fn frame(&mut self, events: Vec<Event>, ui: impl FnMut(&mut Ui)) -> Frame {
        let output = self.pass(events, ui);
        self.paint(output)
    }

    pub fn step(&mut self, events: Vec<Event>, ui: impl FnMut(&mut Ui)) {
        let output = self.pass(events, ui);
        self.discard(output);
    }

    pub fn accessibility_tree(&mut self, ui: impl FnMut(&mut Ui)) -> egui::accesskit::TreeUpdate {
        self.ctx.enable_accesskit();
        let mut output = self.pass(Vec::new(), ui);
        let update = output.platform_output.accesskit_update.take();
        self.discard(output);
        update.unwrap_or_else(|| panic!("egui builds the tree once AccessKit is on"))
    }

    fn pass(&mut self, events: Vec<Event>, ui: impl FnMut(&mut Ui)) -> FullOutput {
        self.time += 1.0 / 60.0;
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)),
            time: Some(self.time),
            predicted_dt: 1.0 / 60.0,
            events,
            ..RawInput::default()
        };
        input.viewports.entry(ViewportId::ROOT).or_default().native_pixels_per_point = Some(self.pixels_per_point);
        let mut output = self.ctx.run_ui(input, ui);
        self.animating = output
            .viewport_output
            .values()
            .any(|viewport| viewport.repaint_delay < std::time::Duration::from_millis(20));
        let mut updates: Vec<_> = output.textures_delta.set.drain().collect();
        updates.sort_by_key(|(id, _)| *id);
        for (id, deltas) in updates {
            for delta in &deltas {
                self.apply_texture(id, delta);
            }
        }
        output
    }

    fn discard(&mut self, mut output: FullOutput) {
        for id in output.textures_delta.free.drain() {
            self.textures.remove(&id);
        }
    }

    fn paint(&mut self, mut output: FullOutput) -> Frame {
        let pixels_per_point = output.pixels_per_point;
        let primitives = self.ctx.tessellate(std::mem::take(&mut output.shapes), pixels_per_point);
        let mut textures = HashMap::new();
        for primitive in &primitives {
            if let Primitive::Mesh(mesh) = &primitive.primitive
                && let Some(texture) = self.textures.get(&mesh.texture_id)
            {
                textures.entry(mesh.texture_id).or_insert_with(|| Arc::clone(texture));
            }
        }
        let frame = Frame {
            primitives,
            pixels_per_point,
            size: self.size,
            background: self.background,
            textures,
            image: OnceLock::new(),
        };
        self.discard(output);
        frame
    }

    fn apply_texture(&mut self, id: TextureId, delta: &ImageDelta) {
        let ImageData::Color(patch) = &delta.image;
        match delta.pos {
            None => {
                self.textures.insert(id, Arc::new((**patch).clone()));
            }
            Some([x0, y0]) => {
                let Some(texture) = self.textures.get_mut(&id).map(Arc::make_mut) else { return };
                let [width, _] = texture.size;
                let [height, columns] = [texture.size[1], patch.size[0]];
                for y in 0..patch.size[1].min(height.saturating_sub(y0)) {
                    for x in 0..columns.min(width.saturating_sub(x0)) {
                        texture.pixels[(y0 + y) * width + x0 + x] = patch.pixels[y * columns + x];
                    }
                }
            }
        }
    }
}

pub struct Frame {
    primitives: Vec<ClippedPrimitive>,
    pixels_per_point: f32,
    size: Vec2,
    background: Color32,
    textures: HashMap<TextureId, Arc<ColorImage>>,
    image: OnceLock<ColorImage>,
}

impl Frame {
    pub fn image(&self) -> &ColorImage {
        self.image.get_or_init(|| self.rasterize())
    }

    pub fn into_image(self) -> ColorImage {
        self.image();
        self.image.into_inner().unwrap_or_else(|| unreachable!("rasterised above"))
    }

    pub fn is_rasterized(&self) -> bool {
        self.image.get().is_some()
    }

    fn rasterize(&self) -> ColorImage {
        let pixels_per_point = self.pixels_per_point;
        let width = (self.size.x * pixels_per_point).round() as usize;
        let height = (self.size.y * pixels_per_point).round() as usize;
        let mut target = ColorImage::new([width, height], vec![self.background; width * height]);
        for primitive in &self.primitives {
            let Primitive::Mesh(mesh) = &primitive.primitive else { continue };
            let Some(texture) = self.textures.get(&mesh.texture_id) else { continue };
            let clip = primitive.clip_rect * pixels_per_point;
            for triangle in mesh.indices.as_chunks::<3>().0 {
                let corner = |index: u32| {
                    let vertex = mesh.vertices[index as usize];
                    Vertex { pos: (vertex.pos.to_vec2() * pixels_per_point).to_pos2(), ..vertex }
                };
                fill_triangle(
                    &mut target,
                    clip,
                    texture,
                    [corner(triangle[0]), corner(triangle[1]), corner(triangle[2])],
                );
            }
        }
        target
    }
}

impl std::ops::Deref for Frame {
    type Target = ColorImage;

    fn deref(&self) -> &ColorImage {
        self.image()
    }
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [width, height] = self.image().size;
        write!(formatter, "Frame({width}×{height})")
    }
}

impl PartialEq for Frame {
    fn eq(&self, other: &Self) -> bool {
        self.image() == other.image()
    }
}

impl PartialEq<ColorImage> for Frame {
    fn eq(&self, other: &ColorImage) -> bool {
        self.image() == other
    }
}

impl From<Frame> for ColorImage {
    fn from(frame: Frame) -> Self {
        frame.into_image()
    }
}

pub fn write_png(path: &std::path::Path, image: &ColorImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let [width, height] = image.size;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|pixel| pixel.to_array()).collect();
    writer.write_image_data(&bytes).map_err(|error| error.to_string())
}

fn edge(a: Pos2, b: Pos2, p: Pos2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

fn is_top_left(a: Pos2, b: Pos2) -> bool {
    (a.y == b.y && b.x < a.x) || b.y < a.y
}

fn fill_triangle(target: &mut ColorImage, clip: Rect, texture: &ColorImage, corners: [Vertex; 3]) {
    let [mut a, b, mut c] = corners;
    let mut area = edge(a.pos, b.pos, c.pos);
    if area == 0.0 {
        return;
    }
    if area < 0.0 {
        std::mem::swap(&mut a, &mut c);
        area = -area;
    }
    let [width, height] = target.size;
    let min_y = a.pos.y.min(b.pos.y).min(c.pos.y).max(clip.min.y).max(0.0).floor() as usize;
    let max_y = (a.pos.y.max(b.pos.y).max(c.pos.y).min(clip.max.y).ceil().max(0.0) as usize).min(height);
    let left = a.pos.x.min(b.pos.x).min(c.pos.x).max(clip.min.x).max(0.0);
    let right = a.pos.x.max(b.pos.x).max(c.pos.x).min(clip.max.x).min(width as f32);
    let edges = [(b.pos, c.pos), (c.pos, a.pos), (a.pos, b.pos)];
    for y in min_y..max_y {
        let center_y = y as f32 + 0.5;
        let (mut from_x, mut to_x) = (left, right);
        for (start, end) in edges {
            let (slope, offset) =
                (start.y - end.y, (end.x - start.x) * (center_y - start.y) + (end.y - start.y) * start.x);
            if slope > 0.0 {
                from_x = from_x.max(-offset / slope - 1.0);
            } else if slope < 0.0 {
                to_x = to_x.min(-offset / slope + 1.0);
            } else if offset < 0.0 {
                to_x = from_x;
            }
        }
        let from = from_x.max(0.0).floor() as usize;
        let to = (to_x.max(0.0).ceil() as usize).min(width);
        for x in from..to {
            let p = Pos2::new(x as f32 + 0.5, center_y);
            if !clip.contains(p) {
                continue;
            }
            let mut weights = [0.0; 3];
            let mut inside = true;
            for (weight, (from, to)) in weights.iter_mut().zip(edges) {
                let w = edge(from, to, p);
                if w < 0.0 || (w == 0.0 && !is_top_left(from, to)) {
                    inside = false;
                    break;
                }
                *weight = w / area;
            }
            if !inside {
                continue;
            }
            let [wa, wb, wc] = weights;
            let color = weighted([a.color, b.color, c.color], [wa, wb, wc]);
            let u = wa * a.uv.x + wb * b.uv.x + wc * c.uv.x;
            let v = wa * a.uv.y + wb * b.uv.y + wc * c.uv.y;
            let color = multiply(color, sample(texture, u, v));
            let pixel = &mut target.pixels[y * width + x];
            *pixel = over(color, *pixel);
        }
    }
}

fn weighted<const N: usize>(colors: [Color32; N], weights: [f32; N]) -> Color32 {
    let channel = |pick: fn(Color32) -> u8| {
        let value: f32 = colors.iter().zip(weights).map(|(color, weight)| f32::from(pick(*color)) * weight).sum();
        value.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_premultiplied(channel(|c| c.r()), channel(|c| c.g()), channel(|c| c.b()), channel(|c| c.a()))
}

fn sample(texture: &ColorImage, u: f32, v: f32) -> Color32 {
    let [width, height] = texture.size;
    if width == 0 || height == 0 {
        return Color32::TRANSPARENT;
    }
    let x = (u * width as f32 - 0.5).clamp(0.0, (width - 1) as f32);
    let y = (v * height as f32 - 0.5).clamp(0.0, (height - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(width - 1), (y0 + 1).min(height - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |x: usize, y: usize| texture.pixels[y * width + x];
    weighted(
        [at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1)],
        [(1.0 - fx) * (1.0 - fy), fx * (1.0 - fy), (1.0 - fx) * fy, fx * fy],
    )
}

fn multiply(a: Color32, b: Color32) -> Color32 {
    let channel = |x: u8, y: u8| ((u16::from(x) * u16::from(y) + 127) / 255) as u8;
    Color32::from_rgba_premultiplied(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
        channel(a.a(), b.a()),
    )
}

fn over(source: Color32, destination: Color32) -> Color32 {
    let keep = 255 - u16::from(source.a());
    let channel = |s: u8, d: u8| (u16::from(s) + (u16::from(d) * keep + 127) / 255).min(255) as u8;
    Color32::from_rgba_premultiplied(
        channel(source.r(), destination.r()),
        channel(source.g(), destination.g()),
        channel(source.b(), destination.b()),
        channel(source.a(), destination.a()),
    )
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Vec2};

    use super::Offscreen;

    fn screen() -> Offscreen {
        let mut screen = Offscreen::new(Vec2::new(120.0, 40.0), 1.0);
        screen.set_background(Color32::BLACK);
        screen
    }

    #[test]
    fn a_frame_is_rasterised_only_when_its_picture_is_read() {
        let mut screen = screen();
        let frame = screen.frame(Vec::new(), |ui| {
            ui.label("lazy");
        });
        assert!(!frame.is_rasterized(), "nothing is painted for a frame nobody looks at");
        assert_eq!(frame.image().size, [120, 40], "reading it paints it");
        assert!(frame.is_rasterized());
        assert!(frame.image().pixels.iter().any(|pixel| *pixel != Color32::BLACK), "the label is drawn");
    }

    #[test]
    fn a_frame_keeps_its_picture_when_later_frames_change_the_textures() {
        let mut screen = screen();
        let first = screen.frame(Vec::new(), |ui| {
            ui.label("abc");
        });
        for _ in 0..3 {
            screen.step(Vec::new(), |ui| {
                ui.heading("ÄÖÜ ßẞ 文字 ¿¡ «»");
            });
        }
        let mut reference = Offscreen::new(Vec2::new(120.0, 40.0), 1.0);
        reference.set_background(Color32::BLACK);
        let drawn_at_once = reference.frame(Vec::new(), |ui| {
            ui.label("abc");
        });
        assert_eq!(first.image(), drawn_at_once.image(), "the late picture is the one drawn then");
    }
}

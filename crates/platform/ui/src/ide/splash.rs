use egui::{
    Align2, Color32, Pos2, Rect, Shape, Stroke, Vec2, ViewportBuilder, ViewportCommand, ViewportId, epaint::Mesh, pos2,
    vec2,
};

use super::IdeProduct;
use crate::theme;

const SIZE: [f32; 2] = [640.0, 400.0];
const LONGEST: f64 = 6.0;
const SHORTEST: f64 = 2.0;
const KOZO_SIDE: f32 = 80.0;
const KOZO_BELOW: f32 = 110.0;

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Restore {
    pub(super) maximized: bool,
    pub(super) fullscreen: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Splash {
    restore: Option<Restore>,
    hidden: bool,
}

impl Splash {
    pub(super) fn starting(restore: Restore) -> Self {
        Self { restore: Some(restore), hidden: false }
    }

    pub(super) fn finished() -> Self {
        Self::default()
    }

    pub(super) fn is_active(&self) -> bool {
        self.restore.is_some()
    }

    pub(super) fn show(&mut self, ctx: &egui::Context, product: &IdeProduct, ready: bool) {
        let Some(restore) = self.restore.clone() else { return };
        if !std::mem::replace(&mut self.hidden, true) {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
        let now = ctx.input(|input| input.time);
        if (ready && now >= SHORTEST) || now >= LONGEST {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            if restore.maximized {
                ctx.send_viewport_cmd(ViewportCommand::Maximized(true));
            }
            if restore.fullscreen {
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
            }
            self.restore = None;
            ctx.request_repaint();
            return;
        }
        ctx.request_repaint();
        let monitor = ctx.input(|input| input.viewport().monitor_size).unwrap_or(vec2(1512.0, 982.0));
        let position = pos2((monitor.x - SIZE[0]) / 2.0, (monitor.y - SIZE[1]) / 2.0);
        let builder = ViewportBuilder::default()
            .with_title(product.name)
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(SIZE)
            .with_position(position)
            .with_window_level(egui::WindowLevel::AlwaysOnTop);
        ctx.show_viewport_immediate(ViewportId::from_hash_of("zen-splash"), builder, |ctx, _class| {
            let screen = ctx.content_rect();
            egui::Area::new(egui::Id::unique("zen-splash-picture")).fixed_pos(screen.min).show(ctx, |ui| {
                ui.allocate_rect(screen, egui::Sense::hover());
                picture(ui, screen, product, now as f32);
            });
        });
    }
}

fn picture(ui: &egui::Ui, screen: Rect, product: &IdeProduct, t: f32) {
    paint(ui.painter(), screen, product, t, 1.0);
    let mark = Rect::from_center_size(screen.center() - vec2(0.0, 70.0), Vec2::splat(64.0));
    crate::icons::product_mark_faded(ui, mark, product.id, product.name, 1.0);
    let kozo = Rect::from_center_size(screen.center() + vec2(0.0, KOZO_BELOW), Vec2::splat(KOZO_SIDE));
    crate::mascot::show(ui, kozo, product.id, zen_mascot::State::Idle, KOZO_SIDE / zen_mascot::SIZE as f32);
}

fn paint(painter: &egui::Painter, screen: Rect, product: &IdeProduct, t: f32, alpha: f32) {
    let fade = |color: Color32, a: f32| color.gamma_multiply(a * alpha);
    painter.rect_filled(screen, 0, fade(Color32::from_rgb(0x15, 0x16, 0x19), 1.0));
    let (from, to) = crate::icons::product_colors(product.id);
    let accent = Color32::from_rgb(0x37, 0x71, 0xE1);
    let size = screen.width().max(screen.height());
    let lights = [
        (from, 0.55, (0.30, 0.35), (0.07, 0.05), 0.11),
        (to, 0.50, (0.70, 0.62), (0.06, 0.08), -0.09),
        (accent, 0.38, (0.52, 0.22), (0.09, 0.04), 0.07),
        (from.lerp_to_gamma(to, 0.5), 0.42, (0.22, 0.78), (0.05, 0.06), -0.13),
    ];
    for (color, radius, (cx, cy), (rx, ry), speed) in lights {
        let angle = t * speed * std::f32::consts::TAU;
        let center = pos2(
            screen.left() + screen.width() * (cx + rx * angle.cos()),
            screen.top() + screen.height() * (cy + ry * angle.sin()),
        );
        soft_light(painter, center, size * radius, fade(color, 0.22));
    }
    let lines = 14;
    for i in 0..lines {
        let y =
            screen.top() + screen.height() * (i as f32 + 0.5) / lines as f32 + (t * 6.0 + i as f32 * 13.0).sin() * 3.0;
        painter.hline(screen.x_range(), y, Stroke::new(1.0, fade(Color32::WHITE, 0.012)));
    }

    let center = screen.center() - vec2(0.0, 30.0);
    painter.text(
        center + vec2(0.0, 24.0),
        Align2::CENTER_TOP,
        product.name,
        theme::semibold(28.0),
        fade(Color32::WHITE, 0.95),
    );
    if !product.version.is_empty() {
        painter.text(
            center + vec2(0.0, 62.0),
            Align2::CENTER_TOP,
            product.version,
            theme::regular(13.0),
            fade(Color32::WHITE, 0.55),
        );
    }
}

fn soft_light(painter: &egui::Painter, center: Pos2, radius: f32, color: Color32) {
    let mut mesh = Mesh::default();
    let segments = 64;
    mesh.colored_vertex(center, color);
    for i in 0..=segments {
        let angle = i as f32 / segments as f32 * std::f32::consts::TAU;
        mesh.colored_vertex(center + vec2(angle.cos(), angle.sin()) * radius, Color32::TRANSPARENT);
    }
    for i in 0..segments {
        mesh.add_triangle(0, i + 1, i + 2);
    }
    painter.add(Shape::mesh(mesh));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_splash_shows_the_products_kozo_under_its_name() {
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from);
        for (id, name) in [
            ("gogarden", "GoGarden"),
            ("rustronin", "RustRonin"),
            ("webwave", "WebWave"),
            ("jvmjizo", "JvmJizo"),
            ("sqlscroll", "SqlScroll"),
            ("devopsdaruma", "DevopsDaruma"),
            ("test", "Test"),
        ] {
            let product = IdeProduct { id, name, version: "0.1.0", ..super::super::file_type_tests::product() };
            let mut screen = crate::offscreen::Offscreen::new(vec2(SIZE[0], SIZE[1]), 2.0);
            let image = screen.render(|ui| picture(ui, Rect::from_min_size(Pos2::ZERO, SIZE.into()), &product, 0.0));
            if let Some(dir) = &snapshots {
                crate::offscreen::write_png(&dir.join(format!("splash-{id}.png")), &image).unwrap();
            }
            let origin = pos2(SIZE[0] / 2.0 - KOZO_SIDE / 2.0, SIZE[1] / 2.0 + KOZO_BELOW - KOZO_SIDE / 2.0);
            let at = |x: usize, y: usize| {
                let point = origin + vec2(x as f32 * 2.0 + 1.0, y as f32 * 2.0 + 1.0);
                image.pixels[(point.y * 2.0) as usize * image.size[0] + (point.x * 2.0) as usize]
            };
            let variant = zen_mascot::Variant::of(id);
            let [r, g, b] = zen_mascot::Ink::Robe.rgb(variant).unwrap();
            assert_eq!(at(14, 24), Color32::from_rgb(r, g, b), "{id}: the robe, crisp");
            let [r, g, b] = zen_mascot::Ink::Outline.rgb(variant).unwrap();
            assert_eq!(at(17, 5), Color32::from_rgb(r, g, b), "{id}: the top of his head");
        }
    }
}

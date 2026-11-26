use std::f32::consts::TAU;

use egui::{Align2, Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

use crate::theme;

fn grid(rect: Rect, x: f32, y: f32) -> Pos2 {
    rect.min + vec2(x / 16.0 * rect.width(), y / 16.0 * rect.height())
}

fn stroke(rect: Rect, color: Color32) -> Stroke {
    Stroke::new((rect.width() / 16.0 * 1.5).max(1.0), color)
}

fn arc(center: Pos2, radius: f32, start: f32, end: f32) -> Vec<Pos2> {
    let steps = 48;
    (0..=steps)
        .map(|i| {
            let angle = start + (end - start) * i as f32 / steps as f32;
            center + radius * Vec2::angled(angle)
        })
        .collect()
}

pub fn laptop(painter: &Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let screen = Rect::from_min_max(grid(rect, 3.0, 3.5), grid(rect, 13.0, 10.5));
    painter.rect_stroke(screen, 1.5, s, egui::StrokeKind::Middle);
    painter.line_segment([grid(rect, 1.0, 12.5), grid(rect, 15.0, 12.5)], s);
}

pub fn gear(painter: &Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let center = rect.center();
    let r = rect.width() / 16.0;
    painter.circle_stroke(center, 4.2 * r, s);
    painter.circle_stroke(center, 1.8 * r, s);
    for i in 0..8 {
        let direction = Vec2::angled(i as f32 * TAU / 8.0);
        painter.line_segment([center + direction * 4.2 * r, center + direction * 6.5 * r], s);
    }
}

pub fn search(painter: &Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(grid(rect, 7.0, 7.0), rect.width() / 16.0 * 4.5, s);
    painter.line_segment([grid(rect, 10.3, 10.3), grid(rect, 14.0, 14.0)], s);
}

pub fn chevron_down(painter: &Painter, rect: Rect, color: Color32) {
    painter.add(Shape::line(
        vec![grid(rect, 4.0, 6.0), grid(rect, 8.0, 10.0), grid(rect, 12.0, 6.0)],
        stroke(rect, color),
    ));
}

pub fn chevron_up(painter: &Painter, rect: Rect, color: Color32) {
    painter.add(Shape::line(
        vec![grid(rect, 4.0, 10.0), grid(rect, 8.0, 6.0), grid(rect, 12.0, 10.0)],
        stroke(rect, color),
    ));
}

pub fn chevron_left(painter: &Painter, rect: Rect, color: Color32) {
    painter.add(Shape::line(
        vec![grid(rect, 10.0, 4.0), grid(rect, 6.0, 8.0), grid(rect, 10.0, 12.0)],
        stroke(rect, color),
    ));
}

pub fn chevron_right(painter: &Painter, rect: Rect, color: Color32) {
    painter.add(Shape::line(
        vec![grid(rect, 6.0, 4.0), grid(rect, 10.0, 8.0), grid(rect, 6.0, 12.0)],
        stroke(rect, color),
    ));
}

pub fn lock(painter: &Painter, rect: Rect, color: Color32, open: bool) {
    let s = stroke(rect, color);
    let r = rect.width() / 16.0;
    let top = if open { 3.5 } else { 5.5 };
    let center = grid(rect, 8.0, top);
    painter.add(Shape::line(arc(center, 2.75 * r, std::f32::consts::PI, TAU), s));
    painter.line_segment([grid(rect, 5.25, top), grid(rect, 5.25, 7.5)], s);
    painter.line_segment([grid(rect, 10.75, top), grid(rect, 10.75, if open { 4.75 } else { 7.5 })], s);
    let body = Rect::from_min_max(grid(rect, 3.5, 7.5), grid(rect, 12.5, 14.0));
    painter.rect_stroke(body, 1.5 * r, s, egui::StrokeKind::Middle);
    painter.circle_filled(grid(rect, 8.0, 10.75), r, color);
}

pub fn pin(painter: &Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let head = Rect::from_min_max(grid(rect, 5.5, 2.0), grid(rect, 10.5, 8.5));
    painter.rect_stroke(head, rect.width() / 16.0, s, egui::StrokeKind::Middle);
    painter.line_segment([grid(rect, 3.5, 8.5), grid(rect, 12.5, 8.5)], s);
    painter.line_segment([grid(rect, 8.0, 8.5), grid(rect, 8.0, 14.5)], s);
}

pub fn more_vertical(painter: &Painter, rect: Rect, color: Color32) {
    let r = rect.width() / 16.0 * 1.3;
    for y in [3.5, 8.0, 12.5] {
        painter.circle_filled(grid(rect, 8.0, y), r, color);
    }
}

fn rounded_outline(rect: Rect, radius: f32) -> Vec<Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let corners = [
        (pos2(rect.left() + r, rect.top() + r), 0.5 * TAU),
        (pos2(rect.right() - r, rect.top() + r), 0.75 * TAU),
        (pos2(rect.right() - r, rect.bottom() - r), 0.0),
        (pos2(rect.left() + r, rect.bottom() - r), 0.25 * TAU),
    ];
    let steps = 12;
    let mut points = Vec::with_capacity(4 * (steps + 1));
    for (center, start) in corners {
        for i in 0..=steps {
            points.push(center + r * Vec2::angled(start + 0.25 * TAU * i as f32 / steps as f32));
        }
    }
    points
}

pub fn fill_rounded(painter: &Painter, rect: Rect, radius: f32, color_at: impl Fn(Vec2) -> Color32) {
    let relative = |p: Pos2| (p - rect.min) / rect.size();
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.center(), color_at(relative(rect.center())));
    let outline = rounded_outline(rect, radius);
    for point in &outline {
        mesh.colored_vertex(*point, color_at(relative(*point)));
    }
    let n = outline.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
}

pub(crate) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let channel = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        channel(a.r(), b.r()),
        channel(a.g(), b.g()),
        channel(a.b(), b.b()),
        channel(a.a(), b.a()),
    )
}

fn tile(painter: &Painter, rect: Rect, from: Color32, to: Color32) {
    let radius = rect.width() * 0.28;
    fill_rounded(painter, rect, radius, |p| mix(from, to, (p.x + p.y) / 2.0));
    let gloss = Color32::from_white_alpha(46);
    fill_rounded(painter, rect, radius, |p| mix(gloss, Color32::TRANSPARENT, (p.y * 1.8).min(1.0)));
}

pub fn zen_mark(painter: &Painter, rect: Rect, _color: Color32) {
    tile(painter, rect, Color32::from_rgb(0x8B, 0x7C, 0xF8), Color32::from_rgb(0x43, 0x38, 0xCA));
    let center = rect.center();
    let radius = rect.width() * 0.25;
    let width = rect.width() * 0.085;
    painter.add(Shape::line(arc(center, radius, -0.22 * TAU, 0.66 * TAU), Stroke::new(width, Color32::WHITE)));
    painter.circle_filled(center + radius * Vec2::angled(0.66 * TAU), width * 0.7, Color32::WHITE);
}

pub(crate) fn product_colors(id: &str) -> (Color32, Color32) {
    let rgb = Color32::from_rgb;
    match id {
        "rustronin" => (rgb(0xFF, 0x9A, 0x5A), rgb(0xD9, 0x3A, 0x2B)),
        "gogarden" => (rgb(0x5E, 0xE7, 0xD0), rgb(0x0E, 0x8F, 0xB0)),
        "pypetal" => (rgb(0xFF, 0xB8, 0xD0), rgb(0xD9, 0x4F, 0x8A)),
        "jvmjizo" => (rgb(0xB8, 0xC4, 0xA6), rgb(0x5E, 0x6E, 0x58)),
        "webwave" => (rgb(0x6F, 0xB6, 0xFF), rgb(0x25, 0x5F, 0xDB)),
        "cppcrane" => (rgb(0x7E, 0xE0, 0x9A), rgb(0x1F, 0x8A, 0x55)),
        "sharpshuriken" => (rgb(0xA5, 0x9C, 0xFF), rgb(0x48, 0x3B, 0xC9)),
        "phppagoda" => (rgb(0x9F, 0xB4, 0xF5), rgb(0x52, 0x60, 0xB8)),
        "rubyryokan" => (rgb(0xFF, 0x7A, 0x8A), rgb(0xC0, 0x1E, 0x3C)),
        "sqlscroll" => (rgb(0x6F, 0xE0, 0xC8), rgb(0x1C, 0x7F, 0x86)),
        "devopsdaruma" => (rgb(0xFF, 0x9E, 0x8E), rgb(0xC7, 0x3B, 0x3B)),
        "testtotami" => (rgb(0xE6, 0xD3, 0x8C), rgb(0x9A, 0x86, 0x2E)),
        _ => (Color32::from_gray(0xA0), Color32::from_gray(0x60)),
    }
}

fn muted(color: Color32) -> Color32 {
    let grey = ((color.r() as u32 + color.g() as u32 + color.b() as u32) / 3) as u8;
    mix(color, Color32::from_gray(grey), 0.75).gamma_multiply(0.55)
}

fn abbreviation(name: &str) -> String {
    let capitals: String = name.chars().filter(char::is_ascii_uppercase).take(2).collect();
    if capitals.len() == 2 { capitals } else { name.chars().take(2).collect::<String>().to_uppercase() }
}

pub fn app_icon(id: &str) -> Option<egui::IconData> {
    const CANVAS: usize = 1024;
    const TILE: u32 = 824;
    let svg = product_svg(id)?;
    let size = egui::SizeHint::Size { width: TILE, height: TILE, maintain_aspect_ratio: true };
    let tile = egui_extras::image::load_svg_bytes_with_size(svg, size, &Default::default()).ok()?;
    let [width, height] = tile.size;
    let (left, top) = ((CANVAS - width) / 2, (CANVAS - height) / 2);
    let mut rgba = vec![0u8; CANVAS * CANVAS * 4];
    for y in 0..height {
        for x in 0..width {
            let pixel = tile.pixels[y * width + x].to_srgba_unmultiplied();
            let at = ((top + y) * CANVAS + left + x) * 4;
            rgba[at..at + 4].copy_from_slice(&pixel);
        }
    }
    Some(egui::IconData { rgba, width: CANVAS as u32, height: CANVAS as u32 })
}

pub fn product_svg(id: &str) -> Option<&'static [u8]> {
    Some(match id {
        "toolbox" => include_bytes!("../../../../assets/icons/products/toolbox.svg"),
        "rustronin" => include_bytes!("../../../../assets/icons/products/rustronin.svg"),
        "gogarden" => include_bytes!("../../../../assets/icons/products/gogarden.svg"),
        "pypetal" => include_bytes!("../../../../assets/icons/products/pypetal.svg"),
        "jvmjizo" => include_bytes!("../../../../assets/icons/products/jvmjizo.svg"),
        "webwave" => include_bytes!("../../../../assets/icons/products/webwave.svg"),
        "cppcrane" => include_bytes!("../../../../assets/icons/products/cppcrane.svg"),
        "sharpshuriken" => include_bytes!("../../../../assets/icons/products/sharpshuriken.svg"),
        "phppagoda" => include_bytes!("../../../../assets/icons/products/phppagoda.svg"),
        "rubyryokan" => include_bytes!("../../../../assets/icons/products/rubyryokan.svg"),
        "sqlscroll" => include_bytes!("../../../../assets/icons/products/sqlscroll.svg"),
        "devopsdaruma" => include_bytes!("../../../../assets/icons/products/devopsdaruma.svg"),
        "testtotami" => include_bytes!("../../../../assets/icons/products/testtotami.svg"),
        _ => return None,
    })
}

pub fn product_mark(ui: &egui::Ui, rect: Rect, id: &str, name: &str, dimmed: bool) {
    if let Some(svg) = product_svg(id) {
        let image = egui::Image::from_bytes(format!("bytes://zen/products/{id}.svg"), svg).tint(if dimmed {
            Color32::from_white_alpha(110)
        } else {
            Color32::WHITE
        });
        image.paint_at(ui, rect);
        return;
    }
    drawn_mark(ui.painter(), rect, id, name, dimmed);
}

pub(crate) fn product_mark_faded(ui: &egui::Ui, rect: Rect, id: &str, name: &str, alpha: f32) {
    if let Some(svg) = product_svg(id) {
        let image = egui::Image::from_bytes(format!("bytes://zen/products/{id}.svg"), svg)
            .tint(Color32::from_white_alpha((alpha * 255.0) as u8));
        image.paint_at(ui, rect);
        return;
    }
    drawn_mark(ui.painter(), rect, id, name, false);
}

fn drawn_mark(painter: &Painter, rect: Rect, id: &str, name: &str, dimmed: bool) {
    let (from, to) = product_colors(id);
    let (from, to) = if dimmed { (muted(from), muted(to)) } else { (from, to) };
    tile(painter, rect, from, to);
    let text = if dimmed { Color32::from_white_alpha(150) } else { Color32::WHITE };
    painter.text(
        rect.center() + vec2(0.0, rect.height() * 0.01),
        Align2::CENTER_CENTER,
        abbreviation(name),
        theme::bold(rect.height() * 0.36),
        text,
    );
}

pub fn status_dot(painter: &Painter, mark: Rect, color: Color32, ring: Color32) {
    let center = pos2(mark.right() - mark.width() * 0.06, mark.bottom() - mark.height() * 0.06);
    let radius = mark.width() * 0.12;
    painter.circle_filled(center, radius + 2.0, ring);
    painter.circle_filled(center, radius, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_product_has_an_icon() {
        for id in [
            "rustronin",
            "gogarden",
            "pypetal",
            "jvmjizo",
            "webwave",
            "cppcrane",
            "sharpshuriken",
            "phppagoda",
            "rubyryokan",
            "sqlscroll",
            "devopsdaruma",
            "testtotami",
        ] {
            let svg = product_svg(id).unwrap_or_else(|| panic!("{id} has no icon"));
            assert!(std::str::from_utf8(svg).unwrap().starts_with("<svg"), "{id}");
        }
    }

    #[test]
    fn abbreviations() {
        assert_eq!(abbreviation("RustRonin"), "RR");
        assert_eq!(abbreviation("WebWave"), "WW");
        assert_eq!(abbreviation("toolbox"), "TO");
    }
}

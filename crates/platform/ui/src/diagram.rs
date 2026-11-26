use egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

use crate::ide::IdePalette;
use crate::{accessibility, theme};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Marker {
    #[default]
    None,
    Key,
    ForeignKey,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    pub text: String,
    pub detail: String,
    pub marker: Marker,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub rect: Rect,
    pub title: String,
    pub rows: Vec<Row>,
    pub more: Option<String>,
    pub dashed: bool,
    pub muted: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum End {
    #[default]
    None,
    One,
    ZeroOrOne,
    Many,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edge {
    pub points: Vec<Pos2>,
    pub from: usize,
    pub to: usize,
    pub start: End,
    pub end: End,
    pub dashed: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub size: Vec2,
    pub header: f32,
    pub row: f32,
    pub padding: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub zoom: f32,
    pub origin: Pos2,
}

impl Default for Camera {
    fn default() -> Self {
        Self { zoom: 1.0, origin: Pos2::ZERO }
    }
}

pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 4.0;

impl Camera {
    pub fn to_screen(&self, canvas: Rect, point: Pos2) -> Pos2 {
        canvas.min + (point - self.origin) * self.zoom
    }

    pub fn to_diagram(&self, canvas: Rect, point: Pos2) -> Pos2 {
        self.origin + (point - canvas.min) / self.zoom
    }

    pub fn zoom_around(&mut self, canvas: Rect, anchor: Pos2, zoom: f32) {
        let fixed = self.to_diagram(canvas, anchor);
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.origin = fixed - (anchor - canvas.min) / self.zoom;
    }

    pub fn visible(&self, canvas: Rect) -> Rect {
        Rect::from_min_max(self.to_diagram(canvas, canvas.min), self.to_diagram(canvas, canvas.max))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Interaction {
    pub clicked: Option<usize>,
    pub opened: Option<usize>,
    pub cleared: bool,
    pub hovered: Option<usize>,
    pub moved: bool,
}

struct Colors {
    fill: Color32,
    header: Color32,
    border: Color32,
    text: Color32,
    secondary: Color32,
    line: Color32,
    selected: Color32,
    key: Color32,
    foreign: Color32,
}

fn colors(p: &IdePalette) -> Colors {
    Colors {
        fill: p.island,
        header: p.sticky_background,
        border: p.button_border,
        text: p.text,
        secondary: p.text_secondary,
        line: p.text_secondary,
        selected: p.accent,
        key: p.warning_text,
        foreign: p.link,
    }
}

pub fn export_colors(p: &IdePalette) -> [Color32; 9] {
    let c = colors(p);
    [p.editor_background, c.fill, c.header, c.border, c.text, c.secondary, c.line, c.key, c.foreign]
}

fn key_icon(painter: &Painter, center: Pos2, size: f32, color: Color32, foreign: bool) {
    let stroke = Stroke::new((size * 0.12).max(1.0), color);
    let ring = pos2(center.x - size * 0.22, center.y);
    painter.circle_stroke(ring, size * 0.2, stroke);
    let start = ring.x + size * 0.2;
    let end = center.x + size * 0.4;
    painter.hline(start..=end, ring.y, stroke);
    painter.vline(end - size * 0.04, ring.y..=ring.y + size * 0.22, stroke);
    painter.vline(end - size * 0.22, ring.y..=ring.y + size * 0.16, stroke);
    if foreign {
        let tip = pos2(center.x + size * 0.45, center.y - size * 0.32);
        painter.line_segment([tip - vec2(size * 0.3, 0.0), tip], stroke);
        painter.line_segment([tip, tip + vec2(-size * 0.12, -size * 0.1)], stroke);
        painter.line_segment([tip, tip + vec2(-size * 0.12, size * 0.1)], stroke);
    }
}

fn end_mark(painter: &Painter, tip: Pos2, before: Pos2, end: End, zoom: f32, stroke: Stroke) {
    let direction = (before - tip).normalized();
    if !direction.x.is_finite() {
        return;
    }
    let across = vec2(-direction.y, direction.x);
    let spread = 5.0 * zoom;
    let depth = 9.0 * zoom;
    match end {
        End::None => {}
        End::Many => {
            let joint = tip + direction * depth;
            for side in [-1.0, 0.0, 1.0] {
                painter.line_segment([joint, tip + across * spread * side], stroke);
            }
        }
        End::One | End::ZeroOrOne => {
            let at = tip + direction * depth * 0.6;
            painter.line_segment([at - across * spread, at + across * spread], stroke);
            if end == End::ZeroOrOne {
                let circle = tip + direction * depth * 1.25;
                painter.circle_stroke(circle, 3.0 * zoom, stroke);
            }
        }
    }
}

fn dashed(painter: &Painter, points: &[Pos2], stroke: Stroke, zoom: f32) {
    for pair in points.windows(2) {
        painter.extend(Shape::dashed_line(&[pair[0], pair[1]], stroke, 5.0 * zoom, 3.0 * zoom));
    }
}

fn fitted(painter: &Painter, text: &str, font: &FontId, color: Color32, room: f32) -> String {
    let width = |text: &str| painter.layout_no_wrap(text.to_owned(), font.clone(), color).size().x;
    if width(text) <= room {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut low, mut high) = (0usize, chars.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        let candidate: String = chars[..middle].iter().collect::<String>() + "…";
        if width(&candidate) <= room { low = middle } else { high = middle - 1 }
    }
    chars[..low].iter().collect::<String>() + "…"
}

fn node_shape(painter: &Painter, c: &Colors, scene: &Scene, node: &Node, rect: Rect, zoom: f32, selected: bool) {
    let fade = |color: Color32| if node.muted { color.gamma_multiply(0.6) } else { color };
    let radius = CornerRadius::same((4.0 * zoom).round().clamp(0.0, 8.0) as u8);
    painter.rect_filled(rect, radius, c.fill);
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), scene.header * zoom));
    painter.rect_filled(header, CornerRadius { sw: 0, se: 0, ..radius }, fade(c.header));
    let outline = if selected { Stroke::new(2.0, c.selected) } else { Stroke::new(1.0, fade(c.border)) };
    if node.dashed && !selected {
        let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()];
        dashed(painter, &corners, outline, zoom.max(0.5));
    } else {
        painter.rect_stroke(rect, radius, outline, StrokeKind::Inside);
    }
    if zoom * 12.0 < 5.0 {
        return;
    }
    let pad = scene.padding * zoom;
    let title_font = theme::semibold(12.5 * zoom);
    let title = fitted(painter, &node.title, &title_font, c.text, rect.width() - 2.0 * pad);
    painter.text(pos2(rect.left() + pad, header.center().y), Align2::LEFT_CENTER, title, title_font, fade(c.text));
    let font = FontId::monospace(12.0 * zoom);
    let icon = scene.row * zoom;
    let room = rect.width() - 2.0 * pad - icon;
    let char_width = painter.layout_no_wrap("m".to_owned(), font.clone(), c.text).size().x.max(1.0);
    for (at, row) in node.rows.iter().enumerate() {
        let y = header.bottom() + (at as f32 + 0.5) * scene.row * zoom;
        let left = rect.left() + pad;
        match row.marker {
            Marker::None => {}
            Marker::Key => key_icon(painter, pos2(left + icon * 0.4, y), icon * 0.7, fade(c.key), false),
            Marker::ForeignKey => key_icon(painter, pos2(left + icon * 0.4, y), icon * 0.7, fade(c.foreign), true),
        }
        let (text, detail) = zen_db::diagram::fit_row(&row.text, &row.detail, room, char_width);
        painter.text(pos2(left + icon, y), Align2::LEFT_CENTER, text, font.clone(), fade(c.text));
        painter.text(pos2(rect.right() - pad, y), Align2::RIGHT_CENTER, detail, font.clone(), fade(c.secondary));
    }
    if let Some(more) = &node.more {
        let y = header.bottom() + (node.rows.len() as f32 + 0.5) * scene.row * zoom;
        painter.text(pos2(rect.left() + pad + icon, y), Align2::LEFT_CENTER, more, font, c.secondary);
    }
}

pub fn paint(painter: &Painter, p: &IdePalette, canvas: Rect, scene: &Scene, camera: &Camera, selected: Option<usize>) {
    let c = colors(p);
    let painter = painter.with_clip_rect(canvas.intersect(painter.clip_rect()));
    let view = camera.visible(canvas).expand(12.0 / camera.zoom);
    let zoom = camera.zoom;
    for edge in &scene.edges {
        let Some(bounds) = bounds_of(&edge.points) else { continue };
        if !view.intersects(bounds.expand(1.0)) {
            continue;
        }
        let near = selected.is_some_and(|chosen| chosen == edge.from || chosen == edge.to);
        let color = if near { c.selected } else { c.line };
        let stroke = Stroke::new(if near { 1.8 } else { 1.2 }, color);
        let points: Vec<Pos2> = edge.points.iter().map(|point| camera.to_screen(canvas, *point)).collect();
        if edge.dashed {
            dashed(&painter, &points, stroke, zoom.max(0.5));
        } else {
            painter.add(Shape::line(points.clone(), stroke));
        }
        if points.len() >= 2 {
            end_mark(&painter, points[0], points[1], edge.start, zoom, stroke);
            end_mark(&painter, points[points.len() - 1], points[points.len() - 2], edge.end, zoom, stroke);
        }
    }
    for (at, node) in scene.nodes.iter().enumerate() {
        if !view.intersects(node.rect) {
            continue;
        }
        let rect = Rect::from_min_max(camera.to_screen(canvas, node.rect.min), camera.to_screen(canvas, node.rect.max));
        node_shape(&painter, &c, scene, node, rect, zoom, selected == Some(at));
    }
}

fn bounds_of(points: &[Pos2]) -> Option<Rect> {
    let first = points.first()?;
    Some(
        points
            .iter()
            .fold(Rect::from_min_max(*first, *first), |rect, point| rect.union(Rect::from_min_max(*point, *point))),
    )
}

pub fn node_at(scene: &Scene, point: Pos2) -> Option<usize> {
    scene.nodes.iter().rposition(|node| node.rect.contains(point))
}

pub fn show(
    ui: &mut Ui,
    p: &IdePalette,
    canvas: Rect,
    (scene, camera, selected): (&Scene, &mut Camera, Option<usize>),
    name: &str,
) -> Interaction {
    let response = ui.interact(canvas, ui.scope_id().with(("diagram-canvas", name)), Sense::click_and_drag());
    accessibility::name_with(&response, egui::Role::Canvas, || name.to_owned());
    let mut interaction = Interaction::default();
    let pointer = response.hover_pos();
    if response.dragged() {
        let delta = response.drag_delta();
        if delta != Vec2::ZERO {
            camera.origin -= delta / camera.zoom;
            interaction.moved = true;
        }
    }
    if let Some(pointer) = pointer {
        let (scroll, zoom, command) =
            ui.input(|input| (input.smooth_scroll_delta, input.zoom_delta(), input.modifiers.command));
        if zoom != 1.0 {
            camera.zoom_around(canvas, pointer, camera.zoom * zoom);
            interaction.moved = true;
        } else if command && scroll.y != 0.0 {
            camera.zoom_around(canvas, pointer, camera.zoom * (scroll.y / 200.0).exp());
            interaction.moved = true;
        } else if scroll != Vec2::ZERO {
            camera.origin -= scroll / camera.zoom;
            interaction.moved = true;
        }
        interaction.hovered = node_at(scene, camera.to_diagram(canvas, pointer));
    }
    if let Some(at) = response.interact_pointer_pos() {
        let hit = node_at(scene, camera.to_diagram(canvas, at));
        if response.double_clicked() {
            interaction.opened = hit;
        } else if response.clicked() {
            interaction.clicked = hit;
            interaction.cleared = hit.is_none();
        }
    }
    if interaction.hovered.is_some() && !response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if response.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }
    ui.painter().rect_filled(canvas, 0, p.editor_background);
    paint(ui.painter(), p, canvas, scene, camera, selected);
    interaction
}

pub fn fit(scene: &Scene, canvas: Rect) -> Camera {
    let margin = 24.0;
    let zoom = ((canvas.width() - 2.0 * margin) / scene.size.x.max(1.0))
        .min((canvas.height() - 2.0 * margin) / scene.size.y.max(1.0))
        .clamp(MIN_ZOOM, 1.0);
    Camera { zoom, origin: pos2(scene.size.x / 2.0 - canvas.width() / 2.0 / zoom, -margin / zoom) }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn render(scene: &Scene, p: &IdePalette, scale: f32) -> egui::ColorImage {
    let margin = 16.0;
    let size = scene.size + Vec2::splat(2.0 * margin);
    let mut screen = crate::offscreen::Offscreen::new(size, scale);
    screen.set_background(p.editor_background);
    p.apply(screen.context());
    let camera = Camera { zoom: 1.0, origin: pos2(-margin, -margin) };
    screen.render(|ui| paint(ui.painter(), p, Rect::from_min_size(Pos2::ZERO, size), scene, &camera, None)).into_image()
}

#[cfg(test)]
mod tests;

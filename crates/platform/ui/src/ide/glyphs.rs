use egui::{Align2, Color32, CornerRadius, Rect, Stroke, StrokeKind, pos2, vec2};
use zen_syntax::SymbolKind;

use super::IdePalette;
use crate::theme;

pub(super) type Glyph = fn(&egui::Painter, Rect, Color32);

pub(super) fn stroke(rect: Rect, color: Color32) -> Stroke {
    Stroke::new((rect.width() / 14.0 * 1.3).max(1.1), color)
}

pub(super) fn glyph_folder(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let tab = Rect::from_min_max(
        rect.left_top() + vec2(0.0, rect.height() * 0.12),
        pos2(rect.left() + rect.width() * 0.45, rect.top() + rect.height() * 0.32),
    );
    painter.rect_stroke(tab, 1.5, s, StrokeKind::Middle);
    let body = Rect::from_min_max(
        pos2(rect.left(), rect.top() + rect.height() * 0.28),
        pos2(rect.right(), rect.bottom() - rect.height() * 0.1),
    );
    painter.rect_stroke(body, 2.0, s, StrokeKind::Middle);
}

pub(super) fn glyph_back(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.line_segment([rect.left_center(), rect.right_center()], s);
    painter.line_segment(
        [rect.left_center(), pos2(rect.left() + rect.width() * 0.4, rect.top() + rect.height() * 0.15)],
        s,
    );
    painter.line_segment(
        [rect.left_center(), pos2(rect.left() + rect.width() * 0.4, rect.bottom() - rect.height() * 0.15)],
        s,
    );
}

pub(super) fn glyph_forward(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.line_segment([rect.left_center(), rect.right_center()], s);
    painter.line_segment(
        [rect.right_center(), pos2(rect.right() - rect.width() * 0.4, rect.top() + rect.height() * 0.15)],
        s,
    );
    painter.line_segment(
        [rect.right_center(), pos2(rect.right() - rect.width() * 0.4, rect.bottom() - rect.height() * 0.15)],
        s,
    );
}

pub(super) fn glyph_branch(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.width() * 0.14;
    let (a, b, c) = (
        pos2(rect.left() + rect.width() * 0.3, rect.top() + r),
        pos2(rect.left() + rect.width() * 0.3, rect.bottom() - r),
        pos2(rect.right() - rect.width() * 0.25, rect.top() + rect.height() * 0.3),
    );
    painter.circle_stroke(a, r, s);
    painter.circle_stroke(b, r, s);
    painter.circle_stroke(c, r, s);
    painter.line_segment([a + vec2(0.0, r), b - vec2(0.0, r)], s);
    painter.line_segment([c + vec2(0.0, r), pos2(a.x, rect.center().y + r)], s);
}

pub(super) fn glyph_graph(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let r = rect.width() * 0.13;
    let (top, bottom, side) = (at(0.28, 0.16), at(0.28, 0.84), at(0.76, 0.5));
    painter.line_segment([top + vec2(0.0, r), bottom - vec2(0.0, r)], s);
    painter.circle_filled(top, r, color);
    painter.circle_filled(bottom, r, color);
    painter.circle_stroke(side, r, s);
    let turn = [at(0.28, 0.3), at(0.5, 0.3), at(0.76, 0.3), side - vec2(0.0, r)];
    painter.add(egui::Shape::line(turn.to_vec(), s));
}

pub(super) fn glyph_git_workspace(painter: &egui::Painter, rect: Rect, color: Color32, accent: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let r = rect.width() * 0.15;
    let (top, bottom, right) = (at(0.19, 0.17), at(0.19, 0.81), at(0.83, 0.81));
    painter.line_segment([top + vec2(0.0, r), bottom - vec2(0.0, r)], s);
    painter.circle_stroke(top, r, s);
    painter.circle_stroke(bottom, r, s);
    painter.circle(right, r, accent, s);
    let bend = rect.width() * 0.17;
    let corner = at(0.83, 0.17);
    let mut path = vec![right - vec2(0.0, r), corner + vec2(0.0, bend)];
    for step in 1..=6 {
        let angle = std::f32::consts::FRAC_PI_2 * step as f32 / 6.0;
        path.push(corner + vec2(-bend + bend * angle.cos(), bend - bend * angle.sin()));
    }
    let tip = at(0.5, 0.17);
    path.push(tip);
    painter.add(egui::Shape::line(path, s));
    let arm = rect.width() * 0.11;
    painter.add(egui::Shape::line(vec![tip + vec2(arm, -arm), tip, tip + vec2(arm, arm)], s));
}

pub(super) fn glyph_run(painter: &egui::Painter, rect: Rect, color: Color32) {
    let points = vec![
        pos2(rect.left() + rect.width() * 0.22, rect.top()),
        pos2(rect.right(), rect.center().y),
        pos2(rect.left() + rect.width() * 0.22, rect.bottom()),
    ];
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

pub(super) fn glyph_run_ring(painter: &egui::Painter, rect: Rect, color: Color32) {
    let width = (rect.width() * 0.09).max(1.0);
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5 - width / 2.0;
    painter.circle_stroke(center, radius, Stroke::new(width, color));
    let points = vec![
        pos2(center.x - radius * 0.3, center.y - radius * 0.46),
        pos2(center.x + radius * 0.5, center.y),
        pos2(center.x - radius * 0.3, center.y + radius * 0.46),
    ];
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

pub(super) fn glyph_bulb(painter: &egui::Painter, rect: Rect, color: Color32) {
    let radius = rect.width() * 0.3;
    let center = pos2(rect.center().x, rect.top() + rect.height() * 0.38);
    painter.circle_filled(center, radius, color);
    let neck = Rect::from_min_max(
        pos2(center.x - radius * 0.55, center.y + radius * 0.5),
        pos2(center.x + radius * 0.55, rect.bottom() - rect.height() * 0.2),
    );
    painter.rect_filled(neck, 1.0, color);
    let base = rect.bottom() - rect.height() * 0.08;
    painter.line_segment([pos2(neck.left() + 0.5, base), pos2(neck.right() - 0.5, base)], stroke(rect, color));
}

pub(super) fn glyph_more(painter: &egui::Painter, rect: Rect, color: Color32) {
    for dy in [-0.36, 0.0, 0.36] {
        painter.circle_filled(rect.center() + vec2(0.0, rect.height() * dy), rect.width() * 0.09, color);
    }
}

pub(super) fn glyph_more_horizontal(painter: &egui::Painter, rect: Rect, color: Color32) {
    for dx in [-0.36, 0.0, 0.36] {
        painter.circle_filled(rect.center() + vec2(rect.width() * dx, 0.0), rect.width() * 0.09, color);
    }
}

pub(super) fn glyph_search(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(rect.center() - vec2(rect.width() * 0.1, rect.height() * 0.1), rect.width() * 0.33, s);
    painter.line_segment([rect.center() + vec2(rect.width() * 0.14, rect.height() * 0.14), rect.right_bottom()], s);
}

pub(super) fn glyph_settings(painter: &egui::Painter, rect: Rect, color: Color32) {
    crate::icons::gear(painter, rect, color);
}

pub(super) fn glyph_commit(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(rect.center(), rect.width() * 0.22, s);
    painter.line_segment([rect.left_center(), rect.center() - vec2(rect.width() * 0.22, 0.0)], s);
    painter.line_segment([rect.center() + vec2(rect.width() * 0.22, 0.0), rect.right_center()], s);
}

pub(super) fn glyph_structure(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    for (index, dy) in [0.15, 0.5, 0.85].into_iter().enumerate() {
        let y = rect.top() + rect.height() * dy;
        let left = rect.left() + if index == 0 { 0.0 } else { rect.width() * 0.3 };
        painter.circle_filled(pos2(left + rect.width() * 0.06, y), rect.width() * 0.07, color);
        painter.line_segment([pos2(left + rect.width() * 0.22, y), pos2(rect.right(), y)], s);
    }
}

pub(super) fn glyph_problems(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(rect.center(), rect.width() * 0.45, s);
    painter.line_segment(
        [rect.center() - vec2(0.0, rect.height() * 0.22), rect.center() + vec2(0.0, rect.height() * 0.06)],
        s,
    );
    painter.circle_filled(rect.center() + vec2(0.0, rect.height() * 0.22), rect.width() * 0.06, color);
}

pub(super) fn glyph_terminal(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.rect_stroke(rect, 2.0, s, StrokeKind::Middle);
    let (a, b) = (rect.left() + rect.width() * 0.22, rect.left() + rect.width() * 0.42);
    painter.line_segment([pos2(a, rect.top() + rect.height() * 0.3), pos2(b, rect.center().y)], s);
    painter.line_segment([pos2(b, rect.center().y), pos2(a, rect.bottom() - rect.height() * 0.3)], s);
    painter.line_segment(
        [
            pos2(rect.left() + rect.width() * 0.5, rect.bottom() - rect.height() * 0.28),
            pos2(rect.right() - rect.width() * 0.2, rect.bottom() - rect.height() * 0.28),
        ],
        s,
    );
}

pub(super) fn glyph_run_window(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(rect.center(), rect.width() * 0.45, s);
    glyph_run(
        painter,
        Rect::from_center_size(rect.center() + vec2(rect.width() * 0.04, 0.0), rect.size() * 0.42),
        color,
    );
}

pub(super) fn glyph_bell(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let dome = Rect::from_min_max(
        pos2(rect.left() + rect.width() * 0.18, rect.top() + rect.height() * 0.08),
        pos2(rect.right() - rect.width() * 0.18, rect.bottom() - rect.height() * 0.22),
    );
    painter.rect_stroke(dome, CornerRadius { nw: 8, ne: 8, sw: 1, se: 1 }, s, StrokeKind::Middle);
    painter.line_segment([pos2(rect.left(), dome.bottom()), pos2(rect.right(), dome.bottom())], s);
    painter.circle_filled(pos2(rect.center().x, rect.bottom() - rect.height() * 0.06), rect.width() * 0.08, color);
}

pub(super) fn glyph_database(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let top = Rect::from_min_max(rect.left_top(), pos2(rect.right(), rect.top() + rect.height() * 0.3));
    painter.rect_stroke(top, rect.width() * 0.4, s, StrokeKind::Middle);
    painter
        .line_segment([pos2(rect.left(), top.center().y), pos2(rect.left(), rect.bottom() - rect.height() * 0.15)], s);
    painter.line_segment(
        [pos2(rect.right(), top.center().y), pos2(rect.right(), rect.bottom() - rect.height() * 0.15)],
        s,
    );
    let bottom = Rect::from_min_max(pos2(rect.left(), rect.bottom() - rect.height() * 0.3), rect.right_bottom());
    painter.rect_stroke(bottom, rect.width() * 0.4, s, StrokeKind::Middle);
}

pub(super) fn glyph_services(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let half = rect.height() * 0.42;
    for top in [rect.top(), rect.bottom() - half] {
        let unit = Rect::from_min_max(pos2(rect.left(), top), pos2(rect.right(), top + half));
        painter.rect_stroke(unit, rect.width() * 0.12, s, StrokeKind::Middle);
        painter.circle_filled(pos2(unit.left() + unit.width() * 0.25, unit.center().y), rect.width() * 0.07, color);
    }
}

pub(super) fn glyph_minus(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.line_segment([rect.left_center(), rect.right_center()], stroke(rect, color));
}

pub(super) fn glyph_collapse(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let mid = rect.center().y;
    painter.line_segment(
        [pos2(rect.left() + rect.width() * 0.2, rect.top()), pos2(rect.center().x, mid - rect.height() * 0.12)],
        s,
    );
    painter.line_segment(
        [pos2(rect.center().x, mid - rect.height() * 0.12), pos2(rect.right() - rect.width() * 0.2, rect.top())],
        s,
    );
    painter.line_segment(
        [pos2(rect.left() + rect.width() * 0.2, rect.bottom()), pos2(rect.center().x, mid + rect.height() * 0.12)],
        s,
    );
    painter.line_segment(
        [pos2(rect.center().x, mid + rect.height() * 0.12), pos2(rect.right() - rect.width() * 0.2, rect.bottom())],
        s,
    );
}

pub(super) fn glyph_locate(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(rect.center(), rect.width() * 0.36, s);
    painter.circle_filled(rect.center(), rect.width() * 0.1, color);
    for (from, to) in [
        (rect.center_top(), 0.14),
        (rect.center_bottom(), -0.14),
        (rect.left_center(), 0.14),
        (rect.right_center(), -0.14),
    ] {
        let toward =
            if from.x == rect.center().x { vec2(0.0, rect.height() * to) } else { vec2(rect.width() * to, 0.0) };
        painter.line_segment([from, from + toward], s);
    }
}

pub(super) fn glyph_square(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_stroke(rect, 2.0, stroke(rect, color), StrokeKind::Middle);
}

pub(super) fn glyph_sort_alpha(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    for (index, length) in [0.3, 0.45, 0.6].into_iter().enumerate() {
        let y = rect.top() + rect.height() * (0.15 + 0.35 * index as f32);
        painter.line_segment([pos2(rect.left(), y), pos2(rect.left() + rect.width() * length, y)], s);
    }
    let x = rect.right() - rect.width() * 0.18;
    let tip = pos2(x, rect.bottom());
    painter.line_segment([pos2(x, rect.top()), tip], s);
    painter.line_segment([tip, tip + vec2(-rect.width() * 0.16, -rect.height() * 0.2)], s);
    painter.line_segment([tip, tip + vec2(rect.width() * 0.16, -rect.height() * 0.2)], s);
}

pub(super) fn glyph_fields(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let owner = Rect::from_min_size(rect.left_top(), rect.size() * 0.34);
    painter.rect_stroke(owner, 1.5, s, StrokeKind::Middle);
    for dy in [0.6, 0.9] {
        let y = rect.top() + rect.height() * dy;
        let left = rect.left() + rect.width() * 0.32;
        painter.circle_stroke(pos2(left + rect.width() * 0.07, y), rect.width() * 0.08, s);
        painter.line_segment([pos2(left + rect.width() * 0.25, y), pos2(rect.right(), y)], s);
    }
}

fn symbol_look(kind: SymbolKind, p: &IdePalette) -> (Color32, &'static str, bool) {
    match kind {
        SymbolKind::Struct => (p.semantic_type, "S", false),
        SymbolKind::Interface => (p.link, "I", false),
        SymbolKind::Type => (p.lifetime, "T", false),
        SymbolKind::Function => (p.function, "F", true),
        SymbolKind::Method => (p.keyword, "M", true),
        SymbolKind::Field => (p.constant, "f", true),
        SymbolKind::Constant => (p.number, "C", true),
        SymbolKind::Variable => (p.string, "V", true),
        SymbolKind::Package => (p.package, "P", false),
        SymbolKind::Module => (p.attribute, "M", false),
        SymbolKind::Class => (p.semantic_type, "C", false),
        SymbolKind::Element => (p.keyword, "E", false),
        SymbolKind::Rule => (p.attribute, "R", false),
        SymbolKind::Property => (p.constant, "p", true),
        SymbolKind::Key => (p.string, "K", true),
        SymbolKind::Other => (p.text_secondary, "", true),
    }
}

pub(super) fn symbol_badge(painter: &egui::Painter, rect: Rect, kind: SymbolKind, p: &IdePalette) {
    let (color, letter, round) = symbol_look(kind, p);
    if round {
        painter.circle_filled(rect.center(), rect.width() / 2.0, color);
    } else {
        painter.rect_filled(rect, rect.width() * 0.22, color);
    }
    if letter.is_empty() {
        painter.circle_filled(rect.center(), rect.width() * 0.14, p.island);
    } else {
        painter.text(rect.center(), Align2::CENTER_CENTER, letter, theme::bold(rect.height() * 0.62), p.island);
    }
}

pub(super) fn glyph_expand(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (left, right, center) = (rect.left() + rect.width() * 0.2, rect.right() - rect.width() * 0.2, rect.center().x);
    let top = rect.top() + rect.height() * 0.12;
    let bottom = rect.bottom() - rect.height() * 0.12;
    let gap = rect.height() * 0.3;
    painter.line_segment([pos2(left, top + gap), pos2(center, top)], s);
    painter.line_segment([pos2(center, top), pos2(right, top + gap)], s);
    painter.line_segment([pos2(left, bottom - gap), pos2(center, bottom)], s);
    painter.line_segment([pos2(center, bottom), pos2(right, bottom - gap)], s);
}

pub(super) fn glyph_plus_menu(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let plus = Rect::from_min_size(rect.min, rect.size() * 0.82);
    painter.line_segment([plus.left_center(), plus.right_center()], s);
    painter.line_segment([plus.center_top(), plus.center_bottom()], s);
    let corner = rect.right_bottom();
    let size = rect.width() * 0.28;
    painter.add(egui::Shape::convex_polygon(
        vec![corner, corner - vec2(size, 0.0), corner - vec2(0.0, size)],
        color,
        Stroke::NONE,
    ));
}

fn arc(center: egui::Pos2, radius: f32, from: f32, to: f32) -> Vec<egui::Pos2> {
    let steps = 24;
    (0..=steps)
        .map(|step| {
            let angle = from + (to - from) * step as f32 / steps as f32;
            center + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

pub(super) fn glyph_mute_breakpoints(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let center = rect.center();
    let radius = rect.width() * 0.32;
    painter.circle_stroke(center, radius, s);
    let arm = rect.width() * 0.42;
    painter.line_segment([center + vec2(-arm, arm), center + vec2(arm, -arm)], s);
}

pub(super) fn glyph_stop(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.rect_filled(rect.shrink(rect.width() * 0.12), rect.width() * 0.14, color);
}

pub(super) fn glyph_rerun(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let radius = rect.width() * 0.4;
    let (from, to) = (-std::f32::consts::FRAC_PI_2 + 0.5, std::f32::consts::PI * 1.5 - 0.2);
    let points = arc(rect.center(), radius, from, to);
    let tip = *points.last().unwrap_or(&rect.center());
    painter.add(egui::Shape::line(points, s));
    let size = rect.width() * 0.26;
    painter.add(egui::Shape::convex_polygon(
        vec![tip + vec2(-size * 0.2, -size), tip + vec2(size, -size * 0.1), tip + vec2(-size * 0.1, size * 0.6)],
        color,
        Stroke::NONE,
    ));
}

pub(super) fn glyph_rerun_failed(painter: &egui::Painter, rect: Rect, color: Color32) {
    glyph_rerun(painter, rect, color);
    let s = stroke(rect, color);
    let x = rect.center().x;
    painter.line_segment([pos2(x, rect.top() + rect.height() * 0.32), pos2(x, rect.top() + rect.height() * 0.56)], s);
    painter.circle_filled(pos2(x, rect.top() + rect.height() * 0.68), s.width * 0.6, color);
}

pub(super) fn glyph_test_history(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let center = rect.center();
    painter.circle_stroke(center, rect.width() * 0.38, s);
    painter.line_segment([center, center - vec2(0.0, rect.height() * 0.24)], s);
    painter.line_segment([center, center + vec2(rect.width() * 0.18, 0.0)], s);
}

pub(super) fn glyph_test_passed(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = Stroke::new(stroke(rect, color).width * 1.2, color);
    let points = vec![
        pos2(rect.left() + rect.width() * 0.14, rect.top() + rect.height() * 0.52),
        pos2(rect.left() + rect.width() * 0.4, rect.top() + rect.height() * 0.78),
        pos2(rect.right() - rect.width() * 0.12, rect.top() + rect.height() * 0.24),
    ];
    painter.add(egui::Shape::line(points, s));
}

pub(super) fn glyph_test_failed(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(rect.center(), rect.width() * 0.42, s);
    let x = rect.center().x;
    painter.line_segment([pos2(x, rect.top() + rect.height() * 0.26), pos2(x, rect.top() + rect.height() * 0.56)], s);
    painter.circle_filled(pos2(x, rect.top() + rect.height() * 0.72), s.width * 0.7, color);
}

pub(super) fn glyph_test_skipped(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(rect.center(), rect.width() * 0.42, s);
    let half = rect.width() * 0.2;
    painter.line_segment([rect.center() - vec2(half, 0.0), rect.center() + vec2(half, 0.0)], s);
}

pub(super) fn glyph_test_running(painter: &egui::Painter, rect: Rect, color: Color32) {
    let points = arc(rect.center(), rect.width() * 0.4, 0.0, std::f32::consts::PI * 1.5);
    painter.add(egui::Shape::line(points, stroke(rect, color)));
}

pub(super) fn glyph_test_terminated(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.circle_stroke(rect.center(), rect.width() * 0.42, stroke(rect, color));
    painter.rect_filled(Rect::from_center_size(rect.center(), rect.size() * 0.32), 1.0, color);
}

pub(super) fn glyph_show_passed(painter: &egui::Painter, rect: Rect, color: Color32) {
    painter.circle_stroke(rect.center(), rect.width() * 0.45, stroke(rect, color));
    glyph_test_passed(painter, rect.shrink(rect.width() * 0.24), color);
}

pub(super) fn glyph_show_ignored(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let radius = rect.width() * 0.42;
    painter.circle_stroke(rect.center(), radius, s);
    let along = vec2(radius, -radius) * std::f32::consts::FRAC_1_SQRT_2;
    painter.line_segment([rect.center() - along, rect.center() + along], s);
}

pub(super) fn glyph_bug(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let body = Rect::from_center_size(pos2(rect.center().x, rect.top() + h * 0.62), vec2(w * 0.5, h * 0.62));
    painter.rect_stroke(body, w * 0.25, s, StrokeKind::Middle);
    painter.line_segment([pos2(body.center().x, body.top() + h * 0.1), pos2(body.center().x, body.bottom())], s);
    let head = pos2(rect.center().x, body.top() - h * 0.04);
    painter.circle_filled(head, w * 0.13, color);
    for dx in [-1.0, 1.0] {
        let x = rect.center().x + dx * w * 0.08;
        let tip = pos2(rect.center().x + dx * w * 0.28, rect.top() + h * 0.04);
        painter.line_segment([pos2(x, head.y - w * 0.08), tip], s);
    }
    for row in 0..3 {
        let y = body.top() + body.height() * (0.25 + row as f32 * 0.27);
        let out = h * (row as f32 - 1.0) * 0.08;
        painter.line_segment([pos2(body.left(), y), pos2(rect.left() + w * 0.06, y + out)], s);
        painter.line_segment([pos2(body.right(), y), pos2(rect.right() - w * 0.06, y + out)], s);
    }
}

pub(super) fn glyph_resume(painter: &egui::Painter, rect: Rect, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let bar = Rect::from_min_max(
        pos2(rect.left() + w * 0.08, rect.top() + h * 0.12),
        pos2(rect.left() + w * 0.24, rect.bottom() - h * 0.12),
    );
    painter.rect_filled(bar, w * 0.04, color);
    let points = vec![
        pos2(rect.left() + w * 0.4, rect.top() + h * 0.12),
        pos2(rect.right() - w * 0.04, rect.center().y),
        pos2(rect.left() + w * 0.4, rect.bottom() - h * 0.12),
    ];
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
}

pub(super) fn glyph_pause(painter: &egui::Painter, rect: Rect, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    for left in [0.2, 0.6] {
        let bar = Rect::from_min_max(
            pos2(rect.left() + w * left, rect.top() + h * 0.14),
            pos2(rect.left() + w * (left + 0.2), rect.bottom() - h * 0.14),
        );
        painter.rect_filled(bar, w * 0.05, color);
    }
}

fn arrow_head(painter: &egui::Painter, tip: egui::Pos2, direction: egui::Vec2, size: f32, color: Color32) {
    let back = tip - direction * size;
    let side = vec2(-direction.y, direction.x) * size * 0.6;
    painter.add(egui::Shape::convex_polygon(vec![tip, back + side, back - side], color, Stroke::NONE));
}

pub(super) fn glyph_step_over(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let center = pos2(rect.center().x, rect.top() + h * 0.55);
    let points = arc(center, w * 0.36, std::f32::consts::PI, std::f32::consts::PI * 1.9);
    let tip = *points.last().unwrap_or(&center);
    painter.add(egui::Shape::line(points, s));
    arrow_head(painter, tip + vec2(w * 0.03, h * 0.1), vec2(0.3, 0.95).normalized(), w * 0.22, color);
    painter.circle_filled(pos2(rect.center().x, rect.bottom() - h * 0.1), w * 0.1, color);
}

pub(super) fn glyph_step_into(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let x = rect.center().x;
    let tip = pos2(x, rect.top() + h * 0.68);
    painter.line_segment([pos2(x, rect.top() + h * 0.04), tip - vec2(0.0, h * 0.12)], s);
    arrow_head(painter, tip, vec2(0.0, 1.0), w * 0.26, color);
    painter.circle_filled(pos2(x, rect.bottom() - h * 0.1), w * 0.1, color);
}

pub(super) fn glyph_step_out(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let x = rect.center().x;
    let tip = pos2(x, rect.top() + h * 0.04);
    painter.line_segment([tip + vec2(0.0, h * 0.12), pos2(x, rect.top() + h * 0.66)], s);
    arrow_head(painter, tip, vec2(0.0, -1.0), w * 0.26, color);
    painter.circle_filled(pos2(x, rect.bottom() - h * 0.1), w * 0.1, color);
}

pub(super) fn glyph_reset_frame(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    for (index, top) in [0.52, 0.76].into_iter().enumerate() {
        let bar = Rect::from_min_max(
            pos2(rect.left() + w * 0.38, rect.top() + h * top),
            pos2(rect.right() - w * 0.06, rect.top() + h * (top + 0.16)),
        );
        if index == 0 {
            painter.rect_stroke(bar, 1.5, s, StrokeKind::Middle);
        } else {
            painter.rect_filled(bar, 1.5, color);
        }
    }
    let x = rect.left() + w * 0.18;
    let tip = pos2(x, rect.top() + h * 0.06);
    painter.line_segment([tip + vec2(0.0, h * 0.14), pos2(x, rect.top() + h * 0.6)], s);
    painter.line_segment([pos2(x, rect.top() + h * 0.6), pos2(rect.left() + w * 0.38, rect.top() + h * 0.6)], s);
    arrow_head(painter, tip, vec2(0.0, -1.0), w * 0.24, color);
}

pub(super) fn glyph_run_to_cursor(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let y = rect.center().y;
    let tip = pos2(rect.left() + w * 0.62, y);
    painter.line_segment([pos2(rect.left() + w * 0.04, y), tip - vec2(w * 0.12, 0.0)], s);
    arrow_head(painter, tip, vec2(1.0, 0.0), w * 0.24, color);
    let x = rect.right() - w * 0.14;
    painter.line_segment([pos2(x, rect.top() + h * 0.1), pos2(x, rect.bottom() - h * 0.1)], s);
    let serif = w * 0.1;
    for y in [rect.top() + h * 0.1, rect.bottom() - h * 0.1] {
        painter.line_segment([pos2(x - serif, y), pos2(x + serif, y)], s);
    }
}

fn diagonal_arrow(painter: &egui::Painter, rect: Rect, color: Color32, from: egui::Pos2, to: egui::Pos2) {
    let s = stroke(rect, color);
    painter.line_segment([from, to], s);
    let head = rect.width() * 0.55;
    let back = from - to;
    painter.line_segment([to, to + vec2(back.x.signum() * head, 0.0)], s);
    painter.line_segment([to, to + vec2(0.0, back.y.signum() * head)], s);
}

pub(super) fn glyph_incoming(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.1);
    diagonal_arrow(painter, r, color, r.right_top(), r.left_bottom());
}

pub(super) fn glyph_outgoing(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = rect.shrink(rect.width() * 0.1);
    diagonal_arrow(painter, r, color, r.left_bottom(), r.right_top());
}

pub(super) fn glyph_plus(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.06);
    painter.line_segment([r.left_center(), r.right_center()], s);
    painter.line_segment([r.center_top(), r.center_bottom()], s);
}

pub(super) fn glyph_collapse_all(painter: &egui::Painter, rect: Rect, color: Color32) {
    let half = rect.width() * 0.42;
    let gap = rect.width() * 0.06;
    let c = rect.center();
    let upper = Rect::from_min_max(pos2(c.x + gap, c.y - gap - half), pos2(c.x + gap + half, c.y - gap));
    let lower = Rect::from_min_max(pos2(c.x - gap - half, c.y + gap), pos2(c.x - gap, c.y + gap + half));
    diagonal_arrow(painter, upper, color, upper.right_top(), upper.left_bottom());
    diagonal_arrow(painter, lower, color, lower.left_bottom(), lower.right_top());
}

pub(super) fn glyph_expand_all(painter: &egui::Painter, rect: Rect, color: Color32) {
    let half = rect.width() * 0.42;
    let gap = rect.width() * 0.06;
    let c = rect.center();
    let upper = Rect::from_min_max(pos2(c.x + gap, c.y - gap - half), pos2(c.x + gap + half, c.y - gap));
    let lower = Rect::from_min_max(pos2(c.x - gap - half, c.y + gap), pos2(c.x - gap, c.y + gap + half));
    diagonal_arrow(painter, upper, color, upper.left_bottom(), upper.right_top());
    diagonal_arrow(painter, lower, color, lower.right_top(), lower.left_bottom());
}

pub(super) fn glyph_label(painter: &egui::Painter, rect: Rect, color: Color32, hole: Color32) {
    let shape = [(0.0, 0.5), (0.36, 0.14), (1.0, 0.14), (1.0, 0.86), (0.36, 0.86)];
    let (sin, cos) = std::f32::consts::FRAC_PI_4.sin_cos();
    let size = rect.width() * 0.92;
    let turn = |(x, y): (f32, f32)| {
        let (dx, dy) = ((x - 0.5) * size, (y - 0.5) * size);
        rect.center() + vec2(dx * cos + dy * sin, -dx * sin + dy * cos)
    };
    let points: Vec<egui::Pos2> = shape.iter().copied().map(turn).collect();
    painter.add(egui::Shape::convex_polygon(points, color, Stroke::NONE));
    painter.circle_filled(turn((0.34, 0.5)), rect.width() * 0.09, hole);
}

pub(super) fn glyph_bookmark(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let outline = vec![at(0.22, 0.08), at(0.78, 0.08), at(0.78, 0.92), at(0.5, 0.68), at(0.22, 0.92)];
    painter.add(egui::Shape::closed_line(outline, s));
}

pub(super) fn glyph_star(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center() + vec2(0.0, rect.height() * 0.04);
    let (outer, inner) = (rect.width() * 0.5, rect.width() * 0.21);
    let points: Vec<egui::Pos2> = (0..10)
        .map(|index| {
            let angle = -std::f32::consts::FRAC_PI_2 + index as f32 * std::f32::consts::PI / 5.0;
            let radius = if index % 2 == 0 { outer } else { inner };
            c + vec2(angle.cos() * radius, angle.sin() * radius)
        })
        .collect();
    let centre: Vec<egui::Pos2> = points.iter().skip(1).step_by(2).copied().collect();
    painter.add(egui::Shape::convex_polygon(centre, color, Stroke::NONE));
    for index in (0..10).step_by(2) {
        let tip = points[index];
        let (left, right) = (points[(index + 9) % 10], points[(index + 1) % 10]);
        painter.add(egui::Shape::convex_polygon(vec![tip, right, left], color, Stroke::NONE));
    }
}

pub(super) fn glyph_trash(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let top = rect.top() + rect.height() * 0.22;
    painter
        .line_segment([pos2(rect.left() + rect.width() * 0.08, top), pos2(rect.right() - rect.width() * 0.08, top)], s);
    let handle = Rect::from_min_max(
        pos2(rect.left() + rect.width() * 0.36, rect.top() + rect.height() * 0.06),
        pos2(rect.right() - rect.width() * 0.36, top),
    );
    painter.rect_stroke(handle, 1.0, s, StrokeKind::Middle);
    let body = Rect::from_min_max(
        pos2(rect.left() + rect.width() * 0.2, top),
        pos2(rect.right() - rect.width() * 0.2, rect.bottom() - rect.height() * 0.04),
    );
    painter.rect_stroke(body, 1.5, s, StrokeKind::Middle);
    for x in [0.42, 0.58] {
        let x = rect.left() + rect.width() * x;
        painter.line_segment([pos2(x, top + rect.height() * 0.18), pos2(x, body.bottom() - rect.height() * 0.14)], s);
    }
}

pub(super) fn glyph_compare(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let head = rect.width() * 0.22;
    for (y, rightward) in [(0.3, true), (0.7, false)] {
        let y = rect.top() + rect.height() * y;
        let (from, to) = if rightward {
            (pos2(rect.left() + rect.width() * 0.08, y), pos2(rect.right() - rect.width() * 0.08, y))
        } else {
            (pos2(rect.right() - rect.width() * 0.08, y), pos2(rect.left() + rect.width() * 0.08, y))
        };
        painter.line_segment([from, to], s);
        let back = if rightward { -head } else { head };
        painter.line_segment([to, to + vec2(back, -head * 0.8)], s);
        painter.line_segment([to, to + vec2(back, head * 0.8)], s);
    }
}

pub(super) fn glyph_fetch(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let x = rect.center().x;
    let tip = pos2(x, rect.bottom() - rect.height() * 0.26);
    painter.line_segment([pos2(x, rect.top() + rect.height() * 0.06), tip], s);
    let head = rect.width() * 0.26;
    painter.line_segment([tip, tip + vec2(-head, -head)], s);
    painter.line_segment([tip, tip + vec2(head, -head)], s);
    let y = rect.bottom() - rect.height() * 0.06;
    painter.line_segment([pos2(rect.left() + rect.width() * 0.12, y), pos2(rect.right() - rect.width() * 0.12, y)], s);
}

fn tray(painter: &egui::Painter, rect: Rect, color: Color32, into: bool) {
    let s = stroke(rect, color);
    let (left, right) = (rect.left() + rect.width() * 0.06, rect.right() - rect.width() * 0.06);
    let (rim, bottom) = (rect.top() + rect.height() * 0.5, rect.bottom() - rect.height() * 0.08);
    painter.add(egui::Shape::line(vec![pos2(left, rim), pos2(left, bottom), pos2(right, bottom), pos2(right, rim)], s));
    let x = rect.center().x;
    let (top, low) = (rect.top() + rect.height() * 0.04, rect.top() + rect.height() * 0.66);
    let (from, to) = if into { (pos2(x, top), pos2(x, low)) } else { (pos2(x, low), pos2(x, top)) };
    painter.line_segment([from, to], s);
    let head = rect.width() * 0.22;
    let back = if into { -head } else { head };
    painter.line_segment([to, to + vec2(-head, back)], s);
    painter.line_segment([to, to + vec2(head, back)], s);
}

pub(super) fn glyph_stash(painter: &egui::Painter, rect: Rect, color: Color32) {
    tray(painter, rect, color, true);
}

pub(super) fn glyph_pop(painter: &egui::Painter, rect: Rect, color: Color32) {
    tray(painter, rect, color, false);
}

pub(super) fn glyph_refresh(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let radius = rect.width() * 0.38;
    let size = rect.width() * 0.2;
    for start in [-0.3_f32, std::f32::consts::PI - 0.3] {
        let points = arc(rect.center(), radius, start, start + 2.4);
        let tip = *points.last().unwrap_or(&rect.center());
        let before = points[points.len().saturating_sub(3)];
        painter.add(egui::Shape::line(points, s));
        let direction = (tip - before).normalized();
        let side = vec2(-direction.y, direction.x);
        painter.add(egui::Shape::convex_polygon(
            vec![tip + direction * size, tip + side * size * 0.8, tip - side * size * 0.8],
            color,
            Stroke::NONE,
        ));
    }
}

pub(super) fn glyph_cherry_pick(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.width() * 0.19;
    let left = pos2(rect.left() + rect.width() * 0.27, rect.bottom() - r - rect.height() * 0.04);
    let right = pos2(rect.right() - rect.width() * 0.25, rect.bottom() - r - rect.height() * 0.16);
    painter.circle_stroke(left, r, s);
    painter.circle_stroke(right, r, s);
    let top = pos2(rect.center().x + rect.width() * 0.1, rect.top() + rect.height() * 0.06);
    painter.line_segment([left - vec2(0.0, r), top], s);
    painter.line_segment([right - vec2(0.0, r), top], s);
}

pub(super) fn glyph_eye(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let c = rect.center();
    let (half, lid) = (rect.width() * 0.48, rect.height() * 0.62);
    let curve = |sign: f32| {
        egui::epaint::QuadraticBezierShape::from_points_stroke(
            [c - vec2(half, 0.0), c + vec2(0.0, sign * lid), c + vec2(half, 0.0)],
            false,
            Color32::TRANSPARENT,
            s,
        )
    };
    painter.add(curve(-1.0));
    painter.add(curve(1.0));
    painter.circle_stroke(c, rect.width() * 0.15, s);
}

pub(super) fn glyph_eye_off(painter: &egui::Painter, rect: Rect, color: Color32) {
    glyph_eye(painter, rect, color);
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.06);
    painter.line_segment([r.left_bottom(), r.right_top()], s);
}

pub(super) fn glyph_solo(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    painter.circle_stroke(rect.center(), rect.width() * 0.42, s);
    painter.circle_filled(rect.center(), rect.width() * 0.18, color);
}

pub(super) fn glyph_sort(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let x = rect.left() + rect.width() * 0.2;
    let tip = pos2(x, rect.bottom());
    painter.line_segment([pos2(x, rect.top()), tip], s);
    painter.line_segment([tip, tip + vec2(-rect.width() * 0.16, -rect.height() * 0.2)], s);
    painter.line_segment([tip, tip + vec2(rect.width() * 0.16, -rect.height() * 0.2)], s);
    for (index, length) in [0.55, 0.42, 0.28].into_iter().enumerate() {
        let y = rect.top() + rect.height() * (0.15 + 0.35 * index as f32);
        let start = rect.left() + rect.width() * 0.45;
        painter.line_segment([pos2(start, y), pos2(start + rect.width() * length, y)], s);
    }
}

pub(super) fn glyph_diff(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width() * 0.34;
    let left = Rect::from_min_size(rect.left_top() + vec2(0.0, rect.height() * 0.08), vec2(w, rect.height() * 0.84));
    let right =
        Rect::from_min_size(pos2(rect.right() - w, rect.top() + rect.height() * 0.08), vec2(w, rect.height() * 0.84));
    painter.rect_stroke(left, 1.5, s, StrokeKind::Middle);
    painter.rect_stroke(right, 1.5, s, StrokeKind::Middle);
    let y = rect.center().y;
    let tip = pos2(right.left() - rect.width() * 0.04, y);
    painter.line_segment([pos2(left.right() + rect.width() * 0.04, y), tip], s);
    painter.line_segment([tip, tip + vec2(-rect.width() * 0.1, -rect.height() * 0.1)], s);
    painter.line_segment([tip, tip + vec2(-rect.width() * 0.1, rect.height() * 0.1)], s);
}

pub(super) fn glyph_group(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let folder = Rect::from_min_size(rect.left_top(), rect.size() * 0.5);
    glyph_folder(painter, folder, color);
    let x = rect.left() + rect.width() * 0.2;
    let (first, second) = (rect.top() + rect.height() * 0.68, rect.bottom() - rect.height() * 0.06);
    painter.line_segment([pos2(x, folder.bottom()), pos2(x, second)], s);
    for y in [first, second] {
        painter.line_segment([pos2(x, y), pos2(rect.left() + rect.width() * 0.42, y)], s);
        painter.line_segment([pos2(rect.left() + rect.width() * 0.55, y), pos2(rect.right(), y)], s);
    }
}

pub(super) fn glyph_cloud(painter: &egui::Painter, rect: Rect, color: Color32) {
    use std::f32::consts::PI;
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let bottom = rect.top() + h * 0.78;
    let left = pos2(rect.left() + w * 0.26, rect.top() + h * 0.58);
    let middle = pos2(rect.left() + w * 0.52, rect.top() + h * 0.46);
    let right = pos2(rect.left() + w * 0.76, rect.top() + h * 0.6);
    let mut points = arc(left, h * 0.2, PI * 0.5, PI * 1.45);
    points.extend(arc(middle, h * 0.27, PI * 1.08, PI * 1.92));
    points.extend(arc(right, h * 0.18, PI * 1.55, PI * 2.5));
    points.push(pos2(left.x, bottom));
    painter.add(egui::Shape::closed_line(points, s));
}

fn turn_arrow(painter: &egui::Painter, rect: Rect, color: Color32, back: bool) {
    use std::f32::consts::PI;
    let s = stroke(rect, color);
    let radius = rect.width() * 0.32;
    let center = rect.center() + vec2(0.0, rect.height() * 0.06);
    let (from, to) = if back { (PI * 0.15, -PI * 1.05) } else { (PI * 0.85, PI * 2.05) };
    let points = arc(center, radius, from, to);
    let tip = *points.last().unwrap_or(&center);
    painter.add(egui::Shape::line(points, s));
    let head = rect.width() * 0.24;
    let out = if back { -1.0 } else { 1.0 };
    painter.line_segment([tip, tip + vec2(-out * head * 0.2, -head)], s);
    painter.line_segment([tip, tip + vec2(out * head, -head * 0.15)], s);
}

pub(super) fn glyph_undo(painter: &egui::Painter, rect: Rect, color: Color32) {
    turn_arrow(painter, rect, color, true);
}

pub(super) fn glyph_redo(painter: &egui::Painter, rect: Rect, color: Color32) {
    turn_arrow(painter, rect, color, false);
}

fn arrow_and_line(painter: &egui::Painter, rect: Rect, color: Color32, down: bool) {
    let s = stroke(rect, color);
    let x = rect.center().x;
    let (top, low) = (rect.top() + rect.height() * 0.06, rect.bottom() - rect.height() * 0.26);
    let (from, tip) = if down { (pos2(x, top), pos2(x, low)) } else { (pos2(x, low), pos2(x, top)) };
    painter.line_segment([from, tip], s);
    let head = rect.width() * 0.28;
    let back = if down { -head } else { head };
    painter.line_segment([tip, tip + vec2(-head, back)], s);
    painter.line_segment([tip, tip + vec2(head, back)], s);
    let y = rect.bottom() - rect.height() * 0.06;
    painter.line_segment([pos2(rect.left() + rect.width() * 0.1, y), pos2(rect.right() - rect.width() * 0.1, y)], s);
}

pub(super) fn glyph_pull(painter: &egui::Painter, rect: Rect, color: Color32) {
    arrow_and_line(painter, rect, color, true);
}

pub(super) fn glyph_push(painter: &egui::Painter, rect: Rect, color: Color32) {
    arrow_and_line(painter, rect, color, false);
}

pub(super) fn glyph_pencil(painter: &egui::Painter, rect: Rect, color: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let along = vec2(0.62, -0.62) * w;
    let across = vec2(0.16, 0.16) * w;
    let point = pos2(rect.left() + w * 0.1, rect.bottom() - h * 0.1);
    let base = point + along * 0.3;
    let end = point + along * 1.12;
    painter.add(egui::Shape::convex_polygon(
        vec![base + across * 0.5, end + across * 0.5, end - across * 0.5, base - across * 0.5],
        color,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![point, base - across * 0.5, base + across * 0.5],
        color,
        Stroke::NONE,
    ));
}

pub(super) fn glyph_list(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    for step in 0..3 {
        let y = rect.top() + rect.height() * (0.2 + 0.3 * step as f32);
        painter.circle_filled(pos2(rect.left() + rect.width() * 0.1, y), s.width * 0.75, color);
        painter.line_segment([pos2(rect.left() + rect.width() * 0.3, y), pos2(rect.right(), y)], s);
    }
}

pub(super) fn glyph_tree(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let node =
        |x: f32, y: f32| Rect::from_min_size(pos2(rect.left() + w * x, rect.top() + h * y), vec2(w * 0.32, h * 0.24));
    let top = node(0.0, 0.04);
    painter.rect_stroke(top, 1.0, s, StrokeKind::Middle);
    let x = top.left() + w * 0.12;
    for y in [0.4, 0.72] {
        let child = node(0.62, y);
        painter.rect_stroke(child, 1.0, s, StrokeKind::Middle);
        painter.line_segment([pos2(x, top.bottom()), pos2(x, child.center().y)], s);
        painter.line_segment([pos2(x, child.center().y), child.left_center()], s);
    }
}

pub(super) fn glyph_check(painter: &egui::Painter, rect: Rect, color: Color32) {
    glyph_test_passed(painter, rect, color);
}

pub(super) fn glyph_rebase(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.width() * 0.13;
    let x = rect.left() + rect.width() * 0.24;
    let (top, middle, bottom) = (rect.top() + r + 0.5, rect.center().y + r * 0.5, rect.bottom() - r - 0.5);
    painter.circle_stroke(pos2(x, middle), r, s);
    painter.circle_stroke(pos2(x, bottom), r, s);
    painter.line_segment([pos2(x, middle + r), pos2(x, bottom - r)], s);
    painter.line_segment([pos2(x, middle - r), pos2(x, top)], s);
    let moved = pos2(rect.right() - r * 1.3, top + r * 0.4);
    painter.circle_filled(moved, r * 1.15, color);
    let tip = pos2(moved.x - r * 2.0, moved.y);
    painter.line_segment([pos2(x + r * 0.4, moved.y), tip], s);
    let head = rect.width() * 0.15;
    painter.line_segment([tip, tip + vec2(-head, -head * 0.8)], s);
    painter.line_segment([tip, tip + vec2(-head, head * 0.8)], s);
}

pub(super) fn glyph_grip(painter: &egui::Painter, rect: Rect, color: Color32) {
    let r = (rect.width() * 0.09).max(1.0);
    for column in [0.36, 0.64] {
        for row in [0.25, 0.5, 0.75] {
            let center = pos2(rect.left() + rect.width() * column, rect.top() + rect.height() * row);
            painter.circle_filled(center, r, color);
        }
    }
}

pub(super) fn glyph_worktree(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let tab = Rect::from_min_max(at(0.0, 0.36), at(0.34, 0.5));
    painter.rect_stroke(tab, 1.5, s, StrokeKind::Middle);
    let body = Rect::from_min_max(at(0.0, 0.46), at(0.66, 0.92));
    painter.rect_stroke(body, 2.0, s, StrokeKind::Middle);
    let r = rect.width() * 0.12;
    let tip = at(0.84, 0.16);
    painter.circle_stroke(tip, r, s);
    let points = vec![at(0.84, 0.16 + 0.12 + 0.02), at(0.84, 0.42), at(0.74, 0.58)];
    painter.add(egui::Shape::line(points, s));
}

pub(super) fn glyph_submodule(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let outer = rect.shrink(rect.width() * 0.08);
    painter.rect_stroke(outer, rect.width() * 0.14, s, StrokeKind::Middle);
    let inner = Rect::from_min_max(
        pos2(outer.center().x, outer.center().y),
        pos2(outer.right() - rect.width() * 0.12, outer.bottom() - rect.height() * 0.12),
    );
    painter.rect_filled(inner, rect.width() * 0.06, color);
}

pub(super) fn glyph_lock(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let body = Rect::from_min_max(
        pos2(rect.left() + rect.width() * 0.16, rect.top() + rect.height() * 0.46),
        pos2(rect.right() - rect.width() * 0.16, rect.bottom() - rect.height() * 0.06),
    );
    painter.rect_filled(body, rect.width() * 0.1, color);
    let radius = rect.width() * 0.22;
    let center = pos2(rect.center().x, body.top() - rect.height() * 0.06);
    let mut points = vec![pos2(center.x - radius, body.top())];
    points.extend(arc(center, radius, std::f32::consts::PI, std::f32::consts::TAU));
    points.push(pos2(center.x + radius, body.top()));
    painter.add(egui::Shape::line(points, s));
}

pub(super) fn glyph_cut(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let radius = rect.width() * 0.16;
    for y in [0.28, 0.72] {
        painter.circle_stroke(at(0.22, y), radius, s);
    }
    painter.line_segment([at(0.36, 0.36), at(0.94, 0.78)], s);
    painter.line_segment([at(0.36, 0.64), at(0.94, 0.22)], s);
}

pub(super) fn glyph_copy(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    let front = Rect::from_min_max(at(0.3, 0.3), at(0.9, 0.94));
    painter.rect_stroke(front, 1.5, s, StrokeKind::Middle);
    painter.add(egui::Shape::line(vec![at(0.1, 0.7), at(0.1, 0.06), at(0.66, 0.06), at(0.66, 0.18)], s));
    for y in [0.52, 0.72] {
        painter.line_segment([at(0.44, y), at(0.76, y)], s);
    }
}

pub(super) fn glyph_paste(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    painter.rect_stroke(Rect::from_min_max(at(0.14, 0.14), at(0.86, 0.96)), 1.5, s, StrokeKind::Middle);
    painter.rect_filled(Rect::from_min_max(at(0.34, 0.04), at(0.66, 0.24)), 1.0, color);
    for y in [0.48, 0.66, 0.82] {
        painter.line_segment([at(0.32, y), at(0.68, y)], s);
    }
}

pub(super) fn glyph_reformat(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    painter.line_segment([at(0.06, 0.14), at(0.94, 0.14)], s);
    painter.line_segment([at(0.44, 0.38), at(0.94, 0.38)], s);
    painter.line_segment([at(0.44, 0.62), at(0.94, 0.62)], s);
    painter.line_segment([at(0.06, 0.86), at(0.94, 0.86)], s);
    painter.add(egui::Shape::convex_polygon(vec![at(0.08, 0.32), at(0.3, 0.5), at(0.08, 0.68)], color, Stroke::NONE));
}

pub(super) fn glyph_open_in_find(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    painter.line_segment([at(0.55, 0.12), at(0.12, 0.12)], s);
    painter.line_segment([at(0.12, 0.12), at(0.12, 0.88)], s);
    painter.line_segment([at(0.12, 0.88), at(0.88, 0.88)], s);
    painter.line_segment([at(0.88, 0.88), at(0.88, 0.45)], s);
    painter.line_segment([at(0.45, 0.55), at(0.88, 0.12)], s);
    painter.line_segment([at(0.62, 0.12), at(0.88, 0.12)], s);
    painter.line_segment([at(0.88, 0.12), at(0.88, 0.38)], s);
}

pub(super) fn glyph_split_right(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    painter.rect_stroke(Rect::from_min_max(at(0.06, 0.12), at(0.94, 0.88)), 2.0, s, StrokeKind::Middle);
    painter.line_segment([at(0.5, 0.12), at(0.5, 0.88)], s);
}

pub(super) fn glyph_ignore(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let at = |x: f32, y: f32| pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * y);
    painter.circle_stroke(rect.center(), rect.width() * 0.42, s);
    painter.line_segment([at(0.21, 0.79), at(0.79, 0.21)], s);
}

pub(super) fn glyph_merge_conflict(painter: &egui::Painter, rect: Rect, color: Color32) {
    let width = rect.width() / 3.6;
    let gap = (rect.width() - 3.0 * width) / 2.0;
    for index in 0..3 {
        let left = rect.left() + index as f32 * (width + gap);
        let pane = Rect::from_min_size(pos2(left, rect.top() + 1.0), vec2(width, rect.height() - 2.0));
        if index == 1 {
            painter.rect_filled(pane, 1.5, color);
        } else {
            painter.rect_stroke(pane, 1.5, Stroke::new(1.2, color), StrokeKind::Inside);
        }
    }
}

pub(super) fn glyph_todo(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let tick = [
        pos2(rect.left(), rect.top() + h * 0.2),
        pos2(rect.left() + w * 0.1, rect.top() + h * 0.3),
        pos2(rect.left() + w * 0.28, rect.top() + h * 0.08),
    ];
    painter.line_segment([tick[0], tick[1]], s);
    painter.line_segment([tick[1], tick[2]], s);
    for (step, y) in [0.2_f32, 0.55, 0.88].into_iter().enumerate() {
        let y = rect.top() + h * y;
        if step > 0 {
            painter.circle_filled(pos2(rect.left() + w * 0.12, y), s.width * 0.8, color);
        }
        painter.line_segment([pos2(rect.left() + w * 0.42, y), pos2(rect.right(), y)], s);
    }
}

pub(super) fn glyph_arrow_up(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (top, bottom) = (rect.center_top(), rect.center_bottom());
    painter.line_segment([top, bottom], s);
    painter.line_segment([top, top + vec2(-rect.width() * 0.35, rect.height() * 0.35)], s);
    painter.line_segment([top, top + vec2(rect.width() * 0.35, rect.height() * 0.35)], s);
}

pub(super) fn glyph_arrow_down(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (top, bottom) = (rect.center_top(), rect.center_bottom());
    painter.line_segment([top, bottom], s);
    painter.line_segment([bottom, bottom + vec2(-rect.width() * 0.35, -rect.height() * 0.35)], s);
    painter.line_segment([bottom, bottom + vec2(rect.width() * 0.35, -rect.height() * 0.35)], s);
}

pub(super) fn glyph_pointer(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let at = |x: f32, y: f32| pos2(rect.left() + w * x, rect.top() + h * y);
    let outline =
        [at(0.2, 0.0), at(0.2, 0.8), at(0.4, 0.62), at(0.56, 1.0), at(0.7, 0.94), at(0.54, 0.56), at(0.8, 0.56)];
    painter.add(egui::Shape::closed_line(outline.to_vec(), s));
}

pub(super) fn glyph_globe(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let center = rect.center();
    let r = rect.width() * 0.45;
    painter.circle_stroke(center, r, s);
    painter.add(egui::Shape::ellipse_stroke(center, vec2(r * 0.42, r), s));
    painter.line_segment([pos2(center.x - r, center.y), pos2(center.x + r, center.y)], s);
}

fn node_square(painter: &egui::Painter, center: egui::Pos2, side: f32, color: Color32, rect: Rect) {
    painter.rect_stroke(Rect::from_center_size(center, vec2(side, side)), 1.0, stroke(rect, color), StrokeKind::Middle);
}

pub(super) fn glyph_hierarchy(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let side = w * 0.28;
    let root = rect.left_top() + vec2(side / 2.0 + w * 0.02, side / 2.0 + w * 0.02);
    node_square(painter, root, side, color, rect);
    let x = root.x;
    let first = pos2(rect.right() - side / 2.0 - w * 0.02, rect.top() + w * 0.5);
    let second = pos2(first.x, rect.bottom() - side / 2.0 - w * 0.02);
    painter.line_segment([pos2(x, root.y + side / 2.0), pos2(x, second.y)], s);
    for child in [first, second] {
        painter.line_segment([pos2(x, child.y), pos2(child.x - side / 2.0, child.y)], s);
        node_square(painter, child, side, color, rect);
    }
}

pub(super) fn glyph_callers(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let side = w * 0.36;
    let target = pos2(rect.right() - side / 2.0, rect.center().y);
    node_square(painter, target, side, color, rect);
    for y in [rect.top() + w * 0.12, rect.bottom() - w * 0.12] {
        let from = pos2(rect.left(), y);
        let tip = pos2(target.x - side / 2.0 - w * 0.06, target.y);
        painter.line_segment([from, tip], s);
        arrow_head(painter, tip, (tip - from).normalized(), w * 0.22, color);
    }
}

pub(super) fn glyph_callees(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let side = w * 0.36;
    let source = pos2(rect.left() + side / 2.0, rect.center().y);
    node_square(painter, source, side, color, rect);
    for y in [rect.top() + w * 0.12, rect.bottom() - w * 0.12] {
        let from = pos2(source.x + side / 2.0 + w * 0.04, source.y);
        let tip = pos2(rect.right(), y);
        painter.line_segment([from, tip], s);
        arrow_head(painter, tip, (tip - from).normalized(), w * 0.22, color);
    }
}

pub(super) fn glyph_supertypes(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let side = w * 0.34;
    let top = pos2(rect.center().x, rect.top() + side / 2.0);
    let bottom = pos2(rect.center().x, rect.bottom() - side / 2.0);
    node_square(painter, top, side, color, rect);
    node_square(painter, bottom, side, color, rect);
    let tip = pos2(top.x, top.y + side / 2.0 + w * 0.04);
    painter.line_segment([pos2(bottom.x, bottom.y - side / 2.0), tip], s);
    arrow_head(painter, tip, vec2(0.0, -1.0), w * 0.2, color);
}

pub(super) fn glyph_subtypes(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let side = w * 0.34;
    let top = pos2(rect.center().x, rect.top() + side / 2.0);
    let bottom = pos2(rect.center().x, rect.bottom() - side / 2.0);
    node_square(painter, top, side, color, rect);
    node_square(painter, bottom, side, color, rect);
    let tip = pos2(bottom.x, bottom.y - side / 2.0 - w * 0.04);
    painter.line_segment([pos2(top.x, top.y + side / 2.0), tip], s);
    arrow_head(painter, tip, vec2(0.0, 1.0), w * 0.2, color);
}

pub(super) fn glyph_pin(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let head = rect.center() + vec2(w * 0.14, -w * 0.14);
    let a = head + vec2(-w * 0.32, -w * 0.02);
    let b = head + vec2(w * 0.02, w * 0.32);
    painter.line_segment([a, b], s);
    painter.circle_stroke(head + vec2(w * 0.08, -w * 0.08), w * 0.2, s);
    painter
        .line_segment([rect.center() + vec2(-w * 0.05, w * 0.05), rect.left_bottom() + vec2(w * 0.06, -w * 0.06)], s);
}

pub(super) fn glyph_close(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.18);
    painter.line_segment([r.left_top(), r.right_bottom()], s);
    painter.line_segment([r.right_top(), r.left_bottom()], s);
}

pub(super) fn glyph_single_click(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let o = rect.left_top() + vec2(w * 0.2, w * 0.05);
    let points = vec![
        o,
        o + vec2(0.0, w * 0.72),
        o + vec2(w * 0.2, w * 0.54),
        o + vec2(w * 0.36, w * 0.88),
        o + vec2(w * 0.5, w * 0.82),
        o + vec2(w * 0.34, w * 0.48),
        o + vec2(w * 0.6, w * 0.48),
    ];
    painter.add(egui::Shape::closed_line(points, s));
}

pub(super) fn glyph_recursive(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let radius = rect.width() * 0.36;
    let points = arc(rect.center(), radius, 0.6, 0.6 + 4.8);
    let tip = *points.last().unwrap_or(&rect.center());
    let before = points[points.len().saturating_sub(3)];
    painter.add(egui::Shape::line(points, s));
    arrow_head(painter, tip, (tip - before).normalized(), rect.width() * 0.26, color);
}

pub(super) fn glyph_caffeine(painter: &egui::Painter, rect: Rect, color: Color32) {
    super::caffeine::draw_stripe_cup(painter, rect, super::caffeine::sprite::Look::Asleep, color);
}

pub(super) fn glyph_build_project(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let top = pos2(rect.center().x, rect.top() + h * 0.08);
    let left = pos2(rect.left() + w * 0.08, rect.top() + h * 0.3);
    let right = pos2(rect.right() - w * 0.08, rect.top() + h * 0.3);
    let middle = pos2(rect.center().x, rect.top() + h * 0.52);
    let bottom = pos2(rect.center().x, rect.bottom() - h * 0.06);
    let left_low = pos2(left.x, rect.bottom() - h * 0.28);
    let right_low = pos2(right.x, rect.bottom() - h * 0.28);
    painter.add(egui::Shape::closed_line(vec![top, right, right_low, bottom, left_low, left], s));
    painter.line_segment([left, middle], s);
    painter.line_segment([right, middle], s);
    painter.line_segment([middle, bottom], s);
}

pub(super) fn glyph_maven(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let h = rect.height();
    for (index, inset) in [0.32_f32, 0.16, 0.0].into_iter().enumerate() {
        let y = rect.top() + h * (0.18 + 0.32 * index as f32);
        let layer = Rect::from_min_max(
            pos2(rect.left() + rect.width() * inset * 0.5, y),
            pos2(rect.right() - rect.width() * inset * 0.5, y + h * 0.2),
        );
        painter.rect_stroke(layer, rect.width() * 0.06, s, StrokeKind::Middle);
    }
}

pub(super) fn glyph_ant(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let x = rect.center().x;
    let head = pos2(x, rect.top() + w * 0.2);
    let thorax = pos2(x, rect.top() + w * 0.45);
    let abdomen = pos2(x, rect.top() + w * 0.74);
    painter.circle_filled(head, w * 0.1, color);
    painter.circle_filled(thorax, w * 0.09, color);
    painter.circle_stroke(abdomen, w * 0.16, s);
    for (dx, dy) in [(-1.0_f32, -0.12_f32), (-1.0, 0.0), (-1.0, 0.12), (1.0, -0.12), (1.0, 0.0), (1.0, 0.12)] {
        let end = pos2(x + dx * w * 0.4, thorax.y + dy * w * 1.6);
        painter.line_segment([thorax, end], s);
    }
    for dx in [-1.0_f32, 1.0] {
        painter.line_segment([head, pos2(x + dx * w * 0.22, rect.top() + w * 0.02)], s);
    }
}

pub(super) fn glyph_dependencies(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let w = rect.width();
    let side = w * 0.34;
    let top = Rect::from_center_size(pos2(rect.center().x, rect.top() + w * 0.2), vec2(side, side));
    let left = Rect::from_center_size(pos2(rect.left() + w * 0.2, rect.top() + w * 0.8), vec2(side, side));
    let right = Rect::from_center_size(pos2(rect.right() - w * 0.2, rect.top() + w * 0.8), vec2(side, side));
    for square in [top, left, right] {
        painter.rect_stroke(square, 1, s, StrokeKind::Middle);
    }
    let middle = rect.top() + w * 0.5;
    painter.line_segment([top.center_bottom(), pos2(top.center().x, middle)], s);
    painter.line_segment([pos2(left.center().x, middle), pos2(right.center().x, middle)], s);
    painter.line_segment([pos2(left.center().x, middle), left.center_top()], s);
    painter.line_segment([pos2(right.center().x, middle), right.center_top()], s);
}

pub(super) fn glyph_npm(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.08);
    let center = r.center();
    let top = pos2(center.x, r.top());
    let bottom = pos2(center.x, r.bottom());
    let (left_top, right_top) = (r.left(), r.right());
    let upper = r.top() + r.height() * 0.25;
    let lower = r.top() + r.height() * 0.75;
    painter.add(egui::Shape::closed_line(
        vec![top, pos2(right_top, upper), pos2(right_top, lower), bottom, pos2(left_top, lower), pos2(left_top, upper)],
        s,
    ));
    painter.line_segment([pos2(left_top, upper), center], s);
    painter.line_segment([pos2(right_top, upper), center], s);
    painter.line_segment([center, bottom], s);
}

pub(super) fn glyph_filter(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.shrink(rect.width() * 0.08);
    let neck_top = r.top() + r.height() * 0.45;
    let left_neck = r.center().x - r.width() * 0.1;
    let right_neck = r.center().x + r.width() * 0.1;
    painter.add(egui::Shape::closed_line(
        vec![
            r.left_top(),
            r.right_top(),
            pos2(right_neck, neck_top),
            pos2(right_neck, r.bottom()),
            pos2(left_neck, r.bottom() - r.height() * 0.12),
            pos2(left_neck, neck_top),
        ],
        s,
    ));
}

pub(super) fn glyph_gradle(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let r = rect.width() * 0.15;
    let a = pos2(rect.left() + r * 1.1, rect.top() + r * 1.1);
    let b = pos2(rect.right() - r * 1.1, rect.top() + r * 1.1);
    let c = pos2(rect.center().x, rect.bottom() - r * 1.1);
    painter.line_segment([a, c], s);
    painter.line_segment([b, c], s);
    for point in [a, b] {
        painter.circle_stroke(point, r, s);
    }
    painter.circle_filled(c, r * 1.1, color);
}

pub(super) fn glyph_endpoints(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.42;
    painter.circle_stroke(center, radius, s);
    painter.line_segment([pos2(center.x - radius, center.y), pos2(center.x + radius, center.y)], s);
    let ellipse: Vec<egui::Pos2> = (0..=24)
        .map(|step| {
            let angle = std::f32::consts::TAU * step as f32 / 24.0;
            pos2(center.x + radius * 0.45 * angle.cos(), center.y + radius * angle.sin())
        })
        .collect();
    painter.add(egui::Shape::line(ellipse, s));
}

pub(super) fn glyph_coverage(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let shield = vec![
        pos2(rect.center().x, rect.top()),
        pos2(rect.right(), rect.top() + h * 0.18),
        pos2(rect.right() - w * 0.06, rect.top() + h * 0.6),
        pos2(rect.center().x, rect.bottom()),
        pos2(rect.left() + w * 0.06, rect.top() + h * 0.6),
        pos2(rect.left(), rect.top() + h * 0.18),
    ];
    painter.add(egui::Shape::closed_line(shield, s));
    let tick = [
        pos2(rect.left() + w * 0.3, rect.top() + h * 0.5),
        pos2(rect.left() + w * 0.45, rect.top() + h * 0.65),
        pos2(rect.left() + w * 0.72, rect.top() + h * 0.36),
    ];
    painter.line_segment([tick[0], tick[1]], s);
    painter.line_segment([tick[1], tick[2]], s);
}

pub(super) fn glyph_export(painter: &egui::Painter, rect: Rect, color: Color32) {
    let s = stroke(rect, color);
    let (w, h) = (rect.width(), rect.height());
    let tray = [
        pos2(rect.left(), rect.top() + h * 0.45),
        rect.left_bottom(),
        rect.right_bottom(),
        pos2(rect.right(), rect.top() + h * 0.45),
    ];
    painter.add(egui::Shape::line(tray.to_vec(), s));
    let x = rect.center().x;
    painter.line_segment([pos2(x, rect.top()), pos2(x, rect.top() + h * 0.68)], s);
    painter.line_segment([pos2(x, rect.top()), pos2(x - w * 0.22, rect.top() + h * 0.22)], s);
    painter.line_segment([pos2(x, rect.top()), pos2(x + w * 0.22, rect.top() + h * 0.22)], s);
}

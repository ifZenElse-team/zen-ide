use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use egui::{Align2, Color32, Mesh, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2, pos2, vec2};
use zen_db::geometry::{self, Bounds, Outlines, Summary};
use zen_db::results::ResultSet;
use zen_i18n::t;

use super::super::{IdePalette, theme};
use crate::accessibility;

const CAPTION: f32 = 24.0;
const MOST_CELLS: usize = 2000;
const GRID_PIXELS: f64 = 72.0;
const MOST_GRID_LINES: usize = 200;
const MOST_POINTS: u64 = 1_000_000;
const MOST_CROSSINGS: usize = 4_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    result: u64,
    rows: Range<usize>,
    columns: Range<usize>,
    head: (usize, usize),
}

struct Drawn {
    head: bool,
    outlines: Outlines,
    summary: Summary,
}

type ViewKey = ([u32; 4], u32, [u32; 2], u64);

#[derive(Default)]
pub(super) struct GeometryCanvas {
    key: Option<Key>,
    drawn: Vec<Drawn>,
    loading: Option<(Receiver<Vec<Drawn>>, Arc<AtomicBool>)>,
    zoom: f64,
    pan: Vec2,
    generation: u64,
    projected: Option<(ViewKey, Vec<Shape>)>,
}

impl Drop for GeometryCanvas {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn read(set: &ResultSet, key: &Key, cancel: &AtomicBool) -> Vec<Drawn> {
    let mut drawn = Vec::new();
    let mut kept = 0u64;
    let mut add = |row: usize, column: usize, head: bool| {
        if let Some(found) = geometry::from_value(&set.get(row, column)) {
            let points = found.geometry.outline_points();
            if kept + points > MOST_POINTS {
                return;
            }
            kept += points;
            drawn.push(Drawn { head, outlines: found.geometry.outlines(), summary: found.summary() });
        }
    };
    add(key.head.0, key.head.1, true);
    let mut looked = 0;
    'rows: for row in key.rows.clone() {
        for column in key.columns.clone() {
            if cancel.load(Ordering::Relaxed) || looked >= MOST_CELLS {
                break 'rows;
            }
            looked += 1;
            if (row, column) != key.head {
                add(row, column, false);
            }
        }
    }
    drawn
}

fn nice_step(raw: f64) -> f64 {
    let power = 10f64.powf(raw.log10().floor());
    let fraction = raw / power;
    let nice = if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * power
}

fn decimals(step: f64) -> usize {
    (-step.log10().floor()).clamp(0.0, 10.0) as usize
}

fn label(value: f64, step: f64) -> f64 {
    if value.abs() < step * 1e-6 { 0.0 } else { value }
}

struct View {
    plot: Rect,
    center: [f64; 2],
    scale: f64,
    pan: Vec2,
}

impl View {
    fn screen(&self, [x, y]: [f64; 2]) -> Pos2 {
        let middle = self.plot.center() + self.pan;
        pos2(
            (f64::from(middle.x) + (x - self.center[0]) * self.scale) as f32,
            (f64::from(middle.y) - (y - self.center[1]) * self.scale) as f32,
        )
    }

    fn world(&self, at: Pos2) -> [f64; 2] {
        let middle = self.plot.center() + self.pan;
        [
            self.center[0] + f64::from(at.x - middle.x) / self.scale,
            self.center[1] - f64::from(at.y - middle.y) / self.scale,
        ]
    }

    fn path(&self, points: &[[f64; 2]]) -> Vec<Pos2> {
        let mut out: Vec<Pos2> = Vec::with_capacity(points.len().min(4096));
        for (index, point) in points.iter().enumerate() {
            let at = self.screen(*point);
            if !at.x.is_finite() || !at.y.is_finite() {
                continue;
            }
            let last = index + 1 == points.len();
            if out.last().is_none_or(|known| known.distance(at) >= 0.75 || last) {
                out.push(at);
            }
        }
        out
    }
}

fn fill(mesh: &mut Mesh, rings: &[Vec<Pos2>], plot: Rect, color: Color32, budget: &mut usize) {
    let top = plot.top().floor();
    let rows = plot.height().ceil().max(0.0) as usize;
    let span = |a: &Pos2, b: &Pos2| {
        let (low, high) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
        let first = ((low - top - 0.5).ceil().max(0.0)) as usize;
        let last = ((high - top - 0.5).ceil().max(0.0) as usize).min(rows);
        last.saturating_sub(first)
    };
    let needed: usize = rings
        .iter()
        .flat_map(|ring| ring.iter().enumerate().map(move |(index, a)| span(a, &ring[(index + 1) % ring.len()])))
        .fold(0usize, usize::saturating_add);
    if needed > *budget {
        *budget = 0;
        return;
    }
    *budget -= needed;
    let mut crossings: Vec<Vec<f32>> = vec![Vec::new(); rows];
    for ring in rings {
        for (index, a) in ring.iter().enumerate() {
            let b = ring[(index + 1) % ring.len()];
            if a.y == b.y {
                continue;
            }
            let (low, high) = if a.y < b.y { (*a, b) } else { (b, *a) };
            let first = ((low.y - top - 0.5).ceil().max(0.0)) as usize;
            let last = ((high.y - top - 0.5).ceil().max(0.0) as usize).min(rows);
            for (row, xs) in crossings.iter_mut().enumerate().take(last).skip(first) {
                let y = top + row as f32 + 0.5;
                xs.push(low.x + (y - low.y) * (high.x - low.x) / (high.y - low.y));
            }
        }
    }
    for (row, xs) in crossings.iter_mut().enumerate() {
        if xs.len() < 2 {
            continue;
        }
        xs.sort_by(f32::total_cmp);
        let y = top + row as f32;
        for pair in xs.as_chunks::<2>().0 {
            let (left, right) = (pair[0].max(plot.left()), pair[1].min(plot.right()));
            if right > left {
                mesh.add_colored_rect(Rect::from_min_max(pos2(left, y), pos2(right, y + 1.0)), color);
            }
        }
    }
}

fn project(drawn: &[Drawn], view: &View, p: &IdePalette) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for current in [false, true] {
        let (line, area) = if current {
            (Stroke::new(1.5, p.accent), p.accent.gamma_multiply(0.22))
        } else {
            (Stroke::new(1.0, p.text_secondary), p.text_secondary.gamma_multiply(0.12))
        };
        let mut mesh = Mesh::default();
        let mut strokes = Vec::new();
        let mut budget = MOST_CROSSINGS;
        for shape in drawn.iter().filter(|shape| shape.head == current) {
            for rings in &shape.outlines.areas {
                let rings: Vec<Vec<Pos2>> =
                    rings.iter().map(|ring| view.path(ring)).filter(|ring| ring.len() > 1).collect();
                fill(&mut mesh, &rings, view.plot, area, &mut budget);
                for mut ring in rings {
                    if ring.first() != ring.last() {
                        ring.push(ring[0]);
                    }
                    strokes.push(Shape::line(ring, line));
                }
            }
            for path in &shape.outlines.lines {
                strokes.push(Shape::line(view.path(path), line));
            }
            for point in &shape.outlines.points {
                let at = view.screen(*point);
                strokes.push(Shape::circle_filled(at, if current { 3.5 } else { 3.0 }, line.color));
                strokes.push(Shape::circle_stroke(
                    at,
                    if current { 3.5 } else { 3.0 },
                    Stroke::new(1.0, p.editor_background),
                ));
            }
        }
        shapes.push(Shape::mesh(mesh));
        shapes.extend(strokes);
    }
    shapes
}

impl GeometryCanvas {
    fn cancel(&mut self) {
        if let Some((_, cancel)) = self.loading.take() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    fn want(&mut self, ctx: &egui::Context, set: &Arc<ResultSet>, key: Key) {
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.cancel();
        self.key = Some(key.clone());
        self.drawn.clear();
        self.generation += 1;
        self.projected = None;
        self.fit();
        let (sender, receiver) = std::sync::mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&cancel);
        let set = Arc::clone(set);
        let waker = ctx.clone();
        let started = std::thread::Builder::new().name("zen-db-geometry".to_owned()).spawn(move || {
            let drawn = read(&set, &key, &stop);
            if !stop.load(Ordering::Relaxed) {
                let _ = sender.send(drawn);
                waker.request_repaint();
            }
        });
        if started.is_ok() {
            self.loading = Some((receiver, cancel));
        }
    }

    fn fit(&mut self) {
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }

    fn take_finished(&mut self, ctx: &egui::Context) {
        let Some((receiver, _)) = &self.loading else { return };
        match receiver.try_recv() {
            Ok(drawn) => {
                self.drawn = drawn;
                self.loading = None;
                self.generation += 1;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => self.loading = None,
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(50)),
        }
    }

    fn caption(&self) -> String {
        let Some(head) = self.drawn.iter().find(|shape| shape.head) else { return String::new() };
        let mut parts = vec![head.summary.kind.clone()];
        if let Some(srid) = head.summary.srid.filter(|srid| *srid != 0) {
            parts.push(t!("ide-database-geometry-srid", srid = srid));
        }
        parts.push(t!("ide-database-geometry-points", points = head.summary.points));
        if self.drawn.len() > 1 {
            parts.push(t!("ide-database-geometry-shapes", count = self.drawn.len()));
        }
        parts.join(" · ")
    }

    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        area: Rect,
        set: &Arc<ResultSet>,
        head: (usize, usize),
        selection: Option<(Range<usize>, Range<usize>)>,
    ) {
        let (rows, columns) = selection
            .filter(|(rows, columns)| rows.contains(&head.0) && columns.contains(&head.1))
            .unwrap_or((head.0..head.0 + 1, head.1..head.1 + 1));
        self.want(ui.ctx(), set, Key { result: set.id(), rows, columns, head });
        self.take_finished(ui.ctx());
        let caption = self.caption();
        let job = super::super::vcs::truncated(&caption, theme::regular(12.0), p.text_secondary, area.width() - 20.0);
        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
        ui.painter().galley(
            pos2(area.left() + 10.0, area.top() + (CAPTION - galley.size().y) / 2.0),
            galley,
            p.text_secondary,
        );
        let plot = Rect::from_min_max(pos2(area.left() + 8.0, area.top() + CAPTION), area.max - vec2(8.0, 8.0));
        if plot.width() < 20.0 || plot.height() < 20.0 {
            return;
        }
        let response = ui.interact(plot, ui.scope_id().with("database-geometry-canvas"), Sense::click_and_drag());
        accessibility::name_with(&response, egui::Role::Image, || {
            format!("{}: {caption}", t!("ide-database-geometry-title"))
        });
        let bounds = self.drawn.iter().filter_map(|shape| shape.summary.bounds).reduce(Bounds::union);
        let Some(bounds) = bounds else {
            let message = if self.loading.is_some() {
                t!("ide-database-geometry-reading")
            } else {
                t!("ide-database-geometry-empty")
            };
            ui.painter().text(plot.center(), Align2::CENTER_CENTER, message, theme::regular(12.5), p.text_secondary);
            return;
        };
        let (width, height) = (bounds.max_x - bounds.min_x, bounds.max_y - bounds.min_y);
        let span = width.max(height);
        let span = if span > 0.0 { span } else { 1.0 };
        let fit = (f64::from(plot.width()) / width.max(span * 0.01))
            .min(f64::from(plot.height()) / height.max(span * 0.01))
            * 0.9;
        if response.double_clicked() {
            self.fit();
        }
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        if let Some(pointer) = response.hover_pos() {
            let (scroll, pinch) = ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
            let factor = f64::from(pinch) * (f64::from(scroll) * 0.002).exp();
            if (factor - 1.0).abs() > f64::EPSILON {
                let zoom = (self.zoom * factor).clamp(0.05, 1e7);
                let applied = (zoom / self.zoom) as f32;
                let offset = pointer - plot.center();
                self.pan = offset - (offset - self.pan) * applied;
                self.zoom = zoom;
            }
        }
        let view = View {
            plot,
            center: [(bounds.min_x + bounds.max_x) / 2.0, (bounds.min_y + bounds.max_y) / 2.0],
            scale: fit * self.zoom,
            pan: self.pan,
        };
        let painter = ui.painter_at(plot);
        self.grid(&painter, p, &view);
        let key: ViewKey = (
            [plot.min.x.to_bits(), plot.min.y.to_bits(), plot.max.x.to_bits(), plot.max.y.to_bits()],
            (self.zoom as f32).to_bits(),
            [self.pan.x.to_bits(), self.pan.y.to_bits()],
            self.generation,
        );
        if self.projected.as_ref().is_none_or(|(known, _)| *known != key) {
            self.projected = Some((key, project(&self.drawn, &view, p)));
        }
        if let Some((_, shapes)) = &self.projected {
            painter.extend(shapes.iter().cloned());
        }
        let step = nice_step(GRID_PIXELS / view.scale);
        if let Some(pointer) = response.hover_pos() {
            let [x, y] = view.world(pointer);
            let places = decimals(step) + 1;
            painter.text(
                pos2(plot.left() + 6.0, plot.bottom() - 6.0),
                Align2::LEFT_BOTTOM,
                format!("{x:.places$}, {y:.places$}"),
                theme::regular(11.5),
                p.text,
            );
        } else {
            painter.text(
                pos2(plot.right() - 6.0, plot.bottom() - 6.0),
                Align2::RIGHT_BOTTOM,
                t!("ide-database-geometry-hint"),
                theme::regular(11.0),
                p.text_secondary,
            );
        }
    }

    fn grid(&self, painter: &egui::Painter, p: &IdePalette, view: &View) {
        let plot = view.plot;
        painter.rect_stroke(plot, 0, Stroke::new(1.0, p.frame), egui::StrokeKind::Inside);
        let step = nice_step(GRID_PIXELS / view.scale);
        if !step.is_finite() || step <= 0.0 {
            return;
        }
        let places = decimals(step);
        let [left, top] = view.world(plot.left_top());
        let [right, bottom] = view.world(plot.right_bottom());
        let stroke = Stroke::new(1.0, p.frame);
        let font = theme::regular(10.5);
        let mut x = (left / step).ceil() * step;
        let mut lines = 0;
        while x <= right && lines < MOST_GRID_LINES {
            let at = view.screen([x, 0.0]).x;
            painter.vline(at, plot.y_range(), stroke);
            painter.text(
                pos2(at + 3.0, plot.top() + 2.0),
                Align2::LEFT_TOP,
                format!("{:.places$}", label(x, step)),
                font.clone(),
                p.text_secondary,
            );
            x += step;
            lines += 1;
        }
        let mut y = (bottom / step).ceil() * step;
        while y <= top && lines < MOST_GRID_LINES * 2 {
            let at = view.screen([0.0, y]).y;
            painter.hline(plot.x_range(), at, stroke);
            painter.text(
                pos2(plot.left() + 3.0, at - 2.0),
                Align2::LEFT_BOTTOM,
                format!("{:.places$}", label(y, step)),
                font.clone(),
                p.text_secondary,
            );
            y += step;
            lines += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_steps_are_round_numbers() {
        assert_eq!(nice_step(0.7), 1.0);
        assert_eq!(nice_step(13.0), 20.0);
        assert_eq!(nice_step(0.0031), 0.005);
        assert_eq!(decimals(0.005), 3);
        assert_eq!(decimals(20.0), 0);
    }

    #[test]
    fn a_square_with_a_hole_fills_only_around_the_hole() {
        let plot = Rect::from_min_size(Pos2::ZERO, vec2(10.0, 10.0));
        let outer = vec![pos2(0.0, 0.0), pos2(10.0, 0.0), pos2(10.0, 10.0), pos2(0.0, 10.0)];
        let hole = vec![pos2(3.0, 3.0), pos2(7.0, 3.0), pos2(7.0, 7.0), pos2(3.0, 7.0)];
        let mut mesh = Mesh::default();
        fill(&mut mesh, &[outer, hole], plot, Color32::default(), &mut MOST_CROSSINGS.clone());
        let area: f32 = mesh
            .vertices
            .as_chunks::<4>()
            .0
            .iter()
            .map(|quad| {
                let (a, d) = (quad[0].pos, quad[3].pos);
                ((d.x - a.x) * (d.y - a.y)).abs()
            })
            .sum();
        assert!((area - 84.0).abs() < 0.5, "100 minus the hole's 16: {area}");
    }

    #[test]
    fn a_fill_past_its_budget_is_left_out_and_takes_the_rest() {
        let plot = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 100.0));
        let zigzag: Vec<Pos2> =
            (0..10_000).map(|at| pos2(at as f32 / 100.0, if at % 2 == 0 { 0.0 } else { 100.0 })).collect();
        let mut mesh = Mesh::default();
        let mut budget = 1000;
        fill(&mut mesh, &[zigzag], plot, Color32::default(), &mut budget);
        assert!(mesh.vertices.is_empty());
        assert_eq!(budget, 0, "nothing else is filled in this projection");
    }
}

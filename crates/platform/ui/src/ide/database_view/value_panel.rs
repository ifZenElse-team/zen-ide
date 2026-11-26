use std::collections::HashSet;
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use egui::{Align2, CursorIcon, FontId, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_db::present::{self, Calc, Rendered, ValueFormat};
use zen_db::results::ResultSet;
use zen_db::value::Value;
use zen_i18n::t;

use super::super::glyphs::glyph_close;
use super::super::{IdePalette, theme};
use super::code::{self, Lang};
use super::geometry_canvas::GeometryCanvas;
use crate::accessibility;

const WIDTH: f32 = 340.0;
const MIN_WIDTH: f32 = 200.0;
const HEADER: f32 = 28.0;
const FORMATS: f32 = 28.0;
const CALC: f32 = 134.0;
const GUTTER: f32 = 16.0;
const SHOWN_CHARS: usize = 100_000;
const QUICK_BYTES: usize = 64 * 1024;
const MOST_CALC_CELLS: usize = 1_000_000;
const STATUS_CHARS: usize = 40;

#[derive(Default)]
pub(crate) struct SelectionFigures {
    pub(crate) text: String,
    pub(crate) pass: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    result: u64,
    row: usize,
    column: usize,
    format: Option<ValueFormat>,
}

struct Shown {
    key: Key,
    name: String,
    type_name: String,
    null: bool,
    formats: Vec<ValueFormat>,
    format: ValueFormat,
    rendered: Rendered,
    bytes: Option<Arc<Vec<u8>>>,
}

struct Picture {
    key: Key,
    texture: Option<egui::TextureHandle>,
    size: [usize; 2],
    format: &'static str,
}

type CalcKey = (u64, u64, Range<usize>, Range<usize>);

type Cells = (Range<usize>, Range<usize>);

#[derive(Default)]
pub(super) struct ValuePanel {
    width: f32,
    shown: Option<Shown>,
    rendering: Option<(Key, Receiver<Shown>)>,
    folded: HashSet<usize>,
    picture: Option<Picture>,
    decoding: Option<(Key, Receiver<Option<zen_image::Picture>>)>,
    calc: Option<(CalcKey, Option<Calc>)>,
    calculating: Option<(CalcKey, Receiver<Calc>)>,
    message: Option<String>,
    geometry: GeometryCanvas,
}

fn shown_of(set: &ResultSet, key: Key) -> Shown {
    let value = set.get(key.row, key.column);
    let column = set.columns().get(key.column).cloned();
    let format = present::chosen(&value, key.format);
    let rendered = present::render(&value, format, SHOWN_CHARS).unwrap_or_default();
    let bytes = match &value {
        Value::Bytes(bytes) => Some(Arc::new(bytes.clone())),
        _ => None,
    };
    Shown {
        key,
        name: column.as_ref().map(|column| column.name.clone()).unwrap_or_default(),
        type_name: column.map(|column| column.type_name).unwrap_or_default(),
        null: value == Value::Null,
        formats: present::formats(&value),
        format,
        rendered,
        bytes,
    }
}

fn lang(format: ValueFormat) -> Lang {
    match format {
        ValueFormat::Json => Lang::Json,
        ValueFormat::Xml => Lang::Xml,
        _ => Lang::Plain,
    }
}

fn format_label(format: ValueFormat) -> String {
    match format {
        ValueFormat::Text => t!("ide-database-value-format-text"),
        ValueFormat::Json => t!("ide-database-value-format-json"),
        ValueFormat::Xml => t!("ide-database-value-format-xml"),
        ValueFormat::Hex => t!("ide-database-value-format-hex"),
        ValueFormat::Image => t!("ide-database-value-format-image"),
        ValueFormat::Geometry => t!("ide-database-value-format-geometry"),
    }
}

fn size_text(bytes: usize) -> String {
    zen_workbench::database::objects::size_text(bytes as u64)
}

fn visible(text: &str, folds: &[(usize, usize)], folded: &HashSet<usize>) -> (String, Vec<usize>) {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut origin = Vec::with_capacity(lines.len());
    let mut at = 0;
    while at < lines.len() {
        let fold = folds.iter().find(|(start, _)| *start == at && folded.contains(start));
        match fold {
            Some((start, end)) => {
                out.push(format!("{} … {}", lines[*start], lines.get(*end).map_or("", |line| line.trim_start())));
                origin.push(*start);
                at = end + 1;
            }
            None => {
                out.push(lines[at].to_owned());
                origin.push(at);
                at += 1;
            }
        }
    }
    (out.join("\n"), origin)
}

fn small_button(
    ui: &mut Ui,
    p: &IdePalette,
    rect: Rect,
    label: &str,
    selected: bool,
    salt: (&str, usize),
) -> egui::Response {
    let response = ui.interact(rect, ui.scope_id().with(salt), Sense::click());
    if selected {
        ui.painter().rect_filled(rect, 4, p.active_tab);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4, p.hover);
    }
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::regular(12.0), p.text);
    accessibility::button_with(&response, || label.to_owned());
    response
}

fn text_width(ui: &Ui, text: &str, font: &FontId) -> f32 {
    ui.fonts_mut(|fonts| fonts.layout_no_wrap(text.to_owned(), font.clone(), ui.visuals().text_color()).size().x)
}

impl ValuePanel {
    pub(super) fn width(&self, available: f32) -> f32 {
        let width = if self.width <= 0.0 { WIDTH } else { self.width };
        width.clamp(MIN_WIDTH.min(available * 0.6), (available * 0.6).max(MIN_WIDTH.min(available)))
    }

    pub(super) fn kept_width(&self) -> Option<f32> {
        (self.width > 0.0).then_some(self.width)
    }

    pub(super) fn restore_width(&mut self, width: f32) {
        self.width = width;
    }

    fn take_finished(&mut self, ctx: &egui::Context) {
        if let Some((_, receiver)) = &self.rendering {
            match receiver.try_recv() {
                Ok(shown) => {
                    self.shown = Some(shown);
                    self.rendering = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.rendering = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(50)),
            }
        }
        if let Some((key, receiver)) = &self.decoding {
            let key = *key;
            match receiver.try_recv() {
                Ok(decoded) => {
                    self.decoding = None;
                    self.picture = Some(match decoded {
                        Some(picture) => {
                            let picture = picture.shrunk_to(2048);
                            let image = egui::ColorImage::from_rgba_unmultiplied(
                                [picture.width, picture.height],
                                &picture.rgba,
                            );
                            let texture =
                                ctx.load_texture("database-value-picture", image, egui::TextureOptions::LINEAR);
                            Picture {
                                key,
                                texture: Some(texture),
                                size: [picture.width, picture.height],
                                format: picture.format,
                            }
                        }
                        None => Picture { key, texture: None, size: [0, 0], format: "" },
                    });
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.decoding = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(50)),
            }
        }
        if let Some((key, receiver)) = &self.calculating {
            match receiver.try_recv() {
                Ok(calc) => {
                    self.calc = Some((key.clone(), Some(calc)));
                    self.calculating = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.calculating = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(50)),
            }
        }
    }

    fn want(&mut self, ctx: &egui::Context, set: &Arc<ResultSet>, key: Key) {
        let current = self.shown.as_ref().is_some_and(|shown| shown.key == key)
            || self.rendering.as_ref().is_some_and(|(pending, _)| *pending == key);
        if current {
            return;
        }
        if self.shown.as_ref().is_none_or(|shown| {
            (shown.key.result, shown.key.row, shown.key.column) != (key.result, key.row, key.column)
        }) {
            self.folded.clear();
        }
        let size = zen_db::results::value_size(&set.get(key.row, key.column));
        if size <= QUICK_BYTES {
            self.shown = Some(shown_of(set, key));
            self.rendering = None;
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let set = Arc::clone(set);
        let waker = ctx.clone();
        let started = std::thread::Builder::new().name("zen-db-value".to_owned()).spawn(move || {
            let _ = sender.send(shown_of(&set, key));
            waker.request_repaint();
        });
        if started.is_ok() {
            self.rendering = Some((key, receiver));
        }
    }

    fn figures(
        &mut self,
        ctx: &egui::Context,
        set: &Arc<ResultSet>,
        selection: (Range<usize>, Range<usize>),
    ) -> Option<Calc> {
        let key: CalcKey = (set.id(), set.info().rows, selection.0.clone(), selection.1.clone());
        if let Some((known, calc)) = &self.calc
            && *known == key
        {
            return calc.clone();
        }
        if self.calculating.as_ref().is_some_and(|(pending, _)| *pending == key) {
            return self.calc.as_ref().and_then(|(_, calc)| calc.clone());
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let set = Arc::clone(set);
        let waker = ctx.clone();
        let (rows, columns) = selection;
        let started = std::thread::Builder::new().name("zen-db-calc".to_owned()).spawn(move || {
            let _ = sender.send(set.calc(rows, columns, MOST_CALC_CELLS));
            waker.request_repaint();
        });
        if started.is_ok() {
            self.calculating = Some((key, receiver));
        }
        self.calc.as_ref().and_then(|(_, calc)| calc.clone())
    }

    pub(super) fn status(
        &mut self,
        ui: &Ui,
        session: &mut Session,
        set: &Arc<ResultSet>,
        selection: Option<(Range<usize>, Range<usize>)>,
    ) -> Option<Calc> {
        self.take_finished(ui.ctx());
        let selection = selection?;
        let cells = selection.0.len() * selection.1.len();
        let calc = self.figures(ui.ctx(), set, selection)?;
        if cells > 1 {
            let mut parts = vec![t!("ide-database-calc-count", count = calc.count)];
            if let Some(sum) = &calc.sum {
                parts.push(t!("ide-database-calc-sum", sum = sum.as_str()));
            }
            if let Some(avg) = &calc.avg {
                parts.push(t!("ide-database-calc-avg", avg = avg.as_str()));
            }
            if let (Some(min), Some(max)) = (&calc.min, &calc.max) {
                let short = |text: &str| match text.char_indices().nth(STATUS_CHARS) {
                    Some((at, _)) => format!("{}…", &text[..at]),
                    None => text.to_owned(),
                };
                parts.push(t!("ide-database-calc-range", min = short(min), max = short(max)));
            }
            let figures = session.state_mut::<SelectionFigures>();
            figures.text = parts.join(" · ");
            figures.pass = ui.ctx().cumulative_pass_nr();
        }
        Some(calc)
    }

    #[expect(clippy::too_many_arguments, reason = "the panel's frame, the window's state and what it shows")]
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        p: &IdePalette,
        registry: &Registry,
        session: &mut Session,
        area: Rect,
        (set, cell, wanted): (&Arc<ResultSet>, Option<(usize, usize)>, Option<ValueFormat>),
        selection: Option<Cells>,
        calc: Option<Calc>,
    ) {
        ui.painter().rect_filled(area, 0, p.island);
        ui.painter().vline(area.left(), area.y_range(), Stroke::new(1.0, p.frame));
        let region = ui.interact(area, ui.scope_id().with("database-value-panel"), Sense::hover());
        accessibility::name_with(&region, egui::Role::Pane, || t!("ide-database-value-title"));
        let splitter = Rect::from_min_max(pos2(area.left() - 3.0, area.top()), pos2(area.left() + 3.0, area.bottom()));
        let drag = ui.interact(splitter, ui.scope_id().with("database-value-splitter"), Sense::drag());
        if drag.hovered() || drag.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if drag.dragged() {
            self.width = (area.width() - drag.drag_delta().x).max(MIN_WIDTH);
        }
        let header = Rect::from_min_size(area.min, vec2(area.width(), HEADER));
        let title = t!("ide-database-value-title");
        ui.painter().text(
            pos2(header.left() + 10.0, header.center().y),
            Align2::LEFT_CENTER,
            &title,
            theme::semibold(12.5),
            p.text,
        );
        let title_width = text_width(ui, &title, &theme::semibold(12.5));
        let close = Rect::from_center_size(pos2(header.right() - 14.0, header.center().y), vec2(16.0, 16.0));
        let closing = ui.interact(close, ui.scope_id().with("database-value-close"), Sense::click());
        if closing.hovered() {
            ui.painter().rect_filled(close, 3, p.hover);
        }
        glyph_close(ui.painter(), close.shrink(3.0), p.text_secondary);
        accessibility::button_with(&closing, || t!("ide-database-value-close"));
        if closing.clicked() {
            let _ = registry.dispatch(session, "db.result.toggleValuePanel", json!({ "shown": false }));
        }
        let calc_area = Rect::from_min_max(pos2(area.left(), (area.bottom() - CALC).max(header.bottom())), area.max);
        let body = Rect::from_min_max(pos2(area.left(), header.bottom()), pos2(area.right(), calc_area.top()));
        self.take_finished(ui.ctx());
        match cell {
            Some((row, column)) if row < set.len() && column < set.columns().len() => {
                let key = Key { result: set.id(), row, column, format: wanted };
                self.want(ui.ctx(), set, key);
            }
            _ => {
                self.shown = None;
                ui.painter().text(
                    body.center(),
                    Align2::CENTER_CENTER,
                    t!("ide-database-value-select-cell"),
                    theme::regular(12.5),
                    p.text_secondary,
                );
            }
        }
        if let Some(shown) = self.shown.as_ref().filter(|shown| Some((shown.key.row, shown.key.column)) == cell) {
            let detail = format!("{} · {}", shown.name, shown.type_name);
            let job = super::super::vcs::truncated(
                &detail,
                theme::regular(12.0),
                p.text_secondary,
                (close.left() - header.left() - title_width - 30.0).max(20.0),
            );
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(
                pos2(header.left() + 20.0 + title_width, header.center().y - galley.size().y / 2.0),
                galley,
                p.text_secondary,
            );
            let actions = self.format_row(ui, p, Rect::from_min_size(body.min, vec2(body.width(), FORMATS)));
            let content = Rect::from_min_max(pos2(body.left(), body.top() + FORMATS), body.max);
            self.content(ui, p, content, set, selection);
            if let Some(action) = actions {
                self.act(ui, registry, session, action);
            }
        } else if self.rendering.is_some() {
            ui.painter().text(
                body.center(),
                Align2::CENTER_CENTER,
                t!("ide-database-value-rendering"),
                theme::regular(12.5),
                p.text_secondary,
            );
        }
        self.calc_section(ui, p, calc_area, calc);
        if let Some(message) = &self.message {
            ui.painter().text(
                pos2(body.left() + 10.0, body.bottom() - 10.0),
                Align2::LEFT_BOTTOM,
                message,
                theme::regular(11.5),
                p.text_secondary,
            );
        }
    }

    fn format_row(&mut self, ui: &mut Ui, p: &IdePalette, rect: Rect) -> Option<Action> {
        let shown = self.shown.as_ref()?;
        ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.frame));
        let mut x = rect.left() + 6.0;
        let mut action = None;
        for (index, format) in shown.formats.iter().enumerate() {
            let label = format_label(*format);
            let width = text_width(ui, &label, &theme::regular(12.0)) + 14.0;
            let button = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(width, rect.height() - 6.0));
            x = button.right() + 2.0;
            let selected = *format == shown.format;
            let response = small_button(ui, p, button, &label, selected, ("database-value-format", index));
            accessibility::pressed(&response, selected);
            if response.clicked() && !selected {
                action = Some(Action::Format(*format));
            }
        }
        let mut right = rect.right() - 6.0;
        let mut offered = vec![(t!("ide-database-value-save"), Action::Save)];
        if shown.format != ValueFormat::Image {
            offered.push((t!("ide-database-value-copy"), Action::Copy));
        }
        for (index, (label, wanted)) in offered.into_iter().enumerate() {
            let width = text_width(ui, &label, &theme::regular(12.0)) + 14.0;
            let button = Rect::from_min_size(pos2(right - width, rect.top() + 3.0), vec2(width, rect.height() - 6.0));
            right = button.left() - 2.0;
            if button.left() < x {
                continue;
            }
            if small_button(ui, p, button, &label, false, ("database-value-action", index)).clicked() {
                action = Some(wanted);
            }
        }
        action
    }

    fn act(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, action: Action) {
        let Some(shown) = self.shown.as_ref() else { return };
        match action {
            Action::Format(format) => {
                let _ = registry.dispatch(session, "db.settings.update", json!({ "valueFormat": format }));
            }
            Action::Copy => {
                ui.ctx().copy_text(shown.rendered.text.clone());
                self.message = Some(t!("ide-database-value-copied"));
            }
            Action::Save => {
                let suggested = match shown.format {
                    ValueFormat::Json => format!("{}.json", shown.name),
                    ValueFormat::Xml => format!("{}.xml", shown.name),
                    ValueFormat::Geometry => format!("{}.wkt", shown.name),
                    ValueFormat::Image => format!(
                        "{}.{}",
                        shown.name,
                        self.picture.as_ref().map_or("bin", |picture| extension(picture.format))
                    ),
                    ValueFormat::Hex if shown.bytes.is_some() => format!("{}.bin", shown.name),
                    _ => format!("{}.txt", shown.name),
                };
                let key = shown.key;
                let format = match shown.format {
                    ValueFormat::Image | ValueFormat::Hex if shown.bytes.is_some() => None,
                    other => Some(other),
                };
                if let Some(path) = pick_file(&suggested) {
                    let params = json!({
                        "id": key.result,
                        "row": key.row,
                        "column": key.column,
                        "path": path,
                        "format": format,
                        "overwrite": true,
                    });
                    self.message = Some(match registry.dispatch(session, "db.result.saveValue", params) {
                        Ok(_) => t!("ide-database-value-saved", path = path.display().to_string()),
                        Err(error) => error.to_string(),
                    });
                }
            }
        }
    }

    fn content(&mut self, ui: &mut Ui, p: &IdePalette, area: Rect, set: &Arc<ResultSet>, selection: Option<Cells>) {
        let Some(shown) = self.shown.as_ref() else { return };
        ui.painter().rect_filled(area, 0, p.editor_background);
        ui.painter().hline(area.x_range(), area.top(), Stroke::new(1.0, p.frame));
        if shown.null {
            ui.painter().text(area.center(), Align2::CENTER_CENTER, "[NULL]", theme::regular(12.5), p.text_secondary);
            return;
        }
        if shown.format == ValueFormat::Image {
            self.image(ui, p, area);
            return;
        }
        if shown.format == ValueFormat::Geometry {
            let head = (shown.key.row, shown.key.column);
            self.geometry.show(ui, p, area, set, head, selection);
            return;
        }
        let shown = self.shown.as_ref().expect("checked above");
        let mut text_area = area;
        if shown.rendered.truncated {
            let note = t!(
                "ide-database-value-cut",
                shown = shown.rendered.text.chars().count(),
                length = shown.rendered.length
            );
            ui.painter().text(
                pos2(area.left() + 8.0, area.bottom() - 4.0),
                Align2::LEFT_BOTTOM,
                note,
                theme::regular(11.5),
                p.warning_text,
            );
            text_area = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - 20.0));
        }
        let wrap = shown.format == ValueFormat::Text;
        let (text, origin) = visible(&shown.rendered.text, &shown.rendered.folds, &self.folded);
        let starts: Vec<(usize, usize)> = shown.rendered.folds.clone();
        let lang = lang(shown.format);
        let font = FontId::monospace(12.5);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("database-value-text", shown.key.result, shown.key.row, shown.key.column))
                .max_rect(text_area.shrink2(vec2(2.0, 2.0))),
        );
        let mut toggled = None;
        let palette = *p;
        let scroll = if wrap { egui::ScrollArea::vertical() } else { egui::ScrollArea::both() };
        scroll.auto_shrink([false, false]).show(&mut child, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let line_height = ui.fonts_mut(|fonts| fonts.row_height(&font));
                let lines = origin.len();
                let (gutter, _) =
                    ui.allocate_exact_size(vec2(GUTTER, lines as f32 * line_height + 8.0), Sense::hover());
                let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, width: f32| {
                    let mut job = code::job(buffer.as_str(), lang, &palette, &font);
                    job.wrap.max_width = if wrap { width } else { f32::INFINITY };
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                let output = egui::TextEdit::multiline(&mut text.as_str())
                    .id_salt("database-value-text")
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::symmetric(2, 4))
                    .desired_width(if wrap { ui.available_width() } else { f32::INFINITY })
                    .layouter(&mut layouter)
                    .show(ui);
                accessibility::labelled(&output.response, &t!("ide-database-value-title"));
                if wrap {
                    return;
                }
                let top = output.galley_pos.y;
                for (visible_line, line) in origin.iter().enumerate() {
                    let Some((start, _)) = starts.iter().find(|(start, _)| start == line) else { continue };
                    let y = top + visible_line as f32 * line_height + line_height / 2.0;
                    let arrow = Rect::from_center_size(pos2(gutter.center().x, y), vec2(12.0, 12.0));
                    let response =
                        ui.interact(arrow, ui.scope_id().with(("database-value-fold", *start)), Sense::click());
                    let folded = self.folded.contains(start);
                    let color = if response.hovered() { palette.text } else { palette.fold_arrow };
                    let center = arrow.center();
                    let shape = if folded {
                        vec![
                            pos2(center.x - 2.5, center.y - 4.0),
                            pos2(center.x + 3.0, center.y),
                            pos2(center.x - 2.5, center.y + 4.0),
                        ]
                    } else {
                        vec![
                            pos2(center.x - 4.0, center.y - 2.5),
                            pos2(center.x + 4.0, center.y - 2.5),
                            pos2(center.x, center.y + 3.0),
                        ]
                    };
                    ui.painter().add(egui::Shape::convex_polygon(shape, color, Stroke::NONE));
                    let label = if folded { t!("ide-database-value-unfold") } else { t!("ide-database-value-fold") };
                    accessibility::button_with(&response, || label.clone());
                    if response.clicked() {
                        toggled = Some(*start);
                    }
                }
            });
        });
        if let Some(start) = toggled
            && !self.folded.remove(&start)
        {
            self.folded.insert(start);
        }
    }

    fn image(&mut self, ui: &mut Ui, p: &IdePalette, area: Rect) {
        let Some(shown) = self.shown.as_ref() else { return };
        let key = shown.key;
        let ready = self.picture.as_ref().is_some_and(|picture| picture.key == key);
        if !ready
            && self.decoding.as_ref().is_none_or(|(pending, _)| *pending != key)
            && let Some(bytes) = shown.bytes.clone()
        {
            let (sender, receiver) = std::sync::mpsc::channel();
            let waker = ui.ctx().clone();
            let started = std::thread::Builder::new().name("zen-db-picture".to_owned()).spawn(move || {
                let _ = sender.send(zen_image::decode(Path::new("value.png"), &bytes).ok());
                waker.request_repaint();
            });
            if started.is_ok() {
                self.decoding = Some((key, receiver));
            }
        }
        let bytes = shown.bytes.as_ref().map_or(0, |bytes| bytes.len());
        match self.picture.as_ref().filter(|picture| picture.key == key) {
            Some(Picture { texture: Some(texture), size, format, .. }) => {
                let caption = t!(
                    "ide-database-value-picture",
                    format = *format,
                    width = size[0],
                    height = size[1],
                    size = size_text(bytes)
                );
                ui.painter().text(
                    pos2(area.left() + 10.0, area.top() + 14.0),
                    Align2::LEFT_CENTER,
                    &caption,
                    theme::regular(12.0),
                    p.text_secondary,
                );
                let room = Rect::from_min_max(pos2(area.left() + 10.0, area.top() + 28.0), area.max - vec2(10.0, 10.0));
                let natural = vec2(size[0] as f32, size[1] as f32);
                let fit = (room.width() / natural.x).min(room.height() / natural.y).clamp(0.01, 8.0);
                let shown_rect = Rect::from_min_size(room.min, natural * fit);
                ui.painter().rect_stroke(
                    shown_rect.expand(1.0),
                    0,
                    Stroke::new(1.0, p.frame),
                    egui::StrokeKind::Outside,
                );
                egui::Image::new((texture.id(), shown_rect.size())).paint_at(ui, shown_rect);
                let response = ui.interact(shown_rect, ui.scope_id().with("database-value-picture"), Sense::hover());
                accessibility::name_with(&response, egui::Role::Image, || caption.clone());
            }
            Some(_) => {
                ui.painter().text(
                    area.center(),
                    Align2::CENTER_CENTER,
                    t!("ide-database-value-not-picture"),
                    theme::regular(12.5),
                    p.text_secondary,
                );
            }
            None => {
                ui.painter().text(
                    area.center(),
                    Align2::CENTER_CENTER,
                    t!("ide-database-value-decoding"),
                    theme::regular(12.5),
                    p.text_secondary,
                );
            }
        }
    }

    fn calc_section(&self, ui: &mut Ui, p: &IdePalette, area: Rect, calc: Option<Calc>) {
        ui.painter().rect_filled(area, 0, p.island);
        ui.painter().hline(area.x_range(), area.top(), Stroke::new(1.0, p.frame));
        let title = t!("ide-database-calc-title");
        ui.painter().text(
            pos2(area.left() + 10.0, area.top() + 14.0),
            Align2::LEFT_CENTER,
            &title,
            theme::semibold(12.5),
            p.text,
        );
        let Some(calc) = calc else {
            ui.painter().text(
                pos2(area.left() + 10.0, area.top() + 38.0),
                Align2::LEFT_CENTER,
                t!("ide-database-calc-none"),
                theme::regular(12.0),
                p.text_secondary,
            );
            return;
        };
        let dash = || "—".to_owned();
        let rows = [
            (t!("ide-database-calc-count-label"), calc.count.to_string()),
            (t!("ide-database-calc-distinct-label"), calc.distinct.to_string()),
            (t!("ide-database-calc-sum-label"), calc.sum.clone().unwrap_or_else(dash)),
            (t!("ide-database-calc-avg-label"), calc.avg.clone().unwrap_or_else(dash)),
            (t!("ide-database-calc-min-label"), calc.min.clone().unwrap_or_else(dash)),
            (t!("ide-database-calc-max-label"), calc.max.clone().unwrap_or_else(dash)),
        ];
        let half = area.width() / 2.0;
        let mut spoken = vec![title.clone()];
        for (index, (label, value)) in rows.iter().enumerate() {
            let x = area.left() + 10.0 + (index % 2) as f32 * half;
            let y = area.top() + 38.0 + (index / 2) as f32 * 22.0;
            ui.painter().text(pos2(x, y), Align2::LEFT_CENTER, label, theme::regular(12.0), p.text_secondary);
            let job = super::super::vcs::truncated(value, theme::semibold(12.0), p.text, half - 80.0);
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            ui.painter().galley(pos2(x + 64.0, y - galley.size().y / 2.0), galley, p.text);
            spoken.push(format!("{label} {value}"));
        }
        if calc.truncated {
            ui.painter().text(
                pos2(area.left() + 10.0, area.bottom() - 8.0),
                Align2::LEFT_BOTTOM,
                t!("ide-database-calc-truncated"),
                theme::regular(11.5),
                p.warning_text,
            );
        }
        accessibility::label(ui, ui.scope_id().with("database-calc"), area, || spoken.join(", "));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Format(ValueFormat),
    Copy,
    Save,
}

fn extension(format: &str) -> &'static str {
    match format {
        "PNG" => "png",
        "JPEG" => "jpg",
        "GIF" => "gif",
        "WebP" => "webp",
        "BMP" => "bmp",
        "ICO" => "ico",
        _ => "bin",
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_file(suggested: &str) -> Option<std::path::PathBuf> {
    if std::env::var_os("ZEN_NO_DIALOGS").is_some() {
        return None;
    }
    rfd::FileDialog::new().set_title(t!("ide-database-value-save-title")).set_file_name(suggested).save_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_file(_suggested: &str) -> Option<std::path::PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folded_regions_close_into_one_line_and_keep_where_lines_came_from() {
        let (text, folds) = present::pretty_json(r#"{"a":{"b":1,"c":[1,2]},"d":2}"#);
        let folded: HashSet<usize> = [1].into_iter().collect();
        let (shown, origin) = visible(&text, &folds, &folded);
        assert_eq!(shown, "{\n  \"a\": { … },\n  \"d\": 2\n}");
        assert_eq!(origin, [0, 1, 8, 9]);
    }
}

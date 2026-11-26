pub(super) mod sprite;

use std::time::{Duration, Instant};

use egui::{Align2, Color32, Mesh, Pos2, Rect, Sense, Shape, Ui, Vec2, pos2, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::caffeine::{
    Activity, Assertion, Caffeine, CaffeineSettings, CaffeineStatus, Length, MAX_MINUTES, PRESETS, Pause,
};

use self::sprite::{Badge, Grid, Ink, Look};
use super::app::Request;
use super::glyphs::glyph_minus;
use super::{IdePalette, IdeView, Side, accessibility, hint, hover, metrics};
use crate::theme;

pub(super) const BLOODSHOT: Color32 = Color32::from_rgb(0xE0, 0x24, 0x2E);

#[derive(Default)]
pub(super) struct CaffeineUi {
    custom_minutes: Option<u32>,
    battery_level: Option<u8>,
    pub(super) look: Option<Look>,
}

pub(super) fn look(status: &CaffeineStatus) -> Look {
    if !status.available || !status.on {
        Look::Asleep
    } else if !status.awake {
        Look::Waiting
    } else if Assertion::ALL.iter().all(|assertion| status.what.contains(assertion)) {
        Look::Wired
    } else if status.what.contains(&Assertion::Display) {
        Look::Wide
    } else {
        Look::Steady
    }
}

pub(super) fn animating(visible: bool, reduce_motion: bool) -> bool {
    visible && !reduce_motion
}

pub(super) fn frame_now(look: Look, time: f64, moving: bool) -> (usize, Option<Duration>) {
    if !moving {
        return (sprite::still(look), None);
    }
    let ms = (time.max(0.0) * 1000.0) as u64;
    let (index, left) = sprite::frame_at(look, ms);
    (index, Some(Duration::from_millis(left.max(16))))
}

pub(super) fn paint(p: &IdePalette, ink: Ink) -> Option<Color32> {
    let luma = |c: Color32| u32::from(c.r()) * 3 + u32::from(c.g()) * 6 + u32::from(c.b());
    let (bright, dark) = if luma(p.text) > luma(p.island) { (p.text, p.island) } else { (p.island, p.text) };
    let mix = |a: Color32, b: Color32, t: f32| a.lerp_to_gamma(b, t);
    let deeper = |channel: u8| (f32::from(channel) * 0.7).round() as u8;
    let ink_dark = Color32::from_rgb(deeper(dark.r()), deeper(dark.g()), deeper(dark.b()));
    Some(match ink {
        Ink::Clear => return None,
        Ink::Outline | Ink::Pupil => ink_dark,
        Ink::Hair => mix(p.warning_text, dark, 0.55),
        Ink::Skin => mix(p.warning_text, bright, 0.55),
        Ink::Shirt => p.accent,
        Ink::ShirtShade => mix(p.accent, dark, 0.3),
        Ink::White => bright,
        Ink::Red => BLOODSHOT,
        Ink::Mouth => mix(p.warning_text, dark, 0.5),
        Ink::Cup => mix(bright, p.text_secondary, 0.15),
        Ink::Coffee => mix(p.warning_text, dark, 0.7),
        Ink::Table | Ink::Snore => p.text_secondary,
        Ink::Steam => mix(p.text_secondary, p.island, 0.2),
    })
}

pub(super) fn paint_grid<const N: usize>(
    painter: &egui::Painter,
    origin: Pos2,
    pixel: f32,
    grid: &Grid<N>,
    color: impl Fn(Ink) -> Option<Color32>,
) {
    let mut mesh = Mesh::default();
    for (row, inks) in grid.iter().enumerate() {
        let mut column = 0;
        while column < N {
            let ink = inks[column];
            let start = column;
            while column < N && inks[column] == ink {
                column += 1;
            }
            if let Some(color) = color(ink) {
                let min = origin + vec2(start as f32 * pixel, row as f32 * pixel);
                mesh.add_colored_rect(Rect::from_min_size(min, vec2((column - start) as f32 * pixel, pixel)), color);
            }
        }
    }
    painter.add(Shape::mesh(mesh));
}

pub(super) fn pixel_size(room: f32, n: usize, pixels_per_point: f32) -> f32 {
    let physical = ((room * pixels_per_point) / n as f32).floor().max(1.0);
    physical / pixels_per_point
}

fn snap(pos: Pos2, pixels_per_point: f32) -> Pos2 {
    pos2((pos.x * pixels_per_point).round() / pixels_per_point, (pos.y * pixels_per_point).round() / pixels_per_point)
}

pub(super) fn draw_stripe_cup(painter: &egui::Painter, rect: Rect, look: Look, color: Color32) {
    let ppp = painter.ctx().pixels_per_point();
    let pixel = pixel_size(rect.width().min(rect.height()), sprite::ICON, ppp);
    let side = pixel * sprite::ICON as f32;
    let origin = snap(rect.center() - Vec2::splat(side / 2.0), ppp);
    let grid = sprite::icon(Badge::of(look));
    paint_grid(painter, origin, pixel, &grid, |ink| match ink {
        Ink::Clear | Ink::Cup => None,
        Ink::Red => Some(BLOODSHOT),
        Ink::Coffee | Ink::Steam | Ink::Snore => Some(color.gamma_multiply(0.85)),
        _ => Some(color),
    });
}

pub(in crate::ide) fn time_left(seconds: u64) -> String {
    let minutes = seconds.div_ceil(60);
    if minutes < 60 {
        t!("ide-caffeine-left-minutes", minutes = minutes)
    } else if minutes.is_multiple_of(60) {
        t!("ide-caffeine-left-whole-hours", hours = minutes / 60)
    } else {
        t!("ide-caffeine-left-hours", hours = minutes / 60, minutes = minutes % 60)
    }
}

pub(in crate::ide) fn assertion_name(assertion: Assertion) -> String {
    t!(match assertion {
        Assertion::Idle => "ide-caffeine-what-idle",
        Assertion::Display => "ide-caffeine-what-display",
        Assertion::Disk => "ide-caffeine-what-disk",
        Assertion::System => "ide-caffeine-what-system",
        Assertion::User => "ide-caffeine-what-user",
    })
}

pub(in crate::ide) fn activity_name(activity: Activity) -> String {
    t!(match activity {
        Activity::Run => "ide-caffeine-while-run",
        Activity::Debug => "ide-caffeine-while-debug",
        Activity::Task => "ide-caffeine-while-task",
        Activity::Indexing => "ide-caffeine-while-indexing",
        Activity::Agent => "ide-caffeine-while-agent",
    })
}

fn names<T: Copy>(items: &[T], name: impl Fn(T) -> String) -> String {
    items.iter().map(|item| name(*item)).collect::<Vec<_>>().join(", ")
}

pub(super) fn headline(look: Look) -> String {
    t!(match look {
        Look::Asleep => "ide-caffeine-look-asleep",
        Look::Waiting => "ide-caffeine-look-waiting",
        Look::Steady => "ide-caffeine-look-steady",
        Look::Wide => "ide-caffeine-look-wide",
        Look::Wired => "ide-caffeine-look-wired",
    })
}

pub(super) fn describe(status: &CaffeineStatus, settings: &CaffeineSettings) -> String {
    if !status.available {
        return t!("ide-caffeine-unavailable");
    }
    let what = names(&status.what, assertion_name).to_lowercase();
    let mut lines = Vec::new();
    if !status.on {
        lines.push(t!("ide-caffeine-off"));
    } else {
        let length = match (status.length, status.seconds_left) {
            (Some(Length::While), _) => {
                t!("ide-caffeine-for-while", what = names(&status.during, activity_name).to_lowercase())
            }
            (_, Some(left)) => t!("ide-caffeine-for-time", left = time_left(left)),
            _ => t!("ide-caffeine-for-ever"),
        };
        lines.push(t!("ide-caffeine-on", what = what, length = length));
        match status.paused {
            Some(Pause::OnBattery) => lines.push(t!("ide-caffeine-paused-battery")),
            Some(Pause::LowBattery) => {
                lines.push(t!("ide-caffeine-paused-low", level = settings.low_battery_level));
            }
            Some(Pause::Waiting) => lines.push(t!("ide-caffeine-paused-waiting")),
            None if !status.busy.is_empty() && status.length == Some(Length::While) => {
                lines.push(t!("ide-caffeine-busy", what = names(&status.busy, activity_name).to_lowercase()));
            }
            None => {}
        }
        if let Some(error) = &status.error {
            lines.push(t!("ide-caffeine-failed", error = error.as_str()));
        }
    }
    if status.what.contains(&Assertion::Display) {
        lines.push(t!("ide-caffeine-lid"));
    }
    lines.join("\n")
}

pub(super) fn short_what(what: &[Assertion]) -> String {
    if Assertion::ALL.iter().all(|assertion| what.contains(assertion)) {
        return t!("ide-caffeine-short-everything");
    }
    names(what, |assertion| {
        t!(match assertion {
            Assertion::Idle => "ide-caffeine-short-idle",
            Assertion::Display => "ide-caffeine-short-display",
            Assertion::Disk => "ide-caffeine-short-disk",
            Assertion::System => "ide-caffeine-short-system",
            Assertion::User => "ide-caffeine-short-user",
        })
    })
}

pub(super) fn summary(status: &CaffeineStatus) -> (String, String) {
    if !status.available {
        return (t!("ide-caffeine-unavailable-short"), String::new());
    }
    if !status.on {
        return (t!("ide-caffeine-summary-off"), String::new());
    }
    let length = match (status.length, status.seconds_left) {
        (Some(Length::While), _) => t!("ide-caffeine-short-while"),
        (_, Some(left)) => t!("ide-caffeine-for-time", left = time_left(left)),
        _ => t!("ide-caffeine-for-ever"),
    };
    let first = t!("ide-caffeine-summary-on", what = short_what(&status.what), length = length);
    let second = if status.error.is_some() {
        t!("ide-caffeine-failed-short")
    } else {
        match status.paused {
            Some(Pause::OnBattery) => t!("ide-caffeine-paused-battery"),
            Some(Pause::LowBattery) => t!("ide-caffeine-paused-low-short"),
            Some(Pause::Waiting) => t!("ide-caffeine-paused-waiting"),
            None if !status.busy.is_empty() && status.length == Some(Length::While) => {
                t!("ide-caffeine-busy", what = names(&status.busy, activity_name).to_lowercase())
            }
            None => String::new(),
        }
    };
    (first, second)
}

const TIME_LINE: f32 = 26.0;
const STATE_LINE: f32 = 20.0;
const SUMMARY_LINE: f32 = 18.0;

fn fixed_line(
    ui: &mut Ui,
    height: f32,
    text: &str,
    font: egui::FontId,
    color: Color32,
    centred: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    if !text.is_empty() {
        let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font, color));
        job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width());
        let galley = ui.painter().layout_job(job);
        let x = if centred { rect.center().x - galley.size().x / 2.0 } else { rect.left() };
        ui.painter().galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
        accessibility::name(&response, egui::Role::Label, text);
    }
    response
}

enum Control {
    Toggle,
    Maximum,
    Start(serde_json::Value),
    Settings(CaffeineSettings),
    OpenSettings,
}

const ART_MAX: f32 = 224.0;

impl IdeView {
    pub(super) fn follow_caffeine_look(&mut self, session: &Session) {
        self.caffeine_ui.look = session.state::<Caffeine>().map(|caffeine| look(&caffeine.status(Instant::now())));
    }

    pub(super) fn caffeine_tool_window(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let (header, _) = ui.allocate_exact_size(vec2(ui.available_width(), metrics::TOOL_HEADER), Sense::hover());
        ui.painter().text(
            pos2(header.left() + 12.0, header.center().y),
            Align2::LEFT_CENTER,
            t!("ide-caffeine-title"),
            theme::semibold(13.0),
            p.text,
        );
        let hide = Rect::from_center_size(pos2(header.right() - 18.0, header.center().y), Vec2::splat(24.0));
        let response = ui.interact(hide, ui.scope_id().with("caffeine-hide"), Sense::click());
        hover(ui, &response, &p);
        glyph_minus(ui.painter(), hide.shrink(6.0), p.text_secondary);
        hint(ui, &response, &t!("ide-run-hide"), None, Side::Below);
        accessibility::button(&response, &t!("ide-run-hide"));
        if response.clicked() {
            self.bottom = None;
        }
        let Some(caffeine) = session.state::<Caffeine>().cloned() else { return };
        let status = caffeine.status(Instant::now());
        let settings = self.overview.settings.caffeine.clone();
        let look = look(&status);
        self.caffeine_ui.look = Some(look);
        let mut controls = Vec::new();
        egui::ScrollArea::vertical().id_salt("caffeine-scroll").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 4)).show(ui, |ui| {
                self.caffeine_art(ui, look, &status);
                ui.add_space(8.0);
                self.caffeine_controls(ui, &status, &settings, &mut controls);
            });
        });
        for control in controls {
            match control {
                Control::Toggle => self.dispatch(ui, registry, session, "caffeine.awake.toggle", json!({})),
                Control::Maximum => self.dispatch(ui, registry, session, "caffeine.awake.maximum", json!({})),
                Control::Start(params) => self.dispatch(ui, registry, session, "caffeine.awake.start", params),
                Control::Settings(caffeine) => {
                    let mut settings = self.overview.settings.clone();
                    settings.caffeine = caffeine;
                    self.overview.settings = settings.clone();
                    self.requests.push(Request::Settings(Box::new(settings)));
                }
                Control::OpenSettings => self.settings.open_at(super::settings::PageId::Caffeine),
            }
        }
    }

    fn caffeine_art(&mut self, ui: &mut Ui, look: Look, status: &CaffeineStatus) {
        let p = self.palette;
        let ppp = ui.ctx().pixels_per_point();
        let room = ui.available_width().clamp(64.0, ART_MAX);
        let pixel = pixel_size(room, sprite::SIZE, ppp);
        let side = pixel * sprite::SIZE as f32;
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), side + 8.0), Sense::hover());
        let origin = snap(pos2(rect.center().x - side / 2.0, rect.top() + 4.0), ppp);
        let visible = ui.input(|input| {
            let viewport = input.viewport();
            viewport.minimized != Some(true) && viewport.occluded != Some(true)
        });
        let moving = animating(visible, accessibility::reduce_motion());
        let (index, next) = frame_now(look, ui.input(|input| input.time), moving);
        paint_grid(ui.painter(), origin, pixel, &sprite::frame(look, index), |ink| paint(&p, ink));
        if let Some(next) = next {
            ui.ctx().request_repaint_after(next);
        }
        let line = headline(look);
        accessibility::name(&response, egui::Role::Image, &line);
        ui.add_space(4.0);
        let left =
            status.seconds_left.filter(|_| status.on).map(|left| t!("ide-caffeine-time-left", left = time_left(left)));
        fixed_line(ui, TIME_LINE, left.as_deref().unwrap_or_default(), theme::semibold(18.0), p.text, true);
        fixed_line(ui, STATE_LINE, &line, theme::semibold(13.0), p.text, true).on_hover_text(&line);
    }

    fn caffeine_controls(
        &mut self,
        ui: &mut Ui,
        status: &CaffeineStatus,
        settings: &CaffeineSettings,
        controls: &mut Vec<Control>,
    ) {
        let p = self.palette;
        let (first, second) = summary(status);
        let font = theme::regular(13.0);
        let top = fixed_line(ui, SUMMARY_LINE, &first, font.clone(), p.text_secondary, false);
        let bottom = fixed_line(ui, SUMMARY_LINE, &second, font, p.text_secondary, false);
        let full = describe(status, settings);
        accessibility::describe(&top, &full);
        let over = ui.interact(top.rect.union(bottom.rect), ui.scope_id().with("caffeine-summary"), Sense::hover());
        over.on_hover_text(full);
        if !status.available {
            return;
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let turn = if status.on { t!("ide-caffeine-turn-off") } else { t!("ide-caffeine-turn-on") };
            let button = if status.on {
                egui::Button::new(egui::RichText::new(turn).color(p.text))
                    .stroke(egui::Stroke::new(1.0, p.button_border))
            } else {
                egui::Button::new(egui::RichText::new(turn).color(p.accent_text)).fill(p.accent)
            };
            if ui.add(button.min_size(vec2(120.0, 28.0))).clicked() {
                controls.push(Control::Toggle);
            }
            let cup = egui::IdSalt::new("caffeine-maximum-cup");
            let name = t!("ide-caffeine-maximum");
            let button = egui::Button::new((
                egui::Atom::custom(cup, Vec2::splat(16.0)),
                egui::RichText::new(&name).color(p.text),
            ))
            .stroke(egui::Stroke::new(1.0, p.button_border))
            .selected(status.maximum)
            .min_size(vec2(0.0, 28.0));
            let response = button.atom_ui(ui);
            if let Some(rect) = response.rect(cup) {
                draw_stripe_cup(ui.painter(), rect, Look::Wired, p.text);
            }
            let tip =
                if status.maximum { t!("ide-caffeine-maximum-off-hint") } else { t!("ide-caffeine-maximum-hint") };
            hint(ui, &response.response, &name, Some(&tip), Side::Below);
            accessibility::pressed(&response.response, status.maximum);
            if response.response.clicked() {
                controls.push(Control::Maximum);
            }
        });
        ui.add_space(12.0);
        ui.label(egui::RichText::new(t!("ide-caffeine-menu-for")).font(theme::semibold(13.0)).color(p.text));
        let current = status.length.filter(|_| status.on);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(egui::Button::selectable(current == Some(Length::Indefinitely), t!("ide-caffeine-for-ever-item")))
                .clicked()
            {
                controls.push(Control::Start(json!({ "minutes": 0 })));
            }
            for minutes in PRESETS {
                let chosen = current == Some(Length::Minutes { minutes: *minutes });
                if ui.add(egui::Button::selectable(chosen, time_left(u64::from(*minutes) * 60))).clicked() {
                    controls.push(Control::Start(json!({ "minutes": minutes })));
                }
            }
            if ui.add(egui::Button::selectable(current == Some(Length::While), t!("ide-caffeine-while-item"))).clicked()
            {
                controls.push(Control::Start(json!({ "while": [] })));
            }
        });
        let mut changed = settings.clone();
        ui.horizontal(|ui| {
            ui.label(t!("ide-caffeine-custom"));
            let mut minutes = self.caffeine_ui.custom_minutes.unwrap_or(settings.custom_minutes);
            let field = ui.add(
                egui::DragValue::new(&mut minutes)
                    .range(1..=MAX_MINUTES)
                    .suffix(format!(" {}", t!("ide-caffeine-min"))),
            );
            accessibility::labelled(&field, &t!("ide-caffeine-custom"));
            self.caffeine_ui.custom_minutes = Some(minutes);
            if field.drag_stopped() || field.lost_focus() || (field.changed() && !field.dragged()) {
                changed.custom_minutes = minutes;
            }
            if ui.button(t!("ide-caffeine-custom-start")).clicked() {
                changed.custom_minutes = minutes;
                controls.push(Control::Start(json!({ "minutes": minutes })));
            }
        });
        ui.add_space(12.0);
        ui.label(egui::RichText::new(t!("ide-caffeine-menu-while")).font(theme::semibold(13.0)).color(p.text));
        let during = if status.length == Some(Length::While) { status.during.clone() } else { settings.during.clone() };
        for activity in Activity::ALL {
            let mut on = during.contains(&activity);
            if ui.checkbox(&mut on, activity_name(activity)).changed() {
                changed.during.clone_from(&during);
                changed.during.retain(|kind| *kind != activity);
                if on {
                    changed.during.push(activity);
                }
                changed.during.sort();
            }
        }
        ui.add_space(12.0);
        ui.label(egui::RichText::new(t!("ide-caffeine-menu-what")).font(theme::semibold(13.0)).color(p.text));
        let what = if status.on { status.what.clone() } else { settings.what.clone() };
        for assertion in Assertion::ALL {
            let mut on = what.contains(&assertion);
            if ui.checkbox(&mut on, assertion_name(assertion)).changed() {
                changed.what.clone_from(&what);
                changed.what.retain(|kind| *kind != assertion);
                if on {
                    changed.what.push(assertion);
                }
                changed.what.sort();
            }
        }
        ui.add_space(12.0);
        ui.label(egui::RichText::new(t!("ide-caffeine-menu-battery")).font(theme::semibold(13.0)).color(p.text));
        ui.checkbox(&mut changed.off_on_battery, t!("ide-caffeine-off-on-battery"));
        ui.horizontal(|ui| {
            ui.checkbox(&mut changed.low_battery, t!("ide-caffeine-low-battery"));
            let mut level = self.caffeine_ui.battery_level.unwrap_or(settings.low_battery_level);
            let field = ui.add(egui::DragValue::new(&mut level).range(5..=95).suffix("%"));
            accessibility::labelled(&field, &t!("ide-caffeine-low-battery"));
            self.caffeine_ui.battery_level = Some(level);
            if field.drag_stopped() || field.lost_focus() || (field.changed() && !field.dragged()) {
                changed.low_battery_level = level;
            }
        });
        ui.checkbox(&mut changed.resume_on_charge, t!("ide-caffeine-resume"));
        ui.add_space(12.0);
        if ui.link(t!("ide-caffeine-settings")).clicked() {
            controls.push(Control::OpenSettings);
        }
        ui.add_space(8.0);
        if changed != *settings {
            controls.push(Control::Settings(changed));
        }
    }
}

#[cfg(test)]
mod tests;

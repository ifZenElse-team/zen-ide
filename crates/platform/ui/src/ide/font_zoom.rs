use std::collections::HashMap;
use std::path::PathBuf;

use egui::{Rect, Sense, Stroke, Ui, pos2, vec2};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_workbench::font_size::{DEFAULT_SIZE, EDITOR_FONT_SIZE, stepped};
use zen_workbench::groups::GroupId;
use zen_workbench::{Document, Workbench};

use super::settings::{DEFAULT_PRESENTATION_FONT_SIZE, EditorFont, PRESENTATION_FONT_SIZES, WheelFontSize};
use super::{IdeView, Request};
use crate::{accessibility, theme};

const INDICATOR_SECONDS: f64 = 3.0;

const WHEEL_POINTS_PER_STEP: f32 = 40.0;

#[derive(Clone, Debug, PartialEq)]
struct Shown {
    path: PathBuf,
    own: Option<f32>,
    base: f32,
    presents: bool,
    until: f64,
}

#[derive(Default)]
pub(super) struct FontZoom {
    wheel: HashMap<GroupId, f32>,
    shown: HashMap<GroupId, Shown>,
}

impl IdeView {
    pub(super) fn change_all_editors_font(&mut self, steps: Option<f32>) -> serde_json::Value {
        let mut settings = self.overview.settings.clone();
        let (size, default) = if self.appearance.presents() {
            let size = steps.map_or(DEFAULT_PRESENTATION_FONT_SIZE, |steps| {
                (settings.presentation_font_size + steps)
                    .clamp(*PRESENTATION_FONT_SIZES.start(), *PRESENTATION_FONT_SIZES.end())
            });
            settings.presentation_font_size = size;
            (size, DEFAULT_PRESENTATION_FONT_SIZE)
        } else {
            let size = steps.map_or(DEFAULT_SIZE, |steps| stepped(settings.editor_font.size, steps));
            settings.editor_font.size = size;
            (size, DEFAULT_SIZE)
        };
        self.overview.settings = settings.clone();
        self.requests.push(Request::Settings(Box::new(settings)));
        serde_json::json!({ "size": size, "default": default })
    }

    pub(super) fn base_editor_font_size(&self) -> f32 {
        self.appearance.editor_font(&self.overview.settings).size
    }

    pub(super) fn share_editor_font_size(&self, session: &mut Session) {
        let _ = session.settings_mut().set(EDITOR_FONT_SIZE, self.base_editor_font_size());
    }

    pub(super) fn shown_editor_font(&self, session: &Session) -> EditorFont {
        let mut font = self.appearance.editor_font(&self.overview.settings);
        if let Some(size) =
            session.state::<Workbench>().and_then(Workbench::active_document).and_then(Document::font_size)
        {
            font.size = size;
        }
        font
    }

    pub(super) fn font_zoom(
        &mut self,
        ui: &mut Ui,
        registry: &Registry,
        session: &mut Session,
        group: GroupId,
        area: Rect,
    ) {
        self.font_wheel(ui, registry, session, group, area);
        let Some(document) = session.state::<Workbench>().and_then(Workbench::active_document) else { return };
        let now = ui.input(|input| input.time);
        let mut shown = Shown {
            path: document.path().to_owned(),
            own: document.font_size(),
            base: self.base_editor_font_size(),
            presents: self.appearance.presents(),
            until: 0.0,
        };
        let changed = self.font_zoom.shown.get(&group).is_some_and(|before| {
            before.path == shown.path
                && before.presents == shown.presents
                && (before.own != shown.own || (before.base != shown.base && !self.settings.is_open()))
        });
        let until = self.font_zoom.shown.get(&group).map_or(0.0, |before| before.until);
        shown.until = if changed { now + INDICATOR_SECONDS } else { until };
        let own = shown.own;
        let until = shown.until;
        self.font_zoom.shown.insert(group, shown);
        if !self.overview.settings.zoom_indicator || now >= until {
            return;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(until - now));
        let size = self.editor_view.font.size;
        let default = match (own, self.appearance.presents()) {
            (Some(_), _) => self.base_editor_font_size(),
            (None, true) => DEFAULT_PRESENTATION_FONT_SIZE,
            (None, false) => DEFAULT_SIZE,
        };
        if self.zoom_indicator(ui, area, size, default) {
            if own.is_some() {
                self.dispatch(ui, registry, session, "editor.fontSize.reset", serde_json::json!({}));
            } else {
                self.change_all_editors_font(None);
            }
        }
    }

    fn font_wheel(&mut self, ui: &Ui, registry: &Registry, session: &mut Session, group: GroupId, area: Rect) {
        let settings = &self.overview.settings;
        if !settings.wheel_font_size || !ui.rect_contains_pointer(area) {
            self.font_zoom.wheel.remove(&group);
            return;
        }
        let travel: f32 = ui.input(|input| {
            input
                .events
                .iter()
                .map(|event| match event {
                    egui::Event::MouseWheel { unit, delta, modifiers, .. } if modifiers.command => match unit {
                        egui::MouseWheelUnit::Point => delta.y / WHEEL_POINTS_PER_STEP,
                        egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => delta.y.signum(),
                    },
                    _ => 0.0,
                })
                .sum()
        });
        let wheel = self.font_zoom.wheel.entry(group).or_default();
        *wheel += travel;
        let steps = wheel.trunc();
        if steps == 0.0 {
            return;
        }
        *wheel -= steps;
        match settings.wheel_font_size_in {
            WheelFontSize::AllEditors => {
                self.change_all_editors_font(Some(steps));
            }
            WheelFontSize::ActiveEditor => {
                let command = if steps > 0.0 { "editor.fontSize.increase" } else { "editor.fontSize.decrease" };
                for _ in 0..steps.abs() as usize {
                    self.dispatch(ui, registry, session, command, serde_json::json!({}));
                }
            }
        }
        ui.ctx().request_repaint();
    }

    fn zoom_indicator(&self, ui: &mut Ui, area: Rect, size: f32, default: f32) -> bool {
        let p = self.palette;
        let font = theme::regular(13.0);
        let painter = ui.painter();
        let size_text =
            painter.layout_no_wrap(t!("ide-zoom-indicator-size", size = points(size)), font.clone(), p.text);
        let default_text = painter.layout_no_wrap(
            t!("ide-zoom-indicator-default", size = points(default)),
            font.clone(),
            p.text_secondary,
        );
        let reset_label = t!("ide-zoom-indicator-reset");
        let reset = (size != default).then(|| painter.layout_no_wrap(reset_label.clone(), font, p.link));
        let gap = 14.0;
        let width = size_text.size().x
            + gap
            + default_text.size().x
            + reset.as_ref().map_or(0.0, |reset| gap + reset.size().x)
            + 2.0 * 12.0;
        let height = 28.0;
        let rect =
            Rect::from_center_size(pos2(area.center().x, area.bottom() - 16.0 - height / 2.0), vec2(width, height));
        let painter = ui.painter_at(area);
        painter.rect(rect, 6, p.popup, Stroke::new(1.0, p.popup_border), egui::StrokeKind::Inside);
        let y = rect.center().y;
        let mut x = rect.left() + 12.0;
        let size_width = size_text.size().x;
        painter.galley(pos2(x, y - size_text.size().y / 2.0), size_text, p.text);
        x += size_width + gap;
        let default_width = default_text.size().x;
        painter.galley(pos2(x, y - default_text.size().y / 2.0), default_text, p.text_secondary);
        x += default_width + gap;
        let Some(reset) = reset else { return false };
        let link = Rect::from_min_size(pos2(x, y - reset.size().y / 2.0), reset.size());
        let response = ui
            .interact(link, ui.scope_id().with("zoom-indicator-reset"), Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        accessibility::name(&response, egui::Role::Link, &reset_label);
        if response.hovered() {
            painter.line_segment([link.left_bottom(), link.right_bottom()], Stroke::new(1.0, p.link));
        }
        painter.galley(link.min, reset, p.link);
        response.clicked()
    }
}

impl FontZoom {
    #[cfg(test)]
    pub(super) fn indicator_until(&self) -> f64 {
        self.shown.values().map(|shown| shown.until).fold(0.0, f64::max)
    }
}

fn points(size: f32) -> String {
    if size.fract() == 0.0 { format!("{size:.0}") } else { format!("{size:.1}") }
}

#[cfg(test)]
mod tests {
    use super::points;

    #[test]
    fn sizes_are_written_without_a_needless_fraction() {
        assert_eq!(points(13.0), "13");
        assert_eq!(points(13.5), "13.5");
    }
}

use egui::{Color32, Key, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_i18n::t;

use super::settings::{DEFAULT_IDE_ZOOM, IDE_ZOOM_PRESETS, IDE_ZOOMS};
use super::{IdePalette, IdeView, Request, dialog_frame};
use crate::theme;

pub(super) const OPEN: &str = "view.ideZoom.open";
pub(super) const IN: &str = "view.ideZoom.in";
pub(super) const OUT: &str = "view.ideZoom.out";
pub(super) const RESET: &str = "view.ideZoom.reset";

pub(super) fn zoomed_in(current: u16) -> u16 {
    IDE_ZOOM_PRESETS.iter().copied().find(|preset| *preset > current).unwrap_or(current.max(*IDE_ZOOMS.start()))
}

pub(super) fn zoomed_out(current: u16) -> u16 {
    IDE_ZOOM_PRESETS.iter().rev().copied().find(|preset| *preset < current).unwrap_or(current.min(*IDE_ZOOMS.end()))
}

fn memory_id() -> egui::Id {
    egui::Id::unique("zen-ide-zoom")
}

pub(super) fn remember(ctx: &egui::Context, zoom: f32, ide: f32) {
    ctx.data_mut(|data| {
        let known = data.get_temp_mut_or_default::<Vec<(f32, f32)>>(memory_id());
        known.push((zoom, ide));
        let extra = known.len().saturating_sub(2);
        known.drain(..extra);
    });
}

pub(super) fn font_zoom(ctx: &egui::Context) -> f32 {
    let zoom = ctx.zoom_factor();
    let known = ctx.data(|data| data.get_temp::<Vec<(f32, f32)>>(memory_id())).unwrap_or_default();
    let ide = known.iter().rev().find(|(set, _)| *set == zoom).map_or(1.0, |(_, ide)| *ide);
    if ide > 0.0 { zoom / ide } else { zoom }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Choice {
    In,
    Out,
    Reset,
    Close,
}

fn show(ui: &mut Ui, p: &IdePalette, percent: u16) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("zoom-ide")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(300.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(egui::RichText::new(t!("ide-zoom-title")).font(theme::semibold(15.0)).color(p.text));
        ui.add_space(14.0);
        let step = |text: &str| {
            egui::Button::new(egui::RichText::new(text).font(theme::regular(15.0)).color(p.text))
                .min_size(vec2(36.0, 30.0))
                .corner_radius(6)
                .fill(Color32::TRANSPARENT)
                .stroke(Stroke::new(1.0, p.button_border))
        };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let out =
                ui.add_enabled(percent > IDE_ZOOM_PRESETS[0], step("−")).on_hover_text(t!("keymap-view-ideZoom-out"));
            if out.clicked() {
                choice = Some(Choice::Out);
            }
            let shown =
                egui::RichText::new(t!("ide-zoom-percent", percent = percent)).font(theme::regular(14.0)).color(p.text);
            ui.add_sized(vec2(64.0, 30.0), egui::Label::new(shown));
            let last = IDE_ZOOM_PRESETS[IDE_ZOOM_PRESETS.len() - 1];
            let zoom_in = ui.add_enabled(percent < last, step("+")).on_hover_text(t!("keymap-view-ideZoom-in"));
            if zoom_in.clicked() {
                choice = Some(Choice::In);
            }
        });
        ui.add_space(20.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let close = egui::Button::new(
                egui::RichText::new(t!("ide-zoom-close")).font(theme::regular(13.0)).color(p.accent_text),
            )
            .min_size(vec2(84.0, 30.0))
            .corner_radius(6)
            .fill(p.accent);
            if ui.add(close).clicked() {
                choice = Some(Choice::Close);
            }
            let reset =
                egui::Button::new(egui::RichText::new(t!("ide-zoom-reset")).font(theme::regular(13.0)).color(p.text))
                    .min_size(vec2(84.0, 30.0))
                    .corner_radius(6)
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, p.button_border));
            if ui.add_enabled(percent != DEFAULT_IDE_ZOOM, reset).clicked() {
                choice = Some(Choice::Reset);
            }
        });
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            choice = Some(Choice::Close);
        }
    });
    choice.or_else(|| modal.should_close().then_some(Choice::Close))
}

impl IdeView {
    pub(super) fn ide_zoom(&self) -> u16 {
        self.overview.settings.ide_zoom
    }

    pub(super) fn set_ide_zoom(&mut self, percent: u16) -> u16 {
        let percent = percent.clamp(*IDE_ZOOMS.start(), *IDE_ZOOMS.end());
        if percent != self.overview.settings.ide_zoom {
            let mut settings = self.overview.settings.clone();
            settings.ide_zoom = percent;
            self.overview.settings = settings.clone();
            self.requests.push(Request::Settings(Box::new(settings)));
        }
        percent
    }

    pub(super) fn ide_zoom_action(&mut self, action: &str) -> Option<Value> {
        let current = self.ide_zoom();
        let percent = match action {
            OPEN => {
                self.ide_zoom_dialog = true;
                current
            }
            IN => self.set_ide_zoom(zoomed_in(current)),
            OUT => self.set_ide_zoom(zoomed_out(current)),
            RESET => self.set_ide_zoom(DEFAULT_IDE_ZOOM),
            _ => return None,
        };
        Some(json!({ "ideZoom": percent }))
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub(super) fn ide_zoom_title(&self) -> String {
        t!("menu-view-ideZoom", percent = self.ide_zoom())
    }

    pub(super) fn ide_zoom_dialog(&mut self, ui: &mut Ui) {
        if !self.ide_zoom_dialog {
            return;
        }
        let p = self.palette;
        let choice = show(ui, &p, self.ide_zoom());
        let action = match choice {
            Some(Choice::In) => IN,
            Some(Choice::Out) => OUT,
            Some(Choice::Reset) => RESET,
            Some(Choice::Close) => {
                self.ide_zoom_dialog = false;
                return;
            }
            None => return,
        };
        let _ = self.ide_zoom_action(action);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_in_and_out_step_through_the_presets() {
        assert_eq!(zoomed_in(100), 110);
        assert_eq!(zoomed_in(110), 125);
        assert_eq!(zoomed_in(115), 125, "a custom value goes to the next preset");
        assert_eq!(zoomed_in(200), 200, "the largest preset is the last step");
        assert_eq!(zoomed_in(250), 250);
        assert_eq!(zoomed_out(100), 90);
        assert_eq!(zoomed_out(115), 110);
        assert_eq!(zoomed_out(70), 70, "the smallest preset is the last step");
        assert_eq!(zoomed_out(60), 60);
        assert_eq!(zoomed_out(250), 200);
        assert_eq!(zoomed_in(60), 70);
    }

    #[test]
    fn the_dialog_draws() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(std::path::PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for language in languages {
            zen_i18n::set_language(Some(language));
            for percent in [100, 125] {
                let mut screen = crate::offscreen::Offscreen::new(vec2(600.0, 300.0), 2.0);
                let palette = IdePalette::zen_dark();
                let mut chosen = None;
                let image = screen.render(|ui| chosen = show(ui, &palette, percent));
                assert!(image.width() > 0 && chosen.is_none());
                if let Some(dir) = &snapshots {
                    let path = dir.join(format!("zoom-ide-{language}-{percent}.png"));
                    crate::offscreen::write_png(&path, &image).unwrap();
                }
            }
        }
        zen_i18n::set_language(Some("en"));
    }

    #[test]
    fn the_font_zoom_leaves_out_the_ide_zoom() {
        let ctx = egui::Context::default();
        ctx.set_zoom_factor(3.0);
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        assert_eq!(font_zoom(&ctx), 3.0, "without an IDE zoom the whole factor is the font's");
        remember(&ctx, 3.0, 1.5);
        assert_eq!(font_zoom(&ctx), 2.0);
        remember(&ctx, 1.0, 1.0);
        assert_eq!(font_zoom(&ctx), 2.0, "until the new zoom is in use, the old one's IDE zoom holds");
        ctx.set_zoom_factor(1.0);
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        assert_eq!(font_zoom(&ctx), 1.0);
    }
}

use egui::{Align, Layout, RichText, Role, Stroke, Ui, vec2};

use super::IdePalette;
use crate::{accessibility, theme};

pub(super) const ROW: f32 = 28.0;

pub(super) fn label(ui: &mut Ui, p: &IdePalette, text: &str, width: f32) {
    ui.allocate_ui_with_layout(vec2(width, ROW), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_width(width);
        ui.add(egui::Label::new(RichText::new(text).font(theme::regular(13.0)).color(p.text)).extend());
    });
}

pub(super) fn framed<R>(ui: &mut Ui, p: &IdePalette, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        let widgets = &mut ui.visuals_mut().widgets;
        for state in [&mut widgets.inactive, &mut widgets.hovered, &mut widgets.active, &mut widgets.open] {
            state.bg_fill = p.text_field;
            state.weak_bg_fill = p.text_field;
            state.bg_stroke = Stroke::new(1.0, p.text_field_border);
            state.fg_stroke.color = p.text;
            state.expansion = 0.0;
        }
        widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent);
        widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
        widgets.open.bg_stroke = Stroke::new(1.0, p.accent);
        widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.text_field_border);
        add(ui)
    })
    .inner
}

pub(super) fn check(ui: &mut Ui, p: &IdePalette, value: &mut bool, name: &str) -> egui::Response {
    let response = framed(ui, p, |ui| ui.checkbox(value, RichText::new(name).font(theme::regular(13.0)).color(p.text)));
    accessibility::name(&response, Role::CheckBox, name);
    response
}

pub(super) fn radio<T: PartialEq>(ui: &mut Ui, p: &IdePalette, value: &mut T, choice: T, name: &str) -> egui::Response {
    let response =
        framed(ui, p, |ui| ui.radio_value(value, choice, RichText::new(name).font(theme::regular(13.0)).color(p.text)));
    accessibility::name(&response, Role::RadioButton, name);
    response
}

pub(super) fn combo(
    ui: &mut Ui,
    p: &IdePalette,
    id: impl std::hash::Hash + std::fmt::Debug,
    name: &str,
    shown: &str,
    width: f32,
    choices: impl FnOnce(&mut Ui),
) -> egui::Response {
    let response = framed(ui, p, |ui| {
        ui.spacing_mut().interact_size.y = 26.0;
        egui::ComboBox::from_id_salt(id)
            .selected_text(RichText::new(shown).font(theme::regular(13.0)).color(p.text))
            .width(width)
            .truncate()
            .show_ui(ui, choices)
            .response
    });
    accessibility::name(&response, Role::ComboBox, name);
    response
}

pub(super) fn segmented(ui: &mut Ui, p: &IdePalette, labels: &[&str], selected: Option<usize>) -> Option<usize> {
    let font = theme::regular(13.0);
    let widths: Vec<f32> = labels
        .iter()
        .map(|label| ui.painter().layout_no_wrap((*label).to_owned(), font.clone(), p.text).size().x + 32.0)
        .collect();
    let (outer, _) = ui.allocate_exact_size(vec2(widths.iter().sum(), ROW), egui::Sense::hover());
    ui.painter().rect_stroke(outer, 5, Stroke::new(1.0, p.text_field_border), egui::StrokeKind::Inside);
    let mut chosen = None;
    let mut left = outer.left();
    for (index, (label, width)) in labels.iter().zip(&widths).enumerate() {
        let segment = egui::Rect::from_min_size(egui::pos2(left, outer.top()), vec2(*width, ROW));
        left += width;
        let response = ui.interact(segment, ui.scope_id().with(("segment", *label, index)), egui::Sense::click());
        let on = selected == Some(index);
        accessibility::item(&response, Role::RadioButton, on, label);
        if on {
            ui.painter().rect_filled(segment, 5, p.hover);
            ui.painter().rect_stroke(segment, 5, Stroke::new(1.0, p.text_secondary), egui::StrokeKind::Inside);
        }
        ui.painter().text(segment.center(), egui::Align2::CENTER_CENTER, *label, font.clone(), p.text);
        if response.clicked() && !on {
            chosen = Some(index);
        }
    }
    chosen
}

pub(super) fn solid_scroll_bars<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        let scroll = &mut ui.style_mut().spacing.scroll;
        scroll.dormant_handle_opacity = 0.6;
        scroll.dormant_background_opacity = 0.0;
        add(ui)
    })
    .inner
}

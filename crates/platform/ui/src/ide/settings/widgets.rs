use std::ops::{Range, RangeInclusive};

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, Response, Ui};
use zen_i18n::t;

use super::super::IdePalette;
use crate::theme;

pub(in crate::ide) fn search_match(label: &str, query: &str) -> Option<Range<usize>> {
    let query: Vec<char> = query.trim().chars().flat_map(char::to_lowercase).collect();
    if query.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = label.char_indices().collect();
    for start in 0..chars.len() {
        let mut wanted = query.iter();
        let mut end = None;
        'label: for &(index, ch) in &chars[start..] {
            for lower in ch.to_lowercase() {
                match wanted.next() {
                    Some(expected) if *expected == lower => {}
                    _ => break 'label,
                }
            }
            if wanted.len() == 0 {
                end = Some(index + ch.len_utf8());
                break;
            }
        }
        if let Some(end) = end {
            return Some(chars[start].0..end);
        }
    }
    None
}

pub(in crate::ide) fn highlighted(
    label: &str,
    query: Option<&str>,
    font: FontId,
    color: Color32,
    highlight: Color32,
) -> LayoutJob {
    let plain = TextFormat { font_id: font.clone(), color, ..TextFormat::default() };
    let mut job = LayoutJob::default();
    match query.and_then(|query| search_match(label, query)) {
        Some(found) => {
            job.append(&label[..found.start], 0.0, plain.clone());
            job.append(&label[found.clone()], 0.0, TextFormat { background: highlight, ..plain.clone() });
            job.append(&label[found.end..], 0.0, plain);
        }
        None => job.append(label, 0.0, plain),
    }
    job
}

fn body(ui: &Ui) -> FontId {
    egui::TextStyle::Body.resolve(ui.style())
}

fn first_match_id() -> egui::Id {
    egui::Id::unique("settings-first-match")
}

pub(in crate::ide) fn forget_first_match(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove_temp::<f32>(first_match_id()));
}

pub(in crate::ide) fn first_match(ctx: &egui::Context) -> Option<f32> {
    ctx.data(|data| data.get_temp::<f32>(first_match_id()))
}

pub(in crate::ide) fn note_match(ui: &Ui, label: &str, query: Option<&str>) {
    if query.is_some_and(|query| search_match(label, query).is_some()) {
        let top = ui.cursor().top();
        ui.ctx().data_mut(|data| {
            data.get_temp_mut_or(first_match_id(), top);
        });
    }
}

pub(in crate::ide) fn label_job(ui: &Ui, p: &IdePalette, label: &str, query: Option<&str>) -> LayoutJob {
    note_match(ui, label, query);
    highlighted(label, query, body(ui), p.text, p.search_highlight)
}

pub(in crate::ide) fn group_title(ui: &mut Ui, p: &IdePalette, label: &str, query: Option<&str>) -> Response {
    note_match(ui, label, query);
    let response = ui.label(highlighted(label, query, theme::semibold(13.0), p.text, p.search_highlight));
    ui.add_space(6.0);
    response
}

pub(in crate::ide) fn checkbox(
    ui: &mut Ui,
    p: &IdePalette,
    value: &mut bool,
    label: &str,
    query: Option<&str>,
) -> Response {
    let job = label_job(ui, p, label, query);
    ui.checkbox(value, job)
}

pub(in crate::ide) fn field_width(ui: &Ui, least: f32, most: f32) -> f32 {
    let left = ui.available_width();
    if left >= least { left.min(most) } else { ui.max_rect().width().min(most) }
}

pub(in crate::ide) fn number_field<N: egui::emath::Numeric>(
    ui: &mut Ui,
    p: &IdePalette,
    label: &str,
    query: Option<&str>,
    value: &mut N,
    range: RangeInclusive<N>,
) -> Response {
    ui.horizontal_wrapped(|ui| {
        let label = ui.label(label_job(ui, p, label, query));
        ui.add(egui::DragValue::new(value).range(range)).labelled_by(label.id)
    })
    .inner
}

pub(in crate::ide) fn text_field(
    ui: &mut Ui,
    p: &IdePalette,
    label: &str,
    query: Option<&str>,
    value: &mut String,
) -> Response {
    ui.horizontal_wrapped(|ui| {
        let label = ui.label(label_job(ui, p, label, query));
        ui.add(egui::TextEdit::singleline(value).desired_width(field_width(ui, 140.0, 380.0) - 20.0))
            .labelled_by(label.id)
    })
    .inner
}

pub(in crate::ide) fn path_field(
    ui: &mut Ui,
    p: &IdePalette,
    label: &str,
    query: Option<&str>,
    value: &mut String,
    folder: bool,
) -> Response {
    ui.horizontal_wrapped(|ui| {
        let label = ui.label(label_job(ui, p, label, query));
        let width = field_width(ui, 220.0, f32::INFINITY) - 90.0;
        let mut response = ui.add(egui::TextEdit::singleline(value).desired_width(width)).labelled_by(label.id);
        if !cfg!(target_arch = "wasm32")
            && ui.button(t!("settings-browse")).clicked()
            && let Some(path) = pick_path(folder)
        {
            *value = path.display().to_string();
            response.mark_changed();
        }
        response
    })
    .inner
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_path(folder: bool) -> Option<std::path::PathBuf> {
    let dialog = rfd::FileDialog::new();
    if folder { dialog.pick_folder() } else { dialog.pick_file() }
}

#[cfg(target_arch = "wasm32")]
fn pick_path(_folder: bool) -> Option<std::path::PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_match_ignores_case_and_surrounding_spaces() {
        assert_eq!(search_match("Enable ligatures", "LIGA"), Some(7..11));
        assert_eq!(search_match("Enable ligatures", "  enable "), Some(0..6));
        assert_eq!(search_match("Font", "font size"), None);
        assert_eq!(search_match("Font", ""), None);
        assert_eq!(search_match("Font", "   "), None);
        assert_eq!(search_match("", "a"), None);
    }

    #[test]
    fn a_match_is_found_in_translated_text() {
        let label = "Включить лигатуры";
        let found = search_match(label, "ЛИГАТ").unwrap();
        assert_eq!(&label[found], "лигат");
        assert_eq!(search_match("Стиль кода", "кОд").map(|found| &"Стиль кода"[found]), Some("код"));
    }

    #[test]
    fn the_first_occurrence_is_marked_after_a_false_start() {
        assert_eq!(search_match("aab", "ab"), Some(1..3));
        assert_eq!(search_match("tab tabs", "tabs"), Some(4..8));
    }

    #[test]
    fn only_the_match_gets_the_highlight() {
        let font = FontId::proportional(13.0);
        let (text, highlight) = (Color32::WHITE, Color32::GREEN);
        let job = highlighted("Tab size", Some("size"), font.clone(), text, highlight);
        assert_eq!(job.text, "Tab size");
        let parts: Vec<(&str, Color32)> = job
            .sections
            .iter()
            .map(|section| (&job.text[section.byte_range.start.0..section.byte_range.end.0], section.format.background))
            .collect();
        assert_eq!(parts, [("Tab ", Color32::TRANSPARENT), ("size", highlight), ("", Color32::TRANSPARENT)]);
        for query in [None, Some("indent")] {
            let job = highlighted("Tab size", query, font.clone(), text, highlight);
            assert_eq!(job.sections.len(), 1);
            assert_eq!(job.sections[0].format.background, Color32::TRANSPARENT);
        }
    }
}

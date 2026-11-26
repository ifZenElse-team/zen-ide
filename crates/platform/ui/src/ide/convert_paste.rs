use std::path::Path;

use egui::{Color32, Key, Stroke, Ui, vec2};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;

use super::{IdePalette, dialog_frame};
use crate::theme;

const COPIED: &str = "zen-copied-from";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PasteOffer {
    pub(super) from: String,
    pub(super) to: String,
    pub(super) text: String,
}

fn extension(path: &Path) -> Option<String> {
    path.extension().map(|extension| extension.to_string_lossy().to_ascii_lowercase())
}

pub(super) fn remember_copy(ctx: &egui::Context, path: &Path, text: &str) {
    let copied = extension(path).map(|extension| (extension, text.to_owned()));
    ctx.data_mut(|data| data.insert_temp(egui::Id::unique(COPIED), copied));
}

pub(super) fn offer(ctx: &egui::Context, session: &Session, path: &Path, text: &str) -> Option<PasteOffer> {
    let (from, copied) =
        ctx.data(|data| data.get_temp::<Option<(String, String)>>(egui::Id::unique(COPIED))).flatten()?;
    let to = extension(path)?;
    if copied != text || from == to {
        return None;
    }
    zen_workbench::plugins::convert::converter(session, &from, &to)?;
    Some(PasteOffer { from, to, text: text.to_owned() })
}

fn language(extension: &str) -> String {
    zen_vfs::file_types::current()
        .type_of(&format!("pasted.{extension}"))
        .filter(|file_type| !file_type.localized && !file_type.name.trim().is_empty())
        .map_or_else(|| format!(".{extension}"), |file_type| file_type.name)
}

pub(super) fn show(ui: &mut Ui, p: &IdePalette, offer: &PasteOffer) -> Option<bool> {
    let mut choice = None;
    let question = t!("ide-convert-paste-question", from = language(&offer.from), to = language(&offer.to));
    let modal = egui::Modal::new(ui.scope_id().with("convert-paste")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add(
            egui::Label::new(
                egui::RichText::new(t!("ide-convert-paste-title")).font(theme::semibold(15.0)).color(p.text),
            )
            .wrap(),
        );
        ui.add_space(10.0);
        ui.add(egui::Label::new(egui::RichText::new(question).font(theme::regular(13.0)).color(p.text)).wrap());
        ui.add_space(20.0);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let yes = egui::Button::new(
                egui::RichText::new(t!("ide-convert-paste-yes")).font(theme::regular(13.0)).color(p.accent_text),
            )
            .min_size(vec2(84.0, 30.0))
            .corner_radius(6)
            .fill(p.accent);
            if ui.add(yes).clicked() {
                choice = Some(true);
            }
            let no = egui::Button::new(
                egui::RichText::new(t!("ide-convert-paste-no")).font(theme::regular(13.0)).color(p.text),
            )
            .min_size(vec2(84.0, 30.0))
            .corner_radius(6)
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, p.button_border));
            if ui.add(no).clicked() {
                choice = Some(false);
            }
        });
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Enter)) {
            choice = Some(true);
        }
    });
    choice.or_else(|| modal.should_close().then_some(false))
}

pub(super) fn paste(registry: &Registry, session: &mut Session, offer: &PasteOffer, convert: bool) -> Option<String> {
    let mut text = offer.text.clone();
    let mut notice = None;
    if convert {
        let params = json!({ "from": offer.from, "to": offer.to, "text": offer.text });
        match registry.dispatch(session, "editor.convert.text", params) {
            Ok(answer) => {
                if let Some(converted) = answer.get("text").and_then(Value::as_str) {
                    text = converted.trim_end_matches('\n').to_owned();
                }
                let problems = answer.get("problems").and_then(Value::as_array).map_or(0, Vec::len);
                if problems > 0 {
                    notice = Some(t!("ide-convert-paste-problems", count = problems));
                }
            }
            Err(error) => notice = Some(error.to_string()),
        }
    }
    let _ = registry.dispatch(session, "editor.edit.paste", json!({ "text": text }));
    notice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_text_copied_from_another_language_is_offered() {
        let ctx = egui::Context::default();
        let session = Session::new();
        remember_copy(&ctx, Path::new("/p/A.java"), "int x = 1;");
        assert_eq!(offer(&ctx, &session, Path::new("/p/B.kt"), "int y = 2;"), None, "other text: copied elsewhere");
        assert_eq!(offer(&ctx, &session, Path::new("/p/B.java"), "int x = 1;"), None, "the same language");
        assert_eq!(offer(&ctx, &session, Path::new("/p/B.kt"), "int x = 1;"), None, "no plug-in converts here");
        assert_eq!(language("zzunknown"), ".zzunknown", "a type the IDE does not know is named by its extension");
    }
}

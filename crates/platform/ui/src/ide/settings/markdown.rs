use egui::Ui;
use zen_i18n::t;
use zen_workbench::preview::{MARKDOWN_FONT_SIZES, PreviewMode};

use super::{SettingsDialog, widgets};
use crate::ide::IdePalette;

fn layout_name(mode: PreviewMode) -> String {
    match mode {
        PreviewMode::EditorOnly => t!("ide-preview-editor-only"),
        PreviewMode::EditorAndPreview => t!("ide-preview-editor-and-preview"),
        PreviewMode::PreviewOnly => t!("ide-preview-preview-only"),
    }
}

impl SettingsDialog {
    pub(super) fn markdown_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let markdown = &mut draft.markdown;
        ui.horizontal_wrapped(|ui| {
            let job = widgets::label_job(ui, p, &t!("settings-markdown-preview-layout"), query);
            let title = ui.label(job);
            let layout = &mut markdown.preview_layout;
            let combo = egui::ComboBox::from_id_salt("settings-markdown-layout")
                .selected_text(layout_name(*layout))
                .width(widgets::field_width(ui, 140.0, 240.0))
                .show_ui(ui, |ui| {
                    for choice in [PreviewMode::EditorOnly, PreviewMode::EditorAndPreview, PreviewMode::PreviewOnly] {
                        ui.selectable_value(layout, choice, layout_name(choice));
                    }
                });
            combo.response.labelled_by(title.id);
        });
        widgets::checkbox(ui, p, &mut markdown.sync_scroll, &t!("settings-markdown-sync-scroll"), query);
        widgets::number_field(
            ui,
            p,
            &t!("settings-markdown-preview-font-size"),
            query,
            &mut markdown.preview_font_size,
            MARKDOWN_FONT_SIZES,
        );
    }
}

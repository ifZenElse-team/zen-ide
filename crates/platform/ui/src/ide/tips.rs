use egui::Ui;
use serde_json::{Value, json};
use zen_i18n::t;

use super::{IdePalette, IdeView, Request, dialog_frame, keymap_page};
use crate::theme;

const TIPS: &[(&str, &str)] = &[
    ("ide-tip-search-everywhere", "view.searchEverywhere.open"),
    ("ide-tip-find-action", "view.findAction.open"),
    ("ide-tip-recent-files", "view.recentFiles.open"),
    ("ide-tip-recent-locations", "view.recentLocations.open"),
    ("ide-tip-goto-file", "view.gotoFile.open"),
    ("ide-tip-goto-symbol", "view.gotoSymbol.open"),
    ("ide-tip-goto-line", "view.gotoLine.open"),
    ("ide-tip-file-structure", "view.fileStructure.open"),
    ("ide-tip-declaration", "navigate.declaration.goTo"),
    ("ide-tip-back", "navigate.history.back"),
    ("ide-tip-last-edit", "navigate.edit.last"),
    ("ide-tip-rename", "view.rename.open"),
    ("ide-tip-intentions", "view.intentions.open"),
    ("ide-tip-reformat", "editor.code.reformat"),
    ("ide-tip-duplicate-line", "editor.line.duplicate"),
    ("ide-tip-move-line", "editor.line.moveUp"),
    ("ide-tip-comment", "editor.comment.line"),
    ("ide-tip-completion", "editor.completion.start"),
    ("ide-tip-documentation", "editor.documentation.show"),
    ("ide-tip-parameter-info", "editor.parameterInfo.show"),
    ("ide-tip-paste-history", "view.pasteFromHistory.open"),
    ("ide-tip-commit", "view.toolWindow.commit"),
    ("ide-tip-terminal", "view.toolWindow.terminal"),
    ("ide-tip-settings", "view.settings.open"),
];

fn text(index: usize) -> String {
    let (key, action) = TIPS[index % TIPS.len()];
    let shortcut = crate::keymap::shortcut(action).unwrap_or_else(|| keymap_page::action_title(action));
    t!(key, shortcut = shortcut)
}

fn answer(index: usize) -> Value {
    let index = index % TIPS.len();
    json!({ "index": index, "count": TIPS.len(), "action": TIPS[index].1, "text": text(index) })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TipDialog {
    index: usize,
}

enum Choice {
    Previous,
    Next,
    Close,
}

fn show(ui: &mut Ui, p: &IdePalette, index: usize, on_startup: &mut bool) -> Option<Choice> {
    let mut choice = None;
    let modal = egui::Modal::new(ui.scope_id().with("tip-of-the-day")).frame(dialog_frame(p)).show(ui.ctx(), |ui| {
        ui.set_width(460.0);
        ui.spacing_mut().item_spacing.y = 10.0;
        ui.label(egui::RichText::new(t!("keymap-view-tipOfTheDay-open")).font(theme::semibold(13.0)).color(p.text));
        ui.add_sized(
            [460.0, 72.0],
            egui::Label::new(egui::RichText::new(text(index)).font(theme::regular(14.0)).color(p.text)).wrap(),
        );
        let mut off = !*on_startup;
        if ui.checkbox(&mut off, t!("ide-tip-dont-show")).changed() {
            *on_startup = !off;
        }
        ui.horizontal(|ui| {
            if ui.button(t!("ide-tip-previous")).clicked() {
                choice = Some(Choice::Previous);
            }
            if ui.button(t!("ide-tip-next")).clicked() {
                choice = Some(Choice::Next);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if super::vcs::primary_button(ui, p, &t!("ide-tip-close")).clicked() {
                    choice = Some(Choice::Close);
                }
            });
        });
    });
    choice.or_else(|| modal.should_close().then_some(Choice::Close))
}

impl IdeView {
    pub(super) fn open_tip(&mut self, params: &Value) -> Value {
        let saved = self.overview.settings.tip_index;
        let index = params.get("index").and_then(Value::as_u64).map_or(saved as usize, |index| index as usize);
        self.tip = Some(TipDialog { index: index % TIPS.len() });
        answer(index)
    }

    pub(super) fn tip_is_open(&self) -> bool {
        self.tip.is_some()
    }

    pub(super) fn tip_dialog(&mut self, ui: &mut Ui) {
        let p = self.palette;
        let Some(dialog) = &mut self.tip else { return };
        let before = self.overview.settings.show_tips_on_startup;
        let mut on_startup = before;
        let choice = show(ui, &p, dialog.index, &mut on_startup);
        let mut settings = (on_startup != before).then(|| self.overview.settings.clone());
        match choice {
            Some(Choice::Previous) => dialog.index = (dialog.index + TIPS.len() - 1) % TIPS.len(),
            Some(Choice::Next) => dialog.index = (dialog.index + 1) % TIPS.len(),
            Some(Choice::Close) => {
                let next = ((dialog.index + 1) % TIPS.len()) as u32;
                settings.get_or_insert_with(|| self.overview.settings.clone()).tip_index = next;
                self.tip = None;
            }
            None => {}
        }
        if let Some(mut settings) = settings {
            settings.show_tips_on_startup = on_startup;
            self.overview.settings.clone_from(&settings);
            self.requests.push(Request::Settings(Box::new(settings)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tip_is_written_in_every_language_and_names_a_command_of_the_keymap() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let keymap = crate::keymap::current().bindings();
        for (key, action) in TIPS {
            assert!(keymap.iter().any(|binding| binding.action == *action), "{action} is not in the keymap");
            for language in ["en", "ru"] {
                let text = zen_i18n::with_language(language, || t!(key, shortcut = "⇧⌘Z"));
                assert!(text.contains("⇧⌘Z"), "{language}: {key} does not show the shortcut: {text}");
            }
        }
    }

    #[test]
    fn the_shortcut_comes_from_the_keymap() {
        zen_i18n::register(crate::toolbox::LOCALES);
        let shortcut = crate::keymap::shortcut("view.findAction.open").unwrap();
        assert!(text(1).contains(&shortcut), "{}", text(1));
        assert_eq!(answer(1 + TIPS.len())["index"], 1, "the tips go round");
    }
}

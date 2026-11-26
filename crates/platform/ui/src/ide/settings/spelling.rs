use std::path::PathBuf;

use egui::{Rect, Sense, Ui, vec2};
use serde_json::json;
use zen_i18n::t;
use zen_workbench::spelling::{Dictionaries, DictionaryLevel};

use super::{Dispatch, SettingsDialog, widgets};
use crate::accessibility;
use crate::ide::IdePalette;
use crate::theme;

const LIST_HEIGHT: f32 = 150.0;

const ROW: f32 = 22.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Add,
    Remove,
    Edit,
}

#[derive(Default)]
pub(super) struct SpellingPage {
    loaded: bool,
    original: Vec<String>,
    words: Vec<String>,
    selected_word: Option<usize>,
    word_field: Option<String>,
    selected_dictionary: Option<usize>,
    path_field: Option<String>,
    read: Vec<zen_workbench::spelling::CustomDictionary>,
    message: Option<String>,
    pub(super) open: Option<PathBuf>,
}

impl SpellingPage {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn begin(&mut self, dispatch: &mut Dispatch<'_>) {
        self.reset();
        self.load(dispatch);
    }

    fn load(&mut self, dispatch: &mut Dispatch<'_>) {
        match dispatch("spelling.dictionaries.get", json!({}))
            .and_then(|value| serde_json::from_value::<Dictionaries>(value).map_err(|error| error.to_string()))
        {
            Ok(dictionaries) => {
                let mut words = dictionaries.application.words;
                words.extend(dictionaries.project.words);
                words.sort();
                words.dedup();
                self.original.clone_from(&words);
                self.words = words;
                self.read = dictionaries.custom;
                self.loaded = true;
            }
            Err(error) => self.message = Some(error),
        }
        self.selected_word = self.selected_word.filter(|index| *index < self.words.len());
    }

    fn requests(&self, level: DictionaryLevel) -> Vec<(&'static str, serde_json::Value)> {
        if !self.loaded {
            return Vec::new();
        }
        let mut requests = Vec::new();
        for word in &self.original {
            if !self.words.contains(word) {
                requests.push(("spelling.word.remove", json!({ "word": word })));
            }
        }
        for word in &self.words {
            if !self.original.contains(word) {
                requests.push(("spelling.word.save", json!({ "word": word, "level": level })));
            }
        }
        requests
    }

    pub(super) fn commit(&mut self, dispatch: &mut Dispatch<'_>, level: DictionaryLevel) {
        let mut failed = None;
        for (command, params) in self.requests(level) {
            if let Err(error) = dispatch(command, params) {
                failed = Some(error);
            }
        }
        if self.loaded {
            self.load(dispatch);
        }
        self.message = failed;
    }

    #[cfg(test)]
    pub(super) fn words(&self) -> &[String] {
        &self.words
    }

    fn add_word(&mut self, word: &str) {
        let word = word.trim().to_ascii_lowercase();
        if word.is_empty() || !word.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'\'') {
            self.message = Some(t!("settings-spelling-bad-word", word = word.as_str()));
            return;
        }
        if !self.words.contains(&word) {
            self.words.push(word.clone());
            self.words.sort();
        }
        self.selected_word = self.words.iter().position(|kept| *kept == word);
        self.message = None;
    }
}

fn toolbar(ui: &mut Ui, p: &IdePalette, id: &str, can_change: bool, edit: bool) -> Option<Tool> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let tools: [(Tool, super::super::glyphs::Glyph, &str, bool); 3] = [
            (Tool::Add, super::super::glyphs::glyph_plus, "settings-spelling-add", true),
            (Tool::Remove, super::super::glyphs::glyph_minus, "settings-spelling-remove", can_change),
            (Tool::Edit, super::super::glyphs::glyph_pencil, "settings-spelling-edit", can_change),
        ];
        for (tool, glyph, name, enabled) in tools.into_iter().filter(|(tool, ..)| edit || *tool != Tool::Edit) {
            let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
            let response = ui.interact(
                rect,
                ui.scope_id().with((id, name)),
                if enabled { Sense::click() } else { Sense::hover() },
            );
            accessibility::button(&response, &t!(name));
            if !enabled {
                accessibility::disabled(&response);
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 4, p.popup_hover);
            }
            glyph(ui.painter(), rect.shrink(6.0), if enabled { p.text } else { p.gutter_text });
            if enabled && response.clicked() {
                picked = Some(tool);
            }
        }
    });
    picked
}

fn list_frame<R>(ui: &mut Ui, p: &IdePalette, id: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .stroke(egui::Stroke::new(1.0, p.text_field_border))
        .corner_radius(4)
        .inner_margin(4)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt(id)
                .max_height(LIST_HEIGHT)
                .min_scrolled_height(LIST_HEIGHT)
                .auto_shrink([false, false])
                .show(ui, body)
                .inner
        })
        .inner
}

fn row(ui: &mut Ui, p: &IdePalette, text: &str, detail: Option<(&str, bool)>, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::click());
    accessibility::tree_item(&response, 1, None, selected, text);
    if selected {
        ui.painter().rect_filled(rect, 3, p.selection);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 3, p.hover);
    }
    let galley = ui.painter().layout_no_wrap(text.to_owned(), theme::regular(13.0), p.text);
    let width = galley.size().x;
    ui.painter().with_clip_rect(rect).galley(rect.left_center() + vec2(6.0, -galley.size().y / 2.0), galley, p.text);
    if let Some((detail, error)) = detail {
        let color = if error { p.diagnostic_error } else { p.text_secondary };
        let left = rect.left() + 6.0 + width + 10.0;
        let room = Rect::from_min_max(egui::pos2(left, rect.top()), rect.max);
        ui.painter().with_clip_rect(room).text(
            egui::pos2(left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            detail,
            theme::regular(12.0),
            color,
        );
    }
    response
}

pub(super) fn field_row(ui: &mut Ui, value: &mut String, hint: &str, id: &str) -> Option<bool> {
    let response = ui.add(
        egui::TextEdit::singleline(value)
            .id(egui::Id::unique(("settings-spelling-field", id)))
            .hint_text(hint)
            .desired_width(ui.available_width()),
    );
    accessibility::labelled(&response, hint);
    if !response.has_focus() && !response.lost_focus() {
        response.request_focus();
    }
    let (enter, escape) = ui.input(|input| (input.key_pressed(egui::Key::Enter), input.key_pressed(egui::Key::Escape)));
    if response.lost_focus() && enter {
        Some(true)
    } else if escape {
        Some(false)
    } else {
        None
    }
}

impl SettingsDialog {
    pub(super) fn natural_languages_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        widgets::group_title(ui, p, &t!("settings-natural-languages-languages"), query);
        list_frame(ui, p, "settings-natural-languages", |ui| {
            row(ui, p, &t!("settings-natural-languages-english"), None, false);
        });
        ui.add_space(8.0);
        if ui.link(egui::RichText::new(t!("settings-natural-languages-configure")).color(p.link)).clicked() {
            self.section = super::PageId::Inspections;
        }
    }

    pub(super) fn spelling_page(&mut self, ui: &mut Ui, p: &IdePalette, query: Option<&str>) {
        let Some(draft) = self.draft.as_mut() else { return };
        let page = &mut self.spelling;
        let spelling = &mut draft.spelling;
        ui.horizontal_wrapped(|ui| {
            let checkbox =
                widgets::checkbox(ui, p, &mut spelling.use_single_dictionary, &t!("settings-spelling-single"), query);
            let width = 150.0_f32.min(ui.max_rect().width());
            if ui.max_rect().right() - checkbox.rect.right() - ui.spacing().item_spacing.x < width {
                ui.end_row();
            }
            ui.add_enabled_ui(spelling.use_single_dictionary, |ui| {
                let name = |level: DictionaryLevel| match level {
                    DictionaryLevel::Project => t!("settings-spelling-project-level"),
                    DictionaryLevel::Application => t!("settings-spelling-application-level"),
                };
                let combo = egui::ComboBox::from_id_salt("settings-spelling-level")
                    .selected_text(name(spelling.saving_dictionary))
                    .width(width)
                    .truncate()
                    .show_ui(ui, |ui| {
                        for level in [DictionaryLevel::Project, DictionaryLevel::Application] {
                            ui.selectable_value(&mut spelling.saving_dictionary, level, name(level));
                        }
                    });
                accessibility::labelled(&combo.response, &t!("settings-spelling-single"));
            });
        });
        ui.add_space(12.0);
        widgets::note_match(ui, &t!("settings-spelling-custom"), query);
        let title = widgets::highlighted(
            &t!("settings-spelling-custom"),
            query,
            theme::semibold(13.0),
            p.text,
            p.search_highlight,
        );
        ui.add(egui::Label::new(title).wrap());
        ui.add_space(6.0);
        let selected = page.selected_dictionary.filter(|index| *index < spelling.custom_dictionaries.len());
        match toolbar(ui, p, "custom", selected.is_some(), true) {
            Some(Tool::Add) => page.path_field = Some(String::new()),
            Some(Tool::Remove) => {
                if let Some(index) = selected {
                    spelling.custom_dictionaries.remove(index);
                    page.selected_dictionary = None;
                }
            }
            Some(Tool::Edit) => page.open = selected.map(|index| PathBuf::from(&spelling.custom_dictionaries[index])),
            None => {}
        }
        let mut path_done = None;
        list_frame(ui, p, "settings-spelling-custom", |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Some(value) = page.path_field.as_mut() {
                ui.horizontal(|ui| {
                    let browse = t!("settings-browse");
                    let room = ui.available_width()
                        - ui.painter().layout_no_wrap(browse.clone(), theme::regular(13.0), p.text).size().x
                        - 24.0;
                    ui.allocate_ui(vec2(room.max(80.0), ROW), |ui| {
                        if let Some(done) = field_row(ui, value, &t!("settings-spelling-new-path"), "path") {
                            path_done = Some((done, value.clone()));
                        }
                    });
                    if !cfg!(target_arch = "wasm32")
                        && ui.button(browse).clicked()
                        && let Some(picked) = pick_dictionary()
                    {
                        path_done = Some((true, picked.display().to_string()));
                    }
                });
            }
            let built_in = t!("settings-spelling-built-in");
            for name in ["settings-spelling-application-dictionary", "settings-spelling-project-dictionary"] {
                row(ui, p, &t!(name), Some((&built_in, false)), false);
            }
            for (index, path) in spelling.custom_dictionaries.iter().enumerate() {
                let read = page.read.iter().find(|read| read.path.as_path() == std::path::Path::new(path));
                let detail = read.map(|read| match &read.error {
                    Some(error) => (error.clone(), true),
                    None if read.format == "hunspell" => (t!("settings-spelling-hunspell", words = read.words), false),
                    None => (t!("settings-spelling-plain", words = read.words), false),
                });
                let detail = detail.as_ref().map(|(text, error)| (text.as_str(), *error));
                if row(ui, p, path, detail, selected == Some(index)).clicked() {
                    page.selected_dictionary = Some(index);
                }
            }
        });
        if let Some((done, value)) = path_done {
            page.path_field = None;
            let value = value.trim().to_owned();
            if done && !value.is_empty() && !spelling.custom_dictionaries.contains(&value) {
                spelling.custom_dictionaries.push(value);
                page.selected_dictionary = Some(spelling.custom_dictionaries.len() - 1);
            }
        }
        ui.add_space(12.0);
        widgets::group_title(ui, p, &t!("settings-spelling-accepted"), query);
        match toolbar(ui, p, "words", page.selected_word.is_some(), false) {
            Some(Tool::Add) => page.word_field = Some(String::new()),
            Some(Tool::Remove) => {
                if let Some(index) = page.selected_word.take().filter(|index| *index < page.words.len()) {
                    page.words.remove(index);
                }
            }
            Some(Tool::Edit) | None => {}
        }
        let mut word_done = None;
        list_frame(ui, p, "settings-spelling-words", |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if let Some(value) = page.word_field.as_mut()
                && let Some(done) = field_row(ui, value, &t!("settings-spelling-new-word"), "word")
            {
                word_done = Some((done, value.clone()));
            }
            for index in 0..page.words.len() {
                let selected = page.selected_word == Some(index);
                if row(ui, p, &page.words[index], None, selected).clicked() {
                    page.selected_word = Some(index);
                }
            }
        });
        if let Some((done, value)) = word_done {
            page.word_field = None;
            if done {
                page.add_word(&value);
            }
        }
        if let Some(message) = &page.message {
            ui.add(egui::Label::new(egui::RichText::new(message).color(p.diagnostic_error)).wrap());
        }
        ui.add_space(8.0);
        if ui.link(egui::RichText::new(t!("settings-spelling-configure")).color(p.link)).clicked() {
            self.section = super::PageId::Inspections;
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn pick_dictionary() -> Option<PathBuf> {
    rfd::FileDialog::new().add_filter("dic", &["dic", "txt"]).pick_file()
}

#[cfg(target_arch = "wasm32")]
fn pick_dictionary() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_accepted_words_of_the_draft_become_commands() {
        let mut page = SpellingPage {
            loaded: true,
            original: vec!["gopls".to_owned(), "zenide".to_owned()],
            words: vec!["gopls".to_owned(), "zenide".to_owned()],
            ..SpellingPage::default()
        };
        assert!(page.requests(DictionaryLevel::Project).is_empty(), "nothing changed");
        page.words.retain(|word| word != "gopls");
        page.add_word(" Kubectl ");
        page.add_word("hadnler");
        page.words.retain(|word| word != "kubectl");
        assert_eq!(page.words, ["hadnler", "zenide"], "the words stay sorted");
        page.add_word("two words");
        assert!(page.message.is_some());
        let sent = page.requests(DictionaryLevel::Project);
        assert_eq!(
            sent,
            [
                ("spelling.word.remove", json!({ "word": "gopls" })),
                ("spelling.word.save", json!({ "word": "hadnler", "level": "project" })),
            ]
        );
        assert!(SpellingPage::default().requests(DictionaryLevel::Application).is_empty(), "not loaded, nothing sent");
    }
}
